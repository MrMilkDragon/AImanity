use super::device_run::{
    DesktopInput, DeviceRun, add_desktop_actions, device_info, device_output, retire_image,
};
use super::*;
use aimanity_model::model_timing::Timing;
use serde::Deserialize;
use serde_json::{Value, json};

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
        let (mut input, mut input_pending) = self.context(n, &job.instructions)?;
        let host = job.request.as_ref().is_some_and(|q| q.use_host_desktop);
        if host
            && !self
                .host
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|(owner, grant)| owner == id && grant.active())
        {
            return Err("请为当前任务重新授权本机桌面".into());
        }
        let target = job.request.as_ref().map_or("", |q| q.device_id.as_str());
        let registry = self.registry()?;
        if target.is_empty() {
            let roster: Vec<_> = registry.devices.iter().map(device_info).collect();
            input.insert(
                1,
                json!({"role":"system","content":format!("Workspace devices: {}",json!(roster))}),
            );
        }
        let mut device_run = DeviceRun::for_devices(
            &self.peers,
            registry
                .devices
                .into_iter()
                .filter(|d| d.paired && d.connected && (target.is_empty() || d.id == target)),
        );
        let previous: wire::HarnessEventList = self.get(&format!("events/{id}"))?;
        if previous
            .events
            .iter()
            .any(|e| e.kind == "device_input" && e.status == "succeeded")
        {
            input_pending = false;
        }
        let mut history = Vec::new();
        for event in previous
            .events
            .iter()
            .filter(|e| matches!(e.title.as_str(), "device_interact" | "host_desktop"))
        {
            let receipt: Value = serde_json::from_str(&event.content)?;
            if event.status == "dispatched"
                && let Some(device) = receipt["device_id"].as_str()
            {
                device_run.uncertain.push(device.into());
            }
            history.push(json!({"status":event.status,"receipt":receipt}));
        }
        if !history.is_empty() {
            input.push(json!({"role":"user","content":format!("Resuming this run. These are recorded device operations. Inspect the current state before continuing: {}",json!(history))}));
        }
        let skills = self.active_skill_ids()?;
        let mut tools = vec![
            json!({"type":"function","name":"skill_read","description":"Read an enabled guide when its topic matches the task.","parameters":{"type":"object","properties":{"id":{"type":"string","enum":skills}},"required":["id"]}}),
            json!({"type":"function","name":"search_knowledge","description":"Find documents in this workspace by title or content.","parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}),
        ];
        let mut events = Vec::new();
        let mut host_image = None;
        if !device_run.devices.is_empty() {
            tools.push(device_run.tool());
            let microphones: Vec<_> = device_run
                .devices
                .iter()
                .filter(|d| d.guest_audio)
                .map(|d| &d.id)
                .collect();
            if !microphones.is_empty() {
                tools.push(json!({"type":"function","name":"device_audio_input","description":"Feed a WAV work file into the selected device microphone.","parameters":{"type":"object","properties":{"device_id":{"type":"string","enum":microphones},"path":{"type":"string"}},"required":["device_id","path"]}}));
            }
        }
        let first_device = if host {
            None
        } else if !target.is_empty() {
            Some(target.to_owned())
        } else if input_pending && device_run.devices.len() == 1 {
            Some(device_run.devices[0].id.clone())
        } else {
            None
        };
        if let Some(target) = first_device {
            let mut event = tool_event(id, "device_interact");
            let data = DesktopInput::observe(&target);
            let output = device_run
                .perform(self, n, id, data, &mut event, &mut events)
                .await?;
            event.finished_at_ms = crate::connection::epoch_ms() as u64;
            self.record_tools(n, id, vec![event])?;
            let turn = input
                .iter()
                .rposition(|v| v["role"] == "user" && v["content"].is_array())
                .ok_or("任务缺少用户消息")?;
            device_run.images.insert(target, turn);
            input[turn]["content"]
                .as_array_mut()
                .unwrap()
                .extend(output.as_array().unwrap().iter().cloned());
        }
        if host {
            let mut tool = json!({"type":"function","name":"host_desktop","description":format!("Operate the authorized local {} desktop. Use the observation ID and screenshot pixels from this display.",ai_phone_api::platform_name(std::env::consts::OS).unwrap_or(std::env::consts::OS)),"parameters":{"type":"object","properties":{"kind":{"type":"string","enum":["observe","click","double_click","scroll","type","key"]},"observation_id":{"type":"string"},"x":{"type":"integer"},"y":{"type":"integer"},"amount":{"type":"integer"},"text":{"type":"string"},"key":{"type":"string"}},"required":["kind"]}});
            add_desktop_actions(&mut tool);
            tools.push(tool);
        }
        loop {
            let started = crate::connection::epoch_ms() as u64;
            let mut timing = Timing::default();
            let mut body = json!({"input":input,"tools":tools,"reasoning":{"effort":"none"}});
            if input_pending {
                let mut choices: Vec<_> = tools
                    .iter()
                    .filter(|t| {
                        matches!(
                            t["name"].as_str(),
                            Some(
                                "device_interact"
                                    | "device_audio_input"
                                    | "skill_read"
                                    | "host_desktop"
                            )
                        )
                    })
                    .cloned()
                    .collect();
                choices.push(json!({"type":"function","name":"report_no_input","description":"End with the actual reason if no device input is appropriate or available for this request.","parameters":{"type":"object","properties":{"reason":{"type":"string"}},"required":["reason"]}}));
                body["tools"] = json!(choices);
                body["tool_choice"] = json!("required");
            }
            let out = aimanity_model::request_with_text(
                &config,
                &client,
                body,
                token,
                &mut timing,
                |delta| {
                    if !input_pending {
                        self.show_text(id, delta)
                    }
                },
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
                if input_pending {
                    self.record_tools(n, id, events)?;
                    return Err("本轮未收到设备输入回执".into());
                }
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
                let mut event = tool_event(id, name);
                let result = match name {
                    "report_no_input" if input_pending => {
                        let args: Value = serde_json::from_str(args)?;
                        self.record_tools(n, id, events)?;
                        return Err(format!(
                            "本轮未执行设备输入 · {}",
                            args["reason"].as_str().unwrap_or("设备未确认操作")
                        )
                        .into());
                    }
                    "skill_read" => {
                        #[derive(Deserialize)]
                        struct Read {
                            id: String,
                        }
                        let q: Read = serde_json::from_str(args)?;
                        event.content = q.id.clone();
                        Value::String(self.read_active_skill(&q.id)?.into())
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
                    "device_audio_input" => {
                        #[derive(Deserialize)]
                        struct Feed {
                            device_id: String,
                            path: String,
                        }
                        let feed: Feed = serde_json::from_str(args)?;
                        if !device_run
                            .devices
                            .iter()
                            .any(|d| d.id == feed.device_id && d.guest_audio)
                        {
                            return Err("设备麦克风未授权".into());
                        }
                        let session = device_run.session(&feed.device_id)?;
                        tokio::select! {
                            biased;
                            _ = token.cancelled() => return Err("任务已停止".into()),
                            result = self.send_audio(&feed.device_id, &feed.path,Some(&session)) => result?,
                        }
                        event.content =
                            json!({"device_id":feed.device_id,"file":feed.path}).to_string();
                        event.kind = "device_input".into();
                        event.status = "succeeded".into();
                        input_pending = false;
                        Value::String(event.content.clone())
                    }
                    "device_interact" => {
                        let data: DesktopInput = serde_json::from_str(args)?;
                        let device = data.device_id.clone();
                        let needs_input = !matches!(data.kind.as_str(), "observe" | "foreground");
                        let kind = data.kind.clone();
                        let result = device_run
                            .perform(self, n, id, data, &mut event, &mut events)
                            .await?;
                        if needs_input {
                            input_pending =
                                event.kind != "device_input" || event.status != "dispatched";
                        }
                        if kind != "foreground" && event.status != "not_executed" {
                            retire_image(device_run.images.remove(&device), &mut input);
                        }
                        if result.is_array() {
                            device_run.images.insert(device, input.len());
                        }
                        result
                    }
                    "host_desktop" => {
                        let data: DesktopInput = serde_json::from_str(args)?;
                        let action = data.request(&event.id, id);
                        let is_input = action.kind != "observe";
                        let mut info = json!({"action_id":event.id,"kind":action.kind});
                        if is_input {
                            event.kind = "device_input".into();
                            event.status = "dispatched".into();
                            event.content = info.to_string();
                            events.push(event.clone());
                            self.record_tools(n, id, std::mem::take(&mut events))?;
                        }
                        let reply = self.host_action(action).await?;
                        retire_image(host_image.take(), &mut input);
                        if !reply.jpeg.is_empty() {
                            host_image = Some(input.len());
                        }
                        if is_input {
                            input_pending = false;
                        }
                        info["observation_id"] = json!(reply.observation_id);
                        info["width"] = json!(reply.width);
                        info["height"] = json!(reply.height);
                        event.content = info.to_string();
                        device_output(&info, reply)
                    }
                    _ => return Err(format!("当前工作区没有工具 {name}").into()),
                };
                if event.status == "dispatched" {
                    event.status = "succeeded".into();
                }
                event.finished_at_ms = crate::connection::epoch_ms() as u64;
                events.push(event);
                self.record_tools(n, id, std::mem::take(&mut events))?;
                input.push(json!({"type":"function_call_output","call_id":call["call_id"],"output":result}));
            }
        }
    }
    pub(super) fn record_tools(
        &self,
        n: u64,
        id: &str,
        events: Vec<wire::HarnessEvent>,
    ) -> Result<()> {
        let tx = self.db.begin_write()?;
        let mut table = tx.open_table(MESSAGES)?;
        let mut reply = wire::ChatMessage::decode(table.get(n + 1)?.ok_or("消息不存在")?.value())?;
        if let Some(live) = self.live.lock().unwrap().take() {
            if !reply.text.is_empty() {
                reply.text.push_str("\n\n");
            }
            reply.text.push_str(&live.text);
        }
        let mut head: wire::ChatState = read(&tx.open_table(RECORDS)?, "chat")?;
        let mut log: wire::HarnessEventList =
            read(&tx.open_table(RECORDS)?, &format!("events/{id}"))?;
        for mut event in events {
            event.message_id = reply.id.clone();
            if matches!(event.kind.as_str(), "tool" | "device_input") && event.status == "succeeded"
            {
                reply.action_receipts.push(wire::ChatActionReceipt {
                    id: event.id.clone(),
                    title: event.title.clone(),
                    received_at_ms: event.finished_at_ms,
                });
            }
            if event.title == "device_interact"
                && event.status == "succeeded"
                && let Some(previous) = log
                    .events
                    .iter_mut()
                    .rev()
                    .find(|e| e.id == event.id && e.status == "dispatched")
            {
                *previous = event;
            } else {
                log.events.push(event);
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
fn tool_event(run: &str, name: &str) -> wire::HarnessEvent {
    wire::HarnessEvent {
        id: uuid::Uuid::now_v7().to_string(),
        run_id: run.into(),
        kind: "tool".into(),
        title: name.into(),
        status: "succeeded".into(),
        created_at_ms: crate::connection::epoch_ms() as u64,
        ..Default::default()
    }
}
