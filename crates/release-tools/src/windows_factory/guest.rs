use super::{Record, read_json, stage_runtime};
use crate::Result;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read},
    path::Path,
    process::Command,
};

const MANIFEST: &str = "WINDOWS-VM-DISK-MANIFEST.json";
const MEMBERS: [(&str, &str); 6] = [
    ("bzImage", "/kernel"),
    ("guest.qcow2", "/disk"),
    ("kernel.config", "/kernel_source/config"),
    ("KERNEL-LICENSE.txt", "/kernel_source/license"),
    ("release-init.cpio.gz", ""),
    ("release.ext4", ""),
];

pub(super) fn identity(path: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    io::copy(&mut File::open(path)?, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

pub(super) fn source(root: &Path) -> Result<Value> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()?;
    let commit = std::str::from_utf8(&output.stdout)?.trim();
    if !output.status.success()
        || commit.len() != 40
        || !commit.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Windows factory requires an exact source commit".into());
    }
    Ok(
        json!({"source_commit":commit,"source_manifest_sha256":identity(&root.join("SOURCE-MANIFEST.json"))?}),
    )
}

struct Reader<R> {
    input: R,
    hash: Sha256,
}

impl<R: Read> Read for Reader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let size = self.input.read(bytes)?;
        self.hash.update(&bytes[..size]);
        Ok(size)
    }
}

fn extract(input: &Path, expected: &str, target: &Path) -> Result<(Value, Vec<Record>)> {
    let mut archive = tar::Archive::new(Reader {
        input: File::open(input)?,
        hash: Sha256::new(),
    });
    let mut records = BTreeMap::new();
    let mut manifest = None;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let name = std::str::from_utf8(&entry.path_bytes())?.to_owned();
        let size = entry.size();
        if !entry.header().entry_type().is_file() || size == 0 {
            return Err("Invalid Windows guest capsule member".into());
        }
        if name == MANIFEST && manifest.is_none() && size <= 16384 {
            manifest = Some(serde_json::from_reader::<_, Value>(entry)?);
            continue;
        }
        if !MEMBERS.iter().any(|(file, _)| name == *file) || records.contains_key(&name) {
            return Err(format!("Unexpected Windows guest capsule member: {name}").into());
        }
        let path = format!("guest/{name}");
        let mut output = File::create_new(target.join(&path))?;
        let mut reader = Reader {
            input: &mut entry,
            hash: Sha256::new(),
        };
        if io::copy(&mut reader, &mut output)? != size {
            return Err(format!("Truncated Windows guest capsule member: {name}").into());
        }
        records.insert(
            name,
            Record {
                path,
                size,
                sha256: format!("{:x}", reader.hash.finalize()),
            },
        );
    }
    let mut reader = archive.into_inner();
    io::copy(&mut reader, &mut io::sink())?;
    let actual = format!("{:x}", reader.hash.finalize());
    let mut manifest = manifest.ok_or("Missing Windows guest capsule manifest")?;
    let mut components: Vec<String> =
        serde_json::from_value(manifest["source_components"].clone())?;
    components.sort_unstable();
    let release = manifest["release_sha256"].as_str().unwrap_or("");
    if actual != expected
        || manifest["format"] != 1
        || manifest["host"] != "windows-x64"
        || manifest["runtime"] != "qemu-linux-backend"
        || components != ["native-services", "phone-apps"]
        || release.len() != 64
        || !release.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid Windows guest capsule".into());
    }
    let layer: Vec<Record> = serde_json::from_value(manifest["release_layer"].take())?;
    for (name, key) in MEMBERS {
        let record = records
            .get(name)
            .ok_or("Missing Windows guest capsule member")?;
        let candidate = if key.is_empty() {
            json!(layer.iter().find(|row| row.path == name))
        } else {
            manifest.pointer(key).cloned().unwrap_or(Value::Null)
        };
        if candidate["path"] != name
            || candidate["sha256"] != record.sha256
            || (!candidate["size"].is_null() && candidate["size"] != record.size)
        {
            return Err(format!("Windows guest capsule changed: {name}").into());
        }
    }
    manifest["release_layer"] = json!(layer);
    Ok((manifest, records.into_values().collect()))
}

pub(super) fn stage(
    root: &Path,
    destination: &Path,
    qemu: &Path,
    qemu_sha: &str,
    guest: &Path,
    guest_sha: &str,
) -> Result<String> {
    let parent = destination
        .parent()
        .ok_or("Missing Windows runtime directory")?;
    fs::create_dir_all(parent)?;
    let temporary = tempfile::tempdir_in(parent)?;
    let staged = temporary.path().join("qemu");
    fs::create_dir_all(staged.join("guest"))?;
    let qemu_path = qemu.join("windows-qemu.json");
    if identity(&qemu_path)? != qemu_sha {
        return Err("Windows QEMU factory manifest changed".into());
    }
    let qemu_manifest = read_json(&qemu_path)?;
    let mut records = stage_runtime(qemu, &qemu_manifest, &staged)?;
    let (guest, files) = extract(guest, guest_sha, &staged)?;
    records.extend(files);
    records.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    let mut manifest = source(root)?;
    if guest["source_commit"] != manifest["source_commit"]
        || guest["source_manifest_sha256"] != manifest["source_manifest_sha256"]
    {
        return Err("Windows guest and installer source differ".into());
    }
    manifest
        .as_object_mut()
        .expect("source receipt is an object")
        .extend(
            json!({
                "format":1,"runtime":"qemu-whpx-backend","files":records,
                "qemu_factory_manifest_sha256":qemu_sha,"guest_capsule_sha256":guest_sha,
                "release_sha256":guest["release_sha256"],"qemu":qemu_manifest,"guest":guest,
                "windows_boot_verified":false,"redistribution_review_complete":false,
            })
            .as_object()
            .expect("runtime receipt is an object")
            .clone(),
        );
    let path = staged.join("manifest.json");
    let data = serde_json::to_vec_pretty(&manifest)?;
    fs::write(&path, &data)?;
    if destination.try_exists()? {
        fs::remove_dir_all(destination)?;
    }
    fs::rename(staged, destination)?;
    Ok(format!("{:x}", Sha256::digest(&data)))
}
