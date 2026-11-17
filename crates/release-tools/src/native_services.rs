use crate::{Result, source};
use regex::bytes::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File},
    io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const ROLES: [(&str, &str); 3] = [
    ("assistant", "ai-phone-assistant"),
    ("runner", "ai-phone-runner"),
    ("vision", "ai-phone-vision"),
];
const CONTRACTS: [(&str, &str); 8] = [
    ("device_setup", "ai-phone-device-setup-v2"),
    ("service_supervision", "ai-phone-linux-supervision-v2"),
    ("phone_input_setup", "ai-phone-input-setup-v2"),
    ("database_prepare", "ai-phone-database-prepare-v1"),
    ("host_profile", "ai-phone-host-profile-v1"),
    ("managed_engine", "ai-phone-managed-engine-v1"),
    ("image_activation", "ai-phone-image-activation-v1"),
    ("runtime_environment", "ai-phone-runtime-environment-v1"),
];
const FIXTURE: &str = "crates/phone-runner/fixtures/ocr-probe.png";
const RECEIPT: &str = "target/release/native-service-build.json";

fn host() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("macos-arm64"),
        ("linux", "x86_64") => Ok("linux-x64"),
        _ => Err("Unsupported native service build host".into()),
    }
}

fn output(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(format!("{program}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().into())
}

fn fingerprint() -> Result<String> {
    let root = Path::new(".");
    let mut paths = [
        "Cargo.toml",
        "Cargo.lock",
        "config/native-client.json",
        "config/ocr-models.json",
    ]
    .map(str::to_owned)
    .to_vec();
    for directory in ["crates", "packages/protocol/proto"] {
        source::files(root, directory, &mut paths)?;
    }
    paths.sort_unstable();
    let mut hash = Sha256::new();
    for path in paths {
        hash.update(path.as_bytes());
        hash.update(Sha256::digest(fs::read(path)?));
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn artifact(path: &Path) -> Result<Value> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!("Native executable missing: {}", path.display()).into());
    }
    let mut digest = Sha256::new();
    let size = io::copy(&mut File::open(path)?, &mut digest)?;
    Ok(json!({"path": path, "size": size, "sha256": format!("{:x}",digest.finalize())}))
}

fn cached(source: &str) -> Result<Option<Value>> {
    let receipt: Value = match File::open(RECEIPT) {
        Ok(file) => serde_json::from_reader(file)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if receipt["host"] != host()? || receipt["source_sha256"] != source {
        return Ok(None);
    }
    for (role, package) in ROLES {
        let path = Path::new("target/release").join(package);
        if receipt["artifacts"][role] != artifact(&path)? {
            return Err(format!("Native service artifact changed: {role}").into());
        }
    }
    Ok(Some(receipt))
}

struct Probe {
    home: tempfile::TempDir,
}

impl Probe {
    async fn run(
        &self,
        path: &Path,
        args: &[&str],
        fixture: Option<&Path>,
        bundled: bool,
    ) -> Result<std::process::Output> {
        let home = self.home.path();
        let mut command = tokio::process::Command::new(path.canonicalize()?);
        command
            .args(args)
            .current_dir(home)
            .env_clear()
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env("AI_PHONE_PROJECT_ROOT", home)
            .env("PATH", "/usr/bin:/bin")
            .kill_on_drop(true);
        command.envs(std::env::vars_os().filter(|(key, _)| key == "TMPDIR" || key == "LANG"));
        if let Some(fixture) = fixture {
            command.stdin(Stdio::from(File::open(fixture)?));
            if !bundled {
                for (source, target) in [
                    ("AI_PHONE_TEST_TESSERACT", "AI_PHONE_TESSERACT"),
                    ("AI_PHONE_TEST_TESSDATA", "TESSDATA_PREFIX"),
                ] {
                    if let Some(value) = std::env::var_os(source) {
                        command.env(target, value);
                    }
                }
            }
        }
        let output = tokio::time::timeout(Duration::from_secs(10), command.output()).await??;
        if !output.status.success() {
            return Err(format!(
                "{}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(output)
    }

    async fn version(&self, path: &Path, package: &str) -> Result<Value> {
        let result = self.run(path, &["--version"], None, false).await?;
        let version = std::str::from_utf8(&result.stdout)?
            .trim()
            .strip_prefix(&format!("{package} "))
            .filter(|version| !version.contains(char::is_whitespace))
            .ok_or("Native version receipt differs")?;
        if !result.stderr.is_empty() || fs::read_dir(self.home.path())?.next().is_some() {
            return Err(format!("Native version probe changed state: {package}").into());
        }
        Ok(json!({"program":package,"version":version,"state_created":false}))
    }

    async fn vision(&self, path: &Path, fixture: &Path, bundled: bool) -> Result<Value> {
        let png = fs::read(fixture)?;
        let bounds = |offset| {
            png.get(offset..offset + 4)
                .and_then(|value| value.try_into().ok())
                .map(u32::from_be_bytes)
                .ok_or("OCR fixture PNG header missing")
        };
        let (width, height) = (bounds(16)?, bounds(20)?);
        let result = self.run(path, &[], Some(fixture), bundled).await?;
        let value: Value = serde_json::from_slice(&result.stdout)?;
        let regions = value["regions"].as_array().ok_or("OCR regions missing")?;
        if value["width"] != width
            || value["height"] != height
            || value["grid"].as_array().map(Vec::len) != Some(2048)
            || !regions.iter().any(|row| {
                row["confidence"].as_f64().is_some_and(|value| value >= 0.8)
                    && row["text"]
                        .as_str()
                        .is_some_and(|text| text.split_whitespace().collect::<String>() == "7319")
            })
        {
            return Err("OCR fixture did not recognize 7319".into());
        }
        Ok(json!({"fixture_text":"7319","width":width,"height":height,"grid":2048}))
    }
}

async fn build() -> Result<Value> {
    let source = fingerprint()?;
    if let Some(receipt) = cached(&source)? {
        return Ok(receipt);
    }
    let host = host()?;
    let compiler = output("rustc", &["--version"])?;
    if compiler.split_ascii_whitespace().nth(1) != Some("1.96.0") {
        return Err("Pinned Rust 1.96.0 required for runtime notices".into());
    }
    let root = std::env::current_dir()?;
    let home = std::env::var("HOME")?;
    let mut command = Command::new("cargo");
    command.args([
        "build",
        "--release",
        "--locked",
        "--features",
        "ai-phone-vision/native",
    ]);
    for (_, package) in ROLES {
        command.args(["-p", package]);
    }
    if !command
        .env("CARGO_BUILD_JOBS", "2")
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("CARGO_PROFILE_RELEASE_DEBUG", "0")
        .env("CARGO_PROFILE_RELEASE_STRIP", "debuginfo")
        .env(
            "CARGO_ENCODED_RUSTFLAGS",
            format!("--remap-path-prefix={home}=/build-user"),
        )
        .status()?
        .success()
    {
        return Err("Native Cargo build failed".into());
    }
    let probe = Probe {
        home: tempfile::tempdir()?,
    };
    let secret = Regex::new(
        r"(?-u)(^|[^A-Za-z0-9_-])(?:sk-[A-Za-z0-9_-]{24,}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|gh[pousr]_[A-Za-z0-9]{30,})",
    )?;
    let mut checks = Map::new();
    let mut artifacts = Map::new();
    for (role, package) in ROLES {
        let path = Path::new("target/release").join(package);
        checks.insert(role.into(), probe.version(&path, package).await?);
        let bytes = fs::read(&path)?;
        let filtered: Vec<_> = bytes.iter().copied().filter(|byte| *byte != 0).collect();
        if secret.is_match(&bytes)
            || secret.is_match(&filtered)
            || bytes
                .windows(home.len() + 1)
                .any(|window| window.starts_with(home.as_bytes()) && window.ends_with(b"/"))
        {
            return Err(format!("Private material in native artifact: {role}").into());
        }
        artifacts.insert(
            role.into(),
            json!({"path":path,"size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}),
        );
    }
    for (name, contract) in CONTRACTS {
        let flag = format!("--{}-contract", name.replace('_', "-"));
        let result = probe
            .run(
                Path::new("target/release/ai-phone-assistant"),
                &[&flag],
                None,
                false,
            )
            .await?;
        if std::str::from_utf8(&result.stdout)?.trim() != contract {
            return Err(format!("Native contract differs: {name}").into());
        }
        checks.insert(name.into(), json!(contract));
    }
    let vision = probe
        .vision(
            Path::new("target/release/ai-phone-vision"),
            Path::new(FIXTURE),
            false,
        )
        .await?;
    let vision_version = checks
        .remove("vision")
        .ok_or("Vision version receipt missing")?;
    checks.insert("vision_version".into(), vision_version);
    checks.insert("vision".into(), vision);
    let receipt = json!({"format":1,"host": host,"source_sha256": source,"compiler":compiler,
        "artifacts":artifacts,"checks":checks,"developer_paths_remapped":true});
    let mut pending = tempfile::NamedTempFile::new_in("target/release")?;
    serde_json::to_writer(&mut pending, &receipt)?;
    pending.persist(RECEIPT)?;
    Ok(receipt)
}

async fn verify(path: &Path) -> Result<Value> {
    let services = path.join("runtime/services");
    let manifest: Value = serde_json::from_reader(File::open(services.join("manifest.json"))?)?;
    let probe = Probe {
        home: tempfile::tempdir()?,
    };
    let mut result = Map::new();
    for (role, package) in &ROLES[..2] {
        let probe = probe
            .version(&services.join("bin").join(role), package)
            .await?;
        if probe["version"] != manifest["versions"][*role] {
            return Err(format!("Packaged native version differs: {role}").into());
        }
        result.insert((*role).into(), probe);
    }
    result.insert(
        "vision".into(),
        probe
            .vision(
                &services.join("bin/vision"),
                &services.join("fixtures/ocr.png"),
                manifest["vision"] == "linux-tesseract-bundled",
            )
            .await?,
    );
    Ok(Value::Object(result))
}

pub(super) fn run(args: &[OsString]) -> Result<Value> {
    match args {
        [operation] if operation == "receipt" => cached(&fingerprint()?)?
            .ok_or_else(|| "Build source-matched native services first".into()),
        [operation] if operation == "build" => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(build()),
        [operation, directory] if operation == "verify" => {
            let directory = PathBuf::from(directory);
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(verify(&directory))
        }
        _ => Err("native-services requires build, receipt, or verify DIRECTORY".into()),
    }
}
