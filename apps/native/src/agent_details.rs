use crate::{
    agents::Tab,
    connection::Connection,
    controls::{Icon, action_button, field, icon_button, tab_button},
    style,
};
use ai_phone_api::wire;
use iced::{
    Element, Length, Subscription, Task, keyboard,
    widget::{button, column, image, pick_list, row, space, text, text_editor},
};

#[derive(Clone, Debug)]
pub enum Message {
    Memories(Result<wire::MemoryList, String>),
    NewMemory,
    MemoryEdit(usize, text_editor::Action),
    SaveMemory(usize),
    DeleteMemory(usize),
    MemorySaved(usize, Result<wire::ModelMemory, String>),
    MemoryDeleted(usize, Result<wire::MemoryList, String>),
    Channels(Result<wire::ChannelListResponse, String>),
    Channel(String),
    AppId(String),
    Secret(String),
    Proxy(String),
    Bind(bool),
    CopyPair(String),
    Unbind,
    Pair,
    ChannelSaved(Result<wire::ChannelListResponse, String>),
    Poll,
    Voice(Result<(wire::VoiceSettings, wire::VoiceCredential), String>),
    VoiceField(VoiceField, String),
    VoiceProtocol(wire::VoiceInputProtocol),
    VoiceReveal,
    VoiceAdvanced,
    VoiceSave(bool),
    VoiceSaved(Result<wire::VoiceSettings, String>),
}
#[derive(Clone, Copy, Debug)]
pub enum VoiceField {
    Url,
    Key,
    Recognition,
    Speech,
    Voice,
    Proxy,
}
struct Memory {
    saved: wire::ModelMemory,
    draft: text_editor::Content,
}
pub struct Details {
    agent_id: String,
    error: String,
    memories: Vec<Memory>,
    memory_loaded: bool,
    memory_busy: bool,
    channels: Vec<wire::ChannelBinding>,
    channels_loaded: bool,
    channels_busy: bool,
    channel: String,
    app_id: String,
    secret: String,
    proxy: String,
    channel_drafts: std::collections::HashMap<String, (String, String, String)>,
    qr: Option<image::Handle>,
    voice: wire::VoiceDraft,
    voice_revision: u64,
    voice_loaded: bool,
    voice_busy: bool,
    voice_visible: bool,
    voice_advanced: bool,
}
impl Details {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            error: String::new(),
            memories: Vec::new(),
            memory_loaded: false,
            memory_busy: false,
            channels: Vec::new(),
            channels_loaded: false,
            channels_busy: false,
            channel: String::new(),
            app_id: String::new(),
            secret: String::new(),
            proxy: String::new(),
            channel_drafts: Default::default(),
            qr: None,
            voice: wire::VoiceDraft {
                input_protocol: wire::VoiceInputProtocol::AudioChat.into(),
                ..Default::default()
            },
            voice_revision: 0,
            voice_loaded: false,
            voice_busy: false,
            voice_visible: true,
            voice_advanced: false,
        }
    }
    pub fn load(&mut self, tab: Tab, client: &Connection) -> Task<Message> {
        let client = client.clone();
        let agent_id = self.agent_id.clone();
        match tab {
            Tab::Memory if !self.memory_loaded && !self.memory_busy => {
                self.memory_busy = true;
                Task::perform(
                    async move {
                        client
                            .rpc(
                                "workbench/memories",
                                wire::WorkbenchRequest {
                                    agent_id,
                                    ..Default::default()
                                },
                            )
                            .await
                    },
                    Message::Memories,
                )
            }
            Tab::Connections if !self.channels_loaded && !self.channels_busy => {
                self.channels_busy = true;
                Task::perform(
                    async move {
                        client
                            .rpc("channels/list", wire::ChannelListRequest { agent_id })
                            .await
                    },
                    Message::Channels,
                )
            }
            Tab::Voice if !self.voice_loaded && !self.voice_busy => {
                self.voice_busy = true;
                Task::perform(
                    async move {
                        let settings: wire::VoiceSettings = client
                            .rpc(
                                "voice/settings",
                                wire::VoiceSettingsRequest {
                                    agent_id: agent_id.clone(),
                                },
                            )
                            .await?;
                        let credential = if settings.has_api_key {
                            client
                                .rpc(
                                    "voice/credential",
                                    wire::VoiceCredentialRequest {
                                        agent_id,
                                        expected_revision: settings.revision,
                                    },
                                )
                                .await?
                        } else {
                            wire::VoiceCredential::default()
                        };
                        Ok((settings, credential))
                    },
                    Message::Voice,
                )
            }
            _ => Task::none(),
        }
    }
    pub fn subscription(&self, tab: Tab) -> Subscription<Message> {
        if tab == Tab::Connections && self.channels_loaded && !self.channels_busy {
            iced::time::every(std::time::Duration::from_millis(2500)).map(|_| Message::Poll)
        } else {
            Subscription::none()
        }
    }
    pub fn update(&mut self, message: Message, client: &Connection) -> Task<Message> {
        self.error.clear();
        match message {
            Message::Memories(result) => {
                self.memory_busy = false;
                match result {
                    Ok(list) => {
                        self.memories = list
                            .memories
                            .into_iter()
                            .map(|saved| Memory {
                                draft: text_editor::Content::with_text(&saved.content),
                                saved,
                            })
                            .collect();
                        self.memory_loaded = true;
                    }
                    Err(e) => self.error = e,
                }
            }
            Message::NewMemory => self.memories.insert(
                0,
                Memory {
                    saved: Default::default(),
                    draft: text_editor::Content::new(),
                },
            ),
            Message::MemoryEdit(i, action) => self.memories[i].draft.perform(action),
            Message::SaveMemory(i) => {
                let memory = &self.memories[i];
                let client = client.clone();
                let request = wire::MemorySaveRequest {
                    id: memory.saved.id.clone(),
                    content: memory.draft.text(),
                    expected_revision: memory.saved.revision,
                    agent_id: self.agent_id.clone(),
                };
                self.memory_busy = true;
                return Task::perform(
                    async move { client.rpc("workbench/memory-save", request).await },
                    move |r| Message::MemorySaved(i, r),
                );
            }
            Message::MemorySaved(i, result) => {
                self.memory_busy = false;
                match result {
                    Ok(saved) => self.memories[i].saved = saved,
                    Err(e) => self.error = e,
                }
            }
            Message::DeleteMemory(i) => {
                let memory = &self.memories[i];
                if memory.saved.id.is_empty() {
                    self.memories.remove(i);
                } else {
                    let request = wire::MemoryDeleteRequest {
                        id: memory.saved.id.clone(),
                        expected_revision: memory.saved.revision,
                        agent_id: self.agent_id.clone(),
                    };
                    let client = client.clone();
                    self.memory_busy = true;
                    return Task::perform(
                        async move { client.rpc("workbench/memory-delete", request).await },
                        move |r| Message::MemoryDeleted(i, r),
                    );
                }
            }
            Message::MemoryDeleted(i, result) => {
                self.memory_busy = false;
                match result {
                    Ok(_) => {
                        self.memories.remove(i);
                    }
                    Err(e) => self.error = e,
                }
            }
            Message::Poll => {
                self.channels_busy = true;
                let client = client.clone();
                let input = wire::ChannelListRequest {
                    agent_id: self.agent_id.clone(),
                };
                return Task::perform(
                    async move { client.rpc("channels/list", input).await },
                    Message::Channels,
                );
            }
            Message::Channels(result) | Message::ChannelSaved(result) => {
                self.channels_busy = false;
                match result {
                    Ok(list) => {
                        self.channels = list.bindings;
                        self.channels_loaded = true;
                        self.qr = self
                            .channels
                            .iter()
                            .find(|c| c.provider == self.channel)
                            .filter(|c| !c.qr_png.is_empty())
                            .map(|c| image::Handle::from_bytes(c.qr_png.clone()));
                    }
                    Err(e) => self.error = e,
                }
            }
            Message::Channel(provider) => {
                self.channel_drafts.insert(
                    self.channel.clone(),
                    (
                        std::mem::take(&mut self.app_id),
                        std::mem::take(&mut self.secret),
                        std::mem::take(&mut self.proxy),
                    ),
                );
                self.channel = provider;
                let saved = self.channels.iter().find(|c| c.provider == self.channel);
                self.app_id = saved.map(|c| c.app_id.clone()).unwrap_or_default();
                self.proxy = saved.map(|c| c.proxy_url.clone()).unwrap_or_default();
                if let Some((id, key, proxy)) = self.channel_drafts.remove(&self.channel) {
                    self.app_id = id;
                    self.secret = key;
                    self.proxy = proxy;
                }
                self.qr = saved
                    .filter(|c| !c.qr_png.is_empty())
                    .map(|c| image::Handle::from_bytes(c.qr_png.clone()));
            }
            Message::AppId(v) => self.app_id = v,
            Message::Secret(v) => self.secret = v,
            Message::Proxy(v) => self.proxy = v,
            Message::CopyPair(command) => return iced::clipboard::write(command),
            Message::Bind(reset_session) => {
                self.channels_busy = true;
                let client = client.clone();
                let app_id = if self.channel == "telegram" {
                    self.secret.split(':').next().unwrap_or_default().into()
                } else {
                    self.app_id.clone()
                };
                let input = wire::ChannelBindRequest {
                    provider: self.channel.clone(),
                    agent_id: self.agent_id.clone(),
                    app_id,
                    secret: self.secret.clone(),
                    proxy_url: self.proxy.clone(),
                    reset_session,
                };
                return Task::perform(
                    async move { client.rpc("channels/bind", input).await },
                    Message::ChannelSaved,
                );
            }
            Message::Pair => {
                self.channels_busy = true;
                let client = client.clone();
                let input = wire::ChannelPairRequest {
                    provider: self.channel.clone(),
                    agent_id: self.agent_id.clone(),
                };
                return Task::perform(
                    async move { client.rpc("channels/pair", input).await },
                    Message::ChannelSaved,
                );
            }
            Message::Unbind => {
                self.channels_busy = true;
                let client = client.clone();
                let input = wire::ChannelUnbindRequest {
                    provider: self.channel.clone(),
                    agent_id: self.agent_id.clone(),
                };
                return Task::perform(
                    async move { client.rpc("channels/unbind", input).await },
                    Message::ChannelSaved,
                );
            }
            Message::Voice(result) => {
                self.voice_busy = false;
                match result {
                    Ok((v, key)) => {
                        self.voice_revision = v.revision;
                        self.voice = wire::VoiceDraft {
                            base_url: v.base_url,
                            api_key: key.api_key,
                            recognition_model: v.recognition_model,
                            speech_model: v.speech_model,
                            voice: v.voice,
                            proxy_url: v.proxy_url,
                            input_protocol: if v.configured {
                                v.input_protocol
                            } else {
                                wire::VoiceInputProtocol::AudioChat.into()
                            },
                        };
                        self.voice_loaded = true;
                    }
                    Err(e) => self.error = e,
                }
            }
            Message::VoiceField(field, v) => match field {
                VoiceField::Url => self.voice.base_url = v,
                VoiceField::Key => self.voice.api_key = v,
                VoiceField::Recognition => self.voice.recognition_model = v,
                VoiceField::Speech => self.voice.speech_model = v,
                VoiceField::Voice => self.voice.voice = v,
                VoiceField::Proxy => self.voice.proxy_url = v,
            },
            Message::VoiceProtocol(v) => self.voice.input_protocol = v.into(),
            Message::VoiceReveal => self.voice_visible = !self.voice_visible,
            Message::VoiceAdvanced => self.voice_advanced = !self.voice_advanced,
            Message::VoiceSave(clear) => {
                self.voice_busy = true;
                let client = client.clone();
                let input = wire::VoiceSettingsSaveRequest {
                    agent_id: self.agent_id.clone(),
                    expected_revision: self.voice_revision,
                    draft: Some(if clear {
                        wire::VoiceDraft::default()
                    } else {
                        self.voice.clone()
                    }),
                };
                return Task::perform(
                    async move { client.rpc("voice/save", input).await },
                    Message::VoiceSaved,
                );
            }
            Message::VoiceSaved(result) => {
                self.voice_busy = false;
                match result {
                    Ok(v) => {
                        self.voice_revision = v.revision;
                        if !v.configured {
                            self.voice = Default::default();
                        }
                    }
                    Err(e) => self.error = e,
                }
            }
        }
        Task::none()
    }
    pub fn view(&self, tab: Tab) -> Element<'_, Message> {
        let mut body = column![].spacing(14);
        match tab {
            Tab::Memory => {
                body = body.push(row![
                    space::horizontal(),
                    icon_button(
                        Icon::Plus,
                        "添加记忆",
                        (self.memory_loaded && !self.memory_busy).then_some(Message::NewMemory)
                    )
                ]);
                for (i, memory) in self.memories.iter().enumerate() {
                    let content = memory.draft.text();
                    let changed = !content.trim().is_empty() && content != memory.saved.content;
                    body = body.push(
                        column![
                            text_editor(&memory.draft)
                                .on_action(move |a| Message::MemoryEdit(i, a))
                                .key_binding(move |press| {
                                    if press.key
                                        == keyboard::Key::Named(keyboard::key::Named::Enter)
                                        && press.modifiers.command()
                                    {
                                        (changed && !self.memory_busy).then_some(
                                            text_editor::Binding::Custom(Message::SaveMemory(i)),
                                        )
                                    } else {
                                        text_editor::Binding::from_key_press(press)
                                    }
                                })
                                .height(100)
                                .size(14)
                                .padding(10)
                                .style(|theme, status| {
                                    let mut s = text_editor::default(theme, status);
                                    s.background = iced::Color::from_rgba(1., 1., 1., 0.35).into();
                                    s.border.radius = style::CONTROL_RADIUS.into();
                                    s.border.width = style::BORDER_WIDTH;
                                    s
                                }),
                            row![
                                space::horizontal(),
                                icon_button(
                                    Icon::Trash,
                                    "删除",
                                    (!self.memory_busy).then_some(Message::DeleteMemory(i))
                                ),
                                icon_button(
                                    Icon::Check,
                                    "保存",
                                    (!self.memory_busy && changed)
                                        .then_some(Message::SaveMemory(i))
                                )
                            ]
                        ]
                        .spacing(6),
                    );
                }
                if self.memory_loaded && self.memories.is_empty() {
                    body = body.push(text("暂无记忆").size(13).color(style::MUTED));
                }
            }
            Tab::Connections => {
                body = body.push(
                    row([
                        ("wecom", "企业微信"),
                        ("feishu", "飞书"),
                        ("qq", "QQ"),
                        ("telegram", "Telegram"),
                        ("whatsapp", "WhatsApp"),
                    ]
                    .into_iter()
                    .map(|(id, label)| {
                        tab_button(label, self.channel == id, Some(Message::Channel(id.into())))
                    }))
                    .spacing(2),
                );
                if !self.channel.is_empty() {
                    let binding = self.channels.iter().find(|c| c.provider == self.channel);
                    if let Some(binding) = binding {
                        body = body.push(
                            text(match binding.status.as_str() {
                                "connected" => "已连接",
                                "connecting" => "连接中",
                                "waiting_qr" => "等待扫码",
                                "failed" => "连接失败",
                                "unbound" => "未连接",
                                _ => &binding.status,
                            })
                            .size(12)
                            .color(style::MUTED),
                        );
                        if !binding.error.is_empty() {
                            body = body.push(text(&binding.error).size(12).color(style::ERROR));
                        }
                    }
                    if self.channel != "whatsapp" {
                        if self.channel != "telegram" {
                            body = body.push(field(
                                if self.channel == "wecom" {
                                    "Bot ID"
                                } else {
                                    "App ID"
                                },
                                &self.app_id,
                                Message::AppId,
                                self.channels_loaded && !self.channels_busy,
                            ));
                        }
                        body = body.push(field(
                            if self.channel == "telegram" {
                                "Bot Token"
                            } else {
                                "Secret"
                            },
                            &self.secret,
                            Message::Secret,
                            self.channels_loaded && !self.channels_busy,
                        ));
                    }
                    body = body.push(field(
                        "代理地址",
                        &self.proxy,
                        Message::Proxy,
                        self.channels_loaded && !self.channels_busy,
                    ));
                    if let Some(qr) = &self.qr {
                        body = body.push(image(qr.clone()).width(220).height(220));
                    }
                    if let Some(binding) = binding.filter(|b| !b.pairing_command.is_empty()) {
                        body = body.push(text(&binding.pairing_command).size(14));
                    }
                    if let Some(binding) = binding.filter(|b| !b.pairing_command.is_empty()) {
                        body = body.push(
                            button("复制配对指令")
                                .style(style::quiet)
                                .on_press(Message::CopyPair(binding.pairing_command.clone())),
                        );
                    }
                    let enabled = self.channels_loaded && !self.channels_busy;
                    if self.channel == "whatsapp" && binding.is_some_and(|b| b.configured) {
                        body = body.push(
                            button("重新扫码")
                                .style(style::quiet)
                                .on_press_maybe(enabled.then_some(Message::Bind(true))),
                        );
                    }
                    body = body.push(row![
                        action_button("连接", enabled.then_some(Message::Bind(false))).width(80),
                        button("配对")
                            .style(style::quiet)
                            .height(40)
                            .on_press_maybe(
                                (enabled && binding.is_some_and(|b| b.configured))
                                    .then_some(Message::Pair)
                            ),
                        space::horizontal(),
                        icon_button(
                            Icon::Trash,
                            "解除连接",
                            (enabled && binding.is_some_and(|b| b.configured))
                                .then_some(Message::Unbind)
                        )
                    ]);
                }
            }
            Tab::Voice => {
                body = body
                    .push(field(
                        "API 地址",
                        &self.voice.base_url,
                        |v| Message::VoiceField(VoiceField::Url, v),
                        self.voice_loaded && !self.voice_busy,
                    ))
                    .push(
                        column![
                            text("API Key").size(12).color(style::MUTED),
                            row![
                                iced::widget::text_input("", &self.voice.api_key)
                                    .secure(!self.voice_visible)
                                    .size(14)
                                    .line_height(iced::Pixels(20.))
                                    .padding([10, 12])
                                    .style(style::input)
                                    .on_input(|v| Message::VoiceField(VoiceField::Key, v)),
                                icon_button(
                                    if self.voice_visible {
                                        Icon::EyeOff
                                    } else {
                                        Icon::Eye
                                    },
                                    "显示 / 隐藏密钥",
                                    Some(Message::VoiceReveal)
                                )
                            ]
                        ]
                        .spacing(6),
                    );
                let protocols = vec![
                    VoiceProtocol(wire::VoiceInputProtocol::RealtimeTranscription),
                    VoiceProtocol(wire::VoiceInputProtocol::AudioChat),
                    VoiceProtocol(wire::VoiceInputProtocol::Transcription),
                ];
                body = body
                    .push(
                        column![
                            text("输入协议").size(12).color(style::MUTED),
                            pick_list(
                                protocols,
                                Some(VoiceProtocol(self.voice.input_protocol())),
                                |v| Message::VoiceProtocol(v.0)
                            )
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
                        "识别模型",
                        &self.voice.recognition_model,
                        |v| Message::VoiceField(VoiceField::Recognition, v),
                        self.voice_loaded && !self.voice_busy,
                    ))
                    .push(
                        button(text("高级").size(12))
                            .style(style::quiet)
                            .on_press(Message::VoiceAdvanced),
                    );
                if self.voice_advanced {
                    body = body
                        .push(field(
                            "远程合成模型",
                            &self.voice.speech_model,
                            |v| Message::VoiceField(VoiceField::Speech, v),
                            self.voice_loaded && !self.voice_busy,
                        ))
                        .push(field(
                            "远程音色",
                            &self.voice.voice,
                            |v| Message::VoiceField(VoiceField::Voice, v),
                            self.voice_loaded && !self.voice_busy,
                        ))
                        .push(field(
                            "代理地址",
                            &self.voice.proxy_url,
                            |v| Message::VoiceField(VoiceField::Proxy, v),
                            self.voice_loaded && !self.voice_busy,
                        ));
                }
                body = body.push(row![
                    action_button(
                        "保存",
                        (self.voice_loaded && !self.voice_busy)
                            .then_some(Message::VoiceSave(false))
                    )
                    .width(80),
                    space::horizontal(),
                    icon_button(
                        Icon::Trash,
                        "清除语音配置",
                        (self.voice_loaded && !self.voice_busy).then_some(Message::VoiceSave(true))
                    )
                ]);
            }
            Tab::Model => {}
        }
        if self.memory_busy || self.voice_busy || (!self.channels_loaded && self.channels_busy) {
            body = body.push(text("读取中…").size(12).color(style::MUTED));
        }
        if !self.error.is_empty() {
            body = body.push(text(&self.error).size(12).color(style::ERROR));
        }
        body.into()
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
struct VoiceProtocol(wire::VoiceInputProtocol);
impl std::fmt::Display for VoiceProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            wire::VoiceInputProtocol::RealtimeTranscription => "实时转写",
            wire::VoiceInputProtocol::AudioChat => "音频对话",
            _ => "语音转写",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_channel_keeps_each_unsaved_configuration() {
        let client = Connection::for_state_test();
        let mut details = Details::new("agent".into());
        let _ = details.update(Message::Channel("wecom".into()), &client);
        let _ = details.update(Message::AppId("draft-app".into()), &client);
        let _ = details.update(Message::Secret("draft-secret".into()), &client);
        let _ = details.update(Message::Channel("telegram".into()), &client);
        let _ = details.update(Message::Secret("draft-token".into()), &client);
        let _ = details.update(Message::Channel("wecom".into()), &client);
        assert_eq!(details.app_id, "draft-app");
        assert_eq!(details.secret, "draft-secret");
        let _ = details.update(Message::Channel("telegram".into()), &client);
        assert_eq!(details.secret, "draft-token");
    }
}
