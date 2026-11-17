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
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let elapsed = if self.reduced_motion {
            0.
        } else {
            start.map_or(0., |start| start.elapsed().as_secs_f32())
        };
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let ink = theme.palette().text;
        let size = bounds.width;
        let yaw = (0.45 - elapsed * 0.66).sin_cos();
        let movement = slice_turn(elapsed);
        let (pitch_sin, pitch_cos) = 0.60_f32.sin_cos();
        let project = |point, slice_axis, slice_rotation| {
            let [x, y, z] = rotate(rotate(point, slice_axis, slice_rotation), 1, yaw);
            [
                x,
                z * pitch_sin - y * pitch_cos,
                z * pitch_cos + y * pitch_sin,
            ]
        };
        let basis = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        let fixed = basis.map(|axis| project(axis, 0, (0., 1.)));
        let turning = basis.map(|axis| project(axis, movement.axis, movement.rotation));
        let scale = size * if self.compact { 0.24 } else { 0.138 };
        let center = size * if self.compact { 0.5 } else { 0.475 };
        if !self.compact {
            ground(&mut frame, size, yaw, ink);
        }

        const SHADES: usize = 8;
        let mut dots: [canvas::path::Builder; SHADES] =
            std::array::from_fn(|_| canvas::path::Builder::new());
        let mut painted = [false; SHADES];
        for dot in lattice(self.compact) {
            let [[xx, xy, xz], [yx, yy, yz], [zx, zy, zz]] = if movement.applies(dot.position) {
                turning
            } else {
                fixed
            };
            let [a, b, c] = dot.faces;
            let facing = (a * xz).max(b * yz).max(c * zz).max(0.);
            if facing == 0. {
                continue;
            }
            let [a, b, c] = dot.position;
            let depth = a * xz + b * yz + c * zz;
            let perspective = 1. + depth * 0.075;
            let strength = (0.54 + 0.21 * depth) * (facing * 3.4).min(1.);
            let shade = (strength.clamp(0., 0.999) * SHADES as f32) as usize;
            let radius = size
                * if self.compact {
                    0.040
                } else {
                    0.0045 * perspective
                };
            let px = size * 0.5 + (a * xx + b * yx + c * zx) * scale * perspective;
            let py = center + (a * xy + b * yy + c * zy) * scale * perspective;
            let diagonal = radius * std::f32::consts::FRAC_1_SQRT_2;
            let dot = &mut dots[shade];
            dot.move_to(iced::Point::new(px + radius, py));
            for (dx, dy) in [
                (diagonal, diagonal),
                (0., radius),
                (-diagonal, diagonal),
                (-radius, 0.),
                (-diagonal, -diagonal),
                (0., -radius),
                (diagonal, -diagonal),
            ] {
                dot.line_to(iced::Point::new(px + dx, py + dy));
            }
            dot.close();
            painted[shade] = true;
        }
        for (index, path) in dots.into_iter().enumerate() {
            if painted[index] {
                frame.fill(
                    &path.build(),
                    ink.scale_alpha((index as f32 + 0.5) / SHADES as f32),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

struct Dot {
    position: [f32; 3],
    faces: [f32; 3],
}

fn lattice(compact: bool) -> &'static [Dot] {
    use std::sync::LazyLock;
    static FULL: LazyLock<Vec<Dot>> = LazyLock::new(|| build_lattice(8));
    static COMPACT: LazyLock<Vec<Dot>> = LazyLock::new(|| build_lattice(3));
    if compact { &COMPACT } else { &FULL }
}

fn build_lattice(last: u8) -> Vec<Dot> {
    let mut dots = Vec::new();
    for a in 0..=last {
        for b in 0..=last {
            for c in 0..=last {
                let index = [a, b, c];
                if index.into_iter().any(|n| n == 0 || n == last) {
                    dots.push(Dot {
                        position: index.map(|n| 2. * f32::from(n) / f32::from(last) - 1.),
                        faces: index.map(|n| {
                            if n == 0 {
                                -1.
                            } else if n == last {
                                1.
                            } else {
                                0.
                            }
                        }),
                    });
                }
            }
        }
    }
    dots
}

struct Slice {
    axis: usize,
    tier: f32,
    rotation: (f32, f32),
}

impl Slice {
    fn applies(&self, point: [f32; 3]) -> bool {
        (point[self.axis] - self.tier).abs() < 0.38
    }
}

fn slice_turn(elapsed: f32) -> Slice {
    const STEPS: [(usize, f32, f32); 6] = [
        (1, 0., 1.),
        (0, 0.75, -1.),
        (1, 0., -1.),
        (2, -0.75, 1.),
        (1, 0.75, 1.),
        (0, 0., 1.),
    ];
    const BEAT: f32 = 0.716;
    let time = (elapsed - 0.47).max(0.);
    let (axis, tier, direction) = STEPS[(time / BEAT) as usize % STEPS.len()];
    let t = ((time % BEAT) / 0.35).min(1.);
    let eased = t * t * (3. - 2. * t);
    let angle = if t == 1. {
        0.
    } else {
        direction * eased * std::f32::consts::FRAC_PI_2
    };
    Slice {
        axis,
        tier,
        rotation: angle.sin_cos(),
    }
}

fn rotate(mut point: [f32; 3], axis: usize, (sin, cos): (f32, f32)) -> [f32; 3] {
    let a = (axis + 2) % 3;
    let b = (axis + 1) % 3;
    let (u, v) = (point[a], point[b]);
    point[a] = cos * u - sin * v;
    point[b] = sin * u + cos * v;
    point
}

fn ground(frame: &mut canvas::Frame, size: f32, yaw: (f32, f32), ink: iced::Color) {
    let project =
        |x: f32, z: f32| iced::Point::new(size * (0.54 + x * 0.19), size * (0.78 + z * 0.090));
    let footprint = |x, z| {
        let [x, _, z] = rotate([x, 0., z], 1, yaw);
        project(x + z * 0.13, z)
    };
    let grid = canvas::Path::new(|path| {
        let project = |x, z| {
            let [x, _, z] = rotate([x, 0., z], 1, 0.78_f32.sin_cos());
            project(x, z)
        };
        for step in -3..=3 {
            let n = step as f32 * 0.40;
            path.move_to(project(n, -1.54));
            path.line_to(project(n, 1.54));
            path.move_to(project(-1.54, n));
            path.line_to(project(1.54, n));
        }
    });
    frame.stroke(
        &grid,
        canvas::Stroke::default()
            .with_width(0.6)
            .with_color(ink.scale_alpha(0.07)),
    );
    let points = canvas::Path::new(|path| {
        for line in -4..=4 {
            let n = line as f32 * 0.22;
            path.move_to(footprint(n, -0.89));
            path.line_to(footprint(n, 0.89));
            path.move_to(footprint(-0.89, n));
            path.line_to(footprint(0.89, n));
        }
    });
    for (width, alpha) in [(0.022, 0.022), (0.009, 0.04)] {
        frame.stroke(
            &points,
            canvas::Stroke::default()
                .with_width(size * width)
                .with_color(iced::Color::BLACK.scale_alpha(alpha)),
        );
    }
}
