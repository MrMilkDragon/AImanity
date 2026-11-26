use super::MicrophoneLevels;
use iced::{Color, Event, Point, Rectangle, Size, mouse, widget::canvas, window};
use std::{
    f32::consts::TAU,
    time::{Duration, Instant},
};

pub(super) struct Particles<'a> {
    pub levels: Option<&'a MicrophoneLevels>,
    pub thinking: bool,
    pub reduced_motion: bool,
}

#[derive(Default)]
pub(super) struct Motion {
    last: Option<Instant>,
    energy: f32,
    phase: f32,
}

impl<M> canvas::Program<M> for Particles<'_> {
    type State = Motion;

    fn update(
        &self,
        state: &mut Motion,
        event: &Event,
        _bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        let voice = self
            .levels
            .map_or(0., |[.., a, b, c]| f32::from(*a.max(b).max(c)) / 255.);
        let target = (voice * 2.4)
            .min(1.)
            .max(if self.thinking { 0.23 } else { 0. });
        if target == 0. && state.energy == 0. {
            return None;
        }
        if self.reduced_motion {
            state.energy = target;
            state.phase = 0.;
            return None;
        }
        let dt = state.last.replace(*now).map_or(0.02, |last| {
            now.saturating_duration_since(last).as_secs_f32().min(0.05)
        });
        let tau = if target > state.energy { 0.028 } else { 0.14 };
        state.energy += (target - state.energy) * (1. - (-dt / tau).exp());
        if target == 0. && state.energy < 0.008 {
            *state = Motion::default();
            return None;
        }
        state.phase = (state.phase + dt * (0.25 + 0.86 * state.energy)).fract();
        Some(canvas::Action::request_redraw_at(
            *now + Duration::from_millis(if voice > 0. {
                17
            } else if target == 0. {
                25
            } else {
                67
            }),
        ))
    }

    fn draw(
        &self,
        state: &Motion,
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        if state.energy == 0. {
            return Vec::new();
        }
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let pitch = (bounds.width / 120.).clamp(2.8, 4.2);
        let t = TAU * state.phase;
        let center = 0.50 + 0.075 * t.sin();
        let palette = [
            Color::from_rgb8(19, 135, 190),
            Color::from_rgb8(4, 165, 158),
            Color::from_rgb8(99, 106, 220),
            Color::from_rgb8(215, 60, 146),
            Color::from_rgb8(228, 151, 51),
        ];
        for column in 0..(bounds.width / pitch) as u32 {
            let x = (column as f32 + 0.5) * pitch;
            let u = x / bounds.width;
            let dx = u - center;
            let envelope = (1. - (dx / 0.50).powi(2)).max(0.).powi(2);
            let level = self.levels.map_or(0., |samples| {
                let delay = (dx.abs() * 1.65).min(1.) * (samples.len() - 1) as f32;
                let bin = samples.len() - 1 - delay as usize;
                let recent = f32::from(samples[bin]);
                (recent + (f32::from(samples[bin.saturating_sub(1)]) - recent) * delay.fract())
                    / 255.
            });
            let ripple = (t * 2. - dx.abs() * 19. + dx * 3.).sin();
            let strength = state.energy * envelope;
            let rows = strength * (3.7 + 7.4 * level + 1.7 * ripple);
            let hue = (u + 0.12 * (t - dx * 3.).sin()).rem_euclid(1.) * palette.len() as f32;
            let a = palette[hue as usize % palette.len()];
            let b = palette[(hue as usize + 1) % palette.len()];
            let mix = hue.fract();
            let color = Color::from_rgb(
                a.r + (b.r - a.r) * mix,
                a.g + (b.g - a.g) * mix,
                a.b + (b.b - a.b) * mix,
            );
            for row in 0..rows.ceil().min(9.) as u32 {
                let seed = ((column * 73 + row * 193 + column * row * 11) % 97) as f32 / 97.;
                let flight = (state.phase + seed).fract();
                let arc = 4. * flight * (1. - flight);
                let top = row as f32 / rows.max(1.);
                let lift = strength * (0.7 + 7.8 * top * seed);
                let point = Point::new(
                    x + arc * lift * (dx * 1.8 + ripple * 0.38),
                    bounds.height - 1.4 - row as f32 * pitch * 0.77 - arc * lift,
                );
                let alpha = (rows - row as f32).min(1.)
                    * (0.48 + 0.52 * seed)
                    * (state.energy * 5.).min(1.);
                let side = pitch * (0.40 + 0.24 * seed);
                frame.fill(
                    &canvas::Path::circle(point, side * 0.44),
                    color.scale_alpha(alpha),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}
pub(super) struct Reveal<'a> {
    pub text: &'a str,
    pub font: iced::Font,
    pub reduced_motion: bool,
}

#[derive(Default)]
pub(super) struct Landing {
    text: String,
    started: Option<Instant>,
    progress: f32,
    target: Point,
}

impl<M> canvas::Program<M> for Reveal<'_> {
    type State = Landing;

    fn update(
        &self,
        state: &mut Landing,
        event: &Event,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        if self.reduced_motion {
            return None;
        }
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        if self.text != state.text {
            let first_changed = self
                .text
                .char_indices()
                .zip(state.text.chars())
                .take_while(|((_, new), old)| new == old)
                .map(|((at, c), _)| at + c.len_utf8())
                .last()
                .unwrap_or(0);
            state.text.clear();
            state.text.push_str(self.text);
            if first_changed < self.text.len() {
                use iced::advanced::text::{
                    Alignment, Paragraph, Renderer, Shaping, Span, Text, Wrapping,
                };
                let spans = [
                    Span::<()>::new(&self.text[..first_changed]),
                    Span::new(&self.text[first_changed..]),
                ];
                let paragraph = <iced::Renderer as Renderer>::Paragraph::with_spans(Text {
                    content: &spans,
                    bounds: Size::new(bounds.width, f32::INFINITY),
                    size: iced::Pixels(14.),
                    line_height: iced::Pixels(20.).into(),
                    font: self.font,
                    align_x: Alignment::Left,
                    align_y: iced::alignment::Vertical::Top,
                    shaping: Shaping::Advanced,
                    wrapping: Wrapping::WordOrGlyph,
                });
                if let Some(last) = paragraph.span_bounds(1).last() {
                    state.target =
                        Point::new((last.x + last.width).min(bounds.width - 1.), last.y + 9.);
                    state.started = Some(*now);
                }
            }
        }
        let elapsed = now.saturating_duration_since(state.started?).as_secs_f32();
        state.progress = (elapsed / 0.28).min(1.);
        if state.progress == 1. {
            state.started = None;
            return None;
        }
        Some(canvas::Action::request_redraw_at(
            *now + Duration::from_millis(16),
        ))
    }

    fn draw(
        &self,
        state: &Landing,
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        if self.reduced_motion || state.started.is_none() {
            return Vec::new();
        }
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let palette = [
            Color::from_rgb8(31, 143, 199),
            Color::from_rgb8(9, 172, 155),
            Color::from_rgb8(153, 111, 229),
            Color::from_rgb8(219, 113, 142),
        ];
        for index in 0..24 {
            let speed = (index % 7) as f32 / 14.;
            let fraction = (state.progress * (1. + speed)).min(1.);
            let ease = fraction * fraction * (3. - 2. * fraction);
            let x = state.target.x + (index as f32 - 11.5) * 4. * (1. - ease);
            let y = state.target.y + (bounds.height - state.target.y) * (1. - ease);
            let opacity = (1. - fraction).sqrt() * 0.8;
            let dot = canvas::Path::circle(Point::new(x, y), 0.6 + speed);
            frame.fill(&dot, palette[index % palette.len()].scale_alpha(opacity));
        }
        vec![frame.into_geometry()]
    }
}
