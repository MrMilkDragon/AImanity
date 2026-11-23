use iced::{Point, Rectangle, Size};

pub const WORKBENCH: Size = Size::new(1440., 900.);

pub fn fit_canvas(window: Size, canvas: Size) -> (f32, iced::Vector) {
    let scale = (window.width / canvas.width).min(window.height / canvas.height);
    let origin = iced::Vector::new(
        ((window.width / scale - canvas.width) * 0.5).max(0.),
        ((window.height / scale - canvas.height) * 0.5).max(0.),
    );
    (scale, origin)
}

const PERSPECTIVE: f32 = 1800.;
pub const STAGE_TOP: f32 = 60.;
pub const SIDE_INSET: f32 = 24.;
pub const CHAT_PADDING: [f32; 2] = [14., 18.];

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub devices: [Rectangle; 2],
    pub toolbars: [Rectangle; 2],
    pub chat: Rectangle,
    pub angles: [f32; 2],
    pub device_radius: [f32; 2],
    pub cable: f32,
    pub cable_end: f32,
}

#[cfg(test)]
#[test]
fn input_uses_the_visible_screen_in_each_orientation() {
    for phone in [0.5, 2.] {
        let aspects = [phone, 1.6];
        let layout = Layout::target(Size::new(1440., 900.), 0, true, aspects);
        for (i, aspect) in aspects.into_iter().enumerate() {
            let rect = layout.devices[i];
            let inset = layout.device_radius[i] / 3.;
            let ratio = (rect.width - 2. * inset) / (rect.height - 2. * inset);
            assert!((ratio - aspect).abs() < 1e-5);
            assert_eq!(layout.local(i, rect.center()), Some(Point::new(0.5, 0.5)));
            assert!(
                layout
                    .local(i, Point::new(rect.x + inset, rect.y + inset))
                    .is_none()
            );
        }
    }
}

impl Layout {
    pub fn target(size: Size, focus: usize, linked: bool, aspects: [f32; 2]) -> Self {
        let wide = linked || focus == 1 || aspects[0] > 1.;
        let (top, gap) = if wide { (60., 18.) } else { (66., 24.) };
        let inset = SIDE_INSET;
        let bottom = 20.;
        let grid_width = size.width - 2. * inset;
        let grid_height = size.height - top - bottom;
        let stage_width = if wide {
            grid_width
        } else {
            (grid_width - gap) * (1.15 / 2.15)
        };
        let stage_height = if wide {
            let share = if !linked {
                0.4
            } else if aspects[0] <= 1. {
                0.44
            } else {
                0.38
            };
            (grid_height * share).clamp(180., 420.)
        } else {
            grid_height
        };
        let stage_center = inset + stage_width * 0.5;
        let linked_column = size.width / 3.;
        let mut devices = [Rectangle::default(); 2];
        let mut toolbars = [Rectangle::default(); 2];
        let mut angles = [0.; 2];
        let mut device_radius = [0.; 2];
        for index in 0..2 {
            let rear = !linked && index != focus;
            let rear_phone = wide && rear && index == 0;
            let pane_width = if linked {
                linked_column - inset
            } else if rear_phone {
                (stage_width * 0.32).min(1000.)
            } else if wide {
                (stage_width * 0.76).min(1000.)
            } else {
                stage_width * 0.86
            };
            let header_height = if rear { 40. } else { 44. };
            let header_space = if linked { 64. } else { 56. };
            let aspect = aspects[index];
            let max_height = stage_height - header_space;
            let fit_width = pane_width.min(max_height * aspect);
            let radius = (fit_width.min(fit_width / aspect) * 0.05).min(crate::style::PANEL_RADIUS);
            device_radius[index] = radius;
            let borders = 2. * radius / 3.;
            let content_width = (pane_width - borders).min((max_height - borders) * aspect);
            let width = content_width + borders;
            let height = content_width / aspect + borders;
            let screen_top = if index == 1 {
                top + (stage_height + header_height + 12. - height) * 0.5
            } else {
                top + header_height + 12.
            };
            let screen_center = if linked {
                stage_center + (index as f32 - 0.5) * linked_column
            } else {
                stage_center
            };
            let screen_left = screen_center - width * 0.5;
            devices[index] = Rectangle {
                x: screen_left,
                y: screen_top,
                width,
                height,
            };
            let toolbar_width = if index == 0 { 156. } else { 124. };
            toolbars[index] = Rectangle {
                x: if linked {
                    screen_left - (toolbar_width - width).max(0.) * 0.5
                } else {
                    screen_left
                },
                y: top,
                width: toolbar_width,
                height: header_height,
            };
            if rear {
                let side = if index == 0 { -1. } else { 1. };
                let (shift, scale) = if rear_phone {
                    (1.05, 0.66)
                } else {
                    (0.52, 0.44)
                };
                let origin_x = size.width * if wide { 0.5 } else { 0.3 };
                let origin_y =
                    STAGE_TOP + (size.height - STAGE_TOP) * if wide { 0.25 } else { 0.5 };
                let depth = PERSPECTIVE / (PERSPECTIVE + 35.);
                let x = origin_x + (stage_center + pane_width * shift * side - origin_x) * depth;
                let y = origin_y
                    + (top
                        + stage_height * 0.5
                        + (devices[index].center_y() - top - stage_height * 0.5) * scale
                        - origin_y)
                        * depth;
                let scale = scale * depth;
                devices[index] = Rectangle {
                    x: x - width * scale * 0.5,
                    y: y - height * scale * 0.5,
                    width: width * scale,
                    height: height * scale,
                };
                angles[index] = -side * 10_f32.to_radians();
                device_radius[index] *= scale;
            }
        }
        let chat = if wide {
            Rectangle {
                x: inset,
                y: top + stage_height + gap,
                width: grid_width,
                height: grid_height - stage_height - gap,
            }
        } else {
            Rectangle {
                x: inset + stage_width + gap,
                y: top,
                width: grid_width - stage_width - gap,
                height: grid_height,
            }
        };
        Self {
            devices,
            toolbars,
            chat,
            angles,
            device_radius,
            cable: if linked { 1. } else { 0. },
            cable_end: if linked { 1. } else { 0. },
        }
    }

