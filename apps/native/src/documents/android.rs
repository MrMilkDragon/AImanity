use crate::android_os;
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(crate) struct AsyncFileDialog {
    mime: &'static str,
    name: String,
}
pub(crate) struct Output(pub(crate) String);
pub(crate) struct Input(PathBuf);
impl Input {
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Output {
    pub async fn write(&self, bytes: &[u8]) -> io::Result<()> {
        use tokio::io::AsyncWriteExt;
        let file = android_os::uri_file(&self.0, "rwt").map_err(io::Error::other)?;
        let mut file = tokio::fs::File::from_std(file);
        file.write_all(bytes).await?;
        file.flush().await
    }
}
impl AsyncFileDialog {
    pub fn new() -> Self {
        Self {
            mime: "*/*",
            name: String::new(),
        }
    }
    pub fn set_title(self, _: &str) -> Self {
        self
    }
    pub fn set_file_name(mut self, name: impl AsRef<str>) -> Self {
        self.name = name.as_ref().to_owned();
        self
    }
    pub fn add_filter(mut self, _: &str, ext: &[&str]) -> Self {
        self.mime = if ext.contains(&"png") {
            "image/*"
        } else {
            "text/*"
        };
        self
    }
    pub async fn save_file(self) -> Option<Output> {
        android_os::choose(
            "android.intent.action.CREATE_DOCUMENT",
            "application/octet-stream",
            Some(&self.name),
            false,
        )
        .await
        .ok()?
        .into_iter()
        .next()
        .map(Output)
    }
    pub async fn pick_file(self) -> Option<Input> {
        self.pick(false).await?.into_iter().next()
    }
    pub async fn pick_files(self) -> Option<Vec<Input>> {
        self.pick(true).await
    }
    async fn pick(self, multi: bool) -> Option<Vec<Input>> {
        let uris = android_os::choose(
            "android.intent.action.OPEN_DOCUMENT",
            self.mime,
            None,
            multi,
        )
        .await
        .ok()?;
        tokio::task::spawn_blocking(move || -> android_os::Result<Vec<Input>> {
            let cache = android_os::cache()?
                .join("imports")
                .join(uuid::Uuid::now_v7().to_string());
            std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
            uris.iter()
                .enumerate()
                .map(|(i, uri)| {
                    let name = android_os::file_name(uri)?;
                    let name = Path::new(&name)
                        .file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("attachment"));
                    let folder = cache.join(i.to_string());
                    std::fs::create_dir(&folder).map_err(|e| e.to_string())?;
                    let path = folder.join(name);
                    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
                    io::copy(&mut android_os::uri_file(uri, "r")?, &mut file)
                        .map_err(|e| e.to_string())?;
                    Ok(Input(path))
                })
                .collect()
        })
        .await
        .ok()?
        .ok()
    }
}
