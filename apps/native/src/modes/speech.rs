mod microphone;
mod speaker;

use super::{Message, MicrophoneLevels};
use ai_phone_api::wire;
use iced::Task;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Debug)]
pub enum Event {
    Ready,
    Levels(MicrophoneLevels),
    Speech(String),
    Transcript {
        id: String,
        revision: u32,
        text: String,
        final_text: bool,
    },
    Error(String),
}

struct Target {
    id: String,
    device: String,
    conversation: String,
    agent: String,
}

struct Reply {
    text: String,
    offset: usize,
    complete: bool,
    interrupted: bool,
}

#[derive(Default)]
pub struct Copilot {
    session: String,
    input: Option<Arc<AtomicBool>>,
    speaking: Option<(u64, tokio::sync::oneshot::Sender<()>)>,
    speech_index: u64,
    language: String,
    target: Option<Target>,
    pub(super) ready: bool,
    pub(super) levels: MicrophoneLevels,
    baseline: u64,
    replies: BTreeMap<u64, Reply>,
    pub transcript: String,
    pub recovered: String,
    pub request: Option<wire::ChatSendRequest>,
    pub error: String,
}

impl Copilot {
    pub fn active(&self) -> bool {
        self.input.is_some()
    }
    pub fn start(
        &mut self,
        model: PathBuf,
        language: &str,
        chat: &wire::ChatState,
    ) -> Task<Message> {
        self.error.clear();
        self.language = language.into();
        self.baseline = chat.messages.iter().map(|m| m.ordinal).max().unwrap_or(0);
        for message in chat.messages.iter().filter(|message| {
            message.role == "assistant"
                && message.run_id == chat.run_id
                && !reply_complete(message, chat)
                && message.system_notice.is_empty()
        }) {
            let text = speaker::readable(&message.text);
            self.replies.insert(
                message.ordinal,
                Reply {
                    offset: text.len(),
                    text,
                    complete: false,
                    interrupted: false,
                },
            );
        }
        self.session = uuid::Uuid::now_v7().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        self.input = Some(cancel.clone());
        let session = self.session.clone();
        Task::run(
            microphone::events(model, language.to_owned(), cancel),
            move |event| Message::Capture(session.clone(), event),
        )
    }
    pub fn stop(&mut self) {
        if let Some(cancel) = self.input.take() {
            cancel.store(true, Ordering::Release);
        }
        self.ready = false;
        self.levels = MicrophoneLevels::default();
        self.target = None;
        self.speaking = None;
        self.replies.clear();
        self.recovered = std::mem::take(&mut self.transcript);
    }
    pub fn capture(
        &mut self,
        session: String,
        event: Event,
        device: &str,
        chat: &wire::ChatState,
    ) -> Task<Message> {
        if !self.active() || self.session != session {
            return Task::none();
        }
        match event {
            Event::Levels(levels) => self.levels = levels,
            Event::Ready => {
                self.ready = true;
                return self.pump();
            }
            Event::Speech(id) => {
                self.speaking = None;
                for reply in self.replies.values_mut() {
                    reply.interrupted = true;
                    reply.offset = reply.text.len();
                }
                self.target = Some(Target {
                    id,
                    device: device.to_owned(),
                    conversation: chat.conversation_id.clone(),
                    agent: chat.agent_id.clone(),
                });
            }
            Event::Transcript {
                id,
                revision,
                text,
                final_text,
            } => {
                if self.target.as_ref().is_none_or(|slot| slot.id != id) {
                    return Task::none();
                }
                if final_text {
                    let target = self.target.take().expect("current utterance");
                    if text.trim().is_empty() {
                        self.recovered = std::mem::take(&mut self.transcript);
                        return self.pump();
                    }
                    self.transcript.clear();
                    self.request = Some(wire::ChatSendRequest {
                        conversation_id: target.conversation,
                        device_id: target.device,
                        agent_id: target.agent,
                        text: text.clone(),
                        request_id: uuid::Uuid::now_v7().to_string(),
                        voice: Some(wire::ChatVoiceInput {
                            session_id: self.session.clone(),
                            utterance_id: id,
                            revision,
                            literal: text,
                            corrects_request_id: String::new(),
                        }),
                        ..Default::default()
                    });
                    return self.pump();
                }
                if !text.is_empty() {
                    self.transcript = text;
                }
            }
            Event::Error(error) => {
                self.stop();
                self.error = error;
            }
        }
        Task::none()
    }
    pub fn observe_chat(&mut self, chat: &wire::ChatState) -> Task<Message> {
        if !self.active() {
            return Task::none();
        }
        for message in &chat.messages {
            if (message.ordinal <= self.baseline && !self.replies.contains_key(&message.ordinal))
                || message.role != "assistant"
                || !message.system_notice.is_empty()
            {
                continue;
            }
            let text = speaker::readable(&message.text);
            let entry = self
                .replies
                .entry(message.ordinal)
                .or_insert_with(|| Reply {
                    text: String::new(),
                    offset: 0,
                    complete: false,
                    interrupted: false,
                });
            if entry.interrupted || !text.starts_with(&entry.text) {
                entry.offset = text.len();
            }
            entry.text = text;
            entry.complete = reply_complete(message, chat);
        }
        self.pump()
    }
    fn pump(&mut self) -> Task<Message> {
        while let Some((&ordinal, reply)) = self.replies.first_key_value() {
            if !reply.complete || reply.offset < reply.text.len() {
                break;
            }
            self.baseline = self.baseline.max(ordinal);
            self.replies.pop_first();
        }
        if !self.ready || self.target.is_some() || self.speaking.is_some() {
            return Task::none();
        }
        for reply in self.replies.values_mut() {
            let rest = &reply.text[reply.offset..];
            if rest.is_empty() {
                continue;
            }
            let Some(count) = speaker::phrase_len(rest, reply.complete) else {
                continue;
            };
            let phrase = rest[..count].trim();
            reply.offset += count;
            if phrase.is_empty() {
                continue;
            }
            self.speech_index += 1;
            let index = self.speech_index;
            let (stop, cancel) = tokio::sync::oneshot::channel();
            self.speaking = Some((index, stop));
            return Task::perform(
                speaker::speak(phrase.to_owned(), self.language.clone(), cancel),
                move |result| Message::Spoken(index, result),
            );
        }
        Task::none()
    }
    pub fn spoken(&mut self, index: u64, result: Result<(), String>) -> Task<Message> {
        if self
            .speaking
            .as_ref()
            .is_some_and(|(current, _)| *current == index)
        {
            self.speaking = None;
            if let Err(error) = result {
                self.error = error;
            } else {
                return self.pump();
            }
        }
        Task::none()
    }
}

