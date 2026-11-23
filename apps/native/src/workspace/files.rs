use super::*;
use std::path::{Component, PathBuf};

impl Workspace {
    pub(crate) fn upload(&self, name: String, bytes: Vec<u8>) -> Result<wire::ChatAttachment> {
        use std::io::Write;
        let file = Path::new(&name);
        if file.file_name().and_then(|name| name.to_str()) != Some(name.as_str()) {
            return Err("文件名无效".into());
        }
        let mut staged = tempfile::NamedTempFile::new_in(&self.files)?;
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        let destination = self.files.join(file);
        let destination = match staged.persist_noclobber(&destination) {
            Ok(_) => destination,
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                let stem = file.file_stem().unwrap_or_default().to_string_lossy();
                let mut unique = self
                    .files
                    .join(format!("{}-{}", stem, uuid::Uuid::now_v7()));
                if let Some(ext) = file.extension() {
                    unique.set_extension(ext);
                }
                e.file.persist_noclobber(&unique)?;
                unique
            }
            Err(e) => return Err(e.into()),
        };
        let result = self.store_attachment(name, &bytes);
        if result.is_err() {
            fs::remove_file(destination)?;
        }
        result
    }

    fn local_path(&self, input: &str) -> Result<PathBuf> {
        let mut path = self.files.clone();
        for part in Path::new(input.trim_start_matches('/')).components() {
            match part {
                Component::Normal(name) => path.push(name),
                Component::CurDir => continue,
                _ => return Err("文件须位于工作目录中".into()),
            }
            if path.is_symlink() {
                return Err("工作目录中的符号链接不参与文件操作".into());
            }
        }
        Ok(path)
    }
    pub(crate) async fn import_file(
        &self,
        src: &Path,
        directory: &str,
    ) -> Result<wire::FileLocation> {
        let file = src
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("文件名无效")?;
        let dest = self.local_path(directory)?.join(file);
        let mut from = tokio::fs::File::open(src).await?;
        let tmp = tempfile::NamedTempFile::new_in(dest.parent().ok_or("目录不存在")?)?;
        let mut writer = tokio::fs::File::from_std(tmp.reopen()?);
        tokio::io::copy(&mut from, &mut writer).await?;
        writer.sync_all().await?;
        drop(writer);
        tmp.persist_noclobber(dest)?;
        Ok(wire::FileLocation {
            path: format!("{}/{file}", directory.trim_end_matches('/')),
            ..Default::default()
        })
    }
    pub(crate) async fn export_file(&self, source: &str, target: &Path) -> Result<()> {
        let src = self.local_path(source)?;
        let temp = tempfile::NamedTempFile::new_in(target.parent().ok_or("目录不存在")?)?;
        tokio::fs::copy(src, temp.path()).await?;
        temp.persist(target)?;
        Ok(())
    }
    pub(super) fn listing(&self, q: wire::FileLocation) -> Result<wire::FileListing> {
        if !q.device_id.is_empty() {
            return Err("设备文件执行器尚未连接".into());
        }
        let path = self.local_path(&q.path)?;
        let mut entries = Vec::new();
        for item in fs::read_dir(path)? {
            let file = item?;
            let stat = fs::symlink_metadata(file.path())?;
            let name = file.file_name().to_string_lossy().into_owned();
            entries.push(wire::FileEntry {
                path: format!("{}/{name}", q.path.trim_end_matches('/')),
                name,
                directory: stat.is_dir(),
                size_bytes: stat.len(),
                modified_at_ms: stat
                    .modified()?
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis() as u64,
                symlink: stat.is_symlink(),
            });
        }
        entries.sort_by(|a, b| b.directory.cmp(&a.directory).then(a.name.cmp(&b.name)));
        Ok(wire::FileListing {
            location: Some(q),
            entries,
        })
    }
}
