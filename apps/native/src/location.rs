use crate::{
    connection::Connection,
    controls::{Icon, action_button, field, icon_button},
    style,
};
use ai_phone_api::wire::{self, GeoPoint, LocationState};
use iced::{
    Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Subscription, Task, Theme,
    Vector, mouse,
    widget::{
        button, canvas, column, container, image, pick_list, row, space, stack, svg, text, toggler,
    },
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug)]
pub enum Message {
    Load,
    Loaded(Result<LocationState, String>),
    Saved(Result<LocationState, String>),
    Enabled(bool),
    Follow,
    Apply,
    Latitude(String),
    Longitude(String),
    Speed(String),
    Repeat(bool),
    Draw,
    Undo,
    Move,
    Pick(GeoPoint),
    Route(Vec<GeoPoint>),
    Pan(Vector),
    Zoom(i8),
    MapSize(Size),
    Tile(Tile, Result<image::Handle, String>),
    Attribution,
}
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Tile(u8, u32, u32);
pub struct Location {
    state: Option<LocationState>,
    latitude: String,
    longitude: String,
    speed: String,
    repeat: bool,
    route: Vec<GeoPoint>,
    edit: bool,
    dirty: bool,
    saving: bool,
    reading: bool,
    error: String,
    center: Option<GeoPoint>,
    zoom: u8,
    size: Size,
    tiles: HashMap<Tile, Option<image::Handle>>,
    tile_error: bool,
    cache: PathBuf,
    http: reqwest::Client,
}
impl Default for Location {
    fn default() -> Self {
        Self {
            state: None,
            latitude: String::new(),
            longitude: String::new(),
            speed: "5".into(),
            repeat: false,
            route: Vec::new(),
            edit: false,
            dirty: false,
            saving: false,
            reading: false,
            error: String::new(),
            center: None,
            zoom: 14,
            size: Size::ZERO,
            tiles: HashMap::new(),
            tile_error: false,
            cache: PathBuf::new(),
            http: reqwest::Client::builder()
                .user_agent(concat!("AImanity/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(10))
                .build()
                .expect("map HTTP client"),
        }
    }
}
impl Location {
    pub fn set_root(&mut self, home: &Path) {
        self.cache = home.join("user/map-tiles");
    }
    pub fn enabled(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.enabled)
    }
    pub fn subscription(&self) -> Subscription<Message> {
        if self.state.as_ref().is_some_and(|s| s.moving) {
            iced::time::every(Duration::from_secs(1)).map(|_| Message::Load)
        } else {
            Subscription::none()
        }
    }
    fn point(&self) -> Option<GeoPoint> {
        Some(GeoPoint {
            latitude: self.latitude.parse().ok()?,
            longitude: self.longitude.parse().ok()?,
            accuracy_m: 5.,
        })
    }
    fn save(
        &mut self,
        client: &Connection,
        enabled: bool,
        use_host: bool,
        moving: bool,
    ) -> Task<Message> {
        let Some(state) = &self.state else {
            return Task::none();
        };
        let speed = self.speed.parse::<f64>();
        let point = self.point();
        if enabled
            && !use_host
            && !point.is_some_and(|p| {
                (-90.0..=90.0).contains(&p.latitude) && (-180.0..=180.0).contains(&p.longitude)
            })
        {
            self.error = "请输入有效的经纬度".into();
            return Task::none();
        }
        let speed_kph = match speed {
            Ok(value) if (0.5..=500.).contains(&value) => value,
            _ if !enabled || use_host => 5.,
            _ => {
                self.error = "速度须在 0.5–500 km/h 之间".into();
                return Task::none();
            }
        };
        let request = wire::LocationUpdate {
            expected_revision: state.revision,
            request_id: uuid::Uuid::now_v7().to_string(),
            enabled,
            use_host,
            point: if use_host { None } else { point },
            route: if use_host {
                Vec::new()
            } else {
                self.route.clone()
            },
            speed_kph,
            moving,
            repeat: self.repeat,
        };
        self.saving = true;
        self.error.clear();
        let client = client.clone();
        Task::perform(
            async move { client.rpc("location/update", request).await },
            Message::Saved,
        )
    }
    fn accept(&mut self, state: LocationState, reset: bool) {
        if self
            .state
            .as_ref()
            .is_some_and(|s| s.revision > state.revision)
        {
            return;
        }
        if reset || !self.dirty {
            self.latitude = state
                .point
                .map_or(String::new(), |p| format!("{:.6}", p.latitude));
            self.longitude = state
                .point
                .map_or(String::new(), |p| format!("{:.6}", p.longitude));
            self.speed = if state.speed_kph > 0. {
                state.speed_kph.to_string()
            } else {
                "5".into()
            };
            self.route.clone_from(&state.route);
            self.repeat = state.repeat;
            self.dirty = false;
        }
        if reset || self.center.is_none() {
            self.center = state.point;
        }
        self.state = Some(state);
    }
    fn load_tiles(&mut self) -> Task<Message> {
        if !self.enabled() || self.size == Size::ZERO {
            return Task::none();
        }
        let Some(center) = self.center else {
            return Task::none();
        };
        let visible = visible_tiles(center, self.zoom, self.size);
        self.tiles
            .retain(|key, _| visible.iter().any(|(k, _)| k == key));
        let mut tasks = Vec::new();
        for (key, _) in visible {
            if let std::collections::hash_map::Entry::Vacant(slot) = self.tiles.entry(key) {
                slot.insert(None);
                let client = self.http.clone();
                let directory = self.cache.clone();
                tasks.push(Task::perform(
                    async move { tile(&client, &directory, key).await },
                    move |r| Message::Tile(key, r),
                ));
            }
        }
        Task::batch(tasks)
    }
    pub fn update(&mut self, message: Message, client: &Connection) -> Task<Message> {
        let moving = self.state.as_ref().is_some_and(|s| s.moving);
        match message {
            Message::Load if !self.reading && !self.saving => {
                self.reading = true;
                let client = client.clone();
                return Task::perform(
                    async move { client.rpc("location/state", wire::LocationRequest {}).await },
                    Message::Loaded,
                );
            }
            Message::Load => {}
            Message::Loaded(_) if self.saving => self.reading = false,
            Message::Loaded(r) | Message::Saved(r) => {
                let reset = std::mem::take(&mut self.saving);
                if !reset {
                    self.reading = false;
                }
                match r {
                    Ok(state) => {
                        self.error.clone_from(&state.error);
                        self.accept(state, reset);
                    }
                    Err(e) => self.error = e,
                }
                return self.load_tiles();
            }
            Message::Enabled(on) => {
                return self.save(client, on, on && self.point().is_none(), false);
            }
            Message::Follow => return self.save(client, true, true, false),
            Message::Apply => return self.save(client, true, false, false),
            Message::Move => return self.save(client, true, false, !moving),
            Message::Latitude(value) => {
                self.latitude = value;
                self.dirty = true;
            }
            Message::Longitude(value) => {
                self.longitude = value;
                self.dirty = true;
            }
            Message::Speed(value) => {
                self.speed = value;
                self.dirty = true;
            }
            Message::Repeat(value) => {
                self.repeat = value;
                self.dirty = true;
            }
            Message::Draw => self.edit = !self.edit,
            Message::Undo => {
                self.route.pop();
                self.dirty = true;
            }
            Message::Pick(point) => {
                self.latitude = format!("{:.6}", point.latitude);
                self.longitude = format!("{:.6}", point.longitude);
                self.dirty = true;
            }
            Message::Route(points) => {
                self.route = points;
                self.dirty = true;
            }
            Message::MapSize(size) => {
                self.size = size;
                return self.load_tiles();
            }
            Message::Zoom(change) => {
                self.zoom = (self.zoom as i8 + change).clamp(3, 19) as u8;
                return self.load_tiles();
            }
            Message::Pan(delta) => {
                if let Some(center) = self.center {
                    let (x, y) = project(center, self.zoom);
                    self.center = Some(inverse(
                        x - f64::from(delta.x),
                        y - f64::from(delta.y),
                        self.zoom,
                    ));
                    return self.load_tiles();
                }
            }
            Message::Tile(key, r) => {
                if let Some(slot) = self.tiles.get_mut(&key) {
                    match r {
                        Ok(handle) => {
                            *slot = Some(handle);
                            self.tile_error = false;
                        }
                        Err(_) => self.tile_error = true,
                    }
                }
            }
            Message::Attribution => {
                let _ = open::that_detached("https://www.openstreetmap.org/copyright");
            }
        }
        Task::none()
    }
    pub fn enable_action(&self) -> Element<'_, Message> {
        toggler(self.enabled())
            .on_toggle_maybe((self.state.is_some() && !self.saving).then_some(Message::Enabled))
            .into()
    }

