#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::run;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
pub use linux::run;

#[derive(Clone, Debug)]
pub struct Plan {
    program: std::path::PathBuf,
}

pub async fn prepare(locale: crate::locale::Locale) -> Result<Option<Plan>, String> {
    #[cfg(target_os = "linux")]
    use linux::plan;
    #[cfg(target_os = "macos")]
    use macos::plan;
    #[cfg(windows)]
    use windows::plan;
    let plan = tokio::task::spawn_blocking(plan)
        .await
        .map_err(|e| e.to_string())??;
    let response = rfd::AsyncMessageDialog::new()
        .set_title(locale.tr("卸载 AImanity"))
        .set_description(locale.tr("两台虚拟设备、聊天、工作文件和设置将永久删除。"))
        .set_level(rfd::MessageLevel::Error)
        .set_buttons(rfd::MessageButtons::OkCancelCustom(
            locale.tr("取消").into(),
            locale.tr("卸载 AImanity").into(),
        ))
        .show()
        .await;
    Ok(
        matches!(response, rfd::MessageDialogResult::Custom(choice) if choice == locale.tr("卸载 AImanity"))
            .then_some(plan),
    )
}
