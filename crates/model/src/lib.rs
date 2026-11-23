pub mod anthropic_messages;
pub mod chat_completions;
mod config;
mod model_sse;
pub mod model_timing;
pub use config::{ModelConfig, client_for_proxy};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{collections::HashSet, time::Duration};
use tokio_util::sync::CancellationToken;

pub async fn request_tracked(
    config: &ModelConfig,
    client: &reqwest::Client,
    mut body: Value,
    cancel: &CancellationToken,
    timing: &mut crate::model_timing::Timing,
) -> Result<Vec<Value>, String> {
    body["model"] = json!(config.model);
    body["stream"] = json!(true);
    body["store"] = json!(false);
    let effort = body["reasoning"]["effort"]
        .as_str()
        .unwrap_or("high")
        .to_owned();
    if config.provider != "deepseek" {
        body.as_object_mut()
            .ok_or("模型请求格式错误")?
            .remove("reasoning");
    }
    let chat_protocol = config.protocol == "chat-completions";
    let messages_protocol = config.protocol == "anthropic-messages";
    let claude_binding = messages_protocol && uses_claude_binding(config);
    if chat_protocol {
        body = crate::chat_completions::request_body(
            &body,
            matches!(
                config.provider.as_str(),
                "deepseek"
                    | "kimi"
                    | "kimi-cn"
                    | "glm"
                    | "zai"
                    | "mimo"
                    | "hunyuan"
                    | "stepfun"
                    | "openrouter"
            ) || qwen_38(config),
        )?;
        configure_chat(&mut body, config, &effort);
    } else if messages_protocol {
        body = crate::anthropic_messages::request_body(&body)?;
        if config.provider == "deepseek" {
            let enabled = effort != "none" && body["tool_choice"]["type"] != "tool";
            body["thinking"] = json!({"type":if enabled {"enabled"} else {"disabled"}});
            if enabled {
                body["output_config"] = json!({"effort":effort});
            }
        } else if claude_binding {
            configure_claude(&mut body, &config.model, &effort);
        }
    } else {
        configure_responses(&mut body, config, &effort);
    }
    let encoded = serde_json::to_vec(&body).map_err(|_| "模型请求编码失败")?;
    drop(body);
    timing.request_started(encoded.len());
    let mut request = client
        .post(config.endpoint()?)
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(encoded);
    if !config.api_key.is_empty() {
        request = if messages_protocol {
            request.header("x-api-key", &config.api_key)
        } else {
            request.bearer_auth(&config.api_key)
        };
    }
    if messages_protocol {
        request = request.header("anthropic-version", "2023-06-01");
    }
    if claude_binding {
        request = request.header("anthropic-beta", "thinking-binding-controls-2026-08-01");
    }
    let response = tokio::select! {_ = cancel.cancelled()=>return Err("canceled".into()),r=request.send()=>r.map_err(|_|"模型网络连接失败，请检查地址、代理与服务状态")?};
    timing.headers_received();
    if !response.status().is_success() {
        // Inspect only bounded error codes; never expose provider messages or echoed input.
        let status = response.status().as_u16();
        if status == 400 {
            return Err(
                "模型拒绝请求（HTTP 400）；请核对协议、工具往返与上下文格式。配置和任务已保留"
                    .into(),
            );
        }
        if status == 402 {
            return Err("模型服务要求付款（HTTP 402）；请检查服务商余额或套餐".into());
        }
        if status == 429 && quota_exhausted(response, cancel).await? {
            return Err("模型额度已耗尽，请检查服务商余额；任务和记录已保留".into());
        }
        return Err(format!(
            "模型服务返回 HTTP {}；请检查模型 ID、密钥和服务权限",
            status
        ));
    }
    if !response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("text/event-stream")
        })
    {
        return Err("模型未返回 text/event-stream；请检查 API 地址与协议".into());
    }
    let mut stream = response.bytes_stream();
    let mut parser = ResponseStream::default();
    let mut messages_parser = crate::anthropic_messages::MessagesStream::default();
    let mut chat_parser = crate::chat_completions::ChatStream::default();
    loop {
        let chunk = tokio::select! {_ = cancel.cancelled()=>return Err("canceled".into()),chunk=stream.next()=>chunk};
        match chunk {
            Some(Ok(chunk)) => {
                if !chunk.is_empty() {
                    timing.first_bytes_received();
                }
                let (output, has_text) = if chat_protocol {
                    (chat_parser.feed(&chunk)?, chat_parser.has_text())
                } else if messages_protocol {
                    (messages_parser.feed(&chunk)?, messages_parser.has_text())
                } else {
                    (parser.feed(&chunk)?, parser.has_text)
                };
                if has_text {
                    timing.text_received();
                }
                if let Some(mut output) = output {
                    if !chat_protocol && !messages_protocol {
                        for call in output
                            .iter_mut()
                            .filter(|item| item["type"] == "function_call")
                        {
                            call["provider_native"] = json!(true);
                        }
                    }
                    return Ok(output);
                }
            }
            Some(Err(_)) => return Err("模型流中断，尚未执行后续操作".into()),
            None => return Err("模型未返回完整结果，尚未执行后续操作".into()),
        }
    }
}

