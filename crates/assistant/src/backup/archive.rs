use super::{BACKUP_FORMAT, DESKTOP_ARCHIVE, Result, file_digest, new_file};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha512};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Component, Path},
};

struct Measured<R> {
    inner: R,
    hash: Sha512,
}
impl<R: Read> Read for Measured<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let size = self.inner.read(bytes)?;
        self.hash.update(&bytes[..size]);
        Ok(size)
    }
}
fn name(path: &Path) -> Result<String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Invalid archive path".into());
    }
    Ok(path.to_str().ok_or("Archive path encoding")?.into())
}
fn regular<W: Write>(
    archive: &mut tar::Builder<W>,
    file: &Path,
    path: &str,
    records: &mut Map<String, Value>,
) -> Result<()> {
    let info = fs::metadata(file)?;
    let mut measured = Measured {
        inner: File::open(file)?,
        hash: Sha512::new(),
    };
    let mut header = tar::Header::new_gnu();
    header.set_size(info.len());
    header.set_mode(info.permissions().mode() & 0o777);
    header.set_cksum();
    archive.append_data(&mut header, path, &mut measured)?;
    records.insert(path.into(),json!({"type":"file","size":info.len(),"allocated_bytes":info.blocks()*512,"sha512":format!("{:x}",measured.hash.finalize())}));
    Ok(())
}
fn omit(name: &str, dependencies: &Value) -> bool {
    let parts: Vec<_> = name.split('/').collect();
    let basename = parts.last().copied().unwrap_or_default();
    name == "deployment/container-host.json"
        || ["backups", "android/backups", "android/downloads"]
            .iter()
            .any(|prefix| {
                (name == *prefix || name.starts_with(&format!("{prefix}/")))
                    && !dependencies.as_object().is_some_and(|deps| {
                        deps.keys().any(|key| {
                            !key.starts_with("sdk:")
                                && (key == name || key.starts_with(&format!("{name}/")))
                        })
                    })
            })
        || name.ends_with(".log")
            && ((parts.len() == 2
                && ["assistant", "channels", "device-stream", "phone-runner"].contains(&parts[0]))
                || name.starts_with("android/logs/"))
        || name.starts_with("android/avd/") && name.ends_with(".lock")
        || basename.starts_with("Singleton")
        || basename == ".DS_Store"
}
fn walk<W: Write>(
    archive: &mut tar::Builder<W>,
    base: &Path,
    folder: &Path,
    dependencies: &Value,
    records: &mut Map<String, Value>,
    excluded: &mut Vec<String>,
) -> Result<()> {
    let mut files = fs::read_dir(folder)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    files.sort();
    for file in files {
        let relative = name(file.strip_prefix(base)?)?;
        if omit(&relative, dependencies) {
            excluded.push(relative);
            continue;
        }
        let info = fs::symlink_metadata(&file)?;
        let path = format!("data/{relative}");
        if info.file_type().is_symlink() {
            let target = fs::read_link(&file)?;
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            header.set_cksum();
            archive.append_link(&mut header, &path, &target)?;
            records.insert(path, json!({"type":"symlink","target":target}));
        } else if info.is_dir() {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            header.set_mode(0o700);
            header.set_cksum();
            archive.append_data(&mut header, &path, io::empty())?;
            walk(archive, base, &file, dependencies, records, excluded)?;
        } else if info.is_file() {
            regular(archive, &file, &path, records)?;
        } else {
            excluded.push(relative);
        }
    }
    Ok(())
}
pub(super) fn write_state(
    root: &Path,
    destination: &Path,
    dependencies: &Value,
) -> Result<(Value, Vec<String>)> {
    let data = root.join("data").canonicalize()?;
    let mut archive =
        tar::Builder::new(GzEncoder::new(new_file(destination)?, Compression::fast()));
    let mut records = Map::new();
    let mut excluded = vec![];
    regular(&mut archive, &root.join(".env"), ".env", &mut records)?;
    walk(
        &mut archive,
        &data,
        &data,
        dependencies,
        &mut records,
        &mut excluded,
    )?;
    archive.into_inner()?.finish()?.sync_all()?;
    Ok((Value::Object(records), excluded))
}
pub(super) fn write_envelope(folder: &Path, destination: &Path) -> Result<()> {
    let mut archive = tar::Builder::new(new_file(destination)?);
    for name in ["manifest.json", "database.dump", "state.tar.gz"] {
        archive.append_path_with_name(folder.join(name), name)?;
    }
    let desktop = folder.join(DESKTOP_ARCHIVE);
    if desktop.try_exists()? {
        archive.append_path_with_name(desktop, DESKTOP_ARCHIVE)?;
    }
    archive.into_inner()?.sync_all()?;
    Ok(())
}

