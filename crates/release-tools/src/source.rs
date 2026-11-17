use crate::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
};

const DIRECTORIES: &[&str] = &[
    "apps",
    "crates",
    "packages",
    "scripts",
    "infra",
    "config",
    "skills",
    "docs/public",
    ".github",
];
const ROOT_FILES: &[&str] = &[
    ".cargo/config.toml",
    "ai-phone",
    "README.md",
    "README.en.md",
    "Cargo.toml",
    "Cargo.lock",
    "package.json",
    "bun.lock",
    "buf.yaml",
    "compose.yaml",
    "compose.dev.yaml",
    "compose.desktop.yaml",
    "compose.kafka.yaml",
    ".env.example",
    ".gitignore",
    ".dockerignore",
    ".gitattributes",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "DISCLAIMER.md",
    "PRIVACY.md",
    "THIRD_PARTY_NOTICES.md",
    "CHANGELOG.md",
    "LICENSE",
    "NOTICE",
];
const OMITTED: &[&str] = &[
    ".git",
    ".next",
    "node_modules",
    "target",
    "data",
    ".tools",
    "evidence",
    "dist",
    ".AppleDouble",
    "__MACOSX",
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
];
const PRIVATE_NAMES: &[&str] = &[
    "agent.md",
    "agents.md",
    "agents.override.md",
    "claude.md",
    "gemini.md",
    "copilot-instructions.md",
    ".cursorrules",
    "contributor-agents.md",
];
const PRIVATE_DIRECTORIES: &[&str] = &[".agents", ".claude", ".codex"];
const OMITTED_EXTENSIONS: &[&str] = &[
    "payload",
    "log",
    "dump",
    "apk",
    "jks",
    "keystore",
    "tsbuildinfo",
    "qcow2",
    "img",
    "tar",
    "gz",
    "tgz",
    "zip",
    "xz",
    "zst",
    "sqlite",
    "db",
    "key",
];
const REQUIRED: &[&str] = &[
    "README.md",
    "README.en.md",
    "SECURITY.md",
    "DISCLAIMER.md",
    "PRIVACY.md",
    "THIRD_PARTY_NOTICES.md",
    ".github/workflows/source.yml",
];

pub(super) struct Entry {
    path: String,
    data: Vec<u8>,
    mode: u32,
    sha256: String,
}

impl Entry {
    pub(super) fn new(path: String, data: Vec<u8>, mode: u32) -> Self {
        let sha256 = format!("{:x}", Sha256::digest(&data));
        Self {
            path,
            data,
            mode,
            sha256,
        }
    }

    pub(super) fn inventory(&self) -> Value {
        json!({"path":self.path,"size":self.data.len(),"sha256":self.sha256,"mode":self.mode})
    }

    pub(super) fn matches_digest(&self, digest: &str) -> bool {
        self.sha256 == digest
    }
}

#[derive(Serialize)]
struct Record<'a> {
    path: &'a str,
    size: usize,
    sha256: &'a str,
}

#[derive(Deserialize)]
struct FileRecord {
    path: String,
    size: usize,
    sha256: String,
    mode: u32,
}

pub(super) fn component(folder: &Path, prefix: &str, manifest: &Value) -> Result<Vec<Entry>> {
    let files: Vec<FileRecord> = serde_json::from_value(manifest["files"].clone())?;
    let owner = format!("{prefix}/");
    let mut entries = Vec::with_capacity(files.len() + 1);
    for row in files {
        let relative = row.path.strip_prefix(&owner).unwrap_or(&row.path);
        if relative.contains(['\\', ':'])
            || relative
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            || !matches!(row.mode, 0o644 | 0o755)
        {
            return Err(format!("Invalid component entry: {}", row.path).into());
        }
        let file = folder.join(relative);
        let metadata = file.symlink_metadata()?;
        if !metadata.is_file() || metadata.len() != row.size as u64 {
            return Err(format!("Component file changed: {}", file.display()).into());
        }
        let entry = Entry::new(format!("{owner}{relative}"), fs::read(&file)?, row.mode);
        if !entry.matches_digest(&row.sha256) {
            return Err(format!("Component file changed: {}", file.display()).into());
        }
        entries.push(entry);
    }
    entries.push(Entry::new(
        format!("{owner}manifest.json"),
        fs::read(folder.join("manifest.json"))?,
        0o644,
    ));
    Ok(entries)
}