pub fn validate_call_ids(context: &[Value], output: &[Value]) -> Result<(), String> {
    let mut calls = output
        .iter()
        .filter(|item| item["type"] == "function_call")
        .peekable();
    if calls.peek().is_none() {
        return Ok(());
    }
    let mut ids: HashSet<_> = context
        .iter()
        .filter(|item| {
            matches!(
                item["type"].as_str(),
                Some("function_call" | "function_call_output")
            )
        })
        .filter_map(|item| item["call_id"].as_str())
        .collect();
    for call in calls {
        let id = call["call_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("模型未提供工具调用标识")?;
        if !ids.insert(id) {
            return Err("模型重复使用工具调用标识，已拒绝本轮调用".into());
        }
    }
    Ok(())
}

async fn quota_exhausted(
    response: reqwest::Response,
    cancel: &CancellationToken,
) -> Result<bool, String> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    // The HTTP status is already known; optional diagnostics cannot hold the
    // worker for the normal 180-second generation timeout.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while let Some(Ok(chunk)) = tokio::select! {
        _ = cancel.cancelled() => return Err("canceled".into()),
        _ = tokio::time::sleep_until(deadline) => None,
        chunk = stream.next() => chunk,
    } {
        if bytes.len() + chunk.len() > 8192 {
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice::<Value>(&bytes)
        .ok()
        .is_some_and(|value| {
            ["insufficient_quota", "billing_hard_limit_reached"]
                .contains(&value["error"]["code"].as_str().unwrap_or(""))
        }))
}

fn configure_responses(body: &mut Value, config: &ModelConfig, effort: &str) {
    let fast = effort == "none";
    match config.provider.as_str() {
        "meta" => {
            body["include"] = json!(["reasoning.encrypted_content"]);
            body["reasoning"] = json!({"effort":if fast {"minimal"} else {effort}});
        }
        "xai"
            if matches!(
                config.model.as_str(),
                "grok-4.7" | "grok-4.7-fast" | "grok-4.6" | "grok-4.5"
            ) =>
        {
            body["reasoning"] = json!({"effort":if fast {"low"} else {effort}});
        }
        "mistral"
            if matches!(
                config.model.as_str(),
                "mistral-large-4" | "mistral-large-4-0"
            ) =>
        {
            body["reasoning"] = json!({"effort":if fast {"low"} else {effort}});
        }
        "compatible"
            if (config.base_url == "https://api.openai.com"
                || config.base_url.starts_with("https://api.openai.com/"))
                && config.model.starts_with("gpt-6") =>
        {
            let requires_reasoning =
                config.model.starts_with("gpt-6-astra") || config.model.starts_with("gpt-6.1-sol");
            body["include"] = json!(["reasoning.encrypted_content"]);
            body["reasoning"] =
                json!({"effort":if fast && requires_reasoning {"low"} else {effort}});
        }
        _ => {}
    }
}

fn qwen_38(config: &ModelConfig) -> bool {
    matches!(config.provider.as_str(), "qwen" | "qwen-global")
        && config.model.starts_with("qwen3.8-")
}

