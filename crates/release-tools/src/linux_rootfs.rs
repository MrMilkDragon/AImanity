use crate::{Result, image_factory::execute};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Write},
    os::{
        fd::AsFd,
        unix::fs::{PermissionsExt, symlink},
    },
    path::Path,
    process::{Command, Stdio},
};

const WORKSPACE_PACKAGES: &str = include_str!("../../../infra/workspace/packages.txt");
const DESKTOP_PACKAGES: &str = include_str!("../../../infra/desktop/packages.txt");
const OPENBOX_SETUP: &str = include_str!("../../../infra/desktop/install-openbox.sh");
const ARCHIVE_KEYRING: &str = "/usr/share/keyrings/debian-archive-keyring.gpg";
const DEBIAN: &str = "https://deb.debian.org/debian";

fn private_mounts() -> Command {
    let mut command = Command::new("unshare");
    command.args(["--mount", "--propagation", "private"]);
    command
}

pub(super) fn chroot(target: &Path, program: &str) -> Command {
    let mut command = private_mounts();
    command
        .args([
            "--",
            "/bin/sh",
            "-ec",
            "mount -t proc proc \"$1/proc\"; exec chroot \"$@\"",
            "linux-factory",
        ])
        .arg(target)
        .arg(program)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .env("TMPDIR", "/tmp");
    command
}

pub(super) fn bootstrap(target: &Path, arch: &str) -> Result<()> {
    fs::set_permissions(target, fs::Permissions::from_mode(0o755))?;
    eprintln!("Bootstrapping Debian trixie ({arch})");
    let mut keyring = OsString::from("--keyring=");
    keyring.push(env::var_os("AI_PHONE_DEBIAN_KEYRING").unwrap_or_else(|| ARCHIVE_KEYRING.into()));
    let bootstrap = private_mounts()
        .args([
            "--",
            "debootstrap",
            "--variant=minbase",
            "--force-check-gpg",
            "--include=ca-certificates",
            &format!("--arch={arch}"),
        ])
        .arg(keyring)
        .arg("trixie")
        .arg(target)
        .arg(DEBIAN)
        .stdout(io::stderr().as_fd().try_clone_to_owned()?)
        .status()?;
    if !bootstrap.success() {
        return Err(format!("debootstrap: {bootstrap}").into());
    }
    fs::write(
        target.join("etc/apt/sources.list"),
        format!(
            "deb {DEBIAN} trixie main\ndeb {DEBIAN} trixie-updates main\ndeb https://deb.debian.org/debian-security trixie-security main\n"
        ),
    )?;
    let policy = target.join("usr/sbin/policy-rc.d");
    fs::write(&policy, "#!/bin/sh\nexit 101\n")?;
    fs::set_permissions(&policy, fs::Permissions::from_mode(0o755))?;
    execute(chroot(target, "/usr/bin/apt-get").args(["-o", "Acquire::Retries=4", "update"]))?;
    Ok(())
}

pub(super) fn install_packages<'a>(
    target: &Path,
    packages: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    execute(
        chroot(target, "/usr/bin/apt-get")
            .args([
                "-o",
                "Acquire::Retries=4",
                "-o",
                "Dpkg::Use-Pty=0",
                "install",
                "-y",
                "--no-install-recommends",
            ])
            .args(packages),
    )
}