fn read_manifest(reader: impl Read) -> Result<Value> {
    let manifest: Value = serde_json::from_reader(reader)?;
    if manifest["format"] != BACKUP_FORMAT {
        return Err("Unsupported backup format".into());
    }
    let id = manifest["id"].as_str().ok_or("Backup identity missing")?;
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Backup identity invalid".into());
    }
    Ok(manifest)
}

const PAYLOADS: [&str; 3] = ["database.dump", "state.tar.gz", DESKTOP_ARCHIVE];

pub(super) fn read_backup(archive: &Path, work: &Path) -> Result<(std::path::PathBuf, Value)> {
    let (folder, manifest) = if archive.is_dir() {
        (
            archive.to_path_buf(),
            read_manifest(File::open(archive.join("manifest.json"))?)?,
        )
    } else {
        let mut tar = tar::Archive::new(File::open(archive)?);
        let mut entries = tar.entries()?;
        let first = entries
            .next()
            .transpose()?
            .ok_or("Backup manifest missing")?;
        if first.path()?.as_ref() != Path::new("manifest.json")
            || !first.header().entry_type().is_file()
        {
            return Err("Backup envelope must begin with its manifest".into());
        }
        let manifest = read_manifest(first)?;
        let folder = work.join("payload");
        fs::create_dir(&folder)?;
        for entry in entries {
            let mut entry = entry?;
            let path = name(&entry.path()?)?;
            if !PAYLOADS.contains(&path.as_str()) || !entry.header().entry_type().is_file() {
                return Err("Unsafe backup envelope".into());
            }
            io::copy(&mut entry, &mut new_file(&folder.join(path))?)?;
        }
        (folder, manifest)
    };
    let files = manifest["files"]
        .as_array()
        .ok_or("Backup inventory missing")?;
    let mut expected = BTreeSet::new();
    for row in files {
        let path = row["path"].as_str().ok_or("Payload path missing")?;
        if !PAYLOADS.contains(&path) || !expected.insert(path) {
            return Err("Invalid backup inventory".into());
        }
        let file = folder.join(path);
        let info = fs::symlink_metadata(&file)?;
        if !info.is_file()
            || info.len() != row["size"].as_u64().ok_or("Payload size missing")?
            || file_digest(&file)? != row["sha512"]
        {
            return Err("Backup payload changed".into());
        }
    }
    let has_desktop = !manifest["desktop"].is_null();
    if !expected.contains("state.tar.gz")
        || !expected.contains("database.dump")
        || expected.contains(DESKTOP_ARCHIVE) != has_desktop
    {
        return Err("Backup inventory incomplete".into());
    }
    for entry in fs::read_dir(&folder)? {
        let file = entry?.file_name();
        let name = file.to_str().ok_or("Payload name invalid")?;
        if name != "manifest.json" && !expected.remove(name) {
            return Err("Backup envelope inventory differs".into());
        }
    }
    if !expected.is_empty() {
        return Err("Backup files missing".into());
    }
    Ok((folder, manifest))
}
pub(super) fn restore_state(payload: &Path, destination: &Path, manifest: &Value) -> Result<()> {
    let records = manifest["records"]
        .as_object()
        .ok_or("Backup records missing")?;
    let mut seen = BTreeSet::new();
    let mut links = vec![];
    let mut archive = tar::Archive::new(GzDecoder::new(File::open(payload)?));
    let mut bytes = [0u8; 128 * 1024];
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = name(&entry.path()?)?;
        if !seen.insert(path.clone()) || (path != ".env" && !path.starts_with("data/")) {
            return Err("Invalid restored state path".into());
        }
        let file = destination.join(&path);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&file)?;
            fs::set_permissions(file, fs::Permissions::from_mode(0o700))?;
            continue;
        }
        let record = records.get(&path).ok_or("Unlisted restored state")?;
        if kind.is_symlink() {
            let target = entry
                .link_name()?
                .ok_or("Restored link target missing")?
                .to_path_buf();
            if record["type"] != "symlink"
                || record["target"] != target.to_str().ok_or("Invalid restored link")?
            {
                return Err("Restored link differs".into());
            }
            links.push((file, target));
            continue;
        }
        if !kind.is_file() || record["type"] != "file" || record["size"] != entry.size() {
            return Err("Restored file differs".into());
        }
        fs::create_dir_all(file.parent().ok_or("Restored parent missing")?)?;
        let mut output = new_file(&file)?;
        fs::set_permissions(
            &file,
            fs::Permissions::from_mode(entry.header().mode()? & 0o777),
        )?;
        let mut hash = Sha512::new();
        let mut total = 0;
        loop {
            let count = entry.read(&mut bytes)?;
            if count == 0 {
                break;
            }
            hash.update(&bytes[..count]);
            if bytes[..count].iter().all(|byte| *byte == 0) {
                output.seek(SeekFrom::Current(count as i64))?;
            } else {
                output.write_all(&bytes[..count])?;
            }
            total += count as u64;
        }
        output.set_len(total)?;
        if format!("{:x}", hash.finalize()) != record["sha512"] {
            return Err("Restored file checksum differs".into());
        }
    }
    if records.keys().any(|path| !seen.contains(path)) {
        return Err("Restored state missing files".into());
    }
    for (file, target) in links {
        fs::create_dir_all(file.parent().ok_or("Restored link parent missing")?)?;
        symlink(target, file)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_backup_round_trip_and_rejected_input() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        let folder = fixture.path().join("backup");
        let output = fixture.path().join("output");
        fs::create_dir_all(source.join("data")).unwrap();
        fs::create_dir(&folder).unwrap();
        fs::create_dir(&output).unwrap();
        fs::write(source.join(".env"), "fixture").unwrap();
        let database = folder.join("database.dump");
        fs::write(&database, b"database payload").unwrap();
        let desktop = folder.join(DESKTOP_ARCHIVE);
        fs::write(&desktop, b"overlay payload").unwrap();
        let payload = folder.join("state.tar.gz");
        let (records, _) = write_state(&source, &payload, &json!({})).unwrap();
        let current = json!({
            "format": BACKUP_FORMAT,
            "id": uuid::Uuid::nil().simple().to_string(),
            "files": [super::super::receipt(&database).unwrap(), super::super::receipt(&payload).unwrap(), super::super::receipt(&desktop).unwrap()],
            "records": records,
            "desktop": {"rootfs_sha256": "1".repeat(64)},
        });
        assert_eq!(
            current["records"][".env"]["sha512"].as_str().unwrap().len(),
            128
        );
        let manifest_path = folder.join("manifest.json");
        fs::write(&manifest_path, current.to_string()).unwrap();
        let envelope = fixture.path().join("backup.tar");
        write_envelope(&folder, &envelope).unwrap();
        for backup in [&folder, &envelope] {
            let (verified_folder, verified) = read_backup(backup, fixture.path()).unwrap();
            restore_state(&verified_folder.join("state.tar.gz"), &output, &verified).unwrap();
            assert_eq!(fs::read_to_string(output.join(".env")).unwrap(), "fixture");
            assert_eq!(
                fs::read(verified_folder.join(DESKTOP_ARCHIVE)).unwrap(),
                b"overlay payload"
            );
            fs::remove_file(output.join(".env")).unwrap();
        }
        let mut retired = current.clone();
        retired["format"] = json!(2);
        fs::write(&manifest_path, retired.to_string()).unwrap();
        assert_eq!(
            read_backup(&folder, fixture.path())
                .unwrap_err()
                .to_string(),
            "Unsupported backup format"
        );
        let retired_envelope = fixture.path().join("retired.tar");
        write_envelope(&folder, &retired_envelope).unwrap();
        assert!(read_backup(&retired_envelope, &output).is_err());
        assert!(!output.join("payload").exists());
        fs::write(&manifest_path, current.to_string()).unwrap();
        let mut changed = fs::read(&payload).unwrap();
        changed[0] ^= 1;
        fs::write(&payload, changed).unwrap();
        assert!(read_backup(&folder, fixture.path()).is_err());
    }
}
