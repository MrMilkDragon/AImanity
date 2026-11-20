use crate::*;
use sqlx::Row;

pub(crate) async fn displaced(app: &App, run: Uuid) -> Result<bool, String> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM runs r JOIN harness_next_todo n USING(conversation_id)
         WHERE r.id=$1 AND r.origin<>'device-handoff' AND n.id<>r.id)",
    )
    .bind(run)
    .fetch_one(&app.db)
    .await
    .map_err(|e| e.to_string())
}

pub(crate) async fn list(app: &App) -> Result<wire::TodoList, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT t.id,t.status,CASE WHEN r.copilot THEN 'copilot' ELSE t.origin END AS source,
         left(COALESCE(NULLIF(concat_ws(' · ',NULLIF(n.title,''),NULLIF(n.body,'')),''),NULLIF(m.text,''),e.title,''),256) AS title
         FROM harness_todos t JOIN runs r ON r.id=t.id
         JOIN conversations c ON c.id=t.conversation_id AND c.owner_id='local-user'
         LEFT JOIN messages m ON m.request_id=r.request_id AND m.role='user'
         LEFT JOIN LATERAL (SELECT title,body FROM phone_notifications WHERE run_id=t.id
             ORDER BY received_at,event_id LIMIT 1) n ON true
         LEFT JOIN harness_events e ON e.id=t.id AND e.kind='task'
         ORDER BY t.lane,t.position,t.created_at,t.id",
    )
    .fetch_all(&app.db)
    .await?;
    Ok(wire::TodoList {
        items: rows
            .into_iter()
            .map(|row| wire::TodoItem {
                id: row.get::<Uuid, _>("id").to_string(),
                title: row.get("title"),
                source: row.get("source"),
                status: row.get("status"),
            })
            .collect(),
    })
}

pub(crate) async fn move_before(app: &App, input: &wire::TodoMove) -> Result<bool, sqlx::Error> {
    let mut tx = app.db.begin().await?;
    let conversation: Uuid = sqlx::query_scalar(
        "SELECT id FROM conversations WHERE owner_id='local-user' FOR NO KEY UPDATE",
    )
    .fetch_one(&mut *tx)
    .await?;
    let (mut ids, lanes): (Vec<Uuid>, Vec<i16>) = sqlx::query_as::<_, (Uuid, i16)>(
        "SELECT r.id,t.lane FROM harness_todos t JOIN runs r ON r.id=t.id
         WHERE t.conversation_id=$1 ORDER BY t.lane,t.position,t.created_at,t.id FOR UPDATE OF r",
    )
    .bind(conversation)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .unzip();
    let Ok(id) = Uuid::parse_str(&input.id) else {
        return Ok(false);
    };
    let Some(from) = ids.iter().position(|item| *item == id) else {
        return Ok(false);
    };
    if input.id == input.before_id {
        return Ok(true);
    }
    ids.remove(from);
    let to = if input.before_id.is_empty() {
        ids.len()
    } else {
        let Some(before) = Uuid::parse_str(&input.before_id)
            .ok()
            .and_then(|before| ids.iter().position(|item| *item == before))
        else {
            return Ok(false);
        };
        before
    };
    if from == to {
        return Ok(true);
    }
    ids.insert(to, id);
    sqlx::query(
        "UPDATE runs r SET todo_lane=v.lane,todo_position=v.pos
         FROM unnest($1::uuid[],$2::smallint[]) WITH ORDINALITY AS v(id,lane,pos)
         WHERE r.id=v.id",
    )
    .bind(ids)
    .bind(lanes)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    app.run_signals.queue.notify_one();
    app.run_signals.chat_changed(conversation);
    Ok(true)
}
