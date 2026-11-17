use crate::{Error, Result, component, profile, runtime_environment};
use serde_json::{Value, json};
use std::{
    env,
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

const INVALID: Error = Error::Invalid("source_project_arguments");

fn option<'a>(args: &'a [OsString], name: &str) -> Option<&'a OsStr> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_os_str())
}

fn read(path: &Path) -> Result<Value> {
    serde_json::from_reader(File::open(path)?).map_err(|_| Error::Invalid("host_profile"))
}

fn prompt(label: &str, current: &Value) -> Result<String> {
    let shown = current
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| current.to_string());
    print!("{label} [{shown}] ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let value = line.trim();
    Ok(match value {
        "" => shown,
        "-" => String::new(),
        _ => value.into(),
    })
}

fn deployment_profile(
    root: &Path,
    user: &Path,
    supplied: Option<&OsStr>,
    installing: bool,
) -> Result<Value> {
    let saved = root.join("data/deployment/profile.json");
    let saved = ((installing || supplied.is_none()) && saved.try_exists()?).then_some(saved);
    if installing && let Some(path) = &saved {
        return read(path);
    }
    let mut value = if let Some(path) = supplied.map(Path::new).or(saved.as_deref()) {
        read(path)?
    } else {
        let env = runtime_environment::resolve(root, user)?;
        let template = if env["AI_PHONE_ANDROID_ABI"] == "x86_64" {
            "android35-x86_64"
        } else {
            "android35-arm64"
        };
        json!({
            "schema_version":1, "local_entry":true, "locale":"en", "launch_path":env["PATH"],
            "sdk_root":env["AI_PHONE_SDK_ROOT"],
            "host_proxy":env["AI_PHONE_HOST_PROXY"], "container_proxy":env["AI_PHONE_CONTAINER_PROXY"],
            "phone":{"template":template,"cpus":2,"memory_mb":2048,"storage_gb":16},
            "desktop":{"template":"debian13-openbox","cpus":2,"memory_mb":2560}
        })
    };
    if supplied.is_none() {
        for key in ["locale", "sdk_root", "host_proxy", "container_proxy"] {
            value[key] = json!(prompt(key, &value[key])?);
        }
        for (device, keys) in [
            ("phone", &["cpus", "memory_mb", "storage_gb"][..]),
            ("desktop", &["cpus", "memory_mb"][..]),
        ] {
            for key in keys {
                value[device][key] = json!(
                    prompt(&format!("{device} {key}"), &value[device][key])?
                        .parse::<u32>()
                        .map_err(|_| Error::Invalid("host_profile"))?
                );
            }
        }
    }
    profile::save_user_profile(root, &value)?;
    Ok(value)
}

fn assistant(root: &Path) -> PathBuf {
    if root.join("runtime/services").is_dir() {
        root.join("runtime/services/bin/assistant")
    } else {
        root.join(env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into()))
            .join("release/ai-phone-assistant")
    }
}

fn total_memory() -> Option<u64> {
    if cfg!(target_os = "macos") {
        String::from_utf8_lossy(
            &Command::new("/usr/sbin/sysctl")
                .args(["-n", "hw.memsize"])
                .output()
                .ok()?
                .stdout,
        )
        .trim()
        .parse()
        .ok()
    } else {
        fs::read_to_string("/proc/meminfo")
            .ok()?
            .lines()
            .find_map(|line| {
                line.strip_prefix("MemTotal:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
                    .map(|value| value * 1024)
            })
    }
}

fn doctor(root: &Path, args: &[OsString]) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let path = env::var_os("PATH");
    let executable = |name: &str| {
        path.as_ref().is_some_and(|path| {
            env::split_paths(path).any(|directory| {
                fs::metadata(directory.join(name))
                    .is_ok_and(|info| info.is_file() && info.permissions().mode() & 0o111 != 0)
            })
        })
    };
    let checks = [
        (
            "host",
            cfg!(any(
                all(target_os = "macos", target_arch = "aarch64"),
                all(target_os = "linux", target_arch = "x86_64")
            )),
        ),
        ("native-services", assistant(root).is_file()),
        ("cargo", executable("cargo")),
        (
            "memory",
            total_memory().is_some_and(|bytes| bytes >= 12 << 30),
        ),
        ("disk", fs2::available_space(root)? >= 35 << 30),
    ];
    if args.iter().any(|arg| arg == "--json") {
        println!(
            "{}",
            Value::Array(
                checks
                    .map(|(name, passed)| json!({"name":name,"passed":passed}))
                    .into()
            )
        );
    } else {
        for (name, passed) in checks {
            println!("{} {name}", if passed { "OK" } else { "FAIL" });
        }
    }
    checks
        .into_iter()
        .all(|(_, passed)| passed)
        .then_some(())
        .ok_or(Error::Invalid("source_prerequisites"))
}

