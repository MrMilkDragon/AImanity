use ai_phone_api::wire::{DesktopActionRequest, HostDesktopReply};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use futures_util::{SinkExt, StreamExt};
use image::{DynamicImage, codecs::jpeg::JpegEncoder};
use prost::Message;
use std::{error::Error, fs, io, path::Path, time::Instant};
use tokio::sync::{oneshot, watch};
use tokio_tungstenite::tungstenite::Message as Frame;
use xcap::Monitor;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub enum Permission {
    Disabled,
    Off,
    Confirm,
    Armed,
    Active {
        request: String,
        _stop: watch::Sender<()>,
    },
}

impl Permission {
    const SETTING: &str = "user/host-desktop.enabled";

    pub fn load(home: &Path) -> Self {
        if home.join(Self::SETTING).is_file() {
            Self::Off
        } else {
            Self::Disabled
        }
    }

    pub fn enabled(&self) -> bool {
        !matches!(self, Self::Disabled)
    }

    pub fn configure(&mut self, home: &Path, enabled: bool) -> io::Result<()> {
        let path = home.join(Self::SETTING);
        if enabled {
            fs::write(path, [])?;
            *self = Self::Off;
        } else {
            *self = Self::Disabled;
            match fs::remove_file(path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                result => result?,
            }
        }
        Ok(())
    }

    pub fn close(&mut self) {
        if self.enabled() {
            *self = Self::Off;
        }
    }

    pub fn end(&mut self, request: &str) -> bool {
        if matches!(self, Self::Active { request: id, .. } if id == request) {
            *self = Self::Off;
            return true;
        }
        false
    }
}

pub(super) struct Views<T>(Vec<(String, Option<String>, T)>);
impl<T> Default for Views<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}
impl<T> Views<T> {
    pub fn remember(&mut self, request: &DesktopActionRequest, id: String, value: T) {
        // Manual previews can advance one frame while a click is in flight.
        if request.epoch == 2 {
            if self
                .0
                .iter()
                .filter(|(_, owner, _)| owner.is_none())
                .count()
                == 2
                && let Some(oldest) = self.0.iter().position(|(_, owner, _)| owner.is_none())
            {
                self.0.remove(oldest);
            }
        } else {
            self.0.retain(|(_, owner, _)| owner.is_none());
        }
        let owner = (request.epoch != 2).then(|| request.control_session.clone());
        self.0.push((id, owner, value));
    }
    pub fn take(&mut self, request: &DesktopActionRequest) -> Option<T> {
        let owner = (request.epoch != 2).then_some(request.control_session.as_str());
        let i = self.0.iter().position(|(id, source, _)| {
            *id == request.observation_id && source.as_deref() == owner
        })?;
        let (_, _, value) = self.0.swap_remove(i);
        self.0.clear();
        Some(value)
    }
}

struct View {
    id: String,
    display: String,
    origin: (i32, i32),
    size: (u32, u32),
    image: (u32, u32),
    focus: Option<usize>,
    at: Instant,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn focused_window() -> Option<usize> {
    xcap::Window::all()
        .ok()?
        .into_iter()
        .find(|w| w.is_focused().unwrap_or(false))?
        .id()
        .ok()
        .map(|id| id as usize)
}

#[cfg(windows)]
fn focused_window() -> Option<usize> {
    let window = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    (!window.is_null()).then_some(window as usize)
}

fn observe(display: &str) -> Result<(View, HostDesktopReply)> {
    let monitors = Monitor::all()?;
    let only_display = monitors.len() == 1;
    let mut displays = Vec::with_capacity(monitors.len());
    let mut selected = None;
    for monitor in monitors {
        let id = monitor.id()?.to_string();
        if id == display || (display.is_empty() && (only_display || monitor.is_primary()?)) {
            selected = Some((id.clone(), monitor));
        }
        displays.push(id);
    }
    let (id, monitor) = selected.ok_or("显示器已断开")?;
    let frame = DynamicImage::ImageRgba8(monitor.capture_image()?);
    let pixels = if frame.width() > 1440 || frame.height() > 1200 {
        frame.thumbnail(1440, 1200)
    } else {
        frame
    };
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 79).encode_image(&pixels)?;
    let view = View {
        id: uuid::Uuid::now_v7().to_string(),
        display: id.clone(),
        origin: (monitor.x()?, monitor.y()?),
        size: (monitor.width()?, monitor.height()?),
        image: (pixels.width(), pixels.height()),
        focus: focused_window(),
        at: Instant::now(),
    };
    let frame = HostDesktopReply {
        observation_id: view.id.clone(),
        jpeg,
        width: view.image.0,
        height: view.image.1,
        display: id,
        displays,
        ..Default::default()
    };
    Ok((view, frame))
}

