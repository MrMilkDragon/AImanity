use crate::{
    audio,
    connection::{DeviceInput, Frame},
    scene::AgentTrace,
};
use ai_phone_api::wire::RegisteredDevice;
use std::{collections::VecDeque, sync::Arc, time::Instant};

pub struct Device {
    pub info: RegisteredDevice,
    pub frame: Option<Arc<Frame>>,
    pub trace: Option<AgentTrace>,
    pub offset: iced::Vector,
    pub audio: audio::Playback,
    pub queue: VecDeque<DeviceInput>,
    pub busy: bool,
    pub waiting: bool,
    pub click: Option<(Instant, u32, u32)>,
    pub visible: tokio::sync::watch::Sender<bool>,
    pub stream: Option<iced::task::Handle>,
}
impl Device {
    pub fn new(info: RegisteredDevice) -> Self {
        Self {
            info,
            frame: None,
            trace: None,
            offset: iced::Vector::ZERO,
            audio: audio::Playback::new(),
            queue: VecDeque::new(),
            busy: false,
            waiting: false,
            click: None,
            visible: tokio::sync::watch::channel(true).0,
            stream: None,
        }
    }
    pub fn receive(&mut self, frame: Frame) {
        self.trace = frame
            .cursor
            .as_deref()
            .and_then(|cursor| crate::scene::AgentTrace::from_cursor(cursor, &self.info));
        self.frame = Some(Arc::new(frame));
    }
    pub fn phone(&self) -> bool {
        self.info.kind == "phone"
    }
    pub fn aspect(&self) -> f32 {
        self.frame
            .as_ref()
            .map(|f| f.width as f32 / f.height as f32)
            .unwrap_or(if self.phone() { 0.50 } else { 1.60 })
    }
}
