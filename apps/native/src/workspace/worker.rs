use super::*;
use aimanity_model::model_timing::Timing;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct DesktopInput {
    kind: String,
    #[serde(default)]
    observation_id: String,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    #[serde(default)]
    amount: i32,
    #[serde(default)]
    text: String,
    #[serde(default)]
    key: String,
}

impl Workspace {
    pub(super) async fn answer(
        &self,
        n: u64,
        id: &str,
        job: &super::conversation::Job,
        token: &CancellationToken,
    ) -> Result<(String, Vec<wire::HarnessEvent>)> {
        let config = Self::model(job.model.clone().ok_or("任务缺少模型")?)?;
        let client = self.client(&config.proxy_url)?;
        let mut input = self.context(n, &job.instructions)?;
        let host = if job.request.as_ref().is_some_and(|q| q.use_host_desktop) {
            Some(
                self.host
                    .lock()
                    .unwrap()
                    .as_ref()
                    .filter(|(owner, grant)| owner == id && grant.active())
                    .map(|(_, g)| g.clone())
                    .ok_or("请为当前任务重新授权本机桌面")?,
            )
        } else {
            None
        };
        let mut tools = vec![
            json!({"type":"function","name":"list_devices","description":"List registered devices and their actual availability.","parameters":{"type":"object","properties":{}}}),
            json!({"type":"function","name":"search_knowledge","description":"Find documents in this workspace by title or content.","parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}),
        ];
        if host.is_some() {
            tools.push(json!({"type":"function","name":"host_desktop","description":"Observe the authorized display or act using the observation ID from that display. Coordinates use screenshot pixels.","parameters":{"type":"object","properties":{"kind":{"type":"string","enum":["observe","click","double_click","scroll","type","key"]},"observation_id":{"type":"string"},"x":{"type":"integer"},"y":{"type":"integer"},"amount":{"type":"integer"},"text":{"type":"string"},"key":{"type":"string"}},"required":["kind"]}}));
        }
        let mut events = Vec::new();
        loop {
            let started = crate::connection::epoch_ms() as u64;
            let mut timing = Timing::default();
            let out = aimanity_model::request_tracked(
                &config,
                &client,
                json!({"input":input,"tools":tools,"reasoning":{"effort":"none"}}),
                token,
                &mut timing,
            )
            .await?;
            aimanity_model::validate_call_ids(&input, &out)?;
            events.push(wire::HarnessEvent {
                id: uuid::Uuid::now_v7().to_string(),
                run_id: id.into(),
                kind: "model".into(),
                title: "模型响应".into(),
                status: "succeeded".into(),
                created_at_ms: started,
                finished_at_ms: crate::connection::epoch_ms() as u64,
                model_timing: aimanity_model::model_timing::wire(&timing.snapshot()),
                ..Default::default()
            });
            if !out.iter().any(|v| v["type"] == "function_call") {
                let text = aimanity_model::text(&out);
                if text.is_empty() {
                    return Err("模型未返回文本".into());
                }
                return Ok((text, events));
            }
            for value in &out {
                let mut value = value.clone();
                if let Some(fields) = value.as_object_mut() {
                    fields.remove("provider_native");
                }
                input.push(value);
            }
            for call in out.iter().filter(|v| v["type"] == "function_call") {
                if token.is_cancelled() {
                    return Err("任务已停止".into());
                }
                let name = call["name"].as_str().ok_or("未知工具")?;
                let args = call["arguments"].as_str().ok_or("无效工具参数")?;
                let action_id = uuid::Uuid::now_v7().to_string();
                let at = crate::connection::epoch_ms() as u64;
                let result = match name {
                    "list_devices" => {
                        let registry: wire::RegistryState = self.get("registry")?;
                        let list:Vec<_>=registry.devices.into_iter().map(|d|json!({"id":d.id,"name":d.name,"kind":d.kind,"connected":d.connected})).collect();
                        Value::String(json!({"devices":list}).to_string())
                    }
                    "search_knowledge" => {
                        #[derive(Deserialize)]
                        struct Query {
                            query: String,
                        }
                        let q: Query = serde_json::from_str(args)?;
                        let bytes = self.documents(
                            "workbench/knowledge",
                            &wire::WorkbenchRequest {
                                query: q.query,
                                ..Default::default()
                            }
                            .encode_to_vec(),
                        )?;
                        let docs = wire::KnowledgeList::decode(bytes.as_slice())?.documents;
                        Value::String(serde_json::to_string(
                            &docs
                                .into_iter()
                                .take(12)
                                .map(|d| json!({"id":d.id,"title":d.title,"excerpt":d.content}))
                                .collect::<Vec<_>>(),
                        )?)
                    }
                    "host_desktop" => {
                        let grant = host.as_ref().ok_or("本任务未授权使用宿主机")?.clone();
                        let data: DesktopInput = serde_json::from_str(args)?;
                        let req = wire::DesktopActionRequest {
                            action_id: action_id.clone(),
                            control_session: id.into(),
                            kind: data.kind,
                            observation_id: data.observation_id,
                            x: data.x,
                            y: data.y,
                            amount: data.amount,
                            text: data.text,
                            key: data.key,
                            ..Default::default()
                        };
                        let reply =
                            tokio::task::spawn_blocking(move || grant.perform(&req)).await??;
                        let info=json!({"action_id":reply.action_id,"observation_id":reply.observation_id,"width":reply.width,"height":reply.height}).to_string();
                        if reply.jpeg.is_empty() {
                            Value::String(info)
                        } else {
                            json!([{"type":"input_text","text":info},{"type":"input_image","image_url":format!("data:image/jpeg;base64,{}",STANDARD.encode(reply.jpeg))}])
                        }
                    }
                    _ => return Err(format!("当前工作区没有工具 {name}").into()),
                };
                let finished = crate::connection::epoch_ms() as u64;
                let event = wire::HarnessEvent {
                    id: action_id,
                    run_id: id.into(),
                    kind: "tool".into(),
                    title: name.into(),
                    status: "succeeded".into(),
                    created_at_ms: at,
                    finished_at_ms: finished,
                    ..Default::default()
                };
                events.push(event);
                self.record_tools(n, id, &mut events)?;
                input.push(json!({"type":"function_call_output","call_id":call["call_id"],"output":result}));
            }
        }
    }
    pub(super) fn record_tools(
        &self,
        n: u64,
        id: &str,
        events: &mut [wire::HarnessEvent],
    ) -> Result<()> {
        let tx = self.db.begin_write()?;
        let mut table = tx.open_table(MESSAGES)?;
        let mut reply = wire::ChatMessage::decode(table.get(n + 1)?.ok_or("消息不存在")?.value())?;
        let mut head: wire::ChatState = read(&tx.open_table(RECORDS)?, "chat")?;
        let mut log: wire::HarnessEventList =
            read(&tx.open_table(RECORDS)?, &format!("events/{id}"))?;
        for event in events {
            if event.message_id.is_empty() {
                event.message_id = reply.id.clone();
                if event.kind == "tool" {
                    reply.action_receipts.push(wire::ChatActionReceipt {
                        id: event.id.clone(),
                        title: event.title.clone(),
                        received_at_ms: event.finished_at_ms,
                    });
                }
                log.events.push(event.clone());
            }
        }
        reply.has_activity = true;
        table.insert(n + 1, reply.encode_to_vec().as_slice())?;
        drop(table);
        head.revision += 1;
        put(&tx, "chat", &head)?;
        put(&tx, &format!("events/{id}"), &log)?;
        tx.commit()?;
        self.changed();
        Ok(())
    }
}