fn omitted(path: &str, name: &str) -> bool {
    OMITTED.contains(&name)
        || PRIVATE_NAMES.contains(&name.to_ascii_lowercase().as_str())
        || PRIVATE_DIRECTORIES.contains(&name.to_ascii_lowercase().as_str())
        || path == "apps/native/gen"
        || name.starts_with("._")
}

pub(super) fn files(root: &Path, directory: &str, result: &mut Vec<String>) -> Result<()> {
    let path = root.join(directory);
    if !path.try_exists()? {
        return Ok(());
    }
    if fs::symlink_metadata(&path)?.is_symlink() {
        return Err(format!("Source alias: {directory}").into());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Invalid source path")?;
        let relative = format!("{directory}/{name}");
        if omitted(&relative, &name) {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(format!("Source alias: {relative}").into());
        }
        if kind.is_dir() {
            files(root, &relative, result)?;
        } else {
            result.push(relative);
        }
    }
    Ok(())
}

fn credential(pattern: &Regex, source: &str) -> bool {
    if !["sk-", "-----BEGIN ", "ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
        .iter()
        .any(|prefix| source.contains(prefix))
    {
        return false;
    }
    pattern.find_iter(source).any(|hit| {
        source
            .as_bytes()
            .get(hit.start().wrapping_sub(1))
            .is_none_or(|byte| !byte.is_ascii_alphanumeric() && !b"_-".contains(byte))
    })
}

fn scan(root: &Path) -> Result<Vec<Entry>> {
    let mut names = Vec::new();
    for name in ROOT_FILES {
        if root.join(name).try_exists()? {
            names.push((*name).to_owned());
        }
    }
    for directory in DIRECTORIES {
        files(root, directory, &mut names)?;
    }
    names.sort_unstable_by(|a, b| a.split('/').cmp(b.split('/')));
    names.dedup();
    let secret = Regex::new(
        r"sk-[A-Za-z0-9_-]{24,}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|gh[pousr]_[A-Za-z0-9]{30,}",
    )?;
    names
        .into_iter()
        .filter_map(|path| {
            let filename = path.rsplit('/').next().unwrap();
            let extension = Path::new(filename)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            let private_env = filename.starts_with(".env") && path != ".env.example";
            let verify_script = path.starts_with("scripts/verify-") && path.ends_with(".ts");
            (!(omitted(&path, filename)
                || OMITTED_EXTENSIONS.contains(&extension)
                || private_env
                || verify_script))
                .then_some(path)
        })
        .map(|path| {
            let full = root.join(&path);
            let info = fs::symlink_metadata(&full)?;
            if !info.is_file() || info.len() > 16 * 1024 * 1024 {
                return Err(format!("Invalid source: {path}").into());
            }
            let data = fs::read(full)?;
            if credential(&secret, &String::from_utf8_lossy(&data)) {
                return Err(format!("Credential in source: {path}").into());
            }
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                if info.permissions().mode() & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                }
            };
            #[cfg(not(unix))]
            let mode = 0o644;
            Ok(Entry::new(path, data, mode))
        })
        .collect()
}

fn records(entries: &[Entry]) -> Vec<Record<'_>> {
    entries
        .iter()
        .map(|entry| Record {
            path: &entry.path,
            size: entry.data.len(),
            sha256: &entry.sha256,
        })
        .collect()
}

