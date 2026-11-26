use super::*;
use serde_json::{Value, json};

impl Workspace {
    pub(super) fn workspace_settings(&self, input: Value) -> Result<Value> {
        let mode = self.proactive_state()?;
        let registry = self.registry()?;
        let id = input["id"].as_str();
        let enabled = || input["enabled"].as_bool().ok_or("缺少开关状态");
        let value = match input["operation"].as_str() {
            Some("show") => json!({
                "active_agent_id": registry.active_agent_id,
                "agents": registry.agents.iter().map(|a| json!({"id":a.id,"name":a.name,"model":registry.models.iter().find(|m|m.id==a.model_connection_id).and_then(|m|m.profile.as_ref()).map(|p|p.model.as_str())})).collect::<Vec<_>>(),
                "liberty": mode.exploring, "notifications": mode.notifications_enabled,
                "goal": mode.goal, "skills": self.active_skill_ids()?
            }),
            Some("plans") => {
                let plans: wire::PlanList = self.get("plans")?;
                json!(plans.items.iter().map(|p| json!({"id":p.id,"task":p.task,"trigger":p.trigger,"time":p.time,"date":p.date,"weekdays":p.weekdays,"app":p.app,"contains":p.contains})).collect::<Vec<_>>())
            }
            Some("select_agent") => {
                let agent = registry
                    .agents
                    .iter()
                    .find(|a| Some(a.id.as_str()) == id)
                    .ok_or("请选择已保存的 Agent")?;
                self.dispatch(
                    "registry/agent-select",
                    &wire::AgentProfileRequest {
                        id: agent.id.clone(),
                        expected_revision: agent.revision,
                    }
                    .encode_to_vec(),
                )?;
                json!({"active_agent_id":agent.id, "name":agent.name})
            }
            Some("goal") => {
                let goal = input["goal"].as_str().ok_or("缺少目标内容")?;
                if goal.len() > 12_000 {
                    return Err("目标内容不能超过 12 KB".into());
                }
                let state = self.set_mode(
                    "proactive/goal",
                    &wire::GoalUpdateRequest {
                        goal: goal.into(),
                        expected_revision: mode.revision,
                        ..Default::default()
                    }
                    .encode_to_vec(),
                )?;
                json!({"goal":state.goal,"active":state.goal_active})
            }
            Some(operation @ ("liberty" | "notifications")) => {
                let value = enabled()?;
                let state = self.set_mode(
                    "proactive/update",
                    &wire::ProactiveUpdateRequest {
                        expected_revision: mode.revision,
                        exploring: if operation == "liberty" {
                            value
                        } else {
                            mode.exploring
                        },
                        notifications_enabled: if operation == "notifications" {
                            value
                        } else {
                            mode.notifications_enabled
                        },
                        autonomous_reply: mode.autonomous_reply,
                        autonomous_install: mode.autonomous_install,
                        interval_seconds: mode.interval_seconds,
                        ..Default::default()
                    }
                    .encode_to_vec(),
                )?;
                json!({"liberty":state.exploring,"notifications":state.notifications_enabled})
            }
            Some("skill") => {
                let id = id.ok_or("请选择 Skill")?;
                let enabled = enabled()?;
                self.update_skill(wire::SkillUpdateRequest {
                    id: id.into(),
                    enabled,
                    remove: false,
                })?;
                json!({"id":id,"enabled":enabled})
            }
            _ => return Err("未知设置操作".into()),
        };
        Ok(Value::String(value.to_string()))
    }
}