fn key(input: &mut Enigo, name: &str) -> Result<()> {
    let command = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };
    let plain = match name {
        "volumedown" => Key::VolumeDown,
        "volumeup" => Key::VolumeUp,
        "volumemute" => Key::VolumeMute,
        "enter" => Key::Return,
        "tab" => Key::Tab,
        "escape" => Key::Escape,
        "backspace" => Key::Backspace,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "save" | "select_all" | "undo" | "redo" => {
            let redo = name == "redo" && cfg!(target_os = "macos");
            let c = match name {
                "save" => 's',
                "select_all" => 'a',
                "redo" if cfg!(windows) => 'y',
                _ => 'z',
            };
            input.key(command, Direction::Press)?;
            if redo {
                input.key(Key::Shift, Direction::Press)?;
            }
            input.key(Key::Unicode(c), Direction::Click)?;
            if redo {
                input.key(Key::Shift, Direction::Release)?;
            }
            input.key(command, Direction::Release)?;
            return Ok(());
        }
        _ => return Err("不支持的桌面按键".into()),
    };
    Ok(input.key(plain, Direction::Click)?)
}

#[cfg(target_os = "macos")]
fn mouse_event(
    source: &core_graphics::event_source::CGEventSource,
    kind: core_graphics::event::CGEventType,
    point: (i32, i32),
    clicks: i64,
) -> Result<()> {
    use core_graphics::{
        event::{
            CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField,
        },
        geometry::CGPoint,
    };
    let p = CGPoint::new(f64::from(point.0), f64::from(point.1));
    let button = if matches!(
        kind,
        CGEventType::RightMouseDown | CGEventType::RightMouseUp
    ) {
        CGMouseButton::Right
    } else {
        CGMouseButton::Left
    };
    let event = CGEvent::new_mouse_event(source.clone(), kind, p, button)
        .map_err(|_| "无法创建鼠标事件")?;
    event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, clicks);
    if matches!(kind, CGEventType::LeftMouseDragged) {
        event.set_flags(CGEventFlags::CGEventFlagNonCoalesced);
    }
    event.post(CGEventTapLocation::HID);
    Ok(())
}

#[cfg(target_os = "macos")]
fn pointer(action: &DesktopActionRequest, x: i32, y: i32) -> Result<()> {
    use core_graphics::{
        event::{CGEvent, CGEventTapLocation, CGEventType, ScrollEventUnit},
        event_source::{CGEventSource, CGEventSourceStateID},
        geometry::CGPoint,
    };
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| "无法取得鼠标输入源")?;
    let point = CGPoint::new(f64::from(x), f64::from(y));
    let post = |kind, clicks| mouse_event(&source, kind, (x, y), clicks);
    post(CGEventType::MouseMoved, 0)?;
    if action.kind == "scroll" {
        let event = CGEvent::new_scroll_event(
            source,
            ScrollEventUnit::LINE,
            1,
            -action.amount.clamp(-30, 30),
            0,
            0,
        )
        .map_err(|_| "无法创建滚动事件")?;
        event.set_location(point);
        event.post(CGEventTapLocation::HID);
    } else {
        let clicks = if action.kind == "double_click" { 2 } else { 1 };
        let (down, up) = if action.kind == "right_click" {
            (CGEventType::RightMouseDown, CGEventType::RightMouseUp)
        } else {
            (CGEventType::LeftMouseDown, CGEventType::LeftMouseUp)
        };
        for n in 1..=clicks {
            let n = if action.kind == "click" && action.clicks == 2 {
                2
            } else {
                n
            };
            post(down, n)?;
            post(up, n)?;
        }
    }
    Ok(())
}

