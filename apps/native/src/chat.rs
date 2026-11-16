use crate::{
    connection::Connection,
    controls::{Icon, icon_button},
    style,
};
use ai_phone_api::wire;
use iced::{
    Element, Length, Subscription, Task, keyboard,
    widget::{
        button, column, container, image, markdown, row, scrollable, space, text, text_editor,
    },
};
use prost::Message as _;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

#[derive(Clone, Debug)]
pub enum Message {
    Edit(text_editor::Action),
    Send,
    Sent(String, Result<wire::ChatState, String>),
    Retry(String),
    History(Result<wire::ChatState, String>),
    Older,
    Scroll(bool),
    Pick,
    Files(Vec<PathBuf>),
    Uploaded(String, Result<wire::ChatAttachment, String>),
    Remove(String),
    Attachment(wire::ChatAttachment),
    Preview(wire::ChatAttachment, Result<image::Handle, String>),
    ClosePreview,
    Download(wire::ChatAttachment),
    Downloaded(Result<(), String>),
    Command(Command),
    Controlled(Result<wire::ChatState, String>),
    Trace(String),
    TraceDetail(String),
    TraceLoaded(String, Result<wire::HarnessEventList, String>),
    Poll,
    Link(String),
    Opened(Result<(), String>),
    Restored(Result<Vec<wire::ChatState>, String>),
}
#[derive(Clone, Copy, Debug)]
pub enum Command {
    Pause,
    Stop,
    Resume,
}
struct Pending {
    request: wire::ChatSendRequest,
    message: wire::ChatMessage,
    error: Option<String>,
    sending: bool,
}
struct Upload {
    id: String,
    name: String,
    attachment: Option<wire::ChatAttachment>,
    error: Option<String>,
}
struct Trace {
    expanded: bool,
    loading: bool,
    loaded: bool,
    events: Vec<wire::HarnessEvent>,
    summaries: HashMap<String, Vec<markdown::Item>>,
}
pub struct Chat {
    pub state: wire::ChatState,
    draft: text_editor::Content,
    pending: Vec<Pending>,
    uploads: Vec<Upload>,
    markdown: HashMap<String, Vec<markdown::Item>>,
    traces: HashMap<String, Trace>,
    expanded: HashSet<String>,
    previews: HashMap<String, Option<image::Handle>>,
    pub preview: Option<wire::ChatAttachment>,
    pub error: String,
    before: String,
    has_older: bool,
    loading_older: bool,
    following: bool,
    control_busy: bool,
    outbox: PathBuf,
}
impl Chat {
    pub fn new() -> Self {
        Self {
            state: Default::default(),
            draft: text_editor::Content::new(),
            pending: Vec::new(),
            uploads: Vec::new(),
            markdown: HashMap::new(),
            traces: HashMap::new(),
            expanded: HashSet::new(),
            previews: HashMap::new(),
            preview: None,
            error: String::new(),
            before: String::new(),
            has_older: false,
            loading_older: false,
            following: true,
            control_busy: false,
            outbox: PathBuf::new(),
        }
    }
    pub fn restore(&mut self, root: &str) -> Task<Message> {
        self.outbox = PathBuf::from(root).join("data/native/outbox");
        let folder = self.outbox.clone();
        Task::perform(
            async move {
                tokio::fs::create_dir_all(&folder)
                    .await
                    .map_err(|e| e.to_string())?;
                let mut directory = tokio::fs::read_dir(folder)
                    .await
                    .map_err(|e| e.to_string())?;
                let mut pending = Vec::new();
                while let Some(entry) = directory.next_entry().await.map_err(|e| e.to_string())? {
                    if entry.path().extension().is_some_and(|v| v == "pb") {
                        let bytes = tokio::fs::read(entry.path())
                            .await
                            .map_err(|e| e.to_string())?;
                        pending.push(
                            wire::ChatState::decode(bytes.as_slice()).map_err(|e| e.to_string())?,
                        );
                    }
                }
                Ok(pending)
            },
            Message::Restored,
        )
    }
    pub fn accept(&mut self, mut next: wire::ChatState, client: &Connection) -> Task<Message> {
        let mut tasks = Vec::new();
        let receipt_ids: HashSet<_> = next
            .messages
            .iter()
            .map(|m| m.request_id.as_str())
            .collect();
        self.pending.retain(|p| {
            if receipt_ids.contains(p.request.request_id.as_str()) {
                let path = self.outbox.join(format!("{}.pb", p.request.request_id));
                tasks.push(Task::perform(
                    async move {
                        tokio::fs::remove_file(path)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::Downloaded,
                ));
                false
            } else {
                true
            }
        });
        if let Some(page) = &next.message_window
            && (self.before.is_empty() || page.kind() == wire::ChatWindowKind::Before)
        {
            self.before = page.next_before_message_id.clone();
            self.has_older = page.has_older;
        }
        for incoming in next.messages.drain(..) {
            let old = self.state.messages.iter().position(|m| m.id == incoming.id);
            if old.is_some() && next.revision < self.state.revision {
                continue;
            }
            if old.is_none_or(|i| self.state.messages[i].text != incoming.text) {
                self.markdown.insert(
                    incoming.id.clone(),
                    markdown::parse(&incoming.text).collect(),
                );
            }
            for file in &incoming.attachments {
                if file.media_type.starts_with("image/") && !self.previews.contains_key(&file.id) {
                    let client = client.clone();
                    let file = file.clone();
                    let id = file.id.clone();
                    self.previews.insert(id.clone(), None);
                    tasks.push(Task::perform(
                        async move {
                            let bytes = client.attachment(&id).await?;
                            image_decode(&bytes)
                        },
                        move |r| Message::Preview(file.clone(), r),
                    ));
                }
            }
            if incoming.role == "assistant" && incoming.has_activity {
                let live = incoming.active
                    && incoming.run_id == next.run_id
                    && matches!(next.run_status.as_str(), "queued" | "running");
                let finished = old.is_some_and(|i| self.is_live(&self.state.messages[i])) && !live;
                let trace = self
                    .traces
                    .entry(incoming.id.clone())
                    .or_insert_with(|| Trace {
                        expanded: live,
                        loading: false,
                        loaded: false,
                        events: Vec::new(),
                        summaries: HashMap::new(),
                    });
                if finished {
                    trace.loaded = false;
                }
            }
            match old {
                Some(i) => self.state.messages[i] = incoming,
                None => self.state.messages.push(incoming),
            }
        }
        self.state.messages.sort_by_key(|m| m.ordinal);
        if next.revision >= self.state.revision {
            next.messages = std::mem::take(&mut self.state.messages);
            self.state = next;
            if self.state.notice_kind() == wire::RunNoticeKind::SystemFailure {
                self.error = self.state.error.clone();
            }
        }
        tasks.push(self.refresh_traces(client));
        if self.following {
            tasks.push(iced::widget::operation::snap_to(
                "shared-chat",
                scrollable::RelativeOffset::END,
            ));
        }
        Task::batch(tasks)
    }
    fn send_pending(&mut self, id: &str, client: &Connection) -> Task<Message> {
        let pending = self
            .pending
            .iter_mut()
            .find(|p| p.request.request_id == id)
            .expect("pending message");
        pending.sending = true;
        pending.error = None;
        let snapshot = wire::ChatState {
            conversation_id: pending.request.conversation_id.clone(),
            device_id: pending.request.device_id.clone(),
            messages: vec![pending.message.clone()],
            ..Default::default()
        };
        let path = self.outbox.join(format!("{id}.pb"));
        let temporary = path.with_extension("tmp");
        let request = pending.request.clone();
        let client = client.clone();
        let id = id.to_string();
        Task::perform(
            async move {
                tokio::fs::write(&temporary, snapshot.encode_to_vec())
                    .await
                    .map_err(|e| e.to_string())?;
                tokio::fs::rename(temporary, path)
                    .await
                    .map_err(|e| e.to_string())?;
                client.rpc("assistant/send", request).await
            },
            move |r| Message::Sent(id.clone(), r),
        )
    }
    fn refresh_traces(&mut self, client: &Connection) -> Task<Message> {
        let mut tasks = Vec::new();
        for message in &self.state.messages {
            let live = self.is_live(message);
            if let Some(trace) = self.traces.get_mut(&message.id)
                && trace.expanded
                && !trace.loading
                && (!trace.loaded || live)
            {
                trace.loading = true;
                let after_ms = trace
                    .events
                    .iter()
                    .map(|e| e.finished_at_ms.max(e.created_at_ms))
                    .max()
                    .unwrap_or(0)
                    .saturating_sub(1);
                let input = wire::HarnessEventRequest {
                    run_id: message.run_id.clone(),
                    message_id: message.id.clone(),
                    after_ms,
                };
                let client = client.clone();
                let id = message.id.clone();
                tasks.push(Task::perform(
                    async move { client.rpc("workbench/events", input).await },
                    move |r| Message::TraceLoaded(id.clone(), r),
                ));
            }
        }
        Task::batch(tasks)
    }
    fn is_live(&self, message: &wire::ChatMessage) -> bool {
        message.active
            && message.run_id == self.state.run_id
            && matches!(self.state.run_status.as_str(), "queued" | "running")
    }
    pub fn subscription(&self) -> Subscription<Message> {
        if self
            .state
            .messages
            .iter()
            .any(|m| self.is_live(m) && self.traces.get(&m.id).is_some_and(|t| t.expanded))
        {
            iced::time::every(std::time::Duration::from_secs(1)).map(|_| Message::Poll)
        } else {
            Subscription::none()
        }
    }
    pub fn update(
        &mut self,
        message: Message,
        client: &Connection,
        device_id: &str,
    ) -> Task<Message> {
        match message {
            Message::Edit(action) => self.draft.perform(action),
            Message::Send => {
                if (self.draft.text().trim().is_empty() && self.uploads.is_empty())
                    || self.uploads.iter().any(|u| u.attachment.is_none())
                {
                    return Task::none();
                }
                let id = uuid::Uuid::now_v7().to_string();
                let attachments = self
                    .uploads
                    .drain(..)
                    .map(|u| u.attachment.expect("completed upload"))
                    .collect::<Vec<_>>();
                let text = self.draft.text();
                self.draft = text_editor::Content::new();
                let request = wire::ChatSendRequest {
                    conversation_id: self.state.conversation_id.clone(),
                    device_id: device_id.into(),
                    text: text.clone(),
                    attachment_ids: attachments.iter().map(|a| a.id.clone()).collect(),
                    request_id: id.clone(),
                    ..Default::default()
                };
                self.pending.push(Pending {
                    message: wire::ChatMessage {
                        id: id.clone(),
                        request_id: id.clone(),
                        role: "user".into(),
                        text,
                        attachments,
                        ..Default::default()
                    },
                    request,
                    error: None,
                    sending: false,
                });
                self.following = true;
                self.error.clear();
                return self.send_pending(&id, client);
            }
            Message::Retry(id) => return self.send_pending(&id, client),
            Message::Sent(id, result) => match result {
                Ok(next) => return self.accept(next, client),
                Err(error) => {
                    self.error = error.clone();
                    if let Some(p) = self.pending.iter_mut().find(|p| p.request.request_id == id) {
                        p.sending = false;
                        p.error = Some(error);
                    }
                }
            },
            Message::History(result) => {
                self.loading_older = false;
                match result {
                    Ok(state) => return self.accept(state, client),
                    Err(e) => self.error = e,
                }
            }
            Message::Restored(result) => match result {
                Ok(records) => {
                    let mut tasks = Vec::new();
                    for record in records {
                        for message in record.messages {
                            if self
                                .state
                                .messages
                                .iter()
                                .any(|m| m.request_id == message.request_id)
                            {
                                let path = self.outbox.join(format!("{}.pb", message.request_id));
                                tasks.push(Task::perform(
                                    async move {
                                        tokio::fs::remove_file(path)
                                            .await
                                            .map_err(|e| e.to_string())
                                    },
                                    Message::Downloaded,
                                ));
                                continue;
                            }
                            let request = wire::ChatSendRequest {
                                conversation_id: record.conversation_id.clone(),
                                device_id: record.device_id.clone(),
                                text: message.text.clone(),
                                attachment_ids: message
                                    .attachments
                                    .iter()
                                    .map(|a| a.id.clone())
                                    .collect(),
                                request_id: message.request_id.clone(),
                                ..Default::default()
                            };
                            self.pending.push(Pending {
                                request,
                                message,
                                error: Some("发送中断".into()),
                                sending: false,
                            });
                        }
                    }
                    return Task::batch(tasks);
                }
                Err(e) => self.error = e,
            },
            Message::Older => {
                self.loading_older = true;
                let client = client.clone();
                let input = wire::ChatHistoryRequest {
                    conversation_id: self.state.conversation_id.clone(),
                    limit: 80,
                    before_message_id: self.before.clone(),
                    ..Default::default()
                };
                return Task::perform(
                    async move { client.rpc("assistant/history", input).await },
                    Message::History,
                );
            }
            Message::Scroll(following) => self.following = following,
            Message::Pick => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("添加附件")
                            .pick_files()
                            .await
                            .unwrap_or_default()
                            .into_iter()
                            .map(|f| f.path().to_path_buf())
                            .collect()
                    },
                    Message::Files,
                );
            }
            Message::Files(paths) => {
                if self.uploads.len() + paths.len() > 8 {
                    self.error = "每条消息最多 8 个附件".into();
                    return Task::none();
                }
                let mut tasks = Vec::new();
                for path in paths {
                    let id = uuid::Uuid::now_v7().to_string();
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.uploads.push(Upload {
                        id: id.clone(),
                        name,
                        attachment: None,
                        error: None,
                    });
                    let client = client.clone();
                    tasks.push(Task::perform(
                        async move { client.upload(&path).await },
                        move |r| Message::Uploaded(id.clone(), r),
                    ));
                }
                return Task::batch(tasks);
            }
            Message::Uploaded(id, result) => {
                if let Some(upload) = self.uploads.iter_mut().find(|u| u.id == id) {
                    match result {
                        Ok(file) => upload.attachment = Some(file),
                        Err(e) => {
                            upload.error = Some(e.clone());
                            self.error = e;
                        }
                    }
                }
            }
            Message::Remove(id) => self.uploads.retain(|u| u.id != id),
            Message::Attachment(file) => {
                if !file.media_type.starts_with("image/") {
                    return self.update(Message::Download(file), client, device_id);
                }
                self.preview = Some(file.clone());
                if !self.previews.contains_key(&file.id) {
                    let client = client.clone();
                    let id = file.id.clone();
                    self.previews.insert(id.clone(), None);
                    return Task::perform(
                        async move {
                            let bytes = client.attachment(&id).await?;
                            image_decode(&bytes)
                        },
                        move |r| Message::Preview(file.clone(), r),
                    );
                }
            }
            Message::Preview(file, result) => match result {
                Ok(handle) => {
                    self.previews.insert(file.id, Some(handle));
                }
                Err(error) => {
                    self.previews.remove(&file.id);
                    self.error = error;
                }
            },
            Message::ClosePreview => self.preview = None,
            Message::Download(file) => {
                let client = client.clone();
                return Task::perform(
                    async move {
                        if let Some(target) = rfd::AsyncFileDialog::new()
                            .set_file_name(&file.name)
                            .save_file()
                            .await
                        {
                            let bytes = client.attachment(&file.id).await?;
                            target.write(&bytes).await.map_err(|e| e.to_string())?;
                        }
                        Ok(())
                    },
                    Message::Downloaded,
                );
            }
            Message::Downloaded(result) | Message::Opened(result) => {
                if let Err(e) = result {
                    self.error = e;
                }
            }
            Message::Command(command) => {
                self.error.clear();
                self.control_busy = true;
                let input = wire::ChatRunRequest {
                    run_id: self.state.run_id.clone(),
                    pause: matches!(command, Command::Pause),
                };
                let client = client.clone();
                return Task::perform(
                    async move {
                        client
                            .rpc(
                                if matches!(command, Command::Resume) {
                                    "assistant/resume"
                                } else {
                                    "assistant/cancel"
                                },
                                input,
                            )
                            .await
                    },
                    Message::Controlled,
                );
            }
            Message::Controlled(result) => {
                self.control_busy = false;
                match result {
                    Ok(next) => return self.accept(next, client),
                    Err(e) => self.error = e,
                }
            }
            Message::Trace(id) => {
                if let Some(trace) = self.traces.get_mut(&id) {
                    trace.expanded = !trace.expanded;
                }
                return self.refresh_traces(client);
            }
            Message::TraceDetail(id) => {
                if !self.expanded.remove(&id) {
                    self.expanded.insert(id);
                }
            }
            Message::Poll => return self.refresh_traces(client),
            Message::TraceLoaded(id, result) => {
                if let Some(trace) = self.traces.get_mut(&id) {
                    trace.loading = false;
                    match result {
                        Ok(list) => {
                            trace.loaded = true;
                            for event in list.events {
                                if event.kind == "summary" {
                                    trace.summaries.insert(
                                        event.id.clone(),
                                        markdown::parse(&event.content).collect(),
                                    );
                                }
                                if let Some(old) =
                                    trace.events.iter_mut().find(|e| e.id == event.id)
                                {
                                    *old = event;
                                } else {
                                    trace.events.push(event);
                                }
                            }
                            trace.events.sort_by(|a, b| {
                                (a.created_at_ms, &a.id).cmp(&(b.created_at_ms, &b.id))
                            });
                        }
                        Err(e) => self.error = e,
                    }
                }
            }
            Message::Link(url) => {
                return Task::perform(
                    async move { open::that(url).map_err(|e| e.to_string()) },
                    Message::Opened,
                );
            }
        }
        Task::none()
    }
    fn attachments<'a>(&'a self, files: &'a [wire::ChatAttachment]) -> Element<'a, Message> {
        column(files.iter().map(|file| {
            let content: Element<'_, Message> =
                if let Some(Some(handle)) = self.previews.get(&file.id) {
                    image(handle.clone()).height(120).into()
                } else {
                    text(&file.name).size(13).into()
                };
            row![
                button(content)
                    .style(style::quiet)
                    .on_press(Message::Attachment(file.clone())),
                space::horizontal(),
                icon_button(
                    Icon::Download,
                    "保存附件",
                    Some(Message::Download(file.clone()))
                )
            ]
            .align_y(iced::Alignment::Center)
            .into()
        }))
        .spacing(4)
        .into()
    }
    fn trace<'a>(&'a self, message: &'a wire::ChatMessage) -> Element<'a, Message> {
        let Some(trace) = self.traces.get(&message.id) else {
            return space().into();
        };
        let status = if message.active && message.run_id == self.state.run_id {
            status_label(&self.state.run_status)
        } else {
            "执行记录"
        };
        let mut body = column![
            button(
                row![
                    if trace.expanded {
                        Icon::Down
                    } else {
                        Icon::Right
                    }
                    .view(),
                    text(status).size(12)
                ]
                .spacing(6)
            )
            .style(style::quiet)
            .on_press(Message::Trace(message.id.clone()))
        ]
        .spacing(8);
        if trace.expanded {
            let calls: Vec<_> = trace
                .events
                .iter()
                .filter_map(|e| e.model_timing.as_ref())
                .collect();
            if !calls.is_empty() {
                let label = format!(
                    "{} 次模型调用 · 模型 {:.2}s",
                    calls.len(),
                    calls.iter().map(|t| t.model_ms).sum::<u64>() as f64 / 1000.
                );
                body = body.push(text(label).size(12).color(style::MUTED));
            }
            for event in &trace.events {
                if event.kind == "summary" {
                    if let Some(items) = trace.summaries.get(&event.id) {
                        body = body.push(
                            markdown::view(
                                items,
                                markdown::Settings::with_text_size(14, style::theme()),
                            )
                            .map(Message::Link),
                        );
                    }
                } else if event.kind == "tool" || event.kind == "audit" {
                    let elapsed = if event.finished_at_ms > 0 {
                        format!(
                            "{:.2}s",
                            event.finished_at_ms.saturating_sub(event.created_at_ms) as f64 / 1000.
                        )
                    } else {
                        String::new()
                    };
                    body = body.push(
                        button(
                            row![
                                if self.expanded.contains(&event.id) {
                                    Icon::Down
                                } else {
                                    Icon::Right
                                }
                                .view(),
                                text(&event.title).size(13),
                                space::horizontal(),
                                text(status_label(&event.status))
                                    .size(12)
                                    .color(style::MUTED),
                                text(elapsed).size(12).color(style::MUTED)
                            ]
                            .spacing(6)
                            .align_y(iced::Alignment::Center),
                        )
                        .width(Length::Fill)
                        .style(style::quiet)
                        .on_press(Message::TraceDetail(event.id.clone())),
                    );
                    if self.expanded.contains(&event.id) {
                        body = body.push(container(text(&event.content).size(12)).padding([8, 16]));
                    }
                }
            }
            if !trace.loaded && trace.loading {
                body = body.push(text("读取过程…").size(12).color(style::MUTED));
            }
        }
        body.into()
    }
    pub fn view(&self) -> Element<'_, Message> {
        let mut messages = column![].spacing(18);
        if self.has_older {
            messages = messages.push(
                button(
                    text(if self.loading_older {
                        "读取中…"
                    } else {
                        "较早消息"
                    })
                    .size(12),
                )
                .style(style::quiet)
                .on_press_maybe((!self.loading_older).then_some(Message::Older)),
            );
        }
        for message in &self.state.messages {
            if !message.system_notice.is_empty() {
                continue;
            }
            let mut item = column![
                text(if message.role == "user" {
                    "你"
                } else if self.state.agent_name.is_empty() {
                    "Agent"
                } else {
                    &self.state.agent_name
                })
                .size(11)
                .color(style::MUTED)
            ]
            .spacing(7);
            if message.has_activity {
                item = item.push(self.trace(message));
            }
            if !message.attachments.is_empty() {
                item = item.push(self.attachments(&message.attachments));
            }
            if let Some(items) = self.markdown.get(&message.id) {
                item = item.push(
                    markdown::view(
                        items,
                        markdown::Settings::with_text_size(14, style::theme()),
                    )
                    .map(Message::Link),
                );
            }
            if !message.text.is_empty() || message.has_activity || !message.attachments.is_empty() {
                messages = messages.push(item);
            }
        }
        for pending in &self.pending {
            let mut item = column![
                text("你").size(11).color(style::MUTED),
                text(&pending.message.text).size(14),
                self.attachments(&pending.message.attachments)
            ]
            .spacing(6);
            let delivery: Element<'_, Message> = if pending.sending {
                text("发送中…").size(12).color(style::MUTED).into()
            } else {
                button(text("重试发送").size(12))
                    .style(style::quiet)
                    .on_press(Message::Retry(pending.request.request_id.clone()))
                    .into()
            };
            item = item.push(delivery);
            messages = messages.push(item);
        }
        let editor = text_editor(&self.draft)
            .placeholder("发送消息…")
            .height(84)
            .padding(10)
            .size(14)
            .on_action(Message::Edit)
            .key_binding(|press| {
                if press.key == keyboard::Key::Named(keyboard::key::Named::Enter)
                    && press.modifiers.command()
                {
                    Some(text_editor::Binding::Custom(Message::Send))
                } else {
                    text_editor::Binding::from_key_press(press)
                }
            })
            .style(|theme, status| {
                let mut s = text_editor::default(theme, status);
                s.background = iced::Color::TRANSPARENT.into();
                s.border.width = 0.;
                s
            });
        let mut actions = row![
            icon_button(Icon::Paperclip, "添加附件", Some(Message::Pick)),
            space::horizontal()
        ]
        .spacing(4);
        if self.state.run_status == "paused" {
            actions = actions.push(icon_button(
                Icon::Play,
                "继续",
                (!self.control_busy).then_some(Message::Command(Command::Resume)),
            ));
        }
        if matches!(
            self.state.run_status.as_str(),
            "queued" | "running" | "paused"
        ) {
            if self.state.can_pause && self.state.run_status != "paused" {
                actions = actions.push(icon_button(
                    Icon::Pause,
                    "暂停",
                    (!self.control_busy).then_some(Message::Command(Command::Pause)),
                ));
            }
            actions = actions.push(icon_button(
                Icon::Stop,
                "停止",
                (!self.control_busy).then_some(Message::Command(Command::Stop)),
            ));
        }
        actions = actions.push(icon_button(
            Icon::Send,
            "发送 · ⌘ / Ctrl + Enter",
            ((!self.draft.text().trim().is_empty() || !self.uploads.is_empty())
                && self.uploads.iter().all(|u| u.attachment.is_some()))
            .then_some(Message::Send),
        ));
        let mut composer = column![].spacing(3);
        for upload in &self.uploads {
            composer = composer.push(
                row![
                    button(text(&upload.name).size(12))
                        .style(style::quiet)
                        .on_press_maybe(upload.attachment.clone().map(Message::Attachment)),
                    text(if upload.error.is_some() {
                        "失败"
                    } else if upload.attachment.is_none() {
                        "上传中…"
                    } else {
                        ""
                    })
                    .size(12)
                    .color(style::MUTED),
                    space::horizontal(),
                    icon_button(
                        Icon::Trash,
                        "移除附件",
                        Some(Message::Remove(upload.id.clone()))
                    )
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            );
        }
        composer = composer.push(editor).push(actions);
        column![
            scrollable(messages)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(4).scroller_width(4)
                ))
                .id("shared-chat")
                .on_scroll(|v| Message::Scroll(v.relative_offset().y > 0.98))
                .height(Length::Fill),
            composer
        ]
        .spacing(10)
        .into()
    }
    pub fn preview_view(&self) -> Element<'_, Message> {
        let Some(file) = &self.preview else {
            return space().into();
        };
        let mut content = column![
            row![
                icon_button(Icon::Back, "返回", Some(Message::ClosePreview)),
                text(&file.name).size(14),
                space::horizontal(),
                icon_button(
                    Icon::Download,
                    "保存图片",
                    Some(Message::Download(file.clone()))
                )
            ]
            .align_y(iced::Alignment::Center)
        ]
        .spacing(12);
        if let Some(Some(handle)) = self.previews.get(&file.id) {
            content = content.push(
                image(handle.clone())
                    .content_fit(iced::ContentFit::Contain)
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
        }
        container(content).padding(20).height(Length::Fill).into()
    }
}
fn image_decode(bytes: &[u8]) -> Result<image::Handle, String> {
    let pixels = ::image::load_from_memory(bytes)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    Ok(image::Handle::from_rgba(
        pixels.width(),
        pixels.height(),
        pixels.into_raw(),
    ))
}
fn status_label(status: &str) -> &str {
    match status {
        "running" => "正在工作",
        "queued" => "排队中",
        "paused" => "已暂停",
        "failed" => "执行失败",
        "canceled" => "已停止",
        "completed" | "succeeded" => "已完成",
        _ => status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(revision: u64, messages: Vec<wire::ChatMessage>) -> wire::ChatState {
        wire::ChatState {
            revision,
            conversation_id: "shared".into(),
            messages,
            ..Default::default()
        }
    }
    fn message(id: &str, ordinal: u64, text: &str) -> wire::ChatMessage {
        wire::ChatMessage {
            id: id.into(),
            ordinal,
            role: "assistant".into(),
            text: text.into(),
            ..Default::default()
        }
    }
    #[test]
    fn tail_updates_keep_older_messages_and_do_not_rewind_stream() {
        let client = Connection::for_state_test();
        let mut chat = Chat::new();
        let _ = chat.accept(
            state(
                2,
                vec![
                    message("older", 1, "older"),
                    message("current", 2, "stream"),
                ],
            ),
            &client,
        );
        let _ = chat.accept(
            state(4, vec![message("current", 2, "stream complete")]),
            &client,
        );
        let _ = chat.accept(state(3, vec![message("current", 2, "stream old")]), &client);
        assert_eq!(chat.state.messages.len(), 2);
        assert_eq!(chat.state.messages[1].text, "stream complete");
        assert_eq!(chat.state.revision, 4);
    }
    #[test]
    fn finished_runs_stop_polling_and_fetch_the_final_receipt() {
        let client = Connection::for_state_test();
        let mut chat = Chat::new();
        let mut active = message("active", 1, "");
        active.run_id = "run".into();
        active.active = true;
        active.has_activity = true;
        let mut running = state(1, vec![active]);
        running.run_id = "run".into();
        running.run_status = "running".into();
        let _ = chat.accept(running.clone(), &client);
        assert_eq!(
            iced::advanced::subscription::into_recipes(chat.subscription()).len(),
            1
        );
        let trace = chat.traces.get_mut("active").unwrap();
        trace.loaded = true;
        trace.loading = false;
        running.revision = 2;
        running.run_status = "canceled".into();
        let _ = chat.accept(running, &client);
        assert!(iced::advanced::subscription::into_recipes(chat.subscription()).is_empty());
        assert!(!chat.is_live(&chat.state.messages[0]));
        assert!(chat.traces["active"].loading);
        assert!(!chat.traces["active"].loaded);
    }
    #[test]
    fn in_flight_submission_does_not_erase_followup_draft() {
        let client = Connection::for_state_test();
        let mut chat = Chat::new();
        chat.draft = text_editor::Content::with_text("先打开设置");
        let _ = chat.update(Message::Send, &client, "phone");
        let id = chat.pending[0].request.request_id.clone();
        chat.draft = text_editor::Content::with_text("再看一下电脑");
        let mut accepted = message("accepted", 1, "先打开设置");
        accepted.request_id = id.clone();
        let _ = chat.update(
            Message::Sent(id, Ok(state(1, vec![accepted]))),
            &client,
            "desktop",
        );
        assert_eq!(chat.draft.text(), "再看一下电脑");
        assert!(chat.pending.is_empty());
    }
    #[test]
    fn failed_send_retries_same_request_and_restores_all_pending() {
        let client = Connection::for_state_test();
        let mut chat = Chat::new();
        chat.draft = text_editor::Content::with_text("消息");
        let _ = chat.update(Message::Send, &client, "phone");
        let request = chat.pending[0].request.clone();
        let _ = chat.update(
            Message::Sent(request.request_id.clone(), Err("连接中断".into())),
            &client,
            "desktop",
        );
        let _ = chat.update(
            Message::Retry(request.request_id.clone()),
            &client,
            "desktop",
        );
        assert_eq!(chat.pending[0].request, request);
        let accepted = wire::ChatMessage {
            request_id: "accepted".into(),
            ..message("a", 1, "one")
        };
        chat.state.messages.push(accepted.clone());
        let pending = wire::ChatMessage {
            request_id: "pending".into(),
            ..message("b", 2, "two")
        };
        let _ = chat.update(
            Message::Restored(Ok(vec![state(1, vec![accepted]), state(1, vec![pending])])),
            &client,
            "desktop",
        );
        assert!(
            chat.pending
                .iter()
                .any(|p| p.request.request_id == "pending")
        );
    }
}
