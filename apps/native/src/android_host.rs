use super::{Event, Host};
use crate::deployment::android_assets;
use ai_phone_host_runtime::vm_control;
use futures_util::{SinkExt, StreamExt};
use ring::rand::{SecureRandom, SystemRandom};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
    sync::{oneshot, watch},
};

#[path = "../../android-host/src/profile.rs"]
mod profile;

type Output = iced::futures::channel::mpsc::Sender<Event>;
static SHUTDOWN: LazyLock<watch::Sender<u64>> = LazyLock::new(|| watch::channel(0).0);

struct InstallationCancel(Arc<AtomicBool>);
impl Drop for InstallationCancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

fn state(home: &Path) -> PathBuf {
    home.join("user/avf")
}

pub(super) fn key(home: &Path) -> PathBuf {
    state(home).join("control.key")
}

fn prepare(host: &Host, cancelled: &AtomicBool) -> io::Result<[PathBuf; 2]> {
    if !cfg!(target_arch = "aarch64") {
        return Err(io::Error::other(
            "Offline guests require an ARM64 Android device",
        ));
    }
    if !Path::new("/apex/com.android.virt/bin/vm").is_file() {
        return Err(io::Error::other(
            "Android Virtualization Framework CLI unavailable on this device",
        ));
    }
    let has_su = std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| directory.join("su").is_file())
    });
    if !has_su {
        return Err(io::Error::other(
            "The Android AVF workbench requires an installed root manager with su",
        ));
    }
    android_assets::stage(&host.resource, cancelled)?;
    let state = state(&host.home);
    fs::create_dir_all(&state)?;
    let key = key(&host.home);
    if !key.try_exists()? {
        let mut bytes = [0; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| io::Error::other("AVF control key generation failed"))?;
        let mut pending = tempfile::NamedTempFile::new_in(&state)?;
        pending.write_all(&bytes)?;
        pending.as_file().sync_all()?;
        pending.persist_noclobber(&key)?;
    }
    profile::prepare(&host.resource, &state, &host.request, cancelled)
}

fn quote(path: &Path) -> io::Result<String> {
    let raw = path
        .to_str()
        .ok_or_else(|| io::Error::other("Invalid Android installation path"))?;
    Ok(format!("'{}'", raw.replace('\'', "'\\''")))
}

fn start(configs: &[PathBuf; 2]) -> io::Result<Child> {
    let helper = android_assets::helper()?;
    let command = format!(
        "exec {} {} {}",
        quote(&helper)?,
        quote(&configs[0])?,
        quote(&configs[1])?
    );
    // Superuser managers mediate approval for the app UID. The paths come from app-private installation state.
    Command::new("su")
        .args(["-c", &command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| io::Error::other(format!("Android root authorization (su) unavailable: {e}")))
}

fn cids(line: &str) -> Option<(u32, u32)> {
    let (phone, linux) = line
        .strip_prefix("Android AVF CID=")?
        .split_once(" Linux AVF CID=")?;
    let phone: u32 = phone.parse().ok()?;
    let linux: u32 = linux.parse().ok()?;
    (phone != linux && [phone, linux].iter().all(|c| (3..u32::MAX).contains(c)))
        .then_some((phone, linux))
}

async fn observe_devices(home: &str) -> Result<(), String> {
    let session = crate::connection::Connection::connect(home.to_owned()).await?;
    let frame = |kind: &'static str| {
        let device = session
            .registry
            .devices
            .iter()
            .find(|device| device.kind == kind && device.runtime_state == "managed")
            .cloned();
        let client = session.client.clone();
        async move {
            let device = device.ok_or_else(|| format!("Missing managed {kind}"))?;
            let frames = client.frames(device);
            futures_util::pin_mut!(frames);
            let frame = frames
                .next()
                .await
                .and_then(|pending| pending.take())
                .ok_or_else(|| format!("Missing {kind} screen frame"))??;
            if frame.width == 0 || frame.height == 0 {
                return Err(format!("Empty {kind} screen frame"));
            }
            Ok(())
        }
    };
    tokio::try_join!(frame("phone"), frame("desktop"))?;
    Ok(())
}

