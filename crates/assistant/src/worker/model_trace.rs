//! One model attempt owns one durable timing record, including steering/cancellation.
use crate::*;
use serde_json::Value;

const PERSISTENCE_UNAVAILABLE: &str =
    "paused:模型调用记录暂时无法保存，已停止后续执行，等待数据库恢复";

#[derive(Debug)]
pub enum Admission {
    Started(Uuid),
    Waiting(std::time::Duration),
    Superseded,
}

pub async fn start(app: &App, run: Uuid, message: Uuid) -> Result<Admission, String> {
    let id = Uuid::now_v7();
    let (remaining, steered, event): (f64, bool, Option<Uuid>) = sqlx::query_as(
        "WITH decision AS (
            SELECT COALESCE(GREATEST(0,input_quiet_ms-extract(epoch FROM clock_timestamp()-input_at)*1000),0)::double precision remaining,
                   jsonb_array_length(steering)>0 steered
            FROM runs WHERE id=$2
        ), admitted AS (
            INSERT INTO harness_events(id,run_id,message_id,kind,title)
            SELECT $1,$2,$3,'model','模型响应' FROM decision
            WHERE remaining=0 AND NOT steered RETURNING id
        ), stale AS (
            UPDATE harness_events SET status='canceled',finished_at=now()
            WHERE run_id=$2 AND kind='model' AND status='running'
            AND EXISTS(SELECT 1 FROM admitted)
        ) SELECT remaining,steered,(SELECT id FROM admitted) FROM decision",
    )
    .bind(id)
    .bind(run)
    .bind(message)
    .fetch_one(&app.db)
    .await
    .map_err(|_| PERSISTENCE_UNAVAILABLE.to_string())?;
    Ok(if steered {
        Admission::Superseded
    } else if let Some(event) = event {
        Admission::Started(event)
    } else {
        Admission::Waiting(std::time::Duration::from_millis(remaining.ceil() as u64))
    })
}

pub async fn finish(app: &App, id: Uuid, status: &str, timing: Value) -> Result<(), String> {
    sqlx::query("UPDATE harness_events SET status=$2,result=$3,finished_at=now() WHERE id=$1 AND kind='model'")
        .bind(id).bind(status).bind(timing).execute(&app.db).await.map_err(|_| PERSISTENCE_UNAVAILABLE.to_string())?;
    Ok(())
}