#[cfg(any(windows, target_os = "linux"))]
fn pointer(action: &DesktopActionRequest, x: i32, y: i32) -> Result<()> {
    use enigo::{Axis, Button, Mouse};
    #[cfg(windows)]
    if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut input = Enigo::new(&Settings::default())?;
    #[cfg(target_os = "linux")]
    input.move_mouse(x, y, enigo::Coordinate::Abs)?;
    if action.kind == "scroll" {
        input.scroll(action.amount.clamp(-30, 30), Axis::Vertical)?;
    } else {
        let button = if action.kind == "right_click" {
            Button::Right
        } else {
            Button::Left
        };
        input.button(button, Direction::Click)?;
        if action.kind == "double_click" {
            input.button(Button::Left, Direction::Click)?;
        }
    }
    Ok(())
}

impl View {
    fn point(&self, x: i32, y: i32) -> Result<(i32, i32)> {
        let map = |p: i32, image: u32, size: u32, origin: i32| -> Result<i32> {
            let p = u32::try_from(p)?;
            if p >= image {
                return Err("操作点在显示器外".into());
            }
            Ok(origin + (u64::from(p) * u64::from(size) / u64::from(image)) as i32)
        };
        Ok((
            map(x, self.image.0, self.size.0, self.origin.0)?,
            map(y, self.image.1, self.size.1, self.origin.1)?,
        ))
    }
}

enum Pen {
    Down,
    Move,
    Up,
}

