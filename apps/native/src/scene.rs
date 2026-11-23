use crate::{
    Message,
    connection::{DeviceInput, Frame, Press},
    layout::Layout,
    scene_gpu,
};
use ai_phone_api::wire;
use iced::{Event, Point, Rectangle, Size, keyboard, mouse, touch, widget::shader};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct DeviceView {
    pub phone: bool,
    pub frame: Option<Arc<Frame>>,
    pub markers: [[f32; 4]; 5],
}
#[derive(Clone)]
pub struct Scene {
    pub layout: Layout,
    pub size: Size,
    pub devices: Vec<DeviceView>,
    pub popup: Rectangle,
    pub popup_opacity: f32,
    pub child_popup: Rectangle,
    pub child_popup_opacity: f32,
    pub mode_effects: [f32; 4],
    pub mode_motion: [f32; 4],
    pub keyboard_target: Option<usize>,
}

const POINTER_MS: u64 = 650;

pub struct AgentTrace {
    points: [[f32; 4]; 5],
    received: Instant,
}

impl AgentTrace {
    pub fn from_cursor(
        cursor: &wire::DeviceCursor,
        device: &wire::RegisteredDevice,
    ) -> Option<Self> {
        let agent = if device.kind == "phone" {
            cursor.actor == "agent"
        } else {
            uuid::Uuid::parse_str(&cursor.actor).is_ok()
        };
        if !agent
            || !cursor.input_confirmed
            || cursor.device_id != device.id
            || cursor.action_id.is_empty()
            || cursor.width == 0
            || cursor.height == 0
            || !matches!(cursor.status.as_str(), "running" | "completed")
        {
            return None;
        }
        let mut points = [[0.; 4]; 5];
        for (slot, point) in points
            .iter_mut()
            .zip(cursor.paths.iter().filter_map(|p| p.points.last()))
        {
            let age = cursor
                .reported_at_ms
                .saturating_sub(cursor.started_at_ms.saturating_add(u64::from(point.at_ms)));
            *slot = [
                (point.x as f32 + 0.5) / cursor.width as f32,
                (point.y as f32 + 0.5) / cursor.height as f32,
                POINTER_MS.saturating_sub(age) as f32,
                cursor.width as f32 / cursor.height as f32,
            ];
        }
        Some(Self {
            points,
            received: Instant::now(),
        })
    }

