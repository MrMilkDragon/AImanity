use crate::{Result, image_factory::execute, linux_ocr, linux_rootfs};
use serde_json::{Value, json};
use std::{ffi::OsString, fs, path::Path, process::Command};

const ROOT: &str = "/opt/aiphone";
const LIB: &str = "/opt/aiphone/linux/lib";
const LOADER: &str = "/opt/aiphone/linux/lib/ld-linux-x86-64.so.2";
const POSTGRES: &str = "lib/postgresql/17";
const PACKAGES: [&str; 10] = [
    "bubblewrap",
    "slirp4netns",
    "zstd",
    "patchelf",
    "postgresql-17",
    "tzdata",
    "redis-server",
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
    let pg = root.join("postgres");
    let pg_destination = pg.join(POSTGRES);
    let pg_source = Path::new("/usr").join(POSTGRES);
    let tools = root.join("linux");
    let redis = root.join("redis/bin");
    let notices = tools.join("licenses");
    for path in [
        pg_destination.join("bin"),
        pg_destination.join("lib"),
        pg.join("share/postgresql"),
        tools.join("bin"),
        Path::new(LIB).to_path_buf(),
        redis.clone(),
        notices.clone(),
    ] {
        fs::create_dir_all(path)?;
    }
    let tool_bin = tools.join("bin");
    for name in ["bwrap", "slirp4netns", "zstd", "redis-server", "redis-cli"] {
        let bin = if name.starts_with("redis-") {
            &redis
        } else {
            &tool_bin
        };
        executable(&Path::new("/usr/bin").join(name), &bin.join(name))?;
    }
    for name in ["postgres", "initdb", "psql", "pg_dump", "pg_restore"] {
        executable(
            &pg_source.join("bin").join(name),
            &pg_destination.join("bin").join(name),
        )?;
    }
    for library in fs::read_dir(pg_source.join("lib"))? {
        let library = library?;
        let name = library.file_name();
        if library
            .path()
            .extension()
            .is_some_and(|value| value == "so")
            && name != "llvmjit.so"
        {
            libraries(&library.path())?;
            fs::copy(library.path(), pg_destination.join("lib").join(name))?;
        }
    }
    execute(
        Command::new("cp")
            .args(["-aL", "/usr/share/postgresql/17"])
            .arg(pg.join("share/postgresql")),
    )?;
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

    let database = tempfile::tempdir()?;
    execute(
        Command::new("chown")
            .arg("65534:65534")
            .arg(database.path()),
    )?;
    execute(
        Command::new("setpriv")
            .args(["--reuid=65534", "--regid=65534", "--clear-groups", "--"])
            .arg(pg_destination.join("bin/initdb"))
            .args([
                "-U",
                "ai_phone",
                "--locale-provider=builtin",
                "--locale=C.UTF-8",
                "-D",
            ])
            .arg(database.path().join("data")),
    )?;
    for executable in [
        pg_destination.join("bin/postgres"),
        redis.join("redis-server"),
        redis.join("redis-cli"),
        tools.join("bin/bwrap"),
        tools.join("bin/slirp4netns"),
        tools.join("bin/zstd"),
    ] {
        execute(Command::new(executable).arg("--version"))?;
    }
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
            fs::rename(staged, &destination)?;
            Ok(json!({"directory":destination}))
        }
        _ => Err("windows-guest-assets requires an output directory".into()),
    }
}
