use crate::{
    connection::Connection,
    controls::{
        Icon, action_button, content_button, form_input, form_select, icon_button, text_button,
    },
    deployment::{self, Field, Request},
    locale::Locale,
    style,
};
use ai_phone_api::wire;
use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, row, rule, space, text},
};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Message {
    Open,
    Edit(String),
    Invite(wire::DevicePairRequest),
    Invitation(
        wire::DevicePairRequest,
        Result<wire::DeviceInvitation, String>,
    ),
    Repair(String),
    ExpireInvitation(std::time::Instant),
    Disconnect(String),
    AskDelete(String),
    Delete(String),
    CancelDelete,
    CopyCode,
    #[cfg(not(target_os = "android"))]
    Share(String),
    New(String),
    Name(String),
    Kind(String),
    Discard,
    Save,
    Saved(Result<wire::RegistryState, String>),
    Check,
    Jobs(Result<wire::DeviceLinkState, String>),
    Cpu(u32),
    Memory(Memory),
    Resource(bool, Field, u16),
    Restart,
    Install,
    Installed(
        wire::DesktopProvisionRequest,
        Result<wire::DesktopProvisionJob, String>,
    ),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory(u32);
impl std::fmt::Display for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} GB", f64::from(self.0) / 1024.)
    }
}

