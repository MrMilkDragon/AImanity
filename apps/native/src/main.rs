#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod agent_details;
mod agents;
mod audio;
mod chat;
#[path = "update_core.rs"]
mod client_update;
mod connection;
mod controls;
mod deployment;
mod devices;
mod files;
mod host;
mod host_desktop;
mod knowledge;
mod layout;
mod loading;
mod locale;
mod location;
mod modes;
mod occlude;
mod platform;
mod proxy;
mod scene;
mod skills;
mod spring_slider;
mod style;
mod surface;
mod todos;
mod uninstall;
mod updates;
use ai_phone_api::wire;
use connection::{Connection, DeviceInput, Frame, Press};
use iced::{
    Element, Event, Length, Rectangle, Size, Subscription, Task, keyboard,
    widget::{
        button, column, container, image, mouse_area, pin, row, scrollable, sensor, space, stack,
        text,
    },
    window,
};
use layout::Layout;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const APP_ID: &str = "dev.aimanity.native";
const SETUP_SIZE: Size = Size::new(680., 600.);
const WORKBENCH_ASPECT: Size = Size::new(8., 5.);
const WORKBENCH: Size = Size::new(1440., 900.);
const WORKBENCH_MIN: Size = Size::new(960., 600.);
const DESKTOP_DOUBLE_CLICK: Duration = Duration::from_millis(320);

