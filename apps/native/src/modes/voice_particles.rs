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
    magnet: Option<Point>,
}

impl<M> canvas::Program<M> for Particles<'_> {
    type State = Motion;

    fn update(
        &self,
        state: &mut Motion,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
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
            state.magnet = None;
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
        let rest_y = bounds.height + 2.;
        let pointer = cursor.land().position_in(bounds);
        if let Some(from) = state
            .magnet
            .or_else(|| pointer.map(|p| Point::new(p.x, rest_y)))
        {
            let to = pointer.unwrap_or(Point::new(from.x, rest_y));
            state.magnet = Some(from + (to - from) * (1. - (-dt / 0.08).exp()));
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
                let mut point = Point::new(
                    x + arc * lift * (dx * 1.8 + ripple * 0.38),
                    bounds.height - 1.4 - row as f32 * pitch * 0.77 - arc * lift,
                );
                if seed < 0.36
                    && let Some(pointer) = state.magnet
                {
                    let reach = (1. - (point.x - pointer.x).abs() / 46.)
                        .clamp(0., 1.)
                        .powi(2);
                    let pull = (point.y - pointer.y).clamp(0., 28.) * reach;
                    point.x += (pointer.x - point.x) * pull / 46.;
                    point.y -= pull * 0.85;
                }
                let alpha = (rows - row as f32).min(1.)
                    * (0.48 + 0.52 * seed)
                    * (state.energy * 5.).min(1.);
                let side = pitch * (0.40 + 0.24 * seed);
                frame.fill_rectangle(
                    Point::new(point.x - side * 0.5, point.y - side),
                    Size::new(side, side),
                    color.scale_alpha(alpha),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}