fn reply_complete(message: &wire::ChatMessage, chat: &wire::ChatState) -> bool {
    !message.active
        || (message.run_id == chat.run_id
            && matches!(
                chat.run_status.as_str(),
                "succeeded" | "canceled" | "failed"
            ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_utterance_remains_bound_to_device_and_agent_at_speech_start() {
        let mut copilot = Copilot::default();
        let chat = wire::ChatState {
            conversation_id: "shared".into(),
            agent_id: "original-agent".into(),
            ..Default::default()
        };
        let _ = copilot.start(PathBuf::new(), "en", &chat);
        let session = copilot.session.clone();
        let _ = copilot.capture(
            session.clone(),
            Event::Speech("utterance".into()),
            "phone",
            &chat,
        );
        let _ = copilot.capture(
            session.clone(),
            Event::Transcript {
                id: "utterance".into(),
                revision: 1,
                text: "Open Set".into(),
                final_text: false,
            },
            "phone",
            &chat,
        );
        assert_eq!(copilot.transcript, "Open Set");
        assert!(copilot.request.is_none());
        let switched = wire::ChatState {
            agent_id: "new-agent".into(),
            ..chat
        };
        let _ = copilot.capture(
            session.clone(),
            Event::Transcript {
                id: "utterance".into(),
                revision: 2,
                text: "Open Settings".into(),
                final_text: true,
            },
            "desktop",
            &switched,
        );
        let request = copilot.request.take().unwrap();
        assert_eq!(request.device_id, "phone");
        assert_eq!(request.agent_id, "original-agent");
        assert_eq!(request.text, "Open Settings");
        assert_eq!(request.voice.unwrap().session_id, session);
    }
}
