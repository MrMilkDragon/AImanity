mod connection;
mod host;
mod layout;
mod scene;
mod style;
mod surface;
use ai_phone_api::wire;
use connection::{Connection, DeviceInput, Frame};
use iced::{
    Color, Element, Event, Length, Rectangle, Size, Subscription, Task, keyboard,
    widget::{
        button, column, container, mouse_area, pin, row, scrollable, space, stack, text,
        text_editor,
    },
    window,
};
use layout::Layout;
use std::{sync::Arc, time::Instant};

#[derive(Clone, Debug)]
enum Message {
    Host(host::Event),
    Connected(Result<Box<connection::Session>, String>),
    Frame(usize, Result<Frame, String>),
    Chat(Result<wire::ChatState, String>),
    Sent(Result<wire::ChatState, String>, String),
    Focus(usize),
    Input(usize, DeviceInput),
    InputDone(usize, Result<(), String>, Instant),
    Edit(text_editor::Action),
    Send,
    Retry,
    Tick(Instant),
    Event(Event),
    Hover(bool),
    Pin,
    Link,
    Linked(Result<wire::DeviceLinkState, String>),
    SelectAgent(wire::AgentProfileRequest),
    SelectedAgent(Result<wire::RegistryState, String>),
    Rotate,
}
struct App {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    host_home: std::path::PathBuf,
    phase: String,
    closing: bool,
    connection: Option<Connection>,
    devices: [Option<wire::RegisteredDevice>; 2],
    frames: [Option<Arc<Frame>>; 2],
    chat: wire::ChatState,
    agents: Vec<wire::RegisteredAgent>,
    active_agent: String,
    draft: text_editor::Content,
    error: String,
    root: String,
    focus: usize,
    linked: bool,
    keyboard_target: Option<usize>,
    busy: [bool; 2],
    input_queue: [std::collections::VecDeque<DeviceInput>; 2],
    streams: Option<iced::task::Handle>,
    size: Size,
    layout: Layout,
    from: Layout,
    target: Layout,
    transition: Option<Instant>,
    popup: bool,
    pinned: bool,
    popup_amount: f32,
    from_popup: f32,
    popup_transition: Option<Instant>,
    started: Instant,
    input_started: Option<Instant>,
}
impl App {
    fn boot() -> (Self, Task<Message>) {
        let configuration = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "native-launch.json".into());
        let host = host::Host::load(&configuration);
        let (stop, receiver) = tokio::sync::oneshot::channel();
        let size = Size::new(1200., 800.);
        let layout = Layout::target(size, 0, false, [0.5, 1.6]);
        let mut app = Self {
            stop: Some(stop),
            host_home: std::path::PathBuf::new(),
            phase: "正在启动设备".into(),
            closing: false,
            connection: None,
            devices: [None, None],
            frames: [None, None],
            chat: Default::default(),
            agents: Vec::new(),
            active_agent: String::new(),
            draft: text_editor::Content::new(),
            error: String::new(),
            root: String::new(),
            focus: 0,
            linked: false,
            keyboard_target: None,
            busy: [false, false],
            input_queue: Default::default(),
            streams: None,
            size,
            layout,
            from: layout,
            target: layout,
            transition: None,
            popup: false,
            pinned: false,
            popup_amount: 0.,
            from_popup: 0.,
            popup_transition: None,
            started: Instant::now(),
            input_started: None,
        };
        let task = match host {
            Ok(host) => {
                app.host_home = host.home.clone();
                Task::run(host.events(receiver), Message::Host)
            }
            Err(error) => {
                app.error = error;
                app.phase.clear();
                app.stop = None;
                Task::none()
            }
        };
        (app, task)
    }
    fn aspects(&self) -> [f32; 2] {
        std::array::from_fn(|i| {
            self.frames[i]
                .as_ref()
                .map(|f| f.width as f32 / f.height as f32)
                .unwrap_or(if i == 0 { 0.5 } else { 1.6 })
        })
    }
    fn rearrange(&mut self) {
        self.from = self.layout;
        self.target = Layout::target(self.size, self.focus, self.linked, self.aspects());
        self.transition = Some(Instant::now());
    }
    fn set_popup(&mut self, open: bool) {
        if self.popup != open {
            self.popup = open;
            self.from_popup = self.popup_amount;
            self.popup_transition = Some(Instant::now());
        }
    }
    fn input(&mut self, index: usize, mut input: DeviceInput) -> Task<Message> {
        if self.busy[index] {
            match (self.input_queue[index].back_mut(), &input) {
                (Some(DeviceInput::Text(previous)), DeviceInput::Text(text)) => {
                    previous.push_str(text);
                    return Task::none();
                }
                (
                    Some(DeviceInput::Scroll {
                        amount: previous, ..
                    }),
                    DeviceInput::Scroll { amount, .. },
                ) => {
                    *previous += amount;
                    return Task::none();
                }
                _ => {}
            }
            self.input_queue[index].push_back(input);
            return Task::none();
        }
        let Some(client) = self.connection.clone() else {
            return Task::none();
        };
        let Some(device) = self.devices[index].clone() else {
            return Task::none();
        };
        if index == 0
            && let DeviceInput::Scroll { y, amount, .. } = &mut input
            && let Some(frame) = &self.frames[0]
        {
            *amount = (*amount).clamp(-((frame.height - 1 - *y) as i32), *y as i32);
        }
        self.busy[index] = true;
        let started = Instant::now();
        Task::perform(
            async move { client.input(&device, input).await },
            move |r| Message::InputDone(index, r, started),
        )
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Host(host::Event::Phase(phase)) => {
                self.phase = match phase.as_str() {
                    "source" => "正在加载组件",
                    "android" => "正在启动手机",
                    "engine" => "正在启动电脑",
                    "services" => "正在启动服务",
                    "input" => "正在连接输入",
                    "devices" => "正在连接设备",
                    _ => "正在启动",
                }
                .into();
            }
            Message::Host(host::Event::Ready(root)) => {
                self.root = root.clone();
                self.phase.clear();
                return Task::perform(Connection::connect(root), |r| {
                    Message::Connected(r.map(Box::new))
                });
            }
            Message::Host(host::Event::Failed(error)) => {
                self.error = error;
                self.phase.clear();
            }
            Message::Host(host::Event::Stopped(result)) => match result {
                Ok(()) => return iced::exit(),
                Err(error) => {
                    self.error = error;
                    self.closing = false;
                }
            },
            Message::Connected(result) => match result {
                Ok(session) => {
                    let connection::Session {
                        client,
                        registry,
                        chat,
                        links,
                    } = *session;
                    self.devices = std::array::from_fn(|i| {
                        registry
                            .devices
                            .iter()
                            .find(|d| {
                                d.kind == if i == 0 { "phone" } else { "desktop" }
                                    && d.runtime_state == "managed"
                            })
                            .cloned()
                    });
                    self.linked = links.links.iter().any(|l| l.connected);
                    self.agents = registry.agents;
                    self.active_agent = registry.active_agent_id;
                    self.chat = chat;
                    self.error.clear();
                    self.connection = Some(client.clone());
                    self.rearrange();
                    eprintln!(
                        "native_metric connected_ms={:0.3}",
                        self.started.elapsed().as_secs_f64() * 1000.
                    );
                    let mut tasks = vec![Task::run(client.clone().chat_events(), Message::Chat)];
                    for (index, device) in self.devices.iter().enumerate() {
                        if let Some(device) = device {
                            tasks
                                .push(Task::run(client.clone().frames(device.clone()), move |r| {
                                    Message::Frame(index, r)
                                }));
                        }
                    }
                    let (task, handle) = Task::batch(tasks).abortable();
                    self.streams = Some(handle.abort_on_drop());
                    return task;
                }
                Err(error) => self.error = error,
            },
            Message::Frame(index, result) => match result {
                Ok(frame) => {
                    let resized = self.frames[index]
                        .as_ref()
                        .is_none_or(|old| old.width != frame.width || old.height != frame.height);
                    if self.frames[index].is_none() {
                        eprintln!(
                            "native_metric first_frame device={index} ms={:0.3}",
                            self.started.elapsed().as_secs_f64() * 1000.
                        );
                    }
                    self.frames[index] = Some(Arc::new(frame));
                    if resized {
                        self.rearrange();
                    }
                }
                Err(error) => self.error = error,
            },
            Message::Chat(result) => match result {
                Ok(chat) => {
                    self.chat = chat;
                }
                Err(error) => self.error = error,
            },
            Message::Sent(result, sent) => match result {
                Ok(chat) => {
                    self.chat = chat;
                    if self.draft.text() == sent {
                        self.draft = text_editor::Content::new();
                    }
                }
                Err(error) => self.error = error,
            },
            Message::Focus(index) => {
                self.keyboard_target = Some(index);
                if self.focus != index {
                    self.focus = index;
                    self.rearrange();
                }
            }
            Message::Input(index, input) => return self.input(index, input),
            Message::InputDone(index, result, started) => {
                self.busy[index] = false;
                eprintln!(
                    "native_metric input_receipt device={index} ms={:0.3} success={}",
                    started.elapsed().as_secs_f64() * 1000.,
                    result.is_ok()
                );
                if let Err(error) = result {
                    self.error = error;
                    self.input_queue[index].clear();
                } else if let Some(next) = self.input_queue[index].pop_front() {
                    return self.input(index, next);
                } else {
                    self.error.clear();
                }
            }
            Message::Edit(action) => {
                self.keyboard_target = None;
                self.input_started = Some(Instant::now());
                self.draft.perform(action);
            }
            Message::Send => {
                let text = self.draft.text();
                if text.trim().is_empty() {
                    return Task::none();
                }
                if let (Some(client), Some(device)) =
                    (self.connection.clone(), &self.devices[self.focus])
                {
                    let input = wire::ChatSendRequest {
                        conversation_id: self.chat.conversation_id.clone(),
                        device_id: device.id.clone(),
                        text: text.clone(),
                        request_id: uuid::Uuid::now_v7().to_string(),
                        ..Default::default()
                    };
                    return Task::perform(
                        async move { client.rpc("assistant/send", input).await },
                        move |result| Message::Sent(result, text),
                    );
                }
            }
            Message::Retry => {
                self.streams = None;
                self.error.clear();
                return Task::perform(Connection::connect(self.root.clone()), |r| {
                    Message::Connected(r.map(Box::new))
                });
            }
            Message::Tick(now) => {
                if let Some(start) = self.input_started.take() {
                    eprintln!(
                        "native_metric edit_to_frame_event_ms={:.3}",
                        start.elapsed().as_secs_f64() * 1000.
                    );
                }
                if let Some(start) = self.transition {
                    let t = (now.saturating_duration_since(start).as_secs_f32()
                        / style::MOTION_DEVICE)
                        .min(1.);
                    let smooth = 1. - (1. - t).powi(3);
                    self.layout = self.from.blend(self.target, smooth);
                    if t >= 1. {
                        self.layout = self.target;
                        self.transition = None;
                    }
                }
                if let Some(start) = self.popup_transition {
                    let (target, duration) = if self.popup {
                        (1., style::MOTION_ENTER)
                    } else {
                        (0., style::MOTION_EXIT)
                    };
                    let t = (now.saturating_duration_since(start).as_secs_f32() / duration).min(1.);
                    self.popup_amount =
                        self.from_popup + (target - self.from_popup) * (1. - (1. - t).powi(3));
                    if t >= 1. {
                        self.popup_transition = None;
                    }
                }
            }
            Message::Event(Event::Window(window::Event::Resized(size))) => {
                self.size = size;
                self.rearrange();
            }
            Message::Event(Event::Window(window::Event::CloseRequested)) => {
                if self.closing {
                    return Task::none();
                }
                self.closing = true;
                self.streams = None;
                self.phase = "正在关闭设备".into();
                if let Some(stop) = self.stop.take() {
                    let _ = stop.send(());
                } else if self.host_home.as_os_str().is_empty() {
                    return iced::exit();
                } else {
                    return Task::perform(host::Host::stop(self.host_home.clone()), |r| {
                        Message::Host(host::Event::Stopped(r))
                    });
                }
            }
            Message::Event(Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            })) => {
                self.set_popup(false);
                self.pinned = false;
                self.keyboard_target = None;
            }
            Message::Event(Event::Mouse(iced::mouse::Event::CursorMoved { position }))
                if self.popup && !self.pinned =>
            {
                let area = Rectangle {
                    x: self.size.width - 340.,
                    y: 16.,
                    width: 324.,
                    height: 250.,
                };
                if !area.contains(position) {
                    self.set_popup(false);
                }
            }
            Message::Hover(inside) => {
                if !self.pinned {
                    self.set_popup(inside);
                }
            }
            Message::Pin => {
                self.pinned = !self.pinned;
                self.set_popup(self.pinned);
                self.keyboard_target = None;
            }
            Message::Link => {
                if let (Some(client), Some(phone), Some(pc)) =
                    (self.connection.clone(), &self.devices[0], &self.devices[1])
                {
                    let request = wire::DeviceLinkRequest {
                        source_device_id: phone.id.clone(),
                        target_device_id: pc.id.clone(),
                        connected: !self.linked,
                    };
                    return Task::perform(
                        async move { client.rpc("desktop/link", request).await },
                        Message::Linked,
                    );
                }
            }
            Message::Linked(result) => match result {
                Ok(state) => {
                    self.linked = state.links.iter().any(|l| l.connected);
                    self.rearrange();
                }
                Err(error) => self.error = error,
            },
            Message::Rotate => {
                if let Some(frame) = &self.frames[0] {
                    let rotation = u32::from(frame.width < frame.height);
                    return self.input(0, DeviceInput::Rotate(rotation));
                }
            }
            Message::SelectAgent(request) => {
                if let Some(client) = self.connection.clone() {
                    return Task::perform(
                        async move { client.rpc("registry/agent-select", request).await },
                        Message::SelectedAgent,
                    );
                }
            }
            Message::SelectedAgent(result) => match result {
                Ok(registry) => {
                    self.agents = registry.agents;
                    self.active_agent = registry.active_agent_id;
                    self.error.clear();
                    if let Some(client) = self.connection.clone() {
                        return Task::perform(async move { client.history().await }, Message::Chat);
                    }
                }
                Err(error) => self.error = error,
            },
            _ => {}
        }
        Task::none()
    }
    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![iced::event::listen_with(|event, _, _| match &event {
            Event::Window(window::Event::Resized(_) | window::Event::CloseRequested)
            | Event::Mouse(iced::mouse::Event::CursorMoved { .. })
            | Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) => Some(Message::Event(event)),
            _ => None,
        })];
        if self.transition.is_some()
            || self.popup_transition.is_some()
            || self.input_started.is_some()
        {
            subscriptions.push(window::frames().map(Message::Tick));
        }
        Subscription::batch(subscriptions)
    }
    fn popup_rect(&self) -> Rectangle {
        if self.popup_amount > 0. {
            Rectangle {
                x: self.size.width - 332.,
                y: 64.,
                width: 300.,
                height: 190. * self.popup_amount,
            }
        } else {
            Rectangle::default()
        }
    }
    fn view(&self) -> Element<'_, Message> {
        let scene = scene::Scene {
            layout: self.layout,
            size: self.size,
            frames: self.frames.clone(),
            focus: self.focus,
            popup: self.popup_rect(),
            keyboard_target: self.keyboard_target,
        };
        let mut layers = vec![surface::Surface::new(scene).into()];
        let error = if self.error.is_empty() {
            text(&self.phase).size(12)
        } else {
            text(&self.error).size(12).color(style::ERROR)
        };
        let nav = row![
            container(error).width(Length::Fill).clip(true),
            button(text(if self.linked { "断开" } else { "连接" }).size(13))
                .on_press_maybe(self.connection.as_ref().map(|_| Message::Link))
                .height(style::CONTROL_HEIGHT)
                .style(style::quiet),
            mouse_area(
                button(text("Agent").size(13))
                    .on_press(Message::Pin)
                    .height(style::CONTROL_HEIGHT)
                    .style(style::quiet)
            )
            .on_enter(Message::Hover(true))
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        layers.push(
            pin(container(nav).width(self.size.width - 64.).height(40.))
                .x(32)
                .y(20)
                .into(),
        );
        for index in 0..2 {
            let rect = self.layout.devices[index];
            let name = self.devices[index]
                .as_ref()
                .map(|d| d.name.as_str())
                .unwrap_or(if index == 0 { "手机" } else { "电脑" });
            let mut tools = row![text(name).size(13)]
                .spacing(8)
                .align_y(iced::Alignment::Center);
            if index == 0 {
                tools = tools.push(
                    button(text("旋转").size(12))
                        .on_press_maybe(self.frames[0].as_ref().map(|_| Message::Rotate))
                        .style(style::quiet),
                );
            }
            layers.push(
                pin(container(tools).width(rect.width.max(100.)).height(32.))
                    .x(rect.x)
                    .y(rect.y - 36.)
                    .into(),
            );
        }
        let r = self.layout.chat;
        let mut messages = column![].spacing(16);
        for message in &self.chat.messages {
            if message.text.is_empty() {
                continue;
            }
            let label = if message.role == "user" {
                "你"
            } else if self.chat.agent_name.is_empty() {
                "Agent"
            } else {
                &self.chat.agent_name
            };
            messages = messages.push(
                column![
                    text(label).size(11).color(style::MUTED),
                    text(&message.text).size(14)
                ]
                .spacing(5),
            );
        }
        let editor = text_editor(&self.draft)
            .placeholder("发送消息…")
            .height(84.)
            .padding(10)
            .size(14)
            .on_action(Message::Edit)
            .key_binding(|press| {
                if press.key == keyboard::Key::Named(keyboard::key::Named::Enter)
                    && press.modifiers.command()
                {
                    Some(text_editor::Binding::Custom(Message::Send))
                } else {
                    text_editor::Binding::from_key_press(press)
                }
            })
            .style(|theme, status| {
                let mut s = text_editor::default(theme, status);
                s.background = Color::TRANSPARENT.into();
                s.border.width = 0.;
                s
            });
        let chat = column![
            scrollable(messages).height(Length::Fill),
            editor,
            row![
                space::horizontal(),
                button(text("发送").size(13))
                    .on_press_maybe(
                        (self.connection.is_some() && !self.draft.text().trim().is_empty())
                            .then_some(Message::Send)
                    )
                    .style(style::primary)
            ]
            .height(32.)
        ]
        .spacing(10);
        layers.push(
            pin(container(chat).padding(18).width(r.width).height(r.height))
                .x(r.x)
                .y(r.y)
                .into(),
        );
        if self.popup_amount > 0. {
            let p = self.popup_rect();
            let mut popup = column![text("Agent").size(14)].spacing(10);
            for agent in &self.agents {
                popup = popup.push(
                    button(text(&agent.name).size(13))
                        .on_press(Message::SelectAgent(wire::AgentProfileRequest {
                            id: agent.id.clone(),
                            expected_revision: agent.revision,
                        }))
                        .style(if agent.id == self.active_agent {
                            button::secondary
                        } else {
                            style::quiet
                        }),
                );
            }
            if self.agents.is_empty() {
                popup = popup.push(text("尚未配置模型").size(13));
            }
            if !self.error.is_empty() && !self.root.is_empty() {
                popup = popup.push(
                    button("重新连接")
                        .on_press(Message::Retry)
                        .style(style::quiet),
                );
            }
            layers.push(
                pin(mouse_area(
                    container(popup)
                        .padding(20)
                        .width(p.width)
                        .height(p.height)
                        .clip(true),
                )
                .on_enter(Message::Hover(true)))
                .x(p.x)
                .y(p.y)
                .into(),
            );
        }
        stack(layers).into()
    }
}
fn main() -> iced::Result {
    if let Some(result) =
        ai_phone_host_runtime::component::run(&std::env::args_os().skip(1).collect::<Vec<_>>())
    {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    iced::application(App::boot, App::update, App::view)
        .title("AImanity · Native")
        .subscription(App::subscription)
        .theme(style::theme())
        .window(window::Settings {
            size: Size::new(1200., 800.),
            min_size: Some(Size::new(720., 600.)),
            exit_on_close_request: false,
            ..Default::default()
        })
        .default_font(iced::Font::with_name(if cfg!(target_os = "macos") {
            "PingFang SC"
        } else {
            "Microsoft YaHei"
        }))
        .run()
}