async fn serve(host: &Host, output: &mut Output, child: &mut Child) -> Result<(), String> {
    let stderr = child.stderr.take().ok_or("Missing AVF error pipe")?;
    let errors = tokio::spawn(async move {
        let mut tail = Vec::new();
        let mut chunk = [0; 1024];
        let mut stderr = stderr;
        while let Ok(n) = stderr.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            tail.extend_from_slice(&chunk[..n]);
            if tail.len() > 2048 {
                tail.drain(..tail.len() - 2048);
            }
        }
        String::from_utf8_lossy(&tail).trim().to_owned()
    });
    let mut lines = BufReader::new(child.stdout.take().ok_or("Missing AVF startup pipe")?).lines();
    let receipt = tokio::time::timeout(Duration::from_secs(125), lines.next_line())
        .await
        .map_err(|_| "Timed out waiting for Android root authorization and AVF startup")?
        .map_err(|e| e.to_string())?;
    if receipt.as_deref().and_then(cids).is_none() {
        let details = tokio::time::timeout(Duration::from_secs(2), errors)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        return Err(format!("Android root/AVF startup failed. {details}"));
    }
    output
        .send(Event::Phase("services".into()))
        .await
        .map_err(|e| e.to_string())?;
    let key = key(&host.home);
    let services = vm_control::expose_native_services(&key)
        .await
        .map_err(|e| e.to_string())?;
    let home = host.home.to_string_lossy().into_owned();
    let ready = async {
        loop {
            // A saved registry is insufficient on resume. Require a decoded frame from each live guest.
            if tokio::time::timeout(Duration::from_secs(25), observe_devices(&home))
                .await
                .is_ok_and(|r| r.is_ok())
            {
                return;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };
    tokio::select! {
        exit = child.wait() => return Err(format!("AVF session ended during guest startup ({})", exit.map_err(|e| e.to_string())?)),
        result = tokio::time::timeout(Duration::from_secs(900),ready) =>
            result.map_err(|_| "Timed out waiting for authenticated Android and Linux workbench services")?,
    }
    output
        .send(Event::Ready(home))
        .await
        .map_err(|e| e.to_string())?;
    let status = child.wait().await.map_err(|e| e.to_string())?;
    drop(services);
    Err(format!("AVF guest session ended ({status})"))
}

async fn stop_child(child: &mut Child, home: &Path) -> Result<(), String> {
    if child.try_wait().map_err(|e| e.to_string())?.is_some() {
        return Ok(());
    }
    // The guest acknowledges before invoking poweroff. Keep both Binder handles alive until
    // Linux actually exits; EOF releases the session on a failed or timed-out shutdown.
    let acknowledged = tokio::time::timeout(
        Duration::from_secs(4),
        vm_control::shutdown(vm_control::GUEST_PORT, &key(home)),
    )
    .await
    .is_ok_and(|r| r.is_ok());
    if acknowledged
        && tokio::time::timeout(Duration::from_secs(18), child.wait())
            .await
            .is_ok_and(|r| r.is_ok())
    {
        return Ok(());
    }
    drop(child.stdin.take());
    match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
        Ok(result) => result.map(|_| ()).map_err(|e| e.to_string()),
        Err(_) => child.kill().await.map_err(|e| e.to_string()),
    }
}

fn lease(home: &Path) -> io::Result<fs::File> {
    fs::create_dir_all(home)?;
    let lease = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.join("avf-session.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lease)?;
    Ok(lease)
}

impl Host {
    pub fn events(
        self,
        mut stop: oneshot::Receiver<()>,
    ) -> impl futures_util::Stream<Item = Event> {
        iced::stream::channel(8, async move |mut output: Output| {
            let mut external = SHUTDOWN.subscribe();
            let stopping = async {
                tokio::select! { _ = &mut stop => {}, _ = external.changed() => {} }
            };
            tokio::pin!(stopping);
            let mut child = None;
            let mut lock = None;
            let result = async {
                let installation = lease(&self.home).map_err(|e| format!("AVF installation is busy or unavailable: {e}"))?;
                output.send(Event::Phase("source".into())).await.map_err(|e| e.to_string())?;
                let cancel = InstallationCancel(Arc::new(AtomicBool::new(false)));
                let preparing = self.clone();
                let worker_cancel = Arc::clone(&cancel.0);
                // Activity teardown can drop this future while a disk is still being written.
                // The blocking writer retains the lease until it observes cancellation.
                let mut worker = tokio::task::spawn_blocking(move || {
                    prepare(&preparing, &worker_cancel).map(|configs| (installation,configs))
                });
                let (installation,configs) = tokio::select! { biased;
                    _ = &mut stopping => {
                        cancel.0.store(true, Ordering::Relaxed);
                        let _ = worker.await;
                        return Ok(());
                    }
                    result = &mut worker => result.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?,
                };
                lock = Some(installation);
                output.send(Event::Phase("android".into())).await.map_err(|e| e.to_string())?;
                child = Some(start(&configs).map_err(|e| e.to_string())?);
                tokio::select! { biased;
                    _ = &mut stopping => Ok(()),
                    result = serve(&self, &mut output, child.as_mut().unwrap()) => result,
                }
            }.await;
            let shutdown = if let Some(mut child) = child {
                stop_child(&mut child, &self.home).await
            } else {
                Ok(())
            };
            drop(lock);
            if let Err(error) = result {
                let _ = output.send(Event::Failed(error)).await;
                stopping.await;
            }
            let _ = output.send(Event::Stopped(shutdown)).await;
        })
    }
}

pub(super) async fn stop(home: &Path) -> Result<(), String> {
    SHUTDOWN.send_modify(|generation| *generation = generation.wrapping_add(1));
    tokio::time::timeout(Duration::from_secs(35), async {
        loop {
            match lease(home) {
                Ok(_) => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(Duration::from_millis(80)).await
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    })
    .await
    .map_err(|_| "Timed out stopping the AVF guest session".to_owned())?
}