fn configure_chat(body: &mut Value, config: &ModelConfig, effort: &str) {
    let fast = effort == "none";
    if config.provider == "deepseek" {
        let forced = body["tool_choice"] == "required" || body["tool_choice"].is_object();
        body["thinking"] = json!({"type":if fast||forced{"disabled"}else{"enabled"}});
        if !fast && !forced {
            body["reasoning_effort"] = json!(effort);
            if let Some(messages) = body["messages"].as_array_mut() {
                for message in messages
                    .iter_mut()
                    .rev()
                    .take_while(|m| m["role"] != "user")
                {
                    if message["role"] == "assistant"
                        && message.get("tool_calls").is_none()
                        && message.get("reasoning_content").is_none()
                        && let Some(text) = message["content"].as_str()
                    {
                        *message = json!({"role":"user","content":format!("Previous assistant draft (not execution evidence): {text}")});
                    }
                }
            }
        }
    }
    if matches!(config.provider.as_str(), "glm" | "zai")
        && (config.model == "glm-5.3" || config.model.starts_with("glm-5.3-flash"))
    {
        body["thinking"] = json!({"type":"enabled","clear_thinking":false});
        body["reasoning_effort"] = json!(if fast { "low" } else { effort });
        body["tool_stream"] = json!(true);
    } else if matches!(config.provider.as_str(), "kimi" | "kimi-cn" | "glm" | "zai") {
        body["thinking"] = json!({"type":if fast{"disabled"}else{"enabled"}});
    }
    let qwen_38 = qwen_38(config);
    if qwen_38 {
        body["reasoning_effort"] = json!(effort);
    } else if config.provider == "stepfun" && config.model == "step-5-preview" {
        body["reasoning_effort"] = json!(if fast { "low" } else { effort });
    }
    if qwen_38 || matches!(config.provider.as_str(), "glm" | "zai" | "mimo") {
        if let Some(name) = body["tool_choice"]["function"]["name"]
            .as_str()
            .map(str::to_owned)
            && let Some(tools) = body["tools"].as_array_mut()
        {
            tools.retain(|tool| tool["function"]["name"] == name);
        }
        if body["tools"].is_array() && body["tool_choice"] != "none" {
            body["tool_choice"] = json!("auto");
        }
        if !qwen_38 {
            body.as_object_mut().unwrap().remove("parallel_tool_calls");
        }
    }
}

pub fn uses_claude_binding(config: &ModelConfig) -> bool {
    config.provider == "anthropic"
        && matches!(
            config.model.as_str(),
            "claude-opus-5-5"
                | "claude-sonnet-5-5"
                | "claude-haiku-5-5"
                | "claude-fable-5-1"
                | "claude-mythos-5-1"
        )
}

fn configure_claude(body: &mut Value, model: &str, effort: &str) {
    // Compaction and changing the available tools can invalidate earlier thinking.
    body["thinking"] = json!({
        "type": "adaptive",
        "block_binding": {"prefix_mismatch_behavior": "drop_block"}
    });
    body["output_config"] = json!({"effort": if effort == "none" { "low" } else { effort }});
    if model != "claude-haiku-5-5"
        && matches!(body["tool_choice"]["type"].as_str(), Some("tool" | "any"))
    {
        if let Some(name) = body["tool_choice"]["name"].as_str().map(str::to_owned)
            && let Some(tools) = body["tools"].as_array_mut()
        {
            tools.retain(|tool| tool["name"] == name);
        }
        body["tool_choice"] = json!({"type":"auto","disable_parallel_tool_use":true});
    }
}
#[derive(Default)]
struct ResponseStream {
    sse: crate::model_sse::SseDecoder,
    has_text: bool,
}

fn response_fields(value: &Value, fields: &[&str]) -> Value {
    Value::Object(
        fields
            .iter()
            .filter_map(|field| Some(((*field).into(), value.get(field)?.clone())))
            .collect(),
    )
}