    pub fn view(&self, locale: crate::locale::Locale) -> Element<'_, Message> {
        let enabled = self.enabled();
        let moving = self.state.as_ref().is_some_and(|s| s.moving);
        let editable = !self.saving && !moving;
        let mut body = column![].spacing(12);
        if enabled {
            body = body.push(
                row![
                    button(locale.tr("跟随宿主位置"))
                        .style(style::quiet)
                        .on_press_maybe(editable.then_some(Message::Follow)),
                    button(locale.tr("路线"))
                        .style(if self.edit {
                            style::primary
                        } else {
                            style::quiet
                        })
                        .on_press_maybe(editable.then_some(Message::Draw)),
                    icon_button(
                        Icon::Undo,
                        locale.tr("撤销路线点"),
                        (editable && self.edit && !self.route.is_empty()).then_some(Message::Undo)
                    )
                ]
                .spacing(6),
            );
            if self.state.as_ref().is_some_and(|s| s.source == "network") && !self.dirty {
                body = body.push(
                    text(locale.tr("宿主网络近似位置"))
                        .size(12)
                        .color(style::MUTED),
                );
            }
            if let Some(center) = self.center {
                let map = Map {
                    location: self,
                    center,
                    editable,
                };
                body = body
                    .push(
                        iced::widget::sensor(
                            stack![
                                canvas(Tiles {
                                    location: self,
                                    center
                                })
                                .width(Length::Fill)
                                .height(260),
                                canvas(map).width(Length::Fill).height(260)
                            ]
                            .clip(true),
                        )
                        .on_resize(Message::MapSize),
                    )
                    .push(
                        row![
                            button(text("© OpenStreetMap").size(11))
                                .style(style::quiet)
                                .on_press(Message::Attribution),
                            space::horizontal(),
                            button("−").style(style::quiet).on_press(Message::Zoom(-1)),
                            button("+").style(style::quiet).on_press(Message::Zoom(1))
                        ]
                        .spacing(4)
                        .align_y(iced::Center),
                    );
                if self.tile_error && self.tiles.values().all(Option::is_none) {
                    body = body.push(
                        text(locale.tr("地图暂不可用，可输入经纬度"))
                            .size(12)
                            .color(style::MUTED),
                    );
                }
            }
            body = body.push(
                row![
                    field(
                        locale.tr("纬度"),
                        &self.latitude,
                        Message::Latitude,
                        editable
                    ),
                    field(
                        locale.tr("经度"),
                        &self.longitude,
                        Message::Longitude,
                        editable
                    ),
                    action_button(
                        locale.tr("应用"),
                        (editable && (self.dirty || !self.error.is_empty()))
                            .then_some(Message::Apply)
                    )
                    .width(64)
                ]
                .spacing(8)
                .align_y(iced::Bottom),
            );
            if self.edit || !self.route.is_empty() {
                body = body.push(
                    row![
                        field(
                            locale.tr("速度 · km/h"),
                            &self.speed,
                            Message::Speed,
                            editable
                        ),
                        pick_list(
                            [locale.tr("单次"), locale.tr("循环")],
                            Some(if self.repeat {
                                locale.tr("循环")
                            } else {
                                locale.tr("单次")
                            }),
                            move |v| Message::Repeat(v == locale.tr("循环"))
                        )
                        .padding([10, 12])
                        .style(style::select)
                        .menu_style(style::menu),
                        icon_button(
                            if moving { Icon::Pause } else { Icon::Play },
                            if moving {
                                locale.tr("暂停")
                            } else {
                                locale.tr("开始移动")
                            },
                            (!self.saving && self.route.len() >= 2).then_some(Message::Move)
                        )
                    ]
                    .spacing(8)
                    .align_y(iced::Bottom),
                );
            }
        } else if self.state.is_some() {
            let locate = Icon::LocateFixed
                .view()
                .width(28)
                .height(28)
                .style(|theme, _| svg::Style {
                    color: Some(style::muted_color(theme)),
                });
            let enable = button(
                text(locale.tr("开启"))
                    .size(14)
                    .line_height(iced::Pixels(20.)),
            )
            .padding([8, 16])
            .style(style::primary)
            .on_press_maybe((!self.saving).then_some(Message::Enabled(true)));
            body = body.push(
                container(column![locate, enable].spacing(24).align_x(iced::Center))
                    .width(Length::Fill)
                    .align_x(iced::Center)
                    .padding([32, 0]),
            );
        }
        if let Some(state) = &self.state {
            body = body.push(
                row(state.devices.iter().map(|d| {
                    let name = if d.kind == "phone" {
                        locale.tr("手机")
                    } else {
                        locale.tr("电脑")
                    };
                    let status = match d.status.as_str() {
                        "applied" if d.kind == "desktop" => locale.tr("浏览器已同步"),
                        "applied" => locale.tr("已同步"),
                        "disabled" => locale.tr("已关闭"),
                        _ => locale.tr("未同步"),
                    };
                    text(format!("{name} · {status} {}", d.error))
                        .size(12)
                        .color(if d.error.is_empty() {
                            style::MUTED
                        } else {
                            style::ERROR
                        })
                        .into()
                }))
                .spacing(12),
            );
        }
        if self.saving || self.state.is_none() && self.reading {
            body = body.push(text(locale.tr("正在同步…")).size(12).color(style::MUTED));
        }
        if !self.error.is_empty() {
            body = body.push(text(&self.error).size(12).color(style::ERROR));
        }
        body.into()
    }
}

