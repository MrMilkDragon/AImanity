//! Complete bundled Unix first-run. The controller calls native owners directly.
mod engine;
mod phone;
#[cfg(target_os = "macos")]
pub use phone::shutdown as stop_mac_phone;
mod readiness;
use crate::wsl::backend::{
    Backend,
    process::{Execute, Process},
};
use crate::{Error, Installation, Result, filesystem, profile};
use ai_phone_api::wire::{NativeAndroidInputSetup, NativeAndroidStart};
use serde_json::json;
use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Output,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{process::Command, sync::watch};

pub(crate) const CONTRACT: &str = "ai-phone-unix-host-initializer-v1";

#[cfg(target_os = "macos")]
pub(crate) async fn workspace_desktop(
    spec: &crate::virtual_device::Instance,
    mut stop: watch::Receiver<bool>,
    ready: tokio::sync::oneshot::Sender<()>,
) -> Result<()> {
    let mut install = Installation::lock(&spec.home)?;
    let id = spec.identity("source.payload")?;
    let cancel = || *stop.borrow();
    let staged = match install.resume_source(id, &cancel)? {
        Some(s) => s,
        None => install.stage_source(
            &spec.bundle.join("source.payload"),
            id,
            &|p| fs2::available_space(p).unwrap_or(0),
            &cancel,
        )?,
    };
    let root = staged.directory;
    crate::desktop_bridge::require_rootfs(&root, Some(spec.identity("images.payload")?))?;
    let folder = root.join("data/deployment");
    filesystem::directory(&folder, true)?;
    filesystem::publish_private(
        &folder,
        "profile.json",
        &serde_json::to_vec(&spec.desktop_profile()).map_err(|_| Error::Invalid("host_profile"))?,
        false,
    )?;
    let user = PathBuf::from(std::env::var_os("HOME").ok_or(Error::Invalid("unix_host_home"))?);
    let exe = std::env::current_exe()?;
    let mut backend = Backend {
        root: &root,
        program: exe.as_os_str(),
        env: std::env::vars()
            .filter(|(k, _)| matches!(k.as_str(), "HOME" | "USER" | "TMPDIR" | "LANG" | "PATH"))
            .collect(),
        execute: Process(stop.clone()),
    };
    let result = async {
        let managed = engine::ensure(&mut backend, &spec.home, &user, &spec.bundle, true).await?;
        backend.env.extend(managed);
        let lima = backend
            .env
            .get("AI_PHONE_LIMA_COMMAND")
            .ok_or(Error::Invalid("managed_engine_path"))?
            .clone();
        let stage = crate::desktop_bridge::native_arguments(&root, "stage", Some(&lima))?;
        backend
            .checked(backend.command(&stage[0], &stage[1..]), Vec::new())
            .await?;
        let args = crate::desktop_bridge::desktop_arguments(&root, Some(&lima), "")?
            .ok_or(Error::Invalid("desktop_resources"))?;
        let mut desktop = backend
            .command(&args[0], &args[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        drop(install);
        let _ = ready.send(());
        tokio::select! {
            _ = stop.wait_for(|end| *end) => {
                let _ = desktop.kill().await;
                Ok(())
            }
            status = desktop.wait() => {
                if status?.success() { Ok(()) } else { Err(Error::Invalid("guest_desktop_exited")) }
            }
        }
    }
    .await;
    crate::managed_engine::shutdown(&spec.home, &user)
        .await
        .and(result)
}

#[cfg(target_os = "macos")]
pub async fn shutdown(home: &Path) -> Result<()> {
    let user = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(Error::Invalid("unix_host_home"))?;
    shutdown_owned(home, &user).await
}

#[cfg(target_os = "macos")]
async fn shutdown_owned(home: &Path, user: &Path) -> Result<()> {
    if !home.try_exists()? {
        return Ok(());
    }
    let home = home.canonicalize()?;
    let services = crate::supervision::macos::deactivate(&home).await;
    let (phone, engine) = tokio::join!(
        phone::shutdown(&home),
        crate::managed_engine::shutdown(&home, user),
    );
    services.and(phone).and(engine)
}

#[cfg(target_os = "macos")]
pub async fn uninstall(home: &Path, user: &Path) -> Result<()> {
    if !home.try_exists()? {
        return Ok(());
    }
    filesystem::existing_directory(home)?;
    let home = home.canonicalize()?;
    let _startup = filesystem::lease(&home.join("initialize.lock"))?.ok_or(Error::Busy)?;
    let _installation = Installation::lock(&home)?;
    shutdown_owned(&home, user).await?;
    crate::managed_engine::remove_state(&home, user)
}
fn text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(Error::Invalid("unix_host_path"))
}
fn progress(phase: &str, state: &str, error: &str) -> Result<()> {
    writeln!(
        std::io::stdout(),
        "AI_PHONE_SETUP {}",
        json!({"phase":phase,"state":state,"error":error})
    )?;
    std::io::stdout().flush()?;
    Ok(())
}

fn activity(command: &str, state: &str) {
    let _ = writeln!(
        std::io::stdout(),
        "AI_PHONE_ACTIVITY {}",
        json!({"command":command,"state":state}),
    );
}

fn command_label(command: &Command) -> Option<&'static str> {
    let process = command.as_std();
    let name = Path::new(process.get_program()).file_name()?.to_str()?;
    let mut args = process
        .get_args()
        .map(|arg| arg.to_str().unwrap_or_default());
    match (name, args.next()?) {
        ("limactl", "list") => Some("limactl list"),
        ("limactl", "start") => Some("limactl start --tty=false"),
        (_, "--service-control") => match (args.nth(1)?, args.next()?) {
            ("runner", "start") => Some("ai-phone-client --service-control … runner start"),
            _ => None,
        },
        _ => None,
    }
}

