mod agent_details;
mod agents;
#[cfg(target_os = "android")]
mod android_keyboard;
#[cfg(target_os = "android")]
mod android_os;
mod audio;
mod board;
mod chat;
mod connection;
mod controls;
mod deployment;
mod devices;
mod documents;
mod files;
mod host;
#[cfg_attr(target_os = "android", path = "host_desktop_mobile.rs")]
mod host_desktop;
mod knowledge;
mod layout;
mod loading;
mod locale;
mod location;
mod modes;
mod occlude;
#[cfg(not(target_os = "android"))]
mod paired_phone;
#[cfg(not(target_os = "android"))]
mod peer_connection;
mod platform;
mod proxy;
mod scene;
mod scene_gpu;
#[cfg(not(target_os = "android"))]
mod sharing;
mod skills;
mod spring_slider;
mod stage;
mod style;
mod surface;
mod todos;
mod uninstall;
mod workspace;
use ai_phone_api::wire;
use connection::{Connection, DeviceInput, Press};
use iced::{
    Element, Event, Length, Rectangle, Size, Subscription, Task, keyboard,
    widget::{
        button, column, container, image, mouse_area, pin, row, scrollable, sensor, space, stack,
        text,
    },
    window,
};
use layout::Layout;
use std::time::{Duration, Instant};

const APP_ID: &str = "dev.aimanity.workspace";
const SETUP_SIZE: Size = Size::new(680., 600.);
#[cfg(not(target_os = "android"))]
const WORKBENCH_ASPECT: Size = Size::new(8., 5.);
use layout::WORKBENCH;
#[cfg(not(target_os = "android"))]
const WORKBENCH_MIN: Size = Size::new(960., 600.);
const DESKTOP_DOUBLE_CLICK: Duration = Duration::from_millis(320);

