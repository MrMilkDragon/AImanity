use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::File,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    time::Instant,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Deserialize)]
struct ReleasePolicy {
    version: String,
}

#[derive(Deserialize)]
struct InstallerReceipt {
    source_commit: String,
    interface: String,
    complete_device_bundle: bool,
    authenticode: Authenticode,
    installer_delivery: Delivery,
    sha256: String,
}

#[derive(Deserialize)]
struct Authenticode {
    cryptographically_verified: bool,
}

#[derive(Deserialize)]
struct Delivery {
    size: u64,
}

fn check(condition: bool, error: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(error.into().into())
    }
}

fn valid_digest(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn main() -> Result<()> {
    let root = PathBuf::from(env::var("AIMANITY_QA_ROOT")?);
    check(
        cfg!(windows)
            && env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
            && root
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("AImanity-QA-")),
        "Use the disposable Windows installer runner",
    )?;
    let source = env::var("AIMANITY_QA_SOURCE")?;
    check(
        valid_digest(&source, 40),
        "Supply the reviewed source commit",
    )?;
    let started = Instant::now();
    let install = root.join("installed");
    let profile = root.join("profile");
    let home = profile.join("dev.aimanity.workspace/host");
    check(
        !home.exists() && !install.exists(),
        "fresh isolated Windows runtime",
    )?;
    let ReleasePolicy { version } =
        serde_json::from_reader(File::open("config/release-policy.json")?)?;
    let candidate = root.join("candidate");
    let executable = format!("AImanity-{version}-windows-x64-setup.exe");
    let installer = candidate.join(&executable);
    let receipt: InstallerReceipt = serde_json::from_reader(File::open(
        candidate.join(format!("{executable}.manifest.json")),
    )?)?;
    check(
        receipt.source_commit == source
            && receipt.interface == "iced-wgpu"
            && receipt.complete_device_bundle
            && receipt.authenticode.cryptographically_verified,
        "complete signed native installer for the reviewed source",
    )?;
    let mut file = File::open(&installer)?;
    check(
        file.metadata()?.len() == receipt.installer_delivery.size,
        "candidate size matches its installer receipt",
    )?;
    let mut hash = Sha256::new();
    let mut block = [0; 64 * 1024];
    loop {
        let count = file.read(&mut block)?;
        if count == 0 {
            break;
        }
        hash.update(&block[..count]);
    }
    check(
        hex::encode(hash.finalize()) == receipt.sha256,
        "candidate matches its signed installer receipt",
    )?;
    drop(file);
    let status = Command::new(&installer)
        .args([
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/SP-",
            "/NORESTART",
            "/RESTARTEXITCODE=3010",
            "/NORESTARTAPPLICATIONS",
        ])
        .arg(format!("/DIR={}", install.display()))
        .stdout(Stdio::null())
        .status()?;
    check(
        status.success(),
        format!("signed Inno installation must succeed without pending reboot: {status}"),
    )?;
    println!(
        "{}",
        json!({"installerExit": 0, "ms": started.elapsed().as_millis()})
    );

    let status = Command::new(install.join("ai-phone-client.exe"))
        .arg("--verify-bundled-host")
        .arg(install.join("host"))
        .status()?;
    check(
        status.success(),
        "installed offline resources must match the executable",
    )
}
