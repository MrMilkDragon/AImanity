#[cfg(any(target_os = "macos", windows))]
mod bundle;
#[cfg(target_os = "linux")]
pub(super) mod linux_packages;
#[cfg(target_os = "linux")]
pub(super) mod linux_payload;
mod notices;
#[cfg(any(target_os = "macos", windows))]
mod resources;

use crate::Result;
#[cfg(any(target_os = "macos", windows))]
use crate::source;
#[cfg(any(target_os = "macos", windows))]
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
#[cfg(any(target_os = "macos", windows))]
use serde_json::json;
#[cfg(any(target_os = "macos", windows))]
use sha2::Sha512;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io,
    path::Path,
    process::Command,
};

#[cfg(any(target_os = "macos", windows))]
use std::{ffi::OsString, io::Read, path::PathBuf};

#[cfg(any(target_os = "macos", windows))]
#[derive(Clone, Copy, PartialEq)]
enum Signing {
    Unsigned,
    Temporary,
    Release,
}

pub(super) fn execute(command: &mut Command) -> Result<()> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{}: {status}", command.get_program().to_string_lossy()).into());
    }
    Ok(())
}
pub(super) fn output(command: &mut Command) -> Result<String> {
    let result = command.output()?;
    if !result.status.success() {
        return Err(format!(
            "{}: {}",
            command.get_program().to_string_lossy(),
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}
pub(super) fn json_file(file: impl AsRef<Path>) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(file)?)?)
}
pub(super) fn file_digest(file: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    io::copy(&mut File::open(file)?, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(any(target_os = "macos", windows))]
fn build_binary(root: &Path, triple: &str) -> Result<PathBuf> {
    if output(Command::new("rustc").arg("--version"))?
        .split_whitespace()
        .nth(1)
        != Some("1.96.0")
    {
        return Err("Rust 1.96.0 is required".into());
    }
    let profile = root.join("apps/native");
    let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    let mut command = Command::new("cargo");
    command
        .args([
            "build",
            "--release",
            "--locked",
            "--target",
            triple,
            "--manifest-path",
        ])
        .arg(profile.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", profile.join("target"))
        .env("CARGO_INCREMENTAL", "0")
        .env("CI", "true")
        .env(
            "AIMANITY_BUNDLE_INTEGRITY",
            root.join(resources::HOST).join("integrity.json"),
        );
    if std::env::var_os("CARGO_BUILD_JOBS").is_none() {
        command.env("CARGO_BUILD_JOBS", "2");
    }
    let mut rustflags = std::env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    if !rustflags.is_empty() {
        rustflags.push('\x1f');
    }
    rustflags.push_str(&format!("--remap-path-prefix={home}=/build-user"));
    if cfg!(windows) {
        rustflags.push_str("\x1f-Ctarget-feature=+crt-static");
    } else {
        for key in ["CFLAGS", "CXXFLAGS"] {
            let existing = std::env::var(key).unwrap_or_default();
            command.env(
                key,
                format!("{existing} -ffile-prefix-map={home}=/build-user"),
            );
        }
    }
    command.env("CARGO_ENCODED_RUSTFLAGS", rustflags);
    execute(&mut command)?;
    Ok(profile
        .join("target")
        .join(triple)
        .join("release")
        .join(if cfg!(windows) {
            "aimanity-native.exe"
        } else {
            "aimanity-native"
        }))
}

#[cfg(any(target_os = "macos", windows))]
fn installer_manifest(
    root: &Path,
    installer: &Path,
    host: &str,
    version: &str,
    signature: Value,
    inspection: Value,
    plan: &resources::Plan,
) -> Result<Value> {
    let mut file = File::open(installer)?;
    let size = file.metadata()?.len();
    let mut sha256 = Sha256::new();
    let mut sha512 = Sha512::new();
    let mut bytes = [0; 128 * 1024];
    loop {
        let length = file.read(&mut bytes)?;
        if length == 0 {
            break;
        }
        sha256.update(&bytes[..length]);
        sha512.update(&bytes[..length]);
    }
    let mut components: Vec<String> = serde_json::from_value(source::payload_components(
        &root.join(resources::HOST).join("source.payload"),
    )?)?;
    for (resource, role) in [
        ("android.payload", "android-runtime"),
        ("images.payload", "container-images"),
        ("qemu/manifest.json", "windows-qemu"),
    ] {
        if plan.integrity.contains_key(resource) {
            components.push(role.into());
        }
    }
    let required: &[&str] = if cfg!(windows) {
        &["android-runtime", "windows-qemu"]
    } else {
        &[
            "native-services",
            "phone-apps",
            "container-host",
            "android-runtime",
            "container-images",
        ]
    };
    let missing: Vec<_> = required
        .iter()
        .filter(|name| !components.iter().any(|component| component == **name))
        .collect();
    let complete = missing.is_empty();
    let source = output(Command::new("git").args(["rev-parse", "HEAD"]))?;
    let mut manifest = json!({
        "version":version,"kind":"native-client","interface":"iced-wgpu","os":std::env::consts::OS,
        "arch":if cfg!(windows) {"x64"} else {"arm64"},"sha256":format!("{:x}",sha256.finalize()),
        "sha512":STANDARD.encode(sha512.finalize()),"runtime_acceptance":"pending",
        "dependencies_license_review_complete":false,"remote_upload":false,
        "third_party_notices":{"rust_missing_files":[],"texts":{"native-client-licenses.txt":file_digest(&root.join(resources::NOTICES))?}},
        "source_commit":source,"builder_commit":source,"source_locks":{"Cargo.lock":file_digest(&root.join("Cargo.lock"))?,"apps/native/Cargo.lock":file_digest(&root.join("apps/native/Cargo.lock"))?},
        "source_payload_included":true,
        "local_bootstrap":{"implemented_on":if cfg!(windows)&&complete {vec!["macos-arm64","windows-x64"]} else {vec!["macos-arm64"]},
            "fresh_install_acceptance":"pending","user_state_bundled":false},
        "payload":inspection,"prebuilt_components":components,
        "server_included":components.iter().any(|name| name=="windows-qemu" || name=="native-services"),
        "runtime_payload_complete":complete,"complete_device_bundle":complete,"missing_runtime_components":missing,
        "deployment_mode":if complete {"embedded-local"} else {"component-bundle"},
        "server_requirement":if complete {"None; the client initializes its local device runtime."} else {"Complete local runtime components are required."},
        "installer_delivery":{"file":installer.file_name(),"host":host,"format":installer.extension(),"size":size,"single_file":true,
            "user_assembly_required":false,"github_asset_eligible":size<2*1024*1024*1024},
    });
    manifest
        .as_object_mut()
        .ok_or("Installer receipt missing")?
        .extend(serde_json::from_value::<serde_json::Map<String, Value>>(
            signature,
        )?);
    let mut receipt = installer.as_os_str().to_owned();
    receipt.push(".manifest.json");
    serde_json::to_writer_pretty(File::create(receipt)?, &manifest)?;
    Ok(manifest)
}

#[cfg(any(target_os = "macos", windows))]
pub(super) fn run(args: &[OsString]) -> Result<Value> {
    let signing = match args {
        [] => Signing::Unsigned,
        [flag] if flag == "--sign-temporary" => Signing::Temporary,
        [flag] if flag == "--sign-release" => Signing::Release,
        _ => return Err("client-installer accepts --sign-temporary or --sign-release".into()),
    };
    let (host, triple, suffix) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("macos-arm64", "aarch64-apple-darwin", "macos-arm64.dmg"),
        ("windows", "x86_64") => (
            "windows-x64",
            "x86_64-pc-windows-msvc",
            "windows-x64-setup.exe",
        ),
        _ => return Err("Unsupported installer build host".into()),
    };
    let root = std::env::current_dir()?;
    source::run(&["check".into()])?;
    let plan = resources::Plan::read(&root)?;
    let policy = json_file(root.join("config/release-policy.json"))?;
    let version = policy["version"]
        .as_str()
        .ok_or("Missing release version")?;
    let dist = root.join("dist");
    fs::create_dir_all(&dist)?;
    let installer = dist.join(format!("AImanity-{version}-{suffix}"));
    notices::write(&root, triple, &root.join(resources::NOTICES))?;
    let binary = build_binary(&root, triple)?;
    let (signature, payload) =
        bundle::assemble(&root, &plan, &binary, &installer, version, signing)?;
    let mut receipt =
        installer_manifest(&root, &installer, host, version, signature, payload, &plan)?;
    receipt["artifact"] = json!(installer);
    Ok(receipt)
}