#[derive(Clone, Debug)]
pub enum Startup {
    Pair(wire::DevicePairRequest),
    Desktop(wire::DesktopProvisionRequest),
}
#[derive(Debug)]
pub struct Issue {
    pub reason: String,
    pub retry: Option<Startup>,
}
pub struct Devices {
    pub issues: std::collections::HashMap<String, Issue>,
    starting: std::collections::HashSet<String>,
    pub selected: String,
    pub(super) invitation: Option<(wire::DeviceInvitation, std::time::Instant)>,
    draft: Option<wire::DeviceSaveRequest>,
    busy: bool,
    delete: Option<String>,
    error: String,
    cpus: u32,
    memory: u32,
    jobs: Vec<wire::DesktopProvisionJob>,
    pub registry: Option<wire::RegistryState>,
    pub applied: Option<Request>,
    pub resources: Option<Request>,
}
impl Default for Devices {
    fn default() -> Self {
        Self {
            selected: String::new(),
            issues: Default::default(),
            starting: Default::default(),
            invitation: None,
            draft: None,
            busy: false,
            delete: None,
            error: String::new(),
            cpus: 2,
            memory: 2560,
            jobs: Vec::new(),
            registry: None,
            applied: None,
            resources: None,
        }
    }
}
impl Devices {
    pub fn connection_failed(&mut self, id: &str, reason: String) -> bool {
        match self.issues.get_mut(id) {
            Some(issue) => {
                if issue.retry.is_none() {
                    issue.reason = reason;
                }
                false
            }
            None => {
                self.issues.insert(
                    id.to_owned(),
                    Issue {
                        reason,
                        retry: None,
                    },
                );
                true
            }
        }
    }
    pub fn frame_received(&mut self, id: &str) -> bool {
        self.issues.remove(id).is_some()
    }
    fn launch(&mut self, action: Startup, client: &Connection) -> Task<Message> {
        let client = client.clone();
        match action {
            Startup::Pair(request) => {
                if !self.starting.insert(request.id.clone()) {
                    return Task::none();
                }
                let retry = request.clone();
                Task::perform(
                    async move { client.rpc("registry/device-invite", request).await },
                    move |r| Message::Invitation(retry.clone(), r),
                )
            }
            Startup::Desktop(mut request) => {
                if !self.starting.insert(request.device_id.clone()) {
                    return Task::none();
                }
                request.request_id = uuid::Uuid::now_v7().to_string();
                let retry = request.clone();
                Task::perform(
                    async move { client.rpc("desktop/provision", request).await },
                    move |r| Message::Installed(retry.clone(), r),
                )
            }
        }
    }
    fn end_device(
        &mut self,
        id: String,
        route: &'static str,
        client: &Connection,
    ) -> Task<Message> {
        self.busy = true;
        if self
            .invitation
            .as_ref()
            .is_some_and(|(invite, _)| invite.device_id == id)
        {
            self.invitation = None;
        }
        let client = client.clone();
        Task::perform(
            async move {
                client
                    .rpc(route, wire::DesktopRequest { device_id: id })
                    .await
            },
            Message::Saved,
        )
    }
    fn available(&self, registry: &wire::RegistryState) -> Vec<&'static str> {
        [
            ("phone", registry.phone_limit),
            ("desktop", registry.desktop_limit),
        ]
        .into_iter()
        .filter(|(kind, max)| {
            *max == 0
                || registry
                    .devices
                    .iter()
                    .filter(|d| !d.paired && &d.kind == kind)
                    .count()
                    < *max as usize
        })
        .map(|(kind, _)| kind)
        .collect()
    }
    fn running(&self) -> bool {
        self.jobs
            .iter()
            .any(|j| matches!(j.status.as_str(), "queued" | "running"))
    }
    pub fn subscription(&self) -> Subscription<Message> {
        if self.running() {
            iced::time::every(Duration::from_millis(1800)).map(|_| Message::Check)
        } else {
            Subscription::none()
        }
    }
    pub fn update(
        &mut self,
        message: Message,
        registry: &wire::RegistryState,
        client: &Connection,
    ) -> Task<Message> {
        let change_kind = matches!(message, Message::Kind(_));
        match message {
            Message::Repair(id) => {
                if let Some(action) = self.issues.get(&id).and_then(|i| i.retry.clone()) {
                    return self.launch(action, client);
                }
            }
            Message::Invite(request) => {
                self.error.clear();
                return self.launch(Startup::Pair(request), client);
            }
            Message::Invitation(request, r) => {
                self.starting.remove(&request.id);
                match r {
                    Ok(invite) => {
                        self.issues.remove(&request.id);
                        self.selected = invite.device_id.clone();
                        self.draft = None;
                        let issued = std::time::Instant::now();
                        if !registry
                            .devices
                            .iter()
                            .any(|d| d.id == invite.device_id && d.connected)
                        {
                            self.invitation = Some((invite, issued));
                            return Task::perform(
                                async { tokio::time::sleep(Duration::from_secs(300)).await },
                                move |()| Message::ExpireInvitation(issued),
                            );
                        }
                    }
                    Err(reason) => {
                        self.selected = request.id.clone();
                        self.draft = None;
                        self.issues.insert(
                            request.id.clone(),
                            Issue {
                                reason,
                                retry: Some(Startup::Pair(request)),
                            },
                        );
                    }
                }
            }
            Message::ExpireInvitation(issued) => {
                if self
                    .invitation
                    .as_ref()
                    .is_some_and(|(_, at)| *at == issued)
                {
                    self.invitation = None;
                }
            }
            Message::AskDelete(id) => self.delete = Some(id),
            Message::CancelDelete => self.delete = None,
            Message::Disconnect(id) => {
                return self.end_device(id, "registry/device-disconnect", client);
            }
            Message::Delete(id) => {
                self.delete = None;
                return self.end_device(id, "registry/device-delete", client);
            }
            Message::CopyCode => {
                if let Some((i, _)) = &self.invitation {
                    return iced::clipboard::write(i.code.clone());
                }
            }
            #[cfg(not(target_os = "android"))]
            Message::Share(locale) => {
                match std::env::current_exe().and_then(|p| {
                    std::process::Command::new(p)
                        .args(["--share-device", &locale])
                        .spawn()
                }) {
                    Ok(_) => {}
                    Err(e) => self.error = e.to_string(),
                }
            }
            Message::Open => {
                self.delete = None;
                self.draft = None;
                self.error.clear();
            }
            Message::Edit(id) => {
                let Some(d) = registry.devices.iter().find(|d| d.id == id) else {
                    return Task::none();
                };
                self.selected = id;
                self.resources = self.applied.clone();
                self.draft = Some(wire::DeviceSaveRequest {
                    id: d.id.clone(),
                    expected_revision: d.revision,
                    name: d.name.clone(),
                    kind: d.kind.clone(),
                });
                return iced::widget::operation::focus("device-name");
            }
            Message::New(kind) => {
                let id = uuid::Uuid::now_v7().to_string();
                self.selected = id.clone();
                self.draft = Some(wire::DeviceSaveRequest {
                    id,
                    kind,
                    ..Default::default()
                });
                return iced::widget::operation::focus("device-name");
            }
            Message::Name(name) | Message::Kind(name) => {
                if let Some(draft) = &mut self.draft {
                    if change_kind {
                        draft.kind = name;
                    } else {
                        draft.name = name;
                    }
                }
            }
            Message::Discard => self.draft = None,
            Message::Save => {
                if let Some(draft) = self.draft.clone() {
                    if let Some(kind) = draft.kind.strip_prefix("paired-") {
                        return self.update(
                            Message::Invite(wire::DevicePairRequest {
                                id: draft.id,
                                name: draft.name,
                                kind: kind.to_owned(),
                            }),
                            registry,
                            client,
                        );
                    }
                    self.busy = true;
                    self.error.clear();
                    let client = client.clone();
                    return Task::perform(
                        async move { client.rpc("registry/device-save", draft).await },
                        Message::Saved,
                    );
                }
            }
            Message::Saved(r) => {
                self.busy = false;
                match r {
                    Ok(state) => {
                        self.draft = None;
                        self.registry = Some(state);
                    }
                    Err(e) => self.error = e,
                }
            }
            Message::Check => {
                let client = client.clone();
                return Task::perform(
                    async move { client.rpc("desktop/state", wire::RegistryRequest {}).await },
                    Message::Jobs,
                );
            }
            Message::Jobs(result) => match result {
                Ok(state) => {
                    let finished =
                        self.running() && state.jobs.iter().any(|j| j.status == "completed");
                    for job in &state.jobs {
                        if job.status == "failed" {
                            self.issues.insert(
                                job.device_id.clone(),
                                Issue {
                                    reason: job.error.clone(),
                                    retry: Some(Startup::Desktop(wire::DesktopProvisionRequest {
                                        device_id: job.device_id.clone(),
                                        cpus: job.cpus,
                                        memory_mb: job.memory_mb,
                                        template_id: job.template_id.clone(),
                                        ..Default::default()
                                    })),
                                },
                            );
                        }
                    }
                    self.jobs = state.jobs;
                    if finished {
                        let client = client.clone();
                        return Task::perform(
                            async move { client.rpc("registry/state", wire::RegistryRequest {}).await },
                            Message::Saved,
                        );
                    }
                }
                Err(e) => self.error = e,
            },
            Message::Cpu(n) => self.cpus = n,
            Message::Memory(n) => self.memory = n.0,
            Message::Resource(desktop, field, value) => {
                if let Some(request) = &mut self.resources {
                    request.set(desktop, field, value);
                }
            }
            Message::Restart => {}
            Message::Install => {
                self.error.clear();
                let request = wire::DesktopProvisionRequest {
                    device_id: self.selected.clone(),
                    request_id: String::new(),
                    cpus: self.cpus,
                    memory_mb: self.memory,
                    template_id: ai_phone_api::LINUX_DESKTOP_TEMPLATE.into(),
                };
                return self.launch(Startup::Desktop(request), client);
            }
            Message::Installed(request, r) => {
                let id = request.device_id.clone();
                self.starting.remove(&id);
                if registry.devices.iter().any(|d| d.id == id) {
                    match r {
                        Ok(job) if job.status != "failed" => {
                            if job.status == "completed" {
                                self.issues.remove(&id);
                            }
                            self.jobs.retain(|old| old.device_id != id);
                            self.jobs.push(job);
                        }
                        other => {
                            let reason = match other {
                                Ok(job) => job.error,
                                Err(cause) => cause,
                            };
                            self.issues.insert(
                                id,
                                Issue {
                                    reason,
                                    retry: Some(Startup::Desktop(request)),
                                },
                            );
                        }
                    }
                }
            }
        }
        Task::none()
    }
    pub fn new_device(&self, registry: &wire::RegistryState) -> Message {
        Message::New(
            self.available(registry)
                .first()
                .copied()
                .unwrap_or("paired-phone")
                .into(),
        )
    }

    pub fn actions<'a>(
        &'a self,
        registry: &'a wire::RegistryState,
        locale: Locale,
    ) -> Element<'a, Message> {
        let add = content_button(
            row![Icon::Plus.view(), text(locale.tr("添加设备")).size(13)]
                .spacing(6)
                .align_y(iced::Center),
            style::TOOL_CONTROL,
        )
        .padding([0, 8])
        .on_press_maybe((!self.busy && self.draft.is_none()).then(|| self.new_device(registry)));
        let actions = row![].align_y(iced::Center).spacing(6);
        #[cfg(not(target_os = "android"))]
        let actions = actions.push(
            text_button(locale.tr("共享设备"))
                .height(style::TOOL_CONTROL)
                .on_press(Message::Share(locale.code().into())),
        );
        actions.push(add).into()
    }

    pub fn editing(&self) -> bool {
        self.draft.is_some()
    }

    pub fn view<'a, M: Clone + 'a>(
        &'a self,
        registry: &'a wire::RegistryState,
        locale: Locale,
        reduced_motion: bool,
        tools: impl Fn(usize, &str) -> Option<Element<'a, M>>,
        receipt: Option<Element<'a, M>>,
        manage: fn(Message) -> M,
    ) -> Element<'a, M> {
        let admin = |element: Element<'a, Message>| element.map(manage);
        let tr = |key: &'static str| locale.tr(key);
        let mut list = column![].spacing(4);
        if let Some(d) = self.draft.as_ref().filter(|d| d.expected_revision == 0) {
            let available = self.available(registry);
            let choices = [
                ("phone", "Android 手机"),
                ("desktop", "Linux 电脑"),
                ("paired-phone", "远程 Android"),
                ("paired-desktop", "远程电脑"),
            ]
            .into_iter()
            .filter(|(kind, _)| kind.starts_with("paired-") || available.contains(kind))
            .map(|(kind, label)| (kind, tr(label)))
            .collect::<Vec<_>>();
            let selected = choices
                .iter()
                .find(|(kind, _)| *kind == d.kind)
                .map(|(_, label)| *label);
            let labels = choices.iter().map(|(_, label)| *label).collect::<Vec<_>>();
            let name = column![
                text(tr("名称")).size(12).style(style::muted_text),
                form_input("", &d.name)
                    .id("device-name")
                    .on_input_maybe((!self.busy).then_some(Message::Name))
            ]
            .spacing(6)
            .width(Length::Fill);
            let kind = column![
                text(tr("设备类型")).size(12).style(style::muted_text),
                form_select(labels, selected, move |label| Message::Kind(
                    choices.iter().find(|(_, l)| *l == label).unwrap().0.into()
                ))
                .width(Length::Fill)
            ]
            .spacing(6)
            .width(Length::Fill);
            let apply = if d.kind.starts_with("paired-") {
                tr("生成配对码")
            } else {
                tr("保存")
            };
            list = list.push(admin(
                column![
                    row![name, kind].spacing(12).align_y(iced::Bottom),
                    row![
                        space::horizontal(),
                        text_button(tr("撤销"))
                            .on_press_maybe((!self.busy).then_some(Message::Discard)),
                        action_button(
                            apply,
                            (!self.busy && !d.name.trim().is_empty()).then_some(Message::Save)
                        )
                    ]
                    .spacing(6)
                    .align_y(iced::Center)
                ]
                .spacing(10)
                .into(),
            ));
            if !registry.devices.is_empty() {
                list = list.push(rule::horizontal(1).style(rule::weak));
            }
        }
        for (index, device) in registry.devices.iter().enumerate() {
            let editing = self
                .draft
                .as_ref()
                .filter(|d| d.expected_revision != 0 && d.id == device.id);
            let icon = if device.kind == "phone" {
                Icon::PhonePortrait
            } else {
                Icon::Screen
            };
            let glyph = container(icon.view()).width(30.).align_x(iced::Center);
            let system = device.system_name();
            let glyph: Element<'_, M> = if system.is_empty() {
                glyph.into()
            } else {
                iced::widget::tooltip(glyph, text(system).size(12), iced::widget::tooltip::Position::Bottom)
                    .gap(4).style(container::rounded_box).into()
            };
            let mut line = row![glyph]
                .align_y(iced::Center)
                .spacing(6);
            if let Some(d) = editing {
                let can_save = !self.busy && !d.name.trim().is_empty();
                let input = form_input(tr("名称"), &d.name)
                    .id("device-name")
                    .padding(6)
                    .on_input_maybe((!self.busy).then_some(Message::Name))
                    .on_submit_maybe(can_save.then_some(Message::Save));
                line = line
                    .push(admin(input.into()))
                    .push(admin(icon_button(
                        Icon::Check,
                        can_save.then_some(Message::Save),
                    )))
                    .push(admin(icon_button(
                        Icon::Close,
                        (!self.busy).then_some(Message::Discard),
                    )));
            } else {
                line = line.push(
                    container(text(&device.name).size(14).wrapping(text::Wrapping::None))
                        .width(Length::Fill)
                        .height(style::CONTROL_HEIGHT)
                        .align_y(iced::Center)
                        .clip(true)
                        .padding([0, 4]),
                );
                if let Some(controls) = tools(index, &device.id) {
                    line = line.push(controls);
                }
                if let Some(issue) = self.issues.get(&device.id) {
                    let repair = issue.retry.is_some();
                    let symbol = Icon::Warning.view().style(|_, _| iced::widget::svg::Style {
                        color: Some(iced::Color::from_rgb8(155, 91, 26)),
                    });
                    let glyph = container(symbol).center(style::CONTROL_HEIGHT);
                    let trigger: Element<'_, Message> = if repair {
                        button(glyph)
                            .padding(0)
                            .style(style::quiet)
                            .on_press_maybe(
                                (!self.starting.contains(&device.id))
                                    .then(|| Message::Repair(device.id.clone())),
                            )
                            .into()
                    } else {
                        glyph.into()
                    };
                    let title = if repair {
                        tr("启动失败")
                    } else {
                        tr("连接失败")
                    };
                    let mut tip = column![
                        text(title).size(13),
                        text(locale.tr(&issue.reason)).size(13)
                    ]
                    .spacing(4);
                    if repair {
                        tip = tip.push(text(tr("点击重试")).size(12).color(style::MUTED));
                    }
                    line = line.push(admin(
                        iced::widget::tooltip(
                            trigger,
                            container(tip).max_width(310.).padding(8),
                            iced::widget::tooltip::Position::Bottom,
                        )
                        .gap(4)
                        .style(container::rounded_box)
                        .into(),
                    ));
                } else if device.paired
                    && !device.connected
                    && self
                        .invitation
                        .as_ref()
                        .is_none_or(|(i, _)| i.device_id != device.id)
                {
                    let request = wire::DevicePairRequest {
                        id: device.id.clone(),
                        name: device.name.clone(),
                        kind: device.kind.clone(),
                    };
                    line = line.push(admin(
                        text_button(tr("生成配对码"))
                            .on_press_maybe(
                                (!self.starting.contains(&device.id))
                                    .then_some(Message::Invite(request)),
                            )
                            .into(),
                    ));
                } else if !device.paired
                    && device.kind == "desktop"
                    && device.runtime_state != "managed"
                {
                    line = line.push(admin(
                        text_button(tr("安装系统"))
                            .on_press(Message::Edit(device.id.clone()))
                            .into(),
                    ));
                }
                if crate::connection::reachable(device)
                    || self
                        .invitation
                        .as_ref()
                        .is_some_and(|(inv, _)| inv.device_id == device.id)
                    || device.runtime_state == "stopping"
                {
                    line = line.push(admin(
                        iced::widget::tooltip(
                            icon_button(
                                Icon::Stop,
                                (!self.busy).then(|| Message::Disconnect(device.id.clone())),
                            ),
                            text(tr("断开")).size(12),
                            iced::widget::tooltip::Position::Bottom,
                        )
                        .style(container::rounded_box)
                        .into(),
                    ));
                }
                line = line
                    .push(admin(icon_button(
                        Icon::Edit,
                        (!self.busy).then(|| Message::Edit(device.id.clone())),
                    )))
                    .push(admin(icon_button(
                        Icon::Trash,
                        (!self.busy).then(|| Message::AskDelete(device.id.clone())),
                    )));
            }
            list = list.push(
                container(line)
                    .padding(4)
                    .height(52)
                    .align_y(iced::Center)
                    .width(Length::Fill),
            );
            if self.delete.as_deref() == Some(&device.id) {
                let confirm = row![
                    text(tr(if device.paired {
                        "移除设备共享？"
                    } else {
                        "删除设备及其数据？"
                    }))
                    .size(13),
                    space::horizontal(),
                    text_button(tr("取消")).on_press(Message::CancelDelete),
                    text_button(tr("删除")).on_press(Message::Delete(device.id.clone()))
                ]
                .spacing(8)
                .align_y(iced::Center);
                list = list.push(admin(
                    container(confirm)
                        .padding([2, 8])
                        .width(Length::Fill)
                        .into(),
                ));
            }
            let invitation = self
                .invitation
                .as_ref()
                .filter(|(i, _)| i.device_id == device.id);
            if editing.is_some() || invitation.is_some() {
                let mut details: Vec<Element<'a, Message>> = Vec::new();
                let desktop = device.kind == "desktop";
                if device.paired {
                    if let Some((invite, _)) = invitation {
                        details.push(
                            row![
                                text(&invite.code).size(13),
                                icon_button(Icon::Copy, Some(Message::CopyCode))
                            ]
                            .spacing(6)
                            .align_y(iced::Center)
                            .into(),
                        );
                    }
                } else if device.runtime_state == "managed"
                    && let (Some(applied), Some(edited)) = (&self.applied, &self.resources)
                {
                    let original = applied.guest(desktop);
                    let guest = edited.guest(desktop);
                    details.push(deployment::resource_controls(
                        desktop,
                        guest,
                        Some(original.storage(desktop)),
                        locale,
                        reduced_motion,
                        #[cfg(target_os = "android")]
                        None,
                        move |f, v| Message::Resource(desktop, f, v),
                    ));
                    if original != guest {
                        details.push(
                            action_button(
                                tr("应用并重启两台设备"),
                                (!self.busy).then_some(Message::Restart),
                            )
                            .into(),
                        );
                    }
                } else if desktop
                    && device.runtime_state != "managed"
                    && !self.issues.contains_key(&device.id)
                {
                    let cpu = column![
                        text(tr("处理器")).size(12).style(style::muted_text),
                        form_select([1, 2, 4], Some(self.cpus), Message::Cpu).width(Length::Fill)
                    ]
                    .spacing(6)
                    .width(Length::Fill);
                    let mem = column![
                        text(tr("内存")).size(12).style(style::muted_text),
                        form_select(
                            [2048, 2560, 3072, 4096].map(Memory),
                            Some(Memory(self.memory)),
                            Message::Memory
                        )
                        .width(Length::Fill)
                    ]
                    .spacing(6)
                    .width(Length::Fill);
                    details.push(row![cpu, mem].spacing(12).into());
                    let stage = self
                        .jobs
                        .iter()
                        .find(|j| {
                            j.device_id == device.id
                                && matches!(j.status.as_str(), "queued" | "running")
                        })
                        .map_or(tr("安装系统"), |j| j.stage.as_str());
                    details.push(
                        action_button(
                            stage,
                            (!self.starting.contains(&device.id)
                                && !self.jobs.iter().any(|j| {
                                    j.device_id == device.id
                                        && matches!(j.status.as_str(), "queued" | "running")
                                }))
                            .then_some(Message::Install),
                        )
                        .into(),
                    );
                }
                if !details.is_empty() {
                    list = list.push(admin(
                        container(column(details).spacing(8))
                            .padding(iced::Padding::ZERO.left(44.).right(4.).bottom(6.))
                            .width(Length::Fill)
                            .into(),
                    ));
                }
            }
            if index + 1 < registry.devices.len() {
                list = list.push(rule::horizontal(1).style(rule::weak));
            }
        }
        if let Some(receipt) = receipt {
            list = list
                .push(rule::horizontal(1).style(rule::weak))
                .push(container(receipt).padding([2, 4]).width(Length::Fill));
        }
        if !self.error.is_empty() {
            list = list.push(text(&self.error).size(12).color(style::ERROR));
        }
        list.into()
    }
}
