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
            "{LIBERTY_PREFIX}{device}开启。没有预设应用、目标或产出。先读 {skill} 并观察当前状态，自行决定行动、改向或休息。值得持续的目标用 task_plan 记录；醒来后根据新观察接着做。\n按需用 device_list 和 device_link 协作，device_message 可交接任务，device_transfer 传文件。依据真实回执继续。可以调用工作区、Skill、知识库和位置工具。只保存可复用且已核验的经验。\n自主回复={reply}，自主安装={install}。遵守主人授权，语音、用户新指令、通知和手动接管优先。真实停顿时用 task_finish 结束本轮。\n{experience}"
        )}]}),
        json!({"role":"user","content":[{"type":"input_text","text":"Liberty。下一步和休息时机由你决定。"}]}),
    ]
}