struct InstallProcess(Process);
impl Execute for InstallProcess {
    async fn run(&mut self, command: Command, input: Vec<u8>) -> Result<Output> {
        let label = command_label(&command);
        if let Some(label) = label {
            activity(label, "running");
        }
        let result = self.0.run(command, input).await;
        if let Some(label) = label {
            activity(
                label,
                if result.as_ref().is_ok_and(|output| output.status.success()) {
                    "completed"
                } else {
                    "failed"
                },
            );
        }
        result
    }
}
struct Arguments {
    resource: PathBuf,
    home: PathBuf,
    request: profile::Request,
    source: String,
    android: String,
    images: Option<String>,
    resume: bool,
}
impl Arguments {
    fn parse(values: &[OsString]) -> Result<Self> {
        let [resource, home, request, source, android, images @ ..] = values else {
            return Err(Error::Invalid("unix_host_arguments"));
        };
        let digest = |value: &OsString| {
            value
                .to_str()
                .filter(|value| crate::manifest::identity(value))
                .map(str::to_owned)
                .ok_or(Error::Invalid("unix_host_arguments"))
        };
        let (images, resume) = match images {
            [] => (None, false),
            [value] => (Some(digest(value)?), false),
            [value, resume] if resume == "--resume" => (Some(digest(value)?), true),
            _ => return Err(Error::Invalid("unix_host_arguments")),
        };
        let source = digest(source)?;
        let android = digest(android)?;
        let resource = PathBuf::from(resource);
        // All three sealed inputs must exist before installation or persistent user state is created.
        for name in ["source.payload", "android.payload", "images.payload"] {
            let value = fs::symlink_metadata(resource.join(name))?;
            if !value.is_file() {
                return Err(Error::Invalid("unix_host_resource"));
            }
        }
        let request = profile::parse_request(&fs::read(request)?)?;
        Ok(Self {
            resource,
            home: home.into(),
            request,
            source,
            android,
            images,
            resume,
        })
    }
}

fn require_host_components(components: &[String]) -> Result<()> {
    if components
        .iter()
        .map(String::as_str)
        .ne(crate::manifest::COMPONENTS.map(|(role, _)| role))
    {
        return Err(Error::Invalid("unix_host_incomplete_bundle"));
    }
    Ok(())
}

