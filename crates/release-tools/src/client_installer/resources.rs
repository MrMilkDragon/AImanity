use super::{Result, execute, file_digest, json_file};
use regex::bytes::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::Path,
    process::Command,
};

pub(super) const HOST: &str = "apps/native/gen/host";
use super::notices::LEGAL;
pub(super) use super::notices::NOTICES;

#[derive(Deserialize)]
struct FileRecord {
    path: String,
    size: u64,
    sha256: String,
}

pub(super) struct Plan {
    pub resources: BTreeMap<String, String>,
    pub integrity: BTreeMap<String, String>,
    pinned: BTreeMap<String, (u64, String)>,
}

impl Plan {
    pub(super) fn read(root: &Path) -> Result<Self> {
        let integrity: BTreeMap<String, String> =
            serde_json::from_value(json_file(root.join(HOST).join("integrity.json"))?)?;
        let mut resources: BTreeMap<_, _> = LEGAL
            .into_iter()
            .map(|(path, name)| (path.into(), format!("legal/{name}")))
            .collect();
        let mut pinned = BTreeMap::new();
        for (name, digest) in &integrity {
            match name.as_str() {
                "source.payload"
                | "android.payload"
                | "images.payload"
                | "speech/ggml-base-q5_1.bin"
                | "qemu/manifest.json"
                | "mihomo"
                | "mihomo.exe" => {}
                _ => return Err(format!("Unknown host resource: {name}").into()),
            }
            let path = format!("{HOST}/{name}");
            let target = format!("host/{name}");
            let size = root.join(&path).metadata()?.len();
            resources.insert(path, target.clone());
            pinned.insert(target, (size, digest.clone()));
        }
        if let Some(expected) = integrity.get("qemu/manifest.json") {
            let manifest = root.join(HOST).join("qemu/manifest.json");
            if file_digest(&manifest)? != *expected {
                return Err("Resource manifest changed".into());
            }
            let files: Vec<FileRecord> =
                serde_json::from_value(json_file(manifest)?["files"].clone())?;
            for row in files {
                if row.path.contains(['\\', ':'])
                    || row
                        .path
                        .split('/')
                        .any(|part| matches!(part, "" | "." | ".."))
                {
                    return Err(format!("Invalid resource path: {}", row.path).into());
                }
                let target = format!("host/qemu/{}", row.path);
                resources.insert(format!("{HOST}/qemu/{}", row.path), target.clone());
                pinned.insert(target, (row.size, row.sha256));
            }
        }
        for required in [
            "source.payload",
            "android.payload",
            "speech/ggml-base-q5_1.bin",
            if cfg!(windows) {
                "qemu/manifest.json"
            } else {
                "images.payload"
            },
        ] {
            if !integrity.contains_key(required) {
                return Err(format!("Missing host resource: {required}").into());
            }
        }
        Ok(Self {
            resources,
            integrity,
            pinned,
        })
    }

