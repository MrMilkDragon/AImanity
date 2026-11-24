use crate::{Result, client_payload, image_factory::execute, native_services, phone_apps, source};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    process::Command,
};

pub(super) mod assets;

const KERNEL_FILES: [&str; 3] = ["bzImage", "kernel.config", "KERNEL-LICENSE.txt"];
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

pub(super) fn prepare(args: &[OsString]) -> Result<Value> {
    let (destination, linux, assets) = match args {
        [destination, rootfs_flag, source, assets_flag, assets]
            if rootfs_flag == "--linux-rootfs" && assets_flag == "--guest-assets" =>
        {
            (destination, source, Path::new(assets))
        }
        _ => {
            return Err(
                "windows-release-context OUTPUT --linux-rootfs FILE --guest-assets DIR".into(),
            );
        }
    };
    let root = std::env::current_dir()?.canonicalize()?;
    if !cfg!(all(target_os = "linux", target_arch = "x86_64"))
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var("AI_PHONE_DISPOSABLE_QUALIFICATION").as_deref() != Ok("1")
        || Path::new(&std::env::var_os("GITHUB_WORKSPACE").ok_or("GitHub workspace missing")?)
            .canonicalize()?
            != root
    {
        return Err("Windows release context requires its disposable Linux x64 workspace".into());
    }
    let destination = std::path::absolute(destination)?;
    let parent = destination
        .parent()
        .ok_or("Windows release output parent missing")?;
    if destination.try_exists()? || fs2::available_space(parent)? < 4 * 1024_u64.pow(3) {
        return Err("Windows release needs a new directory and 4 GiB free".into());
    }
    source::run(&["check".into(), "--repository".into(), "--manifest".into()])?;
    let pending = tempfile::tempdir_in(parent)?;
    let work = pending.path();
    phone_apps::run(&["build".into(), root.clone().into_os_string()])?;
    native_services::run(&["build".into()])?;
    let image = work.join("images.payload");
    let sha256 = client_payload::copy_payload(Some(PathBuf::from(linux)), &image)?
        .ok_or("Linux rootfs missing")?;
    let rootfs = json!({"sha256":sha256,"size_bytes":image.metadata()?.len()});
    let mut runtime = native_services::package(Some(&assets.join("ocr")), Some(rootfs))?;
    runtime.extend(phone_apps::component(&root)?);
    source::payload(&root, work, runtime, vec!["native-services", "phone-apps"])?;
    fs::copy(
        root.join("target/release/host-resources"),
        work.join("bootstrap.program"),
    )?;
    execute(
        Command::new("cp")
            .arg("-a")
            .arg(assets.join("linux"))
            .arg(work),
    )?;
    fs::copy(assets.join("guest.qcow2"), work.join("guest.qcow2"))?;
    fs::rename(work, &destination)?;
    Ok(json!({"directory":destination}))
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
            "windows-release requires kernel resources, compiled context and output".into(),
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
        return Err("Guest kernel inventory differs from its pinned source".into());
    }
    let base: Base = serde_json::from_slice(&manifest)?;
    let parent = destination
        .parent()
        .ok_or("Windows capsule needs a destination directory")?;
    fs::create_dir_all(parent)?;
    let workspace = tempfile::tempdir_in(parent)?;
    let guest = qemu.join("guest");
    let mut members = Vec::new();
    for name in KERNEL_FILES {
        let path = guest.join(name);
        let record = base
            .files
            .iter()
            .find(|row| row.path == format!("guest/{name}"))
            .ok_or("Missing kernel record")?;
        if fs::metadata(&path)?.len() != record.size || identity(&path)? != record.sha256 {
            return Err(format!("Pinned kernel changed: {name}").into());
        }
        members.push((name, path));
    }
    let commit = Command::new("git").args(["rev-parse", "HEAD"]).output()?;
    if !commit.status.success() {
        return Err("Unable to identify Windows source revision".into());
    }
    let commit = String::from_utf8(commit.stdout)?;
    let disk = compiled.join("guest.qcow2");
    let mut metadata = json!({"format":1,"host":"windows-x64","kernel":base.guest["kernel"],"kernel_source":base.guest["kernel_source"],"base_distribution":"Debian trixie","disk":{"path":"guest.qcow2","sha256":identity(&disk)?,"size":fs::metadata(&disk)?.len()}});
    members.push(("guest.qcow2", disk));
    metadata["source_commit"] = json!(commit.trim());
    metadata["source_manifest_sha256"] = json!(identity(Path::new("SOURCE-MANIFEST.json"))?);
    metadata["source_components"] = json!(["native-services", "phone-apps"]);
    metadata["runtime"] = json!("qemu-linux-backend");
    metadata["windows_boot_verified"] = json!(false);
    let release = identity(&compiled.join("source.payload"))?;
    metadata["release_sha256"] = json!(release);
    let status = Command::new("bash")
        .arg("scripts/windows-release-layer.sh")
        .arg(&compiled)
        .arg(&baseline)
        .arg(workspace.path())
        .arg(release)
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
    archive.sparse(false);
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