fn response_item(item: &Value) -> Result<Value, String> {
    if item.get("role").is_some_and(|role| role != "assistant") {
        return Err("模型返回了非助手角色".into());
    }
    match item["type"].as_str() {
        Some("message") => {
            let parts = item["content"].as_array().ok_or("模型消息缺少内容")?;
            let content = parts
                .iter()
                .map(|part| match part["type"].as_str() {
                    Some("output_text") | None if part["text"].is_string() => {
                        let mut part = response_fields(part, &["text", "annotations", "logprobs"]);
                        part["type"] = json!("output_text");
                        Ok(part)
                    }
                    Some("refusal") if part["refusal"].is_string() => {
                        Ok(response_fields(part, &["type", "refusal"]))
                    }
                    _ => Err("模型返回了无效消息内容"),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut message = response_fields(item, &["type", "id", "status", "phase"]);
            message["role"] = json!("assistant");
            message["content"] = json!(content);
            Ok(message)
        }
        Some("function_call")
            if item["name"].as_str().is_some_and(|name| !name.is_empty())
                && item["arguments"].is_string()
                && item
                    .get("status")
                    .is_none_or(|status| status == "completed") =>
        {
            Ok(response_fields(
                item,
                &["type", "id", "status", "call_id", "name", "arguments"],
            ))
        }
        Some("reasoning") => Ok(response_fields(
            item,
            &[
                "type",
                "id",
                "status",
                "summary",
                "encrypted_content",
                "content",
            ],
        )),
        _ => Err("模型返回了未授权的输出类型".into()),
    }
}

impl ResponseStream {
    fn completed(response: &Value) -> Result<Vec<Value>, String> {
        // Compatible gateways may omit status; an explicit value must be completed.
        if response
            .get("status")
            .is_some_and(|status| status != "completed")
        {
            return Err("模型未完成本轮回复；未将部分输出作为操作执行".into());
        }
        response["output"]
            .as_array()
            .ok_or("模型返回缺少完整输出")?
            .iter()
            .map(response_item)
            .collect()
    }

    fn feed(&mut self, chunk: &[u8]) -> Result<Option<Vec<Value>>, String> {
        for data in self.sse.feed(chunk)? {
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let event: Value = serde_json::from_str(&data).map_err(|_| "模型流格式无效")?;
            match event["type"].as_str() {
                Some("response.output_text.delta") => {
                    self.has_text |= has_text(&event["delta"]);
                }
                Some("response.completed") => {
                    let output = Self::completed(&event["response"])?;
                    self.has_text |= output
                        .iter()
                        .filter(|item| item["type"] == "message")
                        .flat_map(|item| item["content"].as_array().into_iter().flatten())
                        .any(|part| part["type"] == "output_text" && has_text(&part["text"]));
                    return Ok(Some(output));
                }
                Some("response.failed" | "response.incomplete" | "error") => {
                    return Err("模型未完成本轮回复；未将部分输出作为操作执行".into());
                }
                _ => {}
            }
        }
        Ok(None)
    }
}
fn has_text(value: &Value) -> bool {
    value.as_str().is_some_and(|text| !text.trim().is_empty())
}
pub fn text(output: &[Value]) -> String {
    output
        .iter()
        .filter(|v| v["type"] == "message")
        .flat_map(|v| v["content"].as_array().into_iter().flatten())
        .filter_map(|v| v["text"].as_str())
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deepseek_forced_tools_disable_thinking_without_removing_the_tool_constraint() {
        let config = ModelConfig {
            provider: "deepseek".into(),
            protocol: "chat-completions".into(),
            api_key: String::new(),
            base_url: "https://fixture.invalid".into(),
            model: "fixture".into(),
            proxy_url: String::new(),
            verified_at_ms: 0,
        };
        for choice in [
            json!("required"),
            json!({"type":"function","function":{"name":"observe"}}),
        ] {
            let mut body = json!({"tool_choice":choice});
            configure_chat(&mut body, &config, "high");
            assert_eq!(body["thinking"]["type"], "disabled");
            assert_eq!(body["tool_choice"], choice);
            assert!(body.get("reasoning_effort").is_none());
        }
    }

    #[test]
    fn glm_keeps_the_requested_tool_boundary_and_explicit_no_tools() {
        let config = ModelConfig {
            provider: "glm".into(),
            protocol: "chat-completions".into(),
            api_key: String::new(),
            base_url: "https://fixture.invalid".into(),
            model: "fixture".into(),
            proxy_url: String::new(),
            verified_at_ms: 0,
        };
        let mut body = json!({"tools":[{"function":{"name":"observe"}},{"function":{"name":"tap"}}],"tool_choice":{"function":{"name":"observe"}},"parallel_tool_calls":false});
        configure_chat(&mut body, &config, "high");
        assert_eq!(body["tools"].as_array().unwrap().len(), 1);
        assert_eq!(body["tools"][0]["function"]["name"], "observe");
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["thinking"]["type"], "enabled");
        body["tool_choice"] = json!("none");
        configure_chat(&mut body, &config, "none");
        assert_eq!(body["tool_choice"], "none");
        assert_eq!(body["thinking"]["type"], "disabled");
        assert!(body.get("parallel_tool_calls").is_none());
    }

    #[test]
    fn stream_accepts_split_utf8_and_completed_without_done() {
        let mut p = ResponseStream::default();
        let bytes="data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"成功\"}]}]}}\r\n\r\n".as_bytes();
        let cut = bytes.iter().position(|b| *b > 127).unwrap() + 1;
        assert!(p.feed(&bytes[..cut]).unwrap().is_none());
        assert_eq!(
            p.feed(&bytes[cut..]).unwrap().unwrap()[0]["content"][0]["text"],
            "成功"
        );
    }

    #[test]
    fn responses_support_standard_multiline_event_data() {
        let mut parser = ResponseStream::default();
        let event = b": heartbeat\r\nevent: response.completed\r\ndata: {\"type\":\"response.completed\",\r\ndata: \"response\":{\"output\":[{\"type\":\"message\",\"content\":[{\"text\":\"receipt\"}]}]}}\r\n\r\n";
        let output = parser.feed(event).unwrap().unwrap();
        assert_eq!(output[0]["content"][0]["text"], "receipt");
    }

    #[test]
    fn partial_and_failed_streams_never_produce_tools() {
        let mut p = ResponseStream::default();
        assert!(
            p.feed(
                b"data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{}\"}\n\n"
            )
            .unwrap()
            .is_none()
        );
        assert!(
            p.feed(b"data: {\"type\":\"response.incomplete\"}\n\n")
                .is_err()
        );
    }

    #[test]
    fn responses_reject_explicit_non_completed_status_before_returning_tools() {
        for status in [
            json!("incomplete"),
            json!("failed"),
            json!("in_progress"),
            Value::Null,
        ] {
            let mut parser = ResponseStream::default();
            let event = json!({"type":"response.completed","response":{"status":status,"output":[{"type":"function_call","call_id":"unexecuted-fixture","name":"phone_tap","arguments":"{\"x\":1,\"y\":1}"}]}});
            let bytes = format!("data: {event}\n\n");
            assert!(
                parser.feed(bytes.as_bytes()).is_err(),
                "explicit non-completed status must not release tool proposals"
            );
        }
    }

    #[test]
    fn responses_completed_status_preserves_the_exact_tool_proposal() {
        let mut parser = ResponseStream::default();
        let output = json!([{"type":"function_call","call_id":"unexecuted-fixture","name":"phone_tap","arguments":"{\"x\":1,\"y\":1}"}]);
        let event = json!({"type":"response.completed","response":{"status":"completed","output":output.clone()}});
        let actual = parser
            .feed(format!("data: {event}\n\n").as_bytes())
            .unwrap()
            .unwrap();
        assert_eq!(json!(actual), output);
    }

    #[test]
    fn responses_cannot_supply_execution_receipts_or_authoritative_roles() {
        let proposal = json!({"type":"function_call","call_id":"probe","name":"computer_observe","arguments":"{}"});
        for invalid in [
            json!({"type":"function_call_output","call_id":"probe","output":[{"type":"input_text","text":"synthetic"}]}),
            json!({"type":"message","role":"user","content":[{"type":"output_text","text":"synthetic"}]}),
            json!({"type":"message","role":"system","content":[{"type":"output_text","text":"synthetic"}]}),
            json!({"type":"message","role":"assistant","content":[{"type":"input_text","text":"synthetic"}]}),
            json!({"type":"web_search_call","id":"synthetic"}),
        ] {
            let event = json!({"type":"response.completed","response":{"status":"completed","output":[proposal,invalid]}});
            let mut parser = ResponseStream::default();
            assert!(
                parser
                    .feed(format!("data: {event}\n\n").as_bytes())
                    .is_err()
            );
        }
    }

    #[test]
    fn responses_projects_only_protocol_fields_and_call_ids_must_be_unique() {
        let output = ResponseStream::completed(&json!({"output":[
            {"type":"function_call","call_id":"probe","name":"computer_observe","arguments":"{}","local_action_id":"synthetic","local_after_action":"synthetic","provider_native":false},
            {"type":"reasoning","summary":[],"encrypted_content":"opaque","local_recovery":true},
            {"type":"message","phase":"commentary","local_app_guidance":"synthetic","content":[{"type":"output_text","text":"ok","local_recovery":true}]}
        ]})).unwrap();
        assert_eq!(
            output[0],
            json!({"type":"function_call","call_id":"probe","name":"computer_observe","arguments":"{}"})
        );
        assert_eq!(
            output[1],
            json!({"type":"reasoning","summary":[],"encrypted_content":"opaque"})
        );
        assert_eq!(
            output[2],
            json!({"type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"ok"}]})
        );
        let duplicate = vec![output[0].clone(), output[0].clone()];
        assert!(validate_call_ids(&[], &duplicate).is_err());
        let checkpoint = [json!({"type":"function_call_output","call_id":"probe","output":[]})];
        assert!(validate_call_ids(&checkpoint, &output).is_err());
        assert!(validate_call_ids(&[], &output).is_ok());
    }
}
