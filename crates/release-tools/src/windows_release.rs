use crate::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io,
    path::Path,
    process::Command,
};

const BASE_FILES: [&str; 4] = [
    "guest.qcow2",
    "bzImage",
    "kernel.config",
    "KERNEL-LICENSE.txt",
];
const LAYER_FILES: [&str; 2] = ["release-init.cpio.gz", "release.ext4"];

#[derive(Deserialize)]
struct Policy {
    bundled_manifest_sha256: String,
}

#[derive(Deserialize)]
struct Base {
    guest: Value,
    files: Vec<FileRecord>,
}

#[derive(Deserialize)]
struct FileRecord {
    path: String,
    size: u64,
    sha256: String,
}

fn identity(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    io::copy(&mut file, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

pub(super) fn seal(args: &[OsString]) -> Result<Value> {
    let [baseline, compiled, destination] = args else {
        return Err(
            "windows-release requires the Calcium baseline, compiled context and output".into(),
        );
    };
    let baseline = Path::new(baseline).canonicalize()?;
    let compiled = Path::new(compiled).canonicalize()?;
    let destination = std::path::absolute(destination)?;
    let qemu = baseline.join("host/qemu");
    let manifest_path = qemu.join("manifest.json");
    let policy: Policy = serde_json::from_slice(&fs::read("config/windows-qemu.json")?)?;
    let manifest = fs::read(manifest_path)?;
    if format!("{:x}", Sha256::digest(&manifest)) != policy.bundled_manifest_sha256 {
        return Err("Windows guest must use the released Calcium backing disk".into());
    }
    let base: Base = serde_json::from_slice(&manifest)?;
    let parent = destination
        .parent()
        .ok_or("Windows capsule needs a destination directory")?;
    fs::create_dir_all(parent)?;
    let workspace = tempfile::tempdir_in(parent)?;
    let guest = qemu.join("guest");
    let mut members = Vec::new();
    for name in BASE_FILES {
        let path = guest.join(name);
        let record = base
            .files
            .iter()
            .find(|row| row.path == format!("guest/{name}"))
            .ok_or("Missing Calcium guest record")?;
        if fs::metadata(&path)?.len() != record.size || identity(&path)? != record.sha256 {
            return Err(format!("Calcium guest bytes changed: {name}").into());
        }
        members.push((name, path));
    }
    let commit = Command::new("git").args(["rev-parse", "HEAD"]).output()?;
    if !commit.status.success() {
        return Err("Unable to identify Windows source revision".into());
    }
    let commit = String::from_utf8(commit.stdout)?;
    let mut metadata = base.guest;
    metadata["source_commit"] = json!(commit.trim());
    metadata["source_manifest_sha256"] = json!(identity(Path::new("SOURCE-MANIFEST.json"))?);
    metadata["source_components"] = json!(["native-services", "phone-apps"]);
    metadata["runtime"] = json!("qemu-linux-backend");
    metadata["windows_boot_verified"] = json!(false);
    metadata["release_sha256"] = json!(identity(&compiled.join("source.payload"))?);
    if !compiled.join("redis/bin/redis-server").is_file()
        || !compiled.join("redis/bin/redis-cli").is_file()
    {
        return Err("The Windows release layer requires the offline Redis runtime".into());
    }
    let status = Command::new("bash")
        .arg("scripts/windows-release-layer.sh")
        .arg(&compiled)
        .arg(&baseline)
        .arg(workspace.path())
        .arg(compiled.join("images.payload"))
        .status()?;
    if !status.success() {
        return Err("Windows Linux release layer build failed".into());
    }
    let mut layer = Vec::new();
    for name in LAYER_FILES {
        let path = workspace.path().join(name);
        layer.push(json!({
            "path": name, "size": fs::metadata(&path)?.len(), "sha256": identity(&path)?,
        }));
        members.push((name, path));
    }
    metadata["release_layer"] = json!(layer);
    let output = workspace.path().join("windows-guest.payload");
    let mut archive = tar::Builder::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)?,
    );
    for (name, path) in members {
        archive.append_file(name, &mut File::open(path)?)?;
    }
    let contents = serde_json::to_vec(&metadata)?;
    let mut header = tar::Header::new_ustar();
    header.set_size(contents.len() as u64);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    archive.append_data(
        &mut header,
        "WINDOWS-VM-DISK-MANIFEST.json",
        contents.as_slice(),
    )?;
    archive.into_inner()?.sync_all()?;
    fs::hard_link(output, &destination)?;
    Ok(metadata)
}