fn install(root: &Path, user: &Path, args: &[OsString]) -> Result<()> {
    let value = deployment_profile(root, user, option(args, "--profile"), true)?;
    let resources = option(args, "--resources")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("apps/native/gen/host"));
    let integrity = read(&resources.join("integrity.json"))?;
    let home = option(args, "--destination")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            (if cfg!(target_os = "macos") {
                user.join("Library/Application Support")
            } else {
                env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| user.join(".local/share"))
            })
            .join("dev.aimanity.native/host")
        });
    let request = tempfile::NamedTempFile::new()?;
    let phone = &value["phone"];
    let desktop = &value["desktop"];
    serde_json::to_writer(
        request.as_file(),
        &json!({"locale":value["locale"],
            "phone":{"cpus":phone["cpus"],"memory_mb":phone["memory_mb"],"storage_gb":phone["storage_gb"]},
            "desktop":{"cpus":desktop["cpus"],"memory_mb":desktop["memory_mb"],"storage_gb":desktop["storage_gb"]}
        }),
    )
    .map_err(|_| Error::Invalid("host_profile"))?;
    let mut command = vec![
        "--initialize-unix-host".into(),
        resources.into(),
        home.into(),
        request.path().into(),
    ];
    for key in ["source.payload", "android.payload", "images.payload"] {
        command.push(
            integrity[key]
                .as_str()
                .ok_or(Error::Invalid("source_resource_identity"))?
                .into(),
        );
    }
    component::run(&command).ok_or(INVALID)?
}

pub(crate) fn run(args: &[OsString]) -> Result<()> {
    let [root, operation, rest @ ..] = args else {
        return Err(INVALID);
    };
    let root = Path::new(root).canonicalize()?;
    let user = PathBuf::from(env::var_os("HOME").ok_or(INVALID)?);
    let status = |status: std::process::ExitStatus| {
        status
            .success()
            .then_some(())
            .ok_or(Error::Invalid("source_project_command"))
    };
    match operation.to_str().ok_or(INVALID)? {
        "doctor" => doctor(&root, rest),
        "configure" => {
            println!(
                "{}",
                deployment_profile(&root, &user, option(rest, "--profile"), false)?
            );
            Ok(())
        }
        "install" => install(&root, &user, rest),
        "start" | "stop" => {
            component::run(&["--source-services".into(), root.into(), operation.clone()])
                .ok_or(INVALID)?
        }
        "open" if cfg!(target_os = "macos") => {
            status(Command::new("open").args(["-a", "AImanity"]).status()?)
        }
        "env" => runtime_environment::run(&[root.into(), user.into(), "shell".into()]),
        "backup" | "restore" | "recover-desktop" => status(
            Command::new(assistant(&root))
                .current_dir(&root)
                .arg("--state-backup")
                .arg(if operation == "backup" {
                    OsStr::new("create")
                } else {
                    operation
                })
                .arg("--root")
                .arg(&root)
                .args(rest)
                .status()?,
        ),
        _ => Err(INVALID),
    }
}
