use ai_phone_api::wire::PhoneNotification;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::mpsc,
};

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
        #[cfg(target_os = "linux")]
        if let Some(home) = std::env::var_os("HOME") {
            let path = std::path::PathBuf::from(home).join(".aiphone/notifications.json");
            let mut seen = std::collections::HashSet::<String>::new();
            let mut last = None;
            let mut timer = tokio::time::interval(Duration::from_secs(1));
            loop {
                timer.tick().await;
                let stamp = tokio::fs::metadata(&path)
                    .await
                    .ok()
                    .and_then(|m| m.modified().ok());
                if stamp == last {
                    continue;
                }
                let Ok(bytes) = tokio::fs::read(&path).await else {
                    continue;
                };
                let Ok(events) = serde_json::from_slice::<Vec<serde_json::Value>>(&bytes) else {
                    continue;
                };
                let mut current = std::collections::HashSet::new();
                for event in events {
                    let text = |key: &str| event[key].as_str().unwrap_or("").to_owned();
                    let id = text("event_id");
                    if id.is_empty() {
                        continue;
                    }
                    if !seen.contains(&id) {
                        let msg = PhoneNotification {
                            event_id: id.clone(),
                            app_name: text("app_name"),
                            title: text("title"),
                            text: text("text"),
                            posted_at_ms: event["posted_at_ms"].as_u64().unwrap_or_default(),
                            ..Default::default()
                        };
                        if send.send(msg).await.is_err() {
                            return;
                        }
                    }
                    current.insert(id);
                }
                seen = current;
                last = stamp;
            }
        }
    }
}
