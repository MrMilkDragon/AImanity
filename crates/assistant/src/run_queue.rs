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
            SELECT r.id,r.device_id FROM runs r JOIN harness_todos t ON t.id=r.id
            WHERE r.status='queued' AND NOT(r.id=ANY($1))
            AND (r.next_wake_at IS NULL OR r.next_wake_at<=now())
            AND NOT EXISTS(SELECT 1 FROM device_execution_leases l WHERE l.device_id=r.device_id)
            AND NOT EXISTS(SELECT 1 FROM runs busy WHERE busy.device_id=r.device_id AND busy.status='running')
            AND (r.origin='device-handoff' OR r.id IN (SELECT id FROM harness_next_todo))
            ORDER BY t.lane,t.position,t.created_at,t.id
            FOR UPDATE OF r SKIP LOCKED LIMIT 1
        ), claimed AS (
            INSERT INTO device_execution_leases(device_id,run_id)
            SELECT device_id,id FROM due WHERE device_id IS NOT NULL ON CONFLICT DO NOTHING RETURNING run_id
        ), started AS (
            UPDATE runs r SET status='running',heartbeat_at=now(),next_wake_at=NULL,error=''
            FROM due WHERE r.id=due.id AND (due.device_id IS NULL OR due.id IN (SELECT run_id FROM claimed))
            RETURNING r.id,r.assistant_message_id
        ), recorded AS (
            INSERT INTO harness_events(id,run_id,message_id,kind,title)
            SELECT id,id,assistant_message_id,'task','任务' FROM started
            ON CONFLICT(id) DO UPDATE SET status='running',finished_at=NULL RETURNING id
        )
        SELECT s.id,s.assistant_message_id FROM started s JOIN recorded e USING(id)",
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