fn paint_strokes(
    current: &View,
    action: &DesktopActionRequest,
    live: &watch::Receiver<()>,
) -> Result<()> {
    if !(1..=32).contains(&action.strokes.len()) {
        return Err("A stroke action accepts 1–32 paths".into());
    }
    let paths = action
        .strokes
        .iter()
        .map(|path| {
            if !(2..=128).contains(&path.points.len()) {
                return Err("Each brush stroke needs 2–128 points".into());
            }
            path.points
                .iter()
                .map(|p| current.point(p.x, p.y))
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    drag_paths(current, paths, live)
}

fn drag_paths(
    current: &View,
    paths: impl IntoIterator<Item = Vec<(i32, i32)>>,
    live: &watch::Receiver<()>,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    let pen = {
        use core_graphics::{
            event::CGEventType as E,
            event_source::{CGEventSource, CGEventSourceStateID},
        };
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .map_err(|_| "无法取得鼠标输入源")?;
        move |p, phase| -> Result<()> {
            if matches!(phase, Pen::Down) {
                mouse_event(&source, E::MouseMoved, p, 0)?;
            }
            mouse_event(
                &source,
                match phase {
                    Pen::Down => E::LeftMouseDown,
                    Pen::Move => E::LeftMouseDragged,
                    Pen::Up => E::LeftMouseUp,
                },
                p,
                1,
            )?;
            // Drawing views sample the live pointer. Give each drag point a 120 Hz input interval.
            std::thread::sleep(std::time::Duration::from_micros(8333));
            Ok(())
        }
    };
    #[cfg(any(windows, target_os = "linux"))]
    let mut pen = {
        use enigo::{Button, Coordinate, Mouse};
        let mut input = Enigo::new(&Settings::default())?;
        move |(x, y), phase| -> Result<()> {
            match phase {
                Pen::Down => {
                    input.move_mouse(x, y, Coordinate::Abs)?;
                    input.button(Button::Left, Direction::Press)?;
                }
                Pen::Move => input.move_mouse(x, y, Coordinate::Abs)?,
                Pen::Up => input.button(Button::Left, Direction::Release)?,
            }
            Ok(())
        }
    };
    for path in paths {
        if current.focus.is_none() || current.focus != focused_window() {
            return Err("前台窗口已切换，请重新观察".into());
        }
        if live.has_changed().is_err() {
            return Err("本机授权已结束".into());
        }
        let mut current = path[0];
        pen(current, Pen::Down)?;
        let movement = path[1..].iter().try_for_each(|&next| -> Result<()> {
            if live.has_changed().is_err() {
                return Err("本机授权已结束".into());
            }
            pen(next, Pen::Move)?;
            current = next;
            Ok(())
        });
        let release = pen(current, Pen::Up);
        movement?;
        release?;
    }
    Ok(())
}

fn execute(
    view: &mut Option<View>,
    action: &DesktopActionRequest,
    live: &watch::Receiver<()>,
) -> Result<HostDesktopReply> {
    if live.has_changed().is_err() {
        return Err("本机授权已结束".into());
    }
    if action.kind == "foreground" {
        let active = xcap::Window::all()?
            .into_iter()
            .find(|w| w.is_focused().unwrap_or(false));
        let foreground = active
            .as_ref()
            .and_then(|w| w.title().ok())
            .unwrap_or_default();
        #[cfg(target_os = "linux")]
        let foreground = if foreground.is_empty()
            && let Some(window) = active
        {
            use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
            let (x11, _) = x11rb::connect(None)?;
            let name = x11
                .get_property(
                    false,
                    window.id()?,
                    AtomEnum::WM_NAME,
                    AtomEnum::ANY,
                    0,
                    200,
                )?
                .reply()?;
            String::from_utf8_lossy(&name.value).into_owned()
        } else {
            foreground
        };
        return Ok(HostDesktopReply {
            foreground: foreground.chars().take(200).collect(),
            ..Default::default()
        });
    }
    if action.kind == "observe" {
        let (next, reply) = observe(&action.device_id)?;
        *view = Some(next);
        return Ok(reply);
    }
    let current = view
        .take()
        .filter(|v| {
            v.id == action.observation_id
                && (action.device_id.is_empty() || action.device_id == v.display)
                && v.at.elapsed().as_secs() < 60
        })
        .ok_or("请先重新观察本机桌面")?;
    if live.has_changed().is_err() {
        return Err("本机授权已结束".into());
    }
    match action.kind.as_str() {
        "click" | "double_click" | "right_click" | "scroll" => {
            let (x, y) = current.point(action.x, action.y)?;
            pointer(action, x, y)?;
        }
        "drag" => {
            let start = current.point(action.x, action.y)?;
            let end = current.point(action.to_x, action.to_y)?;
            let middle = (start.0.midpoint(end.0), start.1.midpoint(end.1));
            drag_paths(&current, [vec![start, middle, end]], live)?;
        }
        "stroke" => paint_strokes(&current, action, live)?,
        "type" | "key" => {
            let send = || -> Result<()> {
                if live.has_changed().is_err() {
                    return Err("本机授权已结束".into());
                }
                if current.focus.is_none() || current.focus != focused_window() {
                    return Err("前台窗口已切换，请重新观察".into());
                }
                let mut input = Enigo::new(&Settings {
                    open_prompt_to_get_permissions: false,
                    ..Default::default()
                })
                .map_err(|e| format!("本机输入权限不可用：{e}"))?;
                if action.kind == "key" {
                    return key(&mut input, &action.key);
                }
                for line in action.text.split_inclusive('\n') {
                    let text = line.trim_end_matches('\n').trim_end_matches('\r');
                    if !text.is_empty() {
                        input.text(text)?;
                    }
                    if line.ends_with('\n') {
                        input.key(Key::Return, Direction::Click)?;
                    }
                }
                Ok(())
            };
            #[cfg(target_os = "macos")]
            {
                let mut result = Ok(());
                dispatch2::DispatchQueue::main().exec_sync(|| result = send());
                result?;
            }
            #[cfg(any(windows, target_os = "linux"))]
            send()?;
        }
        _ => return Err("不支持的本机操作".into()),
    }
    Ok(HostDesktopReply {
        display: current.display,
        ..Default::default()
    })
}

#[cfg(target_os = "macos")]
pub fn permission() -> Result<()> {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }
    if !unsafe { CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess() } {
        return Err("请在系统设置中允许 AImanity 录制屏幕后重试".into());
    }
    Enigo::new(&Settings::default()).map_err(|_| "请在系统设置中允许 AImanity 使用辅助功能")?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn permission() -> Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
    {
        return Err("Host desktop control requires an X11 session.".into());
    }
    Enigo::new(&Settings::default())?;
    Ok(())
}

#[cfg(windows)]
pub fn permission() -> Result<()> {
    Enigo::new(&Settings::default())?;
    Ok(())
}

pub(crate) struct Grant {
    view: std::sync::Mutex<Option<View>>,
    live: watch::Receiver<()>,
}
impl Grant {
    pub(crate) fn new(live: watch::Receiver<()>) -> Self {
        Self {
            view: Default::default(),
            live,
        }
    }
    pub(crate) fn active(&self) -> bool {
        self.live.has_changed().is_ok()
    }
    pub(crate) fn perform(
        &self,
        request: &DesktopActionRequest,
    ) -> std::result::Result<HostDesktopReply, String> {
        let mut reply = execute(&mut self.view.lock().unwrap(), request, &self.live)
            .map_err(|e| e.to_string())?;
        reply.action_id = request.action_id.clone();
        Ok(reply)
    }
}
pub(super) async fn serve_socket<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    mut socket: tokio_tungstenite::WebSocketStream<S>,
    mut phone: Option<crate::paired_phone::Phone>,
    services: crate::device_io::Services,
    ready: oneshot::Sender<()>,
    mut stop: watch::Receiver<()>,
) -> Result<()> {
    let first = tokio::select! {biased; _=stop.changed()=>return Ok(()), msg=tokio::time::timeout(std::time::Duration::from_secs(8),socket.next())=>msg?};
    let Some(Ok(Frame::Binary(first))) = first else {
        return Err("本机桌面已有使用中的任务".into());
    };
    HostDesktopReply::decode(first)?;
    if ready.send(()).is_err() {
        return Ok(());
    }
    let (authority, live) = watch::channel(());
    let mut views = Views::default();
    let mut streams = tokio::task::JoinSet::new();
    let (send_notice, mut notices) = tokio::sync::mpsc::channel(16);
    streams.spawn(crate::device_notices::watch(
        phone.as_ref().map(|p| p.client.clone()),
        send_notice,
    ));
    let mut notices_open = true;
    let mut session_location = None;
    loop {
        let received = tokio::select! {
            biased;
            _ = stop.changed() => break,
            _ = streams.join_next(), if !streams.is_empty() => continue,
            n=notices.recv(),if notices_open=>{if let Some(notification)=n{
                let item=HostDesktopReply{notification:Some(notification),..Default::default()};
                socket.send(Frame::Binary(item.encode_to_vec().into())).await?;
            }else{notices_open=false;}continue;},
            received = socket.next() => received,
        };
        let Some(msg) = received else {
            break;
        };
        let bytes = match msg? {
            Frame::Binary(bytes) => bytes,
            Frame::Close(_) => break,
            _ => continue,
        };
        let action = DesktopActionRequest::decode(bytes)?;
        let id = action.action_id.clone();
        let observing = action.kind == "observe";
        let result = if let Some(io) = action.io {
            let services = services.clone();
            let token = id.clone();
            streams.spawn(async move {
                if let Err(e) = services.serve(&token, io).await {
                    eprintln!("Device stream: {e}");
                }
            });
            Ok(HostDesktopReply::default())
        } else {
            let execute_input = async {
                if let Some(phone) = phone.as_mut() {
                    Ok(phone.perform(&action).await)
                } else if let Some(fix) = &action.location {
                    #[cfg(target_os = "linux")]
                    {
                        let result = async {
                            let response = reqwest::Client::new()
                                .post("http://127.0.0.1:17867/fix")
                                .body(fix.encode_to_vec())
                                .send()
                                .await?
                                .error_for_status()?
                                .bytes()
                                .await?;
                            let ack = ai_phone_api::wire::LocationDevice::decode(response)?;
                            if !ack.error.is_empty() {
                                return Err(ack.error.into());
                            }
                            if ack.status != if fix.enabled { "applied" } else { "disabled" } {
                                return Err("设备未确认定位".into());
                            }
                            Result::Ok(HostDesktopReply {
                                location: Some(*fix),
                                location_scope: "browser".into(),
                                ..Default::default()
                            })
                        }
                        .await;
                        Ok(result.map_err(|e| e.to_string()))
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        Ok(Ok(HostDesktopReply {
                            location: Some(*fix),
                            location_scope: "workspace".into(),
                            ..Default::default()
                        }))
                    }
                } else {
                    let live = live.clone();
                    let mut view = if action.kind == "observe" {
                        None
                    } else {
                        views.take(&action)
                    };
                    let (action, next, result) = tokio::task::spawn_blocking(move || {
                        let result = execute(&mut view, &action, &live).map_err(|e| e.to_string());
                        (action, view, result)
                    })
                    .await?;
                    if let Some(next) = next {
                        views.remember(&action, next.id.clone(), next);
                    }
                    Result::Ok(result)
                }
            };
            tokio::pin!(execute_input);
            let receipt = loop {
                tokio::select! {biased;
                    _=stop.changed()=>break None,
                    incoming=socket.next()=>match incoming.transpose()?{Some(Frame::Ping(_) | Frame::Pong(_))=>continue,_=>break None},
                    result=&mut execute_input=>break Some(result?),
                }
            };
            let Some(result) = receipt else {
                break;
            };
            result
        };
        let mut reply = result.unwrap_or_else(|error| HostDesktopReply {
            error,
            ..Default::default()
        });
        if let Some(fix) = reply.location {
            session_location = fix.enabled.then(|| (fix, reply.location_scope.clone()));
        } else if observing && let Some((fix, scope)) = &session_location {
            reply.location = Some(*fix);
            reply.location_scope.clone_from(scope);
        }
        reply.action_id = id;
        if stop.has_changed().is_err() {
            break;
        }
        socket
            .send(Frame::Binary(reply.encode_to_vec().into()))
            .await?;
    }
    drop(authority);
    streams.shutdown().await;
    let _ = socket.flush().await;
    let _ = socket.close(None).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_views_belong_to_the_observer_and_expire_after_input() {
        let mut frames = Views::default();
        let mut agent = DesktopActionRequest {
            control_session: "task A".into(),
            epoch: 1,
            ..Default::default()
        };
        let mut manual = DesktopActionRequest {
            epoch: 2,
            ..Default::default()
        };
        frames.remember(&agent, "model".into(), 1);
        for (id, frame) in [("first", 2), ("second", 3), ("third", 4)] {
            frames.remember(&manual, id.into(), frame);
        }
        agent.observation_id = "model".into();
        let other = DesktopActionRequest {
            control_session: "task B".into(),
            ..agent.clone()
        };
        assert_eq!(frames.take(&other), None);
        manual.observation_id = "first".into();
        assert_eq!(frames.take(&manual), None);
        manual.observation_id = "model".into();
        assert_eq!(frames.take(&manual), None);
        manual.observation_id = "second".into();
        assert_eq!(frames.take(&manual), Some(3));
        assert_eq!(frames.take(&agent), None);
    }

    #[test]
    fn desktop_access_is_local_and_grants_expire() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join("user")).unwrap();
        let mut access = Permission::load(home.path());
        assert!(!access.enabled());
        access.configure(home.path(), true).unwrap();
        assert!(matches!(Permission::load(home.path()), Permission::Off));
        let (stop, closed) = watch::channel(());
        access = Permission::Active {
            request: "local task".into(),
            _stop: stop,
        };
        access.configure(home.path(), false).unwrap();
        assert!(closed.has_changed().is_err());
        access.close();
        assert!(!access.enabled());
        assert!(!Permission::load(home.path()).enabled());
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "opens Notepad on an isolated Windows desktop"]
    async fn windows_notepad_receives_host_input() -> Result<()> {
        use crate::{peer_connection, workspace::Workspace};
        use ai_phone_api::wire::{DevicePairRequest, FileLocation};
        use std::{net::TcpListener, process::Command, sync::Arc, time::Duration};
        use tokio::time::{sleep, timeout};
        use windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

        let dir = tempfile::tempdir()?;
        let workspace = Workspace::open(dir.path())?;
        let port = TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
        let invitation = workspace
            .start_pairing(
                DevicePairRequest {
                    id: "remote".into(),
                    name: "Windows".into(),
                    kind: "desktop".into(),
                },
                port,
            )
            .await?;
        let (ready, connected) = oneshot::channel();
        let (revoke_share, share_stop) = watch::channel(());
        let peer = tokio::spawn(async move {
            peer_connection::share(&port.to_string(), &invitation.code, None, ready, share_stop)
                .await
        });
        connected.await?;
        let device = workspace.registry()?.devices.remove(0);
        assert!(device.connected);
        assert_eq!(device.system_name(), "Windows");
        let point = ai_phone_api::wire::GeoPoint {
            latitude: 32.,
            longitude: 118.,
            accuracy_m: 5.,
        };
        let update = ai_phone_api::wire::LocationUpdate {
            enabled: true,
            point: Some(point),
            speed_kph: 5.,
            request_id: "shared-position".into(),
            ..Default::default()
        };
        let received = workspace
            .rpc("location/update", update.encode_to_vec())
            .await?;
        let location = ai_phone_api::wire::LocationState::decode(received.as_slice())?;
        assert_eq!(location.devices[0].status, "workspace");
        let drives = workspace
            .file_list(FileLocation {
                device_id: device.id.clone(),
                path: "/".into(),
            })
            .await?;
        let volume = dir
            .path()
            .components()
            .next()
            .unwrap()
            .as_os_str()
            .to_string_lossy();
        assert!(
            drives
                .entries
                .iter()
                .any(|d| d.directory && d.name.eq_ignore_ascii_case(&volume))
        );

        let (revoke_host, live) = watch::channel(());
        *workspace.host.lock().unwrap() = Some(("local".into(), Arc::new(Grant::new(live))));
        for remote in [false, true] {
            let label = if remote { "paired" } else { "local" };
            let file = dir.path().join(format!("Fluorine-{label}.txt"));
            fs::write(&file, [])?;
            let mut editor = Command::new("notepad.exe").arg(&file).spawn()?;
            let result: Result<()> = async {
                let win = timeout(Duration::from_secs(12),async {
                    loop {
                        if let Some(win)=xcap::Window::all()?.into_iter().find(|w|
                             w.pid().ok()==Some(editor.id()) || w.title().is_ok_and(|title| title.contains(&format!("Fluorine-{label}")))) {
                            return Result::Ok(win);
                        }
                        sleep(Duration::from_millis(60)).await;
                    }
                }).await??;
                let hwnd=win.id()? as usize;
                unsafe { SetForegroundWindow(hwnd as _); }
                let expected=format!("Fluorine 中文 {label}");
                let call=async |mut action: DesktopActionRequest| -> Result<HostDesktopReply> {
                    action.action_id=uuid::Uuid::now_v7().to_string();
                    let expected_id=action.action_id.clone();
                    let result=if remote {
                        action.device_id=device.id.clone();
                        workspace.manual_input(action).await?
                    }else{
                        action.control_session="local".into();
                        workspace.host_action(action).await?
                    };
                    assert_eq!(result.action_id,expected_id);
                    Ok(result)
                };
                for (kind,text,key) in [("type", expected.as_str(), ""), ("key", "", "save")] {
                    assert_eq!(focused_window(),Some(hwnd));
                    let frame=call(DesktopActionRequest{kind:"observe".into(),..Default::default()}).await?;
                    assert!(!frame.jpeg.is_empty() && frame.width > 0);
                    if remote { assert_eq!(frame.location.and_then(|fix|fix.point),Some(point)); }
                    call(DesktopActionRequest{ kind:kind.into(),observation_id:frame.observation_id,text:text.into(),key:key.into(),..Default::default()}).await?;
                }
                timeout(Duration::from_secs(5),async {
                    while fs::read_to_string(&file)?.trim_start_matches('\u{feff}')!=expected {sleep(Duration::from_millis(45)).await;}
                    Result::Ok(())
                }).await??;
                println!("{label}: observed Windows, entered Chinese in Notepad and verified the saved file.");
                Ok(())
            }.await;
            let _ = editor.kill();
            let _ = editor.wait();
            result?;
        }
        drop(revoke_host);
        drop(revoke_share);
        peer.await??;
        workspace.stop().await;
        Ok(())
    }
}
