use super::*;
use aimanity_model::ModelConfig;
use serde_json::json;
use std::time::Instant;

fn linked_devices(registry: &wire::RegistryState) -> wire::DeviceLinkState {
    let online: Vec<_> = registry
        .devices
        .iter()
        .filter(|d| crate::connection::reachable(d))
        .collect();
    let mut links = Vec::new();
    for (i, device) in online.iter().enumerate() {
        for peer in &online[i + 1..] {
            let (a, b) = if device.id < peer.id {
                (&device.id, &peer.id)
            } else {
                (&peer.id, &device.id)
            };
            links.push(wire::DeviceLink {
                id: format!("{}:{a}{b}", a.len()),
                source_device_id: a.clone(),
                target_device_id: b.clone(),
                connected: true,
                revision: 1,
            });
        }
    }
    wire::DeviceLinkState {
        links,
        ..Default::default()
    }
}

pub(super) fn revision(actual: u64, expected: u64) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err("内容已更改，请重新打开".into())
    }
}

#[cfg(test)]
#[test]
fn connections_follow_devices_without_storing_pairs() {
    let device = |id: &str, paired, connected, managed| wire::RegisteredDevice {
        id: id.into(),
        paired,
        connected,
        runtime_state: if managed { "managed" } else { "registered" }.into(),
        ..Default::default()
    };
    let mut registry = wire::RegistryState {
        devices: vec![
            device("b", false, true, true),
            device("a", false, true, true),
            device("c", true, true, false),
            device("d", true, false, false),
        ],
        ..Default::default()
    };
    let ids = |s: wire::DeviceLinkState| {
        s.links
            .into_iter()
            .map(|l| (l.source_device_id, l.target_device_id, l.id))
            .collect::<std::collections::BTreeSet<_>>()
    };
    let first = ids(linked_devices(&registry));
    assert_eq!(first.len(), 3);
    registry.devices.reverse();
    assert_eq!(ids(linked_devices(&registry)), first);
    registry
        .devices
        .iter_mut()
        .find(|d| d.id == "c")
        .unwrap()
        .connected = false;
    let links = linked_devices(&registry).links;
    assert_eq!(links.len(), 1);
    assert_eq!(
        (
            links[0].source_device_id.as_str(),
            links[0].target_device_id.as_str()
        ),
        ("a", "b")
    );
}