    pub(super) fn inspect(
        &self,
        root: &Path,
        bundle: &Path,
        program: &Path,
        resource_root: &Path,
    ) -> Result<Vec<Value>> {
        let vectors: Vectors =
            serde_json::from_value(json_file(root.join("config/public-crypto-vectors.json"))?)?;
        let private = Regex::new(
            r"(?-u)(^|[^A-Za-z0-9_-])(?:sk-[A-Za-z0-9_-]{24,}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|gh[pousr]_[A-Za-z0-9]{30,})",
        )?;
        let build_path = Regex::new(r"/Users/[A-Za-z0-9._-]+/")?;
        let expected: BTreeMap<_, _> = self
            .resources
            .iter()
            .map(|(input, name)| (resource_root.join(name), (input, name)))
            .collect();
        let mut compiled = BTreeSet::from_iter(self.integrity.values().map(String::as_str));
        let binary = fs::read(program)?;
        compiled.retain(|digest| {
            !binary
                .windows(digest.len())
                .any(|value| value == digest.as_bytes())
        });
        if !compiled.is_empty() {
            return Err("Compiled resources differ from prepared payload".into());
        }
        execute(
            Command::new(program)
                .arg("--verify-bundled-host")
                .arg(resource_root.join("host")),
        )?;
        let mut files = Vec::new();
        let mut folders = vec![bundle.to_owned()];
        let mut seen = BTreeSet::new();
        while let Some(folder) = folders.pop() {
            for entry in fs::read_dir(folder)? {
                let entry = entry?;
                let path = entry.path();
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    folders.push(path);
                    continue;
                }
                if !kind.is_file() {
                    return Err(format!("Unexpected installer file: {}", path.display()).into());
                }
                let (size, digest) = if let Some((input, name)) = expected.get(&path) {
                    seen.insert(path.clone());
                    let digest = file_digest(&path)?;
                    let size = path.metadata()?.len();
                    if let Some(pinned) = self.pinned.get(*name) {
                        if pinned.0 != size || pinned.1 != digest {
                            return Err(format!("Stale installer resource: {name}").into());
                        }
                    } else if file_digest(&root.join(input))? != digest {
                        return Err(format!("Stale legal resource: {name}").into());
                    }
                    (size, digest)
                } else {
                    if path.starts_with(resource_root.join("host"))
                        || path.starts_with(resource_root.join("legal"))
                    {
                        return Err(
                            format!("Unexpected installer resource: {}", path.display()).into()
                        );
                    }
                    verify_public(&path, &private, &build_path, &vectors.vectors)?
                };
                files.push(json!({"path":path.strip_prefix(bundle)?.to_string_lossy().replace('\\',"/"),"size":size,"sha256":digest}));
            }
        }
        if seen.len() != expected.len() {
            return Err("Installer resources are incomplete".into());
        }
        files.sort_unstable_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
        Ok(files)
    }
}

#[derive(Deserialize)]
struct Vectors {
    vectors: Vec<Vector>,
}
#[derive(Deserialize)]
struct Vector {
    size: usize,
    sha256: String,
}

fn verify_public(
    path: &Path,
    private: &Regex,
    build_path: &Regex,
    vectors: &[Vector],
) -> Result<(u64, String)> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 132 * 1024];
    let mut tail = 0;
    let mut size = 0;
    loop {
        let length = file.read(&mut buffer[tail..])?;
        let end = tail + length;
        if end == 0 {
            break;
        }
        hash.update(&buffer[tail..end]);
        size += length as u64;
        let test = if length == 0 {
            end
        } else {
            end.saturating_sub(4096)
        };
        let mut masked = buffer[..end].to_vec();
        for prefix in [b"-----BEGIN ".as_slice(), b"sk-"] {
            for (offset, bytes) in buffer[..end].windows(prefix.len()).enumerate() {
                if bytes != prefix {
                    continue;
                }
                for grant in vectors {
                    if let Some(value) = buffer[..end].get(offset..offset + grant.size)
                        && value.last() == Some(&0)
                        && format!("{:x}", Sha256::digest(value)) == grant.sha256
                    {
                        masked[offset..offset + grant.size].fill(b' ');
                        break;
                    }
                }
            }
        }
        let clear: Vec<_> = masked.iter().copied().filter(|byte| *byte != 0).collect();
        let compact = masked[..test].iter().filter(|byte| **byte != 0).count();
        if [(masked.as_slice(), test), (clear.as_slice(), compact)]
            .into_iter()
            .any(|(data, boundary)| private.find(data).is_some_and(|m| m.start() < boundary))
            || build_path.is_match(&buffer[..end])
        {
            return Err(format!("Private material in bundle: {}", path.display()).into());
        }
        if length == 0 {
            break;
        }
        tail = end.min(4096);
        buffer.copy_within(end - tail..end, 0);
    }
    Ok((size, format!("{:x}", hash.finalize())))
}

#[cfg(target_os = "macos")]
pub(super) fn link(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target.parent().ok_or("Resource parent missing")?)?;
    fs::hard_link(source, target)?;
    Ok(())
}

#[cfg(windows)]
pub(super) fn windows_program(folder: &Path) -> Result<std::path::PathBuf> {
    let mut folders = vec![folder.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            if entry.file_name() == "ai-phone-client.exe" && entry.file_type()?.is_file() {
                return Ok(entry.path());
            }
            if entry.file_type()?.is_dir() {
                folders.push(entry.path());
            }
        }
    }
    Err("Windows client executable is missing".into())
}