#[derive(Clone, Debug)]
enum Message {
    Closed,
    Setup(deployment::Message),
    Connected(Result<Box<connection::Session>, String>),
    Frame(String, connection::LatestFrame),
    PollAgentPointers,
    AgentPointers(Vec<wire::DeviceCursor>),
    Chat(Result<wire::ChatState, String>),
    Conversation(chat::Message),
    Agents(agents::Message),
    Devices(devices::Message),
    HostAccess(bool),
    Audio(String, audio::Message),
    Files(Box<files::Message>),
    Todos(todos::Message),
    Knowledge(knowledge::Message),
    Skills(skills::Message),
    Location(location::Message),
    Proxy(proxy::Message),
    Modes(modes::Message),
    SetLocale(locale::Locale),
    CaptureDevice(String),
    CloseCapture,
    Captured(String, Result<Capture, String>),
    SaveCapture,
    AttachCapture,
    CaptureSaved(Result<(), String>),
    ShowDevice(usize),
    MoveDevice(String, iced::Vector),
    Pan(iced::Vector),
    Zoom(iced::Point, f32),
    ResetBoard,
    CanvasFocus,
    DeviceState(Result<wire::RegistryState, String>),
    Input(usize, DeviceInput),
    InputDone(String, Result<(), String>, Instant),
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
    Rotate(String),
    MediaKey(String, &'static str),
    Uninstall,
    UninstallPrepared(Result<Option<uninstall::Plan>, String>),
}
#[derive(Clone, Debug)]
struct Capture {
    name: String,
    image: image::Handle,
    png: bytes::Bytes,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Devices,
    Todos,
    Language,
    Skills,
    Location,
    Proxy,
    Knowledge,
    Files,
    Goal,
    Agent,
}
#[derive(Clone, PartialEq, Eq)]
enum ChildPane {
    Agent(String, agents::Tab),
    Knowledge(bool),
    Preview { id: String, name: String },
}
impl ChildPane {
    fn separate(&self) -> bool {
        match self {
            Self::Agent(id, _) => id.is_empty(),
            Self::Preview { .. } => false,
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
struct App {
    setup: Option<deployment::Setup>,
    host_home: std::path::PathBuf,
    uninstall: Option<uninstall::Plan>,
    #[cfg(any(windows, target_os = "linux"))]
    instance_guard: Option<host::InstanceGuard>,
    phase: String,
    closing: bool,
    connection: Option<Connection>,
    devices: Vec<stage::Device>,
    pointer_polling: bool,
    chat_connection_error: String,
    chat: chat::Chat,
    agents: agents::Agents,
    device_admin: devices::Devices,
    files: files::Files,
    todos: todos::Todos,
    knowledge: knowledge::Knowledge,
    skills: skills::Skills,
    location: location::Location,
    proxy: proxy::Proxy,
    modes: modes::Modes,
    reduced_motion: bool,
    locale: locale::Locale,
    capture: Option<(String, Option<Capture>)>,
    cursor: iced::Point,
    error: String,
    root: String,
    focus: usize,
    camera: layout::Camera,
    keyboard_target: Option<usize>,
    streams: Option<iced::task::Handle>,
    ui_scale: f32,
    canvas_origin: iced::Vector,
    home: Layout,
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
    agent_list_height: f32,
    agent_add_height: f32,
    panel_leave_pending: bool,
    started: Instant,
    input_started: Option<Instant>,
}
impl App {
    fn boot() -> (Self, Task<Message>) {
        let camera = layout::Camera::default();
        let layout = Layout::home(WORKBENCH, []);
        let mut app = Self {
            setup: None,
            host_home: std::path::PathBuf::new(),
            uninstall: None,
            #[cfg(any(windows, target_os = "linux"))]
            instance_guard: host::instance_guard().ok(),
            phase: String::new(),
            closing: false,
            connection: None,
            devices: Vec::new(),
            pointer_polling: false,
            chat_connection_error: String::new(),
            chat: chat::Chat::new(),
            agents: agents::Agents::new(),
            device_admin: Default::default(),
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
            error: String::new(),
            root: String::new(),
            focus: 0,
            camera,
            keyboard_target: None,
            streams: None,
            ui_scale: 1.,
            canvas_origin: iced::Vector::ZERO,
            home: layout.clone(),
            layout: layout.clone(),
            from: layout.clone(),
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
            agent_list_height: 232.,
            agent_add_height: 505.,
            panel_leave_pending: false,
            started: Instant::now(),
            input_started: None,
        };
        #[cfg(any(windows, target_os = "linux"))]
        if app.instance_guard.is_none() {
            return (app, iced::exit());
        }
        let task = match deployment::Setup::load() {
            Ok(setup) => {
                let ready = setup.saved;
                app.locale = setup.language;
                app.setup = Some(setup);
                if ready && cfg!(target_os = "android") {
                    app.update(Message::Setup(deployment::Message::Continue))
                } else {
                    Task::none()
                }
            }
            Err(error) => {
                app.error = error;
                Task::none()
            }
        };
        (app, task)
    }
    fn rearrange(&mut self, animate: bool) {
        let on_canvas = |d: &stage::Device| {
            connection::reachable(&d.info) && !self.device_admin.issues.contains_key(&d.info.id)
        };
        let mut home = Layout::home(
            WORKBENCH,
            self.devices
                .iter()
                .filter(|d| on_canvas(d))
                .map(|d| (d.info.id.clone(), d.aspect())),
        );
        let mut visible = home.tiles.into_iter();
        home.tiles = self
            .devices
            .iter()
            .map(|device| {
                if on_canvas(device) {
                    let mut tile = visible.next().unwrap();
                    tile.shift(device.offset);
                    tile
                } else {
                    layout::Tile::hidden(device.info.id.clone())
                }
            })
            .collect();
        self.home = home;
        self.move_camera(animate);
    }
    fn move_camera(&mut self, animate: bool) {
        self.target = self.home.project(&mut self.camera);
        for (d, tile) in self.devices.iter().zip(&self.target.tiles) {
            let shown = if tile.visible() {
                tile.screen.intersects(&self.target.board) || d.busy || d.waiting
            } else {
                connection::reachable(&d.info)
            };
            d.visible.send_if_modified(|was| {
                let changed = *was != shown;
                *was = shown;
                changed
            });
        }
        if self.target == self.layout {
            self.transition = None;
            return;
        }
        if animate && !self.reduced_motion {
            self.from = self.layout.clone();
            // Reconcile identities before hit-testing the first frame of the transition.
            self.layout = self.from.blend(&self.target, 0.);
            self.transition = Some(Instant::now());
        } else {
            self.layout = self.target.clone();
            self.transition = None;
        }
    }
    fn disconnect_streams(&mut self) {
        self.streams = None;
        for device in &mut self.devices {
            device.stream = None;
        }
    }
    fn display_devices(
        &mut self,
        registry: wire::RegistryState,
        client: &Connection,
    ) -> Task<Message> {
        let focused = self.devices.get(self.focus).map(|d| d.info.id.clone());
        let keyboard = self
            .keyboard_target
            .and_then(|i| self.devices.get(i))
            .map(|d| d.info.id.clone());
        let mut previous: std::collections::HashMap<_, _> = std::mem::take(&mut self.devices)
            .into_iter()
            .map(|d| (d.info.id.clone(), d))
            .collect();
        let mut tasks = Vec::new();
        for info in &registry.devices {
            let mut device = previous
                .remove(&info.id)
                .unwrap_or_else(|| stage::Device::new(info.clone()));
            if info.paired
                && !info.connected
                && let Some(cause) = client.workspace.disconnect_reason(&info.id)
            {
                self.device_admin.connection_failed(&info.id, cause);
            }
            device.info = info.clone();
            if connection::reachable(info) {
                if device.stream.is_none() {
                    if !info.paired {
                        let id = info.id.clone();
                        tasks.push(
                            device
                                .audio
                                .restore(&self.root, &id)
                                .map(move |m| Message::Audio(id.clone(), m)),
                        );
                    }
                    let id = info.id.clone();
                    let (task, handle) = Task::run(
                        client
                            .clone()
                            .frames(info.clone(), device.visible.subscribe()),
                        move |frame| Message::Frame(id.clone(), frame),
                    )
                    .abortable();
                    device.stream = Some(handle.abort_on_drop());
                    tasks.push(task);
                }
            } else if device.stream.take().is_some() {
                device.audio.stop();
                device.frame = None;
                device.trace = None;
                device.busy = false;
                device.waiting = false;
                device.queue.clear();
                device.click = None;
            }
            self.devices.push(device);
        }
        for device in previous.values_mut() {
            device.audio.stop();
        }
        self.focus = focused.and_then(|id| self.device_index(&id)).unwrap_or(0);
        self.keyboard_target = keyboard
            .and_then(|id| self.device_index(&id))
            .filter(|&i| connection::reachable(&self.devices[i].info));
        self.device_admin
            .issues
            .retain(|id, _| registry.devices.iter().any(|d| d.id == *id));
        self.agents.registry = registry;
        self.files.set_devices(&self.agents.registry.devices);
        self.rearrange(true);
        if self
            .capture
            .as_ref()
            .is_some_and(|(id, _)| self.device_index(id).is_none())
        {
            self.capture = None;
        }
        Task::batch(tasks)
    }
    fn device_index(&self, id: &str) -> Option<usize> {
        self.devices.iter().position(|d| d.info.id == id)
    }
    fn set_popup(&mut self, open: bool) {
        if self.popup != open {
            if !open {
                self.capture = None;
            }
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
            #[cfg(any(windows, target_os = "linux"))]
            drop(self.instance_guard.take());
            if let Err(error) = plan.launch() {
                #[cfg(any(windows, target_os = "linux"))]
                {
                    self.instance_guard = host::instance_guard().ok();
                }
                self.phase.clear();
                self.error = error;
                return window::latest()
                    .and_then(|id| window::set_mode(id, window::Mode::Windowed));
            }
        }
        iced::exit()
    }
    fn desktop_input(&mut self, index: usize, mut input: DeviceInput) -> Task<Message> {
        let previous = self.devices[index].click.take();
        if let DeviceInput::Tap {
            x,
            y,
            press: Press::Left,
        } = input
        {
            let now = Instant::now();
            if previous.is_some_and(|(at, px, py)| {
                now.duration_since(at) < DESKTOP_DOUBLE_CLICK
                    && x.abs_diff(px).pow(2) + y.abs_diff(py).pow(2) < 144
            }) {
                input = DeviceInput::Tap {
                    x,
                    y,
                    press: Press::Second,
                };
            } else {
                self.devices[index].click = Some((now, x, y));
            }
        }
        self.input(index, input)
    }
    fn input(&mut self, index: usize, mut input: DeviceInput) -> Task<Message> {
        if self.devices[index].busy || self.devices[index].waiting {
            match (self.devices[index].queue.back_mut(), &input) {
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
            self.devices[index].queue.push_back(input);
            return Task::none();
        }
        let Some(client) = self.connection.clone() else {
            return Task::none();
        };
        let Some(device) = self.devices.get(index).map(|d| d.info.clone()) else {
            return Task::none();
        };
        if self.devices[index].phone()
            && let DeviceInput::Scroll { y, amount, .. } = &mut input
            && let Some(frame) = &self.devices[index].frame
        {
            *amount = (*amount).clamp(-((frame.height - 1 - *y) as i32), *y as i32);
        }
        self.devices[index].busy = true;
        let started = Instant::now();
        let target = device.id.clone();
        let observation = self.devices[index]
            .frame
            .as_ref()
            .map(|f| f.observation_id.clone())
            .unwrap_or_default();
        Task::perform(
            async move { client.input(&device, input, observation).await },
            move |r| Message::InputDone(target.clone(), r, started),
        )
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        if self.closing && !matches!(message, Message::Closed) {
            return Task::none();
        }
        match message {
            Message::Closed => return self.finish_exit(),
            Message::Setup(message) => {
                if let Some(setup) = &mut self.setup {
                    match message {
                        deployment::Message::Choose(language) => {
                            setup.language = language;
                            self.locale = language;
                        }
                        deployment::Message::Continue => {
                            if !self.phase.is_empty() {
                                return Task::none();
                            }
                            if let Err(error) = setup.save() {
                                self.error = error;
                                return Task::none();
                            }
                            self.host_home = setup.home.clone();
                            self.root = setup.home.to_string_lossy().into_owned();
                            self.chat.host = host_desktop::Permission::load(&self.host_home);
                            self.location.set_root(&self.host_home);
                            self.locale = setup.language;
                            self.modes.set_language(self.locale.code());
                            self.modes
                                .set_speech_model(setup.resource.join("speech/ggml-tiny-q5_1.bin"));
                            self.phase = "正在启动工作区".into();
                            self.started = Instant::now();
                            return Task::perform(Connection::connect(self.root.clone()), |r| {
                                Message::Connected(r.map(Box::new))
                            });
                        }
                    }
                }
            }
            Message::Connected(result) => match result {
                Ok(session) => {
                    let connection::Session { client, registry } = *session;
                    self.setup = None;
                    let device_streams = self.display_devices(registry, &client);
                    let restore = self.chat.restore(&client).map(Message::Conversation);
                    self.phase.clear();
                    let first_connection = self.connection.replace(client.clone()).is_none();
                    eprintln!(
                        "native_metric connected_ms={:0.3}",
                        self.started.elapsed().as_secs_f64() * 1000.
                    );
                    let mut tasks = vec![
                        restore,
                        Task::run(client.clone().chat_events(), Message::Chat),
                        Task::run(client.clone().device_events(), Message::DeviceState),
                    ];
                    #[cfg(target_os = "android")]
                    if first_connection {
                        tasks.push(
                            window::latest()
                                .and_then(|id| platform::content_size(id).map(Message::Viewport)),
                        );
                    }
                    #[cfg(not(target_os = "android"))]
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
                    tasks.push(
                        self.modes
                            .update(modes::Message::Refresh, &client, "", &self.chat.state)
                            .map(Message::Modes),
                    );
                    let (task, handle) = Task::batch(tasks).abortable();
                    self.streams = Some(handle.abort_on_drop());
                    return Task::batch([task, device_streams]);
                }
                Err(error) => {
                    self.error = error;
                    self.phase.clear();
                }
            },
            Message::Uninstall if !self.closing => {
                return Task::perform(uninstall::prepare(self.locale), Message::UninstallPrepared);
            }
            Message::UninstallPrepared(result) if !self.closing => match result {
                Ok(Some(plan)) => {
                    self.uninstall = Some(plan);
                    return self.update(Message::Quit);
                }
                Ok(None) => {}
                Err(error) => self.error = error,
            },
            Message::Frame(id, pending) => {
                let Some(index) = self.device_index(&id) else {
                    return Task::none();
                };
                if self.devices[index].stream.is_none() {
                    return Task::none();
                }
                match pending.take() {
                    Some(Ok(frame)) => {
                        let resized = self.devices[index].frame.as_ref().is_none_or(|old| {
                            old.width != frame.width || old.height != frame.height
                        });
                        if self.devices[index].frame.is_none() {
                            eprintln!(
                                "native_metric first_frame device={index} ms={:0.3}",
                                self.started.elapsed().as_secs_f64() * 1000.
                            );
                        }
                        self.devices[index].receive(frame);
                        let recovered = self.device_admin.frame_received(&id);
                        if resized || recovered {
                            self.rearrange(true);
                        }
                        if std::mem::take(&mut self.devices[index].waiting)
                            && let Some(next) = self.devices[index].queue.pop_front()
                        {
                            return self.input(index, next);
                        }
                    }
                    Some(Err(error)) => {
                        if self.device_admin.connection_failed(&id, error) {
                            self.devices[index].frame = None;
                            self.devices[index].trace = None;
                            self.devices[index].queue.clear();
                            self.devices[index].waiting = false;
                            if self.keyboard_target == Some(index) {
                                self.keyboard_target = None;
                            }
                            self.rearrange(true);
                        }
                    }
                    None => {}
                }
            }
            Message::PollAgentPointers if !self.pointer_polling && !self.closing => {
                if let Some(client) = self.connection.clone() {
                    self.pointer_polling = true;
                    let devices = self
                        .devices
                        .iter()
                        .filter(|d| d.stream.is_some() && !d.info.paired)
                        .map(|d| d.info.clone())
                        .collect::<Vec<_>>();
                    return Task::perform(
                        async move {
                            futures_util::future::join_all(
                                devices.into_iter().map(|d| client.cursor(d)),
                            )
                            .await
                            .into_iter()
                            .flatten()
                            .collect()
                        },
                        Message::AgentPointers,
                    );
                }
            }
            Message::AgentPointers(cursors) => {
                self.pointer_polling = false;
                for cursor in cursors {
                    if let Some(index) = self.device_index(&cursor.device_id) {
                        let d = &mut self.devices[index];
                        d.trace = scene::AgentTrace::from_cursor(&cursor, &d.info);
                    }
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
                    return Task::batch([task, speech, sample, todos]);
                }
            }
            Message::Conversation(chat::Message::Todos(click)) => {
                return self.update(Message::OpenPanel(Panel::Todos, click));
            }
            Message::Conversation(chat::Message::Goal(click)) => {
                return self.update(Message::OpenPanel(Panel::Goal, click));
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
                if self.layout.chat.contains(self.cursor) {
                    self.keyboard_target = None;
                }
                if let chat::Message::Mode(message) = message {
                    return self.update(Message::Modes(message));
                }
                if matches!(message, chat::Message::Edit(_)) {
                    self.keyboard_target = None;
                    self.input_started = Some(Instant::now());
                }
                if matches!(message, chat::Message::Pick | chat::Message::Attachment(_)) {
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
                    let task = self
                        .chat
                        .update(message, client, "")
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
                    let device = "";
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
            Message::Audio(id, message) => {
                let Some(i) = self.device_index(&id) else {
                    return Task::none();
                };
                if matches!(message, audio::Message::Toggle | audio::Message::Volume(_)) {
                    self.keyboard_target = None;
                }
                let kind = usize::from(!self.devices[i].phone());
                return self.devices[i]
                    .audio
                    .update(message, self.connection.as_ref(), kind)
                    .map(move |m| Message::Audio(id.clone(), m));
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
            Message::CaptureDevice(device_id) => {
                let Some(index) = self.device_index(&device_id) else {
                    return Task::none();
                };
                let Some(frame) = self.devices[index].frame.clone() else {
                    return Task::none();
                };
                let kind = if self.devices[index].phone() {
                    "Android"
                } else {
                    "Linux"
                };
                self.capture = Some((device_id.clone(), None));
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
                            let png = bytes::Bytes::from(png);
                            let image = image::Handle::from_bytes(png.clone());
                            let name = format!(
                                "{}-{}.png",
                                kind,
                                chrono::Local::now().format("%Y%m%d-%H%M%S%.3f")
                            );
                            Ok(Capture { png, image, name })
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    move |r| Message::Captured(device_id.clone(), r),
                );
            }
            Message::Captured(id, result) => {
                if let Some((device, preview)) = &mut self.capture
                    && *device == id
                {
                    match result {
                        Ok(image) => *preview = Some(image),
                        Err(error) => {
                            self.capture = None;
                            self.error = error;
                        }
                    }
                }
            }
            Message::CloseCapture => self.capture = None,
            Message::AttachCapture => {
                if let Some((_, Some(capture))) = self.capture.take() {
                    self.set_popup(false);
                    return self.update(Message::Conversation(chat::Message::Capture(
                        capture.name,
                        capture.png.to_vec(),
                    )));
                }
            }
            Message::SaveCapture => {
                if let Some((_, Some(capture))) = self.capture.clone() {
                    return Task::perform(
                        async move {
                            if let Some(target) = documents::AsyncFileDialog::new()
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
            Message::Agents(agents::Message::ListMeasured(height)) => {
                if self.agent_list_height != height {
                    self.agent_list_height = height;
                    if self.popup && self.panel == Panel::Agent {
                        self.reflow_popup();
                        self.reflow_child();
                    }
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
            Message::Devices(devices::Message::Restart) => {
                self.error = "运行设备的配置尚未接入本地工作区".into();
            }
            Message::Devices(message) => {
                if let Some(client) = self.connection.clone() {
                    let status_change = matches!(
                        &message,
                        devices::Message::Installed(..)
                            | devices::Message::Invitation(..)
                            | devices::Message::Jobs(..)
                    );
                    let task = self
                        .device_admin
                        .update(message, &self.agents.registry, &client)
                        .map(Message::Devices);
                    if let Some(registry) = self.device_admin.registry.take() {
                        return Task::batch([task, self.display_devices(registry, &client)]);
                    }
                    if status_change {
                        self.rearrange(true);
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
            Message::DeviceState(Ok(registry)) => {
                if let Some(client) = self.connection.clone() {
                    if let Some((invitation, _)) = &self.device_admin.invitation
                        && registry
                            .devices
                            .iter()
                            .any(|d| d.id == invitation.device_id && d.connected)
                    {
                        self.device_admin.invitation = None;
                    }
                    return self.display_devices(registry, &client);
                }
            }
            Message::DeviceState(Err(error)) => self.error = error,
            Message::ShowDevice(index) => {
                self.focus = index;
                self.keyboard_target = self
                    .devices
                    .get(index)
                    .filter(|d| connection::reachable(&d.info))
                    .map(|_| index);
            }
            Message::MoveDevice(id, delta) => {
                if let Some(index) = self.device_index(&id)
                    && self.home.tiles[index].visible()
                {
                    let delta = delta * (1. / self.camera.zoom);
                    self.devices[index].offset += delta;
                    self.home.tiles[index].shift(delta);
                    self.move_camera(false);
                }
            }
            Message::CanvasFocus => self.keyboard_target = None,
            Message::Pan(delta) => {
                self.keyboard_target = None;
                self.camera.pan += delta;
                self.move_camera(false);
            }
            Message::Zoom(point, delta) => {
                self.camera.zoom_at(point, delta, self.home.grid_origin);
                self.move_camera(false);
            }
            Message::ResetBoard => {
                self.keyboard_target = None;
                self.camera = layout::Camera::default();
                for device in &mut self.devices {
                    device.offset = iced::Vector::ZERO;
                }
                self.rearrange(true);
            }
            Message::Input(index, input) if !self.devices[index].phone() => {
                return self.desktop_input(index, input);
            }
            Message::Input(index, input) => return self.input(index, input),
            Message::InputDone(id, result, started) => {
                let Some(index) = self.device_index(&id) else {
                    return Task::none();
                };
                self.devices[index].busy = false;
                eprintln!(
                    "native_metric input_receipt device={index} ms={:0.3} success={}",
                    started.elapsed().as_secs_f64() * 1000.,
                    result.is_ok()
                );
                if let Err(error) = result {
                    self.error = error;
                    self.devices[index].queue.clear();
                } else if self.devices[index].info.paired {
                    self.devices[index].waiting = true;
                    self.error.clear();
                } else if let Some(next) = self.devices[index].queue.pop_front() {
                    return self.input(index, next);
                } else {
                    self.error.clear();
                }
            }
            Message::Retry => {
                self.disconnect_streams();
                self.error.clear();
                return Task::perform(
                    {
                        let client = self.connection.clone();
                        let root = self.root.clone();
                        async move {
                            match client {
                                Some(c) => c.session().await,
                                None => Connection::connect(root).await,
                            }
                        }
                    },
                    |r| Message::Connected(r.map(Box::new)),
                );
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
                    self.layout = self.from.blend(&self.target, smooth);
                    if t >= 1. {
                        self.layout = self.target.clone();
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
                let start = if self.setup.as_ref().is_some_and(|setup| setup.saved) {
                    self.update(Message::Setup(deployment::Message::Continue))
                } else {
                    Task::none()
                };
                #[cfg(target_os = "macos")]
                return Task::batch([platform::menu(), start]);
                #[cfg(not(target_os = "macos"))]
                return start;
            }
            Message::FitViewport(id)
                if cfg!(target_os = "android") || self.connection.is_some() =>
            {
                return platform::content_size(id).map(Message::Viewport);
            }
            Message::Viewport(size) if size.width > 0. && size.height > 0. => {
                let canvas = if cfg!(target_os = "android") && self.setup.is_some() {
                    SETUP_SIZE
                } else {
                    WORKBENCH
                };
                (self.ui_scale, self.canvas_origin) = layout::fit_canvas(size, canvas);
            }
            Message::Event(Event::Window(window::Event::Focused)) => {
                self.reduced_motion = platform::reduced_motion();
            }
            Message::Quit | Message::Event(Event::Window(window::Event::CloseRequested)) => {
                self.chat.host.close();
                self.closing = true;
                self.disconnect_streams();
                for device in &mut self.devices {
                    device.audio.stop();
                }
                self.modes.stop_capture();
                if let Some(client) = self.connection.take() {
                    return Task::perform(async move { client.workspace.stop().await }, |()| {
                        Message::Closed
                    });
                }
                return self.finish_exit();
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
                if self.popup && self.panel == Panel::Devices && self.device_admin.editing() {
                    return self.update(Message::Devices(devices::Message::Discard));
                }
                if self.popup && self.panel == Panel::Agent && self.agents.adding() {
                    return self.update(Message::Agents(agents::Message::Back));
                }
                if self.popup && self.panel == Panel::Agent && self.agents.is_editing() {
                    return self.update(Message::Agents(agents::Message::Back));
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
            Message::Event(
                Event::Mouse(iced::mouse::Event::ButtonPressed(_))
                | Event::Touch(iced::touch::Event::FingerPressed { .. }),
            ) => {
                if let Message::Event(Event::Touch(iced::touch::Event::FingerPressed {
                    position,
                    ..
                })) = message
                {
                    self.cursor = position - self.canvas_origin;
                }
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
                    self.capture = None;
                    self.todos.cancel_drag();
                    self.panel = panel;
                    self.popup_amount = 0.;
                    self.panel_size = self.panel_target();
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
            Message::MediaKey(id, key) => {
                if let Some(i) = self.device_index(&id) {
                    return self.input(i, DeviceInput::Key(key.into()));
                }
            }
            Message::Rotate(id) => {
                if let Some(index) = self.device_index(&id)
                    && let Some(frame) = &self.devices[index].frame
                {
                    let rotation = u32::from(frame.width < frame.height);
                    return self.input(index, DeviceInput::Rotate(rotation));
                }
            }
            Message::Event(Event::Window(window::Event::FileDropped(path))) => {
                if self.popup
                    && self.panel == Panel::Files
                    && self.popup_rect().contains(self.cursor)
                {
                    let body = surface::tool_content_rect(self.popup_rect(), WORKBENCH.width);
                    return match self.files.local_drop(self.cursor, body, path) {
                        Some(message) => self.update(Message::Files(Box::new(message))),
                        None => Task::none(),
                    };
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
        if self.closing {
            return Subscription::none();
        }
        let mut subscriptions = vec![iced::event::listen_with(|event, status, id| match &event {
            Event::Window(window::Event::Opened { .. }) if cfg!(target_os = "android") => {
                Some(Message::FitViewport(id))
            }
            Event::Window(window::Event::Resized(_)) => Some(Message::FitViewport(id)),
            Event::Window(
                window::Event::Opened { .. }
                | window::Event::Focused
                | window::Event::CloseRequested,
            )
            | Event::Window(window::Event::FileDropped(_))
            | Event::Touch(iced::touch::Event::FingerPressed { .. })
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
            || (!self.reduced_motion && (self.modes.animating()))
        {
            subscriptions.push(window::frames().map(Message::Tick));
        }
        subscriptions.push(self.chat.subscription().map(Message::Conversation));
        if !self.closing && self.connection.is_some() && self.agent_pointer_active() {
            subscriptions.push(
                iced::time::every(std::time::Duration::from_millis(100))
                    .map(|_| Message::PollAgentPointers),
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
        if self.popup && self.panel == Panel::Devices {
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
            Panel::Agent => {
                let editing = self.agents.editor().is_some_and(|(id, _)| !id.is_empty());
                (
                    680.,
                    if editing {
                        600.
                    } else {
                        self.agent_list_height.min(600.)
                    },
                    false,
                )
            }
            Panel::Devices => (650., 640., true),
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
        if !matches!(self.panel, Panel::Todos | Panel::Goal) {
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
        Panel::Devices,
        Panel::Language,
        Panel::Skills,
        Panel::Location,
        Panel::Proxy,
        Panel::Knowledge,
        Panel::Files,
        Panel::Agent,
    ];
    fn device_tools(&self, index: usize, id: &str) -> Option<Element<'_, Message>> {
        let device = self.devices.get(index).filter(|d| d.info.id == id)?;
        if !connection::reachable(&device.info)
            || self.device_admin.issues.contains_key(&device.info.id)
        {
            return None;
        }
        let id = &device.info.id;
        let ready = device.frame.is_some();
        let key = |name| ready.then(|| Message::MediaKey(id.clone(), name));
        let volume: Element<'_, Message> = if device.info.paired {
            row![
                surface::tool_action(controls::Icon::Minus, key("volumedown")),
                surface::tool_action(controls::Icon::Volume, key("volumemute")),
                surface::tool_action(controls::Icon::Plus, key("volumeup"))
            ]
            .into()
        } else {
            device
                .audio
                .view(self.connection.is_some(), self.reduced_motion)
                .map(move |m| Message::Audio(id.clone(), m))
        };
        let rotate: Element<'_, Message> = if device.phone() {
            iced::widget::tooltip(
                surface::tool_action(
                    controls::Icon::RotatePhone,
                    ready.then(|| Message::Rotate(id.clone())),
                ),
                text(self.locale.tr(if device.aspect() > 1. {
                    "切换竖屏"
                } else {
                    "切换横屏"
                }))
                .size(12),
                iced::widget::tooltip::Position::Bottom,
            )
            .style(container::rounded_box)
            .into()
        } else {
            space::horizontal().width(style::TOOL_CONTROL).into()
        };
        let pending = matches!(&self.capture,Some((owner,None)) if owner == id);
        let shot = iced::widget::tooltip(
            surface::tool_action(
                controls::Icon::Camera,
                (ready && !pending).then(|| Message::CaptureDevice(id.clone())),
            ),
            text(self.locale.tr("截图")).size(12),
            iced::widget::tooltip::Position::Bottom,
        )
        .style(container::rounded_box);
        Some(
            row![
                rotate,
                container(volume).width(3. * style::TOOL_CONTROL),
                shot
            ]
            .spacing(6)
            .align_y(iced::Center)
            .into(),
        )
    }
    fn capture_receipt(&self) -> Option<Element<'_, Message>> {
        let (_, preview) = self.capture.as_ref()?;
        let Some(capture) = preview else {
            return Some(crate::loading::orb(18., self.reduced_motion).into());
        };
        let preview = iced::widget::tooltip(
            container(text(&capture.name).size(13).wrapping(text::Wrapping::None))
                .clip(true)
                .width(Length::Fill),
            image(capture.image.clone())
                .width(210)
                .height(150)
                .content_fit(iced::ContentFit::Contain),
            iced::widget::tooltip::Position::Bottom,
        )
        .gap(4)
        .style(container::rounded_box);
        Some(
            row![
                controls::Icon::Image.view(),
                preview,
                controls::text_button(self.locale.tr("保存图片")).on_press(Message::SaveCapture),
                controls::text_button(self.locale.tr("附到对话")).on_press(Message::AttachCapture),
                surface::tool_action(controls::Icon::Close, Some(Message::CloseCapture))
            ]
            .spacing(8)
            .align_y(iced::Center)
            .into(),
        )
    }
    fn panel_anchor(&self, panel: Panel) -> Rectangle {
        if matches!(panel, Panel::Todos | Panel::Goal) {
            let chat = self.layout.chat;
            let [vertical, horizontal] = layout::CHAT_PADDING;
            let size = style::CONTROL_HEIGHT;
            let goal_offset = if panel == Panel::Goal {
                size + chat::COMPOSER_ACTION_GAP
            } else {
                0.
            };
            return Rectangle::new(
                iced::Point::new(
                    chat.x + horizontal + chat::COMPOSER_PADDING.left + goal_offset,
                    chat.y + chat.height - vertical - chat::COMPOSER_PADDING.bottom - size,
                ),
                Size::new(size, size),
            );
        }
        let [top, gap, height] = style::NAVIGATION;
        let inset = layout::SIDE_INSET;
        let trailing = Self::NAV_PANELS
            .iter()
            .position(|&entry| entry == panel)
            .map_or(0, |index| Self::NAV_PANELS.len() - index - 1);
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
        if matches!(self.panel, Panel::Todos | Panel::Goal) && position.y >= anchor.y {
            return anchor.contains(position);
        }
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
            Panel::Devices => Message::Devices(devices::Message::Open),
            Panel::Todos => Message::Todos(todos::Message::Load),
            Panel::Skills => Message::Skills(skills::Message::Load),
            Panel::Knowledge => Message::Knowledge(knowledge::Message::Load),
            Panel::Location => Message::Location(location::Message::Load),
            Panel::Proxy => Message::Proxy(proxy::Message::Load),
            Panel::Files => Message::Files(Box::new(files::Message::Load)),
            Panel::Goal => Message::Modes(modes::Message::Refresh),
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
        }
    }
    fn panel_body(&self) -> Element<'_, Message> {
        let locale = self.locale;
        let width = WORKBENCH.width;
        let padding = surface::tool_padding(width);
        if self.panel == Panel::Agent {
            let action = |label, message| controls::text_button(locale.tr(label)).on_press(message);
            let settings = row![];
            #[cfg(not(target_os = "android"))]
            let settings = settings.push(
                iced::widget::toggler(self.chat.host.enabled())
                    .label(locale.tr("允许本机协作"))
                    .text_size(13)
                    .on_toggle(Message::HostAccess),
            );
            let settings = settings
                .push(space::horizontal())
                .push(
                    action("卸载 AImanity", Message::Uninstall)
                        .padding(iced::Padding::ZERO.left(12)),
                )
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
            Panel::Devices => locale.tr("设备"),
            Panel::Todos => locale.tr("待办"),
            Panel::Language => "Language",
            Panel::Skills => locale.tr("Skill 管理"),
            Panel::Knowledge => locale.tr("知识库"),
            Panel::Files => locale.tr("文件管理"),
            Panel::Location => locale.tr("模拟位置"),
            Panel::Proxy => locale.tr("工作台代理"),
            Panel::Goal => "Task & Goal",
            Panel::Agent => unreachable!(),
        };
        let actions = row![].align_y(iced::Center).spacing(2);
        let actions = match self.panel {
            Panel::Devices => actions.push(
                self.device_admin
                    .actions(&self.agents.registry, locale)
                    .map(Message::Devices),
            ),
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
        let language_row_inset = 12.;
        let language_border_inset = 1.;
        let content = match self.panel {
            Panel::Devices => self.device_admin.view(
                &self.agents.registry,
                locale,
                self.reduced_motion,
                |i, id| self.device_tools(i, id),
                self.capture_receipt(),
                Message::Devices,
            ),
            Panel::Agent => unreachable!(),
            Panel::Skills => self.skills.view(locale).map(Message::Skills),
            Panel::Knowledge => self
                .knowledge
                .view(locale, self.reduced_motion)
                .map(Message::Knowledge),
            Panel::Todos => self.todos.view(locale).map(Message::Todos),
            Panel::Files => self
                .files
                .view(
                    surface::tool_content_rect(Rectangle::with_size(self.panel_target()), width),
                    locale,
                )
                .map(|m| Message::Files(Box::new(m))),
            Panel::Location => self.location.view(locale).map(Message::Location),
            Panel::Proxy => self.proxy.view(locale).map(Message::Proxy),
            Panel::Goal => self.modes.goal_view().map(Message::Modes),
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
        };
        let padded = container(content).padding(padding).width(Length::Fill);
        let body: Element<'_, Message> = match self.panel {
            Panel::Knowledge | Panel::Files => padded.height(Length::Fill).into(),
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
            Panel::Todos => scrollable(padded)
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
            let content = if self.phase.is_empty() {
                setup.view(&self.error).map(Message::Setup)
            } else {
                container(crate::loading::orb(180., self.reduced_motion))
                    .center(Length::Fill)
                    .into()
            };
            #[cfg(target_os = "android")]
            let content = container(
                container(content)
                    .width(SETUP_SIZE.width)
                    .height(SETUP_SIZE.height),
            )
            .center(Length::Fill)
            .into();
            return deployment::glass_background(content);
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
        let (mode_effects, mode_motion) = self.modes.effects(self.reduced_motion);
        let scene = scene::Scene {
            layout: self.layout.clone(),
            size: WORKBENCH,
            devices: self
                .devices
                .iter()
                .map(|d| scene::DeviceView {
                    phone: d.phone(),
                    frame: d.frame.clone(),
                    markers: d
                        .trace
                        .as_ref()
                        .zip(d.frame.as_deref())
                        .map_or([[0.; 4]; 5], |(t, f)| t.markers(f)),
                })
                .collect(),
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
            keyboard_target: self.keyboard_target,
        };
        let mut layers = vec![surface::Surface::new(scene).into()];
        let error_message = [
            self.error.as_str(),
            self.modes.voice_error(),
            self.modes.proactive_error(),
            self.devices.get(self.focus).map_or("", |d| d.audio.error()),
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
        let mut shared = row![];
        for panel in Self::NAV_PANELS {
            let (icon, active) = match panel {
                Panel::Devices => (Icon::Devices, false),
                Panel::Language | Panel::Agent => (Icon::Settings, false),
                Panel::Skills => (Icon::Wrench, false),
                Panel::Location => (Icon::Pin, self.location.enabled()),
                Panel::Proxy => (Icon::Network, self.proxy.enabled()),
                Panel::Knowledge => (Icon::Book, false),
                Panel::Files => (Icon::Folder, false),
                Panel::Todos | Panel::Goal => unreachable!(),
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
        let mut leading = row![].spacing(6).align_y(iced::Center);
        if self.camera != layout::Camera::default()
            || self.devices.iter().any(|d| d.offset != iced::Vector::ZERO)
        {
            leading = leading.push(sized_icon_button(
                Icon::LocateFixed,
                Some(Message::ResetBoard),
                36.,
                36.,
            ));
        }
        let leading = leading
            .push(container(error).width(Length::Fill).clip(true))
            .width(Length::Fill);
        let nav = row![
            leading,
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
        let board = self.layout.board;
        let mut board_widgets = vec![
            space::Space::new()
                .width(WORKBENCH.width)
                .height(WORKBENCH.height)
                .into(),
        ];
        for (index, (d, tile)) in self.devices.iter().zip(&self.layout.tiles).enumerate() {
            if !tile.visible() {
                continue;
            }
            let h = tile.header;
            if h.intersects(&board) && h.height >= 17. {
                let icon = if d.phone() {
                    Icon::PhonePortrait
                } else {
                    Icon::Screen
                };
                let status = if index == self.focus {
                    style::FOREGROUND
                } else {
                    iced::Color::from_rgb8(69, 79, 88)
                };
                let font = iced::Font {
                    weight: if index == self.focus {
                        iced::font::Weight::Semibold
                    } else {
                        iced::font::Weight::Normal
                    },
                    ..style::CJK_FONT
                };
                let label = row![
                    icon.view(),
                    text(&d.info.name)
                        .font(font)
                        .size((h.height * 0.55).clamp(12., 17.))
                        .color(status)
                        .wrapping(text::Wrapping::None)
                ]
                .spacing(7)
                .align_y(iced::Center);
                let label = container(label)
                    .align_y(iced::Center)
                    .clip(true)
                    .height(h.height)
                    .width(h.width)
                    .padding([0, 1]);
                board_widgets.push(pin(label).x(h.x).y(h.y).into());
            }
            if d.frame.is_none() && tile.screen.intersects(&board) {
                let status = if connection::reachable(&d.info) {
                    self.locale.tr("正在连接…")
                } else {
                    self.locale.tr("已断开")
                };
                let rect = tile.screen;
                board_widgets.push(
                    pin(container(text(status).size(13).color(style::MUTED))
                        .center(Length::Fill)
                        .width(rect.width)
                        .height(rect.height))
                    .x(rect.x)
                    .y(rect.y)
                    .into(),
                );
            }
        }
        if !self.layout.tiles.iter().any(layout::Tile::visible) {
            let add = button(
                row![Icon::Plus.view(), text(self.locale.tr("添加设备")).size(15)]
                    .spacing(8)
                    .align_y(iced::Center),
            )
            .padding([9, 14])
            .style(style::quiet)
            .on_press(Message::OpenPanel(Panel::Devices, true));
            board_widgets.push(
                pin(container(add)
                    .center_x(Length::Fill)
                    .width(self.layout.chat.x - 2. * layout::SIDE_INSET))
                .x(layout::SIDE_INSET)
                .y(board.center_y() - 20.)
                .into(),
            );
        }
        stage.insert(
            1,
            occlude::behind(
                occlude::within(stack(board_widgets).into(), board + self.canvas_origin),
                self.layout.chat + self.canvas_origin,
            ),
        );
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
fn run_workbench(
    #[cfg(target_os = "android")] activity: android_activity::AndroidApp,
) -> iced::Result {
    #[cfg(not(target_os = "android"))]
    {
        let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
        if let [mode, target, serial] = arguments.as_slice()
            && mode == "--share-android"
        {
            std::process::exit(peer_connection::android_cli(
                &target.to_string_lossy(),
                &serial.to_string_lossy(),
            ));
        }
        if let [mode, locale] = arguments.as_slice()
            && mode == "--share-device"
        {
            return sharing::window(locale::Locale::parse(&locale.to_string_lossy()));
        }
        #[cfg(unix)]
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
    }
    let program = iced::application(App::boot, App::update, App::view)
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
        .default_font(style::CJK_FONT);
    #[cfg(target_os = "android")]
    {
        // fontdb does not enumerate Android's system font directories.
        let mut program = program;
        for path in [
            "/system/fonts/Roboto-Regular.ttf",
            "/system/fonts/Roboto-RegularStatic.ttf",
            "/system/fonts/NotoSansCJK-Regular.ttc",
            "/system/fonts/NotoNaskhArabic-Regular.ttf",
            "/system/fonts/NotoSansArabic-Regular.ttf",
        ] {
            if let Ok(font) = std::fs::read(path) {
                program = program.font(font);
            }
        }
        android_keyboard::attach(activity.clone());
        let result = iced_winit::run_android(program, activity, android_keyboard::show);
        android_keyboard::detach();
        Ok(result?)
    }
    #[cfg(not(target_os = "android"))]
    program.run()
}