    pub fn markers(&self, frame: &Frame) -> [[f32; 4]; 5] {
        let elapsed = self.received.elapsed().as_secs_f32() * 1000.;
        let aspect = frame.width as f32 / frame.height as f32;
        self.points.map(|[x, y, remaining, captured_aspect]| {
            let opacity = if (aspect - captured_aspect).abs() < 0.05 {
                ((remaining - elapsed) / 160.).clamp(0., 1.)
            } else {
                0.
            };
            [x, y, opacity, 0.]
        })
    }
}
#[derive(Default)]
pub struct Pointer {
    pressed: Option<PointerPress>,
    pan: Option<BoardPress>,
    previous_tap: Option<(Point, Instant)>,
    modifiers: keyboard::Modifiers,
    touch_released: Option<(Point, Instant)>,
    pub anchor: Point,
    pub preedit: Option<iced::advanced::input_method::Preedit>,
}
struct BoardPress {
    source: PointerSource,
    start: Point,
    last: Point,
    moved: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum PointerSource {
    Mouse(mouse::Button),
    Touch(touch::Finger),
}
struct PointerPress {
    device: usize,
    id: String,
    from: Point,
    source: PointerSource,
    started: Instant,
    moved: Option<Instant>,
}

impl Scene {
    fn occludes_device(&self, point: Point) -> bool {
        !self.layout.board.contains(point)
            || self.layout.chat.contains(point)
            || self.popup.contains(point)
            || self.child_popup.contains(point)
    }
    fn begin_press(
        &self,
        state: &mut Pointer,
        position: Point,
        source: PointerSource,
    ) -> Option<shader::Action<Message>> {
        if self.occludes_device(position) {
            return None;
        }
        let hit = self.layout.device_at(position);
        if matches!(source, PointerSource::Mouse(mouse::Button::Middle)) || hit.is_none() {
            if matches!(source, PointerSource::Mouse(mouse::Button::Right)) {
                return None;
            }
            state.pan = Some(BoardPress {
                source,
                start: position,
                last: position,
                moved: false,
            });
            state.pressed = None;
            return Some(shader::Action::publish(Message::CanvasFocus).and_capture());
        }
        let (device, from) = hit?;
        state.previous_tap = None;
        state.anchor = position;
        state.pressed = Some(PointerPress {
            device,
            id: self.layout.tiles[device].id.clone(),
            from,
            source,
            started: Instant::now(),
            moved: None,
        });
        Some(shader::Action::publish(Message::ShowDevice(device)).and_capture())
    }
    fn pan_to(
        state: &mut Pointer,
        at: Point,
        source: PointerSource,
    ) -> Option<shader::Action<Message>> {
        let pan = state.pan.as_mut().filter(|p| p.source == source)?;
        if at == pan.last || !pan.moved && at.distance(pan.start) < 3. {
            return Some(shader::Action::capture());
        }
        let delta = at - pan.last;
        pan.last = at;
        pan.moved = true;
        state.previous_tap = None;
        Some(shader::Action::publish(Message::Pan(delta)).and_capture())
    }
    fn release_press(
        &self,
        state: &mut Pointer,
        position: Option<Point>,
        source: PointerSource,
    ) -> Option<shader::Action<Message>> {
        if state.pan.as_ref().is_some_and(|p| p.source == source) {
            let pan = state.pan.take()?;
            if let Some(at) = position {
                let now = Instant::now();
                if matches!(source, PointerSource::Touch(_)) {
                    state.touch_released = Some((at, now));
                }
                if !pan.moved
                    && at.distance(pan.start) < 4.
                    && !self.occludes_device(at)
                    && self.layout.device_at(at).is_none()
                    && !matches!(source, PointerSource::Mouse(mouse::Button::Middle))
                {
                    if state.previous_tap.take().is_some_and(|(p, t)| {
                        p.distance(at) < 7. && now.duration_since(t) < Duration::from_millis(380)
                    }) {
                        return Some(shader::Action::publish(Message::ResetBoard).and_capture());
                    }
                    state.previous_tap = Some((at, now));
                }
            }
            return Some(shader::Action::capture());
        }
        if state.pressed.as_ref()?.source != source {
            return None;
        }
        let press = state.pressed.take()?;
        let position = position?;
        let now = Instant::now();
        if matches!(source, PointerSource::Touch(_)) {
            state.touch_released = Some((position, now));
        }
        if self.occludes_device(position)
            || self
                .layout
                .tiles
                .get(press.device)
                .is_none_or(|t| t.id != press.id)
        {
            return Some(shader::Action::capture());
        }
        let to = self.layout.local(press.device, position)?;
        let frame = self.devices[press.device].frame.as_ref()?;
        let pixel = |point: Point| {
            (
                (point.x * frame.width as f32).min((frame.width - 1) as f32) as u32,
                (point.y * frame.height as f32).min((frame.height - 1) as f32) as u32,
            )
        };
        let (x, y) = pixel(press.from);
        let input = if press.from.distance(to) > 0.015 {
            let (to_x, to_y) = pixel(to);
            let (began, hold, longest) = match source {
                PointerSource::Mouse(_) => (press.started, true, 2000),
                PointerSource::Touch(_) => {
                    let moved = press.moved.unwrap_or(press.started);
                    (
                        moved,
                        moved.duration_since(press.started) >= Duration::from_millis(400),
                        650,
                    )
                }
            };
            DeviceInput::Drag {
                x,
                y,
                to_x,
                to_y,
                duration: now.duration_since(began).as_millis().clamp(120, longest) as u32,
                hold,
            }
        } else {
            let gesture = match source {
                PointerSource::Mouse(mouse::Button::Right) => Press::Right,
                _ if self.devices[press.device].phone
                    && now.duration_since(press.started) >= Duration::from_millis(500) =>
                {
                    Press::Hold
                }
                _ => Press::Left,
            };
            DeviceInput::Tap {
                x,
                y,
                press: gesture,
            }
        };
        Some(shader::Action::publish(Message::Input(press.device, input)).and_capture())
    }
}

impl shader::Program<Message> for Scene {
    type State = Pointer;
    type Primitive = scene_gpu::Primitive;
    fn update(
        &self,
        state: &mut Pointer,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        let origin = iced::Vector::new(bounds.x, bounds.y);
        let cursor = cursor - origin;
        let touch_active = state
            .pressed
            .as_ref()
            .is_some_and(|press| matches!(press.source, PointerSource::Touch(_)))
            || state
                .pan
                .as_ref()
                .is_some_and(|p| matches!(p.source, PointerSource::Touch(_)));
        if let Event::Mouse(mouse) = event
            && (touch_active
                || (matches!(
                    mouse,
                    mouse::Event::ButtonPressed(_) | mouse::Event::ButtonReleased(_)
                ) && state
                    .touch_released
                    .is_some_and(|(touch_position, released)| {
                        released.elapsed() < Duration::from_millis(120)
                            && cursor
                                .position()
                                .is_some_and(|p| p.distance(touch_position) < 8.)
                    })))
        {
            return Some(shader::Action::capture());
        }
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(
                button @ (mouse::Button::Left | mouse::Button::Right | mouse::Button::Middle),
            )) => {
                return self.begin_press(state, cursor.position()?, PointerSource::Mouse(*button));
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.pan.is_some() => {
                return Self::pan_to(state, cursor.position()?, state.pan.as_ref()?.source);
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers
            }
            Event::Window(iced::window::Event::Unfocused) => {
                state.pan = None;
                state.pressed = None;
                state.previous_tap = None;
                state.modifiers = keyboard::Modifiers::default();
            }
            Event::Mouse(mouse::Event::ButtonReleased(button)) => {
                return self.release_press(state, cursor.position(), PointerSource::Mouse(*button));
            }
            Event::Touch(touch::Event::FingerPressed { id, position }) => {
                if touch_active {
                    return Some(shader::Action::capture());
                }
                let position = *position - origin;
                if self.occludes_device(position) {
                    return None;
                }
                return self.begin_press(state, position, PointerSource::Touch(*id));
            }
            Event::Touch(touch::Event::FingerMoved { id, position }) => {
                if state.pan.is_some() {
                    return Self::pan_to(state, *position - origin, PointerSource::Touch(*id));
                }
                let press = state
                    .pressed
                    .as_mut()
                    .filter(|p| p.source == PointerSource::Touch(*id))?;
                if press.moved.is_none()
                    && self
                        .layout
                        .local(press.device, *position - origin)
                        .is_some_and(|point| press.from.distance(point) > 0.015)
                {
                    press.moved = Some(Instant::now());
                }
                return Some(shader::Action::capture());
            }
            Event::Touch(touch::Event::FingerLifted { id, position }) => {
                return self.release_press(
                    state,
                    Some(*position - origin),
                    PointerSource::Touch(*id),
                );
            }
            Event::Touch(touch::Event::FingerLost { id, position }) => {
                if state
                    .pan
                    .as_ref()
                    .is_some_and(|p| p.source == PointerSource::Touch(*id))
                {
                    state.pan = None;
                }
                if state
                    .pressed
                    .as_ref()
                    .is_some_and(|p| p.source == PointerSource::Touch(*id))
                {
                    state.pressed = None;
                }
                state.touch_released = Some((*position - origin, Instant::now()));
                return Some(shader::Action::capture());
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                state.previous_tap = None;
                let at = cursor.position()?;
                if self.occludes_device(at) {
                    return None;
                }
                let hit = self.layout.device_at(at);
                if hit.is_none() && !state.modifiers.command() && !state.modifiers.control() {
                    let horizontal = match delta {
                        mouse::ScrollDelta::Pixels { x, y } if x.abs() > y.abs() => {
                            Some(iced::Vector::new(*x, *y))
                        }
                        mouse::ScrollDelta::Lines { x, y } if state.modifiers.shift() => {
                            Some(iced::Vector::new((*x + *y) * 48., 0.))
                        }
                        _ => None,
                    };
                    if let Some(pan) = horizontal {
                        return Some(shader::Action::publish(Message::Pan(pan)).and_capture());
                    }
                }
                if hit.is_none() || state.modifiers.command() || state.modifiers.control() {
                    let dy = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => *y * 75.,
                        mouse::ScrollDelta::Pixels { y, .. } => *y,
                    };
                    return Some(shader::Action::publish(Message::Zoom(at, dy)).and_capture());
                }
                let (device, p) = hit?;
                let frame = self.devices[device].frame.as_ref()?;
                let amount = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 96.,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                return Some(
                    shader::Action::publish(Message::Input(
                        device,
                        DeviceInput::Scroll {
                            x: (p.x * frame.width as f32) as u32,
                            y: (p.y * frame.height as f32) as u32,
                            amount: amount as i32,
                        },
                    ))
                    .and_capture(),
                );
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text,
                modifiers,
                ..
            }) => {
                use keyboard::key::Named;
                let target = self.keyboard_target?;
                let control = modifiers.command() || modifiers.control();
                if self.devices[target].phone {
                    let phone_key = match key.as_ref() {
                        keyboard::Key::Character(value)
                            if control && value.eq_ignore_ascii_case("a") =>
                        {
                            Some("SELECT_ALL")
                        }
                        keyboard::Key::Named(name) if !control && !modifiers.alt() => match name {
                            Named::Enter => Some("ENTER"),
                            Named::Backspace => Some("DEL"),
                            Named::Delete => Some("FORWARD_DEL"),
                            Named::Escape => Some("BACK"),
                            Named::Tab => Some("TAB"),
                            Named::ArrowLeft => Some("DPAD_LEFT"),
                            Named::ArrowRight => Some("DPAD_RIGHT"),
                            Named::ArrowUp => Some("DPAD_UP"),
                            Named::ArrowDown => Some("DPAD_DOWN"),
                            _ => None,
                        },
                        _ => None,
                    };
                    let input = match phone_key {
                        Some(key) => DeviceInput::Key(key.into()),
                        None if !control && !modifiers.alt() => {
                            DeviceInput::Text(text.as_ref()?.to_string())
                        }
                        _ => return None,
                    };
                    return Some(
                        shader::Action::publish(Message::Input(target, input)).and_capture(),
                    );
                }
                if !self.devices[target].phone {
                    let named = match key.as_ref() {
                        keyboard::Key::Named(name) => match name {
                            Named::Enter => Some("Return"),
                            Named::Backspace => Some("BackSpace"),
                            Named::Delete => Some("Delete"),
                            Named::Escape => Some("Escape"),
                            Named::Tab => Some("Tab"),
                            Named::ArrowLeft => Some("Left"),
                            Named::ArrowRight => Some("Right"),
                            Named::ArrowUp => Some("Up"),
                            Named::ArrowDown => Some("Down"),
                            Named::Home => Some("Home"),
                            Named::End => Some("End"),
                            Named::PageUp => Some("Prior"),
                            Named::PageDown => Some("Next"),
                            _ => None,
                        },
                        _ => None,
                    };
                    if named.is_some() || control || modifiers.alt() {
                        let value = match (named, key.as_ref()) {
                            (Some(name), _) => name,
                            (_, keyboard::Key::Character(value)) => value,
                            _ => return None,
                        };
                        let mut chord = String::new();
                        if control {
                            chord.push_str("ctrl+");
                        }
                        if modifiers.alt() {
                            chord.push_str("alt+");
                        }
                        if modifiers.shift() {
                            chord.push_str("shift+");
                        }
                        chord.push_str(value);
                        return Some(
                            shader::Action::publish(Message::Input(
                                target,
                                DeviceInput::Key(chord),
                            ))
                            .and_capture(),
                        );
                    }
                }
                let input = DeviceInput::Text(text.as_ref()?.to_string());
                return Some(shader::Action::publish(Message::Input(target, input)).and_capture());
            }
            _ => {}
        }
        None
    }
    fn draw(&self, _: &Pointer, _: mouse::Cursor, _: Rectangle) -> scene_gpu::Primitive {
        scene_gpu::Primitive::new(self)
    }
    fn mouse_interaction(
        &self,
        state: &Pointer,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.pan.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if let Some(at) = cursor
            .position_from(bounds.position())
            .filter(|p| !self.occludes_device(*p))
        {
            return if self.layout.device_at(at).is_some() {
                mouse::Interaction::Pointer
            } else {
                mouse::Interaction::Grab
            };
        }
        mouse::Interaction::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::shader::Program;
    #[test]
    fn blank_canvas_gestures_and_device_input_are_disjoint() {
        let frame = Frame {
            width: 20,
            height: 40,
            pixels: Arc::new(vec![0; 3200]),
            sequence: 1,
            observation_id: "v".into(),
            captured_ms: 0.,
            received: Instant::now(),
        };
        let layout = Layout::home(crate::layout::WORKBENCH, [("one".into(), 0.5)]);
        let screen = layout.tiles[0].screen;
        let blank = layout.board.position() + iced::Vector::new(8., 8.);
        let chat = layout.chat.center();
        let scene = Scene {
            layout,
            size: crate::layout::WORKBENCH,
            devices: vec![DeviceView {
                phone: true,
                frame: Some(Arc::new(frame)),
                markers: [[0.; 4]; 5],
            }],
            popup: Rectangle::default(),
            child_popup: Rectangle::default(),
            popup_opacity: 0.,
            child_popup_opacity: 0.,
            mode_effects: [0.; 4],
            mode_motion: [0.; 4],
            keyboard_target: None,
        };
        let mut state = Pointer::default();
        let mut fire = |at, ev| {
            scene
                .update(
                    &mut state,
                    &Event::Mouse(ev),
                    Rectangle::with_size(scene.size),
                    mouse::Cursor::Available(at),
                )
                .and_then(|a| a.into_inner().0)
        };
        let press = mouse::Event::ButtonPressed(mouse::Button::Left);
        let release = mouse::Event::ButtonReleased(mouse::Button::Left);
        let wheel = mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0., y: 1. },
        };
        assert!(matches!(fire(blank,wheel),Some(Message::Zoom(p,_)) if p==blank));
        assert!(matches!(
            fire(screen.center(), wheel),
            Some(Message::Input(0, DeviceInput::Scroll { .. }))
        ));
        assert!(fire(chat, wheel).is_none());
        let to = blank + iced::Vector::new(34., 18.);
        assert!(matches!(fire(blank, press), Some(Message::CanvasFocus)));
        assert!(
            matches!(fire(to,mouse::Event::CursorMoved{position:to}),Some(Message::Pan(v)) if v==to-blank)
        );
        assert!(fire(to, release).is_none());
        for taps in 1..=2 {
            assert!(matches!(fire(blank, press), Some(Message::CanvasFocus)));
            assert_eq!(
                matches!(fire(blank, release), Some(Message::ResetBoard)),
                taps == 2
            );
        }
        for _ in 0..2 {
            assert!(matches!(
                fire(screen.center(), press),
                Some(Message::ShowDevice(0))
            ));
            assert!(matches!(
                fire(screen.center(), release),
                Some(Message::Input(0, DeviceInput::Tap { .. }))
            ));
        }
    }
    #[test]
    fn floating_chat_keeps_input_over_a_visible_part_of_a_device() {
        let home = Layout::home(crate::layout::WORKBENCH, [("pc".into(), 1.6)]);
        let at = Point::new(home.chat.x + 20., home.chat.center_y());
        let mut camera = crate::layout::Camera {
            pan: at - home.tiles[0].screen.center(),
            zoom: 1.,
        };
        let layout = home.project(&mut camera);
        assert!(layout.tiles[0].screen.contains(at));
        assert!(layout.chat.contains(at));
        assert!(layout.device_at(at).is_none());
        let visible = Point::new(layout.chat.x - 20., at.y);
        assert!(layout.device_at(visible).is_some());
        let scene = Scene {
            layout,
            size: crate::layout::WORKBENCH,
            devices: vec![DeviceView {
                phone: false,
                frame: None,
                markers: [[0.; 4]; 5],
            }],
            popup: Rectangle::default(),
            child_popup: Rectangle::default(),
            popup_opacity: 0.,
            child_popup_opacity: 0.,
            mode_effects: [0.; 4],
            mode_motion: [0.; 4],
            keyboard_target: None,
        };
        let mut state = Pointer::default();
        for ev in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
            mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0., y: 2. },
            },
        ] {
            assert!(
                scene
                    .update(
                        &mut state,
                        &Event::Mouse(ev),
                        Rectangle::with_size(scene.size),
                        mouse::Cursor::Available(at)
                    )
                    .is_none()
            );
        }
    }
}