impl Workspace {
    pub(super) fn dispatch(&self, name: &str, bytes: &[u8]) -> Result<Vec<u8>> {
        macro_rules! req {
            ($t:ident) => {
                wire::$t::decode(bytes)?
            };
        }
        let result = match name {
            "registry/state" => self.registry()?.encode_to_vec(),
            "desktop/state" => linked_devices(&self.registry()?).encode_to_vec(),
            "registry/device-save" => self
                .edit_registry(|state, _| {
                    let q = req!(DeviceSaveRequest);
                    let system = ai_phone_api::guest_system(&q.kind).ok_or("设备类型无效")?;
                    if q.name.trim().is_empty() || q.id.is_empty() {
                        return Err("设备名称或类型无效".into());
                    }
                    let existing = state.devices.iter_mut().find(|d| d.id == q.id);
                    revision(
                        existing.as_ref().map_or(0, |d| d.revision),
                        q.expected_revision,
                    )?;
                    match existing {
                        Some(d) if d.kind == q.kind => {
                            d.name = q.name;
                            d.revision += 1;
                        }
                        Some(_) => return Err("设备类型已确定".into()),
                        None => {
                            fs::create_dir_all(self.device_home(&q.id))?;
                            state.devices.push(wire::RegisteredDevice {
                                id: q.id,
                                revision: 1,
                                name: q.name,
                                kind: q.kind,
                                system: system.into(),
                                runtime_state: "configured".into(),
                                ..Default::default()
                            });
                        }
                    }
                    Ok(())
                })?
                .encode_to_vec(),
            "registry/agent-config-save" => self
                .save_agent(req!(AgentConfigurationSaveRequest))?
                .encode_to_vec(),
            "registry/credential" => {
                let q = req!(ConnectionCredentialRequest);
                let state: wire::RegistryState = self.get("registry")?;
                let model = state
                    .models
                    .iter()
                    .find(|m| m.id == q.id)
                    .ok_or("模型不存在")?;
                revision(model.revision, q.expected_revision)?;
                let d: wire::ModelDraft = self.get(&format!("model/{}", q.id))?;
                wire::ModelCredential {
                    api_key: d.api_key,
                    revision: model.revision,
                }
                .encode_to_vec()
            }
            "registry/agent-select" | "registry/agent-delete" => self
                .edit_registry(|s, tx| {
                    let q = req!(AgentProfileRequest);
                    let i = s
                        .agents
                        .iter()
                        .position(|a| a.id == q.id)
                        .ok_or("Agent 不存在")?;
                    revision(s.agents[i].revision, q.expected_revision)?;
                    if name == "registry/agent-select" {
                        s.active_agent_id = q.id;
                    } else {
                        let removed = s.agents.remove(i);
                        if !s
                            .agents
                            .iter()
                            .any(|a| a.model_connection_id == removed.model_connection_id)
                        {
                            s.models.retain(|m| m.id != removed.model_connection_id);
                            tx.open_table(RECORDS)?.remove(
                                format!("model/{}", removed.model_connection_id).as_str(),
                            )?;
                        }
                        if s.active_agent_id == q.id {
                            s.active_agent_id =
                                s.agents.first().map(|a| a.id.clone()).unwrap_or_default();
                        }
                    }
                    Ok(())
                })?
                .encode_to_vec(),
            "history" => self.history(req!(ChatHistoryRequest))?.encode_to_vec(),
            "send" => self.submit(req!(ChatSendRequest))?.encode_to_vec(),
            "cancel" | "resume" => self
                .control(req!(ChatRunRequest), name == "resume")?
                .encode_to_vec(),
            "workbench/skills" => self.skill_list()?.encode_to_vec(),
            "workbench/skill-update" => self
                .update_skill(req!(SkillUpdateRequest))?
                .encode_to_vec(),
            "workbench/events" => self
                .get::<wire::HarnessEventList>(&format!(
                    "events/{}",
                    req!(HarnessEventRequest).run_id
                ))?
                .encode_to_vec(),
            "location/state" => {
                let mut s: wire::LocationState = self.get("location")?;
                if s.speed_kph == 0. {
                    s.speed_kph = 5.;
                }
                s.encode_to_vec()
            }
            "proactive/todos" => self.todos()?.encode_to_vec(),
            "proactive/move-todo" => self.move_todo(req!(TodoMove))?.encode_to_vec(),
            "proactive/state" => self.proactive_state()?.encode_to_vec(),
            "proactive/update" | "proactive/goal" => self.set_mode(name, bytes)?.encode_to_vec(),
            "workbench/knowledge"
            | "workbench/knowledge-related"
            | "workbench/knowledge-read"
            | "workbench/knowledge-save"
            | "workbench/knowledge-delete"
            | "workbench/knowledge-link" => self.documents(name, bytes)?,
            "workbench/knowledge-image-upload" => {
                let item = self
                    .store_attachment("image".into(), &req!(KnowledgeImageUploadRequest).image)?;
                wire::KnowledgeImage { id: item.id }.encode_to_vec()
            }
            "workbench/memories" => self
                .get::<wire::MemoryList>(&format!("memory/{}", req!(WorkbenchRequest).agent_id))?
                .encode_to_vec(),
            "workbench/memory-save" | "workbench/memory-delete" => self.memory(name, bytes)?,
            _ => return Err(format!("此操作尚未接入本地工作区 · {name}").into()),
        };
        Ok(result)
    }

    pub(crate) fn registry(&self) -> Result<wire::RegistryState> {
        Ok(self.peers.available(self.get("registry")?))
    }
    fn edit_registry(
        &self,
        edit: impl FnOnce(&mut wire::RegistryState, &WriteTransaction) -> Result<()>,
    ) -> Result<wire::RegistryState> {
        let state = self.write_registry(edit)?;
        self.peers.signal();
        self.ready.notify_one();
        Ok(self.peers.available(state))
    }
    pub(super) fn write_registry(
        &self,
        edit: impl FnOnce(&mut wire::RegistryState, &WriteTransaction) -> Result<()>,
    ) -> Result<wire::RegistryState> {
        let tx = self.db.begin_write()?;
        let mut state = read(&tx.open_table(RECORDS)?, "registry")?;
        edit(&mut state, &tx)?;
        put(&tx, "registry", &state)?;
        tx.commit()?;
        Ok(state)
    }

