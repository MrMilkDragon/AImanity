use iced::{Event, Rectangle, mouse, widget::canvas, window};
use std::time::{Duration, Instant};

pub fn orb<M: 'static>(size: f32, reduced_motion: bool) -> canvas::Canvas<Orb, M> {
    canvas::Canvas::new(Orb {
        compact: size < 30.,
        reduced_motion,
    })
    .width(size)
    .height(size)
}

pub fn label<'a, M: 'static>(
    text: &'a str,
    size: u32,
    busy: bool,
    reduced_motion: bool,
) -> iced::widget::Row<'a, M> {
    let mut row = iced::widget::row![].spacing(6).align_y(iced::Center);
    if busy {
        row = row.push(orb(15., reduced_motion));
    }
    row.push(iced::widget::text(text).size(size))
}
pub struct Orb {
    compact: bool,
    reduced_motion: bool,
}
impl<M> canvas::Program<M> for Orb {
    type State = Option<Instant>;
    fn update(
        &self,
        start: &mut Self::State,
        event: &Event,
        _: Rectangle,
        _: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        if self.reduced_motion {
            return None;
        }
        start.get_or_insert(*now);
        Some(canvas::Action::request_redraw_at(
            *now + Duration::from_millis(if self.compact { 42 } else { 34 }),
        ))
    }
    fn draw(
        &self,
        start: &Self::State,
        renderer: &iced::Renderer,
        theme: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::{
            Color, Point, Vector,
            widget::canvas::{Frame, Path},
        };
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.width;
        let unit = size * if self.compact { 0.25 } else { 0.235 };
        if !self.compact {
            frame.with_save(|frame| {
                frame.translate(Vector::new(size * 0.5, size * 0.875));
                frame.scale_nonuniform(Vector::new(1., 0.25));
                for ring in (1_u8..=16).rev() {
                    frame.fill(
                        &Path::circle(Point::ORIGIN, unit * 1.51 * f32::from(ring) / 16.),
                        Color::BLACK.scale_alpha(0.01),
                    );
                }
            });
        }
        let elapsed = if self.reduced_motion {
            0.
        } else {
            start.map_or(0., |start| start.elapsed().as_secs_f32())
        };
        let (sin_elevation, cos_elevation) = 0.49_f32.sin_cos();
        let yaw = -0.44 + elapsed * 0.91;
        let pulse = (elapsed * 3.7 - 1.95).cos();
        let return_fold = ((1. - pulse) / 2.).powi(7);
        let torsion = 0.43 * ((1. + pulse) / 2.).powi(6) - 0.29 * return_fold;
        let seam = -0.06 + 0.4 * return_fold;
        let last = if self.compact { 3 } else { 8 };
        let center_y = if self.compact { 0.5 } else { 0.455 };
        let coordinate = |index: u8| 2. * f32::from(index) / f32::from(last) - 1.;
        let normal =
            |index: u8| f32::from(u8::from(index == last)) - f32::from(u8::from(index == 0));
        for layer in 0..=last {
            let y = coordinate(layer);
            let (s, c) = (yaw + torsion * ((y - seam) * 4.).tanh()).sin_cos();
            let facing_y = normal(layer) * sin_elevation;
            for column in 0..=last {
                let x = coordinate(column);
                let facing_x = -normal(column) * s * cos_elevation;
                for row in 0..=last {
                    let facing = (normal(row) * c * cos_elevation)
                        .max(facing_x)
                        .max(facing_y);
                    if facing <= 0. {
                        continue;
                    }
                    let z = coordinate(row);
                    let depth = z * c - x * s;
                    let light = 0.66 + (depth * cos_elevation + y * sin_elevation) * 0.17;
                    let point = Point::new(
                        size * 0.5 + unit * (x * c + z * s),
                        size * center_y + unit * (depth * sin_elevation - y * cos_elevation),
                    );
                    frame.fill(
                        &Path::circle(
                            point,
                            size * if self.compact {
                                0.043
                            } else {
                                0.0051 + 0.0043 * light
                            },
                        ),
                        theme
                            .palette()
                            .text
                            .scale_alpha((0.35 + 0.65 * light) * (facing * 3.15).min(1.)),
                    );
                }
            }
        }
        vec![frame.into_geometry()]
    }
}
