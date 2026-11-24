use crate::deployment::android_assets;
use ai_phone_api::wire::{NativeAndroidInputSetup, VirtualDeviceProfile};
use ai_phone_host_runtime::{android::input, guest_channel, vm_control};
use ring::rand::{SecureRandom, SystemRandom};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
    sync::{oneshot, watch},
};
mod profile;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub(crate) fn attached_client(home: &Path) -> Result<Option<ai_phone_adb::Client>> {
    let Some((port, key)) = guest_channel::mobile_endpoint(home)? else {
        return Ok(None);
    };
    Ok(Some(ai_phone_adb::Client::new(
        port,
        hex::encode(fs::read(key)?),
    )?))
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

pub(crate) async fn run(
    home: PathBuf,
    bundle: PathBuf,
    kind: &str,
    id: &str,
    resources: VirtualDeviceProfile,
    locale: &str,
    mut stop: watch::Receiver<bool>,
    ready: oneshot::Sender<()>,
) -> Result<()> {
    if !Path::new("/apex/com.android.virt/bin/vm").is_file() {
        return Err("This Android host does not provide the AVF virtual machine service".into());
    }
    let group = if kind == "phone" { "android" } else { "linux" };
    let user = home.join("user");
    let state = user.join("avf");
    fs::create_dir_all(&state)?;
    let lease = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state.join("session.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lease)?;
    let endpoint = state.join("port");
    if endpoint.try_exists()? {
        fs::remove_file(&endpoint)?;
    }
    let keyfile = state.join("control.key");
    if !keyfile.try_exists()? {
        let mut secret = [0; 32];
        SystemRandom::new()
            .fill(&mut secret)
            .map_err(|_| "Instance key generation failed")?;
        crate::deployment::write_atomic(&keyfile, &secret)?;
    }
    let key: [u8; 32] = fs::read(&keyfile)?
        .try_into()
        .map_err(|_| "Invalid instance key")?;
    let cancelled = || *stop.borrow();
    android_assets::stage(&bundle, group, &cancelled)?;
    let config = profile::prepare(&bundle, &state, kind, &resources, locale, &key, &cancelled)?;
    let apps = bundle.join("android/apps");
    if kind == "phone" {
        let runtime = user.join("runtime");
        fs::create_dir_all(&runtime)?;
        let link = runtime.join("phone-apps");
        if fs::read_link(&link).ok().as_ref() != Some(&apps) {
            let swap = tempfile::tempdir_in(&runtime)?;
            let staged = swap.path().join("apps");
            std::os::unix::fs::symlink(&apps, &staged)?;
            fs::rename(staged, &link)?;
        }
    }
    if cancelled() {
        return Ok(());
    }
    let command = format!(
        "exec {} {} {} {}",
        quote(
            android_assets::helper()?
                .to_str()
                .ok_or("Invalid helper path")?
        ),
        kind,
        quote(config.to_str().ok_or("Invalid instance path")?),
        quote(&format!("aimanity-{id}"))
    );
    let mut child = Command::new("su")
        .args(["-c", &command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| io::Error::other(format!("Android root authorization (su): {e}")))?;
    let owner = child
        .stdin
        .take()
        .ok_or("AVF supervisor input unavailable")?;
    let mut lines = BufReader::new(
        child
            .stdout
            .take()
            .ok_or("AVF supervisor output unavailable")?,
    )
    .lines();
    let mut stderr = child
        .stderr
        .take()
        .ok_or("AVF supervisor stderr unavailable")?;
    let errors = tokio::spawn(async move {
        let mut tail = Vec::new();
        let mut buf = [0; 1024];
        while let Ok(n @ 1..) = stderr.read(&mut buf).await {
            tail.extend_from_slice(&buf[..n]);
            if tail.len() > 4096 {
                tail.drain(..tail.len() - 4096);
            }
        }
        String::from_utf8_lossy(&tail).trim().to_owned()
    });
    let up = async {
        let line = lines
            .next_line()
            .await?
            .ok_or("AVF supervisor exited before assigning a device endpoint")?;
        let mut fields = line.split_whitespace();
        if fields.next() != Some("READY")
            || fields.next().and_then(|s| s.parse::<u32>().ok()).is_none()
        {
            return Err("Unexpected AVF supervisor reply".into());
        }
        let port = fields
            .next()
            .ok_or("Missing instance endpoint")?
            .parse::<u16>()?;
        if kind == "phone" {
            let serial = hex::encode(key);
            let adb = ai_phone_adb::Client::new(port, serial.clone())?;
            loop {
                if let Ok(status) = adb
                    .shell(
                        "getprop sys.boot_completed",
                        ai_phone_adb::Limits {
                            stdout: 64,
                            stderr: 256,
                        },
                        Duration::from_secs(4),
                    )
                    .await
                    && status.code == 0
                    && status.stdout.trim_ascii() == b"1"
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(350)).await;
            }
            input::setup_at(
                port,
                NativeAndroidInputSetup {
                    version: 1,
                    project_root: user.to_str().ok_or("Invalid guest path")?.into(),
                    serial,
                    apk_path: apps
                        .join("input.apk")
                        .to_str()
                        .ok_or("Invalid APK path")?
                        .into(),
                    offline_apps: true,
                    ..Default::default()
                },
                stop.clone(),
            )
            .await?;
        } else {
            while vm_control::mobile_ready(port, &keyfile).await.is_err() {
                tokio::time::sleep(Duration::from_millis(350)).await;
            }
        }
        crate::deployment::write_atomic(&endpoint, port.to_string().as_bytes())?;
        let _ = ready.send(());
        Result::Ok(())
    };
    let mut cancelled = stop.clone();
    let outcome = tokio::select! {biased;
        _=cancelled.wait_for(|v|*v)=>Ok(()),
        status=child.wait()=>Err(format!("AVF supervisor exited: {}",status?).into()),
        started=tokio::time::timeout(Duration::from_secs(600),up)=>match started{
            Err(_)=>Err("Guest startup timed out".into()),Ok(Err(e))=>Err(e),
            Ok(Ok(()))=>tokio::select!{biased; _=stop.wait_for(|v|*v)=>Ok(()), status=child.wait()=>Err(format!("Guest stopped: {}",status?).into())}
        }
    };
    drop(owner);
    if tokio::time::timeout(Duration::from_secs(8), child.wait())
        .await
        .is_err()
    {
        child.kill().await?;
    }
    let details = errors.await?;
    if endpoint.try_exists()? {
        fs::remove_file(&endpoint)?;
    }
    if !details.is_empty()
        && let Err(error) = outcome
    {
        return Err(format!("{error}: {details}").into());
    }
    outcome
}