fn read_json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn verify(root: &Path, entries: &[Entry], records: &[Record<'_>], draft: bool) -> Result<()> {
    let indexed: BTreeMap<_, _> = records.iter().map(|r| (r.path, r.sha256)).collect();
    let provenance = root.join("config/source-assets.json");
    if provenance.try_exists()? {
        let value = read_json(&provenance)?;
        for asset in value["assets"]
            .as_array()
            .ok_or("Invalid source asset list")?
        {
            if let Some(rows) = asset["sha256"].as_object() {
                for (name, expected) in rows {
                    if indexed.get(name.as_str()).copied() != expected.as_str()
                        || !indexed.contains_key(name.as_str())
                    {
                        return Err(format!("Source asset provenance mismatch: {name}").into());
                    }
                }
            }
            if let Some(rows) = asset["references"].as_array() {
                for item in rows {
                    let name = item["license_file"]
                        .as_str()
                        .ok_or("Invalid source asset reference")?;
                    if indexed
                        .get(name)
                        .is_none_or(|value| Some(*value) != item["license_sha256"].as_str())
                    {
                        return Err(format!("Source asset provenance mismatch: {name}").into());
                    }
                }
            }
        }
    }
    for file in [
        "native-notice-supplements.json",
        "rust-runtime-notices.json",
    ] {
        let path = root.join("config").join(file);
        if path.try_exists()? {
            let value = read_json(&path)?;
            for package in value["packages"]
                .as_array()
                .ok_or("Invalid notice statements")?
            {
                for item in package["files"].as_array().ok_or("Invalid notice files")? {
                    let name = item["file"].as_str().ok_or("Invalid notice path")?;
                    if indexed
                        .get(name)
                        .is_none_or(|value| Some(*value) != item["sha256"].as_str())
                    {
                        return Err(format!("Upstream notice mismatch: {name}").into());
                    }
                }
            }
        }
    }
    for name in REQUIRED
        .iter()
        .copied()
        .chain((!draft).then_some("LICENSE"))
    {
        if !indexed.contains_key(name) {
            return Err(format!("Required source missing: {name}").into());
        }
    }
    let shebang = Regex::new(r"^#![^\r\n]*\b(?:python|pypy)(?:[\d.]+)?\b")?;
    let origin = Regex::new(
        r"/Users/[A-Za-z0-9._-]+/|https?://(?:172\.(?:1[6-9]|2[0-9]|3[01])\.|192\.168\.)",
    )?;
    for entry in entries {
        let lower = entry.path.to_ascii_lowercase();
        let source = String::from_utf8_lossy(&entry.data);
        if [".py", ".pyc", ".pyi", ".pyo", ".pyw"]
            .iter()
            .any(|ext| lower.ends_with(ext))
            || (source.starts_with("#!") && shebang.is_match(&source))
        {
            return Err(format!("Python source is not allowed: {}", entry.path).into());
        }
        if ["/Users/", "://172.", "://192.168."]
            .iter()
            .any(|marker| source.contains(marker))
            && origin.is_match(&source)
        {
            return Err(format!("Private origin in source: {}", entry.path).into());
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct Manifest<'a> {
    format: u32,
    version: Value,
    license: Value,
    remote_publication_authorized: bool,
    remote_review_authorized: bool,
    kind: &'static str,
    files: &'a [Record<'a>],
}

fn manifest<'a>(root: &Path, records: &'a [Record<'a>], draft: bool) -> Result<Manifest<'a>> {
    let policy = read_json(&root.join("config/release-policy.json"))?;
    let preview = &policy["source_preview"];
    let review = preview["remote_review_authorized"] == true;
    let kind = match (draft, review) {
        (true, true) => "private-source-review-draft",
        (true, false) => "local-review-draft",
        (false, _) => "source-preview",
    };
    Ok(Manifest {
        format: 1,
        version: policy["version"].clone(),
        license: preview["license"].clone(),
        remote_publication_authorized: !draft && preview["remote_publication_authorized"] == true,
        remote_review_authorized: review,
        kind,
        files: records,
    })
}

fn regular_manifest(path: &Path) -> Result<()> {
    if path.try_exists()? && !fs::symlink_metadata(path)?.is_file() {
        return Err("Source manifest must be a regular file".into());
    }
    Ok(())
}

fn manifest_bytes(value: &Manifest<'_>) -> Result<Vec<u8>> {
    let mut data = serde_json::to_vec_pretty(value)?;
    data.push(b'\n');
    Ok(data)
}

fn git(root: &Path, args: &[OsString]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(["--no-optional-locks", "-c", "core.fsmonitor=false"])
        .args(args)
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "Git source command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output.stdout)
}

fn flag(args: &[OsString], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn check(
    root: &Path,
    entries: &[Entry],
    records: &[Record<'_>],
    args: &[OsString],
) -> Result<Value> {
    verify(root, entries, records, false)?;
    let repository = flag(args, "--repository");
    if repository {
        let indexed = git(root, &["ls-files".into(), "-z".into()])?;
        for name in String::from_utf8(indexed)?
            .split('\0')
            .filter(|name| !name.is_empty())
        {
            if name != "SOURCE-MANIFEST.json"
                && records
                    .binary_search_by(|row| row.path.split('/').cmp(name.split('/')))
                    .is_err()
            {
                return Err(format!("Outside source allowlist: {name}").into());
            }
        }
    }
    let path = root.join("SOURCE-MANIFEST.json");
    if flag(args, "--manifest") || (repository && path.try_exists()?) {
        regular_manifest(&path)?;
        let actual = read_json(&path)?;
        if actual["format"] != 1 || actual["files"] != serde_json::to_value(records)? {
            return Err("Source manifest mismatch".into());
        }
    }
    Ok(
        json!({"files":entries.len(),"private_state_included":false,"license_present":true,"source_scan_passed":true}),
    )
}

fn append_file<W: Write>(
    archive: &mut tar::Builder<W>,
    path: impl AsRef<Path>,
    data: &[u8],
    mode: u32,
) -> Result<()> {
    append_entry(archive, path, data.len() as u64, mode, data)
}

pub(super) fn append_entry(
    archive: &mut tar::Builder<impl Write>,
    path: impl AsRef<Path>,
    size: u64,
    mode: u32,
    contents: impl Read,
) -> Result<()> {
    let mut header = tar::Header::new_ustar();
    header.set_mode(mode);
    header.set_size(size);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_entry_type(tar::EntryType::Regular);
    archive.append_data(&mut header, path, contents)?;
    Ok(())
}

const HOST_MANIFEST: &str = "HOST-MANIFEST.json";
struct Hashed<T> {
    stream: T,
    digest: Sha256,
}

impl<W: Write> Write for Hashed<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = self.stream.write(bytes)?;
        self.digest.update(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

pub(super) fn payload(
    root: &Path,
    folder: &Path,
    runtime: Vec<Entry>,
    components: Vec<&str>,
) -> Result<String> {
    let mut entries = scan(root)?;
    entries.extend(runtime);
    let mut temporary = tempfile::NamedTempFile::new_in(folder)?;
    let mut encoded = Hashed {
        stream: temporary.as_file_mut(),
        digest: Sha256::new(),
    };
    crate::capsule::write_encoded(&mut encoded, "amd64", 6, folder, |writer| {
        let mut output = tar::Builder::new(writer);
        for entry in &entries {
            append_file(&mut output, &entry.path, &entry.data, entry.mode)?;
        }
        let files: Vec<_> = entries.iter().map(Entry::inventory).collect();
        let manifest = json!({"format": if components.is_empty() { 1 } else { 2 }, "components":components, "files":files});
        append_file(
            &mut output,
            HOST_MANIFEST,
            &serde_json::to_vec(&manifest)?,
            0o644,
        )?;
        output.finish()?;
        Ok(())
    })?;
    let digest = encoded.digest.finalize();
    temporary.persist(folder.join("source.payload"))?;
    Ok(format!("{digest:x}"))
}

fn payload_components(path: &Path) -> Result<Value> {
    let mut archive = tar::Archive::new(xz2::read::XzDecoder::new(File::open(path)?));
    for entry in archive.entries()? {
        let entry = entry?;
        if entry.path_bytes() == HOST_MANIFEST.as_bytes() {
            let mut manifest: Value = serde_json::from_reader(entry)?;
            return Ok(manifest["components"].take());
        }
    }
    Err("Missing source payload manifest".into())
}

fn package(
    destination: &Path,
    root: &Path,
    entries: &[Entry],
    records: &[Record<'_>],
) -> Result<PathBuf> {
    verify(root, entries, records, false)?;
    let manifest = manifest(root, records, false)?;
    let prefix = format!("ai-phone-{}", manifest.version.as_str().unwrap_or("null"));
    fs::create_dir_all(destination)?;
    let archive = destination.join(format!("{prefix}-source.tar.gz"));
    let index = manifest_bytes(&manifest)?;
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
        File::create(&archive)?,
        flate2::Compression::default(),
    ));
    for entry in entries {
        append_file(
            &mut tar,
            Path::new(&prefix).join(&entry.path),
            &entry.data,
            entry.mode,
        )?;
    }
    append_file(
        &mut tar,
        Path::new(&prefix).join("RELEASE-MANIFEST.json"),
        &index,
        0o644,
    )?;
    tar.into_inner()?.finish()?;
    fs::write(
        destination.join(format!("{prefix}-source.tar.gz.manifest.json")),
        index,
    )?;
    Ok(archive)
}

fn export(
    root: &Path,
    destination: &Path,
    entries: &[Entry],
    records: &[Record<'_>],
    args: &[OsString],
) -> Result<Value> {
    let destination =
        std::path::absolute(destination)?
            .components()
            .fold(PathBuf::new(), |mut path, part| {
                match part {
                    Component::ParentDir => {
                        path.pop();
                    }
                    _ => path.push(part),
                }
                path
            });
    if destination.try_exists()? {
        return Err("Source export already exists".into());
    }
    if let Ok(relative) = destination.strip_prefix(root)
        && !relative
            .parent()
            .is_some_and(|path| path.starts_with("dist") || path.starts_with("data/backups"))
    {
        return Err("Export outside source or into dist".into());
    }
    let draft = flag(args, "--draft");
    let initialize = flag(args, "--git");
    verify(root, entries, records, draft)?;
    let value = manifest(root, records, draft)?;
    let parent = destination
        .parent()
        .ok_or("Source destination requires a parent")?;
    fs::create_dir_all(parent)?;
    let temporary = tempfile::tempdir_in(parent)?;
    let work = temporary.path().join("source");
    fs::create_dir(&work)?;
    for entry in entries {
        let path = work.join(&entry.path);
        fs::create_dir_all(path.parent().unwrap())?;
        let mut file = fs::OpenOptions::new();
        file.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            file.mode(entry.mode);
        }
        file.open(path)?.write_all(&entry.data)?;
    }
    fs::write(work.join("SOURCE-MANIFEST.json"), manifest_bytes(&value)?)?;
    let marker = if value.remote_review_authorized {
        "REVIEW-ONLY.txt"
    } else {
        "LOCAL-REVIEW-ONLY.txt"
    };
    if draft {
        fs::write(
            work.join(marker),
            "Review candidate. Not a product release. See SOURCE-MANIFEST.json.\n",
        )?;
    }
    if initialize {
        git(&work, &["init".into(), "--initial-branch=master".into()])?;
        let mut add = vec!["add".into(), "--".into()];
        add.extend(entries.iter().map(|entry| OsString::from(&entry.path)));
        add.push("SOURCE-MANIFEST.json".into());
        add.extend(draft.then(|| OsString::from(marker)));
        git(&work, &add)?;
        git(
            &work,
            &[
                "commit".into(),
                "-m".into(),
                "Initial AI Phone source snapshot".into(),
            ],
        )?;
    }
    fs::rename(work, &destination)?;
    Ok(
        json!({"destination":destination,"files":entries.len(),"git_initialized":initialize,"remote_created":false,"draft":draft}),
    )
}

pub(super) fn run(args: &[OsString]) -> Result<Option<Value>> {
    let root = std::env::current_dir()?;
    let [operation, rest @ ..] = args else {
        return Err("Expected source check|manifest|package|export|list|components".into());
    };
    if let [path] = rest
        && operation == "components"
    {
        return payload_components(Path::new(path)).map(Some);
    }
    let entries = scan(&root)?;
    let records = records(&entries);
    if operation == "list" {
        return Ok(Some(Value::Array(entries.iter().zip(&records).map(|(entry, record)| {
            json!({"path":record.path,"mode":entry.mode,"sha256":record.sha256})
        }).collect())));
    }
    let result = match operation.to_str() {
        Some("check") => check(&root, &entries, &records, rest)?,
        Some("manifest") => {
            let path = root.join("SOURCE-MANIFEST.json");
            regular_manifest(&path)?;
            fs::write(path, manifest_bytes(&manifest(&root, &records, false)?)?)?;
            json!({"source_files":records.len()})
        }
        Some("package" | "export") => {
            let [destination, flags @ ..] = rest else {
                return Err("Source destination required".into());
            };
            let destination = Path::new(destination);
            if operation == "package" {
                println!(
                    "{}",
                    package(destination, &root, &entries, &records)?.display()
                );
                return Ok(None);
            }
            export(&root, destination, &entries, &records, flags)?
        }
        _ => {
            return Err("Expected source check|manifest|package|export|list|components".into());
        }
    };
    Ok(Some(result))
}
