use futures_util::SinkExt;
use serde::Deserialize;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Stdio;
use tokio::sync::oneshot;
#[cfg(target_os = "macos")]
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

#[derive(Clone, Deserialize)]
pub struct Host {
    pub home: PathBuf,
    pub resource: PathBuf,
    pub request: PathBuf,
    #[cfg(target_os = "macos")]
    pub source: String,
    pub android: String,
    pub images: String,
    #[serde(default = "default_resume")]
    pub resume: bool,
}

fn default_resume() -> bool {
    true
}

#[derive(Clone, Debug)]
pub enum Event {
    Phase(String),
    #[cfg(target_os = "macos")]
    Activity(Activity),
    Ready(String),
    Failed(String),
    Stopped(Result<(), String>),
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, Deserialize)]
pub struct Activity {
    pub command: String,
    pub state: ActivityState,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActivityState {
    Running,
    Completed,
    Failed,
}

impl Host {
    pub fn load(path: &str) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    #[cfg(target_os = "macos")]
    pub fn events(
        self,
        mut stop: oneshot::Receiver<()>,
    ) -> impl futures_util::Stream<Item = Event> {
        iced::stream::channel(8, async move |mut output| {
            let mut closing = false;
            let result: Result<bool, String> = async {
                let mut command = Command::new(std::env::current_exe().map_err(|e|e.to_string())?);
                command
                    .arg("--initialize-unix-host")
                    .arg(&self.resource).arg(&self.home).arg(&self.request)
                    .arg(&self.source).arg(&self.android).arg(&self.images);
                if self.resume {
                    command.arg("--resume");
                }
                let mut child = command.stdout(Stdio::piped()).spawn().map_err(|e|e.to_string())?;
                let mut lines=BufReader::new(child.stdout.take().unwrap()).lines();
                let mut failure = None;
                loop {
                    tokio::select! {
                        _ = &mut stop => {
                            closing = true;
                            let status=Command::new("/bin/kill").arg("-TERM").arg(child.id().unwrap().to_string()).status().await.map_err(|e|e.to_string())?;
                            if !status.success() && child.try_wait().map_err(|e|e.to_string())?.is_none() { return Err("无法停止设备启动进程".into()); }
                            child.wait().await.map_err(|e|e.to_string())?;
                            return Ok(false);
                        }
                        line=lines.next_line() => {
                            let Some(line)=line.map_err(|e|e.to_string())? else {break};
                            let event = if let Some(json) = line.strip_prefix("AI_PHONE_SETUP ") {
                                #[derive(Deserialize)]
                                struct Progress {phase:String, error:String}
                                let progress:Progress=serde_json::from_str(json).map_err(|e|e.to_string())?;
                                if !progress.error.is_empty() {
                                    failure = Some(progress.error);
                                    continue;
                                }
                                Event::Phase(progress.phase)
                            } else if let Some(json) = line.strip_prefix("AI_PHONE_ACTIVITY ") {
                                Event::Activity(serde_json::from_str(json).map_err(|e|e.to_string())?)
                            } else {
                                continue;
                            };
                            if output.send(event).await.is_err() {
                                return Ok(false);
                            }
                        }
                    }
                }
                let status=child.wait().await.map_err(|e|e.to_string())?;
                if !status.success() {
                    return Err(failure.unwrap_or_else(|| status.to_string()));
                }
                let root=self.home.join("releases").join(&self.source);
                output.send(Event::Ready(root.to_string_lossy().into_owned())).await.map_err(|e|e.to_string())?;
                Ok(true)
            }.await;
            match result {
                Ok(true) => {
                    let _ = stop.await;
                }
                Ok(false) => {}
                Err(error) => {
                    let _ = output.send(Event::Failed(error)).await;
                    if !closing {
                        let _ = stop.await;
                    }
                }
            }
            let result = Self::stop(self.home).await;
            let _ = output.send(Event::Stopped(result)).await;
        })
    }

