//! Durable device ownership for task attempts, including cross-device control.
use sqlx::PgPool;
use uuid::Uuid;

pub(crate) async fn recover(db: &PgPool) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query(
        "WITH interrupted AS (
        UPDATE runs SET status='queued',control_epoch=0,
            loop_state=jsonb_set(loop_state,'{resume_requested}','true')
        WHERE status='running' OR (status='queued' AND control_epoch<>0) RETURNING id
    ) UPDATE harness_events e SET status='waiting' FROM interrupted r
        WHERE e.id=r.id AND e.kind='task' AND e.status='running'",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE harness_events SET status='canceled',finished_at=now()
        WHERE kind='model' AND status='running'",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM device_execution_leases")
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

pub(crate) async fn claim(
    db: &PgPool,
    active: &[Uuid],
) -> Result<Option<(Uuid, Uuid)>, sqlx::Error> {
    sqlx::query_as(
        "WITH due AS (
            SELECT r.id,r.device_id FROM runs r
            WHERE r.status='queued' AND NOT(r.id=ANY($1))
            AND (r.next_wake_at IS NULL OR r.next_wake_at<=now())
            AND NOT EXISTS(SELECT 1 FROM device_execution_leases l WHERE l.device_id=r.device_id)
            AND NOT EXISTS(SELECT 1 FROM runs busy WHERE busy.device_id=r.device_id AND busy.status='running')
            ORDER BY CASE WHEN r.origin='user' AND (r.step=0 OR jsonb_array_length(r.steering)>0) THEN 0 WHEN r.origin='notification' THEN 1 WHEN r.origin='user' THEN 2 WHEN r.origin='device-handoff' THEN 3 ELSE 4 END,COALESCE(r.heartbeat_at,r.created_at),r.id
            FOR UPDATE OF r SKIP LOCKED LIMIT 1
        ), claimed AS (
            INSERT INTO device_execution_leases(device_id,run_id)
            SELECT device_id,id FROM due ON CONFLICT DO NOTHING RETURNING run_id
        )
        UPDATE runs r SET status='running',heartbeat_at=now(),next_wake_at=NULL,error=''
        FROM claimed WHERE r.id=claimed.run_id RETURNING r.id,r.assistant_message_id",
    )
    .bind(active)
    .fetch_optional(db)
    .await
}

pub(crate) async fn reserve(db: &PgPool, run: Uuid, device: &str) -> Result<(), String> {
    // Authorization belongs to the device adapter. This atomic claim must finish
    // before contacting the guest; it never holds a transaction during I/O.
    let acquired = sqlx::query(
        "INSERT INTO device_execution_leases(device_id,run_id)
         SELECT $2,id FROM runs WHERE id=$1 AND status='running'
         AND NOT EXISTS(SELECT 1 FROM runs busy WHERE busy.device_id=$2 AND busy.status='running' AND busy.id<>$1)
         ON CONFLICT(device_id) DO UPDATE SET run_id=excluded.run_id
         WHERE device_execution_leases.run_id=excluded.run_id",
    )
    .bind(run)
    .bind(device)
    .execute(db)
    .await
    .map_err(|_| "无法确认设备任务占用，未开始操作".to_string())?
    .rows_affected();
    if acquired == 0 {
        return Err(
            "device_busy:设备正由其他任务使用或当前任务已停止；请先task_wait再重新观察，或用device_message交接".into(),
        );
    }
    Ok(())
}
