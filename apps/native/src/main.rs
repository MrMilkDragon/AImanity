mod agent_details;
mod agents;
mod chat;
mod connection;
mod controls;
mod host;
mod layout;
mod occlude;
mod platform;
mod scene;
mod style;
mod surface;
use ai_phone_api::wire;
use connection::{Connection, DeviceInput, Frame};
use iced::{
    Element, Event, Length, Rectangle, Size, Subscription, Task, keyboard,
    widget::{button, container, mouse_area, pin, row, stack, text},
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
    Conversation(chat::Message),
    Agents(agents::Message),
    Focus(usize),
    Input(usize, DeviceInput),
    InputDone(usize, Result<(), String>, Instant),
    Retry,
    Tick(Instant),
    Event(Event),
    Hover(bool),
    Pin,
    PanelFocus,
    Link,
    Linked(Result<wire::DeviceLinkState, String>),
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
    chat: chat::Chat,
    agents: agents::Agents,
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
    panel_size: Size,
    panel_content_height: f32,
    from_panel: Size,
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
            chat: chat::Chat::new(),
            agents: agents::Agents::new(),
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
            panel_size: Size::new(400., 140.),
            panel_content_height: 140.,
            from_panel: Size::new(400., 140.),
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
            self.from_panel = self.panel_size;
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
                    self.agents.registry = registry;
                    let chat_task = self.chat.accept(chat, &client).map(Message::Conversation);
                    let restore = self.chat.restore(&self.root).map(Message::Conversation);
                    self.error.clear();
                    self.connection = Some(client.clone());
                    self.rearrange();
                    eprintln!(
                        "native_metric connected_ms={:0.3}",
                        self.started.elapsed().as_secs_f64() * 1000.
                    );
                    let mut tasks = vec![
                        chat_task,
                        restore,
                        Task::run(client.clone().chat_events(), Message::Chat),
                    ];
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
            Message::Chat(result) => {
                if let Some(client) = &self.connection {
                    return self
                        .chat
                        .update(chat::Message::History(result), client, "")
                        .map(Message::Conversation);
                }
            }
            Message::Conversation(message) => {
                if matches!(message, chat::Message::Edit(_)) {
                    self.keyboard_target = None;
                    self.input_started = Some(Instant::now());
                }
                if matches!(message, chat::Message::Attachment(_)) {
                    self.set_popup(false);
                    self.pinned = false;
                }
                if let Some(client) = &self.connection {
                    let device_id = self.devices[self.focus]
                        .as_ref()
                        .map(|d| d.id.as_str())
                        .unwrap_or("");
                    return self
                        .chat
                        .update(message, client, device_id)
                        .map(Message::Conversation);
                }
            }
            Message::Agents(agents::Message::Measured(size)) => {
                let height = size.height.ceil();
                if self.panel_content_height != height {
                    self.panel_content_height = height;
                    self.from_panel = self.panel_size;
                    self.from_popup = self.popup_amount;
                    self.popup_transition = Some(Instant::now());
                }
            }
            Message::Agents(message) => {
                let interaction = match &message {
                    agents::Message::Registry(_) | agents::Message::Credential(_) => false,
                    agents::Message::Detail(_, message) => !matches!(
                        message,
                        agent_details::Message::Memories(_)
                            | agent_details::Message::MemorySaved(..)
                            | agent_details::Message::MemoryDeleted(..)
                            | agent_details::Message::Channels(_)
                            | agent_details::Message::ChannelSaved(_)
                            | agent_details::Message::Poll
                            | agent_details::Message::Voice(_)
                            | agent_details::Message::VoiceSaved(_)
                    ),
                    _ => true,
                };
                if interaction {
                    self.keyboard_target = None;
                    self.pinned = true;
                }
                if let Some(client) = &self.connection {
                    let changed = matches!(&message, agents::Message::Registry(Ok(_)));
                    let before = self.panel_target();
                    let task = self.agents.update(message, client).map(Message::Agents);
                    if self.panel_target() != before {
                        self.from_panel = self.panel_size;
                        self.from_popup = self.popup_amount;
                        self.popup_transition = Some(Instant::now());
                    }
                    if changed {
                        let client = client.clone();
                        return Task::batch([
                            task,
                            Task::perform(async move { client.history().await }, Message::Chat),
                        ]);
                    }
                    return task;
                }
            }
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
                    let to = self.panel_target();
                    let blend = 1. - (1. - t).powi(3);
                    self.panel_size = Size::new(
                        self.from_panel.width + (to.width - self.from_panel.width) * blend,
                        self.from_panel.height + (to.height - self.from_panel.height) * blend,
                    );
                    if t >= 1. {
                        self.popup_transition = None;
                    }
                }
            }
            Message::Event(Event::Window(window::Event::Opened { .. })) => {
                #[cfg(target_os = "macos")]
                platform::menu();
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
                self.chat.preview = None;
                self.keyboard_target = None;
            }
            Message::Event(Event::Mouse(iced::mouse::Event::CursorMoved { position }))
                if self.popup && !self.pinned =>
            {
                let p = self.popup_rect();
                let area = Rectangle {
                    x: p.x - 8.,
                    y: 16.,
                    width: p.width + 16.,
                    height: p.height + 56.,
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
                self.chat.preview = None;
                self.pinned = !self.pinned;
                self.set_popup(self.pinned);
                self.keyboard_target = None;
            }
            Message::PanelFocus => {
                self.keyboard_target = None;
                self.pinned = true;
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
            Message::Event(Event::Window(window::Event::FileDropped(path))) => {
                return self.update(Message::Conversation(chat::Message::Files(vec![path])));
            }
            _ => {}
        }
        Task::none()
    }
    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![iced::event::listen_with(|event, _, _| match &event {
            Event::Window(
                window::Event::Opened { .. }
                | window::Event::Resized(_)
                | window::Event::CloseRequested,
            )
            | Event::Window(window::Event::FileDropped(_))
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
        subscriptions.push(self.chat.subscription().map(Message::Conversation));
        subscriptions.push(self.agents.subscription(self.popup).map(Message::Agents));
        Subscription::batch(subscriptions)
    }
    fn popup_rect(&self) -> Rectangle {
        if self.chat.preview.is_some() {
            return Rectangle {
                x: 32.,
                y: 64.,
                width: (self.size.width - 64.).min(720.),
                height: self.size.height - 96.,
            };
        }
        if self.popup_amount > 0. {
            let width = self.panel_size.width.min(self.size.width - 64.);
            let height = self.panel_size.height;
            Rectangle {
                x: self.size.width - width - 32.,
                y: 64.,
                width,
                height: height.min(self.size.height - 96.) * self.popup_amount,
            }
        } else {
            Rectangle::default()
        }
    }
    fn panel_target(&self) -> Size {
        Size::new(
            (self.size.width - 64.).min(if self.agents.is_editing() { 480. } else { 400. }),
            self.panel_content_height.min(self.size.height - 96.),
        )
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
        let error_message = if !self.error.is_empty() {
            &self.error
        } else {
            &self.chat.error
        };
        let error = if error_message.is_empty() {
            text(&self.phase).size(12)
        } else {
            text(error_message).size(12).color(style::ERROR)
        };
        let nav = row![
            container(error).width(Length::Fill).clip(true),
            controls::icon_button(
                controls::Icon::Refresh,
                "重新连接",
                (!self.error.is_empty() && !self.root.is_empty()).then_some(Message::Retry)
            ),
            controls::icon_button(
                controls::Icon::Cable,
                if self.linked { "断开" } else { "连接" },
                self.connection.as_ref().map(|_| Message::Link)
            ),
            mouse_area(controls::icon_button(
                controls::Icon::Settings,
                "Agent",
                self.connection.as_ref().map(|_| Message::Pin)
            ))
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
        layers.push(
            pin(container(occlude::behind(
                self.chat.view().map(Message::Conversation),
                self.popup_rect(),
            ))
            .padding(18)
            .width(r.width)
            .height(r.height))
            .x(r.x)
            .y(r.y)
            .into(),
        );
        let p = self.popup_rect();
        if self.chat.preview.is_some() {
            layers.push(
                pin(
                    container(self.chat.preview_view().map(Message::Conversation))
                        .width(p.width)
                        .height(p.height)
                        .clip(true),
                )
                .x(p.x)
                .y(p.y)
                .into(),
            );
        } else if self.popup_amount > 0. {
            layers.push(
                pin(mouse_area(
                    container(
                        container(self.agents.view().map(Message::Agents))
                            .width(self.panel_target().width)
                            .height(p.height),
                    )
                    .width(p.width)
                    .height(p.height)
                    .clip(true),
                )
                .on_enter(Message::Hover(true))
                .on_press(Message::PanelFocus))
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