async fn initialize(
    args: Arguments,
    cancel: Arc<AtomicBool>,
    end: watch::Sender<bool>,
    stop: watch::Receiver<bool>,
) -> Result<()> {
    let user = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(Error::Invalid("unix_host_home"))?
        .canonicalize()?;
    let template = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "android35-arm64",
        _ => return Err(Error::Invalid("unix_host_platform")),
    };
    progress("source", "running", "")?;
    filesystem::directory(&args.home, true)?;
    let _startup = filesystem::lease(&args.home.join("initialize.lock"))?.ok_or(Error::Busy)?;
    let home = args.home.canonicalize()?;
    let payload = args.resource.join("source.payload");
    let source = args.source;
    let resume = args.resume;
    let task_home = home.clone();
    let cancelled = cancel.clone();
    let (installation, staged, resumed) = tokio::task::spawn_blocking(move || {
        let mut installation = Installation::lock(&task_home)?;
        let cancelled = || cancelled.load(Ordering::Relaxed);
        let staged = if resume {
            installation.resume_source(&source, &cancelled)?
        } else {
            None
        };
        let resumed = staged.is_some();
        let staged = match staged {
            Some(staged) => staged,
            None => installation.recover_source(
                &payload,
                &source,
                &|path| fs2::available_space(path).unwrap_or(0),
                &cancelled,
            )?,
        };
        Ok::<_, Error>((installation, staged, resumed))
    })
    .await
    .map_err(|_| Error::Invalid("unix_host_worker"))??;
    require_host_components(&staged.components)?;
    let root = staged.directory;
    if root.join("data/restore-quarantine.json").try_exists()? {
        return Err(Error::Invalid("restored_state_quarantined"));
    }
    crate::desktop_bridge::require_rootfs(&root, args.images.as_deref())?;
    let resources = profile::phone_resources(&root, template, &args.request)?;
    let reconfigure = args.request.reconfigure;
    progress("android", "running", "")?;
    let android = args.resource.join("android.payload");
    let identity = args.android;
    let cancelled = cancel.clone();
    let (installation, prepared) = tokio::task::spawn_blocking(move || {
        let mut installation = installation;
        let cached = if resumed {
            installation.resume_android(&identity)?
        } else {
            None
        };
        let prepared = match cached {
            Some(prepared) => {
                if reconfigure {
                    prepared.configure(&resources)?;
                }
                prepared
            }
            None => installation.prepare_android(
                &android,
                &identity,
                resources,
                &|path| fs2::available_space(path).unwrap_or(0),
                &|| cancelled.load(Ordering::Relaxed),
            )?,
        };
        Ok::<_, Error>((installation, prepared))
    })
    .await
    .map_err(|_| Error::Invalid("unix_host_worker"))??;
    // Keep the installation lease for the complete source/profile/engine/device transaction.
    let _installation = installation;
    #[cfg(target_os = "macos")]
    crate::supervision::macos::activate(&root).await?;
    progress("profile", "running", "")?;
    profile::prepare_request(
        &root,
        &prepared.sdk_root,
        template,
        args.request,
        "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
    )?;
    let program = std::env::current_exe()?;
    let mut environment = crate::runtime_environment::profile_environment(&root, &user)?;
    for key in ["HOME", "USER", "TMPDIR", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            environment.insert(key.into(), value);
        }
    }
    environment.insert(
        "PATH".into(),
        "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".into(),
    );
    let engine = crate::android::Engine::installed(&prepared.sdk_root);
    #[cfg(target_os = "macos")]
    if engine == crate::android::Engine::Avo {
        environment.insert(
            "AI_PHONE_AVO_CONTROL_SOCKET".into(),
            text(&crate::android::avo_runtime(5564).join("control.sock"))?,
        );
    } else {
        environment.insert(
            "AI_PHONE_ANDROID_GRPC_HOME".into(),
            text(&root.join("data/android/native/profile/Library/Caches/TemporaryItems"))?,
        );
    }
    environment.insert("AI_PHONE_SERIAL".into(), engine.serial(5564));
    let mut backend = Backend {
        root: &root,
        program: program.as_os_str(),
        env: environment,
        execute: InstallProcess(Process(stop.clone())),
    };
    {
        let linux = async {
            progress("engine", "running", "")?;
            engine::start(&mut backend, &home, &user, &args.resource, reconfigure).await?;
            progress("services", "running", "")?;
            backend.start(&user).await
        };
        let android = async {
            progress("android", "running", "")?;
            let boot = phone::start(
                NativeAndroidStart {
                    phone: Some(prepared.wire()?),
                    ..Default::default()
                },
                stop.clone(),
            )
            .await?;
            progress("input", "running", "")?;
            crate::android::input::setup(
                NativeAndroidInputSetup {
                    version: 1,
                    project_root: text(&root)?,
                    adb_program: text(&prepared.sdk_root.join("platform-tools/adb"))?,
                    serial: boot.serial,
                    apk_path: text(&root.join("runtime/phone-apps/input.apk"))?,
                    offline_apps: true,
                    ..Default::default()
                },
                stop.clone(),
            )
            .await?;
            Ok(())
        };
        tokio::pin!(linux, android);
        let (first, second) = tokio::select! {
            result = &mut linux => {
                if result.is_err() {
                    end.send_replace(true);
                }
                (result, android.await)
            }
            result = &mut android => {
                if result.is_err() {
                    end.send_replace(true);
                }
                (result, linux.await)
            }
        };
        first.and(second)?;
    }
    readiness::wait(&root, stop).await?;
    progress("ready", "ready", "")
}

pub(crate) async fn run(values: &[OsString]) -> Result<()> {
    let args = Arguments::parse(values)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let signal_cancel = cancel.clone();
    let (end, stop) = watch::channel(false);
    let signal_end = end.clone();
    let signal = tokio::spawn(async move {
        let result = crate::component::interrupt().await;
        if result.is_ok() {
            signal_cancel.store(true, Ordering::Relaxed);
            signal_end.send_replace(true);
        }
        result
    });
    let result = initialize(args, cancel, end, stop).await;
    signal.abort();
    let _ = signal.await;
    if let Err(error) = &result {
        progress("install", "failed", error.code())?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::require_host_components;

    #[test]
    fn install_requires_all_three_device_components() {
        let native = ["native-services", "phone-apps", "container-host"].map(str::to_owned);
        require_host_components(&native).unwrap();

        for omitted in &native {
            let incomplete: Vec<_> = native
                .iter()
                .filter(|component| *component != omitted)
                .cloned()
                .collect();
            assert_eq!(
                require_host_components(&incomplete).unwrap_err().code(),
                "unix_host_incomplete_bundle"
            );
        }
    }
}
