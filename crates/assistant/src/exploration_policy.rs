//! One policy for self-directed exploration on every device.
use serde_json::{Value, json};

const LIBERTY_PREFIX: &str = "Liberty 已在这台";

pub fn is_context(context: &[Value]) -> bool {
    context.iter().any(|message| {
        message["role"] == "system"
            && message["content"][0]["text"].as_str().is_some_and(|text| {
                text.starts_with(LIBERTY_PREFIX)
                    || text.starts_with("主人已为这台手机开启自主探索。")
                    || text.starts_with("主人已为这台电脑开启自主探索。")
            })
    })
}

pub fn context(phone: bool, reply: bool, install: bool, experience: &str) -> Vec<Value> {
    let device = if phone { "手机" } else { "电脑" };
    let skill = if phone { "phone-use" } else { "computer-use" };
    vec![
        json!({"role":"system","content":[{"type":"input_text","text":format!(
            "{LIBERTY_PREFIX}{device}开启。没有预设应用或目标。读 {skill} 并观察当前状态，自行决定下一步、改向或休息。可按需操作工作区、选择已授权设备、交接任务和文件，保存已核验的经验。\n自主回复={reply}，自主安装={install}。语音、用户新指令、通知和手动接管优先。真实停顿时用 task_finish 结束本轮。"
        )}]}),
        crate::model_context::environment(experience),
        json!({"role":"user","content":[{"type":"input_text","text":"Liberty。下一步和休息时机由你决定。"}]}),
    ]
}
