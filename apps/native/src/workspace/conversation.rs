use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub(super) struct LiveReply {
    pub run: String,
    pub text: String,
    last_sent: Instant,
}

#[derive(Clone, prost::Message)]
pub(super) struct Job {
    #[prost(message, optional, tag = "1")]
    pub(super) request: Option<wire::ChatSendRequest>,
    #[prost(message, optional, tag = "2")]
    pub(super) model: Option<wire::ModelDraft>,
    #[prost(string, tag = "3")]
    pub(super) instructions: String,
    #[prost(bool, tag = "4")]
    paused: bool,
}

impl Workspace {
    pub(super) fn show_text(&self, run: &str, delta: &str) {
        if delta.is_empty() {
            return;
        }
        let mut live = self.live.lock().unwrap();
        let entry = live.get_or_insert_with(|| LiveReply {
            run: run.into(),
            text: String::new(),
            last_sent: Instant::now(),
        });
        let show = entry.text.is_empty() || entry.last_sent.elapsed() >= Duration::from_millis(40);
        entry.text.push_str(delta);
        if show {
            entry.last_sent = Instant::now();
            self.changed();
        }
    }
    pub(crate) fn history(&self, q: wire::ChatHistoryRequest) -> Result<wire::ChatState> {
        let tx = self.db.begin_read()?;
        let mut state: wire::ChatState = read(&tx.open_table(RECORDS)?, "chat")?;
        let messages = tx.open_table(MESSAGES)?;
        let ids = tx.open_table(IDS)?;
        let before = if q.before_message_id.is_empty() {
            u64::MAX
        } else {
            ids.get(q.before_message_id.as_str())?
                .ok_or("消息不存在")?
                .value()
        };
        let size = (q.limit as usize).clamp(1, 80);
        let mut rows = messages.range(..before)?.rev();
        let mut page: Vec<_> = rows
            .by_ref()
            .take(size)
            .map(|e| Ok(wire::ChatMessage::decode(e?.1.value())?))
            .collect::<Result<_>>()?;
        let older = rows.next().transpose()?.is_some();
        page.reverse();
        state.message_window = Some(wire::ChatMessageWindow {
            kind: if q.before_message_id.is_empty() {
                wire::ChatWindowKind::Tail
            } else {
                wire::ChatWindowKind::Before
            } as i32,
            next_before_message_id: page.first().map(|m| m.id.clone()).unwrap_or_default(),
            has_older: older,
            page_message_ids: page.iter().map(|m| m.id.clone()).collect(),
        });
        for key in q.watch_message_ids.into_iter().chain(q.receipt_request_ids) {
            if let Some(n) = ids.get(key.as_str())?
                && !page.iter().any(|m| m.ordinal == n.value())
                && let Some(v) = messages.get(n.value())?
            {
                page.push(wire::ChatMessage::decode(v.value())?)
            }
        }
        page.sort_by_key(|m| m.ordinal);
        if let Some(active) = self.live.lock().unwrap().as_ref()
            && let Some(reply) = page
                .iter_mut()
                .find(|m| m.run_id == active.run && m.role == "assistant" && m.active)
        {
            if !reply.text.is_empty() {
                reply.text.push_str("\n\n");
            }
            reply.text.push_str(&active.text);
        }
        state.messages = page;
        Ok(state)
    }
    pub(super) fn submit(&self, mut q: wire::ChatSendRequest) -> Result<wire::ChatState> {
        if q.request_id.is_empty() || (q.text.trim().is_empty() && q.attachment_ids.is_empty()) {
            return Err("请输入消息或添加附件".into());
        }
        let tx = self.db.begin_write()?;
        let records = tx.open_table(RECORDS)?;
        let mut head: wire::ChatState = read(&records, "chat")?;
        if !q.conversation_id.is_empty() && q.conversation_id != head.conversation_id {
            return Err("会话已更改".into());
        }
        let mut messages = tx.open_table(MESSAGES)?;
        let mut ids = tx.open_table(IDS)?;
        if let Some(n) = ids.get(q.request_id.as_str())? {
            head.messages = vec![wire::ChatMessage::decode(
                messages.get(n.value())?.ok_or("消息不存在")?.value(),
            )?];
            return Ok(head);
        }
        let registry: wire::RegistryState = read(&records, "registry")?;
        if q.agent_id.is_empty() {
            q.agent_id = registry.active_agent_id;
        }
        let agent = registry
            .agents
            .iter()
            .find(|a| a.id == q.agent_id)
            .ok_or("请先配置模型")?;
        let model: wire::ModelDraft =
            read(&records, &format!("model/{}", agent.model_connection_id))?;
        if !q.device_id.is_empty()
            && !registry
                .devices
                .iter()
                .any(|device| device.id == q.device_id && device.paired)
        {
            return Err("当前工作区未授权这台设备".into());
        }
        let n = messages.last()?.map_or(1, |(k, _)| k.value() + 1);
        let mut attachments = Vec::with_capacity(q.attachment_ids.len());
        for id in &q.attachment_ids {
            let a: wire::ChatAttachment = read(&records, &format!("attachment/{id}"))?;
            if a.id.is_empty() {
                return Err("附件不存在".into());
            }
            attachments.push(a);
        }
        let user = wire::ChatMessage {
            id: q.request_id.clone(),
            request_id: q.request_id.clone(),
            role: "user".into(),
            text: q.text.clone(),
            attachments,
            ordinal: n,
            run_id: q.request_id.clone(),
            voice: q.voice.clone(),
            ..Default::default()
        };
        let reply = wire::ChatMessage {
            id: uuid::Uuid::now_v7().to_string(),
            role: "assistant".into(),
            run_id: q.request_id.clone(),
            ordinal: n + 1,
            active: true,
            ..Default::default()
        };
        for m in [&user, &reply] {
            messages.insert(m.ordinal, m.encode_to_vec().as_slice())?;
            ids.insert(m.id.as_str(), m.ordinal)?;
        }
        head.revision += 1;
        head.run_id = q.request_id.clone();
        head.run_status = "queued".into();
        head.error.clear();
        head.notice_kind = 0;
        head.can_pause = true;
        head.agent_id = agent.id.clone();
        head.agent_name = agent.name.clone();
        drop(records);
        drop(messages);
        drop(ids);
        let request_id = q.request_id.clone();
        tx.open_table(JOBS)?.insert(
            n,
            Job {
                request: Some(q),
                model: Some(model),
                instructions: agent.instructions.clone(),
                paused: false,
            }
            .encode_to_vec()
            .as_slice(),
        )?;
        put(&tx, "chat", &head)?;
        tx.open_table(PENDING)?.remove(request_id.as_str())?;
        tx.commit()?;
        self.changed();
        self.ready.notify_one();
        head.messages = vec![user, reply];
        Ok(head)
    }