    pub fn blend(self, target: Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        let rect = |a: Rectangle, b: Rectangle| Rectangle {
            x: mix(a.x, b.x),
            y: mix(a.y, b.y),
            width: mix(a.width, b.width),
            height: mix(a.height, b.height),
        };
        Self {
            devices: std::array::from_fn(|i| rect(self.devices[i], target.devices[i])),
            toolbars: std::array::from_fn(|i| rect(self.toolbars[i], target.toolbars[i])),
            chat: rect(self.chat, target.chat),
            angles: std::array::from_fn(|i| mix(self.angles[i], target.angles[i])),
            device_radius: std::array::from_fn(|i| {
                mix(self.device_radius[i], target.device_radius[i])
            }),
            cable: mix(self.cable, target.cable),
            cable_end: mix(self.cable_end, target.cable_end),
        }
    }

    pub fn device_at(
        &self,
        focus: usize,
        include_rear: bool,
        point: Point,
    ) -> Option<(usize, Point)> {
        if self.chat.contains(point) || point.y < STAGE_TOP {
            return None;
        }
        match self.local(focus, point) {
            Some(point) => Some((focus, point)),
            None if include_rear => self.local(1 - focus, point).map(|point| (1 - focus, point)),
            None => None,
        }
    }

    pub fn local(&self, index: usize, p: Point) -> Option<Point> {
        let r = self.devices[index];
        let sx = p.x - r.center_x();
        let sy = p.y - r.center_y();
        let a = self.angles[index];
        let x = sx / (a.cos() - sx * a.sin() / PERSPECTIVE);
        let y = sy * (1. + x * a.sin() / PERSPECTIVE);
        let radius = self.device_radius[index];
        let bezel = radius / 3.;
        let dx = (x.abs() - (r.width * 0.5 - radius)).max(0.);
        let dy = (y.abs() - (r.height * 0.5 - radius)).max(0.);
        let inner = radius - bezel;
        if dx * dx + dy * dy > inner * inner {
            return None;
        }
        let q = Point::new(
            x / (r.width - 2. * bezel) + 0.5,
            y / (r.height - 2. * bezel) + 0.5,
        );
        Some(q)
    }
}
