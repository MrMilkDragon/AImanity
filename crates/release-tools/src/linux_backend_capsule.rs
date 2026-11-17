use crate::{Result, source};
use flate2::read::MultiGzDecoder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, File},
    io::{self, Read},
    path::Path,
};

const PHONE_ROOT: &str = "opt/aiphone/native-phone-bridge";
const PHONE_SCHEMA: &str = "emulator/lib/emulator_controller.proto";
const ROOTFS: &str = "rootfs.tar.gz";
const REQUIRED: &[&str] = &[
    "etc/os-release",
    "etc/wsl.conf",
    "opt/aiphone/bundle/source.payload",
    "opt/aiphone/bundle/images.payload",
    "opt/aiphone/bundle/source.sha256",
    "usr/local/libexec/aiphone-bootstrap",
    "usr/local/bin/docker",
    "usr/local/bin/dockerd",
    "opt/aiphone/redis/bin/redis-server",
    "opt/aiphone/redis/bin/redis-cli",
    "opt/aiphone/redis/lib/ld-musl-x86_64.so.1",
];
const PHONE_FILES: &[&str] = &[
    "platform-tools/adb",
    "platform-tools/source.properties",
    "platform-tools/NOTICE.txt",
    PHONE_SCHEMA,
    "PHONE-TOOLS-MANIFEST.json",
    "provenance.json",
    "LICENSE",
];

struct Measured {
    file: File,
    hash: Sha256,
}
impl Read for Measured {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let count = self.file.read(bytes)?;
        self.hash.update(&bytes[..count]);
        Ok(count)
    }
}

fn inspect(rootfs: &Path) -> Result<(Value, String)> {
    let input = Measured {
        file: File::open(rootfs)?,
        hash: Sha256::new(),
    };
    let mut archive = tar::Archive::new(MultiGzDecoder::new(input));
    let mut names = BTreeSet::new();
    let mut aliases = BTreeSet::new();
    let mut required: BTreeSet<String> = REQUIRED.iter().map(|name| (*name).into()).collect();
    required.extend(
        PHONE_FILES
            .iter()
            .map(|name| format!("{PHONE_ROOT}/{name}")),
    );
    let mut files = BTreeMap::new();
    let mut licences = Vec::new();
    let mut total = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = String::from_utf8(entry.path_bytes().into_owned())?;
        let name = path
            .strip_prefix("./")
            .unwrap_or(&path)
            .trim_end_matches('/');
        if name == "." {
            continue;
        }
        if name.split('/').any(|part| matches!(part, "" | "." | ".."))
            || name.contains('\\')
            || name.chars().any(char::is_control)
            || !names.insert(name.to_owned())
        {
            return Err(format!("Invalid Linux rootfs path: {name}").into());
        }
        let kind = entry.header().entry_type();
        total = total
            .checked_add(entry.size())
            .ok_or("Linux rootfs size overflow")?;
        if names.len() > 100_000
            || total > 16 * 1024 * 1024 * 1024
            || !(kind.is_file() || kind.is_dir() || kind.is_symlink() || kind.is_hard_link())
        {
            return Err("Invalid Linux rootfs inventory".into());
        }
        if !kind.is_dir() {
            let parts: Vec<_> = name.split('/').collect();
            let last = parts.last().copied().unwrap_or("");
            let shell = ((parts[0] == "root" && parts.len() == 2)
                || (parts[0] == "home" && parts.len() == 3))
                && matches!(last, ".bashrc" | ".profile" | ".bash_logout");
            if parts.iter().any(|part| {
                matches!(
                    *part,
                    ".git" | ".ssh" | ".env" | "authorized_keys" | "id_rsa" | "limactl"
                ) || part.starts_with("qemu-system-")
            }) || (parts.contains(&"emulator")
                && name
                    .strip_prefix(PHONE_ROOT)
                    .and_then(|path| path.strip_prefix('/'))
                    != Some(PHONE_SCHEMA))
                || name.starts_with("var/lib/aiphone/")
                || ((name.starts_with("home/") || name.starts_with("root/")) && !shell)
            {
                return Err("Private state or foreign runtime in Linux rootfs".into());
            }
        }
        if kind.is_hard_link() || kind.is_symlink() {
            let path = entry.link_name_bytes().ok_or("Missing Linux rootfs link")?;
            let target = std::str::from_utf8(&path)?;
            if target.is_empty() || target.chars().any(char::is_control) {
                return Err("Invalid Linux rootfs link".into());
            }
            let mut depth = if kind.is_symlink() {
                name.bytes().filter(|byte| *byte == b'/').count()
            } else {
                0
            };
            for segment in target.split('/') {
                match segment {
                    "" | "." => {}
                    ".." => depth = depth.checked_sub(1).ok_or("Linux rootfs link escapes")?,
                    _ => depth += 1,
                }
            }
            aliases.insert(name.to_owned());
        }
        if kind.is_file() && required.remove(name) {
            let mut hash = Sha256::new();
            io::copy(&mut entry, &mut hash)?;
            files.insert(name.to_owned(), format!("{:x}", hash.finalize()));
        }
        if kind.is_file() && name.starts_with("usr/share/doc/") && name.ends_with("/copyright") {
            licences.push(name.to_owned());
        }
    }
    for name in &names {
        if Path::new(name)
            .ancestors()
            .skip(1)
            .any(|parent| aliases.contains(parent.to_str().unwrap_or("")))
        {
            return Err("Linux rootfs writes through an alias".into());
        }
    }
    if !required.is_empty() || licences.is_empty() {
        return Err("Incomplete Linux rootfs or licence inventory".into());
    }
    let mut decoder = archive.into_inner();
    io::copy(&mut decoder, &mut io::sink())?;
    let digest = format!("{:x}", decoder.into_inner().hash.finalize());
    licences.sort_unstable();
    Ok((
        json!({"entries":names.len(),"uncompressed_bytes":total,"required_files":files,"base_license_files":licences}),
        digest,
    ))
}

pub(super) fn seal(args: &[OsString]) -> Result<Value> {
    let [rootfs, output] = args else {
        return Err("seal-linux-backend requires ROOTFS OUTPUT".into());
    };
    let rootfs = Path::new(rootfs);
    let output = std::path::absolute(output)?;
    let provenance: Value = serde_json::from_reader(io::stdin().lock())?;
    let (inventory, digest) = inspect(rootfs)?;
    let size = fs::metadata(rootfs)?.len();
    let value = json!({"format":1,"host":"windows-x64","backend_host":"linux-x64","runtime":"wsl2-backend",
        "rootfs":{"path":ROOTFS,"size":size,"sha256":digest},"inventory":inventory,"provenance":provenance,
        "redistribution":"review-pending","windows_boot_verified":false,"complete_device_bundle":false});
    let mut temporary =
        tempfile::NamedTempFile::new_in(output.parent().ok_or("Missing capsule directory")?)?;
    let mut tar = tar::Builder::new(temporary.as_file_mut());
    source::append_entry(&mut tar, ROOTFS, size, 0o644, File::open(rootfs)?)?;
    let manifest = serde_json::to_vec(&value)?;
    source::append_entry(
        &mut tar,
        "WSL-BACKEND-MANIFEST.json",
        manifest.len() as u64,
        0o644,
        manifest.as_slice(),
    )?;
    tar.into_inner()?;
    temporary.persist_noclobber(output)?;
    Ok(value)
}