    #[cfg(windows)]
    pub fn events(
        self,
        mut stop: oneshot::Receiver<()>,
    ) -> impl futures_util::Stream<Item = Event> {
        use ai_phone_host_runtime::{qemu_host, vm_control, windows_native::Deployment};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        iced::stream::channel(
            8,
            async move |mut output: iced::futures::channel::mpsc::Sender<Event>| {
                let home = self.home.clone();
                let cancel = Arc::new(AtomicBool::new(false));
                let cancelled = Arc::clone(&cancel);
                let progress = output.clone();
                let mut startup = tokio::task::spawn_blocking(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    let request = Deployment {
                        resource: &self.resource,
                        home: &self.home,
                        request: &self.request,
                        android: &self.android,
                        vm_manifest: &self.images,
                        resume: self.resume,
                    };
                    runtime.block_on(request.start(
                        &|| cancelled.load(Ordering::Relaxed),
                        |phase| {
                            let _ = progress.clone().try_send(Event::Phase(phase.into()));
                        },
                    ))
                });
                let started = tokio::select! { biased;
                    _ = &mut stop => {
                        cancel.store(true, Ordering::Relaxed);
                        let _ = startup.await;
                        None
                    }
                    result = &mut startup => Some(result.map_err(|e| e.to_string()).and_then(|r| r))
                };
                if let Some(started) = started {
                    let key = qemu_host::key_path(&home.join("user/qemu"));
                    match async {
                        started?;
                        let routes = vm_control::expose_native_services(&key)
                            .await
                            .map_err(|e| e.to_string())?;
                        output
                            .send(Event::Ready(home.to_string_lossy().into_owned()))
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok::<_, String>(routes)
                    }
                    .await
                    {
                        Ok(routes) => {
                            let _ = stop.await;
                            drop(routes);
                        }
                        Err(error) => {
                            let _ = output.send(Event::Failed(error)).await;
                            let result = Self::stop(home).await;
                            let _ = stop.await;
                            let _ = output.send(Event::Stopped(result)).await;
                            return;
                        }
                    }
                }
                let _ = output.send(Event::Stopped(Self::stop(home).await)).await;
            },
        )
    }

    pub async fn stop(home: PathBuf) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        let result = ai_phone_host_runtime::stop_macos_installation(&home).await;
        #[cfg(windows)]
        let result = ai_phone_host_runtime::qemu_host::stop_installed(&home).await;
        result.map_err(|e| e.to_string())
    }
}

pub async fn credentials(root: &Path) -> Result<(String, String), String> {
    #[cfg(target_os = "macos")]
    {
        tokio::try_join!(
            tokio::fs::read_to_string(root.join("data/assistant/token")),
            tokio::fs::read_to_string(root.join("data/phone-runner/token")),
        )
        .map_err(|e| e.to_string())
    }
    #[cfg(windows)]
    {
        let key = ai_phone_host_runtime::qemu_host::key_path(&root.join("user/qemu"));
        ai_phone_host_runtime::vm_control::native_credentials(&key)
            .await
            .map(|value| (value.assistant_token, value.phone_token))
            .map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
mod lifecycle {
    include!("windows-lifecycle.rs");
}

#[cfg(windows)]
pub fn windows_guard() -> std::io::Result<std::os::windows::io::OwnedHandle> {
    lifecycle::open_guard()
}

#[cfg(windows)]
pub fn windows_data_home() -> Result<PathBuf, String> {
    lifecycle::roaming_host()
}

#[cfg(windows)]
pub fn windows_command(arguments: &[std::ffi::OsString]) -> Option<i32> {
    let [argument] = arguments else { return None };
    let result = match argument.to_str()? {
        "--uninstall-windows-user-data" => lifecycle::remove().map(|_| 0),
        "--stop-windows-backend" => crate::deployment::data_home()
            .and_then(|home| lifecycle::stop_host(&home))
            .map(|_| 0),
        "--prepare-windows-host" => prepare_windows_host(),
        _ => return None,
    };
    Some(match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    })
}

#[cfg(windows)]
fn prepare_windows_host() -> Result<i32, String> {
    use std::{
        os::windows::process::CommandExt,
        process::{Command, Stdio},
    };

    let resource = crate::deployment::Bundle::resource_dir()?;
    crate::deployment::bundle_inventory(&resource)?;
    let system = PathBuf::from(std::env::var_os("SystemRoot").ok_or("windows_system_path")?)
        .join("System32");
    let code = Command::new(system.join("dism.exe"))
        .args([
            "/Online",
            "/Enable-Feature",
            "/FeatureName:HypervisorPlatform",
            "/All",
            "/NoRestart",
            "/Quiet",
        ])
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("Windows Hypervisor Platform: {error}"))?
        .code();
    match code {
        Some(value @ (0 | 3010)) => Ok(value),
        _ => Err(format!("Windows Hypervisor Platform: exit {code:?}")),
    }
}