    pub(super) fn model(d: wire::ModelDraft) -> Result<ModelConfig> {
        if d.model.trim().is_empty() {
            return Err("请输入模型名称".into());
        }
        let config = ModelConfig {
            provider: d.provider,
            protocol: d.protocol,
            base_url: d.base_url,
            model: d.model,
            api_key: d.api_key,
            proxy_url: d.proxy_url,
            verified_at_ms: 0,
        };
        let endpoint = reqwest::Url::parse(&config.endpoint()?)?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
        {
            return Err("请输入模型服务的 HTTP 地址".into());
        }
        Ok(config)
    }
    fn resolved(&self, id: &str, mut draft: wire::ModelDraft) -> Result<wire::ModelDraft> {
        if draft.use_saved_key {
            let saved: wire::ModelDraft = self.get(&format!("model/{id}"))?;
            if saved.base_url != draft.base_url || saved.provider != draft.provider {
                return Err("请为新地址填写密钥".into());
            }
            draft.api_key = saved.api_key;
            draft.use_saved_key = false;
        }
        Ok(draft)
    }
    pub(super) async fn probe(
        &self,
        q: wire::ConnectionTestRequest,
    ) -> Result<wire::ModelTestResult> {
        let draft = self.resolved(&q.id, q.draft.ok_or("缺少模型配置")?)?;
        let config = Self::model(draft.clone())?;
        let at = Instant::now();
        let client = self.client(&config.proxy_url)?;
        let output=aimanity_model::request_tracked(&config,&client,json!({"input":[{"role":"user","content":"Reply with OK."}],"reasoning":{"effort":"none"},"max_output_tokens":128}),&self.shutdown,&mut Default::default()).await?;
        if aimanity_model::text(&output).trim().is_empty() {
            return Err("模型未返回文本".into());
        }
        let ticket = uuid::Uuid::now_v7().to_string();
        self.verified
            .lock()
            .unwrap()
            .insert(q.id, (ticket.clone(), q.expected_revision, draft));
        Ok(wire::ModelTestResult {
            ready: true,
            verification_id: ticket,
            checks: vec![wire::CapabilityCheck {
                name: "模型连接".into(),
                passed: true,
                duration_ms: at.elapsed().as_millis() as u64,
                ..Default::default()
            }],
        })
    }
    fn save_agent(&self, q: wire::AgentConfigurationSaveRequest) -> Result<wire::RegistryState> {
        let a = q.agent.ok_or("缺少 Agent")?;
        if a.id.is_empty() || a.name.trim().is_empty() {
            return Err("请输入 Agent 名称".into());
        }
        self.edit_registry(|s, tx| {
            let i = s.agents.iter().position(|old| old.id == a.id);
            revision(i.map_or(0, |i| s.agents[i].revision), a.expected_revision)?;
            if let Some(m) = q.connection {
                if m.id != a.model_connection_id {
                    return Err("模型与 Agent 配置不匹配".into());
                }
                let j = s.models.iter().position(|old| old.id == m.id);
                revision(j.map_or(0, |i| s.models[i].revision), m.expected_revision)?;
                let d = self.resolved(&m.id, m.draft.ok_or("缺少模型配置")?)?;
                let verified = self.verified.lock().unwrap();
                if !verified.get(&m.id).is_some_and(|(id, rev, draft)| {
                    id == &m.verification_id && *rev == m.expected_revision && draft == &d
                }) {
                    return Err("请重新检测此模型".into());
                }
                let mut config = Self::model(d.clone())?;
                config.verified_at_ms = crate::connection::epoch_ms() as u64;
                let n = m.expected_revision + 1;
                let model = wire::RegisteredModel {
                    id: m.id.clone(),
                    revision: n,
                    name: m.name,
                    profile: Some(config.profile(n)),
                };
                put(tx, &format!("model/{}", m.id), &d)?;
                if let Some(j) = j {
                    s.models[j] = model
                } else {
                    s.models.push(model)
                }
            }
            if !s.models.iter().any(|m| m.id == a.model_connection_id) {
                return Err("模型不存在".into());
            }
            let agent = wire::RegisteredAgent {
                id: a.id.clone(),
                revision: a.expected_revision + 1,
                name: a.name,
                model_connection_id: a.model_connection_id,
                instructions: a.instructions,
            };
            if let Some(i) = i {
                s.agents[i] = agent
            } else {
                s.agents.push(agent)
            }
            if s.active_agent_id.is_empty() {
                s.active_agent_id = a.id;
            }
            Ok(())
        })
    }
    fn memory(&self, name: &str, bytes: &[u8]) -> Result<Vec<u8>> {
        let (agent, id, expected, content) = if name == "workbench/memory-save" {
            let q = wire::MemorySaveRequest::decode(bytes)?;
            (q.agent_id, q.id, q.expected_revision, Some(q.content))
        } else {
            let q = wire::MemoryDeleteRequest::decode(bytes)?;
            (q.agent_id, q.id, q.expected_revision, None)
        };
        let key = format!("memory/{agent}");
        let tx = self.db.begin_write()?;
        let mut list: wire::MemoryList = read(&tx.open_table(RECORDS)?, &key)?;
        let i = list.memories.iter().position(|m| m.id == id);
        revision(i.map_or(0, |i| list.memories[i].revision), expected)?;
        let result = if let Some(content) = content {
            let item = wire::ModelMemory {
                id: if id.is_empty() {
                    uuid::Uuid::now_v7().to_string()
                } else {
                    id
                },
                content,
                revision: expected + 1,
            };
            let result = item.encode_to_vec();
            if let Some(i) = i {
                list.memories[i] = item
            } else {
                list.memories.push(item)
            }
            result
        } else {
            list.memories.remove(i.ok_or("记忆不存在")?);
            list.encode_to_vec()
        };
        put(&tx, &key, &list)?;
        tx.commit()?;
        Ok(result)
    }
}
