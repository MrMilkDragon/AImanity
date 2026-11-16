mod agent_details;
mod agents;
mod audio;
mod chat;
mod connection;
mod controls;
mod devices;
mod files;
mod host;
mod knowledge;
mod layout;
mod locale;
mod location;
mod modes;
mod occlude;
mod platform;
mod proxy;
mod scene;
mod skills;
mod style;
mod surface;
use ai_phone_api::wire;
use connection::{Connection, DeviceInput, Frame};
use iced::{
    Element, Event, Length, Rectangle, Size, Subscription, Task, keyboard,
    widget::{
        button, column, container, image, mouse_area, pin, row, scrollable, sensor, space, stack,
        text,
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
    Conversation(chat::Message),
    Agents(agents::Message),
    Devices(devices::Message),
    CloseDevices,
    Audio(usize, audio::Message),
    Files(Box<files::Message>),
    Knowledge(knowledge::Message),
    Skills(skills::Message),
    Location(location::Message),
    Proxy(proxy::Message),
    Modes(modes::Message),
    SetLocale(locale::Locale),
    Captured(usize, Result<Capture, String>),
    SaveCapture,
    AttachCapture,
    CaptureSaved(Result<(), String>),
    Focus(usize),
    Input(usize, DeviceInput),
    InputDone(usize, Result<(), String>, Instant),
    Retry,
    Tick(Instant),
    Event(Event),
    Hover(bool),
    OpenPanel(Panel, bool),
    PanelSize(iced::Size),
    PanelFocus,
    Link,
    Linked(Result<wire::DeviceLinkState, String>),
    Rotate,
}
#[derive(Clone, Debug)]
struct Capture {
    name: String,
    image: image::Handle,
    png: Arc<Vec<u8>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Language,
    Skills,
    Location,
    Proxy,
    Knowledge,
    Files,
    Goal,
    Agent,
    Capture(usize),
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
    device_admin: devices::Devices,
    show_devices: bool,
    audio: [audio::Playback; 2],
    files: files::Files,
    knowledge: knowledge::Knowledge,
    skills: skills::Skills,
    location: location::Location,
    proxy: proxy::Proxy,
    modes: modes::Modes,
    locale: locale::Locale,
    capture: Option<Capture>,
    cursor: iced::Point,
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
    panel: Panel,
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
            device_admin: Default::default(),
            show_devices: false,
            audio: std::array::from_fn(|_| audio::Playback::new()),
            files: Default::default(),
            knowledge: Default::default(),
            skills: Default::default(),
            location: Default::default(),
            proxy: Default::default(),
            modes: Default::default(),
            locale: Default::default(),
            capture: None,
            cursor: iced::Point::ORIGIN,
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
            panel: Panel::Agent,
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
                app.location.set_root(&host.home);
                app.locale = std::fs::read_to_string(host.home.join("user/native-locale"))
                    .map_or_else(
                        |_| locale::Locale::default(),
                        |s| locale::Locale::parse(s.trim()),
                    );
                app.modes.set_language(app.locale.code());
                app.modes
                    .set_speech_model(host.resource.join("speech/ggml-tiny-q5_1.bin"));
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
                    self.files.set_devices(&self.devices);
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
                            tasks.push(
                                self.audio[index]
                                    .restore(&self.root, &device.id)
                                    .map(move |m| Message::Audio(index, m)),
                            );
                            tasks
                                .push(Task::run(client.clone().frames(device.clone()), move |r| {
                                    Message::Frame(index, r)
                                }));
                        }
                    }
                    tasks.push(
                        self.modes
                            .update(modes::Message::Refresh, &client, "", &self.chat.state)
                            .map(Message::Modes),
                    );
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
                    let task = self
                        .chat
                        .update(chat::Message::History(result), client, "")
                        .map(Message::Conversation);
                    return Task::batch([
                        task,
                        self.modes
                            .observe_chat(&self.chat.state)
                            .map(Message::Modes),
                    ]);
                }
            }
            Message::Conversation(message) => {
                if let chat::Message::Mode(message) = message {
                    return self.update(Message::Modes(message));
                }
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
            Message::Modes(message) => {
                if matches!(message, modes::Message::GoalEdit(_)) {
                    self.keyboard_target = None;
                    self.pinned = true;
                }
                if let Some(client) = &self.connection {
                    let device = self.devices[self.focus]
                        .as_ref()
                        .map_or("", |d| d.id.as_str());
                    let task = self
                        .modes
                        .update(message, client, device, &self.chat.state)
                        .map(Message::Modes);
                    self.chat.recover_voice(&self.modes.take_recovered());
                    if let Some(request) = self.modes.take_voice() {
                        return Task::batch([
                            task,
                            self.chat
                                .send_voice(request, client)
                                .map(Message::Conversation),
                        ]);
                    }
                    return task;
                }
            }
            Message::Audio(i, message) => {
                self.keyboard_target = None;
                return self.audio[i]
                    .update(message, self.connection.as_ref(), i)
                    .map(move |m| Message::Audio(i, m));
            }
            Message::Files(message) => {
                if let Some(client) = &self.connection {
                    return self
                        .files
                        .update(*message, client)
                        .map(|m| Message::Files(Box::new(m)));
                }
            }
            Message::Knowledge(message) => {
                if let Some(client) = &self.connection {
                    return self
                        .knowledge
                        .update(message, client)
                        .map(Message::Knowledge);
                }
            }
            Message::Skills(message) => {
                if let Some(client) = &self.connection {
                    return self.skills.update(message, client).map(Message::Skills);
                }
            }
            Message::Location(message) => {
                if let Some(client) = &self.connection {
                    return self.location.update(message, client).map(Message::Location);
                }
            }
            Message::Proxy(message) => {
                if let Some(client) = &self.connection {
                    return self.proxy.update(message, client).map(Message::Proxy);
                }
            }
            Message::SetLocale(locale) => {
                self.locale = locale;
                self.modes.set_language(locale.code());
                self.set_popup(false);
                if let Err(error) =
                    std::fs::write(self.host_home.join("user/native-locale"), locale.code())
                {
                    self.error = error.to_string();
                }
            }
            Message::Captured(index, result) => {
                if self.panel == Panel::Capture(index) {
                    match result {
                        Ok(capture) => self.capture = Some(capture),
                        Err(error) => self.error = error,
                    }
                }
            }
            Message::AttachCapture => {
                if let Some(capture) = self.capture.take() {
                    self.set_popup(false);
                    return self.update(Message::Conversation(chat::Message::Capture(
                        capture.name,
                        (*capture.png).clone(),
                    )));
                }
            }
            Message::SaveCapture => {
                if let Some(capture) = self.capture.clone() {
                    return Task::perform(
                        async move {
                            if let Some(target) = rfd::AsyncFileDialog::new()
                                .set_file_name(capture.name)
                                .save_file()
                                .await
                            {
                                target
                                    .write(&capture.png)
                                    .await
                                    .map_err(|e| e.to_string())?;
                            }
                            Ok(())
                        },
                        Message::CaptureSaved,
                    );
                }
            }
            Message::CaptureSaved(Err(error)) => {
                self.error = error;
            }
            Message::PanelSize(size) | Message::Agents(agents::Message::Measured(size)) => {
                let height = size.height.ceil();
                if self.panel_content_height != height {
                    self.panel_content_height = height;
                    self.from_panel = self.panel_size;
                    self.from_popup = self.popup_amount;
                    self.popup_transition = Some(Instant::now());
                }
            }
            Message::Agents(agents::Message::Devices) => {
                self.show_devices = true;
                self.from_panel = self.panel_size;
                self.from_popup = self.popup_amount;
                self.popup_transition = Some(Instant::now());
                if let Some(device) = &self.devices[self.focus] {
                    return self.update(Message::Devices(devices::Message::Choose(
                        device.id.clone(),
                    )));
                }
            }
            Message::CloseDevices => {
                self.show_devices = false;
                self.from_panel = self.panel_size;
                self.from_popup = self.popup_amount;
                self.popup_transition = Some(Instant::now());
            }
            Message::Devices(message) => {
                if let Some(client) = &self.connection {
                    let task = self
                        .device_admin
                        .update(message, &self.agents.registry, client)
                        .map(Message::Devices);
                    if let Some(registry) = self.device_admin.registry.take() {
                        let added = registry.devices.iter().any(|d| {
                            d.runtime_state == "managed"
                                && self.devices.iter().flatten().all(|old| old.id != d.id)
                        });
                        for device in self.devices.iter_mut().flatten() {
                            if let Some(updated) =
                                registry.devices.iter().find(|d| d.id == device.id)
                            {
                                device.clone_from(updated);
                            }
                        }
                        self.agents.registry = registry;
                        self.files.set_devices(&self.devices);
                        if added {
                            let root = self.root.clone();
                            return Task::batch([
                                task,
                                Task::perform(Connection::connect(root), |r| {
                                    Message::Connected(r.map(Box::new))
                                }),
                            ]);
                        }
                    }
                    return task;
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
                self.modes.animate(now);
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
                for audio in &mut self.audio {
                    audio.stop();
                }
                self.modes.stop_capture();
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
                if self.popup
                    && self.panel == Panel::Knowledge
                    && let Some(message) = self.knowledge.escape()
                {
                    return self.update(Message::Knowledge(message));
                }
                self.set_popup(false);
                self.pinned = false;
                self.chat.preview = None;
                self.keyboard_target = None;
            }
            Message::Event(Event::Mouse(iced::mouse::Event::CursorMoved { position })) => {
                self.cursor = position;
                let p = self.popup_rect();
                let area = Rectangle {
                    x: p.x - 8.,
                    y: 16.,
                    width: p.width + 16.,
                    height: p.height + 56.,
                };
                if self.popup
                    && !self.pinned
                    && !area.contains(position)
                    && !self.child_rect().contains(position)
                {
                    self.set_popup(false);
                }
            }
            Message::Hover(inside) => {
                if !self.pinned {
                    self.set_popup(inside);
                }
            }
            Message::OpenPanel(panel, click) => {
                if panel == self.panel && self.popup {
                    if click {
                        self.pinned = !self.pinned;
                        self.set_popup(self.pinned);
                    }
                    return Task::none();
                }
                self.chat.preview = None;
                if self.panel == Panel::Knowledge && panel != Panel::Knowledge {
                    let _ = self.update(Message::Knowledge(knowledge::Message::CloseDocument));
                }
                self.panel = panel;
                self.pinned = click;
                self.from_panel = self.panel_size;
                self.from_popup = self.popup_amount;
                self.popup_transition = Some(Instant::now());
                self.set_popup(true);
                self.keyboard_target = None;
                return self.load_panel();
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
                if self.popup
                    && self.panel == Panel::Files
                    && self.popup_rect().contains(self.cursor)
                {
                    let message = self.files.local_drop(self.cursor, self.popup_rect(), path);
                    return self.update(Message::Files(Box::new(message)));
                }
                return self.update(Message::Conversation(chat::Message::Files(vec![path])));
            }
            Message::Event(Event::Mouse(iced::mouse::Event::ButtonReleased(_)))
                if self.panel == Panel::Files =>
            {
                return self.update(Message::Files(Box::new(files::Message::EndDrag)));
            }
            _ => {}
        }
        Task::none()
    }
    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![iced::event::listen_with(|event, status, _| match &event {
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
            Event::Mouse(iced::mouse::Event::ButtonReleased(_))
                if status == iced::event::Status::Ignored =>
            {
                Some(Message::Event(event))
            }
            _ => None,
        })];
        if self.transition.is_some()
            || self.popup_transition.is_some()
            || self.input_started.is_some()
            || self.modes.animating()
        {
            subscriptions.push(window::frames().map(Message::Tick));
        }
        subscriptions.push(self.chat.subscription().map(Message::Conversation));
        subscriptions.push(
            self.agents
                .subscription(self.popup && self.panel == Panel::Agent)
                .map(Message::Agents),
        );
        if self.connection.is_some() {
            subscriptions.push(self.modes.subscription().map(Message::Modes));
        }
        if self.popup && self.panel == Panel::Knowledge {
            subscriptions.push(self.knowledge.subscription().map(Message::Knowledge));
        }
        if self.popup && self.panel == Panel::Location {
            subscriptions.push(self.location.subscription().map(Message::Location));
        }
        if self.show_devices {
            subscriptions.push(self.device_admin.subscription().map(Message::Devices));
        }
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
            let anchor = self.panel_anchor();
            let y = anchor
                .y
                .min(self.size.height - height.min(self.size.height - 96.) - 24.);
            Rectangle {
                x: (anchor.x - width).clamp(32., self.size.width - width - 32.),
                y,
                width,
                height: height.min(self.size.height - 96.) * self.popup_amount,
            }
        } else {
            Rectangle::default()
        }
    }
    fn panel_target(&self) -> Size {
        Size::new(
            (self.size.width - 64.).min(match self.panel {
                Panel::Agent if self.agents.is_editing() => 480.,
                Panel::Agent if self.show_devices => 680.,
                Panel::Agent => 400.,
                Panel::Language => 280.,
                Panel::Skills => 650.,
                Panel::Knowledge => 850.,
                Panel::Files => 1120.,
                Panel::Location => 560.,
                Panel::Proxy => 420.,
                Panel::Goal => 650.,
                Panel::Capture(0) => 440.,
                Panel::Capture(_) => 700.,
            }),
            (match self.panel {
                Panel::Files => 736.,
                Panel::Knowledge => 590.,
                Panel::Skills => 550.,
                _ => self.panel_content_height,
            })
            .min(self.size.height - 96.),
        )
    }
    const NAV_PANELS: [Panel; 8] = [
        Panel::Language,
        Panel::Skills,
        Panel::Location,
        Panel::Proxy,
        Panel::Knowledge,
        Panel::Files,
        Panel::Goal,
        Panel::Agent,
    ];
    fn panel_anchor(&self) -> iced::Point {
        if let Panel::Capture(index) = self.panel {
            let device = self.layout.devices[index];
            return iced::Point::new(device.x + self.panel_target().width, device.y);
        }
        let index = Self::NAV_PANELS
            .iter()
            .position(|p| *p == self.panel)
            .unwrap();
        iced::Point::new(self.size.width - 80. - (7 - index) as f32 * 48., 64.)
    }
    fn load_panel(&mut self) -> Task<Message> {
        let message = match self.panel {
            Panel::Skills => Message::Skills(skills::Message::Load),
            Panel::Knowledge => Message::Knowledge(knowledge::Message::Load),
            Panel::Location => Message::Location(location::Message::Load),
            Panel::Proxy => Message::Proxy(proxy::Message::Load),
            Panel::Files => Message::Files(Box::new(files::Message::Load)),
            Panel::Goal => Message::Modes(modes::Message::Refresh),
            Panel::Capture(index) => {
                self.capture = None;
                let Some(frame) = self.frames[index].clone() else {
                    return Task::none();
                };
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let mut png = Vec::new();
                            use ::image::ImageEncoder;
                            ::image::codecs::png::PngEncoder::new(&mut png)
                                .write_image(
                                    &frame.pixels,
                                    frame.width,
                                    frame.height,
                                    ::image::ExtendedColorType::Rgba8,
                                )
                                .map_err(|e| e.to_string())?;
                            let image = image::Handle::from_rgba(
                                frame.width,
                                frame.height,
                                frame.pixels.as_ref().clone(),
                            );
                            let name = format!(
                                "{}-{}.png",
                                if index == 0 { "Android" } else { "Linux" },
                                chrono::Local::now().format("%Y%m%d-%H%M%S")
                            );
                            Ok(Capture {
                                png: Arc::new(png),
                                image,
                                name,
                            })
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    move |r| Message::Captured(index, r),
                );
            }
            _ => return Task::none(),
        };
        self.update(message)
    }
    fn child_rect(&self) -> Rectangle {
        if !self.popup || self.panel != Panel::Knowledge || !self.knowledge.document_open() {
            return Rectangle::default();
        }
        let parent = self.popup_rect();
        let width = 620f32.min(self.size.width - 64.);
        let height = 600f32.min(self.size.height - 96.);
        let anchor = self.knowledge.document_anchor().unwrap_or(iced::Point::new(
            parent.x + parent.width - 20.,
            parent.y + 44.,
        ));
        Rectangle::new(
            iced::Point::new(
                (anchor.x + 12.).clamp(32., self.size.width - width - 32.),
                (anchor.y + 12.).clamp(64., self.size.height - height - 24.),
            ),
            Size::new(width, height),
        )
    }
    fn panel_body(&self) -> Element<'_, Message> {
        let locale = self.locale;
        let content = match self.panel {
            Panel::Agent if self.show_devices => column![
                row![
                    controls::icon_button(
                        controls::Icon::Back,
                        locale.tr("返回"),
                        Some(Message::CloseDevices)
                    ),
                    text(locale.tr("设备")).size(14)
                ]
                .align_y(iced::Center),
                self.device_admin
                    .view(&self.agents.registry, locale)
                    .map(Message::Devices)
            ]
            .spacing(12)
            .into(),
            Panel::Agent => return self.agents.view(locale).map(Message::Agents),
            Panel::Skills => return self.skills.view(locale).map(Message::Skills),
            Panel::Knowledge => return self.knowledge.view(locale).map(Message::Knowledge),
            Panel::Files => self
                .files
                .view(self.size.height - 96., locale)
                .map(|m| Message::Files(Box::new(m))),
            Panel::Location => self.location.view(locale).map(Message::Location),
            Panel::Proxy => self.proxy.view(locale).map(Message::Proxy),
            Panel::Goal => self.modes.goal_view().map(Message::Modes),
            Panel::Language => column(locale::Locale::all().map(|l| {
                button(
                    row![
                        text(l.glyph()).size(12).width(32),
                        text(l.name()).size(14),
                        space::horizontal(),
                        container(controls::Icon::Check.view().opacity(if locale == l {
                            1.
                        } else {
                            0.
                        }))
                        .width(18)
                    ]
                    .spacing(8)
                    .align_y(iced::Center),
                )
                .on_press(Message::SetLocale(l))
                .height(40)
                .width(Length::Fill)
                .style(style::quiet)
                .into()
            }))
            .spacing(4)
            .into(),
            Panel::Capture(_) => {
                let mut body = column![text(locale.tr("截图")).size(14)].spacing(12);
                if let Some(capture) = &self.capture {
                    body = body
                        .push(
                            image(capture.image.clone())
                                .width(Length::Fill)
                                .height(410)
                                .content_fit(iced::ContentFit::Contain),
                        )
                        .push(
                            row![
                                space::horizontal(),
                                button(locale.tr("保存图片"))
                                    .on_press(Message::SaveCapture)
                                    .style(style::quiet),
                                controls::action_button(
                                    locale.tr("附到对话"),
                                    Some(Message::AttachCapture)
                                )
                                .width(100)
                            ]
                            .spacing(8),
                        );
                } else {
                    body = body.push(text(locale.tr("正在截图")).size(13));
                }
                body.into()
            }
        };
        scrollable(sensor(container(content).padding(24)).on_resize(Message::PanelSize))
            .height(Length::Fill)
            .into()
    }
    fn view(&self) -> Element<'_, Message> {
        use controls::{Icon, icon_button};
        let tr = |key: &'static str| self.locale.tr(key);
        let popup = self.popup_rect();
        let child_popup = self.child_rect();
        let scene = scene::Scene {
            layout: self.layout,
            size: self.size,
            frames: self.frames.clone(),
            focus: self.focus,
            popup,
            child_popup,
            mode_effects: self.modes.effects(),
            keyboard_target: self.keyboard_target,
        };
        let mut layers = vec![surface::Surface::new(scene).into()];
        let error_message = [
            self.error.as_str(),
            self.modes.voice_error(),
            self.modes.proactive_error(),
            self.audio[0].error(),
            self.audio[1].error(),
            self.chat.error.as_str(),
        ]
        .into_iter()
        .find(|error| !error.is_empty())
        .unwrap_or("");
        let error = if error_message.is_empty() {
            text(self.locale.tr(&self.phase)).size(12)
        } else {
            text(error_message).size(12).color(style::ERROR)
        };
        let mut nav = row![
            container(error).width(Length::Fill).clip(true),
            icon_button(
                Icon::Cable,
                tr(if self.linked {
                    "断开数据线"
                } else {
                    "连接数据线"
                }),
                self.connection.as_ref().map(|_| Message::Link)
            ),
        ];
        for panel in Self::NAV_PANELS {
            let (icon, label, active) = match panel {
                Panel::Language => (Icon::Settings, self.locale.name(), false),
                Panel::Skills => (Icon::Wrench, tr("Skill管理"), false),
                Panel::Location => (Icon::Pin, tr("模拟位置"), self.location.enabled()),
                Panel::Proxy => (Icon::Network, tr("工作台代理"), self.proxy.enabled()),
                Panel::Knowledge => (Icon::Book, tr("知识库"), false),
                Panel::Files => (Icon::Folder, tr("文件管理"), false),
                Panel::Goal => (Icon::Target, tr("任务与目标"), self.modes.goal_active()),
                Panel::Agent => (Icon::Settings, tr("配置中心"), false),
                Panel::Capture(_) => unreachable!(),
            };
            let click = self
                .connection
                .as_ref()
                .map(|_| Message::OpenPanel(panel, true));
            let chosen = self.popup && self.panel == panel || active;
            let trigger: Element<'_, Message> = if panel == Panel::Language || chosen {
                let content: Element<'_, Message> = if panel == Panel::Language {
                    text(self.locale.glyph()).size(13).into()
                } else {
                    icon.view().into()
                };
                iced::widget::tooltip(
                    button(container(content).center(Length::Fill))
                        .width(40)
                        .height(40)
                        .style(move |theme, status| {
                            let mut s = style::quiet(theme, status);
                            if chosen {
                                s.background =
                                    Some(iced::Color::from_rgba(0., 0., 0., 0.10).into());
                            }
                            s
                        })
                        .on_press_maybe(click),
                    text(label).size(12),
                    iced::widget::tooltip::Position::Bottom,
                )
                .into()
            } else {
                icon_button(icon, label, click)
            };
            nav = nav.push(if self.connection.is_some() {
                mouse_area(trigger)
                    .on_enter(Message::OpenPanel(panel, false))
                    .into()
            } else {
                trigger
            });
        }
        nav = nav.push(icon_button(
            Icon::Refresh,
            tr("刷新对话显示"),
            if !self.error.is_empty() && !self.root.is_empty() {
                Some(Message::Retry)
            } else {
                self.connection
                    .as_ref()
                    .map(|_| Message::Conversation(chat::Message::RefreshDisplay))
            },
        ));
        let nav = nav.spacing(8).align_y(iced::Center);
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
                .unwrap_or(tr(if index == 0 { "手机" } else { "电脑" }));
            let mut tools = row![text(name).size(13)]
                .spacing(8)
                .align_y(iced::Alignment::Center);
            if self.focus == index || self.linked {
                tools = tools.push(
                    self.audio[index]
                        .view(self.connection.is_some(), index, self.locale)
                        .map(move |m| Message::Audio(index, m)),
                );
                if index == 0 {
                    tools = tools.push(icon_button(
                        Icon::Screen,
                        tr(if self.aspects()[0] > 1. {
                            "切换竖屏"
                        } else {
                            "切换横屏"
                        }),
                        self.frames[0].as_ref().map(|_| Message::Rotate),
                    ));
                }
                tools = tools.push(icon_button(
                    Icon::Camera,
                    tr("截图"),
                    self.frames[index]
                        .as_ref()
                        .map(|_| Message::OpenPanel(Panel::Capture(index), true)),
                ));
            }
            layers.push(
                pin(container(occlude::behind(
                    occlude::behind(tools.into(), popup),
                    child_popup,
                ))
                .width(rect.width.max(255.))
                .height(40.))
                .x(rect.x)
                .y(rect.y - 41.)
                .into(),
            );
        }
        let r = self.layout.chat;
        layers.push(
            pin(container(occlude::behind(
                occlude::behind(
                    self.chat
                        .view(&self.modes, self.locale)
                        .map(Message::Conversation),
                    popup,
                ),
                child_popup,
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
                pin(container(
                    self.chat
                        .preview_view(self.locale)
                        .map(Message::Conversation),
                )
                .width(p.width)
                .height(p.height)
                .clip(true))
                .x(p.x)
                .y(p.y)
                .into(),
            );
        } else if self.popup_amount > 0. {
            layers.push(
                pin(mouse_area(
                    container(
                        container(occlude::behind(self.panel_body(), child_popup))
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
        if child_popup.width > 0.
            && let Some(document) = self.knowledge.document_view(self.locale)
        {
            layers.push(
                pin(container(document.map(Message::Knowledge))
                    .width(child_popup.width)
                    .height(child_popup.height)
                    .clip(true))
                .x(child_popup.x)
                .y(child_popup.y)
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
        .theme(|app: &App| {
            if app.modes.copilot_active() {
                style::copilot_theme()
            } else {
                style::theme()
            }
        })
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