fn world(zoom: u8) -> f64 {
    f64::from(256u32 << zoom)
}
fn project(point: GeoPoint, zoom: u8) -> (f64, f64) {
    let width = world(zoom);
    let y = point
        .latitude
        .clamp(-85.0511, 85.0511)
        .to_radians()
        .tan()
        .asinh();
    (
        (point.longitude + 180.) / 360. * width,
        (1. - y / std::f64::consts::PI) / 2. * width,
    )
}
fn inverse(x: f64, y: f64, zoom: u8) -> GeoPoint {
    let width = world(zoom);
    GeoPoint {
        latitude: ((1. - 2. * (y / width).clamp(0., 1.)) * std::f64::consts::PI)
            .sinh()
            .atan()
            .to_degrees(),
        longitude: x.rem_euclid(width) / width * 360. - 180.,
        accuracy_m: 5.,
    }
}
fn visible_tiles(center: GeoPoint, zoom: u8, size: Size) -> Vec<(Tile, Rectangle)> {
    let (x, y) = project(center, zoom);
    let left = x - f64::from(size.width) / 2.;
    let top = y - f64::from(size.height) / 2.;
    let x0 = (left / 256.).floor() as i32;
    let y0 = (top / 256.).floor() as i32;
    let x1 = ((left + f64::from(size.width)) / 256.).ceil() as i32;
    let y1 = ((top + f64::from(size.height)) / 256.).ceil() as i32;
    let count = 1i32 << zoom;
    (y0.max(0)..y1.min(count))
        .flat_map(|i| {
            (x0..x1).map(move |j| {
                (
                    Tile(zoom, j.rem_euclid(count) as u32, i as u32),
                    Rectangle::new(
                        Point::new(
                            (f64::from(j) * 256. - left) as f32,
                            (f64::from(i) * 256. - top) as f32,
                        ),
                        Size::new(256., 256.),
                    ),
                )
            })
        })
        .collect()
}
async fn tile(client: &reqwest::Client, cache: &Path, key: Tile) -> Result<image::Handle, String> {
    use tokio::fs;
    let Tile(z, x, y) = key;
    let path = cache.join(format!("{z}-{x}-{y}.png"));
    let fresh = fs::metadata(&path)
        .await
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(604800));
    let bytes = if fresh {
        fs::read(&path).await.map_err(|e| e.to_string())?
    } else {
        let bytes = client
            .get(format!("https://tile.openstreetmap.org/{z}/{x}/{y}.png"))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?
            .to_vec();
        fs::create_dir_all(cache).await.map_err(|e| e.to_string())?;
        fs::write(path, &bytes).await.map_err(|e| e.to_string())?;
        bytes
    };
    let pixels = ::image::load_from_memory(&bytes)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    Ok(image::Handle::from_rgba(
        pixels.width(),
        pixels.height(),
        pixels.into_raw(),
    ))
}
struct Map<'a> {
    location: &'a Location,
    center: GeoPoint,
    editable: bool,
}
struct Tiles<'a> {
    location: &'a Location,
    center: GeoPoint,
}
impl canvas::Program<Message> for Tiles<'_> {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let mut frame = canvas::Frame::new(renderer, size);
        frame.fill_rectangle(Point::ORIGIN, size, Color::from_rgb8(235, 237, 233));
        frame.with_clip(Rectangle::new(Point::ORIGIN, size), |frame| {
            for (tile, rect) in visible_tiles(self.center, self.location.zoom, size) {
                if let Some(Some(handle)) = self.location.tiles.get(&tile) {
                    frame.draw_image(rect, iced::advanced::image::Image::new(handle.clone()));
                }
            }
        });
        vec![frame.into_geometry()]
    }
}
#[derive(Default)]
struct Gesture {
    start: Option<Point>,
    stroke: Vec<Point>,
}
impl Map<'_> {
    fn local(&self, point: GeoPoint, size: Size) -> Point {
        let (cx, cy) = project(self.center, self.location.zoom);
        let (x, y) = project(point, self.location.zoom);
        let width = world(self.location.zoom);
        let dx = (x - cx + width / 2.).rem_euclid(width) - width / 2.;
        Point::new(
            size.width / 2. + dx as f32,
            size.height / 2. + (y - cy) as f32,
        )
    }
    fn coordinate(&self, p: Point, size: Size) -> GeoPoint {
        let (cx, cy) = project(self.center, self.location.zoom);
        inverse(
            cx + f64::from(p.x - size.width / 2.),
            cy + f64::from(p.y - size.height / 2.),
            self.location.zoom,
        )
    }
}
impl canvas::Program<Message> for Map<'_> {
    type State = Gesture;
    fn update(
        &self,
        state: &mut Gesture,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if let Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) = event
            && let Some(start) = state.start.take()
        {
            let stroke = std::mem::take(&mut state.stroke);
            let Some(p) = cursor.position_in(bounds) else {
                return Some(canvas::Action::request_redraw());
            };
            let msg = if self.location.edit && self.editable {
                let mut route = self.location.route.clone();
                if stroke.len() <= 1
                    && let Some(index) = route
                        .iter()
                        .position(|p0| self.local(*p0, bounds.size()).distance(p) < 8.)
                {
                    route.remove(index);
                } else {
                    let step = (stroke.len() / 300).max(1);
                    route.extend(
                        stroke
                            .into_iter()
                            .step_by(step)
                            .chain([p])
                            .map(|p| self.coordinate(p, bounds.size())),
                    );
                    if route.len() > 600 {
                        route = route
                            .iter()
                            .step_by(route.len().div_ceil(599))
                            .copied()
                            .chain(route.last().copied())
                            .collect();
                    }
                }
                Message::Route(route)
            } else if start.distance(p) > 4. {
                if self.editable
                    && self
                        .location
                        .point()
                        .is_some_and(|point| self.local(point, bounds.size()).distance(start) < 11.)
                {
                    Message::Pick(self.coordinate(p, bounds.size()))
                } else {
                    Message::Pan(p - start)
                }
            } else if self.editable {
                Message::Pick(self.coordinate(p, bounds.size()))
            } else {
                return Some(canvas::Action::request_redraw());
            };
            return Some(canvas::Action::publish(msg).and_capture());
        }
        let p = cursor.position_in(bounds)?;
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                state.start = Some(p);
                state.stroke = vec![p];
                Some(canvas::Action::request_redraw().and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
                if self.location.edit && self.editable && state.start.is_some() =>
            {
                if state
                    .stroke
                    .last()
                    .is_some_and(|last| last.distance(p) > 4.)
                {
                    state.stroke.push(p);
                }
                Some(canvas::Action::request_redraw().and_capture())
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } | mouse::ScrollDelta::Pixels { y, .. } => {
                        *y
                    }
                };
                Some(
                    canvas::Action::publish(Message::Zoom(if y > 0. { 1 } else { -1 }))
                        .and_capture(),
                )
            }
            _ => None,
        }
    }
    fn draw(
        &self,
        state: &Gesture,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let mut frame = canvas::Frame::new(renderer, size);
        frame.with_clip(Rectangle::new(Point::ORIGIN, size), |frame| {
            let points: Vec<_> = self
                .location
                .route
                .iter()
                .map(|p| self.local(*p, size))
                .chain(
                    state
                        .stroke
                        .iter()
                        .copied()
                        .filter(|_| self.location.edit && self.editable),
                )
                .collect();
            let green = Color::from_rgb8(80, 116, 105);
            if let Some(first) = points.first() {
                frame.stroke(
                    &canvas::Path::new(|b| {
                        b.move_to(*first);
                        for p in points.iter().skip(1) {
                            b.line_to(*p);
                        }
                    }),
                    canvas::Stroke::default().with_color(green).with_width(3.),
                );
                for p in points {
                    frame.fill(&canvas::Path::circle(p, 4.), green);
                }
            }
            if let Some(point) = self.location.point() {
                let p = self.local(point, size);
                frame.fill(&canvas::Path::circle(p, 10.), Color::WHITE);
                frame.fill(&canvas::Path::circle(p, 7.), style::FOREGROUND);
            }
        });
        vec![frame.into_geometry()]
    }
    fn mouse_interaction(
        &self,
        _: &Gesture,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            if self.location.edit {
                mouse::Interaction::Crosshair
            } else {
                mouse::Interaction::Grab
            }
        } else {
            mouse::Interaction::None
        }
    }
}
