//! Sealed Linux backend initialization; native phone readiness belongs to the host.
mod postgres;
pub(crate) mod process;
#[cfg(any(test, target_os = "linux"))]
use crate::profile;
use crate::{Error, Result, filesystem};
#[cfg(target_os = "macos")]
pub(crate) use postgres::MAC_POSTGRES_SHUTDOWN;
use process::Execute;
#[cfg(any(test, target_os = "linux"))]
use process::Process;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    path::Path,
    time::{Duration, Instant},
};
use tokio::process::Command;
#[cfg(any(test, target_os = "linux"))]
use tokio::sync::watch;

#[cfg(any(test, target_os = "linux"))]
const PATH: &str = "/opt/aiphone/linux/bin:/usr/local/bin:/usr/bin:/bin";
const REDIS: &str = "/opt/aiphone/redis";
const REDIS_CLI: &str = "/opt/aiphone/redis/bin/redis-cli";

fn phase(name: &str) {
    eprintln!("AI_PHONE_INITIALIZE {name}");
}
fn text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(Error::Invalid("wsl_initializer_path"))
}
#[cfg(any(test, target_os = "linux"))]
fn environment(inherited: impl IntoIterator<Item = (String, String)>) -> BTreeMap<String, String> {
    let mut env: BTreeMap<_, _> = inherited
        .into_iter()
        .filter(|(key, _)| !matches!(key.as_str(), "LIMA_HOME" | "AI_PHONE_LIMA_COMMAND"))
        .collect();
    env.insert("PATH".into(), PATH.into());
    env
}
pub(crate) struct Backend<'a, E> {
    pub(crate) root: &'a Path,
    pub(crate) program: &'a OsStr,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) execute: E,
}

fn mac_guest(lima: &str, owner: &str, args: &[&str]) -> Vec<String> {
    [
        lima,
        "shell",
        "--tty=false",
        "--workdir",
        "/",
        "ai-phone",
        "sudo",
        "-n",
        "-u",
        owner,
        "--",
    ]
    .into_iter()
    .chain(args.iter().copied())
    .map(str::to_owned)
    .collect()
}

