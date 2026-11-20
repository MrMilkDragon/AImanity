use crate::{
    connection::Connection,
    controls::{Icon, icon_button},
    modes,
    platform::{self, ClipboardContent},
    style,
};
use ai_phone_api::wire;
use iced::{
    Border, Element, Font, Length, Point, Rectangle, Size, Subscription, Task, font, keyboard,
    widget::{
        button, column, container, image, markdown, pin, row, scrollable, space, stack, text,
        text_editor,
    },
};
use prost::Message as _;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

#[derive(Clone, Debug)]
pub enum Message {
    Todos(bool),
    HostToggle,
    HostAllow,
    HostGranted(Result<(), String>),
    HostEnded(String, Result<(), String>),
    RefreshDisplay,
    Mode(modes::Message),
    Edit(text_editor::Action),
    Send,
    Sent(String, Result<wire::ChatState, String>),
    Retry(String),
    History(Result<wire::ChatState, String>),
    Older,
    Scroll(bool),
    Pick,
    Paste,
    Pasted(Result<Option<ClipboardContent>, String>),
    Files(Vec<PathBuf>),
    Capture(String, Vec<u8>),
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
    display_after: Option<u64>,
    pub state: wire::ChatState,
    draft: text_editor::Content,
    pending: Vec<Pending>,
    uploads: Vec<Upload>,
    pasting: bool,
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
    pub host: crate::host_desktop::Permission,
    outbox: PathBuf,
}
impl Chat {
    pub fn new() -> Self {
        Self {
            display_after: None,
            state: Default::default(),
            draft: text_editor::Content::new(),
            pending: Vec::new(),
            uploads: Vec::new(),
            pasting: false,
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
            host: crate::host_desktop::Permission::Disabled,
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
    fn accept(
        &mut self,
        mut next: wire::ChatState,
        client: &Connection,
        snapshot: bool,
    ) -> Task<Message> {
        if !self.state.agent_id.is_empty()
            && !next.agent_id.is_empty()
            && self.state.agent_id != next.agent_id
        {
            self.host.close();
        }
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
        let sorted_len = self.state.messages.len();
        for incoming in next.messages.drain(..) {
            let old = self.state.messages[..sorted_len]
                .binary_search_by_key(&incoming.ordinal, |m| m.ordinal)
                .ok();
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
        if self.state.messages.len() != sorted_len {
            self.state.messages.sort_by_key(|m| m.ordinal);
        }
        if snapshot && next.revision >= self.state.revision {
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
    fn send_pending(&mut self, id: &str, client: &Connection, fresh: bool) -> Task<Message> {
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
            agent_id: pending.request.agent_id.clone(),
            messages: vec![pending.message.clone()],
            ..Default::default()
        };
        let path = self.outbox.join(format!("{id}.pb"));
        let temporary = path.with_extension("tmp");
        let mut request = pending.request.clone();
        request.receipt_only = true;
        let client = client.clone();
        let id = id.to_string();
        let (ready, host_task) =
            if fresh && matches!(self.host, crate::host_desktop::Permission::Armed) {
                request.use_host_desktop = true;
                let (stop, closed) = tokio::sync::watch::channel(());
                let (started, ready) = tokio::sync::oneshot::channel();
                let request = id.clone();
                let task = Task::perform(
                    crate::host_desktop::serve(
                        client.assistant.clone(),
                        request.clone(),
                        started,
                        closed,
                    ),
                    move |result| Message::HostEnded(request.clone(), result),
                );
                self.host = crate::host_desktop::Permission::Active {
                    request: id.clone(),
                    _stop: stop,
                };
                (Some(ready), task)
            } else {
                if fresh {
                    self.host.close();
                }
                (None, Task::none())
            };
        let send = Task::perform(
            async move {
                tokio::fs::write(&temporary, snapshot.encode_to_vec())
                    .await
                    .map_err(|e| e.to_string())?;
                tokio::fs::rename(temporary, path)
                    .await
                    .map_err(|e| e.to_string())?;
                if let Some(ready) = ready {
                    ready.await.map_err(|_| "本机桌面未连接")?;
                }
                client.rpc("assistant/send", request).await
            },
            move |r| Message::Sent(id.clone(), r),
        );
        Task::batch([host_task, send])
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
            Message::RefreshDisplay => {
                self.display_after = self.state.messages.last().map(|m| m.ordinal);
                self.following = true;
            }
            Message::Mode(_) | Message::Todos(_) => {}
            Message::Edit(action) => self.draft.perform(action),
            Message::Send => {
                if !self.can_send() {
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
                return self.send_pending(&id, client, true);
            }
            Message::Retry(id) => return self.send_pending(&id, client, false),
            Message::HostToggle => {
                use crate::host_desktop::Permission;
                self.host = match self.host {
                    Permission::Disabled => Permission::Disabled,
                    Permission::Off => Permission::Confirm,
                    _ => Permission::Off,
                };
            }
            Message::HostAllow => {
                if matches!(self.host, crate::host_desktop::Permission::Confirm) {
                    return Task::perform(
                        async {
                            #[cfg(target_os = "macos")]
                            crate::host_desktop::permission().map_err(|e| e.to_string())?;
                            Ok(())
                        },
                        Message::HostGranted,
                    );
                }
            }
            Message::HostGranted(result) => {
                if matches!(self.host, crate::host_desktop::Permission::Confirm) {
                    self.host = crate::host_desktop::Permission::Off;
                    match result {
                        Ok(()) => self.host = crate::host_desktop::Permission::Armed,
                        Err(error) => self.error = error,
                    }
                }
            }
            Message::HostEnded(id, result) => {
                if self.host.end(&id)
                    && let Err(error) = result
                {
                    self.error = error;
                }
            }
            Message::Sent(id, result) => match result {
                Ok(receipt) => return self.accept(receipt, client, false),
                Err(error) => {
                    self.host.end(&id);
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
                    Ok(state) => return self.accept(state, client, true),
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
                                agent_id: record.agent_id.clone(),
                                text: message.text.clone(),
                                attachment_ids: message
                                    .attachments
                                    .iter()
                                    .map(|a| a.id.clone())
                                    .collect(),
                                request_id: message.request_id.clone(),
                                voice: message.voice.clone(),
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
            Message::Paste if !self.pasting => {
                self.pasting = true;
                return Task::perform(platform::read_clipboard(), Message::Pasted);
            }
            Message::Paste => {}
            Message::Pasted(result) => {
                self.pasting = false;
                match result {
                    Ok(Some(ClipboardContent::Png(png))) => {
                        return self.update(
                            Message::Capture("clipboard.png".into(), png),
                            client,
                            device_id,
                        );
                    }
                    Ok(Some(ClipboardContent::Text(text))) => {
                        self.draft
                            .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                                Arc::new(text),
                            )));
                    }
                    Ok(None) => {}
                    Err(error) => self.error = error,
                }
            }
            Message::Capture(name, bytes) => {
                if self.uploads.len() == 8 {
                    self.error = "每条消息最多 8 个附件".into();
                    return Task::none();
                }
                let id = uuid::Uuid::now_v7().to_string();
                self.uploads.push(Upload {
                    id: id.clone(),
                    name: name.clone(),
                    attachment: None,
                    error: None,
                });
                let client = client.clone();
                return Task::perform(
                    async move { client.upload_bytes(name, bytes).await },
                    move |r| Message::Uploaded(id.clone(), r),
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
                self.host.close();
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
                    Ok(next) => return self.accept(next, client, true),
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
    pub fn send_voice(
        &mut self,
        request: wire::ChatSendRequest,
        client: &Connection,
    ) -> Task<Message> {
        let id = request.request_id.clone();
        self.pending.push(Pending {
            message: wire::ChatMessage {
                id: id.clone(),
                request_id: id.clone(),
                role: "user".into(),
                text: request.text.clone(),
                voice: request.voice.clone(),
                ..Default::default()
            },
            request,
            error: None,
            sending: false,
        });
        self.following = true;
        self.error.clear();
        self.send_pending(&id, client, true)
    }
    pub fn recover_voice(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let draft = self.draft.text();
        self.draft = text_editor::Content::with_text(
            if draft.trim().is_empty() {
                text.to_owned()
            } else {
                format!("{}\n{text}", draft.trim_end())
            }
            .as_str(),
        );
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
                icon_button(Icon::Download, Some(Message::Download(file.clone())))
            ]
            .align_y(iced::Alignment::Center)
            .into()
        }))
        .spacing(4)
        .into()
    }
    fn trace<'a>(
        &'a self,
        message: &'a wire::ChatMessage,
        theme: &iced::Theme,
        locale: crate::locale::Locale,
    ) -> Element<'a, Message> {
        let Some(trace) = self.traces.get(&message.id) else {
            return space().into();
        };
        let status = if message.active && message.run_id == self.state.run_id {
            locale.tr(status_label(&self.state.run_status))
        } else {
            locale.tr("执行记录")
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
                body = body.push(text(label).size(12).style(style::muted_text));
            }
            for event in &trace.events {
                if event.kind == "summary" {
                    if let Some(items) = trace.summaries.get(&event.id) {
                        body = body.push(
                            markdown::view(
                                items,
                                markdown::Settings::with_text_size(14, theme.clone()),
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
                                text(locale.tr(status_label(&event.status)))
                                    .size(12)
                                    .style(style::muted_text),
                                text(elapsed).size(12).style(style::muted_text)
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
                body = body.push(
                    text(locale.tr("读取过程…"))
                        .size(12)
                        .style(style::muted_text),
                );
            }
        }
        body.into()
    }
    fn can_send(&self) -> bool {
        !self.pasting
            && (!self.draft.text().trim().is_empty() || !self.uploads.is_empty())
            && self
                .uploads
                .iter()
                .all(|upload| upload.attachment.is_some())
    }
    pub fn view<'a>(
        &'a self,
        modes: &'a modes::Modes,
        locale: crate::locale::Locale,
        width: f32,
        reduced_motion: bool,
    ) -> Element<'a, Message> {
        let theme = style::theme();
        let bubble = |item| {
            row![
                space::horizontal(),
                container(item)
                    .max_width(width * 0.85)
                    .padding([10, 16])
                    .style(move |_| {
                        container::Style {
                            background: Some(iced::Color::from_rgb8(242, 242, 242).into()),
                            border: iced::Border {
                                radius: iced::border::Radius::new(style::PANEL_RADIUS)
                                    .bottom_right(style::CONTROL_RADIUS * 0.5),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    })
            ]
        };
        let mut messages = column![].spacing(22);
        if self.has_older {
            messages = messages.push(
                button(crate::loading::label(
                    locale.tr("较早消息"),
                    12,
                    self.loading_older,
                    reduced_motion,
                ))
                .style(style::quiet)
                .on_press_maybe((!self.loading_older).then_some(Message::Older)),
            );
        }
        for message in &self.state.messages {
            if !message.system_notice.is_empty() {
                continue;
            }
            if self
                .display_after
                .is_some_and(|ordinal| message.ordinal <= ordinal)
                && !message.active
            {
                continue;
            }
            let mut item = column![].spacing(7);
            if message.has_activity {
                item = item.push(self.trace(message, &theme, locale));
            }
            if !message.attachments.is_empty() {
                item = item.push(self.attachments(&message.attachments));
            }
            if let Some(items) = self.markdown.get(&message.id) {
                item = item.push(
                    markdown::view(items, markdown::Settings::with_text_size(14, theme.clone()))
                        .map(Message::Link),
                );
            }
            if !message.text.is_empty() || message.has_activity || !message.attachments.is_empty() {
                messages = messages.push(if message.role == "user" {
                    Element::from(bubble(item))
                } else {
                    container(item).padding([0, 4]).into()
                });
            }
        }
        for pending in &self.pending {
            let mut item = column![
                text(&pending.message.text).size(14),
                self.attachments(&pending.message.attachments)
            ]
            .spacing(6);
            let delivery: Element<'_, Message> = if pending.sending {
                text(locale.tr("发送中…"))
                    .size(12)
                    .style(style::muted_text)
                    .into()
            } else {
                button(text(locale.tr("重试发送")).size(12))
                    .style(style::quiet)
                    .on_press(Message::Retry(pending.request.request_id.clone()))
                    .into()
            };
            item = item.push(delivery);
            messages = messages.push(bubble(item));
        }
        let host_permission = matches!(self.host, crate::host_desktop::Permission::Confirm).then(|| {
            column![
                text(locale.tr("本次任务可跨应用操作你的整个桌面。屏幕画面会发给当前模型，并保存在本地任务记录中。")).size(13),
                row![
                    button(text(locale.tr("取消")).size(13)).style(style::quiet).on_press(Message::HostToggle),
                    button(text(locale.tr("允许本次任务")).size(13)).style(style::primary).on_press(Message::HostAllow)
                ].spacing(8)
            ].spacing(6)
        });
        let editor = text_editor(&self.draft)
            .placeholder(locale.tr("发送消息…"))
            .height(72.)
            .padding([0, 4])
            .size(15)
            .line_height(iced::widget::text::LineHeight::Relative(1.6))
            .on_action(Message::Edit)
            .key_binding(|press| {
                if press.key == keyboard::Key::Named(keyboard::key::Named::Enter)
                    && (press.modifiers.command() || press.modifiers.control())
                {
                    Some(text_editor::Binding::Custom(Message::Send))
                } else {
                    match text_editor::Binding::from_key_press(press) {
                        Some(text_editor::Binding::Paste) => {
                            Some(text_editor::Binding::Custom(Message::Paste))
                        }
                        binding => binding,
                    }
                }
            })
            .style(|theme, status| {
                let mut s = text_editor::default(theme, status);
                s.background = iced::Color::TRANSPARENT.into();
                s.border.width = 0.;
                s
            });
        let mut right_actions = row![
            modes
                .copilot_button(!self.state.conversation_id.is_empty())
                .map(Message::Mode),
        ]
        .spacing(4)
        .align_y(iced::Center);
        if matches!(self.state.run_status.as_str(), "queued" | "running") {
            right_actions = right_actions.push(crate::loading::label(
                locale.tr(if self.state.run_status == "queued" {
                    "待继续"
                } else {
                    "正在处理"
                }),
                12,
                true,
                reduced_motion,
            ));
        }
        let mut submit = row![].spacing(4).align_y(iced::Center);
        if self.state.run_status == "paused" {
            submit = submit.push(icon_button(
                Icon::Play,
                (!self.control_busy).then_some(Message::Command(Command::Resume)),
            ));
        }
        if matches!(
            self.state.run_status.as_str(),
            "queued" | "running" | "paused"
        ) {
            if self.state.can_pause && self.state.run_status != "paused" {
                submit = submit.push(icon_button(
                    Icon::Pause,
                    (!self.control_busy).then_some(Message::Command(Command::Pause)),
                ));
            }
            submit = submit.push(icon_button(
                Icon::Stop,
                (!self.control_busy).then_some(Message::Command(Command::Stop)),
            ));
        }
        let can_send = self.can_send();
        let send = Icon::Send
            .view()
            .opacity(if can_send { 1. } else { 0.5 })
            .style(|_, _| iced::widget::svg::Style {
                color: Some(iced::Color::WHITE),
            });
        submit = submit.push(
            button(container(send).center(Length::Fill))
                .width(36)
                .height(36)
                .padding(0)
                .style(style::primary)
                .on_press_maybe(can_send.then_some(Message::Send)),
        );
        use crate::host_desktop::Permission;
        let host_button: Option<Element<'_, Message>> = match self.host {
            Permission::Disabled => None,
            Permission::Armed | Permission::Active { .. } => Some(
                button(text(locale.tr("结束本机协作")).size(12))
                    .style(style::quiet)
                    .on_press(Message::HostToggle)
                    .into(),
            ),
            _ => Some(
                iced::widget::tooltip(
                    icon_button(Icon::Screen, Some(Message::HostToggle)),
                    text(locale.tr("本机协作")).size(12),
                    iced::widget::tooltip::Position::Top,
                )
                .into(),
            ),
        };
        let actions = row![
            row![
                iced::widget::mouse_area(icon_button(Icon::Todos, Some(Message::Todos(true))))
                    .on_enter(Message::Todos(false)),
                icon_button(Icon::Paperclip, Some(Message::Pick)),
            ]
            .extend(host_button)
            .push(modes.liberty_button().map(Message::Mode))
            .spacing(4)
            .align_y(iced::Center),
            container(
                right_actions
                    .push(submit)
                    .wrap()
                    .align_x(iced::alignment::Horizontal::Right)
                    .vertical_spacing(6),
            )
            .width(Length::Fill)
            .align_x(iced::Right),
        ]
        .spacing(4);
        let mut composer = column![]
            .spacing(3)
            .extend(host_permission.map(Element::from));
        for upload in &self.uploads {
            composer = composer.push(
                row![
                    button(text(&upload.name).size(12))
                        .style(style::quiet)
                        .on_press_maybe(upload.attachment.clone().map(Message::Attachment)),
                    text(if upload.error.is_some() {
                        locale.tr("失败")
                    } else if upload.attachment.is_none() {
                        locale.tr("上传中…")
                    } else {
                        ""
                    })
                    .size(12)
                    .style(style::muted_text),
                    space::horizontal(),
                    icon_button(Icon::Trash, Some(Message::Remove(upload.id.clone())))
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            );
        }
        if !modes.transcript().is_empty() {
            composer = composer.push(container(text(modes.transcript()).size(13)).padding([6, 10]));
        }
        composer = composer.push(editor).push(actions);
        column![
            scrollable(container(messages).padding([12, 2]))
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(4).scroller_width(4)
                ))
                .id("shared-chat")
                .on_scroll(|v| Message::Scroll(v.relative_offset().y > 0.98))
                .height(Length::Fill),
            container(composer.spacing(8))
                .padding(12)
                .style(move |_| container::Style {
                    background: Some(iced::Color::WHITE.into()),
                    border: iced::Border {
                        color: iced::Color::from_rgb8(222, 222, 222),
                        width: 1.,
                        radius: style::PANEL_RADIUS.into()
                    },
                    ..Default::default()
                }),
        ]
        .into()
    }
    pub fn preview_bounds(&self, file_id: &str, viewport: Size) -> Rectangle {
        let width = (viewport.width * 0.9).min(960.);
        let image_height = match self.previews.get(file_id) {
            Some(Some(image::Handle::Rgba {
                width: native_width,
                height,
                ..
            })) => {
                ((width - 48.) * *height as f32 / *native_width as f32).min(viewport.height * 0.75)
            }
            _ => 0.,
        };
        let height = 82. + image_height;
        Rectangle::new(
            Point::new(
                (viewport.width - width) * 0.5,
                (viewport.height - height) * 0.5,
            ),
            Size::new(width, height),
        )
    }

    pub fn preview_view<'a>(
        &'a self,
        file_id: &str,
        name: &'a str,
        bounds: Rectangle,
    ) -> Element<'a, Message> {
        let mut content = column![
            container(
                text(name)
                    .size(18)
                    .font(Font {
                        weight: font::Weight::Semibold,
                        ..Font::DEFAULT
                    })
                    .line_height(iced::Pixels(18.))
                    .wrapping(text::Wrapping::None)
            )
            .width(bounds.width - 72.)
            .clip(true),
        ]
        .spacing(16);
        if let Some(Some(handle)) = self.previews.get(file_id) {
            content = content.push(
                image(handle.clone())
                    .content_fit(iced::ContentFit::Contain)
                    .width(Length::Fill)
                    .height(bounds.height - 82.),
            );
        }
        let close = crate::surface::tool_action(Icon::Close, Some(Message::ClosePreview));
        container(stack![
            container(content).padding(24),
            pin(close).x(bounds.width - 48.).y(8.)
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|theme| {
            container::Style::default()
                .background(theme.palette().background)
                .border(Border {
                    width: 1.,
                    radius: style::PANEL_RADIUS.into(),
                    color: theme.palette().text.scale_alpha(0.14),
                })
        })
        .into()
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
            true,
        );
        let _ = chat.accept(
            state(4, vec![message("current", 2, "stream complete")]),
            &client,
            true,
        );
        let _ = chat.accept(
            state(3, vec![message("current", 2, "stream old")]),
            &client,
            true,
        );
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
        let _ = chat.accept(running.clone(), &client, true);
        assert_eq!(
            iced::advanced::subscription::into_recipes(chat.subscription()).len(),
            1
        );
        let trace = chat.traces.get_mut("active").unwrap();
        trace.loaded = true;
        trace.loading = false;
        running.revision = 2;
        running.run_status = "canceled".into();
        let _ = chat.accept(running, &client, true);
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
        chat.state.run_id = "live-run".into();
        chat.state.run_status = "running".into();
        let mut accepted = message("accepted", 1, "先打开设置");
        accepted.request_id = id.clone();
        let _ = chat.update(
            Message::Sent(id, Ok(state(0, vec![accepted]))),
            &client,
            "desktop",
        );
        assert_eq!(chat.draft.text(), "再看一下电脑");
        assert!(chat.pending.is_empty());
        assert_eq!(chat.state.run_id, "live-run");
        assert_eq!(chat.state.run_status, "running");
        assert_eq!(chat.state.messages[0].id, "accepted");
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