fn install(root: &Path, target: &Path, arch: &str) -> Result<()> {
    bootstrap(target, arch)?;
    eprintln!("Installing Linux desktop and workspace packages");
    install_packages(
        target,
        WORKSPACE_PACKAGES
            .split_ascii_whitespace()
            .chain(DESKTOP_PACKAGES.split_ascii_whitespace()),
    )?;
    execute(chroot(target, "/bin/sh").args(["-ec", OPENBOX_SETUP]))?;
    execute(chroot(target, "/usr/bin/apt-get").arg("clean"))?;
    fs::remove_file(target.join("usr/sbin/policy-rc.d"))?;
    fs::remove_dir_all(target.join("var/lib/apt/lists"))?;
    fs::write(target.join("etc/hostname"), "aimanity\n")?;
    fs::write(
        target.join("etc/hosts"),
        "127.0.0.1 localhost\n::1 localhost\n",
    )?;
    fs::write(target.join("etc/resolv.conf"), "")?;
    execute(
        Command::new("find")
            .arg(target.join("usr"))
            .args(["-type", "f", "-name", "*.py", "-delete"]),
    )?;
    execute(chroot(target, "/usr/sbin/useradd").args([
        "-m",
        "-u",
        "1000",
        "-s",
        "/bin/bash",
        "ai",
    ]))?;
    fs::write(
        target.join("etc/sudoers.d/ai-phone"),
        "ai ALL=(ALL:ALL) NOPASSWD: ALL\n",
    )?;
    fs::set_permissions(
        target.join("etc/sudoers.d/ai-phone"),
        fs::Permissions::from_mode(0o440),
    )?;

    execute(
        Command::new("cp")
            .arg("-R")
            .arg(root.join("infra/desktop/appearance/."))
            .arg(target),
    )?;
    execute(chroot(target, "/bin/sh").args([
        "-ec",
        include_str!("../../../infra/desktop/install-appearance.sh"),
    ]))?;
    for (source, destination) in [
        ("LICENSE", "usr/share/doc/ai-phone/LICENSE"),
        ("NOTICE", "usr/share/doc/ai-phone/NOTICE"),
        (
            "docs/public/native-services-licenses.txt",
            "usr/share/doc/ai-phone/native-services-licenses.txt",
        ),
        (
            "docs/public/native-services-license-inventory.json",
            "usr/share/doc/ai-phone/native-services-license-inventory.json",
        ),
        ("infra/desktop/start.sh", "usr/local/bin/desktop-start"),
        (
            "infra/desktop/home-initialize.sh",
            "usr/local/lib/ai-phone/home-initialize.sh",
        ),
        ("infra/desktop/dunstrc", "etc/xdg/dunst/dunstrc"),
        ("infra/desktop/menu.xml", "etc/xdg/openbox/menu.xml"),
        ("infra/desktop/pulse.pa", "etc/ai-phone-pulse.pa"),
        ("infra/desktop/mimeapps.list", "etc/xdg/mimeapps.list"),
    ] {
        let file = target.join(destination);
        fs::create_dir_all(file.parent().expect("rootfs asset parent"))?;
        fs::copy(root.join(source), file)?;
    }
    fs::set_permissions(
        target.join("usr/local/bin/desktop-start"),
        fs::Permissions::from_mode(0o755),
    )?;
    // The guest mounts the architecture-matched sealed bridge over this file.
    File::create(target.join("usr/local/bin/desktop-bridge"))?;
    symlink(
        "desktop-bridge",
        target.join("usr/local/bin/desktop-notification-record"),
    )?;
    execute(chroot(target, "/bin/sh").args(["-ec", "! command -v python3 && ! command -v python && test -x /usr/bin/localc && test -x /usr/bin/lowriter && test -x /usr/bin/loimpress && test -x /usr/bin/lodraw && test \"$(xdg-mime query default text/plain)\" = org.xfce.mousepad.desktop && test \"$(xdg-mime query default text/markdown)\" = org.xfce.mousepad.desktop"]))?;
    Ok(())
}

pub(super) fn build(root: &Path, arch: &str, destination: &Path) -> Result<Value> {
    if !matches!(
        (env::consts::OS, env::consts::ARCH, arch),
        ("linux", "aarch64", "arm64") | ("linux", "x86_64", "amd64")
    ) {
        return Err("Build the Linux root filesystem on a matching Linux architecture".into());
    }
    let destination = std::path::absolute(destination)?;
    let parent = destination
        .parent()
        .ok_or("Missing Linux output directory")?;
    fs::create_dir_all(parent)?;
    let target = tempfile::tempdir()?;
    install(root, target.path(), arch)?;

    eprintln!("Compressing shared Linux filesystem");
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    let mut export = Command::new("tar")
        .args(["--numeric-owner", "--sort=name", "-cf", "-", "-C"])
        .arg(target.path())
        .arg(".")
        .stdout(Stdio::piped())
        .spawn()?;
    let mut archive = Command::new("zstd")
        .args(["-q", "-15", "-T2", "--stdout"])
        .stdin(export.stdout.take().expect("root filesystem tar pipe"))
        .stdout(Stdio::piped())
        .spawn()?;
    let mut compressed = archive
        .stdout
        .take()
        .expect("compressed root filesystem pipe");
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let count = compressed.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        pending.write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
    }
    let exported = export.wait()?;
    if !archive.wait()?.success() || !exported.success() {
        return Err("Linux root filesystem export failed".into());
    }
    let size = pending.as_file().metadata()?.len();
    pending
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o644))?;
    pending.persist(destination)?;
    Ok(json!({"size_bytes":size,"sha256":format!("{:x}", digest.finalize())}))
}
