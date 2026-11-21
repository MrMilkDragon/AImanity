UPDATE phone_notifications SET notification_key='screen:change' WHERE notification_key='desktop-screen:title';
CREATE INDEX notifications_screen_recent ON phone_notifications(device_id,posted_at) WHERE notification_key='screen:change';
ALTER TABLE proactive_state ADD COLUMN next_knowledge_review_at timestamptz NOT NULL DEFAULT now(),
                            ADD COLUMN knowledge_review_cursor uuid;
ALTER TABLE runs DROP CONSTRAINT runs_origin_check;
ALTER TABLE runs ADD CONSTRAINT runs_origin_check CHECK(origin IN ('user','exploration','notification','device-handoff','knowledge-review'));
