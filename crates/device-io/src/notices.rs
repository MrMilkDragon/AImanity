use ai_phone_api::wire::PhoneNotification;
use std::{path::PathBuf, time::Duration};
use tokio::sync::mpsc;
pub async fn watch(path: PathBuf, send: mpsc::Sender<PhoneNotification>) {
    let mut seen = std::collections::HashSet::<String>::new();
    let mut last = None;
    let mut timer = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! { _=send.closed()=>return, _=timer.tick()=>{} }
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
                    package_name: text("package_name"),
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
