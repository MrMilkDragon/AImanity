use crate::Result;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub fn work_path(home: &Path, value: &str) -> Result<PathBuf> {
    let root = home.canonicalize()?.join("Files");
    if root.is_symlink() {
        return Err("只允许工作文件目录".into());
    }
    let path = root.join(value).canonicalize()?;
    if !path.starts_with(&root) {
        return Err("只允许工作文件目录".into());
    }
    Ok(path)
}

pub fn list(home: &Path, value: &str) -> Result<Value> {
    let root = home.canonicalize()?.join("Files");
    let path = work_path(home, value)?;
    let mut entries = std::fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let files = entries
        .iter()
        .take(100)
        .map(|entry| file_record(entry, &root))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"files": files}))
}

fn file_record(entry: &std::fs::DirEntry, root: &Path) -> Result<Value> {
    let metadata = std::fs::metadata(entry.path())?;
    let name = entry.file_name();
    let path = entry.path();
    let relative = path.strip_prefix(root)?;
    Ok(json!({
        "name": name.to_str().ok_or("文件名不是有效的UTF-8")?,
        "path": relative.to_str().ok_or("文件路径不是有效的UTF-8")?,
        "directory": metadata.is_dir(),
        "bytes": metadata.len(),
    }))
}

pub(crate) fn applications(home: &Path) -> Result<Value> {
    let mut ids = std::collections::BTreeSet::new();
    for folder in [
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        home.join(".local/share/applications"),
    ] {
        let entries = match std::fs::read_dir(folder) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            other => other?,
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "desktop") {
                continue;
            }
            let contents = std::fs::read_to_string(&path)?;
            let mut fields = contents
                .lines()
                .skip_while(|line| line.trim() != "[Desktop Entry]")
                .skip(1)
                .take_while(|line| !line.starts_with('['));
            if !fields.clone().any(|line| line.trim() == "Type=Application")
                || fields
                    .clone()
                    .any(|line| matches!(line.trim(), "NoDisplay=true" | "Hidden=true"))
            {
                continue;
            }
            let id = fields
                .find_map(|line| {
                    line.strip_prefix("StartupWMClass=")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                })
                .or_else(|| path.file_stem()?.to_str());
            if let Some(id) = id {
                ids.insert(id.to_ascii_lowercase());
            }
        }
    }
    Ok(json!(ids))
}
