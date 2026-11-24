use crate::{
    connection::Connection,
    controls::{
        Icon, action_button, content_button, form_input, form_select, icon_button, text_button,
    },
    deployment::{self, Field},
    locale::Locale,
    style,
};
use ai_phone_api::wire;
use iced::{
    Element, Length, Task,
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
    Resource(Field, u32),
    Start(wire::DeviceStartRequest),
    Installed(
        wire::DeviceStartRequest,
        Result<wire::RegistryState, String>,
    ),
}

#[derive(Clone, Debug)]
pub enum Startup {
    Pair(wire::DevicePairRequest),
    Virtual(wire::DeviceStartRequest),
}
#[derive(Debug)]
pub struct Issue {
    pub reason: String,
    pub retry: Option<Startup>,
}
#[derive(Default)]
pub struct Devices {
    pub issues: std::collections::HashMap<String, Issue>,
    starting: std::collections::HashSet<String>,
    pub selected: String,
    pub(super) invitation: Option<(wire::DeviceInvitation, std::time::Instant)>,
    draft: Option<wire::DeviceSaveRequest>,
    busy: bool,
    delete: Option<String>,
    error: String,
    pub registry: Option<wire::RegistryState>,
    resources: wire::VirtualDeviceProfile,
}
fn defaults(kind: &str) -> wire::VirtualDeviceProfile {
    wire::VirtualDeviceProfile {
        cpus: 2,
        memory_mb: if kind == "desktop" { 2560 } else { 2048 },
        storage_gb: if kind == "desktop" { 32 } else { 16 },
    }
}
impl Devices {
    pub fn start_failed(&mut self, d: &wire::RegisteredDevice) {
        self.issues.insert(
            d.id.clone(),
            Issue {
                reason: d.start_error.clone(),
                retry: Some(Startup::Virtual(wire::DeviceStartRequest {
                    device_id: d.id.clone(),
                    resources: d.resources,
                })),
            },
        );
    }
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
            Startup::Virtual(request) => {
                if !self.starting.insert(request.device_id.clone()) {
                    return Task::none();
                }
                let retry = request.clone();
                Task::perform(
                    async move { client.rpc("registry/device-start", request).await },
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
        if !cfg!(any(
            target_os = "macos",
            windows,
            all(target_os = "android", target_arch = "aarch64"),
            all(target_os = "linux", target_arch = "x86_64")
        )) {
            return Vec::new();
        }
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
                self.resources = d.resources.unwrap_or_else(|| defaults(&d.kind));
                self.draft = Some(wire::DeviceSaveRequest {
                    id: d.id.clone(),
                    expected_revision: d.revision,
                    name: d.name.clone(),
                    kind: d.kind.clone(),
                });
                return iced::widget::operation::focus("device-name");
            }
            Message::New(kind) => {
                self.resources = defaults(&kind);
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
                        self.resources = defaults(&name);
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
                    let start = (draft.expected_revision == 0).then(|| wire::DeviceStartRequest {
                        device_id: draft.id.clone(),
                        resources: Some(self.resources),
                    });
                    return Task::perform(
                        async move {
                            let state: wire::RegistryState =
                                client.rpc("registry/device-save", draft).await?;
                            match start {
                                Some(q) => client.rpc("registry/device-start", q).await,
                                None => Ok(state),
                            }
                        },
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
            Message::Resource(field, value) => match field {
                Field::Cpu => self.resources.cpus = value,
                Field::Memory => self.resources.memory_mb = value,
                Field::Storage => self.resources.storage_gb = value,
            },
            Message::Start(request) => {
                self.error.clear();
                return self.launch(Startup::Virtual(request), client);
            }
            Message::Installed(request, result) => {
                let id = request.device_id.clone();
                self.starting.remove(&id);
                match result {
                    Ok(state) => {
                        self.issues.remove(&id);
                        self.registry = Some(state);
                        self.draft = None;
                    }
                    Err(reason) => {
                        self.issues.insert(
                            id,
                            Issue {
                                reason,
                                retry: Some(Startup::Virtual(request)),
                            },
                        );
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
                tr("开始安装")
            };
            let mut fields = column![row![name, kind].spacing(12).align_y(iced::Bottom)];
            if !d.kind.starts_with("paired-") {
                fields = fields.push(deployment::resource_controls(
                    d.kind == "desktop",
                    &self.resources,
                    None,
                    locale,
                    reduced_motion,
                    #[cfg(target_os = "android")]
                    None,
                    Message::Resource,
                ));
            }
            list = list.push(admin(
                fields
                    .push(
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
                        .align_y(iced::Center),
                    )
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
                iced::widget::tooltip(
                    glyph,
                    text(system).size(12),
                    iced::widget::tooltip::Position::Bottom,
                )
                .gap(4)
                .style(container::rounded_box)
                .into()
            };
            let mut line = row![glyph].align_y(iced::Center).spacing(6);
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
                    && !device.connected
                    && !matches!(
                        device.runtime_state.as_str(),
                        "starting" | "booted" | "stopping" | "deleting"
                    )
                {
                    line = line.push(admin(
                        text_button(tr(if device.resources.is_some() {
                            "启动设备"
                        } else {
                            "安装系统"
                        }))
                        .on_press(Message::Start(wire::DeviceStartRequest {
                            device_id: device.id.clone(),
                            resources: Some(
                                device.resources.unwrap_or_else(|| defaults(&device.kind)),
                            ),
                        }))
                        .into(),
                    ));
                }
                if crate::connection::reachable(device)
                    || self
                        .invitation
                        .as_ref()
                        .is_some_and(|(inv, _)| inv.device_id == device.id)
                    || matches!(
                        device.runtime_state.as_str(),
                        "starting" | "booted" | "stopping"
                    )
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
                } else {
                    let existing = device.resources.map(|p| p.storage_gb);
                    details.push(deployment::resource_controls(
                        desktop,
                        &self.resources,
                        existing,
                        locale,
                        reduced_motion,
                        #[cfg(target_os = "android")]
                        None,
                        Message::Resource,
                    ));
                    let starting = matches!(device.runtime_state.as_str(), "starting" | "booted");
                    if !device.connected || device.resources != Some(self.resources) {
                        let label = if starting {
                            "正在启动"
                        } else if device.connected {
                            "应用并重启"
                        } else if device.resources.is_some() {
                            "启动设备"
                        } else {
                            "安装系统"
                        };
                        details.push(
                            action_button(
                                tr(label),
                                (!self.busy && !starting && !self.starting.contains(&device.id))
                                    .then_some(Message::Start(wire::DeviceStartRequest {
                                        device_id: device.id.clone(),
                                        resources: Some(self.resources),
                                    })),
                            )
                            .into(),
                        );
                    }
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
