use crate::{
    connection::Connection,
    controls::{
        Icon, action_button, content_button, form_input, form_select, sized_icon_button, tab_button,
    },
    style,
    surface::{tool_action, tool_header, tool_padding, tool_scroll},
};
use ai_phone_api::wire;
use iced::{
    Border, Element, Length, Size, Task,
    widget::{button, column, container, row, rule, scrollable, sensor, space, text},
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
    ListMeasured(f32),
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
    pub fn editor(&self) -> Option<(&str, Tab)> {
        let id = self.current.as_deref()?;
        Some((id, self.editors.get(id)?.tab))
    }
    pub fn adding(&self) -> bool {
        self.current.as_deref() == Some("")
    }
    pub fn prune_editors(&mut self, keep_current: bool) {
        self.editors.retain(|id, editor| {
            (keep_current && self.current.as_ref() == Some(id))
                || self
                    .registry
                    .agents
                    .iter()
                    .find(|agent| agent.id == editor.agent.id)
                    .map_or(editor.agent.revision == 0, |agent| {
                        agent.revision == editor.agent.revision
                    })
        });
    }
    pub fn update(&mut self, message: Message, client: &Connection) -> Task<Message> {
        if self.busy && matches!(message, Message::Field(..) | Message::Provider(_)) {
            return Task::none();
        }
        match message {
            Message::Measured(_) | Message::ListMeasured(_) => {}
            Message::New | Message::Edit(_) => {
                self.prune_editors(false);
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
                        self.prune_editors(true);
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
    pub fn view<'a>(
        &'a self,
        locale: crate::locale::Locale,
        editing_tab: Option<(&str, Tab)>,
        width: f32,
        reduced_motion: bool,
    ) -> Element<'a, Message> {
        let adding = editing_tab.is_some_and(|(id, _)| id.is_empty());
        let editing = editing_tab.is_some();
        let control = style::CONTROL_HEIGHT;
        let (_, _, header_height) = style::window_insets(width);
        let padding = tool_padding(width);
        let title = if adding {
            locale.tr("添加 Agent")
        } else if editing {
            locale.tr("编辑 Agent")
        } else {
            "Agent"
        };
        let actions = if adding {
            row![]
        } else if editing {
            row![tool_action(Icon::Close, Some(Message::Back))]
        } else {
            let open = self.adding();
            row![tool_action(
                if open { Icon::Close } else { Icon::Plus },
                (!self.busy).then_some(if open { Message::Back } else { Message::New }),
            )]
        };
        let mut heading = column![tool_header(title, width, actions)];
        let mut body = column![].spacing(if editing { 8 } else { 0 });
        let mut footer = row![].spacing(10).align_y(iced::Center);
        let model_field = |label: &'a str, value: &'a str, field| {
            column![
                text(label).size(12).style(style::muted_text),
                form_input("", value)
                    .on_input_maybe(
                        (!self.busy).then_some(move |value| Message::Field(field, value))
                    )
                    .style(style::registry_input),
            ]
            .spacing(6)
        };
        if let Some((id, current_tab)) = editing_tab
            && let Some(editor) = self.editors.get(id)
        {
            if editor.agent.revision > 0 {
                heading = heading.push(
                    container(
                        row([
                            (Tab::Model, locale.tr("配置")),
                            (Tab::Memory, locale.tr("记忆")),
                            (Tab::Voice, locale.tr("语音")),
                        ]
                        .into_iter()
                        .map(|(tab, label)| {
                            tab_button(
                                label,
                                current_tab == tab,
                                (!self.busy).then_some(Message::Tab(tab)),
                            )
                        }))
                        .spacing(6),
                    )
                    .padding(padding.top(0).bottom(0)),
                );
            }
            if !self.error.is_empty() {
                body = body.push(text(&self.error).size(14).style(style::error_text));
            }
            if current_tab != Tab::Model {
                body = body.push(
                    editor
                        .details
                        .view(current_tab, locale)
                        .map(|m| Message::Detail(editor.agent.id.clone(), m)),
                );
                footer = footer.push(space::horizontal()).push(
                    button(container(text(locale.tr("返回")).size(14)).center_y(Length::Fill))
                        .height(control)
                        .padding([0, 17])
                        .style(style::quiet)
                        .on_press(Message::Back),
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
                    text(locale.tr("服务商")).size(12).color(style::MUTED),
                    form_select(self.providers.clone(), selected, Message::Provider)
                        .placeholder(locale.tr("兼容服务"))
                        .width(Length::Fill)
                        .style(style::registry_select)
                ]
                .spacing(6);
                body = body
                    .push(model_field(locale.tr("名称"), &editor.name, Field::Name))
                    .push(provider)
                    .push(model_field(
                        locale.tr("模型"),
                        &editor.draft.model,
                        Field::Model,
                    ))
                    .push(model_field(
                        locale.tr("API 地址"),
                        &editor.draft.base_url,
                        Field::Url,
                    ))
                    .push(
                        column![
                            text("API Key").size(12).color(style::MUTED),
                            row![
                                form_input(
                                    if editor.draft.use_saved_key {
                                        locale.tr("已保存")
                                    } else {
                                        locale.tr("填写 API Key")
                                    },
                                    &editor.draft.api_key
                                )
                                .secure(!editor.visible)
                                .style(style::registry_input)
                                .on_input_maybe(
                                    (!self.busy).then_some(|v| Message::Field(Field::Key, v))
                                ),
                                sized_icon_button(
                                    if editor.visible {
                                        Icon::EyeOff
                                    } else {
                                        Icon::Eye
                                    },
                                    (!self.busy).then_some(Message::Reveal),
                                    control,
                                    control,
                                )
                            ]
                            .spacing(4)
                        ]
                        .spacing(6),
                    )
                    .push(
                        content_button(
                            row![
                                if editor.advanced {
                                    Icon::Down
                                } else {
                                    Icon::Right
                                }
                                .view(),
                                text(locale.tr("高级")).size(12)
                            ]
                            .spacing(6)
                            .align_y(iced::Center),
                            control,
                        )
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
                                text(locale.tr("协议")).size(12).color(style::MUTED),
                                form_select(protocols, Some(editor.draft.protocol.clone()), |v| {
                                    Message::Field(Field::Protocol, v)
                                })
                                .width(Length::Fill)
                                .style(style::registry_select)
                            ]
                            .spacing(6),
                        )
                        .push(model_field(
                            locale.tr("代理地址"),
                            &editor.draft.proxy_url,
                            Field::Proxy,
                        ));
                }
                footer = footer
                    .push(space::horizontal())
                    .push(
                        button(container(text(locale.tr("取消")).size(14)).center_y(Length::Fill))
                            .height(control)
                            .padding([0, 17])
                            .style(style::quiet)
                            .on_press_maybe((!self.busy).then_some(Message::Back)),
                    )
                    .push(
                        button(
                            container(crate::loading::label(
                                if self.busy {
                                    locale.tr("正在保存…")
                                } else {
                                    locale.tr("保存")
                                },
                                14,
                                self.busy,
                                reduced_motion,
                            ))
                            .center_y(Length::Fill),
                        )
                        .height(control)
                        .padding([0, 17])
                        .style(style::primary)
                        .on_press_maybe((!self.busy).then_some(Message::Save)),
                    );
            }
        } else {
            if !self.error.is_empty() && !self.adding() {
                body = body.push(text(&self.error).size(14).style(style::error_text));
            }
            body = body.push(rule::horizontal(1));
            if self.registry.agents.is_empty() {
                body = body.push(
                    container(
                        text(locale.tr("暂无 Agent"))
                            .size(14)
                            .style(style::muted_text),
                    )
                    .padding([20, 0]),
                );
            }
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
                    style::section_title(&agent.name),
                    text(subtitle).size(12).style(style::muted_text)
                ]
                .spacing(3);
                let actions = if self.deleting.as_ref() == Some(&agent.id) {
                    row![
                        action_button(
                            locale.tr("取消"),
                            (!self.busy).then_some(Message::CancelDelete)
                        )
                        .style(style::quiet),
                        action_button(
                            locale.tr("删除"),
                            (!self.busy).then_some(Message::ConfirmDelete)
                        )
                        .style(|theme, status| {
                            let mut s = button::danger(theme, status);
                            s.border.radius = style::CONTROL_RADIUS.into();
                            s
                        })
                    ]
                } else {
                    row![
                        sized_icon_button(
                            Icon::Edit,
                            (!self.busy).then_some(Message::Edit(agent.id.clone())),
                            control,
                            control,
                        ),
                        sized_icon_button(
                            Icon::Trash,
                            (!self.busy).then_some(Message::Delete(agent.id.clone())),
                            control,
                            control,
                        )
                    ]
                };
                let active = agent.id == self.registry.active_agent_id;
                let mark = container(
                    Icon::Check
                        .view()
                        .opacity(if active { 1. } else { 0. })
                        .style(|theme, _| iced::widget::svg::Style {
                            color: Some(theme.palette().background),
                        }),
                )
                .width(19)
                .height(19)
                .center(19)
                .style(move |theme| {
                    container::Style::default()
                        .border(Border {
                            radius: (10.).into(),
                            width: 1.,
                            color: theme
                                .palette()
                                .text
                                .scale_alpha(if active { 1. } else { 0.55 }),
                        })
                        .background(if active {
                            theme.palette().text
                        } else {
                            iced::Color::TRANSPARENT
                        })
                });
                body = body
                    .push(
                        row![
                            content_button(
                                row![mark, label]
                                    .spacing(12)
                                    .align_y(iced::Alignment::Center),
                                64.,
                            )
                            .width(Length::Fill)
                            .padding(0)
                            .on_press_maybe(
                                (!self.busy).then_some(Message::Select(agent.id.clone()))
                            ),
                            actions
                        ]
                        .align_y(iced::Alignment::Center),
                    )
                    .push(rule::horizontal(1));
            }
        }
        let footer = container(column![rule::horizontal(1), footer].spacing(13))
            .padding(padding.top(0))
            .width(Length::Fill);
        let padded = container(body)
            .padding(padding.bottom(16))
            .width(Length::Fill);
        let content: Element<'a, Message> = if adding {
            sensor(padded)
                .on_resize(move |size| {
                    Message::Measured(Size::new(
                        size.width,
                        size.height + header_height + 14. + control + padding.bottom,
                    ))
                })
                .into()
        } else if !editing {
            sensor(padded)
                .on_resize(move |size| {
                    Message::ListMeasured(
                        (size.height + header_height + 14. + control + padding.bottom).ceil(),
                    )
                })
                .into()
        } else {
            padded.into()
        };
        column![
            heading,
            scrollable(content)
                .direction(tool_scroll())
                .height(Length::Fill),
        ]
        .extend(editing.then_some(footer.into()))
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
