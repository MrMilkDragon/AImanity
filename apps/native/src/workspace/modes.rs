use super::{records::revision, *};
use std::time::Duration;

impl Workspace {
    pub(super) fn proactive_state(&self) -> Result<wire::ProactiveState> {
        let mut s: wire::ProactiveState = self.get("proactive")?;
        if s.revision == 0 {
            s.notifications_enabled = true;
            s.autonomous_reply = true;
            s.interval_seconds = 60;
        }
        Ok(s)
    }
    pub(super) fn set_mode(&self, route: &str, bytes: &[u8]) -> Result<wire::ProactiveState> {
        let tx = self.db.begin_write()?;
        let mut state: wire::ProactiveState = read(&tx.open_table(RECORDS)?, "proactive")?;
        if state.revision == 0 {
            state.notifications_enabled = true;
            state.autonomous_reply = true;
            state.interval_seconds = 60;
        }
        let old = state.active_run_id.clone();
        let interrupt = if route == "proactive/goal" {
            let q = wire::GoalUpdateRequest::decode(bytes)?;
            revision(state.revision, q.expected_revision)?;
            state.goal = q.goal.trim().into();
            state.goal_active = !state.goal.is_empty();
            state.goal_device_id.clear();
            state.goal_status.clear();
            if state.goal_active {
                state.exploring = false;
            }
            true
        } else {
            let q = wire::ProactiveUpdateRequest::decode(bytes)?;
            revision(state.revision, q.expected_revision)?;
            if state.goal_active && q.exploring {
                return Err("目标进行中".into());
            }
            let changed = state.exploring != q.exploring;
            state.exploring = q.exploring;
            state.notifications_enabled = q.notifications_enabled;
            state.autonomous_reply = q.autonomous_reply;
            state.forward_notifications = false;
            state.autonomous_install = q.autonomous_install;
            state.interval_seconds = q.interval_seconds.max(1);
            changed
        };
        state.revision += 1;
        if interrupt {
            state.active_run_id.clear();
            state.next_wake_at_ms = 0;
        }
        put(&tx, "proactive", &state)?;
        tx.commit()?;
        if interrupt
            && !old.is_empty()
            && self
                .queued(false)?
                .iter()
                .any(|(_, j)| j.request.as_ref().is_some_and(|q| q.request_id == old))
        {
            self.control(
                wire::ChatRunRequest {
                    run_id: old,
                    ..Default::default()
                },
                false,
            )?;
        }
        self.ready.notify_one();
        Ok(state)
    }
    pub(super) fn mode_wait(&self) -> Option<Duration> {
        let state = self.proactive_state().ok()?;
        if (!state.exploring && !state.goal_active)
            || !state.active_run_id.is_empty()
            || state.next_wake_at_ms == 0
            || self.registry().ok()?.active_agent_id.is_empty()
        {
            return None;
        }
        Some(Duration::from_millis(
            state
                .next_wake_at_ms
                .saturating_sub(crate::connection::epoch_ms() as u64),
        ))
    }
    pub(super) fn schedule_mode(&self) -> Result<()> {
        let s = self.proactive_state()?;
        if !(s.exploring || s.goal_active)
            || !s.active_run_id.is_empty()
            || s.goal_status == "awaiting_user"
            || s.next_wake_at_ms > crate::connection::epoch_ms() as u64
        {
            return Ok(());
        }
        if self.registry()?.active_agent_id.is_empty() {
            return Ok(());
        }
        let (source, text) = if s.goal_active {
            ("goal", s.goal.clone())
        } else {
            ("liberty","Choose your own useful activity in this workspace. Select the available devices or work locally within the workspace. You may finish this session when there is nothing worthwhile to do.".into())
        };
        let id = format!("mode/{source}/{}/{}", s.revision, s.next_wake_at_ms);
        self.submit_job(
            wire::ChatSendRequest {
                request_id: id.clone(),
                text,
                ..Default::default()
            },
            source,
            "",
        )?;
        let tx = self.db.begin_write()?;
        let mut current: wire::ProactiveState = read(&tx.open_table(RECORDS)?, "proactive")?;
        if current.revision == s.revision {
            current.active_run_id = id;
            current.next_wake_at_ms = 0;
            if current.goal_active {
                current.goal_status = "running".into();
            }
            put(&tx, "proactive", &current)?;
        } else {
            drop(tx);
            self.control(
                wire::ChatRunRequest {
                    run_id: id,
                    ..Default::default()
                },
                false,
            )?;
            return Ok(());
        }
        tx.commit()?;
        Ok(())
    }
    pub(super) fn finish_mode(
        &self,
        tx: &WriteTransaction,
        id: &str,
        done: bool,
        error: bool,
        awaiting: bool,
    ) -> Result<()> {
        let mut s: wire::ProactiveState = read(&tx.open_table(RECORDS)?, "proactive")?;
        if s.active_run_id != id {
            return Ok(());
        }
        s.active_run_id.clear();
        if awaiting {
            s.goal_status = "awaiting_user".into();
            s.exploring = false;
            s.next_wake_at_ms = 0;
            return put(tx, "proactive", &s);
        }
        if done {
            s.goal_active = false;
            s.goal_status = "completed".into();
        } else if s.goal_active {
            s.goal_status = if error { "failed" } else { "waiting" }.into();
        }
        s.next_wake_at_ms = (crate::connection::epoch_ms() as u64)
            .saturating_add(u64::from(s.interval_seconds.max(5)) * 1000);
        put(tx, "proactive", &s)?;
        Ok(())
    }
}
