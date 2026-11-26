use super::*;
use chrono::{Datelike, Local, NaiveTime, Timelike};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Draft {
    task: String,
    trigger: String,
    #[serde(default)]
    time: String,
    #[serde(default)]
    weekdays: Vec<u32>,
    #[serde(default)]
    date: String,
    #[serde(default)]
    app: String,
    #[serde(default)]
    contains: String,
}

impl Workspace {
    pub(super) async fn plan_request(&self, operation: &str, body: &[u8]) -> Result<Vec<u8>> {
        match operation {
            "list" => Ok(self.get::<wire::PlanList>("plans")?.encode_to_vec()),
            "interpret" => {
                let prompt = wire::PlanPrompt::decode(body)?;
                if prompt.text.trim().is_empty() || prompt.text.len() > 12_000 {
                    return Err("请输入有效的计划".into());
                }
                let registry = self.registry()?;
                let active = registry
                    .agents
                    .iter()
                    .find(|a| a.id == registry.active_agent_id)
                    .ok_or("请先配置模型")?;
                let config =
                    Self::model(self.get(&format!("model/{}", active.model_connection_id))?)?;
                let client = self.client(&config.proxy_url)?;
                let body = json!({"input":[
                    {"role":"system","content":format!("Convert the user's rule to JSON only. Local date and time: {}. Keys: task (instructions), trigger (clock or notification), time (24-hour HH:MM local), date (YYYY-MM-DD, for one-time schedules, empty for repeating), weekdays (ISO Monday=1 through Sunday=7 for weekly schedules, empty for daily), app (notification app name), contains (title or body substring). Optional values may be omitted. Keep task in the user's language. If the trigger cannot be expressed as a local clock time or notification, set trigger to unsupported. No tools.",Local::now().format("%Y-%m-%d %H:%M"))},
                    {"role":"user","content":prompt.text}
                ],"reasoning":{"effort":"none"}});
                let mut timing = aimanity_model::model_timing::Timing::default();
                let response = aimanity_model::request_tracked(
                    &config,
                    &client,
                    body,
                    &self.shutdown,
                    &mut timing,
                )
                .await?;
                let text = aimanity_model::text(&response);
                let text = text.trim();
                let text = text
                    .strip_prefix("```json")
                    .and_then(|s| s.strip_suffix("```"))
                    .unwrap_or(text);
                let draft: Draft =
                    serde_json::from_str(text.trim()).map_err(|_| "模型未返回可保存的计划")?;
                let plan = wire::Plan {
                    task: draft.task,
                    trigger: draft.trigger,
                    time: draft.time,
                    weekdays: draft.weekdays,
                    date: draft.date,
                    app: draft.app,
                    contains: draft.contains,
                    ..Default::default()
                };
                validate(&plan)?;
                Ok(plan.encode_to_vec())
            }
            "save" | "remove" => {
                let tx = self.db.begin_write()?;
                let mut list: wire::PlanList = read(&tx.open_table(RECORDS)?, "plans")?;
                if operation == "save" {
                    let mut plan = wire::Plan::decode(body)?;
                    validate(&plan)?;
                    plan.id = uuid::Uuid::now_v7().to_string();
                    list.items.push(plan);
                } else {
                    let q = wire::PlanRemove::decode(body)?;
                    list.items.retain(|p| p.id != q.id);
                }
                put(&tx, "plans", &list)?;
                tx.commit()?;
                self.ready.notify_one();
                Ok(list.encode_to_vec())
            }
            _ => Err("未知计划操作".into()),
        }
    }

    pub(super) fn tick_plans(&self) -> Result<()> {
        let now = Local::now();
        let clock = format!("{:02}:{:02}", now.hour(), now.minute());
        let day = now.weekday().number_from_monday();
        for plan in self.get::<wire::PlanList>("plans")?.items {
            if plan.trigger == "clock"
                && plan.time == clock
                && (plan.date.is_empty() || plan.date == now.date_naive().to_string())
                && (plan.weekdays.is_empty() || plan.weekdays.contains(&day))
            {
                self.submit_job(
                    wire::ChatSendRequest {
                        request_id: format!("plan/{}/{}", plan.id, now.date_naive()),
                        text: plan.task,
                        ..Default::default()
                    },
                    "plan",
                    "",
                )?;
            }
        }
        Ok(())
    }
}

fn validate(plan: &wire::Plan) -> Result<()> {
    if plan.task.trim().is_empty() || plan.task.len() > 12_000 {
        return Err("计划缺少任务内容".into());
    }
    match plan.trigger.as_str() {
        "clock"
            if plan.time.len() == 5
                && NaiveTime::parse_from_str(&plan.time, "%H:%M").is_ok()
                && plan.weekdays.iter().all(|d| (1..=7).contains(d))
                && (plan.date.is_empty()
                    || (plan.weekdays.is_empty()
                        && chrono::NaiveDate::parse_from_str(&plan.date, "%Y-%m-%d").is_ok())) =>
        {
            Ok(())
        }
        "notification" if !plan.app.trim().is_empty() || !plan.contains.trim().is_empty() => Ok(()),
        _ => Err("请选择具体的时间或设备通知条件".into()),
    }
}
