use iced::advanced::{
    Clipboard, Layout, Renderer as _, Shell, Widget, layout, renderer,
    widget::{Operation, Tree, operation::Focusable, tree},
};
use iced::{Border, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme};
use iced::{keyboard, mouse, touch, window};
use std::time::Instant;

const THUMB: Size = Size::new(12., 16.);
const ACTIVE: [f32; 2] = [1.28, 1.16];
const REST: [f32; 2] = [1., 1.];

pub struct SpringSlider<F> {
    limit: u8,
    value: u8,
    width: Length,
    active: bool,
    reduce_motion: bool,
    on_change: F,
}

impl<F> SpringSlider<F> {
    pub fn new(limit: u8, value: u8, on_change: F) -> Self {
        Self {
            limit,
            value,
            width: Length::Fill,
            active: true,
            reduce_motion: false,
            on_change,
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn reduce_motion(mut self, reduce: bool) -> Self {
        self.reduce_motion = reduce;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pointer {
    Mouse,
    Finger(touch::Finger),
}

#[derive(Default)]
struct State {
    pointer: Option<Pointer>,
    focused: bool,
    focus_visible: bool,
    motion: Option<Motion>,
}

impl Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }
    fn focus(&mut self) {
        self.focused = true;
        self.focus_visible = true;
    }
    fn unfocus(&mut self) {
        self.focused = false;
        self.focus_visible = false;
    }
}

struct Motion {
    from: [f32; 2],
    start: Instant,
    pressed: bool,
}

impl Motion {
    fn progress(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.start).as_secs_f32()
            / if self.pressed { 0.12 } else { 0.26 }
    }

    fn scale(&self, now: Instant) -> [f32; 2] {
        let t = self.progress(now).min(1.);
        let (from, to, position) = if self.pressed {
            (self.from, ACTIVE, t)
        } else if t <= 0.48 {
            (self.from, [0.94, 0.96], t / 0.48)
        } else if t <= 0.75 {
            ([0.94, 0.96], [1.04, 1.03], (t - 0.48) / 0.27)
        } else {
            ([1.04, 1.03], REST, (t - 0.75) / 0.25)
        };
        let eased = if self.pressed || t <= 0.48 {
            position
        } else if t <= 0.75 {
            1. - (1. - position).powi(3)
        } else {
            position * position * (3. - 2. * position)
        };
        std::array::from_fn(|axis| from[axis] + (to[axis] - from[axis]) * eased)
    }
}

impl State {
    fn scale(&self, now: Instant) -> [f32; 2] {
        self.motion.as_ref().map_or(
            if self.pointer.is_some() { ACTIVE } else { REST },
            |motion| motion.scale(now),
        )
    }

