CREATE TABLE app_mcp_jobs (
    app_id text PRIMARY KEY,
    run_id uuid REFERENCES runs(id) ON DELETE SET NULL,
    state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','ready','skipped','failed')),
    eligible_at timestamptz NOT NULL DEFAULT now(),
    attempts smallint NOT NULL DEFAULT 0,
    lease uuid,
    skill_id text REFERENCES runtime_skills(id) ON DELETE SET NULL
);
CREATE INDEX app_mcp_pending ON app_mcp_jobs(eligible_at) WHERE state='pending';
CREATE INDEX app_learning_latest_device ON app_learning_observations(device_id,observed_at DESC,event_id DESC);
CREATE TABLE app_mcp_inventories (
    device_id text PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
    applications text[] NOT NULL
);

CREATE FUNCTION queue_observed_app_tools() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO app_mcp_jobs(app_id,run_id)
        SELECT DISTINCT app_id,NEW.id FROM app_learning_observations WHERE run_id=NEW.id
        ON CONFLICT (app_id) DO UPDATE SET run_id=excluded.run_id,state='pending',lease=NULL,eligible_at=now(),attempts=0
        WHERE app_mcp_jobs.run_id IS NULL AND app_mcp_jobs.state IN ('pending','skipped','failed');
    RETURN NEW;
END
$$;
CREATE TRIGGER queue_observed_app_tools AFTER UPDATE OF status ON runs
    FOR EACH ROW WHEN (NEW.status='succeeded' AND OLD.status IS DISTINCT FROM NEW.status)
    EXECUTE FUNCTION queue_observed_app_tools();
