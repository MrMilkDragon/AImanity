use super::{checked, guest, read_json};
use crate::{Result, android_component, client_payload, source};
use serde_json::{Value, json};
use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const HOST: &str = "apps/native/gen/host";

fn output(command: &mut Command) -> Result<Value> {
    let result = command.output()?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).into_owned().into());
    }
    Ok(serde_json::from_slice(&result.stdout)?)
}

fn prepare_android(root: &Path, work: &Path) -> Result<(PathBuf, Value)> {
    let (file, digest) = match (
        env::var_os("AIMANITY_ANDROID_SDK_INPUT"),
        env::var_os("AIMANITY_ANDROID_PAYLOAD"),
        env::var_os("AIMANITY_ANDROID_PAYLOAD_SHA256"),
    ) {
        (Some(sdk), None, None) => {
            let file = work.join("android.payload");
            let receipt = android_component::seal(&[sdk, file.as_os_str().into(), "6".into()])?;
            (
                file,
                receipt["archive_sha256"]
                    .as_str()
                    .ok_or("Missing Android capsule identity")?
                    .to_owned(),
            )
        }
        (None, Some(file), Some(digest)) => (
            std::path::absolute(file)?,
            digest
                .into_string()
                .map_err(|_| "Invalid Android capsule identity")?,
        ),
        _ => {
            return Err(
                "Provide a staged Android SDK or a sealed Android payload with its identity".into(),
            );
        }
    };
    let target = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let qualifier = tempfile::tempdir_in(work)?;
    let stage = output(
        Command::new(target.join("release/host-resources.exe"))
            .arg("--prepare-android")
            .arg(qualifier.path())
            .arg(&file)
            .arg(&digest),
    )?;
    let sdk = Path::new(
        stage["sdkRoot"]
            .as_str()
            .ok_or("Missing qualified Android SDK")?,
    );
    let manifest = read_json(
        &sdk.parent()
            .ok_or("Missing Android SDK directory")?
            .join("ANDROID-MANIFEST.json"),
    )?;
    if manifest["engine"] != "whpx" {
        return Err("The Android payload requires Windows Hypervisor Platform".into());
    }
    let files = manifest["files"]
        .as_array()
        .ok_or("Missing Android file inventory")?;
    Ok((
        file,
        json!({
            "host":manifest["host"],"abi":manifest["abi"],"engine":"whpx",
            "files":files.len(),"bytes":files.iter().filter_map(|row| row["size"].as_u64()).sum::<u64>(),
            "archive_sha256":digest,"redistribution":"review-pending",
            "programs_prepared":true,"guest_boot":"not-executed",
        }),
    ))
}

pub(super) fn build(
    root: &Path,
    qemu: &Path,
    qemu_sha: &OsStr,
    guest_capsule: &Path,
    guest_sha: &OsStr,
    signing: &[OsString],
) -> Result<Value> {
    let (signing, signature) = match signing {
        [] => ("--sign-temporary", "authenticode-self-signed"),
        [flag] if flag == "--sign-release" => ("--sign-release", "authenticode-trusted"),
        _ => return Err("Expected an optional --sign-release flag".into()),
    };
    source::run(&["check".into(), "--repository".into(), "--manifest".into()])?;
    let source = guest::source(root)?;
    checked(Command::new("cargo").args([
        "build",
        "--release",
        "--locked",
        "-p",
        "ai-phone-host-runtime",
        "--bin",
        "host-resources",
    ]))?;
    let work = root.join(".tools");
    fs::create_dir_all(&work)?;
    let work = tempfile::tempdir_in(work)?;
    let (android_file, android) = prepare_android(root, work.path())?;
    let runtime = root.join(HOST).join("qemu");
    let identity = guest::stage(
        root,
        &runtime,
        qemu,
        qemu_sha.to_str().ok_or("Invalid QEMU identity")?,
        guest_capsule,
        guest_sha.to_str().ok_or("Invalid Windows guest identity")?,
    )?;
    client_payload::run(&[
        "--android-runtime".into(),
        android_file.into(),
        "--qemu-manifest".into(),
        identity.into(),
    ])?;
    checked(Command::new(env::current_exe()?).args(["client-installer", signing]))?;

    let policy = read_json(&root.join("config/release-policy.json"))?;
    let version = policy["version"]
        .as_str()
        .ok_or("Missing release version")?;
    let artifact = format!("AImanity-{version}-windows-x64-setup.exe");
    let dist = root.join("dist");
    let path = dist.join(format!("{artifact}.manifest.json"));
    let mut manifest = read_json(&path)?;
    if manifest["signing"] != signature
        || manifest["complete_device_bundle"] != true
        || signing == "--sign-release"
            && (manifest["authenticode"]["cryptographically_verified"] != true
                || manifest["authenticode"]["timestamped"] != true)
    {
        return Err("Windows installer incomplete".into());
    }
    if source != guest::source(root)? {
        return Err("Source inventory changed during Windows assembly".into());
    }
    manifest["backend_source_commit"] = source["source_commit"].clone();
    manifest["backend_source_manifest_sha256"] = source["source_manifest_sha256"].clone();
    fs::write(&path, serde_json::to_vec_pretty(&manifest)?)?;
    let delivery = &manifest["installer_delivery"];
    let file = json!({"file":artifact,"size":delivery["size"],"sha256":manifest["sha256"]});
    let mut artifact = file.clone();
    artifact["delivery"] = json!({
        "prebuilt_components":manifest["prebuilt_components"],
        "server_included":true,"runtime_payload_complete":true,"complete_device_bundle":true,
        "missing_runtime_components":[],"deployment_mode":"embedded-local",
        "server_requirement":"None; the client initializes its local device runtime.",
    });
    let mut package = file;
    package["format"] = json!("single");
    let report = json!({
        "format":1,"host":"windows-x64","source_commit":source["source_commit"],
        "source_manifest_sha256":source["source_manifest_sha256"],"backend_source_commit":source["source_commit"],
        "backend_source_manifest_sha256":source["source_manifest_sha256"],"builder_commit":source["source_commit"],
        "artifact":artifact,"package_delivery":package,"android":android,"backend_runtime":"qemu-whpx-backend",
        "backend_vm":read_json(&runtime.join("manifest.json"))?,"backend_runtime_acceptance":"not-tested",
        "physical_installation":"not-tested","windows_boot_verified":false,"binary_publication":false,
    });
    fs::write(
        dist.join("windows-full-factory-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if delivery["github_asset_eligible"] != true {
        return Err(format!(
            "Complete Windows EXE is {} bytes; GitHub release assets require less than 2 GiB",
            delivery["size"]
        )
        .into());
    }
    Ok(report)
}