impl<E: Execute> Backend<'_, E> {
    pub(crate) fn command(
        &self,
        program: impl AsRef<OsStr>,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    ) -> Command {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(self.root)
            .env_clear()
            .envs(&self.env);
        command
    }
    pub(crate) async fn checked(&mut self, command: Command, input: Vec<u8>) -> Result<Vec<u8>> {
        let output = self.execute.run(command, input).await?;
        if !output.status.success() {
            return Err(Error::Invalid("wsl_initializer_command"));
        }
        Ok(output.stdout)
    }
    pub(crate) async fn native(&mut self, args: &[OsString], input: Vec<u8>) -> Result<Vec<u8>> {
        self.checked(self.command(self.program, args), input).await
    }
    pub(crate) async fn unit(
        &mut self,
        name: &str,
        operation: &str,
        command: Vec<String>,
        extra: &[(&str, String)],
    ) -> Result<Value> {
        let mut env = self.env.clone();
        env.extend(
            extra
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone())),
        );
        let input = if operation == "start" {
            serde_json::to_vec(&json!({"command":command,"environment":env}))
                .map_err(|_| Error::Invalid("service_spec"))?
        } else {
            Vec::new()
        };
        let bytes = self
            .native(
                &[
                    "--service-control".into(),
                    self.root.as_os_str().into(),
                    name.into(),
                    operation.into(),
                ],
                input,
            )
            .await?;
        serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("service_receipt"))
    }
    pub(crate) async fn start(&mut self, user: &Path) -> Result<()> {
        phase("environment");
        self.env
            .extend(crate::runtime_environment::profile_environment(
                self.root, user,
            )?);
        for name in ["assistant", "phone-runner", "channels"] {
            token(self.root, name)?;
        }
        let lima = self.env.get("AI_PHONE_LIMA_COMMAND").cloned();
        let lima = lima.as_deref();
        let redis_port = self.env["AI_PHONE_REDIS_PORT"].clone();
        if lima.is_none() {
            self.start_redis(&redis_port, None).await?;
        }
        phase("database_start");
        self.start_postgres(lima).await?;
        if lima.is_some() {
            self.start_redis(&redis_port, lima).await?;
        }
        self.wait_redis(&redis_port, lima).await?;
        let assistant = text(&self.root.join("runtime/services/bin/assistant"))?;
        phase("database_prepare");
        if self
            .unit("assistant", "status", Vec::new(), &[])
            .await?
            .is_null()
        {
            fs::write(self.root.join(crate::DEVICE_SETUP_PENDING), [])?;
            for flag in ["--prepare-database", "--migrate"] {
                self.checked(self.command(&assistant, [flag]), Vec::new())
                    .await?;
            }
        }
        if let Some(command) = crate::desktop_bridge::desktop_arguments(
            self.root,
            lima,
            &self.env["AI_PHONE_CONTAINER_PROXY"],
        )? {
            self.unit("desktop", "start", command, &[]).await?;
        }
        phase("services_start");
        let ocr = text(&self.root.join("runtime/services/bin/vision"))?;
        self.unit(
            "runner",
            "start",
            vec![text(&self.root.join("runtime/services/bin/runner"))?],
            &[("AI_PHONE_OCR", ocr)],
        )
        .await?;
        self.unit("assistant", "start", vec![assistant.clone()], &[])
            .await?;
        self.unit(
            "channels",
            "start",
            vec![assistant, "--channels".into()],
            &[],
        )
        .await?;
        self.unit(
            "media",
            "start",
            vec![
                text(&self.root.join("runtime/services/bin/runner"))?,
                "--media".into(),
            ],
            &[],
        )
        .await?;
        Ok(())
    }
    async fn start_redis(&mut self, port: &str, lima: Option<&str>) -> Result<()> {
        phase("redis_start");
        let (mut command, directory) = if let Some(lima) = lima {
            (
                mac_guest(lima, "redis", &["/usr/bin/redis-server"]),
                "/var/lib/redis",
            )
        } else {
            let args = [
                "/usr/bin/setpriv",
                "--reuid=65534",
                "--regid=65534",
                "--clear-groups",
                "--",
                "/opt/aiphone/redis/bin/redis-server",
            ];
            (args.map(str::to_owned).into(), REDIS)
        };
        command.extend(
            [
                "--dir",
                directory,
                "--bind",
                "127.0.0.1",
                "--port",
                port,
                "--save",
                "",
                "--appendonly",
                "no",
                "--maxmemory",
                "128mb",
                "--maxmemory-policy",
                "noeviction",
            ]
            .map(str::to_owned),
        );
        self.unit("redis", "start", command, &[]).await?;
        Ok(())
    }
    async fn wait_redis(&mut self, port: &str, lima: Option<&str>) -> Result<()> {
        let args = if let Some(lima) = lima {
            mac_guest(lima, "redis", &["/usr/bin/redis-cli", "-p", port, "PING"])
        } else {
            [REDIS_CLI, "-p", port, "PING"].map(str::to_owned).into()
        };
        let ready = Instant::now() + Duration::from_secs(10);
        loop {
            let command = self.command(&args[0], &args[1..]);
            let result = self.execute.run(command, Vec::new()).await?;
            if result.status.success() && result.stdout == b"PONG\n" {
                return Ok(());
            }
            if Instant::now() >= ready {
                return Err(Error::Invalid("wsl_redis_not_ready"));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
pub(crate) fn token(root: &Path, section: &str) -> Result<()> {
    let folder = root.join("data").join(section);
    filesystem::directory(&folder, true)?;
    if folder.join("token").try_exists()? {
        return Ok(());
    }
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Invalid("host_profile_random"))?;
    let value = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    filesystem::publish_private(&folder, "token", value.as_bytes(), true)
}
#[cfg(target_os = "linux")]
pub(crate) async fn initialize_request(request: &[u8]) -> Result<()> {
    let request = profile::parse_request(request)
        .map_err(|_| Error::Invalid("wsl_initialization_request"))?;
    postgres::check_guest()?;
    if request.desktop.storage_gb.is_some() {
        crate::linux_vm::grow_root()?;
    }
    let (send, stop) = watch::channel(false);
    let signal = tokio::spawn(async move {
        let result = crate::component::interrupt().await;
        if result.is_ok() {
            send.send_replace(true);
        }
        result
    });
    let result = initialize(
        Path::new("/opt/aiphone/bundle"),
        Path::new("/var/lib/aiphone/installation"),
        request,
        Process(stop),
        &|path| Ok(fs2::available_space(path)?),
    )
    .await;
    signal.abort();
    let _ = signal.await;
    result
}
#[cfg(any(test, target_os = "linux"))]
async fn initialize<E: Execute>(
    bundle: &Path,
    home: &Path,
    request: profile::Request,
    execute: E,
    available: &dyn Fn(&Path) -> Result<u64>,
) -> Result<()> {
    phase("preflight");
    let parent = home
        .parent()
        .ok_or(Error::Invalid("wsl_initializer_path"))?;
    filesystem::directory(parent, true)?;
    let _lock = filesystem::lease(&parent.join("initialize.lock"))?.ok_or(Error::Busy)?;
    if available(parent)? < 4 * 1024 * 1024 * 1024 {
        return Err(Error::LowSpace);
    }
    let id = fs::read_to_string(bundle.join("source.sha256"))?;
    let id = id.trim();
    if !crate::manifest::identity(id) {
        return Err(Error::Invalid("source_resource_identity"));
    }
    let program = std::env::current_exe()?;
    let mut backend = Backend {
        root: parent,
        program: program.as_os_str(),
        env: environment(std::env::vars()),
        execute,
    };
    phase("source");
    let receipt = backend
        .native(
            &[
                "--stage-source".into(),
                bundle.join("source.payload").into_os_string(),
                home.as_os_str().into(),
                id.into(),
            ],
            Vec::new(),
        )
        .await?;
    let root = home.join("releases").join(id).canonicalize()?;
    if receipt != format!("{id} {}\n", root.display()).as_bytes() {
        return Err(Error::Invalid("source_resource_identity"));
    }
    backend.root = &root;
    if root.join("data/restore-quarantine.json").try_exists()? {
        return Err(Error::Invalid("restored_state_quarantined"));
    }
    if fs::symlink_metadata(root.join("data/deployment/container-host.json")).is_ok() {
        return Err(Error::Invalid("foreign_installation"));
    }
    crate::desktop_bridge::require_rootfs(&root, None)?;
    let stage = crate::desktop_bridge::native_arguments(&root, "stage", None)?;
    backend
        .checked(backend.command(&stage[0], &stage[1..]), Vec::new())
        .await?;
    phase("profile");
    #[cfg(target_arch = "aarch64")]
    backend
        .env
        .insert("AI_PHONE_SERIAL".into(), "127.0.0.1:5555".into());
    if request.android_qemu {
        filesystem::directory(Path::new(crate::device_bridge::guest::RUNTIME), true)?;
        backend.env.insert(
            "AI_PHONE_SERIAL".into(),
            crate::android::Engine::Cuttlefish.serial(5564),
        );
        backend
            .env
            .insert("AI_PHONE_ANDROID_QEMU".into(), "1".into());
    }
    profile::prepare_request(
        &root,
        Path::new("/opt/aiphone/native-phone-bridge"),
        if cfg!(target_arch = "aarch64") {
            "android35-arm64"
        } else {
            "android35-x86_64"
        },
        request,
        PATH,
    )?;
    let load_tun = backend.command("/sbin/modprobe", ["tun"]);
    backend.checked(load_tun, Vec::new()).await?;
    backend.start(Path::new("/root")).await
}

#[cfg(test)]
mod tests;
