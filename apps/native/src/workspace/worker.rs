use super::device_run::{
    DesktopInput, DeviceRun, add_desktop_actions, device_info, device_output, retire_image,
};
use super::*;
use aimanity_model::model_timing::Timing;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    hash::{BuildHasher, BuildHasherDefault, DefaultHasher},
};

impl Workspace {
    pub(super) async fn answer(
        self: &Arc<Self>,
        n: u64,
        id: &str,
        job: &super::conversation::Job,
        token: &CancellationToken,
    ) -> Result<(String, Vec<wire::HarnessEvent>)> {
        let config = Self::model(job.model.clone().ok_or("任务缺少模型")?)?;
        let client = self.client(&config.proxy_url)?;
        let is_child = self.run.lock().unwrap().get(id).is_some_and(|a| a.child);
        let (mut input, mut input_pending) = self.context(
            n,
            &job.instructions,
            is_child || job.source == "notification",
        )?;
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
        let mut repair_pending = cfg!(any(
            target_os = "macos",
            windows,
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "android", target_arch = "aarch64")
        )) && !is_child
            && matches!(job.source.as_str(), "user" | "copilot")
            && job.request.as_ref().is_some_and(|q| {
                let text = q.text.trim().to_lowercase();
                text.contains("repair") && text.contains("virtual_devices")
                    || registry.devices.iter().any(|d| {
                        !d.paired
                            && text.contains(&d.name.to_lowercase())
                            && ["repair ", "fix ", "修复", "帮我修复"]
                                .iter()
                                .any(|verb| text.starts_with(verb))
                    })
            });
        let workspace_only = !job.parent.is_empty() && target.is_empty();
        if target.is_empty() && !workspace_only {
            let roster: Vec<_> = registry.devices.iter().map(device_info).collect();
            input.insert(
                1,
                json!({"role":"system","content":format!("Workspace devices: {}",json!(roster))}),
            );
        }
        let mut device_run = DeviceRun::for_devices(
            &self.peers,
            registry.devices.into_iter().filter(|d| {
                !workspace_only && d.connected && (target.is_empty() || d.id == target)
            }),
        );
        let previous: wire::HarnessEventList = self.get(&format!("events/{id}"))?;
        repair_pending &= !previous
            .events
            .iter()
            .any(|event| event.kind == "device_repair" && event.status == "succeeded");
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
            json!({"type":"function","name":"skill_read","description":"Read the requested enabled guide by its exact ID before answering from its contents.","parameters":{"type":"object","properties":{"id":{"type":"string","enum":skills}},"required":["id"]}}),
            json!({"type":"function","name":"publish_image","description":"Attach an existing image to the current reply. Use an absolute path within Work Files (empty device_id) or an authorized device. This returns an attachment ID after reading real bytes.","parameters":{"type":"object","properties":{"path":{"type":"string"},"device_id":{"type":"string"}},"required":["path"]}}),
            json!({"type":"function","name":"ask_user","description":"Stop and ask one concrete question when progress requires a missing detail, human action or permission. Do not repeat blocked actions.","parameters":{"type":"object","properties":{"question":{"type":"string"}},"required":["question"]}}),
            json!({"type":"function","name":"search_knowledge","description":"Find documents in this workspace by title or content.","parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}),
        ];
        let location: wire::LocationState = self.get("location")?;
        if location.enabled
            && let Some(point) = location.point
        {
            input.insert(1,json!({"role":"system","content":format!("Workspace simulated location for all devices: {}, {}. Operating-system delivery status by device: {:?}",point.latitude,point.longitude,location.devices.iter().map(|d|(&d.device_id,&d.status)).collect::<Vec<_>>())}));
        }
        if job.source == "goal" {
            tools.push(json!({"type":"function","name":"complete_goal","description":"Close the current workspace goal only after checking its deliverables. Provide the verified outcome.","parameters":{"type":"object","properties":{"outcome":{"type":"string"}},"required":["outcome"]}}));
        }
        if !is_child && job.parent.is_empty() && job.source != "notification" {
            tools.push(json!({"type":"function","name":"workspace_todos","description":"Read the shared queue or move an item before another item. Omit before_id to move it to the end.","parameters":{"type":"object","properties":{"id":{"type":"string"},"before_id":{"type":"string"}}}}));
            tools.push(json!({"type":"function","name":"set_workspace_location","description":"Set or switch off one simulated location for all workspace devices when requested. Returns each device's delivery result.","parameters":{"type":"object","properties":{"enabled":{"type":"boolean"},"latitude":{"type":"number"},"longitude":{"type":"number"}},"required":["enabled"]}}));
        }
        if !is_child && job.parent.is_empty() {
            tools.push(json!({"type":"function","name":"delegate_tasks","description":"Delegate genuinely independent work in parallel when it benefits the task. Use a different connected device for each device task, or omit the device for workspace work. Returns completed results.","parameters":{"type":"object","properties":{"tasks":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"object","properties":{"task":{"type":"string"},"device_id":{"type":"string"}},"required":["task"]}}},"required":["tasks"]}}));
        }
        if matches!(job.source.as_str(), "user" | "copilot") && !is_child {
            tools.push(json!({"type":"function","name":"workspace_settings","description":"Inspect native settings, list saved model profiles or plans, or apply a user-requested setting. Use a saved Agent id to switch model for future tasks. Operations do not request system permissions.","parameters":{"type":"object","properties":{"operation":{"type":"string","enum":["show","plans","select_agent","goal","liberty","notifications","skill"]},"id":{"type":"string"},"enabled":{"type":"boolean"},"goal":{"type":"string"}},"required":["operation"]}}));
        }
        #[cfg(any(
            target_os = "macos",
            windows,
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "android", target_arch = "aarch64")
        ))]
        if !is_child && job.source != "notification" {
            tools.push(json!({"type":"function","name":"virtual_devices","description":"Inspect local virtual device status or repair an existing failed virtual device in place. Use id from status. This returns readiness or a startup error.","parameters":{"type":"object","properties":{"operation":{"type":"string","enum":["status","repair"]},"id":{"type":"string"}},"required":["operation"]}}));
        }
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
        let first_device = if host || repair_pending {
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
            if let (Some(content), Value::Array(parts)) =
                (input[turn]["content"].as_array_mut(), output)
            {
                device_run.images.insert(target, turn);
                content.extend(parts);
            }
        }
        if host {
            let mut tool = json!({"type":"function","name":"host_desktop","description":format!("Operate the authorized local {} desktop. Use the observation ID and screenshot pixels from this display.",ai_phone_api::platform_name(std::env::consts::OS).unwrap_or(std::env::consts::OS)),"parameters":{"type":"object","properties":{"kind":{"type":"string","enum":["observe","click","double_click","scroll","type","key"]},"observation_id":{"type":"string"},"x":{"type":"integer"},"y":{"type":"integer"},"amount":{"type":"integer"},"text":{"type":"string"},"key":{"type":"string"}},"required":["kind"]}});
            add_desktop_actions(&mut tool);
            tools.push(tool);
        }
        let mut last_rejection = None;
        let mut required_tools = std::cell::OnceCell::new();
        let mut cycles_without_action = 0;
        let mut workspace_results = HashSet::new();
        loop {
            if cycles_without_action == 8 {
                return Ok((
                    "暂时没有取得可确认的进展。请告诉我应继续哪一步。".into(),
                    vec![wire::HarnessEvent {
                        id: uuid::Uuid::now_v7().to_string(),
                        run_id: id.into(),
                        kind: "ask_user".into(),
                        status: "waiting".into(),
                        ..Default::default()
                    }],
                ));
            }
            let started = crate::connection::epoch_ms() as u64;
            let mut timing = Timing::default();
            let selected = if input_pending {
                required_tools.get_or_init(|| {
            let mut choices: Vec<_> = tools.iter().filter(|t| matches!(t["name"].as_str(),
                Some("device_interact" | "delegate_tasks" | "device_audio_input" | "skill_read" | "host_desktop" | "ask_user" | "virtual_devices"))).cloned().collect();
            choices.push(json!({"type":"function","name":"report_no_input","description":"Explain why no input is appropriate.","parameters":{"type":"object","properties":{"reason":{"type":"string"}},"required":["reason"]}}));
            choices
                })
            } else {
                &tools
            };
            let mut body = json!({"input":input,"tools":selected,"reasoning":{"effort":"none"}});
            if repair_pending {
                body["tool_choice"] = json!({"type":"function","name":"virtual_devices"});
            } else if input_pending {
                body["tool_choice"] = json!("required");
            }
            let out = aimanity_model::request_with_text(
                &config,
                &client,
                body,
                token,
                &mut timing,
                |delta| {
                    if !is_child && !input_pending && !repair_pending {
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
                if repair_pending {
                    self.record_tools(n, id, events)?;
                    return Err("本轮未收到设备修复回执".into());
                }
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
            cycles_without_action += 1;
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
                    "publish_image" => {
                        #[derive(Deserialize)]
                        struct ImageInput {
                            path: String,
                            #[serde(default)]
                            device_id: String,
                        }
                        let image: ImageInput = serde_json::from_str(args)?;
                        if !image.device_id.is_empty() {
                            device_run.session(&image.device_id)?;
                            self.claim_device(id, &image.device_id, "publish_image")
                                .await?;
                        }
                        let attachment = self
                            .publish_image(
                                n,
                                wire::FileLocation {
                                    path: image.path,
                                    device_id: image.device_id,
                                },
                            )
                            .await?;
                        event.content = attachment.name.clone();
                        cycles_without_action = 0;
                        Value::String(
                            json!({"id":attachment.id,"filename":attachment.name}).to_string(),
                        )
                    }
                    "ask_user" => {
                        let fields: Value = serde_json::from_str(args)?;
                        let question = fields["question"]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .ok_or("需要说明当前障碍")?;
                        event.kind = "ask_user".into();
                        event.status = "waiting".into();
                        event.content = question.into();
                        event.finished_at_ms = crate::connection::epoch_ms() as u64;
                        events.push(event);
                        return Ok((question.into(), events));
                    }
                    #[cfg(any(
                        target_os = "macos",
                        windows,
                        all(target_os = "linux", target_arch = "x86_64"),
                        all(target_os = "android", target_arch = "aarch64")
                    ))]
                    "virtual_devices" if !is_child && job.source != "notification" => {
                        let args: Value = serde_json::from_str(args)?;
                        let devices = match args["operation"].as_str() {
                            Some("status") => self.registry()?.devices,
                            Some("repair") => {
                                let device_id = args["id"].as_str().ok_or("缺少设备编号")?;
                                let device = self.repair_device(device_id, token).await?;
                                event.kind = "device_repair".into();
                                event.content = device.id.clone();
                                if device.connected {
                                    repair_pending = false;
                                    cycles_without_action = 0;
                                    if device_run.attach(&self.peers, device.clone())? {
                                        let schema = device_run.tool();
                                        if let Some(tool) = tools
                                            .iter_mut()
                                            .find(|t| t["name"] == "device_interact")
                                        {
                                            *tool = schema;
                                        } else {
                                            tools.push(schema);
                                        }
                                        required_tools.take();
                                    }
                                } else {
                                    event.status = "failed".into();
                                    event.content.clone_from(&device.start_error);
                                    if repair_pending {
                                        let error = if device.start_error.is_empty() {
                                            "设备修复后仍未连接".to_owned()
                                        } else {
                                            device.start_error
                                        };
                                        event.finished_at_ms = crate::connection::epoch_ms() as u64;
                                        events.push(event);
                                        self.record_tools(n, id, events)?;
                                        return Err(error.into());
                                    }
                                }
                                vec![device]
                            }
                            _ => return Err("请选择查看状态或修复".into()),
                        };
                        Value::String(json!(devices.iter().filter(|d| !d.paired).map(|d| json!({
                            "id":d.id,"name":d.name,"system":d.kind,"state":d.runtime_state,"online":d.connected,
                            "stage":d.start_stage,"error":self.disconnect_reason(&d.id).unwrap_or_else(|| d.start_error.clone())
                        })).collect::<Vec<_>>()).to_string())
                    }
                    "workspace_settings"
                        if matches!(job.source.as_str(), "user" | "copilot") && !is_child =>
                    {
                        self.workspace_settings(serde_json::from_str(args)?)?
                    }
                    "report_no_input" if input_pending => {
                        let args: Value = serde_json::from_str(args)?;
                        self.record_tools(n, id, events)?;
                        return Err(format!(
                            "本轮未执行设备输入 · {}",
                            args["reason"].as_str().unwrap_or("设备未确认操作")
                        )
                        .into());
                    }
                    "complete_goal" if job.source == "goal" && !input_pending => {
                        let value: Value = serde_json::from_str(args)?;
                        let outcome = value["outcome"]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .ok_or("缺少目标结果")?;
                        event.kind = "goal_complete".into();
                        event.content = outcome.into();
                        events.push(event);
                        return Ok((outcome.into(), events));
                    }
                    "delegate_tasks" if !is_child && job.parent.is_empty() => {
                        #[derive(Deserialize)]
                        struct Task {
                            task: String,
                            #[serde(default)]
                            device_id: String,
                        }
                        #[derive(Deserialize)]
                        struct Batch {
                            tasks: Vec<Task>,
                        }
                        let batch: Batch = serde_json::from_str(args)?;
                        let result = self
                            .delegate(
                                id,
                                call["call_id"].as_str().ok_or("缺少工具编号")?,
                                batch
                                    .tasks
                                    .into_iter()
                                    .map(|t| (t.device_id, t.task))
                                    .collect(),
                                token,
                            )
                            .await
                            .unwrap_or_else(|error| json!({"error":error.to_string()}));
                        if result.as_array().is_some_and(|parts| {
                            parts
                                .iter()
                                .any(|p| p["receipts"].as_u64().unwrap_or_default() > 0)
                        }) {
                            input_pending = false;
                        }
                        if result.get("error").is_some() {
                            event.status = "failed".into();
                        }
                        event.content = result.to_string();
                        Value::String(event.content.clone())
                    }
                    "workspace_todos" if !is_child && job.source != "notification" => {
                        let args: Value = serde_json::from_str(args)?;
                        let list = if let Some(id) = args["id"].as_str() {
                            match self.move_todo(wire::TodoMove {
                                id: id.into(),
                                before_id: args["before_id"].as_str().unwrap_or_default().into(),
                            }) {
                                Ok(list) => list,
                                Err(error) => {
                                    event.status = "failed".into();
                                    event.content = error.to_string();
                                    self.todos()?
                                }
                            }
                        } else {
                            self.todos()?
                        };
                        Value::String(json!(list.items.iter().map(|t|json!({"id":t.id,"title":t.title,"source":t.source,"device":t.device,"status":t.status})).collect::<Vec<_>>()).to_string())
                    }
                    "set_workspace_location" if !is_child && job.source != "notification" => {
                        #[derive(Deserialize)]
                        struct Update {
                            enabled: bool,
                            latitude: Option<f64>,
                            longitude: Option<f64>,
                        }
                        let a: Update = serde_json::from_str(args)?;
                        let state: wire::LocationState = self.get("location")?;
                        let point = a.latitude.zip(a.longitude).map(|(latitude, longitude)| {
                            wire::GeoPoint {
                                latitude,
                                longitude,
                                accuracy_m: 5.,
                            }
                        });
                        let changed = self
                            .set_location(wire::LocationUpdate {
                                expected_revision: state.revision,
                                request_id: format!("{id}/{}", event.id),
                                enabled: a.enabled,
                                point,
                                speed_kph: if state.speed_kph > 0. {
                                    state.speed_kph
                                } else {
                                    5.
                                },
                                ..Default::default()
                            })
                            .await?;
                        Value::String(json!({"enabled":changed.enabled,"devices":changed.devices.iter().map(|d|json!({"id":d.device_id,"status":d.status,"error":d.error})).collect::<Vec<_>>()}).to_string())
                    }
                    "skill_read" => {
                        #[derive(Deserialize)]
                        struct Read {
                            id: String,
                        }
                        let q: Read = serde_json::from_str(args)?;
                        event.content = q.id.clone();
                        Value::String(self.read_active_skill(&q.id)?)
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
                        self.claim_device(id, &feed.device_id, "device_audio_input")
                            .await?;
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
                        let mut data: DesktopInput = serde_json::from_str(args)?;
                        if data.device_id.is_empty()
                            && let [device] = device_run.devices.as_slice()
                        {
                            data.device_id.clone_from(&device.id);
                        }
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
                        if event.status == "not_executed" {
                            let rejected = (device.clone(), serde_json::from_str::<Value>(args)?);
                            if last_rejection.as_ref() == Some(&rejected) {
                                events.push(event);
                                self.record_tools(n, id, events)?;
                                return Err("设备连续拒绝了相同的输入，未执行操作".into());
                            }
                            last_rejection = Some(rejected);
                        } else {
                            last_rejection = None;
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
                if event.status == "succeeded"
                    && (event.kind == "device_input"
                        || (matches!(
                            name,
                            "skill_read"
                                | "search_knowledge"
                                | "workspace_settings"
                                | "workspace_todos"
                                | "set_workspace_location"
                                | "delegate_tasks"
                        ) && workspace_results.insert(
                            BuildHasherDefault::<DefaultHasher>::default()
                                .hash_one((name, result.as_str())),
                        )))
                {
                    cycles_without_action = 0;
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
        if let Some(live) = self.live.lock().unwrap().take_if(|live| live.run == id) {
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
pub(super) fn tool_event(run: &str, name: &str) -> wire::HarnessEvent {
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
