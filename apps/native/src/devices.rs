use crate::{
    connection::Connection,
    controls::{Icon, action_button, field, icon_button},
    locale::Locale,
    style,
};
use ai_phone_api::wire;
use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, pick_list, row, space, text},
};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Message {
    Choose(String),
    New(String),
    Name(String),
    Kind(String),
    Discard,
    Save,
    Saved(Result<wire::RegistryState, String>),
    Check,
    Jobs(Result<wire::DeviceLinkState, String>),
    Cpu(u32),
    Memory(u32),
    Install,
    Installed(Result<wire::DesktopProvisionJob, String>),
}

pub struct Devices {
    selected: String,
    draft: Option<wire::DeviceSaveRequest>,
    busy: bool,
    error: String,
    cpus: u32,
    memory: u32,
    job: Option<wire::DesktopProvisionJob>,
    pub registry: Option<wire::RegistryState>,
}
impl Default for Devices {
    fn default() -> Self {
        Self {
            selected: String::new(),
            draft: None,
            busy: false,
            error: String::new(),
            cpus: 2,
            memory: 2560,
            job: None,
            registry: None,
        }
    }
}
impl Devices {
    fn available(&self, registry: &wire::RegistryState) -> Vec<&'static str> {
        [
            ("phone", registry.phone_limit),
            ("desktop", registry.desktop_limit),
        ]
        .into_iter()
        .filter(|(kind, max)| {
            registry.devices.iter().filter(|d| &d.kind == kind).count() < *max as usize
        })
        .map(|(kind, _)| kind)
        .collect()
    }
    fn running(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|j| matches!(j.status.as_str(), "queued" | "running"))
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
            Message::Choose(id) => {
                self.selected = id;
                self.draft = None;
                return self.update(Message::Check, registry, client);
            }
            Message::New(kind) => {
                let id = uuid::Uuid::now_v7().to_string();
                self.selected = id.clone();
                self.draft = Some(wire::DeviceSaveRequest {
                    id,
                    kind,
                    ..Default::default()
                });
            }
            Message::Name(name) | Message::Kind(name) => {
                if self.draft.is_none()
                    && let Some(device) = registry
                        .devices
                        .iter()
                        .find(|d| d.id == self.selected)
                        .or_else(|| registry.devices.first())
                {
                    self.draft = Some(wire::DeviceSaveRequest {
                        id: device.id.clone(),
                        expected_revision: device.revision,
                        name: device.name.clone(),
                        kind: device.kind.clone(),
                    });
                }
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
                    let was_running = self.running();
                    self.job = state
                        .jobs
                        .into_iter()
                        .find(|j| j.device_id == self.selected);
                    if was_running && self.job.as_ref().is_some_and(|j| j.status == "completed") {
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
            Message::Memory(n) => self.memory = n,
            Message::Install => {
                self.busy = true;
                self.error.clear();
                let client = client.clone();
                let request = wire::DesktopProvisionRequest {
                    device_id: self.selected.clone(),
                    request_id: uuid::Uuid::now_v7().to_string(),
                    cpus: self.cpus,
                    memory_mb: self.memory,
                    template_id: "debian13-openbox".into(),
                };
                return Task::perform(
                    async move { client.rpc("desktop/provision", request).await },
                    Message::Installed,
                );
            }
            Message::Installed(r) => {
                self.busy = false;
                match r {
                    Ok(job) => {
                        self.job = Some(job);
                    }
                    Err(e) => self.error = e,
                }
            }
        }
        Task::none()
    }
    pub fn view<'a>(
        &'a self,
        registry: &'a wire::RegistryState,
        locale: Locale,
    ) -> Element<'a, Message> {
        let tr = |key: &'static str| locale.tr(key);
        let available = self.available(registry);
        let device = registry
            .devices
            .iter()
            .find(|d| d.id == self.selected)
            .or_else(|| registry.devices.first());
        let draft = self.draft.as_ref();
        let name = draft
            .map(|d| d.name.as_str())
            .or_else(|| device.map(|d| d.name.as_str()));
        let mut list = column![].spacing(4);
        for d in &registry.devices {
            list = list.push(
                button(
                    row![Icon::Screen.view(), text(&d.name).size(13)]
                        .spacing(8)
                        .align_y(iced::Center),
                )
                .width(Length::Fill)
                .height(40)
                .style(style::quiet)
                .on_press_maybe((!self.busy).then(|| Message::Choose(d.id.clone()))),
            );
        }
        if let Some(kind) = available.first() {
            list = list.push(
                row![
                    text(tr("添加设备")).size(12),
                    icon_button(
                        Icon::Plus,
                        (!self.busy && draft.is_none_or(|d| d.expected_revision != 0))
                            .then(|| Message::New((*kind).into()))
                    )
                ]
                .align_y(iced::Center),
            );
        }
        let mut form = column![].spacing(10);
        if let Some(name) = name {
            form = form.push(field(tr("名称"), name, Message::Name, !self.busy));
        }
        if let Some(draft) = draft.filter(|d| d.expected_revision == 0) {
            let choices: Vec<_> = available
                .into_iter()
                .map(|kind| {
                    if kind == "phone" {
                        tr("Android 手机")
                    } else {
                        tr("Linux 电脑")
                    }
                })
                .collect();
            let phone = tr("Android 手机");
            form = form
                .push(text(tr("系统")).size(12).color(style::MUTED))
                .push(
                    pick_list(
                        choices,
                        Some(if draft.kind == "phone" {
                            phone
                        } else {
                            tr("Linux 电脑")
                        }),
                        move |label| {
                            Message::Kind(if label == phone { "phone" } else { "desktop" }.into())
                        },
                    )
                    .width(Length::Fill)
                    .style(style::select)
                    .menu_style(style::menu)
                    .padding([10, 12]),
                );
        } else if let Some(device) = device {
            form = form.push(
                text(device.system.replace(" · 待安装", ""))
                    .size(12)
                    .color(style::MUTED),
            );
            if device.kind == "desktop"
                && device.runtime_state != "managed"
                && device.id == self.selected
            {
                form = form
                    .push(
                        row![
                            column![
                                text(tr("处理器")).size(12),
                                pick_list([1, 2, 4], Some(self.cpus), Message::Cpu)
                                    .style(style::select)
                                    .width(Length::Fill)
                            ],
                            column![
                                text(tr("内存")).size(12),
                                pick_list(
                                    [2048, 2560, 3072, 4096],
                                    Some(self.memory),
                                    Message::Memory
                                )
                                .style(style::select)
                                .width(Length::Fill)
                            ]
                        ]
                        .spacing(8),
                    )
                    .push(action_button(
                        self.job
                            .as_ref()
                            .filter(|_| self.running())
                            .map_or(tr("安装系统"), |j| j.stage.as_str()),
                        (!self.busy && !self.running()).then_some(Message::Install),
                    ));
            }
        }
        if !self.error.is_empty() {
            form = form.push(text(&self.error).size(12).color(style::ERROR));
        }
        if let Some(job) = self.job.as_ref().filter(|j| !j.error.is_empty()) {
            form = form.push(text(&job.error).size(12).color(style::ERROR));
        }
        let actions = if let Some(draft) = draft {
            row![
                space::horizontal(),
                button(tr("撤销"))
                    .style(style::quiet)
                    .on_press_maybe((!self.busy).then_some(Message::Discard)),
                action_button(
                    tr("保存"),
                    (!self.busy && !draft.name.trim().is_empty()).then_some(Message::Save)
                )
                .width(80)
            ]
            .align_y(iced::Center)
            .spacing(8)
        } else {
            row![]
        };
        column![
            row![
                iced::widget::container(list).width(142),
                form.width(Length::Fill)
            ]
            .spacing(14),
            actions
        ]
        .spacing(12)
        .into()
    }
}
