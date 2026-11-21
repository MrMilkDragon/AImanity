use serde::{Deserialize, Serialize};
use serde_json::Value;
use xxhash_rust::xxh3::Xxh3;

pub const SLICE_TURNS: i32 = 8;
pub const ARCHIVED_OBSERVATION: &str = "AI_PHONE_VALIDATED_OBSERVATION_ARCHIVED:";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolOutcome {
    Completed,
    Failed,
    Blocked,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ExplorationWindow {
    first_step: i32,
    started_at_ms: u64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LoopState {
    pub resume_requested: bool,
    pub stalls: u32,
    pub observation_recoveries: u32,
    pub model_retries: u32,
    pub model_replay_boundary: usize,
    pub recent_results: Vec<String>,
    pub last_issue: String,
    pub plan: Option<Value>,
    pub exploration_window: Option<ExplorationWindow>,
    pub guidance_app: String,
}
impl LoopState {
    pub fn exploration_yield_due(&mut self, now: u64, step: i32) -> bool {
        let window = self.exploration_window.get_or_insert(ExplorationWindow {
            first_step: step,
            started_at_ms: now,
        });
        if step.saturating_sub(window.first_step) < 40
            && now.saturating_sub(window.started_at_ms) < 600_000
        {
            return false;
        }
        // Start the next window on wake, not before its potentially long wait.
        self.exploration_window = None;
        true
    }
    pub fn no_progress(&mut self, reason: &str) -> bool {
        self.stalls = self.stalls.saturating_add(1);
        self.last_issue = reason.chars().take(1000).collect();
        self.stalls >= 3
    }
    pub fn tool_result(
        &mut self,
        name: &str,
        arguments: &str,
        result: &Value,
        metadata: Option<&Value>,
        outcome: ToolOutcome,
    ) -> bool {
        if outcome == ToolOutcome::Blocked {
            return false;
        }
        if outcome == ToolOutcome::Failed {
            return self.no_progress(&format!(
                "工具{name}失败：{}",
                result.to_string().chars().take(500).collect::<String>()
            ));
        }
        if name == "task_wait" {
            return false;
        }
        if name == "task_history" {
            return self.no_progress("已核对执行记录，但尚未新增目标执行结果");
        }
        if name == "task_plan" {
            return if self.plan.is_some() {
                self.no_progress("重复调整计划，但没有新增执行结果")
            } else {
                false
            };
        }
        let mut fingerprint = Xxh3::new();
        let mut feed = |value: &str| {
            fingerprint.update(value.as_bytes());
            fingerprint.update(b"\n");
        };
        feed(name);
        if name == "host_desktop"
            && let Some(receipt) = metadata.filter(|v| v["kind"] == "observe")
            && let Some(frame) = result
                .as_array()
                .and_then(|parts| parts.iter().find_map(|part| part["image_url"].as_str()))
        {
            feed(receipt["host_session"].as_str().unwrap_or(""));
            feed(receipt["device_id"].as_str().unwrap_or(""));
            feed(frame);
        } else {
            feed(arguments);
            if let Some(receipt) = metadata.filter(|_| crate::app_learning::observation_tool(name))
                && let (Some(device), Some(observation)) = (
                    receipt["device_id"].as_str(),
                    receipt["observation_id"].as_str(),
                )
            {
                feed(device);
                feed(observation);
            } else {
                feed(&result.to_string());
            }
        }
        let signature = format!("{:032x}", fingerprint.digest128());
        if self.recent_results.contains(&signature) {
            self.no_progress("重复取得相同工具结果，没有观察到新进展")
        } else {
            self.stalls = 0;
            self.last_issue.clear();
            self.recent_results.push(signature);
            if self.recent_results.len() > 24 {
                self.recent_results.remove(0);
            }
            false
        }
    }
}

pub fn slice_due(start_step: i32, step: i32) -> bool {
    step.saturating_sub(start_step) >= SLICE_TURNS
}
pub fn model_retry_delay(error: &str, attempt: u32) -> Option<u64> {
    let transient = [
        "HTTP 429",
        "HTTP 500",
        "HTTP 502",
        "HTTP 503",
        "HTTP 504",
        "网络连接失败",
        "模型流中断",
        "模型未返回完整结果",
        "模型未完成本轮回复",
    ]
    .iter()
    .any(|text| error.contains(text));
    if !transient || attempt > 8 {
        return None;
    }
    Some((2000 * 2_u64.pow(attempt.saturating_sub(1).min(5))).min(60000))
}
pub fn wake_at(now_ms: u64, seconds: u64) -> Result<u64, String> {
    if !(1..=300).contains(&seconds) {
        return Err("等待时间必须为1–300秒".into());
    }
    Ok(now_ms.saturating_add(seconds * 1000))
}
pub fn compact_observations(context: &mut [Value]) {
    fn images(parts: &[Value]) -> impl Iterator<Item = &Value> {
        parts.iter().filter(|part| part["type"] == "input_image")
    }

    let mut keep = std::collections::HashMap::<String, Vec<&[Value]>>::new();
    let mut archive = Vec::new();
    for (index, item) in context.iter().enumerate().rev() {
        if item["type"] != "function_call_output" {
            continue;
        }
        let Some(parts) = item["output"].as_array() else {
            continue;
        };
        if images(parts).next().is_none() {
            continue;
        }
        let device = parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .find_map(|text| {
                let data: Value = serde_json::from_str(text).ok()?;
                data["observation_id"].as_str()?;
                let frames = if data["host_session"].is_string() {
                    1
                } else {
                    2
                };
                Some((data["device_id"].as_str()?.to_owned(), frames))
            })
            .or_else(|| {
                parts
                    .iter()
                    .any(|p| {
                        p["text"]
                            .as_str()
                            .is_some_and(|t| t.starts_with("真实手机屏幕 "))
                    })
                    .then(|| ("phone-local-01".into(), 2))
            });
        let Some((device, frames)) = device else {
            continue;
        };
        let recent = keep.entry(device).or_default();
        if recent.len() < frames && !recent.iter().any(|prior| images(prior).eq(images(parts))) {
            recent.push(parts);
        } else {
            archive.push(index);
        }
    }
    for index in archive {
        for part in context[index]["output"].as_array_mut().unwrap() {
            if part["type"] == "input_image" {
                *part = serde_json::json!({"type":"input_text","text":format!("{ARCHIVED_OBSERVATION}（本轮已验证观察；原图保存在执行记录）")});
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn repeated_host_screens_keep_the_latest_image_and_each_action_receipt() {
        let mut state = LoopState::default();
        let mut history = Vec::new();
        for (index, frame) in ["same", "same", "same", "same", "changed"]
            .into_iter()
            .enumerate()
        {
            let receipt = json!({"kind":"observe","host_session":"grant","device_id":"host-display-1",
                "action_id":index.to_string(),"observation_id":format!("view-{index}")});
            let parts = json!([{"type":"input_text","text":receipt.to_string()},
                {"type":"input_image","image_url":frame}]);
            assert_eq!(
                state.tool_result(
                    "host_desktop",
                    &format!("look {index}"),
                    &parts,
                    Some(&receipt),
                    ToolOutcome::Completed
                ),
                index == 3
            );
            history.push(json!({"type":"function_call_output","output":parts}));
        }
        compact_observations(&mut history);
        for (index, item) in history.iter().enumerate() {
            assert_eq!(item["output"][1]["type"] == "input_image", index == 4);
            let receipt: Value =
                serde_json::from_str(item["output"][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(receipt["action_id"], index.to_string());
        }
        assert_eq!(state.stalls, 0);
    }

    #[test]
    fn exploration_windows_preserve_activity_and_errors_across_restart_and_long_waits() {
        let mut state = LoopState {
            plan: Some(json!({"goal":"self-chosen"})),
            ..Default::default()
        };
        state.no_progress("actual failure");
        assert!(!state.exploration_yield_due(1000, 100));
        let mut resumed: LoopState =
            serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        assert!(!resumed.exploration_yield_due(2000, 139));
        assert!(resumed.exploration_yield_due(3000, 140));
        assert_eq!(resumed.plan, state.plan);
        assert_eq!(resumed.stalls, 1);
        let mut resumed: LoopState =
            serde_json::from_value(serde_json::to_value(&resumed).unwrap()).unwrap();
        assert!(!resumed.exploration_yield_due(3_600_000, 140));
        assert!(resumed.exploration_yield_due(4_200_000, 141));
        let legacy: LoopState = serde_json::from_value(json!({"stalls":2})).unwrap();
        assert!(legacy.exploration_window.is_none());
        assert_eq!(legacy.stalls, 2);
    }
    #[test]
    fn transient_model_errors_retry_without_replaying_tools() {
        assert_eq!(model_retry_delay("模型服务返回 HTTP 429", 1), Some(2000));
        assert_eq!(model_retry_delay("模型服务返回 HTTP 503", 8), Some(60000));
        assert_eq!(model_retry_delay("模型服务返回 HTTP 429", 9), None);
        assert_eq!(model_retry_delay("模型服务返回 HTTP 401", 1), None);
        assert_eq!(model_retry_delay("模型额度已耗尽", 1), None);
    }
    #[test]
    fn more_than_forty_turns_continue_in_bounded_slices() {
        let mut step = 0;
        let mut slices = 0;
        while step < 56 {
            let start = step;
            while !slice_due(start, step) {
                step += 1;
            }
            slices += 1;
        }
        assert_eq!((step, slices), (56, 7));
    }
    #[test]
    fn persisted_wait_keeps_its_original_deadline_after_restart() {
        let deadline = wake_at(1000, 120).unwrap();
        let saved = json!({"wake_at_ms":deadline});
        let loaded: Value = serde_json::from_str(&saved.to_string()).unwrap();
        assert_eq!(loaded["wake_at_ms"], 121000);
        assert!(60000 < loaded["wake_at_ms"].as_u64().unwrap());
        assert!(122000 >= loaded["wake_at_ms"].as_u64().unwrap());
        assert!(wake_at(0, 0).is_err());
        assert!(wake_at(0, 301).is_err());
    }
    #[test]
    fn explicit_resume_signal_survives_a_checkpoint_without_losing_prior_evidence() {
        let saved = json!({"stalls":3,"model_retries":2,"last_issue":"old failure",
            "recent_results":["completed-action"],"plan":{"goal":"same task"},"resume_requested":true});
        let state: LoopState = serde_json::from_value(saved).unwrap();
        let restored = serde_json::to_value(state).unwrap();
        assert_eq!(restored["resume_requested"], true);
        assert_eq!(restored["stalls"], 3);
        assert_eq!(restored["recent_results"], json!(["completed-action"]));
        assert_eq!(restored["plan"], json!({"goal":"same task"}));
    }
    #[test]
    fn repeated_failures_and_stalls_survive_restart() {
        let mut state = LoopState::default();
        assert!(!state.no_progress("no result"));
        let mut restored: LoopState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert!(!restored.no_progress("same error"));
        assert!(restored.no_progress("same error"));
    }
    #[test]
    fn repeated_device_receipt_counts_even_if_the_same_frame_was_reencoded() {
        let mut state = LoopState::default();
        let result = |metadata: &Value, image: &str| {
            json!([{"type":"input_text","text":metadata.to_string()},
                {"type":"input_image","image_url":image}])
        };
        let metadata = json!({"device_id":"desktop","observation_id":"same-observation"});
        for (index, image) in ["frame", "frame-copy", "frame-copy-2", "frame-copy-3"]
            .into_iter()
            .enumerate()
        {
            let value = result(&metadata, image);
            assert_eq!(
                state.tool_result(
                    "computer_type",
                    "{}",
                    &value,
                    Some(&metadata),
                    ToolOutcome::Completed
                ),
                index == 3
            );
            assert_eq!(state.stalls, index as u32);
        }
        let metadata = json!({"device_id":"desktop","observation_id":"next-observation"});
        assert!(!state.tool_result(
            "computer_type",
            "{}",
            &result(&metadata, "frame"),
            Some(&metadata),
            ToolOutcome::Completed
        ));
        assert_eq!(state.stalls, 0);
    }
    #[test]
    fn observation_compression_keeps_latest_two_and_validation_markers() {
        let mut context:Vec<_>=(0..5).map(|index|json!({"type":"function_call_output","call_id":index.to_string(),"output":[{"type":"input_text","text":"真实手机屏幕 720×1440"},{"type":"input_image","image_url":format!("data:image/png;base64,frame-{index}")}]})).collect();
        compact_observations(&mut context);
        let image_count = context
            .iter()
            .flat_map(|item| item["output"].as_array().unwrap())
            .filter(|part| part["type"] == "input_image")
            .count();
        assert_eq!(image_count, 2);
        assert!(
            context[0]["output"][1]["text"]
                .as_str()
                .unwrap()
                .starts_with(ARCHIVED_OBSERVATION)
        );
    }
}
