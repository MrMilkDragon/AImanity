use ai_phone_api::wire::PhoneNotification;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::mpsc,
};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(any(windows, target_os = "macos"))]
struct Banner {
    key: String,
    notice: PhoneNotification,
}

#[cfg(any(windows, target_os = "macos"))]
async fn desktop(send: mpsc::Sender<PhoneNotification>) {
    #[cfg(target_os = "macos")]
    use macos::visible;
    #[cfg(windows)]
    use windows::visible;
    let mut previous = None::<std::collections::HashSet<String>>;
    let mut timer = tokio::time::interval(Duration::from_secs(1));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! { _ = send.closed() => break, _ = timer.tick() => {} }
        let result = tokio::task::spawn_blocking(visible).await;
        let Ok(Ok(banners)) = result else { continue };
        let mut current = std::collections::HashSet::new();
        for mut banner in banners {
            if current.insert(banner.key.clone())
                && previous.as_ref().is_some_and(|p| !p.contains(&banner.key))
            {
                if banner.notice.event_id.is_empty() {
                    banner.notice.event_id = uuid::Uuid::now_v7().to_string();
                }
                banner.notice.posted_at_ms = crate::connection::epoch_ms() as u64;
                if send.send(banner.notice).await.is_err() {
                    return;
                }
            }
        }
        previous = Some(current);
    }
}

pub(super) async fn watch(
    phone: Option<ai_phone_adb::Client>,
    send: mpsc::Sender<PhoneNotification>,
) {
    if let Some(adb) = phone {
        let Ok(stream) = adb
            .open_stream(
                "exec:logcat -b events -v epoch -T 64 -s notification_enqueue:I",
                Duration::from_secs(6),
            )
            .await
        else {
            return;
        };
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Some((header, fields)) = line.split_once("notification_enqueue:") else {
                continue;
            };
            let Some(stamp) = header.split_whitespace().next() else {
                continue;
            };
            let parts: Vec<_> = fields
                .trim()
                .trim_start_matches('[')
                .splitn(7, ',')
                .collect();
            let Some(app) = parts.get(2).map(|s| s.trim()).filter(|s| !s.is_empty()) else {
                continue;
            };
            let notice = PhoneNotification {
                event_id: format!("{stamp}:{}", parts[..parts.len().min(6)].join(",")),
                app_name: app.into(),
                package_name: app.into(),
                ..Default::default()
            };
            if send.send(notice).await.is_err() {
                break;
            }
        }
    } else {
        #[cfg(any(windows, target_os = "macos"))]
        desktop(send).await;
        #[cfg(target_os = "linux")]
        if let Some(home) = std::env::var_os("HOME") {
            aimanity_device_io::notices::watch(
                std::path::PathBuf::from(home).join(".aiphone/notifications.json"),
                send,
            )
            .await;
        }
    }
}
