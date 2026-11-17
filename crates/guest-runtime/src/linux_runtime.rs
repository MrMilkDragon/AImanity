use crate::Result;
use ai_phone_api::LINUX_ROOTFS;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs::{self, File},
    os::{
        fd::AsFd,
        unix::{
            fs::{MetadataExt, PermissionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::unix::pipe::Receiver,
    process::{Child, Command},
};

const STATE: &str = "/var/lib/aiphone-linux";
const DESKTOP: &str = "aiphone-native-desktop";
const SANDBOX_SUBNET: &str = "10.253.0";
const DESKTOP_CAPS: &str = "--bounding-set=-all,+chown,+dac_override,+fowner,+setuid,+setgid,+kill";
const DESKTOP_ADMIN_CAPS: &str =
    "--bounding-set=-all,+chown,+dac_override,+fowner,+setuid,+setgid,+kill,+net_admin,+mknod";

fn rootfs(source: &Path) -> Result<PathBuf> {
    let manifest: Value =
        serde_json::from_reader(File::open(source.join("runtime/services/manifest.json"))?)?;
    let digest = manifest["linux_rootfs"]["sha256"]
        .as_str()
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or("Linux root filesystem absent from the native services manifest")?;
    Ok(Path::new(STATE).join(digest))
}

async fn stage(source: &Path) -> Result<()> {
    let path = rootfs(source)?;
    fs::create_dir_all(STATE)?;
    let lock = File::create(Path::new(STATE).join("stage.lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    fs::write(
        Path::new(STATE).join("resolv.conf"),
        format!("nameserver {SANDBOX_SUBNET}.3\n"),
    )?;
    if path.exists() {
        return Ok(());
    }
    let temporary = tempfile::tempdir_in(STATE)?;
    let mut reader = Command::new("zstd")
        .arg("-dc")
        .arg(LINUX_ROOTFS)
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let input: Stdio = reader.stdout.take().expect("archive pipe").try_into()?;
    let mut tar = Command::new("tar")
        .args(["-xf", "-", "-C"])
        .arg(temporary.path())
        .stdin(input)
        .kill_on_drop(true)
        .spawn()?;
    let (decoded, unpacked) = tokio::try_join!(reader.wait(), tar.wait())?;
    if !decoded.success() || !unpacked.success() {
        return Err("Linux root filesystem extraction failed".into());
    }
    for directory in ["workspace", "skills", "home/ai", "run"] {
        fs::create_dir_all(temporary.path().join(directory))?;
    }
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o755))?;
    fs::rename(temporary.path(), path)?;
    Ok(())
}

fn join_group(name: &str, cpus: u32, memory_mb: u32, pids: u32) -> Result<()> {
    let group = Path::new("/sys/fs/cgroup").join(name);
    fs::create_dir_all(&group)?;
    fs::write(
        "/sys/fs/cgroup/cgroup.subtree_control",
        b"+cpu +memory +pids",
    )?;
    for (name, budget) in [
        ("cpu.max", format!("{} 100000", cpus * 100000)),
        (
            "memory.max",
            (u64::from(memory_mb) * 1024 * 1024).to_string(),
        ),
        ("pids.max", pids.to_string()),
    ] {
        fs::write(group.join(name), budget)?;
    }
    fs::write(group.join("cgroup.procs"), std::process::id().to_string())?;
    Ok(())
}

fn sandbox(proxy: &OsString, overlay: Option<(&str, &Path)>) -> Command {
    let mut command = if let Some((options, mount)) = overlay {
        let mut command = Command::new("unshare");
        command.args(["-m", "--", "/bin/sh", "-ec",
            "mount --make-rprivate /; mount -t overlay overlay -o \"$1\" \"$2\"; shift 2; exec bwrap \"$@\" 3>&2",
            "linux-desktop", options]).arg(mount);
        command
    } else {
        let mut command = Command::new("/bin/sh");
        command.args(["-ec", "exec bwrap \"$@\" 3>&2", "linux-workspace"]);
        command
    };
    command.args([
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
        "--unshare-net",
        "--new-session",
        "--die-with-parent",
        "--info-fd",
        "3",
        "--cap-drop",
        "ALL",
        "--cap-add",
        "CAP_SETUID",
        "--cap-add",
        "CAP_SETGID",
        "--cap-add",
        "CAP_SETPCAP",
    ]);
    command.args([
        "--clearenv",
        "--setenv",
        "PATH",
        "/usr/local/bin:/usr/local/sbin:/usr/sbin:/usr/bin:/sbin:/bin",
        "--setenv",
        "LANG",
        "C.UTF-8",
        "--setenv",
        "NO_PROXY",
        "localhost,127.0.0.1",
    ]);
    for key in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
        command.args(["--setenv", key]).arg(proxy);
    }
    command
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

fn sandbox_mounts(command: &mut Command) {
    command
        .args(["--ro-bind"])
        .arg(Path::new(STATE).join("resolv.conf"));
    command.args([
        "/etc/resolv.conf",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--perms",
        "01777",
        "--tmpfs",
        "/dev/shm",
        "--perms",
        "01777",
        "--tmpfs",
        "/tmp",
        "--tmpfs",
        "/run",
    ]);
}

fn workspace(source: &Path, script: &OsString, proxy: &OsString) -> Result<Child> {
    let work = source.join("data/workspaces/admin").canonicalize()?;
    let skills = source.join("data/runtime-skills").canonicalize()?;
    let (uid, gid) = if let Ok(uid) = std::env::var("SUDO_UID") {
        (
            uid.parse::<u32>()?,
            std::env::var("SUDO_GID")?.parse::<u32>()?,
        )
    } else {
        let owner = fs::metadata(&work)?;
        (owner.uid(), owner.gid())
    };
    let mut command = sandbox(proxy, None);
    command
        .args(["--ro-bind"])
        .arg(rootfs(source)?)
        .args(["/", "--bind"])
        .arg(work)
        .args(["/workspace", "--ro-bind"])
        .arg(skills)
        .args(["/skills"]);
    sandbox_mounts(&mut command);
    command
        .args([
            "--chdir",
            "/workspace",
            "--setenv",
            "HOME",
            "/workspace/.home",
            "--",
            "/usr/bin/setpriv",
            "--clear-groups",
            "--no-new-privs",
            "--bounding-set=-all",
        ])
        .arg(format!("--reuid={uid}"))
        .arg(format!("--regid={gid}"))
        .args([
            "/bin/sh",
            "-ec",
            "read -r _; exec /bin/sh -lc \"$1\" </dev/null",
            "workspace",
        ])
        .arg(script);
    Ok(command.spawn()?)
}

#[derive(Deserialize, Serialize)]
struct Namespace {
    #[serde(rename = "child-pid")]
    pid: u32,
    #[serde(rename = "mnt-namespace")]
    mount: u64,
}

async fn start_network(
    job: &mut Child,
) -> Result<(
    Namespace,
    Child,
    tokio::task::JoinHandle<std::io::Result<u64>>,
)> {
    let mut diagnostics = BufReader::new(job.stderr.take().expect("sandbox diagnostics"));
    let mut info = Vec::new();
    let (namespace, network) = tokio::time::timeout(Duration::from_secs(10), async {
        diagnostics.read_until(b'}', &mut info).await?;
        let namespace: Namespace = serde_json::from_slice(&info)
            .map_err(|_| String::from_utf8_lossy(&info).into_owned())?;
        diagnostics.read_u8().await?;
        let mut network = Command::new("/bin/sh")
            .args([
                "-ec",
                "exec slirp4netns --configure --disable-host-loopback --enable-sandbox --exit-fd=0 --ready-fd=3 --cidr=\"$2\" \"$1\" tap0 3>&1 >/dev/null",
                "linux-network",
            ])
            .arg(namespace.pid.to_string())
            .arg(format!("{SANDBOX_SUBNET}.0/24"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        network
            .stdout
            .as_mut()
            .expect("network ready pipe")
            .read_u8()
            .await
            .map_err(|error| format!("Linux network initialization: {error}"))?;
        Ok::<_, crate::Error>((namespace, network))
    }).await??;
    job.stdin
        .take()
        .expect("sandbox handshake")
        .write_all(b"ready\n")
        .await?;
    let logs =
        tokio::spawn(
            async move { tokio::io::copy(&mut diagnostics, &mut tokio::io::stderr()).await },
        );
    Ok((namespace, network, logs))
}

async fn run_workspace(source: &Path, script: &OsString, proxy: &OsString) -> Result<u8> {
    let lease = Receiver::from_owned_fd(std::io::stdin().as_fd().try_clone_to_owned()?)?;
    let cancelled = lease.readable();
    tokio::pin!(cancelled);
    join_group("aiphone-workspace", 2, 2048, 256)?;
    let mut job = workspace(source, script, proxy)?;
    let (_, mut network, logs) = tokio::select! {
        result = start_network(&mut job) => result?,
        result = &mut cancelled => { result?; job.kill().await?; return Ok(130); }
    };
    let status = tokio::select! {
        status = job.wait() => status?.code().unwrap_or(1) as u8,
        result = &mut cancelled => { result?; job.kill().await?; 130 },
        _ = tokio::time::sleep(Duration::from_secs(90)) => { job.kill().await?; 124 },
    };
    network.kill().await?;
    logs.await??;
    Ok(status)
}

fn desktop_receipt() -> PathBuf {
    Path::new(STATE).join("desktop.json")
}

async fn run_desktop(source: &Path, cpus: u32, memory_mb: u32, proxy: &OsString) -> Result<u8> {
    let home = source.join("data/desktop/home");
    fs::create_dir_all(&home)?;
    stage(source).await?;
    let owned = Path::new(STATE).join("desktop");
    let upper = owned.join("upper");
    let work = owned.join("work");
    let mount = owned.join("root");
    fs::create_dir_all(&upper)?;
    fs::create_dir_all(&work)?;
    fs::create_dir_all(&mount)?;
    let lock = File::create(owned.join("session.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)?;
    join_group(DESKTOP, cpus, memory_mb, 512)?;
    let options = format!(
        "lowerdir={},upperdir={},workdir={}",
        rootfs(source)?.display(),
        upper.display(),
        work.display()
    );
    let mut desktop = sandbox(proxy, Some((&options, &mount)));
    for capability in ["CAP_CHOWN", "CAP_DAC_OVERRIDE", "CAP_FOWNER", "CAP_KILL"] {
        desktop.args(["--cap-add", capability]);
    }
    desktop
        .arg("--bind")
        .arg(mount)
        .args(["/", "--bind"])
        .arg(home)
        .args(["/home/ai", "--ro-bind"])
        .arg(source.join("runtime/services/bin/desktop-bridge"))
        .arg("/usr/local/bin/desktop-bridge");
    sandbox_mounts(&mut desktop);
    desktop.args([
        "--setenv",
        "HOME",
        "/home/ai",
        "--setenv",
        "DISPLAY",
        ":0",
        "--setenv",
        "SAL_USE_VCLPLUGIN",
        "gtk3",
        "--",
        "/usr/bin/setpriv",
        DESKTOP_CAPS,
        "/bin/sh",
        "-ec",
        "read -r _; exec /usr/local/bin/desktop-start </dev/null",
    ]);
    let mut child = desktop.spawn()?;
    let (namespace, mut network, logs) = start_network(&mut child).await?;
    let mut receipt = tempfile::NamedTempFile::new_in(STATE)?;
    serde_json::to_writer(&mut receipt, &namespace)?;
    receipt.persist(desktop_receipt())?;
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let status = tokio::select! {
        status = child.wait() => status?.code().unwrap_or(1) as u8,
        _ = terminate.recv() => { child.kill().await?; 0 },
        _ = interrupt.recv() => { child.kill().await?; 0 },
    };
    network.kill().await?;
    fs::remove_file(desktop_receipt())?;
    logs.await??;
    Ok(status)
}

fn attach(args: &[OsString], admin: bool) -> Result<u8> {
    let namespace: Namespace = serde_json::from_reader(File::open(desktop_receipt())?)?;
    let mount = fs::read_link(format!("/proc/{}/ns/mnt", namespace.pid))?;
    if mount != PathBuf::from(format!("mnt:[{}]", namespace.mount)) {
        return Err("Linux desktop namespace changed".into());
    }
    let mut command = std::process::Command::new("nsenter");
    command.args([
        "-t",
        &namespace.pid.to_string(),
        "-m",
        "-p",
        "-i",
        "-n",
        "-r",
        "--",
        "/usr/bin/setpriv",
        if admin {
            DESKTOP_ADMIN_CAPS
        } else {
            DESKTOP_CAPS
        },
    ]);
    if !admin {
        command.args([
            "/usr/sbin/runuser",
            "-u",
            "ai",
            "--",
            "/usr/bin/env",
            "HOME=/home/ai",
            "DISPLAY=:0",
            "LANG=C.UTF-8",
            "PULSE_SERVER=unix:/tmp/ai-runtime/pulse/native",
            "/usr/local/bin/desktop-bridge",
        ]);
    }
    Err(command.args(args).exec().into())
}

pub async fn cli(args: &[OsString]) -> Result<u8> {
    match args {
        [source, operation] if operation == "stage" => {
            stage(Path::new(source)).await?;
            Ok(0)
        }
        [source, operation, script, proxy] if operation == "workspace" => {
            run_workspace(Path::new(source), script, proxy).await
        }
        [source, operation, cpus, memory, proxy] if operation == "desktop" => {
            run_desktop(
                Path::new(source),
                cpus.to_str().ok_or("desktop CPU count")?.parse()?,
                memory.to_str().ok_or("desktop memory")?.parse()?,
                proxy,
            )
            .await
        }
        [_, operation, commands @ ..]
            if !commands.is_empty() && (operation == "exec" || operation == "admin") =>
        {
            attach(commands, operation == "admin")
        }
        _ => Err("invalid native Linux runtime arguments".into()),
    }
}
