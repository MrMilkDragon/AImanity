use super::{Result, execute, file_digest, json_file};
use crate::{android_component, source};
use serde_json::{Value, json};
use std::{ffi::OsString, fs, path::Path, process::Command};

const PHONE_IMAGES: [&str; 7] = [
    "aimanity-audio.jar",
    "bzImage",
    "initrd.img",
    "disk.qcow2",
    "userdata-16.qcow2",
    "userdata-24.qcow2",
    "userdata-32.qcow2",
];
const DESKTOP_FILES: [&str; 6] = [
    "bzImage",
    "guest.qcow2",
    "release.ext4",
    "release-init.cpio.gz",
    "kernel.config",
    "KERNEL-LICENSE.txt",
];

pub(super) fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    execute(
        Command::new("cp")
            .args(["-a", "--reflink=auto"])
            .arg(source.join("."))
            .arg(destination),
    )
}

pub(crate) fn prepare(args: &[OsString]) -> Result<Value> {
    let [published, tools] = args else {
        return Err("linux-payload WINDOWS_HOST_RESOURCES LINUX_TOOLS".into());
    };
    let root = std::env::current_dir()?;
    let published = Path::new(published);
    let tools = Path::new(tools);
    for file in [
        "bin/qemu-system-x86_64",
        "bin/qemu-img",
        "bin/adb",
        "share/qemu/bios-256k.bin",
    ] {
        if !tools.join(file).is_file() {
            return Err(format!("Missing Linux runtime resource: {file}").into());
        }
    }
    let generated = root.join("apps/native/gen");
    fs::create_dir_all(&generated)?;
    let scratch = tempfile::tempdir_in(&generated)?;
    let host = scratch.path().join("host");
    fs::create_dir_all(host.join("qemu/guest"))?;
    let mut tar = Command::new("tar");
    tar.arg("-xJf")
        .arg(published.join("android.payload"))
        .arg("-C")
        .arg(scratch.path())
        .arg("--");
    tar.args(PHONE_IMAGES.map(|name| format!("sdk/cuttlefish/{name}")));
    execute(&mut tar)?;
    let android = android_component::seal(&[
        scratch.path().join("sdk").into(),
        host.join("android.payload").into(),
        "6".into(),
        "linux-x64".into(),
    ])?;
    let original = json_file(published.join("qemu/manifest.json"))?;
    let inventory = original["files"]
        .as_array()
        .ok_or("Missing offline guest inventory")?;
    let mut files = Vec::new();
    for name in DESKTOP_FILES {
        let relative = format!("guest/{name}");
        let file = published.join("qemu").join(&relative);
        let row = inventory
            .iter()
            .find(|entry| entry["path"] == relative)
            .ok_or("Missing desktop guest file")?;
        if row["size"] != file.metadata()?.len() || row["sha256"] != file_digest(&file)? {
            return Err(format!("Invalid published guest file {relative}").into());
        }
        fs::copy(file, host.join("qemu").join(&relative))?;
        files.push(row);
    }
    let qemu = json!({
        "format":1, "runtime":"qemu-kvm-backend", "release_sha256":original["release_sha256"],
        "guest":original["guest"], "files":files
    });
    let manifest = host.join("qemu/manifest.json");
    fs::write(&manifest, serde_json::to_vec_pretty(&qemu)?)?;
    copy_tree(tools, &host.join("tools"))?;
    let speech = json_file(root.join("config/speech-model.json"))?;
    let model = format!(
        "speech/{}",
        speech["file"].as_str().ok_or("Missing speech model name")?
    );
    fs::create_dir(host.join("speech"))?;
    fs::copy(published.join(&model), host.join(&model))?;
    if file_digest(&host.join(&model))?
        != speech["sha256"]
            .as_str()
            .ok_or("Missing speech model identity")?
    {
        return Err("Invalid packaged speech model".into());
    }
    let apps = published.join("source.payload");
    if source::payload_components(&apps)? != json!(["phone-apps"]) {
        return Err("The shared x86_64 payload must contain the Android guest applications".into());
    }
    fs::copy(&apps, host.join("source.payload"))?;
    let integrity = json!({
        "source.payload": file_digest(&apps)?,
        "android.payload": android["archive_sha256"], "qemu/manifest.json": file_digest(&manifest)?, model: speech["sha256"]
    });
    fs::write(
        host.join("integrity.json"),
        serde_json::to_vec_pretty(&integrity)?,
    )?;
    fs::rename(host, generated.join("host"))?;
    Ok(integrity)
}
