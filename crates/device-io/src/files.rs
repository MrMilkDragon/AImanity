use crate::Result;
use ai_phone_api::wire::{FileEntry, FileListing};
use std::{
    path::{Component, Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};
use tokio::{
    fs,
    io::{AsyncRead, AsyncWrite},
};

#[derive(Clone)]
pub enum Endpoint {
    Directory(PathBuf),
    Desktop,
    Android(ai_phone_adb::Client),
}

pub fn absolute(path: &str) -> Result<&str> {
    if !path.starts_with('/')
        || path.contains(['\0', '\\'])
        || path.split('/').any(|p| p == ".." || p == ".")
    {
        return Err("文件路径无效".into());
    }
    Ok(path)
}
pub fn child(parent: &str, name: &str) -> Result<String> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err("文件名无效".into());
    }
    Ok(format!(
        "{}/{name}",
        absolute(parent)?.trim_end_matches('/')
    ))
}
fn root_path(root: &Path, path: &str) -> Result<PathBuf> {
    let mut result = root.to_owned();
    for component in Path::new(absolute(path)?.trim_start_matches('/')).components() {
        let Component::Normal(name) = component else {
            return Err("文件路径无效".into());
        };
        #[cfg(windows)]
        if name.as_encoded_bytes().contains(&b':') {
            return Err("文件路径无效".into());
        }
        result.push(name);
        if result.is_symlink() {
            return Err("不能通过符号链接执行文件操作".into());
        }
    }
    Ok(result)
}
const TRANSFER: Duration = Duration::from_secs(3600);
pub(super) fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
async fn shell(adb: &ai_phone_adb::Client, command: String) -> Result<()> {
    let result = adb
        .shell(
            &command,
            ai_phone_adb::Limits {
                stdout: 4096,
                stderr: 8192,
            },
            Duration::from_secs(30),
        )
        .await?;
    if result.code != 0 {
        return Err(format!(
            "Android 文件操作失败 ({}) {}",
            result.code,
            String::from_utf8_lossy(&result.stderr).trim()
        )
        .into());
    }
    Ok(())
}
fn rename_new(src: &Path, dst: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};
        renameat_with(CWD, src, CWD, dst, RenameFlags::NOREPLACE)?;
    }
    #[cfg(windows)]
    #[allow(unsafe_code)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
        let wide = |p: &Path| p.as_os_str().encode_wide().chain([0]).collect::<Vec<_>>();
        let src = wide(src);
        let dst = wide(dst);
        if unsafe { MoveFileExW(src.as_ptr(), dst.as_ptr(), 0) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

async fn android_rename(adb: &ai_phone_adb::Client, src: &str, dst: &str) -> Result<()> {
    let from = quoted(src);
    let to = quoted(absolute(dst)?);
    shell(adb, format!("mv -nT -- {from} {to} && test ! -e {from}")).await
}

struct AndroidUpload<'a> {
    adb: &'a ai_phone_adb::Client,
    temporary: Option<String>,
}
impl Drop for AndroidUpload<'_> {
    fn drop(&mut self) {
        if let Some(path) = self.temporary.take() {
            let adb = self.adb.clone();
            tokio::spawn(async move {
                let _ = shell(&adb, format!("rm -f -- {}", quoted(&path))).await;
            });
        }
    }
}