    pub(super) fn next_job(&self) -> Result<Option<(u64, Job)>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(JOBS)?;
        for row in table.iter()? {
            let (key, data) = row?;
            let job = Job::decode(data.value())?;
            if !job.paused {
                return Ok(Some((key.value(), job)));
            }
        }
        Ok(None)
    }
    pub(super) fn control(&self, r: wire::ChatRunRequest, resume: bool) -> Result<wire::ChatState> {
        let tx = self.db.begin_write()?;
        let n = tx
            .open_table(IDS)?
            .get(r.run_id.as_str())?
            .ok_or("任务不存在")?
            .value();
        let mut jobs = tx.open_table(JOBS)?;
        let mut job = Job::decode(jobs.get(n)?.ok_or("任务已结束")?.value())?;
        let status = if resume {
            "queued"
        } else if r.pause {
            "paused"
        } else {
            "canceled"
        };
        let mut active = self.run.lock().unwrap();
        if !resume {
            self.host.lock().unwrap().take_if(|(id, _)| id == &r.run_id);
        }
        if !resume
            && let Some((id, token)) = &*active
            && id == &r.run_id
        {
            token.cancel();
        }
        if !resume && !r.pause {
            jobs.remove(n)?;
        } else {
            job.paused = !resume;
            jobs.insert(n, job.encode_to_vec().as_slice())?;
        }
        let mut head: wire::ChatState = read(&tx.open_table(RECORDS)?, "chat")?;
        head.run_id = r.run_id;
        head.revision += 1;
        head.run_status = status.into();
        head.can_pause = resume;
        let mut msgs = tx.open_table(MESSAGES)?;
        let mut m = wire::ChatMessage::decode(msgs.get(n + 1)?.ok_or("消息不存在")?.value())?;
        m.active = resume;
        msgs.insert(n + 1, m.encode_to_vec().as_slice())?;
        drop(msgs);
        drop(jobs);
        put(&tx, "chat", &head)?;
        tx.commit()?;
        active.take_if(|(id, _)| id == &head.run_id);
        drop(active);
        self.changed();
        if resume {
            self.ready.notify_one()
        }
        head.messages = vec![m];
        Ok(head)
    }

    pub(super) async fn execute(&self, (n, job): (u64, Job)) -> Result<()> {
        let q = job.request.as_ref().ok_or("任务缺少消息")?;
        let id = q.request_id.clone();
        let token = self.shutdown.child_token();
        {
            let mut running = self.run.lock().unwrap();
            let tx = self.db.begin_read()?;
            let table = tx.open_table(JOBS)?;
            let Some(current) = table.get(n)? else {
                return Ok(());
            };
            if Job::decode(current.value())?.paused {
                return Ok(());
            }
            *running = Some((id.clone(), token.clone()));
        }
        let started = crate::connection::epoch_ms() as u64;
        let output = self.answer(n, &id, &job, &token).await;
        if token.is_cancelled() {
            self.live.lock().unwrap().take();
            return Ok(());
        }
        let tx = self.db.begin_write()?;
        let mut running = self.run.lock().unwrap();
        if token.is_cancelled() {
            self.live.lock().unwrap().take();
            return Ok(());
        }
        let mut msgs = tx.open_table(MESSAGES)?;
        let mut reply = wire::ChatMessage::decode(msgs.get(n + 1)?.ok_or("消息不存在")?.value())?;
        let mut head: wire::ChatState = read(&tx.open_table(RECORDS)?, "chat")?;
        let (status, error) = match output {
            Ok((text, events)) => {
                if !reply.text.is_empty() {
                    reply.text.push_str("\n\n");
                }
                reply.text.push_str(&text);
                let mut log: wire::HarnessEventList =
                    read(&tx.open_table(RECORDS)?, &format!("events/{id}"))?;
                for mut event in events {
                    event.message_id = reply.id.clone();
                    log.events.push(event);
                }
                put(&tx, &format!("events/{id}"), &log)?;
                ("succeeded", String::new())
            }
            Err(error) => {
                let error = error.to_string();
                let mut log: wire::HarnessEventList =
                    read(&tx.open_table(RECORDS)?, &format!("events/{id}"))?;
                log.events.push(wire::HarnessEvent {
                    id: uuid::Uuid::now_v7().to_string(),
                    run_id: id.clone(),
                    message_id: reply.id.clone(),
                    kind: "task".into(),
                    title: "任务".into(),
                    content: error.clone(),
                    status: "failed".into(),
                    created_at_ms: started,
                    finished_at_ms: crate::connection::epoch_ms() as u64,
                    ..Default::default()
                });
                put(&tx, &format!("events/{id}"), &log)?;
                reply.system_notice = error.clone();
                ("failed", error)
            }
        };

        head.run_id = id.clone();
        head.run_status = status.into();
        head.revision += 1;
        head.error = error;
        head.can_pause = false;
        head.notice_kind = if status == "failed" {
            wire::RunNoticeKind::SystemFailure as i32
        } else {
            0
        };
        reply.active = false;
        reply.has_activity = true;
        msgs.insert(n + 1, reply.encode_to_vec().as_slice())?;
        drop(msgs);
        tx.open_table(JOBS)?.remove(n)?;
        put(&tx, "chat", &head)?;
        tx.commit()?;
        self.live.lock().unwrap().take();
        running.take();
        drop(running);
        self.changed();
        Ok(())
    }

    pub(super) fn context(&self, n: u64, instructions: &str) -> Result<(Vec<Value>, bool)> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(MESSAGES)?;
        let mut rows = table
            .range(..=n)?
            .rev()
            .take(80)
            .map(|r| Ok(wire::ChatMessage::decode(r?.1.value())?))
            .collect::<Result<Vec<_>>>()?;
        let mut repeat_input = false;
        if let Some(request) = rows.first() {
            let records = tx.open_table(RECORDS)?;
            for prior in rows.iter().skip(1).filter(|m| {
                m.role == "user" && m.text == request.text && m.attachments == request.attachments
            }) {
                let events: wire::HarnessEventList =
                    read(&records, &format!("events/{}", prior.run_id))?;
                if events
                    .events
                    .iter()
                    .any(|e| e.kind == "device_input" && e.status == "succeeded")
                {
                    repeat_input = true;
                    break;
                }
            }
        }
        rows.reverse();
        let mut input = vec![
            json!({"role":"system","content":"Carry out the current user's device instructions through the available tools. Every user message starts a new run, even when it repeats an earlier instruction. Previous replies and screen observations are not input receipts. Report actions only after their tool confirms input in this run."}),
        ];
        if !instructions.trim().is_empty() {
            input.push(json!({"role":"system","content":instructions}));
        }
        for m in rows {
            if m.text.is_empty() && m.attachments.is_empty() {
                continue;
            }
            let mut parts = vec![];
            if !m.text.is_empty() {
                parts.push(json!({"type":if m.role=="assistant"{"output_text"}else{"input_text"},"text":m.text}));
            }
            for a in m.attachments {
                if m.ordinal != n
                    || !(a.media_type.starts_with("image/") || a.media_type == "text/plain")
                {
                    parts.push(json!({"type":"input_text","text":format!("Attachment: {} ({}, {} bytes)",a.name,a.media_type,a.size_bytes)}));
                    continue;
                }
                let bytes = self.attachment(&a.id)?;
                if a.media_type.starts_with("image/") {
                    parts.push(json!({"type":"input_image","image_url":format!("data:{};base64,{}",a.media_type,STANDARD.encode(bytes))}));
                } else {
                    parts.push(json!({"type":"input_text","text":format!("{}\n{}",a.name,String::from_utf8(bytes)?)}));
                }
            }
            input.push(json!({"role":m.role,"content":parts}));
        }
        Ok((input, repeat_input))
    }
}
