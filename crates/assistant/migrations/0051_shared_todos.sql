DROP INDEX runs_one_notification_per_conversation;

ALTER TABLE runs ADD COLUMN copilot boolean NOT NULL DEFAULT false,
                 ADD COLUMN todo_lane smallint,
                 ADD COLUMN todo_position bigint;
UPDATE runs r SET copilot=true WHERE r.status IN ('queued','running','paused')
  AND EXISTS (SELECT 1 FROM chat_voice_inputs v WHERE v.request_id=r.request_id OR EXISTS
    (SELECT 1 FROM run_inputs i WHERE i.run_id=r.id AND i.request_id=v.request_id));

CREATE VIEW harness_todos AS
SELECT id,conversation_id,origin,status,created_at,next_wake_at,
       COALESCE(todo_lane, CASE WHEN copilot THEN 0 WHEN origin IN ('user','device-handoff') THEN 1
                               WHEN origin='notification' THEN 2 ELSE 3 END)::smallint AS lane,
       COALESCE(todo_position,9223372036854775807) AS position
FROM runs WHERE status IN ('queued','running','paused');

CREATE VIEW harness_next_todo AS
SELECT DISTINCT ON (conversation_id) id,conversation_id FROM harness_todos
WHERE origin<>'device-handoff' AND (status='running' OR
     (status='queued' AND (next_wake_at IS NULL OR next_wake_at<=now())))
ORDER BY conversation_id,lane,position,created_at,id;
