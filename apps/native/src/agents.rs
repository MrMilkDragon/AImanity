use crate::{
    connection::Connection,
    controls::{Icon, action_button, field, icon_button, tab_button},
    style,
};
use ai_phone_api::wire;
use iced::{
    Element, Length, Size, Task,
    widget::{
        button, column, container, pick_list, row, scrollable, sensor, space, text, text_input,
    },
};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    id: String,
    label: String,
    provider: String,
    protocol: String,
    base_url: String,
    model: String,
}
impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Model,
    Connections,
    Memory,
    Voice,
}
#[derive(Clone, Copy, Debug)]
pub enum Field {
    Name,
    Url,
    Model,
    Key,
    Proxy,
    Protocol,
}
#[derive(Clone, Debug)]
pub enum Message {
    Measured(Size),
    New,
    Edit(String),
    Back,
    Field(Field, String),
    Provider(Provider),
    Advanced,
    Reveal,
    Credential(Result<wire::ModelCredential, String>),
    Save,
    Select(String),
    Delete(String),
    ConfirmDelete,
    CancelDelete,
    Registry(Result<wire::RegistryState, String>),
    Tab(Tab),
    Detail(String, super::agent_details::Message),
}
struct Editor {
    agent: wire::RegisteredAgent,
    model: wire::RegisteredModel,
    name: String,
    draft: wire::ModelDraft,
    original: wire::ModelDraft,
    visible: bool,
    advanced: bool,
    tab: Tab,
    details: super::agent_details::Details,
}
pub struct Agents {
    pub registry: wire::RegistryState,
    providers: Vec<Provider>,
    editors: HashMap<String, Editor>,
    current: Option<String>,
    deleting: Option<String>,
    busy: bool,
    pub error: String,
}
impl Agents {
    pub fn new() -> Self {
        Self {
            registry: Default::default(),
            providers: serde_json::from_str(include_str!("../../../config/model-providers.json"))
                .expect("bundled provider catalogue"),
            editors: HashMap::new(),
            current: None,
            deleting: None,
            busy: false,
            error: String::new(),
        }
    }
    pub fn is_editing(&self) -> bool {
        self.current.is_some()
    }
    pub fn update(&mut self, message: Message, client: &Connection) -> Task<Message> {
        if self.busy && matches!(message, Message::Field(..) | Message::Provider(_)) {
            return Task::none();
        }
        match message {
            Message::Measured(_) => {}
            Message::New | Message::Edit(_) => {
                let id = if let Message::Edit(id) = message {
                    id
                } else {
                    String::new()
                };
                self.editors.entry(id.clone()).or_insert_with(|| {
                    let mut agent = self
                        .registry
                        .agents
                        .iter()
                        .find(|a| a.id == id)
                        .cloned()
                        .unwrap_or_default();
                    let mut model = self
                        .registry
                        .models
                        .iter()
                        .find(|m| m.id == agent.model_connection_id)
                        .cloned()
                        .unwrap_or_default();
                    if agent.id.is_empty() {
                        agent.id = uuid::Uuid::now_v7().to_string();
                    }
                    if model.id.is_empty() {
                        model.id = uuid::Uuid::now_v7().to_string();
                    }
                    let preset = &self.providers[0];
                    let draft = match &model.profile {
                        Some(p) => wire::ModelDraft {
                            provider: p.provider.clone(),
                            protocol: p.protocol.clone(),
                            base_url: p.base_url.clone(),
                            model: p.model.clone(),
                            proxy_url: p.proxy_url.clone(),
                            use_saved_key: p.has_api_key,
                            ..Default::default()
                        },
                        None => wire::ModelDraft {
                            provider: preset.provider.clone(),
                            protocol: preset.protocol.clone(),
                            base_url: preset.base_url.clone(),
                            model: preset.model.clone(),
                            ..Default::default()
                        },
                    };
                    Editor {
                        name: agent.name.clone(),
                        agent,
                        model,
                        original: draft.clone(),
                        draft,
                        visible: false,
                        advanced: false,
                        tab: Tab::Model,
                        details: super::agent_details::Details::new(id.clone()),
                    }
                });
                self.current = Some(id);
                self.error.clear();
            }
            Message::Detail(id, message) => {
                if let Some(editor) = self.editors.get_mut(&id) {
                    return editor
                        .details
                        .update(message, client)
                        .map(move |m| Message::Detail(id.clone(), m));
                }
            }
            Message::Back => {
                self.current = None;
                self.deleting = None;
                self.error.clear();
            }
            Message::Registry(result) => {
                self.busy = false;
                match result {
                    Ok(registry) => {
                        self.registry = registry;
                        self.editors.retain(|_, editor| {
                            self.registry
                                .agents
                                .iter()
                                .find(|a| a.id == editor.agent.id)
                                .map_or(editor.agent.revision == 0, |a| {
                                    a.revision == editor.agent.revision
                                })
                        });
                        self.current = None;
                        self.deleting = None;
                        self.error.clear();
                    }
                    Err(e) => self.error = e,
                }
            }
            Message::Delete(id) => self.deleting = Some(id),
            Message::CancelDelete => self.deleting = None,
            Message::Select(id) => {
                if let Some(agent) = self.registry.agents.iter().find(|a| a.id == id) {
                    let input = wire::AgentProfileRequest {
                        id: agent.id.clone(),
                        expected_revision: agent.revision,
                    };
                    self.busy = true;
                    let client = client.clone();
                    return Task::perform(
                        async move { client.rpc("registry/agent-select", input).await },
                        Message::Registry,
                    );
                }
            }
            Message::ConfirmDelete => {
                if let Some(agent) = self
                    .registry
                    .agents
                    .iter()
                    .find(|a| Some(&a.id) == self.deleting.as_ref())
                {
                    let input = wire::AgentProfileRequest {
                        id: agent.id.clone(),
                        expected_revision: agent.revision,
                    };
                    self.busy = true;
                    let client = client.clone();
                    return Task::perform(
                        async move { client.rpc("registry/agent-delete", input).await },
                        Message::Registry,
                    );
                }
            }
            message => {
                let Some(editor) = self
                    .current
                    .as_ref()
                    .and_then(|id| self.editors.get_mut(id))
                else {
                    return Task::none();
                };
                match message {
                    Message::Field(field, value) => match field {
                        Field::Name => editor.name = value,
                        Field::Model => editor.draft.model = value,
                        Field::Proxy => editor.draft.proxy_url = value,
                        Field::Protocol => editor.draft.protocol = value,
                        Field::Key => {
                            editor.draft.api_key = value;
                            editor.draft.use_saved_key = false;
                        }
                        Field::Url => {
                            editor.draft.base_url = value;
                            editor.draft.use_saved_key = false;
                            editor.draft.api_key.clear();
                        }
                    },
                    Message::Provider(p) => {
                        editor.draft = wire::ModelDraft {
                            provider: p.provider,
                            protocol: p.protocol,
                            base_url: p.base_url,
                            model: p.model,
                            ..Default::default()
                        };
                    }
                    Message::Advanced => editor.advanced = !editor.advanced,
                    Message::Reveal => {
                        editor.visible = !editor.visible;
                        if editor.visible && editor.draft.use_saved_key {
                            self.busy = true;
                            let input = wire::ConnectionCredentialRequest {
                                id: editor.model.id.clone(),
                                expected_revision: editor.model.revision,
                            };
                            let client = client.clone();
                            return Task::perform(
                                async move { client.rpc("registry/credential", input).await },
                                Message::Credential,
                            );
                        }
                    }
                    Message::Credential(result) => {
                        self.busy = false;
                        match result {
                            Ok(v) => editor.draft.api_key = v.api_key,
                            Err(e) => self.error = e,
                        }
                    }
                    Message::Tab(tab) => {
                        editor.tab = tab;
                        let id = editor.agent.id.clone();
                        return editor
                            .details
                            .load(tab, client)
                            .map(move |m| Message::Detail(id.clone(), m));
                    }

                    Message::Save => {
                        if editor.draft.model.trim().is_empty()
                            || editor.draft.base_url.trim().is_empty()
                        {
                            self.error = "请填写模型和 API 地址".into();
                            return Task::none();
                        }
                        let mut draft = editor.draft.clone();
                        if draft.use_saved_key {
                            draft.api_key.clear();
                        }
                        let changed = editor.model.profile.is_none() || draft != editor.original;
                        let name = if editor.name.trim().is_empty() {
                            draft.model.clone()
                        } else {
                            editor.name.trim().into()
                        };
                        let mut input = wire::AgentConfigurationSaveRequest {
                            agent: Some(wire::AgentSaveRequest {
                                id: editor.agent.id.clone(),
                                expected_revision: editor.agent.revision,
                                name: name.clone(),
                                model_connection_id: editor.model.id.clone(),
                                instructions: editor.agent.instructions.clone(),
                            }),
                            connection: None,
                        };
                        let test = wire::ConnectionTestRequest {
                            id: editor.model.id.clone(),
                            expected_revision: editor.model.revision,
                            draft: Some(draft.clone()),
                        };
                        let model_id = editor.model.id.clone();
                        let revision = editor.model.revision;
                        self.busy = true;
                        self.error.clear();
                        let client = client.clone();
                        return Task::perform(
                            async move {
                                if changed || model_id.is_empty() {
                                    let result: wire::ModelTestResult =
                                        client.rpc("registry/model-test", test).await?;
                                    if !result.ready {
                                        return Err(result
                                            .checks
                                            .into_iter()
                                            .find(|c| !c.passed)
                                            .map(|c| c.detail)
                                            .unwrap_or_else(|| "连接检测失败".into()));
                                    }
                                    input.connection = Some(wire::ConnectionSaveRequest {
                                        id: model_id,
                                        expected_revision: revision,
                                        name,
                                        draft: Some(draft),
                                        verification_id: result.verification_id,
                                    });
                                }
                                client.rpc("registry/agent-config-save", input).await
                            },
                            Message::Registry,
                        );
                    }
                    _ => {}
                }
            }
        }
        Task::none()
    }
    pub fn subscription(&self, open: bool) -> iced::Subscription<Message> {
        match self.current.as_ref().and_then(|id| self.editors.get(id)) {
            Some(editor) if open => {
                let id = editor.agent.id.clone();
                editor
                    .details
                    .subscription(editor.tab)
                    .with(id)
                    .map(|(id, m)| Message::Detail(id, m))
            }
            _ => iced::Subscription::none(),
        }
    }
    pub fn view(&self) -> Element<'_, Message> {
        let mut body = column![].spacing(16);
        if let Some(editor) = self.current.as_ref().and_then(|id| self.editors.get(id)) {
            body = body.push(
                row![
                    icon_button(Icon::Back, "返回", (!self.busy).then_some(Message::Back)),
                    text(if editor.agent.revision == 0 {
                        "添加 Agent"
                    } else {
                        "Agent"
                    })
                    .size(14)
                ]
                .align_y(iced::Alignment::Center),
            );
            if editor.agent.revision > 0 {
                body = body.push(
                    row([
                        (Tab::Model, "模型"),
                        (Tab::Connections, "连接应用"),
                        (Tab::Memory, "记忆"),
                        (Tab::Voice, "语音"),
                    ]
                    .into_iter()
                    .map(|(tab, label)| {
                        tab_button(
                            label,
                            editor.tab == tab,
                            (!self.busy).then_some(Message::Tab(tab)),
                        )
                    }))
                    .spacing(6),
                );
            }
            if editor.tab != Tab::Model {
                body = body.push(
                    editor
                        .details
                        .view(editor.tab)
                        .map(|m| Message::Detail(editor.agent.id.clone(), m)),
                );
            } else {
                let selected = self
                    .providers
                    .iter()
                    .find(|p| {
                        p.provider == editor.draft.provider && p.base_url == editor.draft.base_url
                    })
                    .cloned();
                let provider = column![
                    text("服务商").size(12).color(style::MUTED),
                    pick_list(self.providers.clone(), selected, Message::Provider)
                        .placeholder("自定义")
                        .width(Length::Fill)
                        .padding([10, 12])
                        .text_size(14)
                        .text_line_height(iced::Pixels(20.))
                        .style(style::select)
                        .menu_style(style::menu)
                ]
                .spacing(6);
                body = body
                    .push(field(
                        "名称",
                        &editor.name,
                        |v| Message::Field(Field::Name, v),
                        !self.busy,
                    ))
                    .push(provider)
                    .push(field(
                        "模型",
                        &editor.draft.model,
                        |v| Message::Field(Field::Model, v),
                        !self.busy,
                    ))
                    .push(field(
                        "API 地址",
                        &editor.draft.base_url,
                        |v| Message::Field(Field::Url, v),
                        !self.busy,
                    ))
                    .push(
                        column![
                            text("API Key").size(12).color(style::MUTED),
                            row![
                                text_input(
                                    if editor.draft.use_saved_key {
                                        "已保存"
                                    } else {
                                        ""
                                    },
                                    &editor.draft.api_key
                                )
                                .secure(!editor.visible)
                                .size(14)
                                .line_height(iced::Pixels(20.))
                                .padding([10, 12])
                                .style(style::input)
                                .on_input_maybe(
                                    (!self.busy).then_some(|v| Message::Field(Field::Key, v))
                                ),
                                icon_button(
                                    if editor.visible {
                                        Icon::EyeOff
                                    } else {
                                        Icon::Eye
                                    },
                                    "显示 / 隐藏密钥",
                                    (!self.busy).then_some(Message::Reveal)
                                )
                            ]
                            .spacing(4)
                        ]
                        .spacing(6),
                    )
                    .push(
                        button(
                            row![
                                if editor.advanced {
                                    Icon::Down
                                } else {
                                    Icon::Right
                                }
                                .view(),
                                text("高级").size(12)
                            ]
                            .spacing(6),
                        )
                        .style(style::quiet)
                        .on_press(Message::Advanced),
                    );
                if editor.advanced {
                    let protocols = vec![
                        "chat-completions".to_string(),
                        "responses".into(),
                        "anthropic-messages".into(),
                    ];
                    body = body
                        .push(
                            column![
                                text("协议").size(12).color(style::MUTED),
                                pick_list(protocols, Some(editor.draft.protocol.clone()), |v| {
                                    Message::Field(Field::Protocol, v)
                                })
                                .width(Length::Fill)
                                .padding([10, 12])
                                .text_size(14)
                                .text_line_height(iced::Pixels(20.))
                                .style(style::select)
                                .menu_style(style::menu)
                            ]
                            .spacing(6),
                        )
                        .push(field(
                            "代理地址",
                            &editor.draft.proxy_url,
                            |v| Message::Field(Field::Proxy, v),
                            !self.busy,
                        ));
                }
                body = body.push(action_button(
                    if self.busy {
                        "正在保存…"
                    } else {
                        "保存"
                    },
                    (!self.busy).then_some(Message::Save),
                ));
            }
        } else {
            body = body.push(
                row![
                    text("Agent").size(14),
                    space::horizontal(),
                    icon_button(
                        Icon::Plus,
                        "添加 Agent",
                        (!self.busy).then_some(Message::New)
                    )
                ]
                .align_y(iced::Alignment::Center),
            );
            for agent in &self.registry.agents {
                let subtitle = self
                    .registry
                    .models
                    .iter()
                    .find(|m| m.id == agent.model_connection_id)
                    .and_then(|m| m.profile.as_ref())
                    .map(|p| format!("{} · {}", p.provider, p.model))
                    .unwrap_or_default();
                let label = column![
                    text(&agent.name).size(14),
                    text(subtitle).size(12).color(style::MUTED)
                ]
                .spacing(4);
                let actions = if self.deleting.as_ref() == Some(&agent.id) {
                    row![
                        action_button("取消", (!self.busy).then_some(Message::CancelDelete))
                            .style(style::quiet)
                            .width(64),
                        action_button("删除", (!self.busy).then_some(Message::ConfirmDelete))
                            .width(64)
                            .style(|theme, status| {
                                let mut s = button::danger(theme, status);
                                s.border.radius = style::CONTROL_RADIUS.into();
                                s
                            })
                    ]
                } else {
                    row![
                        icon_button(
                            Icon::Edit,
                            "编辑",
                            (!self.busy).then_some(Message::Edit(agent.id.clone()))
                        ),
                        icon_button(
                            Icon::Trash,
                            "删除",
                            (!self.busy).then_some(Message::Delete(agent.id.clone()))
                        )
                    ]
                };
                body = body.push(
                    row![
                        button(
                            row![
                                container(if agent.id == self.registry.active_agent_id {
                                    Icon::Check.view()
                                } else {
                                    Icon::Right.view()
                                })
                                .width(24),
                                label
                            ]
                            .spacing(8)
                            .align_y(iced::Alignment::Center)
                        )
                        .width(Length::Fill)
                        .style(style::quiet)
                        .on_press_maybe((!self.busy).then_some(Message::Select(agent.id.clone()))),
                        actions
                    ]
                    .align_y(iced::Alignment::Center),
                );
            }
        }
        if !self.error.is_empty() {
            body = body.push(text(&self.error).size(12).color(style::ERROR));
        }
        scrollable(sensor(container(body).padding(24)).on_resize(Message::Measured))
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(4).scroller_width(4),
            ))
            .height(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_profile_keeps_valid_ids_and_draft_across_popover_navigation() {
        let client = Connection::for_state_test();
        let mut agents = Agents::new();
        let _ = agents.update(Message::New, &client);
        let _ = agents.update(Message::Field(Field::Name, "我的模型".into()), &client);
        let editor = &agents.editors[""];
        let id = editor.agent.id.clone();
        let model = editor.model.id.clone();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        assert!(uuid::Uuid::parse_str(&model).is_ok());
        assert_ne!(id, model);
        let _ = agents.update(Message::Back, &client);
        let _ = agents.update(Message::New, &client);
        assert_eq!(agents.editors[""].agent.id, id);
        assert_eq!(agents.editors[""].name, "我的模型");
    }
}
