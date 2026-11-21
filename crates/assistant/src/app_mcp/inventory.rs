use crate::*;
use futures_util::stream::{self, StreamExt};
use std::time::Duration;

async fn installed(app: &App, device: &str, kind: &str) -> Result<Vec<String>, String> {
    let mut ids: Vec<_> = if kind == "phone" {
        let port = std::env::var("ANDROID_ADB_SERVER_PORT")
            .map_or(Ok(15037), |p| p.parse::<u16>())
            .map_err(|e| e.to_string())?;
        let adb =
            ai_phone_adb::Client::new(port, crate::phone_system::target(app, device).await?.serial)
                .map_err(|e| e.to_string())?;
        let result = adb
            .shell(
                "cmd package list packages -3",
                ai_phone_adb::Limits {
                    stdout: 256 * 1024,
                    stderr: 1024,
                },
                Duration::from_secs(4),
            )
            .await
            .map_err(|e| e.to_string())?;
        if result.code != 0 {
            return Err("Android package manager unavailable".into());
        }
        String::from_utf8(result.stdout)
            .map_err(|e| e.to_string())?
            .lines()
            .filter_map(|s| s.strip_prefix("package:"))
            .filter(|id| !id.starts_with("dev.aiphone.") && crate::application_identity::valid(id))
            .map(str::to_owned)
            .collect()
    } else {
        let value = crate::desktop::helper("applications", serde_json::json!({})).await?;
        let names: Vec<String> = serde_json::from_value(value).map_err(|e| e.to_string())?;
        names
            .iter()
            .filter_map(|n| crate::application_identity::desktop(n))
            .collect()
    };
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

pub(super) async fn record(db: &PgPool, device: &str, ids: &[String]) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    // A first snapshot establishes the baseline. A successful empty list is meaningful too.
    let first=sqlx::query("INSERT INTO app_mcp_inventories(device_id,applications) VALUES($1,$2) ON CONFLICT DO NOTHING")
        .bind(device).bind(ids).execute(&mut *tx).await?;
    if first.rows_affected() == 0 {
        let old: Vec<String> = sqlx::query_scalar(
            "SELECT applications FROM app_mcp_inventories WHERE device_id=$1 FOR UPDATE",
        )
        .bind(device)
        .fetch_one(&mut *tx)
        .await?;
        if ids != old {
            sqlx::query("INSERT INTO app_mcp_jobs(app_id) SELECT name FROM unnest($1::text[]) name WHERE name<>ALL($2::text[]) ON CONFLICT DO NOTHING")
                .bind(ids).bind(&old).execute(&mut *tx).await?;
            sqlx::query("UPDATE app_mcp_inventories SET applications=$2 WHERE device_id=$1")
                .bind(device)
                .bind(ids)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await
}

pub(super) async fn scan(app: &App) -> Result<(), sqlx::Error> {
    let query = format!(
        "SELECT id,kind FROM devices WHERE owner_id='local-user'
        AND runtime_state='managed' AND kind IN ('phone','desktop') AND {}",
        super::IDLE
    );
    let devices: Vec<(String, String)> = sqlx::query_as(&query).fetch_all(&app.db).await?;
    let mut reads = stream::iter(devices)
        .map(|(device, kind)| async move {
            let result = installed(app, &device, &kind).await;
            (device, result)
        })
        .buffer_unordered(2);
    while let Some((device, result)) = reads.next().await {
        if let Ok(ids) = result {
            record(&app.db, &device, &ids).await?
        }
    }
    Ok(())
}