#[derive(Clone, Debug)]
enum Message {
    Host(host::Event),
    Setup(deployment::Message),
    Connected(Result<Box<connection::Session>, String>),
    Frame(usize, Result<Frame, String>),
    PollAgentPointers,
    AgentPointers(Box<[Option<wire::DeviceCursor>; 2]>),
    Chat(Result<wire::ChatState, String>),
    Conversation(chat::Message),
    Agents(agents::Message),
    Devices(devices::Message),
    CloseDevices,
    OpenDevices,
    HostAccess(bool),
    Audio(usize, audio::Message),
    Files(Box<files::Message>),
    Todos(todos::Message),
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
    DesktopClickElapsed(Instant),
    InputDone(usize, Result<(), String>, Instant),
    Retry,
    Quit,
    Tick(Instant),
    FitViewport(window::Id),
    Viewport(Size),
    Event(Event),
    OpenPanel(Panel, bool),
    PanelHovered(Panel),
    PanelLeft,
    ClosePanel,
    PanelSize(iced::Size),
    PanelFocus,
    ClosedPaneInput,
    Link,
    Linked(Result<wire::DeviceLinkState, String>),
    RefreshLink,
    LinkSnapshot(Result<wire::DeviceLinkState, String>),
    Rotate,
    Update(updates::Message),
    Uninstall,
    UninstallPrepared(Result<Option<uninstall::Plan>, String>),
}
#[derive(Clone, Debug)]
struct Capture {
    name: String,
    image: image::Handle,
    png: Arc<Vec<u8>>,
    width: f32,
    height: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Todos,
    Language,
    Skills,
    Location,
    Proxy,
    Knowledge,
    Files,
    Goal,
    Agent,
    Update,
    Capture(usize),
}
#[derive(Clone, PartialEq, Eq)]
enum ChildPane {
    Agent(String, agents::Tab),
    Devices,
    Knowledge(bool),
    Preview { id: String, name: String },
}
impl ChildPane {
    fn separate(&self) -> bool {
        match self {
            Self::Agent(id, _) => id.is_empty(),
            Self::Devices | Self::Preview { .. } => false,
            Self::Knowledge(_) => true,
        }
    }
    fn same_window(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Agent(id, _), Self::Agent(other, _)) => id == other,
            (Self::Knowledge(_), Self::Knowledge(_)) => true,
            _ => false,
        }
    }
}
struct ChildWindow {
    pane: ChildPane,
    previous: Option<ChildPane>,
    anchor: Rectangle,
    bounds: Rectangle,
    rect: Rectangle,
    opacity: f32,
    animation: Option<(Instant, Rectangle, f32)>,
    open: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterStop {
    Exit,
    InstallAgain,
    Stay,
}
struct App {
    setup: Option<deployment::Setup>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    host_home: std::path::PathBuf,
    updates: updates::State,
    uninstall: Option<uninstall::Plan>,
    #[cfg(windows)]
    instance_guard: Option<std::os::windows::io::OwnedHandle>,
    phase: String,
    after_stop: Option<AfterStop>,
    connection: Option<Connection>,
    devices: [Option<wire::RegisteredDevice>; 2],
    frames: [Option<Arc<Frame>>; 2],
    agent_traces: [Option<scene::AgentTrace>; 2],
    pointer_polling: bool,
    frame_errors: [String; 2],
    chat_connection_error: String,
    chat: chat::Chat,
    agents: agents::Agents,
    device_admin: devices::Devices,
    show_devices: bool,
    audio: [audio::Playback; 2],
    files: files::Files,
    todos: todos::Todos,
    knowledge: knowledge::Knowledge,
    skills: skills::Skills,
    location: location::Location,
    proxy: proxy::Proxy,
    modes: modes::Modes,
    reduced_motion: bool,
    locale: locale::Locale,
    capture: Option<Capture>,
    cursor: iced::Point,
    capture_offset: f32,
    error: String,
    root: String,
    focus: usize,
    linked: bool,
    link_revision: u64,
    keyboard_target: Option<usize>,
    busy: [bool; 2],
    input_queue: [std::collections::VecDeque<DeviceInput>; 2],
    desktop_click: Option<(Instant, DeviceInput)>,
    streams: Option<iced::task::Handle>,
    ui_scale: f32,
    canvas_origin: iced::Vector,
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
    child: Option<ChildWindow>,
    agent_add_height: f32,
    panel_leave_pending: bool,
    started: Instant,
    input_started: Option<Instant>,
}
impl App {
    fn boot() -> (Self, Task<Message>) {
        let layout = Layout::target(WORKBENCH, 0, false, [0.5, 1.6]);
        let mut app = Self {
            setup: None,
            stop: None,
            host_home: std::path::PathBuf::new(),
            updates: Default::default(),
            uninstall: None,
            #[cfg(windows)]
            instance_guard: host::windows_guard().ok(),
            phase: String::new(),
            after_stop: None,
            connection: None,
            devices: [None, None],
            frames: [None, None],
            agent_traces: [None, None],
            pointer_polling: false,
            frame_errors: Default::default(),
            chat_connection_error: String::new(),
            chat: chat::Chat::new(),
            agents: agents::Agents::new(),
            device_admin: Default::default(),
            show_devices: false,
            audio: std::array::from_fn(|_| audio::Playback::new()),
            files: Default::default(),
            todos: Default::default(),
            knowledge: Default::default(),
            skills: Default::default(),
            location: Default::default(),
            proxy: Default::default(),
            modes: Default::default(),
            reduced_motion: platform::reduced_motion(),
            locale: Default::default(),
            capture: None,
            cursor: iced::Point::ORIGIN,
            capture_offset: 0.,
            error: String::new(),
            root: String::new(),
            focus: 0,
            linked: false,
            link_revision: 0,
            keyboard_target: None,
            busy: [false, false],
            input_queue: Default::default(),
            desktop_click: None,
            streams: None,
            ui_scale: 1.,
            canvas_origin: iced::Vector::ZERO,
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
            child: None,
            agent_add_height: 505.,
            panel_leave_pending: false,
            started: Instant::now(),
            input_started: None,
        };
        #[cfg(windows)]
        if app.instance_guard.is_none() {
            return (app, iced::exit());
        }
        let task = if let Some(path) = std::env::args().nth(1) {
            app.start(host::Host::load(&path))
        } else {
            match deployment::Bundle::load().and_then(|bundle| {
                let request = bundle.request()?;
                let installed = bundle.installed()?;
                Ok((bundle, request, installed))
            }) {
                Ok((bundle, request, installed)) => {
                    let setup = deployment::Setup::new(bundle, request, installed);
                    app.locale = setup.locale.unwrap_or_default();
                    app.setup = Some(setup);
                    if installed {
                        app.update(Message::Setup(deployment::Message::Start))
                    } else {
                        Task::none()
                    }
                }
                Err(error) => {
                    app.error = error;
                    Task::none()
                }
            }
        };
        (app, task)
    }
    fn start(&mut self, host: Result<host::Host, String>) -> Task<Message> {
        match host {
            Ok(host) => {
                if let Some(setup) = &mut self.setup {
                    setup.attempted = true;
                    #[cfg(target_os = "macos")]
                    setup.activities.clear();
                }
                self.phase = "正在启动设备".into();
                self.error.clear();
                self.started = Instant::now();
                self.host_home = host.home.clone();
                self.chat.host = host_desktop::Permission::load(&host.home);
                self.location.set_root(&host.home);
                let locale = self
                    .setup
                    .as_ref()
                    .and_then(|setup| setup.locale)
                    .unwrap_or_default();
                self.locale = std::fs::read_to_string(host.home.join("user/native-locale"))
                    .map_or(locale, |s| locale::Locale::parse(s.trim()));
                self.modes.set_language(self.locale.code());
                self.modes
                    .set_speech_model(host.resource.join("speech/ggml-tiny-q5_1.bin"));
                let (stop, receiver) = tokio::sync::oneshot::channel();
                self.stop = Some(stop);
                Task::run(host.events(receiver), Message::Host)
            }
            Err(error) => {
                self.error = error;
                Task::none()
            }
        }
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
        self.target = Layout::target(WORKBENCH, self.focus, self.linked, self.aspects());
        self.transition = if self.reduced_motion {
            self.layout = self.target;
            None
        } else {
            Some(Instant::now())
        };
    }
    fn accept_link(&mut self, state: wire::DeviceLinkState) {
        if let [Some(phone), Some(pc)] = &self.devices
            && let Some(link) = state.links.into_iter().find(|link| {
                (link.source_device_id == phone.id && link.target_device_id == pc.id)
                    || (link.source_device_id == pc.id && link.target_device_id == phone.id)
            })
            && link.revision >= self.link_revision
        {
            self.link_revision = link.revision;
            if self.linked != link.connected {
                self.linked = link.connected;
                self.rearrange();
            }
        }
    }
    fn set_popup(&mut self, open: bool) {
        if self.popup != open {
            if !open && self.panel == Panel::Todos {
                self.todos.cancel_drag();
            }
            self.popup = open;
            self.reflow_popup();
            self.reflow_child();
        }
    }
    fn reflow_popup(&mut self) {
        if self.reduced_motion {
            self.popup_amount = f32::from(self.popup);
            self.panel_size = self.panel_target();
            self.popup_transition = None;
        } else {
            self.from_panel = self.panel_size;
            self.from_popup = self.popup_amount;
            self.popup_transition = Some(Instant::now());
        }
    }
    fn finish_exit(&mut self) -> Task<Message> {
        if let Some(plan) = self.uninstall.take() {
            #[cfg(windows)]
            drop(self.instance_guard.take());
            if let Err(error) = plan.launch() {
                #[cfg(windows)]
                {
                    self.instance_guard = host::windows_guard().ok();
                }
                self.phase.clear();
                self.error = error;
                return window::latest()
                    .and_then(|id| window::set_mode(id, window::Mode::Windowed));
            }
        }
        iced::exit()
    }
    fn desktop_input(&mut self, input: DeviceInput) -> Task<Message> {
        let pending = self.desktop_click.take();
        let now = Instant::now();
        let primary = matches!(
            input,
            DeviceInput::Tap {
                press: Press::Left,
                ..
            }
        );
        if let (
            Some((previous, DeviceInput::Tap { x: x0, y: y0, .. })),
            DeviceInput::Tap {
                x,
                y,
                press: Press::Left,
            },
        ) = (&pending, &input)
            && now.duration_since(*previous) < DESKTOP_DOUBLE_CLICK
            && f64::from(x0.abs_diff(*x)).hypot(f64::from(y0.abs_diff(*y))) < 12.
        {
            return self.input(
                1,
                DeviceInput::Tap {
                    x: *x,
                    y: *y,
                    press: Press::Double,
                },
            );
        }
        let previous = pending.map_or_else(Task::none, |(_, input)| self.input(1, input));
        let next = if primary {
            self.desktop_click = Some((now, input));
            Task::perform(
                async move { tokio::time::sleep_until((now + DESKTOP_DOUBLE_CLICK).into()).await },
                move |()| Message::DesktopClickElapsed(now),
            )
        } else {
            self.input(1, input)
        };
        Task::batch([previous, next])
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
        if self.after_stop == Some(AfterStop::Exit)
            && !matches!(message, Message::Host(host::Event::Stopped(_)))
        {
            return Task::none();
        }
        match message {
            Message::Setup(message) if self.after_stop.is_none() => {
                if let Some(setup) = &mut self.setup {
                    match message {
                        deployment::Message::Start if self.phase.is_empty() => {
                            if let Some(stop) = self.stop.take() {
                                self.after_stop = Some(AfterStop::InstallAgain);
                                self.streams = None;
                                self.error.clear();
                                let _ = stop.send(());
                                self.phase = "正在重新启动设备".into();
                            } else {
                                let host = setup.bundle.host(&setup.request);
                                return self.start(host);
                            }
                        }
                        deployment::Message::Stop if !self.phase.is_empty() => {
                            if let Some(stop) = self.stop.take() {
                                self.after_stop = Some(AfterStop::Stay);
                                self.streams = None;
                                self.error.clear();
                                self.phase = "正在停止安装".into();
                                let _ = stop.send(());
                            }
                        }
                        _ => setup.update(message),
                    }
                }
            }
            Message::Setup(_) => {}
            Message::Host(
                host::Event::Phase(_) | host::Event::Ready(_) | host::Event::Failed(_),
            )
            | Message::Connected(_)
                if self.after_stop.is_some() || (self.setup.is_some() && self.stop.is_none()) => {}
            #[cfg(target_os = "macos")]
            Message::Host(host::Event::Activity(activity)) => {
                if self.after_stop.is_none()
                    && let Some(setup) = &mut self.setup
                {
                    setup.activity(activity);
                }
            }
            Message::Host(host::Event::Phase(phase)) => {
                eprintln!(
                    "native_metric startup_phase={phase} ms={:.3}",
                    self.started.elapsed().as_secs_f64() * 1000.
                );
                self.phase = match phase.as_str() {
                    "source" => "正在加载组件",
                    "profile" => "正在配置设备",
                    "android" => "正在启动手机",
                    "engine" | "backend" => "正在启动电脑",
                    "services" => "正在启动服务",
                    "input" => "正在连接输入",
                    "devices" | "ready" => "正在连接设备",
                    _ => "正在启动",
                }
                .into();
            }
            Message::Host(host::Event::Ready(root)) => {
                self.root = root.clone();
                self.phase = "正在连接设备".into();
                let (task, handle) = Task::perform(Connection::connect(root), |r| {
                    Message::Connected(r.map(Box::new))
                })
                .abortable();
                self.streams = Some(handle.abort_on_drop());
                return task;
            }
            Message::Host(host::Event::Failed(error)) => {
                self.error = error;
                self.phase.clear();
            }
            Message::Host(host::Event::Stopped(result)) => {
                self.phase.clear();
                let action = self.after_stop.take();
                match result {
                    Ok(()) => match action {
                        Some(AfterStop::Exit) => return self.finish_exit(),
                        Some(AfterStop::InstallAgain) => {
                            return self.update(Message::Setup(deployment::Message::Start));
                        }
                        _ => {}
                    },
                    Err(error) => {
                        self.uninstall = None;
                        self.error = error;
                        if action == Some(AfterStop::Exit) {
                            return window::latest()
                                .and_then(|id| window::set_mode(id, window::Mode::Windowed));
                        }
                    }
                }
            }
            Message::Connected(result) => match result {
                Ok(session) => {
                    let connection::Session {
                        client,
                        registry,
                        links,
                    } = *session;
                    let packaged = self.setup.is_some();
                    if let Some(setup) = self.setup.take()
                        && let Err(error) = setup.bundle.mark_installed()
                    {
                        self.error = error;
                    }
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
                    self.accept_link(links);
                    self.agents.registry = registry;
                    self.files.set_devices(&self.devices);
                    let restore = self.chat.restore(&self.root).map(Message::Conversation);
                    self.phase.clear();
                    let first_connection = self.connection.replace(client.clone()).is_none();
                    self.rearrange();
                    eprintln!(
                        "native_metric connected_ms={:0.3}",
                        self.started.elapsed().as_secs_f64() * 1000.
                    );
                    let mut tasks = vec![
                        restore,
                        Task::run(client.clone().chat_events(), Message::Chat),
                    ];
                    if first_connection {
                        tasks.push(window::latest().and_then(|id| {
                            window::monitor_size(id).then(move |monitor| {
                                let monitor = monitor.unwrap_or(WORKBENCH);
                                let scale = ((monitor.width - 48.) / WORKBENCH.width)
                                    .min((monitor.height - 96.) / WORKBENCH.height)
                                    .clamp(WORKBENCH_MIN.width / WORKBENCH.width, 1.);
                                let size =
                                    Size::new(WORKBENCH.width * scale, WORKBENCH.height * scale);
                                window::position(id).then(move |position| {
                                    let resize = window::set_resizable(id, true)
                                        .chain(window::set_min_size(id, Some(WORKBENCH_MIN)))
                                        .chain(window::resize(id, size));
                                    let resize = if let Some(position) = position {
                                        let growth = iced::Vector::new(
                                            (size.width - SETUP_SIZE.width) / 2.,
                                            (size.height - SETUP_SIZE.height) / 2.,
                                        );
                                        resize.chain(window::move_to(id, position - growth))
                                    } else {
                                        resize
                                    };
                                    resize
                                        .chain(
                                            platform::lock_aspect(id, WORKBENCH_ASPECT).discard(),
                                        )
                                        .chain(platform::content_size(id).map(Message::Viewport))
                                })
                            })
                        }));
                    }
                    if packaged {
                        tasks.push(updates::check().map(Message::Update));
                    }
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
                Err(error) => {
                    self.error = error;
                    self.phase.clear();
                }
            },
            Message::Update(message) => match message {
                updates::Message::Checked(Ok(offer)) => self.updates.offer = offer,
                updates::Message::Checked(Err(error)) => eprintln!("Client update: {error}"),
                updates::Message::Install => return self.updates.install().map(Message::Update),
                updates::Message::Progress(progress) => self.updates.progress = Some(progress),
                updates::Message::Finished(Ok(())) => {
                    return self.update(Message::Quit);
                }
                updates::Message::Finished(Err(error)) => {
                    self.updates.progress = None;
                    self.updates.error = error;
                }
            },
            Message::Uninstall if self.after_stop.is_none() && self.updates.progress.is_none() => {
                return Task::perform(uninstall::prepare(self.locale), Message::UninstallPrepared);
            }
            Message::UninstallPrepared(result)
                if self.after_stop.is_none() && self.updates.progress.is_none() =>
            {
                match result {
                    Ok(Some(plan)) => {
                        self.uninstall = Some(plan);
                        return self.update(Message::Quit);
                    }
                    Ok(None) => {}
                    Err(error) => self.error = error,
                }
            }
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
                    self.frame_errors[index].clear();
                    if resized {
                        self.rearrange();
                    }
                }
                Err(error) => self.frame_errors[index] = error,
            },
            Message::PollAgentPointers if !self.pointer_polling && self.after_stop.is_none() => {
                if let Some(client) = self.connection.clone() {
                    self.pointer_polling = true;
                    let [phone, desktop] = self.devices.clone();
                    return Task::perform(
                        async move {
                            let (phone, desktop) =
                                tokio::join!(client.cursor(phone), client.cursor(desktop));
                            Box::new([phone, desktop])
                        },
                        Message::AgentPointers,
                    );
                }
            }
            Message::AgentPointers(cursors) => {
                self.pointer_polling = false;
                if self.after_stop.is_none() {
                    self.agent_traces = std::array::from_fn(|i| {
                        scene::AgentTrace::from_cursor(
                            cursors[i].as_ref()?,
                            self.devices[i].as_ref()?,
                        )
                    });
                }
            }
            Message::Chat(result) => {
                if let Some(client) = &self.connection {
                    let state = match result {
                        Ok(state) => {
                            self.chat_connection_error.clear();
                            state
                        }
                        Err(error) => {
                            self.chat_connection_error = error;
                            return Task::none();
                        }
                    };
                    let todos = if self.popup && self.panel == Panel::Todos {
                        self.todos
                            .update(todos::Message::Load, client)
                            .map(Message::Todos)
                    } else {
                        Task::none()
                    };
                    let was_active = self.agent_pointer_active();
                    let task = self
                        .chat
                        .update(chat::Message::History(Ok(state)), client, "")
                        .map(Message::Conversation);
                    let speech = self
                        .modes
                        .observe_chat(&self.chat.state)
                        .map(Message::Modes);
                    let active = self.agent_pointer_active();
                    let sample = if was_active != active {
                        self.update(Message::PollAgentPointers)
                    } else {
                        Task::none()
                    };
                    let link = if was_active && !active {
                        self.update(Message::RefreshLink)
                    } else {
                        Task::none()
                    };
                    return Task::batch([task, speech, sample, link, todos]);
                }
            }
            Message::Conversation(chat::Message::Todos(click)) => {
                return self.update(Message::OpenPanel(Panel::Todos, click));
            }
            Message::Todos(message) => {
                if let Some(client) = &self.connection {
                    let before = self.todos.rows_height();
                    let task = self.todos.update(message, client).map(Message::Todos);
                    if self.popup
                        && self.panel == Panel::Todos
                        && before != self.todos.rows_height()
                    {
                        self.reflow_popup();
                    }
                    return task;
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
                    self.keyboard_target = None;
                }
                if let Some(client) = &self.connection {
                    let preview_changed = matches!(
                        message,
                        chat::Message::Attachment(_)
                            | chat::Message::Preview(..)
                            | chat::Message::ClosePreview
                    );
                    let device_id = self.devices[self.focus]
                        .as_ref()
                        .map(|d| d.id.as_str())
                        .unwrap_or("");
                    let task = self
                        .chat
                        .update(message, client, device_id)
                        .map(Message::Conversation);
                    if preview_changed {
                        self.reflow_child();
                    }
                    return task;
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
                if matches!(message, audio::Message::Toggle | audio::Message::Volume(_)) {
                    self.keyboard_target = None;
                }
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
                    let task = self
                        .knowledge
                        .update(message, client)
                        .map(Message::Knowledge);
                    self.reflow_child();
                    return task;
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
                if let Some(capture) = self.capture.clone() {
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
            Message::PanelSize(size) => {
                let header = style::window_insets(WORKBENCH.width).2;
                let height = (size.height + header).ceil();
                if self.panel_content_height != height {
                    self.panel_content_height = height;
                    self.reflow_popup();
                }
            }
            Message::Agents(agents::Message::Measured(size)) if self.agents.adding() => {
                let height = size.height.ceil().min(505.);
                if self.agent_add_height != height {
                    self.agent_add_height = height;
                    self.reflow_child();
                }
            }
            Message::HostAccess(enabled) => {
                if let Err(error) = self.chat.host.configure(&self.host_home, enabled) {
                    self.error = error.to_string();
                }
            }
            Message::OpenDevices => {
                self.show_devices = true;
                self.reflow_child();
                if let Some(device) = &self.devices[self.focus] {
                    return self.update(Message::Devices(devices::Message::Choose(
                        device.id.clone(),
                    )));
                }
            }
            Message::CloseDevices => {
                self.show_devices = false;
                self.reflow_child();
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
                    let reload = changed.then(|| client.clone());
                    self.reflow_child();
                    if self.panel_target() != before {
                        self.reflow_popup();
                    }
                    if let Some(client) = reload {
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
                    if !self.linked {
                        self.rearrange();
                    }
                }
            }
            Message::Input(1, input) => return self.desktop_input(input),
            Message::Input(index, input) => return self.input(index, input),
            Message::DesktopClickElapsed(at) => {
                if matches!(&self.desktop_click, Some((current, _)) if *current == at)
                    && let Some((_, input)) = self.desktop_click.take()
                {
                    return self.input(1, input);
                }
            }
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
                    let smooth = if t < 0.5 {
                        4. * t.powi(3)
                    } else {
                        1. - 4. * (1. - t).powi(3)
                    };
                    self.layout = self.from.blend(self.target, smooth);
                    if self.target.cable > self.from.cable {
                        let draw = style::bezier(t, [0.22, 0.8, 0.2, 1.]);
                        self.layout.cable = self.from.cable + (1. - self.from.cable) * draw;
                        self.layout.cable_end =
                            self.from.cable_end + (1. - self.from.cable_end) * draw;
                    } else if self.target.cable < self.from.cable {
                        let fade = style::bezier(
                            (t * style::MOTION_DEVICE / 0.22).min(1.),
                            [0., 0., 0.58, 1.],
                        );
                        self.layout.cable = self.from.cable * (1. - fade);
                        self.layout.cable_end = self.from.cable_end;
                    }
                    if t >= 1. {
                        self.layout = self.target;
                        self.transition = None;
                    }
                }
                if let Some(start) = self.popup_transition {
                    let target = f32::from(self.popup);
                    let (blend, finished) =
                        surface::window_progress(now.saturating_duration_since(start), self.popup);
                    self.popup_amount = self.from_popup + (target - self.from_popup) * blend;
                    let to = self.panel_target();
                    self.panel_size = Size::new(
                        self.from_panel.width + (to.width - self.from_panel.width) * blend,
                        self.from_panel.height + (to.height - self.from_panel.height) * blend,
                    );
                    if finished {
                        self.popup_transition = None;
                    }
                }
                self.animate_child(now);
            }
            Message::Event(Event::Window(window::Event::Opened { .. })) => {
                #[cfg(target_os = "macos")]
                return platform::menu();
            }
            Message::FitViewport(id) if self.connection.is_some() => {
                return platform::content_size(id).map(Message::Viewport);
            }
            Message::Viewport(size) if size.width > 0. && size.height > 0. => {
                self.ui_scale = (size.width / WORKBENCH.width).min(size.height / WORKBENCH.height);
                self.canvas_origin = iced::Vector::new(
                    ((size.width / self.ui_scale - WORKBENCH.width) * 0.5).max(0.),
                    ((size.height / self.ui_scale - WORKBENCH.height) * 0.5).max(0.),
                );
            }
            Message::Event(Event::Window(window::Event::Focused)) => {
                self.reduced_motion = platform::reduced_motion();
            }
            Message::Quit | Message::Event(Event::Window(window::Event::CloseRequested)) => {
                self.chat.host.close();
                let stopping = self.after_stop.replace(AfterStop::Exit).is_some();
                self.connection = None;
                self.streams = None;
                self.desktop_click = None;
                self.agent_traces = [None, None];
                for audio in &mut self.audio {
                    audio.stop();
                }
                self.modes.stop_capture();
                self.error.clear();
                self.phase = "正在关闭设备".into();
                if self.host_home.as_os_str().is_empty() {
                    return self.finish_exit();
                }
                let hide =
                    window::latest().and_then(|id| window::set_mode(id, window::Mode::Hidden));
                if stopping || self.stop.take().is_some_and(|stop| stop.send(()).is_ok()) {
                    return hide;
                }
                return Task::batch([
                    hide,
                    Task::perform(host::Host::stop(self.host_home.clone()), |r| {
                        Message::Host(host::Event::Stopped(r))
                    }),
                ]);
            }
            Message::Event(Event::Keyboard(keyboard::Event::KeyPressed {
                key:
                    keyboard::Key::Named(
                        key @ (keyboard::key::Named::ArrowUp | keyboard::key::Named::ArrowDown),
                    ),
                ..
            })) if self.popup && self.panel == Panel::Todos => {
                return self.update(Message::Todos(todos::Message::Step(
                    key == keyboard::key::Named::ArrowDown,
                )));
            }
            Message::Event(Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Tab),
                modifiers,
                ..
            })) if self.keyboard_target.is_none() => {
                return if modifiers.shift() {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
            Message::Event(Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            })) => {
                if self.chat.preview.is_some() {
                    return self.update(Message::Conversation(chat::Message::ClosePreview));
                }
                if self.popup
                    && self.panel == Panel::Knowledge
                    && let Some(message) = self.knowledge.escape()
                {
                    return self.update(Message::Knowledge(message));
                }
                if self.popup && self.panel == Panel::Agent && self.agents.adding() {
                    return self.update(Message::Agents(agents::Message::Back));
                }
                if self.popup && self.panel == Panel::Agent {
                    if self.show_devices {
                        return self.update(Message::CloseDevices);
                    }
                    if self.agents.is_editing() {
                        return self.update(Message::Agents(agents::Message::Back));
                    }
                }
                self.set_popup(false);
                self.pinned = false;
                self.keyboard_target = None;
            }
            Message::Event(Event::Mouse(iced::mouse::Event::CursorMoved { position })) => {
                let position = position - self.canvas_origin;
                self.cursor = position;
                if self.popup
                    && !self.pinned
                    && !self.panel_leave_pending
                    && !self.within_panel_corridor(position)
                {
                    self.panel_leave_pending = true;
                    return Task::perform(
                        tokio::time::sleep(std::time::Duration::from_millis(280)),
                        |()| Message::PanelLeft,
                    );
                }
            }
            Message::PanelLeft => {
                self.panel_leave_pending = false;
                if self.popup && !self.pinned && !self.within_panel_corridor(self.cursor) {
                    self.set_popup(false);
                }
            }
            Message::ClosePanel => {
                self.pinned = false;
                self.set_popup(false);
            }
            Message::Event(Event::Mouse(iced::mouse::Event::ButtonPressed(_))) => {
                if self.popup
                    && !self.popup_rect().contains(self.cursor)
                    && !self.child_rect().contains(self.cursor)
                    && !self.panel_anchor(self.panel).contains(self.cursor)
                {
                    self.set_popup(false);
                    self.pinned = false;
                }
            }
            Message::OpenPanel(panel, false) => {
                if panel != self.panel || !self.popup {
                    return Task::perform(
                        tokio::time::sleep(std::time::Duration::from_millis(240)),
                        move |()| Message::PanelHovered(panel),
                    );
                }
            }
            Message::OpenPanel(panel, true) | Message::PanelHovered(panel) => {
                let click = matches!(message, Message::OpenPanel(_, true));
                if !click && !self.panel_anchor(panel).contains(self.cursor) {
                    return Task::none();
                }
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
                if self.panel == Panel::Agent && panel != Panel::Agent && self.agents.adding() {
                    let _ = self.update(Message::Agents(agents::Message::Back));
                }
                if self.panel != panel {
                    self.show_devices = false;
                    self.todos.cancel_drag();
                    self.panel = panel;
                    self.popup_amount = 0.;
                    self.panel_size = self.panel_target();
                }
                if let Panel::Capture(index) = panel {
                    self.capture_offset = self.cursor.x - self.layout.toolbars[index].x;
                }
                self.pinned = click;
                self.popup = true;
                self.reflow_popup();
                self.reflow_child();
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
                Ok(state) => self.accept_link(state),
                Err(error) => self.error = error,
            },
            Message::RefreshLink => {
                if let Some(client) = self.connection.clone() {
                    return Task::perform(
                        async move { client.rpc("desktop/state", wire::RegistryRequest {}).await },
                        Message::LinkSnapshot,
                    );
                }
            }
            Message::LinkSnapshot(Ok(state)) => self.accept_link(state),
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
                if self.panel == Panel::Todos =>
            {
                return self.update(Message::Todos(todos::Message::Cancel));
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
    fn agent_pointer_active(&self) -> bool {
        let chat = &self.chat.state;
        chat.run_status == "running"
            || (chat.run_status == "queued"
                && chat.next_wake_at_ms <= connection::epoch_ms() as i64)
    }
    fn subscription(&self) -> Subscription<Message> {
        if self.after_stop == Some(AfterStop::Exit) {
            return Subscription::none();
        }
        let mut subscriptions = vec![iced::event::listen_with(|event, status, id| match &event {
            Event::Window(window::Event::Resized(_)) => Some(Message::FitViewport(id)),
            Event::Window(
                window::Event::Opened { .. }
                | window::Event::Focused
                | window::Event::CloseRequested,
            )
            | Event::Window(window::Event::FileDropped(_))
            | Event::Mouse(iced::mouse::Event::CursorMoved { .. })
            | Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => Some(Message::Event(event)),
            Event::Keyboard(keyboard::Event::KeyPressed {
                key:
                    keyboard::Key::Named(
                        keyboard::key::Named::Escape
                        | keyboard::key::Named::Tab
                        | keyboard::key::Named::ArrowUp
                        | keyboard::key::Named::ArrowDown,
                    ),
                ..
            }) if status == iced::event::Status::Ignored => Some(Message::Event(event)),
            Event::Mouse(iced::mouse::Event::ButtonReleased(_))
                if status == iced::event::Status::Ignored =>
            {
                Some(Message::Event(event))
            }
            _ => None,
        })];
        if self.transition.is_some()
            || self.popup_transition.is_some()
            || self
                .child
                .as_ref()
                .is_some_and(|child| child.animation.is_some())
            || self.input_started.is_some()
            || (!self.reduced_motion
                && (self.modes.animating()
                    || self
                        .setup
                        .as_ref()
                        .is_some_and(deployment::Setup::animating_language)))
        {
            subscriptions.push(window::frames().map(Message::Tick));
        }
        subscriptions.push(self.chat.subscription().map(Message::Conversation));
        if self
            .setup
            .as_ref()
            .is_some_and(|setup| !setup.attempted && setup.locale.is_some())
        {
            subscriptions.push(
                iced::time::every(std::time::Duration::from_secs(3))
                    .map(|_| Message::Setup(deployment::Message::RefreshCapacity)),
            );
        }
        if self.after_stop.is_none() && self.connection.is_some() && self.agent_pointer_active() {
            subscriptions.push(
                iced::time::every(std::time::Duration::from_millis(100))
                    .map(|_| Message::PollAgentPointers),
            );
            subscriptions.push(
                iced::time::every(std::time::Duration::from_secs(4)).map(|_| Message::RefreshLink),
            );
        }
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
        if self.popup_amount > 0. {
            let anchor = self.panel_anchor(self.panel);
            let panel = self.panel_bounds(self.panel_size);
            surface::tool_motion(anchor, panel, self.popup_amount)
        } else {
            Rectangle::default()
        }
    }
    fn panel_target(&self) -> Size {
        let (width, max_height, fit) = match self.panel {
            Panel::Todos => {
                let padding = surface::tool_padding(WORKBENCH.width);
                let height = style::window_insets(WORKBENCH.width).2
                    + padding.top
                    + self.todos.rows_height()
                    + padding.bottom;
                (400., height.min(430.), false)
            }
            Panel::Agent => (680., 600., false),
            Panel::Language => (280., 480., false),
            Panel::Skills => (650., 560., true),
            Panel::Knowledge => (850., 590., false),
            Panel::Files => (1120., 760., false),
            Panel::Location => (
                720.,
                if self.location.enabled() { 750. } else { 248. },
                true,
            ),
            Panel::Proxy => (420., 480., true),
            Panel::Goal => (650., 364., true),
            Panel::Update => (360., 400., true),
            Panel::Capture(0) => (360., 580., true),
            Panel::Capture(_) => (700., 580., true),
        };
        let height = if fit {
            self.panel_content_height.min(max_height)
        } else {
            max_height
        };
        self.panel_bounds(Size::new(width, height)).size()
    }
    fn panel_bounds(&self, size: Size) -> Rectangle {
        let anchor = self.panel_anchor(self.panel);
        if self.panel != Panel::Todos {
            return surface::tool_rect(WORKBENCH, anchor, size, false);
        }
        let height = size.height.min(anchor.y - 26.);
        Rectangle::new(
            iced::Point::new(
                anchor
                    .x
                    .min(WORKBENCH.width - size.width - layout::SIDE_INSET),
                anchor.y - height - 10.,
            ),
            Size::new(size.width, height),
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
    fn panel_anchor(&self, panel: Panel) -> Rectangle {
        if panel == Panel::Todos {
            let chat = self.layout.chat;
            let [vertical, horizontal] = layout::CHAT_PADDING;
            let size = style::CONTROL_HEIGHT;
            return Rectangle::new(
                iced::Point::new(
                    chat.x + horizontal + 12.,
                    chat.y + chat.height - vertical - 12. - size,
                ),
                Size::new(size, size),
            );
        }
        if let Panel::Capture(index) = panel {
            let toolbar = self.layout.toolbars[index];
            return Rectangle::new(
                iced::Point::new(
                    toolbar.x + self.capture_offset - 16.,
                    toolbar.center_y() - 17.,
                ),
                Size::new(32., 34.),
            );
        }
        let [top, gap, height] = style::NAVIGATION;
        let inset = layout::SIDE_INSET;
        let trailing = Self::NAV_PANELS
            .iter()
            .position(|&entry| entry == panel)
            .map_or(0, |index| {
                Self::NAV_PANELS.len() - index - 1 + usize::from(self.updates.offer.is_some())
            });
        let width = style::NAV_ICON_WIDTH;
        Rectangle::new(
            iced::Point::new(
                WORKBENCH.width
                    - inset
                    - 2. * width
                    - gap
                    - style::NAV_SEPARATOR
                    - trailing as f32 * (width + style::NAV_TOOL_GAP),
                top,
            ),
            Size::new(width, height),
        )
    }
    fn within_panel_corridor(&self, position: iced::Point) -> bool {
        let anchor = self.panel_anchor(self.panel);
        let panel = self.popup_rect();
        let left = anchor.x.min(panel.x);
        let right = (anchor.x + anchor.width).max(panel.x + panel.width);
        let top = anchor.y.min(panel.y);
        let bottom = (anchor.y + anchor.height).max(panel.y + panel.height);
        self.child_rect().contains(position)
            || Rectangle::new(
                iced::Point::new(left, top),
                Size::new(right - left, bottom - top),
            )
            .contains(position)
    }
    fn load_panel(&mut self) -> Task<Message> {
        let message = match self.panel {
            Panel::Todos => Message::Todos(todos::Message::Load),
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
                                width: frame.width as f32,
                                height: frame.height as f32,
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
        self.child.as_ref().map_or(Rectangle::default(), |child| {
            if child.previous.is_some() {
                child.bounds
            } else {
                child.rect
            }
        })
    }
    fn child_target(&self) -> Option<(ChildPane, Rectangle, Rectangle)> {
        let button = |point: iced::Point, size: f32| {
            Rectangle::new(
                point - iced::Vector::new(size / 2., size / 2.),
                Size::new(size, size),
            )
        };
        if let Some(file) = &self.chat.preview {
            let pane = ChildPane::Preview {
                id: file.id.clone(),
                name: file.name.clone(),
            };
            let anchor = self
                .child
                .as_ref()
                .filter(|child| child.pane == pane)
                .map_or_else(|| button(self.cursor, 32.), |child| child.anchor);
            return Some((pane, anchor, self.chat.preview_bounds(&file.id, WORKBENCH)));
        }
        if !self.popup {
            return None;
        }
        let parent = surface::tool_rect(
            WORKBENCH,
            self.panel_anchor(self.panel),
            self.panel_target(),
            false,
        );
        let (_, trailing, header) = style::window_insets(WORKBENCH.width);
        match self.panel {
            Panel::Knowledge if self.knowledge.document_open() => {
                let anchor = self.knowledge.document_anchor().map_or_else(
                    || {
                        Rectangle::new(
                            iced::Point::new(
                                parent.x + parent.width - trailing - 66.,
                                parent.y + (header - 32.) / 2.,
                            ),
                            Size::new(32., 32.),
                        )
                    },
                    |point| button(point, 24.),
                );
                let rect = surface::tool_rect(WORKBENCH, anchor, Size::new(620., 600.), true);
                Some((ChildPane::Knowledge(self.knowledge.editing()), anchor, rect))
            }
            Panel::Agent if self.show_devices => {
                let origin = if parent.contains(self.cursor) {
                    self.cursor
                } else {
                    iced::Point::new(parent.x + 42., parent.y + parent.height - 45.)
                };
                Some((ChildPane::Devices, button(origin, 44.), parent))
            }
            Panel::Agent if self.agents.adding() => {
                let anchor = Rectangle::new(
                    iced::Point::new(
                        parent.x + parent.width - trailing - 32.,
                        parent.y + (header - 32.) / 2.,
                    ),
                    Size::new(32., 32.),
                );
                let rect = surface::tool_rect(
                    WORKBENCH,
                    anchor,
                    Size::new(480., self.agent_add_height),
                    false,
                );
                Some((
                    ChildPane::Agent(String::new(), agents::Tab::Model),
                    anchor,
                    rect,
                ))
            }
            Panel::Agent => self.agents.editor().map(|(id, tab)| {
                let origin = if parent.contains(self.cursor) {
                    self.cursor
                } else {
                    iced::Point::new(parent.x + parent.width - trailing - 66., parent.y + header)
                };
                (
                    ChildPane::Agent(id.to_owned(), tab),
                    button(origin, 44.),
                    parent,
                )
            }),
            _ => None,
        }
    }
    fn reflow_child(&mut self) {
        let Some((pane, anchor, bounds)) = self.child_target() else {
            if self.reduced_motion {
                if let Some(child) = self.child.take() {
                    self.finish_child_close(child.pane);
                }
            } else if let Some(child) = &mut self.child
                && child.open
            {
                child.open = false;
                if child.previous.take().is_some() {
                    child.rect = child.bounds;
                    child.opacity = 1.;
                }
                child.animation = Some((Instant::now(), child.rect, child.opacity));
            }
            return;
        };
        if let Some(child) = &mut self.child
            && child.pane == pane
        {
            if child.open && child.bounds == bounds && (child.anchor == anchor || !pane.separate())
            {
                return;
            }
            child.open = true;
            if pane.separate() {
                child.anchor = anchor;
            }
            child.bounds = bounds;
            if self.reduced_motion {
                child.rect = bounds;
                child.opacity = 1.;
                child.animation = None;
            } else {
                child.animation = Some((Instant::now(), child.rect, child.opacity));
            }
        } else {
            let previous = self.child.as_ref().filter(|current| {
                !self.reduced_motion
                    && current.open
                    && current.bounds == bounds
                    && current.pane.same_window(&pane)
            });
            let transition_anchor =
                previous
                    .filter(|_| bounds.contains(self.cursor))
                    .map_or(anchor, |_| {
                        Rectangle::new(
                            self.cursor - iced::Vector::new(16., 16.),
                            Size::new(32., 32.),
                        )
                    });
            let anchor = previous.map_or(anchor, |child| child.anchor);
            let previous = previous.map(|child| child.pane.clone());
            let rect = if self.reduced_motion {
                bounds
            } else {
                surface::tool_motion(transition_anchor, bounds, 0.)
            };
            let opacity = f32::from(self.reduced_motion);
            self.child = Some(ChildWindow {
                pane,
                previous,
                anchor,
                bounds,
                rect,
                opacity,
                animation: (!self.reduced_motion).then(|| (Instant::now(), rect, opacity)),
                open: true,
            });
        }
    }
    fn animate_child(&mut self, now: Instant) {
        if let Some(child) = &mut self.child
            && let Some((started, from, opacity)) = child.animation
        {
            let (blend, finished) =
                surface::window_progress(now.saturating_duration_since(started), child.open);
            let to = if child.open {
                child.bounds
            } else {
                surface::tool_motion(child.anchor, child.bounds, 0.)
            };
            child.rect = Rectangle::new(
                from.position() + (to.position() - from.position()) * blend,
                Size::new(
                    from.width + (to.width - from.width) * blend,
                    from.height + (to.height - from.height) * blend,
                ),
            );
            child.opacity = (opacity + (f32::from(child.open) - opacity) * blend).clamp(0., 1.);
            if finished {
                child.animation = None;
                child.previous = None;
                if !child.open
                    && let Some(child) = self.child.take()
                {
                    self.finish_child_close(child.pane);
                }
            }
        }
    }
    fn finish_child_close(&mut self, pane: ChildPane) {
        match pane {
            ChildPane::Agent(..) => self.agents.prune_editors(false),
            ChildPane::Knowledge(_) => self.knowledge.finish_document_close(),
            _ => {}
        }
    }
    fn child_view<'a>(&'a self, pane: &'a ChildPane, bounds: Rectangle) -> Element<'a, Message> {
        let locale = self.locale;
        match pane {
            ChildPane::Agent(id, tab) => self
                .agents
                .view(
                    locale,
                    Some((id, *tab)),
                    WORKBENCH.width,
                    self.reduced_motion,
                )
                .map(Message::Agents),
            ChildPane::Knowledge(editing) => self
                .knowledge
                .document_view(locale, WORKBENCH.width, *editing)
                .map(Message::Knowledge),
            ChildPane::Preview { id, name } => self
                .chat
                .preview_view(id, name, bounds)
                .map(Message::Conversation),
            ChildPane::Devices => scrollable(
                container(
                    column![
                        row![
                            controls::icon_button(
                                controls::Icon::Back,
                                Some(Message::CloseDevices)
                            ),
                            text(locale.tr("设备")).size(14)
                        ]
                        .align_y(iced::Center),
                        self.device_admin
                            .view(&self.agents.registry, locale)
                            .map(Message::Devices)
                    ]
                    .spacing(12),
                )
                .padding(surface::tool_padding(WORKBENCH.width)),
            )
            .direction(surface::tool_scroll())
            .height(Length::Fill)
            .into(),
        }
    }
    fn panel_body(&self) -> Element<'_, Message> {
        let locale = self.locale;
        let width = WORKBENCH.width;
        let padding = surface::tool_padding(width);
        if self.panel == Panel::Agent {
            let action = |label, message| {
                button(text(locale.tr(label)).size(13))
                    .height(style::REGISTRY_CONTROL)
                    .style(style::quiet)
                    .on_press(message)
            };
            let settings = row![
                action("设备", Message::OpenDevices),
                iced::widget::toggler(self.chat.host.enabled())
                    .label(locale.tr("允许本机协作"))
                    .text_size(13)
                    .on_toggle(Message::HostAccess),
                space::horizontal(),
                action("卸载 AImanity", Message::Uninstall)
            ]
            .spacing(14)
            .align_y(iced::Center);
            return column![
                self.agents
                    .view(locale, None, width, self.reduced_motion)
                    .map(Message::Agents),
                container(column![iced::widget::rule::horizontal(1), settings].spacing(13))
                    .padding(padding.top(0))
                    .width(Length::Fill)
            ]
            .into();
        }
        let title = match self.panel {
            Panel::Todos => locale.tr("待办"),
            Panel::Language => "Language",
            Panel::Skills => locale.tr("Skill 管理"),
            Panel::Knowledge => locale.tr("知识库"),
            Panel::Files => locale.tr("文件管理"),
            Panel::Location => locale.tr("模拟位置"),
            Panel::Proxy => locale.tr("工作台代理"),
            Panel::Goal => "Task & Goal",
            Panel::Update => locale.tr("系统更新"),
            Panel::Capture(_) => locale.tr("截图"),
            Panel::Agent => unreachable!(),
        };
        let actions = row![].align_y(iced::Center).spacing(2);
        let actions = match self.panel {
            Panel::Skills => actions.push(self.skills.actions().map(Message::Skills)),
            Panel::Knowledge => actions.push(self.knowledge.new_action().map(Message::Knowledge)),
            Panel::Location if self.location.enabled() => {
                actions.push(self.location.enable_action().map(Message::Location))
            }
            _ => actions,
        };
        let header = surface::tool_header(
            title,
            width,
            actions.push(surface::tool_action(
                controls::Icon::Close,
                Some(Message::ClosePanel),
            )),
        );
        let body_height = self.panel_target().height
            - style::window_insets(width).2
            - padding.top
            - padding.bottom;
        let language_row_inset = 12.;
        let language_border_inset = 1.;
        let content = match self.panel {
            Panel::Agent => unreachable!(),
            Panel::Skills => self.skills.view(locale).map(Message::Skills),
            Panel::Knowledge => self
                .knowledge
                .view(locale, self.reduced_motion)
                .map(Message::Knowledge),
            Panel::Todos => self.todos.view(locale).map(Message::Todos),
            Panel::Files => self
                .files
                .view(body_height, locale)
                .map(|m| Message::Files(Box::new(m))),
            Panel::Location => self.location.view(locale).map(Message::Location),
            Panel::Proxy => self.proxy.view(locale).map(Message::Proxy),
            Panel::Goal => self.modes.goal_view().map(Message::Modes),
            Panel::Update => self
                .updates
                .view(locale, self.reduced_motion)
                .map(Message::Update),
            Panel::Language => column(locale::Locale::all().map(|l| {
                let rtl = l.code() == "ar";
                let glyph = container(
                    text(l.glyph())
                        .size(13)
                        .line_height(iced::Pixels(13.))
                        .font(iced::Font {
                            weight: iced::font::Weight::Semibold,
                            ..l.font()
                        }),
                )
                .width(24)
                .align_x(if rtl { iced::Right } else { iced::Left });
                let name = text(l.name())
                    .size(14)
                    .line_height(iced::Pixels(22.))
                    .font(l.font());
                let check = controls::Icon::Check
                    .view()
                    .width(16)
                    .height(16)
                    .opacity(if locale == l { 1. } else { 0. });
                let option = if rtl {
                    row![check, space::horizontal(), name, glyph]
                } else {
                    row![glyph, name, space::horizontal(), check]
                };
                button(option.height(42).spacing(12).align_y(iced::Center))
                    .on_press(Message::SetLocale(l))
                    .padding([0., language_row_inset])
                    .width(Length::Fill)
                    .style(move |theme, status| {
                        let mut appearance = style::quiet(theme, status);
                        if locale == l
                            || matches!(
                                status,
                                iced::widget::button::Status::Hovered
                                    | iced::widget::button::Status::Pressed
                            )
                        {
                            appearance.background = Some(style::ACCENT.into());
                        }
                        appearance
                    })
                    .into()
            }))
            .spacing(3)
            .into(),
            Panel::Capture(_) => {
                let mut body = column![].spacing(16);
                let inset = style::window_insets(width).0;
                if let Some(capture) = &self.capture {
                    let side = self.panel_target().width - 2. * inset;
                    let height = (side * capture.height / capture.width)
                        .min(420.)
                        .min((WORKBENCH.height - 230.).max(1.));
                    body = body
                        .push(
                            image(capture.image.clone())
                                .width(Length::Fill)
                                .height(height)
                                .content_fit(iced::ContentFit::Contain),
                        )
                        .push(
                            row![
                                space::horizontal(),
                                button(text(locale.tr("保存图片")).size(14))
                                    .padding([10, 0])
                                    .on_press(Message::SaveCapture)
                                    .style(style::quiet),
                                button(
                                    container(text(locale.tr("附到对话")).size(14))
                                        .center_y(Length::Fill)
                                )
                                .height(style::CONTROL_HEIGHT)
                                .padding([0, 16])
                                .on_press(Message::AttachCapture)
                                .style(|theme, status| {
                                    let mut appearance = style::quiet(theme, status);
                                    let fill = if matches!(
                                        status,
                                        iced::widget::button::Status::Hovered
                                            | iced::widget::button::Status::Pressed
                                    ) {
                                        style::ACCENT
                                    } else {
                                        iced::Color::from_rgb8(0xf2, 0xf2, 0xf2)
                                    };
                                    appearance.background = Some(fill.into());
                                    appearance
                                })
                            ]
                            .spacing(18)
                            .align_y(iced::Center),
                        );
                } else {
                    body = body.push(
                        container(crate::loading::orb(20., self.reduced_motion))
                            .center_x(Length::Fill),
                    );
                }
                body.into()
            }
        };
        let padded = container(content).padding(padding).width(Length::Fill);
        let body: Element<'_, Message> = match self.panel {
            Panel::Knowledge => padded.height(Length::Fill).into(),
            Panel::Language => container(
                scrollable(
                    padded.padding(
                        padding
                            .left(padding.left - language_row_inset - language_border_inset)
                            .right(padding.right - language_row_inset - language_border_inset),
                    ),
                )
                .direction(surface::tool_scroll())
                .style(|theme, status| {
                    let mut appearance = scrollable::default(theme, status);
                    let rail_engaged = matches!(
                        status,
                        scrollable::Status::Hovered {
                            is_vertical_scrollbar_hovered: true,
                            ..
                        } | scrollable::Status::Dragged {
                            is_vertical_scrollbar_dragged: true,
                            ..
                        }
                    );
                    appearance.vertical_rail.background = None;
                    appearance.vertical_rail.scroller.background = theme
                        .palette()
                        .text
                        .scale_alpha(if rail_engaged { 0.32 } else { 0. })
                        .into();
                    appearance
                })
                .height(Length::Fill),
            )
            .padding(language_border_inset)
            .height(Length::Fill)
            .into(),
            Panel::Files | Panel::Todos => scrollable(padded)
                .direction(surface::tool_scroll())
                .height(Length::Fill)
                .into(),
            _ => scrollable(sensor(padded).on_resize(Message::PanelSize))
                .direction(surface::tool_scroll())
                .height(Length::Fill)
                .into(),
        };
        column![header, body].into()
    }
    fn view(&self) -> Element<'_, Message> {
        if let Some(setup) = &self.setup {
            return deployment::glass_background(if setup.attempted || !self.phase.is_empty() {
                let can_stop = !setup.installed && self.stop.is_some() && self.after_stop.is_none();
                setup
                    .progress(
                        self.locale,
                        &self.phase,
                        &self.error,
                        can_stop,
                        self.reduced_motion,
                    )
                    .map(Message::Setup)
            } else {
                setup
                    .view(&self.error, self.reduced_motion)
                    .map(Message::Setup)
            });
        }
        if self.connection.is_none() && !self.phase.is_empty() {
            return deployment::glass_background(
                container(
                    column![
                        loading::orb(180., self.reduced_motion),
                        text(self.locale.tr(&self.phase)).size(14)
                    ]
                    .spacing(16)
                    .align_x(iced::Center),
                )
                .center(Length::Fill)
                .into(),
            );
        }
        use controls::{Icon, sized_icon_button};
        let popup = self.popup_rect();
        let child_popup = self.child_rect();
        let (mode_effects, mode_motion, microphone) = self.modes.effects(self.reduced_motion);
        let scene = scene::Scene {
            layout: self.layout,
            size: WORKBENCH,
            frames: self.frames.clone(),
            focus: self.focus,
            linked: self.linked,
            popup,
            popup_opacity: self.popup_amount.clamp(0., 1.),
            child_popup,
            child_popup_opacity: self
                .child
                .as_ref()
                .filter(|child| child.pane.separate())
                .map_or(0., |child| {
                    if child.previous.is_some() {
                        1.
                    } else {
                        child.opacity
                    }
                }),
            mode_effects,
            mode_motion,
            microphone,
            keyboard_target: self.keyboard_target,
            agent_pointers: std::array::from_fn(|i| {
                self.agent_traces[i]
                    .as_ref()
                    .zip(self.frames[i].as_deref())
                    .map_or([[0.; 4]; 5], |(trace, frame)| trace.markers(frame))
            }),
        };
        let mut layers = vec![surface::Surface::new(scene).into()];
        let error_message = [
            self.error.as_str(),
            self.modes.voice_error(),
            self.modes.proactive_error(),
            self.audio[0].error(),
            self.audio[1].error(),
            self.frame_errors[self.focus].as_str(),
            self.chat.error.as_str(),
            self.chat_connection_error.as_str(),
        ]
        .into_iter()
        .find(|error| !error.is_empty())
        .unwrap_or("");
        let error = if error_message.is_empty() {
            text(self.locale.tr(&self.phase)).size(12)
        } else {
            text(self.locale.tr(error_message))
                .size(12)
                .color(style::ERROR)
                .wrapping(iced::widget::text::Wrapping::None)
        };
        let [nav_top, nav_gap, nav_height] = style::NAVIGATION;
        let inset = layout::SIDE_INSET;
        let linked = self.linked;
        let cable = Icon::Cable
            .view()
            .style(move |_, _| iced::widget::svg::Style {
                color: Some(if linked {
                    iced::Color::WHITE
                } else {
                    style::MUTED
                }),
            });
        let mut shared = row![
            button(container(cable).center(Length::Fill))
                .width(style::NAV_ICON_WIDTH)
                .height(nav_height)
                .padding(0)
                .on_press_maybe(self.connection.as_ref().map(|_| Message::Link))
                .style(if linked { style::primary } else { style::quiet })
        ];
        for panel in Self::NAV_PANELS {
            let (icon, active) = match panel {
                Panel::Language | Panel::Agent => (Icon::Settings, false),
                Panel::Skills => (Icon::Wrench, false),
                Panel::Location => (Icon::Pin, self.location.enabled()),
                Panel::Proxy => (Icon::Network, self.proxy.enabled()),
                Panel::Knowledge => (Icon::Book, false),
                Panel::Files => (Icon::Folder, false),
                Panel::Goal => (Icon::Target, self.modes.goal_active()),
                Panel::Capture(_) | Panel::Update | Panel::Todos => unreachable!(),
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
                button(container(content).center(Length::Fill))
                    .width(style::NAV_ICON_WIDTH)
                    .height(nav_height)
                    .padding(0)
                    .style(move |theme, status| {
                        let mut s = style::quiet(theme, status);
                        if chosen {
                            s.background = Some(style::ACCENT.into());
                        }
                        s
                    })
                    .on_press_maybe(click)
                    .into()
            } else {
                sized_icon_button(icon, click, style::NAV_ICON_WIDTH, nav_height)
            };
            shared = shared.push(if self.connection.is_some() {
                mouse_area(trigger)
                    .on_enter(Message::OpenPanel(panel, false))
                    .into()
            } else {
                trigger
            });
        }
        if self.updates.offer.is_some() {
            shared = shared.push(
                mouse_area(sized_icon_button(
                    Icon::Download,
                    Some(Message::OpenPanel(Panel::Update, true)),
                    style::NAV_ICON_WIDTH,
                    nav_height,
                ))
                .on_enter(Message::OpenPanel(Panel::Update, false)),
            );
        }
        let nav = row![
            container(error).width(Length::Fill).clip(true),
            row![
                shared.spacing(style::NAV_TOOL_GAP).align_y(iced::Center),
                space::horizontal().width(12.),
                iced::widget::rule::vertical(1),
                space::horizontal().width(6.)
            ]
            .height(nav_height),
            sized_icon_button(
                Icon::Refresh,
                if !self.error.is_empty() && !self.root.is_empty() {
                    Some(Message::Retry)
                } else {
                    self.connection
                        .as_ref()
                        .map(|_| Message::Conversation(chat::Message::RefreshDisplay))
                },
                style::NAV_ICON_WIDTH,
                nav_height,
            )
        ]
        .spacing(nav_gap)
        .align_y(iced::Center);
        layers.push(
            pin(container(nav)
                .width(WORKBENCH.width - 2. * inset)
                .height(nav_height))
            .x(inset)
            .y(nav_top)
            .into(),
        );
        let mut stage = vec![
            space::Space::new()
                .width(WORKBENCH.width)
                .height(WORKBENCH.height)
                .into(),
        ];
        for index in 0..2 {
            if self.focus != index && !self.linked {
                continue;
            }
            let rect = self.layout.toolbars[index];
            let mut tools = row![
                self.audio[index]
                    .view(self.connection.is_some(), self.reduced_motion)
                    .map(move |m| Message::Audio(index, m))
            ]
            .align_y(iced::Center);
            if index == 0 {
                let landscape = self.aspects()[0] > 1.;
                tools = tools.push(controls::sized_icon_button(
                    if landscape {
                        Icon::PhoneLandscape
                    } else {
                        Icon::PhonePortrait
                    },
                    self.frames[0].as_ref().map(|_| Message::Rotate),
                    32.,
                    34.,
                ));
            }
            tools = tools.push(controls::sized_icon_button(
                Icon::Camera,
                self.frames[index]
                    .as_ref()
                    .map(|_| Message::OpenPanel(Panel::Capture(index), true)),
                32.,
                34.,
            ));
            stage.push(
                pin(container(tools)
                    .width(rect.width)
                    .height(rect.height)
                    .align_y(iced::Center)
                    .clip(true))
                .x(rect.x)
                .y(rect.y)
                .into(),
            );
        }
        let r = self.layout.chat;
        stage.push(
            pin(container(
                self.chat
                    .view(
                        &self.modes,
                        self.locale,
                        r.width - 2. * layout::CHAT_PADDING[1],
                        self.reduced_motion,
                    )
                    .map(Message::Conversation),
            )
            .padding(layout::CHAT_PADDING)
            .width(r.width)
            .height(r.height))
            .x(r.x)
            .y(r.y)
            .into(),
        );
        let place = |rect: Rectangle| rect + self.canvas_origin;
        layers.push(occlude::behind(
            occlude::behind(stack(stage).into(), place(popup)),
            place(child_popup),
        ));
        let p = self.popup_rect();
        if self.popup_amount > 0. {
            let settled = self.panel_bounds(self.panel_size);
            let content = container(occlude::behind(self.panel_body(), place(child_popup)))
                .width(self.panel_target().width)
                .height(settled.height);
            layers.push(surface::tool_layer(
                content.into(),
                settled,
                p,
                Message::PanelFocus,
            ));
        }
        if let Some(child) = &self.child
            && child.opacity > 0.
        {
            if matches!(child.pane, ChildPane::Preview { .. }) {
                let opacity = child.opacity;
                layers.push(
                    mouse_area(
                        container(space())
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .style(move |_| {
                                container::Style::default()
                                    .background(iced::Color::BLACK.scale_alpha(0.5 * opacity))
                            }),
                    )
                    .on_press(Message::Conversation(chat::Message::ClosePreview))
                    .into(),
                );
            }
            if let Some(previous) = &child.previous {
                let content =
                    occlude::behind(self.child_view(previous, child.bounds), place(child.rect));
                let outgoing = surface::tool_layer(
                    content,
                    child.bounds,
                    child.bounds,
                    Message::ClosedPaneInput,
                );
                layers.push(outgoing.map(|_| Message::ClosedPaneInput));
            }
            let content = self.child_view(&child.pane, child.bounds);
            let in_place = child.previous.is_some()
                || matches!(child.pane, ChildPane::Devices)
                || matches!(&child.pane, ChildPane::Agent(id, _) if !id.is_empty());
            let layer = if in_place && child.animation.is_some() {
                surface::tool_layer(
                    occlude::within(content, place(child.rect)),
                    child.bounds,
                    child.bounds,
                    Message::PanelFocus,
                )
            } else {
                surface::tool_layer(content, child.bounds, child.rect, Message::PanelFocus)
            };
            layers.push(if child.open {
                layer
            } else {
                layer.map(|_| Message::ClosedPaneInput)
            });
        }
        container(
            container(stack(layers))
                .width(WORKBENCH.width)
                .height(WORKBENCH.height)
                .clip(true),
        )
        .center(Length::Fill)
        .into()
    }
}
fn main() -> iced::Result {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if let Some(code) = client_update::run(&arguments) {
        std::process::exit(code);
    }
    #[cfg(target_os = "macos")]
    if let Some(code) = uninstall::run(&arguments) {
        std::process::exit(code);
    }
    #[cfg(windows)]
    if let Some(code) = host::windows_command(&arguments) {
        std::process::exit(code);
    }
    if let [command, resource] = arguments.as_slice()
        && command == "--verify-bundled-host"
    {
        if let Err(error) = deployment::verify_bundle(std::path::Path::new(resource)) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(result) = ai_phone_host_runtime::component::run(&arguments) {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    iced::application(App::boot, App::update, App::view)
        .title(|app: &App| {
            if app.connection.is_some() {
                "AImanity".into()
            } else {
                String::new()
            }
        })
        .subscription(App::subscription)
        .scale_factor(|app| app.ui_scale)
        .theme(|_: &App| style::theme())
        .window(window::Settings {
            size: SETUP_SIZE,
            position: window::Position::Centered,
            resizable: false,
            exit_on_close_request: false,
            ..Default::default()
        })
        .default_font(style::CJK_FONT)
        .run()
}
