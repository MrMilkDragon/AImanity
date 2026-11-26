use crate::{
    agents::Tab,
    connection::Connection,
    controls::{Icon, action_button, field, form_input, form_select, icon_button, text_button},
    style,
};
use ai_phone_api::wire;
use iced::{
    Element, Length, Task, keyboard,
    widget::{column, row, space, text, text_editor},
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
    pub fn view(&self, tab: Tab, locale: crate::locale::Locale) -> Element<'_, Message> {
        let mut body = column![].spacing(14);
        match tab {
            Tab::Memory => {
                body = body.push(row![
                    space::horizontal(),
                    icon_button(
                        Icon::Plus,
                        (self.memory_loaded && !self.memory_busy).then_some(Message::NewMemory)
                    )
                ]);
                for (i, memory) in self.memories.iter().enumerate() {
                    let content = memory.draft.text();
                    let changed = !content.trim().is_empty() && content != memory.saved.content;
                    body = body.push(
                        column![
                            crate::touch_editor::editor(
                                text_editor(&memory.draft)
                                    .on_action(move |a| Message::MemoryEdit(i, a))
                                    .key_binding(move |press| {
                                        if press.key
                                            == keyboard::Key::Named(keyboard::key::Named::Enter)
                                            && press.modifiers.command()
                                        {
                                            (changed && !self.memory_busy).then_some(
                                                text_editor::Binding::Custom(Message::SaveMemory(
                                                    i,
                                                )),
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
                                        s.background =
                                            iced::Color::from_rgba(1., 1., 1., 0.35).into();
                                        s.border.radius = style::CONTROL_RADIUS.into();
                                        s.border.width = style::BORDER_WIDTH;
                                        s
                                    })
                            ),
                            row![
                                space::horizontal(),
                                icon_button(
                                    Icon::Trash,
                                    (!self.memory_busy).then_some(Message::DeleteMemory(i))
                                ),
                                icon_button(
                                    Icon::Check,
                                    (!self.memory_busy && changed)
                                        .then_some(Message::SaveMemory(i))
                                )
                            ]
                        ]
                        .spacing(6),
                    );
                }
                if self.memory_loaded && self.memories.is_empty() {
                    body = body.push(text(locale.tr("暂无记忆")).size(13).color(style::MUTED));
                }
            }
            Tab::Voice => {
                body = body
                    .push(field(
                        locale.tr("API 地址"),
                        &self.voice.base_url,
                        |v| Message::VoiceField(VoiceField::Url, v),
                        self.voice_loaded && !self.voice_busy,
                    ))
                    .push(
                        column![
                            text("API Key").size(12).color(style::MUTED),
                            row![
                                form_input("", &self.voice.api_key)
                                    .secure(!self.voice_visible)
                                    .on_input(|v| Message::VoiceField(VoiceField::Key, v)),
                                icon_button(
                                    if self.voice_visible {
                                        Icon::EyeOff
                                    } else {
                                        Icon::Eye
                                    },
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
                            text(locale.tr("输入协议")).size(12).color(style::MUTED),
                            form_select(
                                protocols,
                                Some(VoiceProtocol(self.voice.input_protocol())),
                                |v| Message::VoiceProtocol(v.0)
                            )
                            .width(Length::Fill)
                        ]
                        .spacing(6),
                    )
                    .push(field(
                        locale.tr("识别模型"),
                        &self.voice.recognition_model,
                        |v| Message::VoiceField(VoiceField::Recognition, v),
                        self.voice_loaded && !self.voice_busy,
                    ))
                    .push(text_button(locale.tr("高级")).on_press(Message::VoiceAdvanced));
                if self.voice_advanced {
                    body = body
                        .push(field(
                            locale.tr("远程合成模型"),
                            &self.voice.speech_model,
                            |v| Message::VoiceField(VoiceField::Speech, v),
                            self.voice_loaded && !self.voice_busy,
                        ))
                        .push(field(
                            locale.tr("远程音色"),
                            &self.voice.voice,
                            |v| Message::VoiceField(VoiceField::Voice, v),
                            self.voice_loaded && !self.voice_busy,
                        ))
                        .push(field(
                            locale.tr("代理地址"),
                            &self.voice.proxy_url,
                            |v| Message::VoiceField(VoiceField::Proxy, v),
                            self.voice_loaded && !self.voice_busy,
                        ));
                }
                body = body.push(row![
                    action_button(
                        locale.tr("保存"),
                        (self.voice_loaded && !self.voice_busy)
                            .then_some(Message::VoiceSave(false))
                    ),
                    space::horizontal(),
                    icon_button(
                        Icon::Trash,
                        (self.voice_loaded && !self.voice_busy).then_some(Message::VoiceSave(true))
                    )
                ]);
            }
            Tab::Model => {}
        }
        if self.memory_busy || self.voice_busy {
            body = body.push(text(locale.tr("读取中…")).size(12).color(style::MUTED));
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