impl Endpoint {
    pub fn local_path(&self, path: &str) -> Result<PathBuf> {
        match self {
            Self::Directory(root) => root_path(root, path),
            #[cfg(not(windows))]
            Self::Desktop => root_path(Path::new("/"), path),
            #[cfg(windows)]
            Self::Desktop => {
                let relative = absolute(path)?.trim_start_matches('/');
                let (volume, rest) = relative.split_once('/').unwrap_or((relative, ""));
                let [letter, b':'] = volume.as_bytes() else {
                    return Err("Windows 驱动器路径无效".into());
                };
                if !letter.is_ascii_alphabetic() {
                    return Err("Windows 驱动器路径无效".into());
                }
                root_path(Path::new(&format!("{volume}\\")), &format!("/{rest}"))
            }
            Self::Android(_) => Err("设备远端路径".into()),
        }
    }
    pub async fn list(&self, path: &str) -> Result<FileListing> {
        let mut entries = Vec::new();
        #[cfg(windows)]
        if matches!(self, Self::Desktop) && absolute(path)?.trim_matches('/').is_empty() {
            let drives = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
            if drives == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            for index in 0..26 {
                if drives & (1 << index) != 0 {
                    let name = format!("{}:", char::from(b'A' + index));
                    entries.push(FileEntry {
                        path: format!("/{name}"),
                        name,
                        directory: true,
                        ..Default::default()
                    });
                }
            }
            return Ok(FileListing {
                entries,
                location: None,
            });
        }
        match self {
            Self::Desktop | Self::Directory(_) => {
                let mut dir = fs::read_dir(self.local_path(path)?).await?;
                while let Some(item) = dir.next_entry().await? {
                    let stat = fs::symlink_metadata(item.path()).await?;
                    let name = item.file_name().to_string_lossy().into_owned();
                    entries.push(FileEntry {
                        path: child(path, &name)?,
                        name,
                        directory: stat.is_dir(),
                        size_bytes: stat.len(),
                        modified_at_ms: stat
                            .modified()?
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                        symlink: stat.is_symlink(),
                    });
                }
            }
            Self::Android(adb) => {
                let path = absolute(path)?;
                for item in adb.list(path, Duration::from_secs(20)).await? {
                    entries.push(FileEntry {
                        path: child(path, &item.name)?,
                        name: item.name,
                        directory: item.mode & 0o170000 == 0o040000,
                        symlink: item.mode & 0o170000 == 0o120000,
                        size_bytes: item.bytes,
                        modified_at_ms: item.modified * 1000,
                    });
                }
            }
        }
        entries.sort_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(FileListing {
            entries,
            location: None,
        })
    }
    pub async fn read(&self, path: &str, out: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
        match self {
            Self::Desktop | Self::Directory(_) => {
                let mut f = fs::File::open(self.local_path(path)?).await?;
                tokio::io::copy(&mut f, out).await?;
            }
            Self::Android(adb) => {
                adb.pull(absolute(path)?, out, TRANSFER).await?;
            }
        }
        Ok(())
    }
    pub async fn write(&self, path: &str, input: &mut (impl AsyncRead + Unpin)) -> Result<()> {
        match self {
            Self::Desktop | Self::Directory(_) => {
                let dst = self.local_path(path)?;
                let staged = tempfile::NamedTempFile::new_in(dst.parent().ok_or("文件名无效")?)?;
                let mut out = fs::File::from_std(staged.reopen()?);
                tokio::io::copy(input, &mut out).await?;
                out.sync_all().await?;
                drop(out);
                staged.persist_noclobber(dst)?;
            }
            Self::Android(adb) => {
                let dst = absolute(path)?;
                let parent = dst.rsplit_once('/').ok_or("文件名无效")?.0;
                let tmp = format!("{parent}/.aimanity-{}", uuid::Uuid::new_v4());
                let mut pending = AndroidUpload {
                    adb,
                    temporary: Some(tmp.clone()),
                };
                adb.push(
                    &tmp,
                    input,
                    std::time::SystemTime::now()
                        .duration_since(UNIX_EPOCH)?
                        .as_secs() as u32,
                    TRANSFER,
                )
                .await?;
                android_rename(adb, &tmp, dst).await?;
                pending.temporary.take();
            }
        }
        Ok(())
    }
    pub async fn change(
        &self,
        op: ai_phone_api::wire::DeviceIoOperation,
        path: &str,
        to: &str,
    ) -> Result<()> {
        use ai_phone_api::wire::DeviceIoOperation as Op;
        if absolute(path)?.trim_matches('/').is_empty() {
            return Err("不能更改根目录".into());
        }
        match self {
            Self::Desktop | Self::Directory(_) => {
                let src = self.local_path(path)?;
                if src.parent().is_none() {
                    return Err("不能更改根目录".into());
                }
                match op {
                    Op::Mkdir => fs::create_dir(src).await?,
                    Op::Remove => {
                        if fs::symlink_metadata(&src).await?.is_dir() {
                            fs::remove_dir_all(src).await?;
                        } else {
                            fs::remove_file(src).await?;
                        }
                    }
                    Op::Rename => rename_new(&src, &self.local_path(to)?)?,
                    _ => return Err("文件操作无效".into()),
                }
            }
            Self::Android(adb) => {
                let p = quoted(path);
                let cmd = match op {
                    Op::Mkdir => format!("mkdir -- {p}"),
                    Op::Remove => format!("rm -r -- {p}"),
                    Op::Rename => return android_rename(adb, path, to).await,
                    _ => return Err("文件操作无效".into()),
                };
                shell(adb, cmd).await?;
            }
        }
        Ok(())
    }
}
