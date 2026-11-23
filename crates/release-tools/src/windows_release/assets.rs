use crate::{Result, image_factory::execute, linux_ocr, linux_rootfs};
use serde_json::{Value, json};
use std::{ffi::OsString, fs, path::Path, process::Command};

const ROOT: &str = "/opt/aiphone";
const LIB: &str = "/opt/aiphone/linux/lib";
const LOADER: &str = "/opt/aiphone/linux/lib/ld-linux-x86-64.so.2";
const PACKAGES: [&str; 8] = [
    "bubblewrap",
    "slirp4netns",
    "zstd",
    "patchelf",
    "tzdata",
    "tesseract-ocr",
    "tesseract-ocr-chi-sim",
    "tesseract-ocr-eng",
];

fn libraries(program: &Path) -> Result<()> {
    for source in linux_ocr::libraries(program.to_str().ok_or("Invalid ELF path")?)? {
        let source = Path::new(&source);
        let destination = Path::new(LIB).join(source.file_name().ok_or("Missing ELF name")?);
        if !destination.exists() {
            fs::copy(source, destination)?;
        }
    }
    Ok(())
}

fn executable(source: &Path, destination: &Path) -> Result<()> {
    libraries(source)?;
    fs::copy(source, destination)?;
    execute(
        Command::new("patchelf")
            .args([
                "--set-interpreter",
                LOADER,
                "--force-rpath",
                "--set-rpath",
                LIB,
            ])
            .arg(destination),
    )
}

fn collect() -> Result<()> {
    let root = Path::new(ROOT);
    let tools = root.join("linux");
    let notices = tools.join("licenses");
    for path in [
        tools.join("bin"),
        Path::new(LIB).to_path_buf(),
        notices.clone(),
    ] {
        fs::create_dir_all(path)?;
    }
    let tool_bin = tools.join("bin");
    for name in ["bwrap", "slirp4netns", "zstd"] {
        executable(&Path::new("/usr/bin").join(name), &tool_bin.join(name))?;
    }
    execute(
        Command::new("cp")
            .args(["-a", "/usr/share/zoneinfo"])
            .arg(tools.join("zoneinfo")),
    )?;
    let installed = linux_ocr::output("dpkg-query", &["-W", "-f=${Package} ${Version}\n"])?;
    fs::write(notices.join("packages.tsv"), &installed)?;
    for line in installed.lines() {
        let (package, _) = line
            .split_once(' ')
            .ok_or("Debian package record missing")?;
        let copyright = Path::new("/usr/share/doc").join(package).join("copyright");
        if copyright.is_file() {
            fs::copy(copyright, notices.join(package))?;
        }
    }
    linux_ocr::collect(&root.join("ocr"))?;

    for executable in [
        tools.join("bin/bwrap"),
        tools.join("bin/slirp4netns"),
        tools.join("bin/zstd"),
    ] {
        execute(Command::new(executable).arg("--version"))?;
    }
    Ok(())
}

fn guest_disk(output: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;
    let work = tempfile::tempdir_in(output)?;
    let root = work.path().join("root");
    fs::create_dir(&root)?;
    linux_rootfs::bootstrap(&root, "amd64")?;
    linux_rootfs::install_packages(
        &root,
        [
            "busybox-static",
            "adb",
            "kmod",
            "util-linux",
            "iproute2",
            "tzdata",
        ],
    )?;
    for path in [
        "opt/aiphone/engine/bin",
        "opt/aiphone/native-phone-bridge/platform-tools",
        "opt/aiphone/linux",
        "opt/aiphone/bundle",
        "usr/local/libexec",
        "mnt",
        "run",
        "sys",
        "proc",
        "dev",
    ] {
        fs::create_dir_all(root.join(path))?;
    }
    symlink("/bin/busybox", root.join("opt/aiphone/engine/bin/busybox"))?;
    symlink(
        "/usr/bin/adb",
        root.join("opt/aiphone/native-phone-bridge/platform-tools/adb"),
    )?;
    fs::write(
        root.join("etc/inittab"),
        "::once:/usr/local/libexec/aiphone-bootstrap --qemu-guest-control\n::shutdown:/bin/umount -a -r\n",
    )?;
    let resolver = root.join("etc/resolv.conf.install");
    fs::write(&resolver, "nameserver 10.0.2.3\n")?;
    fs::rename(resolver, root.join("etc/resolv.conf"))?;
    fs::write(root.join("etc/machine-id"), "")?;
    for path in [
        "usr/local/libexec/aiphone-bootstrap",
        "opt/aiphone/bundle/source.payload",
        "opt/aiphone/bundle/source.sha256",
        "opt/aiphone/bundle/images.payload",
    ] {
        fs::File::create(root.join(path))?;
    }
    execute(linux_rootfs::chroot(&root, "/usr/bin/apt-get").arg("clean"))?;
    fs::remove_dir_all(root.join("var/lib/apt/lists"))?;
    let disk = work.path().join("guest.ext4");
    fs::File::create(&disk)?.set_len(2 << 30)?;
    execute(
        Command::new("mke2fs")
            .args(["-q", "-F", "-t", "ext4", "-m", "0", "-L", "aimanity", "-d"])
            .arg(&root)
            .arg(&disk),
    )?;
    execute(
        Command::new("qemu-img")
            .args(["convert", "-f", "raw", "-O", "qcow2", "-c"])
            .arg(disk)
            .arg(output.join("guest.qcow2")),
    )?;
    Ok(())
}

pub(crate) fn run(args: &[OsString]) -> Result<Value> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err("Windows guest assets require Linux amd64".into());
    }
    match args {
        [command] if command == "collect" => {
            collect()?;
            Ok(Value::Null)
        }
        [destination] => {
            let destination = std::path::absolute(destination)?;
            let parent = destination
                .parent()
                .ok_or("Windows asset output parent missing")?;
            let chroot = tempfile::tempdir_in(parent)?;
            linux_rootfs::bootstrap(chroot.path(), "amd64")?;
            linux_rootfs::install_packages(chroot.path(), PACKAGES)?;
            fs::copy(
                std::env::current_exe()?,
                chroot.path().join("usr/local/bin/release-tools"),
            )?;
            execute(
                linux_rootfs::chroot(chroot.path(), "/usr/local/bin/release-tools")
                    .args(["windows-guest-assets", "collect"]),
            )?;
            let staged = chroot.path().join(ROOT.trim_start_matches('/'));
            linux_ocr::seal(&staged.join("ocr"))?;
            guest_disk(&staged)?;
            fs::rename(staged, &destination)?;
            Ok(json!({"directory":destination}))
        }
        _ => Err("windows-guest-assets requires an output directory".into()),
    }
}
