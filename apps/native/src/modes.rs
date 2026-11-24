mod speech;
mod voice_particles;

pub type MicrophoneLevels = [u8; 32];

use crate::{
    connection::Connection,
    controls::{Icon, action_button},
    locale::Locale,
    style,
};
use ai_phone_api::wire;
use iced::{
    Color, Element, Length, Subscription, Task, keyboard,
    widget::{button, column, container, row, space, text, text_editor},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum Message {
    Refresh,
    Refreshed(Result<wire::ProactiveState, String>),
    ToggleLiberty,
    GoalEdit(text_editor::Action),
    SetGoal(bool),
    Saved(Option<u64>, Result<wire::ProactiveState, String>),
    ToggleCopilot,
    Capture(String, speech::Event),
    Spoken(u64, Result<(), String>),
}

#[derive(Default)]
pub struct Modes {
    state: Option<wire::ProactiveState>,
    reading: bool,
    saving: bool,
    error: String,
    connection_error: String,
    goal_error: String,
    goal: text_editor::Content,
    goal_edited: bool,
    goal_revision: u64,
    speech: speech::Copilot,
    model: PathBuf,
    locale: Locale,
    goal_time: f32,
    last_tick: Option<Instant>,
    edge: f32,
    edge_changed: Option<(Instant, f32)>,
    edge_started: Option<Instant>,
    edge_time: f32,
    last_edge: bool,
}

impl Modes {
    pub fn set_speech_model(&mut self, path: PathBuf) {
        self.model = path;
    }
    pub fn set_language(&mut self, language: &str) {
        self.locale = Locale::parse(language);
    }
    pub fn update(
        &mut self,
        message: Message,
        client: &Connection,
        device_id: &str,
        chat: &wire::ChatState,
    ) -> Task<Message> {
        match message {
            Message::Refresh if !self.saving && !self.reading => {
                self.reading = true;
                let client = client.clone();
                return Task::perform(
                    async move {
                        client
                            .rpc("proactive/state", wire::ProactiveRequest {})
                            .await
                    },
                    Message::Refreshed,
                );
            }
            Message::Refreshed(result) => {
                self.reading = false;
                if !self.saving {
                    match result {
                        Ok(next) => {
                            self.connection_error.clear();
                            self.apply(next);
                        }
                        Err(error) => self.connection_error = error,
                    }
                }
            }
            Message::ToggleLiberty if !self.saving => {
                let Some(state) = &self.state else {
                    return Task::none();
                };
                if state.goal_active && !state.exploring {
                    return Task::none();
                }
                self.saving = true;
                self.error.clear();
                let request = wire::ProactiveUpdateRequest {
                    expected_revision: state.revision,
                    exploring: !state.exploring,
                    notifications_enabled: state.notifications_enabled,
                    forward_notifications: state.forward_notifications,
                    autonomous_reply: state.autonomous_reply,
                    autonomous_install: state.autonomous_install,
                    interval_seconds: state.interval_seconds,
                    wechat_watch: Some(state.wechat_watch),
                };
                let client = client.clone();
                return Task::perform(
                    async move { client.rpc("proactive/update", request).await },
                    |result| Message::Saved(None, result),
                );
            }
            Message::GoalEdit(action) => {
                if action.is_edit() {
                    self.goal_edited = true;
                    self.goal_revision += 1;
                    self.goal_error.clear();
                }
                self.goal.perform(action);
            }
            Message::SetGoal(clear) if !self.saving => {
                let Some(state) = &self.state else {
                    return Task::none();
                };
                let value = self.goal.text();
                if !clear && !self.can_save_goal(value.trim()) {
                    return Task::none();
                }
                let request = wire::GoalUpdateRequest {
                    expected_revision: state.revision,
                    goal: if clear {
                        String::new()
                    } else {
                        value.trim().to_owned()
                    },
                    device_id: String::new(),
                };
                self.saving = true;
                self.goal_error.clear();
                let revision = self.goal_revision;
                let client = client.clone();
                return Task::perform(
                    async move { client.rpc("proactive/goal", request).await },
                    move |result| Message::Saved(Some(revision), result),
                );
            }
            Message::Saved(revision, result) => {
                self.saving = false;
                match result {
                    Ok(next) => {
                        if revision == Some(self.goal_revision) {
                            self.goal_edited = false;
                        }
                        self.apply(next);
                    }
                    Err(error) => {
                        if revision.is_some() {
                            self.goal_error = error;
                        } else {
                            self.error = error;
                        }
                        return self.update(Message::Refresh, client, device_id, chat);
                    }
                }
            }
            Message::ToggleCopilot => {
                if self.speech.active() {
                    self.speech.stop();
                } else if !chat.conversation_id.is_empty() {
                    return self
                        .speech
                        .start(self.model.clone(), self.locale.code(), chat);
                }
            }
            Message::Capture(session, event) => {
                return self.speech.capture(session, event, device_id, chat);
            }
            Message::Spoken(index, result) => return self.speech.spoken(index, result),
            _ => {}
        }
        Task::none()
    }
    fn apply(&mut self, next: wire::ProactiveState) {
        if self
            .state
            .as_ref()
            .is_some_and(|current| current.revision > next.revision)
        {
            return;
        }
        if !self.goal_edited
            && self
                .state
                .as_ref()
                .is_none_or(|current| current.goal != next.goal)
        {
            self.goal = text_editor::Content::with_text(&next.goal);
        }
        let edge_enabled = next.exploring && !next.goal_active;
        if self.last_edge != edge_enabled {
            let now = Instant::now();
            if edge_enabled && self.edge == 0. {
                self.edge_started = Some(now);
                self.edge_time = 0.;
            }
            self.edge_changed = Some((now, self.edge));
            self.last_edge = edge_enabled;
        }
        if !next.goal_active {
            self.goal_time = 0.;
        }
        self.state = Some(next);
    }
    pub fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_millis(2500)).map(|_| Message::Refresh)
    }
    pub fn stop_capture(&mut self) {
        self.speech.stop();
    }
    pub fn observe_chat(&mut self, chat: &wire::ChatState) -> Task<Message> {
        self.speech.observe_chat(chat)
    }
    pub fn take_voice(&mut self) -> Option<wire::ChatSendRequest> {
        self.speech.request.take()
    }
    pub fn take_recovered(&mut self) -> String {
        std::mem::take(&mut self.speech.recovered)
    }
    pub fn transcript(&self) -> &str {
        &self.speech.transcript
    }
    pub fn voice_error(&self) -> &str {
        self.locale.tr(&self.speech.error)
    }
    pub fn proactive_error(&self) -> &str {
        [&self.error, &self.connection_error]
            .into_iter()
            .map(String::as_str)
            .find(|error| !error.is_empty())
            .or_else(|| {
                self.state
                    .as_ref()
                    .filter(|state| state.exploring)
                    .map(|state| state.last_error.as_str())
            })
            .unwrap_or("")
    }
    pub fn goal_active(&self) -> bool {
        self.state.as_ref().is_some_and(|state| state.goal_active)
    }
    fn goal_moving(&self) -> bool {
        self.state.as_ref().is_some_and(|state| {
            state.goal_active && !matches!(state.goal_status.as_str(), "paused" | "failed")
        })
    }
    pub fn animating(&self) -> bool {
        self.last_edge || self.edge_changed.is_some() || self.goal_moving()
    }
    pub fn animate(&mut self, now: Instant) {
        if let Some(previous) = self.last_tick.replace(now) {
            let delta = now
                .saturating_duration_since(previous)
                .as_secs_f32()
                .min(0.09);
            if self.goal_moving() {
                self.goal_time += delta;
            }
        }
        if let Some(started) = self.edge_started {
            self.edge_time = now.saturating_duration_since(started).as_secs_f32();
        }
        if let Some((since, from)) = self.edge_changed {
            let duration = if self.last_edge { 1.4 } else { 0.7 };
            let fraction = (now.saturating_duration_since(since).as_secs_f32() / duration).min(1.);
            let target = if self.last_edge { 1. } else { 0. };
            self.edge = from + (target - from) * fraction * fraction * (3. - 2. * fraction);
            if fraction == 1. {
                self.edge_changed = None;
                if !self.last_edge {
                    self.edge_started = None;
                }
            }
        }
    }
    pub fn voice_strip<M: 'static>(&self, running: bool, reduced_motion: bool) -> Element<'_, M> {
        iced::widget::canvas(voice_particles::Particles {
            levels: self.speech.ready.then_some(&self.speech.levels),
            thinking: self.speech.ready && running,
            reduced_motion,
        })
        .width(Length::Fill)
        .height(42.)
        .into()
    }
    pub fn effects(&self, reduced_motion: bool) -> ([f32; 4], [f32; 4]) {
        let goal = if self.goal_active() {
            if reduced_motion {
                1.
            } else {
                self.goal_time + 1.
            }
        } else {
            0.
        };
        let edge = if reduced_motion {
            f32::from(self.last_edge)
        } else {
            self.edge
        };
        let reveal = if reduced_motion {
            1.1
        } else {
            (1. - (1. - (self.edge_time / 1.8).min(1.)).powi(3)) * 1.1
        };
        let orbit = if reduced_motion { 0. } else { self.edge_time };
        ([0., edge, goal, 0.], [orbit, reveal, 0., 0.])
    }
    pub fn liberty_button(&self) -> Element<'_, Message> {
        let liberty = self.state.as_ref().is_some_and(|state| state.exploring);
        let ready = self.state.is_some() && !self.saving && (!self.goal_active() || liberty);
        let ink = if liberty {
            Color::from_rgb8(0x51, 0x4b, 0x8a)
        } else {
            Color::from_rgb8(0x65, 0x6a, 0x78)
        };
        mode_button(
            text("Liberty")
                .size(12)
                .wrapping(text::Wrapping::None)
                .into(),
            if liberty { Icon::Pause } else { Icon::Compass },
            ink,
            ready.then_some(Message::ToggleLiberty),
        )
        .style(move |theme, status| {
            let mut s = style::quiet(theme, status);
            s.text_color = if ready { ink } else { ink.scale_alpha(0.5) };
            if liberty {
                s.background = Some(
                    iced::gradient::Linear::new(iced::Degrees(115.))
                        .add_stop(0., Color::from_rgba8(0xb0, 0xdf, 0xfb, 0.19))
                        .add_stop(0.5, Color::from_rgba8(0xe2, 0xbf, 0xfb, 0.19))
                        .add_stop(1., Color::from_rgba8(0xff, 0xd4, 0xbb, 0.19))
                        .into(),
                );
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                s.background = Some(Color::from_rgba8(0x77, 0x71, 0x8a, 11. / 255.).into());
                s.text_color = Color::from_rgb8(0x26, 0x23, 0x37);
            }
            s
        })
        .into()
    }
    pub fn copilot_button(&self, conversation_ready: bool) -> Element<'_, Message> {
        use iced::widget::{rich_text, span};
        let active = self.speech.active();
        let blue = Color::from_rgb8(0x19, 0x68, 0x94);
        let label = if active {
            rich_text::<(), _, _, _>([
                span("Co").color(blue),
                span("pi").color(Color::from_rgb8(0x63, 0x4d, 0xa3)),
                span("lot").color(Color::from_rgb8(0xa4, 0x3c, 0x68)),
            ])
            .size(12)
            .wrapping(text::Wrapping::None)
            .into()
        } else {
            text("Copilot")
                .size(12)
                .wrapping(text::Wrapping::None)
                .into()
        };
        mode_button(
            label,
            if active { Icon::Mic } else { Icon::MicOff },
            if active { blue } else { style::MUTED },
            (active || conversation_ready).then_some(Message::ToggleCopilot),
        )
        .style(|_, status| button::Style {
            text_color: match status {
                button::Status::Disabled => style::MUTED.scale_alpha(0.4),
                button::Status::Hovered | button::Status::Pressed => style::FOREGROUND,
                _ => style::MUTED,
            },
            ..Default::default()
        })
        .into()
    }
    fn can_save_goal(&self, value: &str) -> bool {
        self.state.as_ref().is_some_and(|state| {
            !self.saving
                && !value.is_empty()
                && value.len() <= 12000
                && (!state.goal_active || value != state.goal)
        })
    }
    pub fn goal_view(&self) -> Element<'_, Message> {
        let draft = self.goal.text();
        let value = draft.trim();
        let can_save = self.can_save_goal(value);
        let active = self.goal_active();
        let goal_status =
            self.state
                .as_ref()
                .filter(|s| !s.goal.is_empty())
                .map_or("", |s| match s.goal_status.as_str() {
                    "queued" => "待继续",
                    "running" => "正在处理",
                    "paused" => "任务已暂停",
                    "failed" => "执行失败",
                    "succeeded" => "已完成",
                    "canceled" => "已停止",
                    _ => "",
                });
        let editor = text_editor(&self.goal)
            .placeholder(self.locale.tr("填入你的目标与要求"))
            .height(222)
            .padding([14, 16])
            .size(15)
            .line_height(iced::Pixels(24.))
            .on_action(Message::GoalEdit)
            .style(|theme, status| {
                let mut s = text_editor::default(theme, status);
                s.background = style::CONTROL_FILL.into();
                s.border.width = style::BORDER_WIDTH;
                s.border.radius = style::CONTROL_RADIUS.into();
                s
            })
            .key_binding(move |press| {
                if press.key == keyboard::Key::Named(keyboard::key::Named::Enter)
                    && press.modifiers.command()
                {
                    can_save.then_some(text_editor::Binding::Custom(Message::SetGoal(false)))
                } else {
                    text_editor::Binding::from_key_press(press)
                }
            });
        let mut actions = row![
            text(self.locale.tr(goal_status))
                .size(13)
                .style(style::muted_text),
            space::horizontal()
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        if active {
            actions = actions.push(
                action_button(
                    self.locale.tr("关闭目标"),
                    (!self.saving).then_some(Message::SetGoal(true)),
                )
                .style(style::quiet),
            );
        }
        if !active || self.state.as_ref().is_some_and(|s| value != s.goal) {
            actions = actions.push(action_button(
                self.locale.tr(if active { "更新目标" } else { "激活" }),
                can_save.then_some(Message::SetGoal(false)),
            ));
        }
        let error = if value.len() > 12000 {
            self.locale.tr("目标内容不能超过 12 KB")
        } else {
            &self.goal_error
        };
        let mut body = column![editor].spacing(14);
        if !error.is_empty() {
            body = body.push(text(error).size(12).style(style::error_text));
        }
        body.push(actions).width(Length::Fill).into()
    }
}

fn mode_button(
    label: Element<'static, Message>,
    icon: Icon,
    ink: Color,
    action: Option<Message>,
) -> button::Button<'static, Message> {
    button(
        container(
            row![
                icon.view()
                    .width(15)
                    .height(15)
                    .opacity(if action.is_some() { 1. } else { 0.5 })
                    .style(move |_, _| iced::widget::svg::Style { color: Some(ink) }),
                label,
            ]
            .align_y(iced::Alignment::Center)
            .spacing(7),
        )
        .center_y(Length::Fill),
    )
    .height(style::CONTROL_HEIGHT)
    .padding([0, 10])
    .on_press_maybe(action)
}