    fn grip(&mut self, pointer: Option<Pointer>, reduce_motion: bool) {
        let now = Instant::now();
        self.motion = (!reduce_motion).then(|| Motion {
            from: self.scale(now),
            start: now,
            pressed: pointer.is_some(),
        });
        self.pointer = pointer;
    }
}

impl<M, F: Fn(u8) -> M> Widget<M, Theme, Renderer> for SpringSlider<F> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Fixed(44.))
    }
    fn layout(&mut self, _: &mut Tree, _: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, self.width, Length::Fixed(44.))
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        _: &Renderer,
        operation: &mut dyn Operation,
    ) {
        if self.active {
            operation.focusable(None, layout.bounds(), tree.state.downcast_mut::<State>());
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            if let Some(motion) = &state.motion {
                if self.reduce_motion || motion.progress(*now) >= 1. {
                    state.motion = None;
                } else {
                    shell.request_redraw();
                }
            }
            return;
        }
        if !self.active {
            return;
        }

        let previous_value = self.value;
        let mut set = |value| {
            if value != self.value {
                self.value = value;
                shell.publish((self.on_change)(value));
            }
            shell.capture_event();
        };
        let inside = |point: Point| bounds.contains(point) && viewport.contains(point);
        let value_at = |point: Point| {
            let fraction =
                (point.x - bounds.x - THUMB.width / 2.) / (bounds.width - THUMB.width).max(1.);
            (fraction.clamp(0., 1.) * f32::from(self.limit)).round() as u8
        };
        let press = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                cursor.position().map(|p| (Pointer::Mouse, p))
            }
            Event::Touch(touch::Event::FingerPressed { id, position }) => {
                Some((Pointer::Finger(*id), *position))
            }
            _ => None,
        };
        if let Some((pointer, point)) = press {
            if state.pointer.is_none() && inside(point) {
                state.focused = true;
                state.focus_visible = false;
                state.grip(Some(pointer), self.reduce_motion);
                set(value_at(point));
                shell.request_redraw();
            } else if state.focused && !inside(point) {
                state.unfocus();
                shell.request_redraw();
            }
            return;
        }
        let release = match event {
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.pointer == Some(Pointer::Mouse)
            }
            Event::Touch(
                touch::Event::FingerLifted { id, .. } | touch::Event::FingerLost { id, .. },
            ) => state.pointer == Some(Pointer::Finger(*id)),
            Event::Window(window::Event::Unfocused) => state.pointer.is_some(),
            _ => false,
        };
        if release {
            state.grip(None, self.reduce_motion);
            shell.request_redraw();
            return;
        }
        let moved = match event {
            Event::Mouse(mouse::Event::CursorMoved { position })
                if state.pointer == Some(Pointer::Mouse) =>
            {
                Some(*position)
            }
            Event::Touch(touch::Event::FingerMoved { id, position })
                if state.pointer == Some(Pointer::Finger(*id)) =>
            {
                Some(*position)
            }
            _ => None,
        };
        if let Some(point) = moved {
            set(value_at(point));
        }
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key),
            ..
        }) = event
            && (state.focused || cursor.position().is_some_and(inside))
        {
            use keyboard::key::Named;
            let value = match key {
                Named::ArrowLeft | Named::ArrowDown => previous_value.saturating_sub(1),
                Named::ArrowRight | Named::ArrowUp => {
                    previous_value.saturating_add(1).min(self.limit)
                }
                Named::Home => 0,
                Named::End => self.limit,
                _ => return,
            };
            set(value);
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if !bounds.intersects(viewport) {
            return;
        }
        let state = tree.state.downcast_ref::<State>();
        let ink = theme
            .palette()
            .text
            .scale_alpha(if self.active { 1. } else { 0.5 });
        if self.active && state.focus_visible {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: Border {
                        color: ink,
                        width: 2.,
                        radius: crate::style::CONTROL_RADIUS.into(),
                    },
                    ..Default::default()
                },
                iced::Color::TRANSPARENT,
            );
        }
        let x = bounds.x + THUMB.width / 2.;
        let y = bounds.center_y();
        let rail_width = (bounds.width - THUMB.width).max(0.);
        let neutral = if theme.palette().background == crate::style::BACKGROUND {
            iced::Color::from_rgb8(222, 222, 222)
        } else {
            crate::style::accent(theme)
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(Point::new(x, y - 2.), Size::new(rail_width, 4.)),
                border: Border::default().rounded(1),
                ..Default::default()
            },
            neutral.scale_alpha(if self.active { 1. } else { 0.5 }),
        );
        let [sx, sy] = if self.reduce_motion {
            REST
        } else {
            state.scale(Instant::now())
        };
        let thumb = Size::new(THUMB.width * sx, THUMB.height * sy);
        let center = x + rail_width * f32::from(self.value) / f32::from(self.limit);
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(
                    Point::new(center - thumb.width / 2., y - thumb.height / 2.),
                    thumb,
                ),
                border: Border::default().rounded(4),
                ..Default::default()
            },
            ink,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        _: &Renderer,
    ) -> mouse::Interaction {
        if !self.active {
            return mouse::Interaction::None;
        }
        if tree.state.downcast_ref::<State>().pointer.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if cursor.is_over(layout.bounds()) && cursor.is_over(*viewport) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::None
        }
    }
}

impl<'a, M: 'a, F: Fn(u8) -> M + 'a> From<SpringSlider<F>> for Element<'a, M> {
    fn from(slider: SpringSlider<F>) -> Self {
        Self::new(slider)
    }
}
