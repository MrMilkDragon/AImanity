#[cfg(not(target_os = "android"))]
pub(crate) use rfd::AsyncFileDialog;
#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub(crate) use android::AsyncFileDialog;

pub(crate) async fn download_targets(names: &[String]) -> Result<Vec<Destination>, String> {
    if let [name] = names {
        let handle = AsyncFileDialog::new().set_file_name(name).save_file().await;
        #[cfg(not(target_os = "android"))]
        return Ok(handle.into_iter().map(|f| f.path().to_owned()).collect());
        #[cfg(target_os = "android")]
        return Ok(handle.into_iter().collect());
    }
    #[cfg(not(target_os = "android"))]
    {
        let Some(folder) = AsyncFileDialog::new().pick_folder().await else {
            return Ok(Vec::new());
        };
        Ok(names.iter().map(|n| folder.path().join(n)).collect())
    }
    #[cfg(target_os = "android")]
    {
        let Some(folder) =
            crate::android_os::choose("android.intent.action.OPEN_DOCUMENT_TREE", "", None, false)
                .await?
                .into_iter()
                .next()
        else {
            return Ok(Vec::new());
        };
        names
            .iter()
            .map(|name| crate::android_os::create_child(&folder, name).map(android::Output))
            .collect()
    }
}
#[cfg(not(target_os = "android"))]
type Destination = std::path::PathBuf;
#[cfg(target_os = "android")]
type Destination = android::Output;
pub(crate) async fn download(
    client: &crate::connection::Connection,
    source: &str,
    target: &Destination,
) -> Result<(), String> {
    #[cfg(not(target_os = "android"))]
    return client.download_file(source, target).await;
    #[cfg(target_os = "android")]
    {
        let tmp = tempfile::NamedTempFile::new_in(crate::android_os::cache()?)
            .map_err(|e| e.to_string())?;
        client.download_file(source, tmp.path()).await?;
        let uri = target.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut source = std::fs::File::open(tmp.path()).map_err(|e| e.to_string())?;
            std::io::copy(&mut source, &mut crate::android_os::uri_file(&uri, "rwt")?)
                .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }
}
