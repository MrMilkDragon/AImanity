use iced::{Point, Rectangle, Size};
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub devices: [Rectangle; 2],
    pub chat: Rectangle,
    pub angles: [f32; 2],
}
fn fit(area: Rectangle, aspect: f32) -> Rectangle {
    let width = area.width.min(area.height * aspect);
    let height = width / aspect;
    Rectangle {
        x: area.x + (area.width - width) / 2.,
        y: area.y + (area.height - height) / 2.,
        width,
        height,
    }
}
impl Layout {
    pub fn target(size: Size, focus: usize, linked: bool, aspects: [f32; 2]) -> Self {
        let w = size.width;
        let h = size.height;
        let wide = focus == 1 || aspects[0] > 1. || linked || w < 850.;
        if wide {
            let stage_h = ((h - 100.) * 0.43).min(420.);
            let stage = Rectangle {
                x: 32.,
                y: 88.,
                width: w - 64.,
                height: stage_h,
            };
            let mut devices = [Rectangle::default(); 2];
            if linked {
                let ph = fit(
                    Rectangle {
                        width: stage.width * 0.27,
                        ..stage
                    },
                    aspects[0],
                );
                let pc = fit(
                    Rectangle {
                        x: stage.x + stage.width * 0.30,
                        width: stage.width * 0.70,
                        ..stage
                    },
                    aspects[1],
                );
                devices = [ph, pc];
            } else {
                devices[focus] = fit(
                    Rectangle {
                        x: stage.x + stage.width * 0.16,
                        width: stage.width * 0.84,
                        ..stage
                    },
                    aspects[focus],
                );
                devices[1 - focus] = fit(
                    Rectangle {
                        width: stage.width * 0.14,
                        height: stage.height * 0.75,
                        y: stage.y + stage.height * 0.12,
                        ..stage
                    },
                    aspects[1 - focus],
                );
            }
            Self {
                devices,
                chat: Rectangle {
                    x: 32.,
                    y: stage.y + stage.height + 24.,
                    width: w - 64.,
                    height: (h - stage.y - stage.height - 48.).max(160.),
                },
                angles: if linked {
                    [0., 0.]
                } else if focus == 0 {
                    [0., -0.16]
                } else {
                    [0.16, 0.]
                },
            }
        } else {
            let width = (w * 0.51).min(w - 380.);
            let primary = fit(
                Rectangle {
                    x: width * 0.18,
                    y: 94.,
                    width: width * 0.76,
                    height: h - 130.,
                },
                aspects[0],
            );
            let back = fit(
                Rectangle {
                    x: 16.,
                    y: h * 0.32,
                    width: width * 0.3,
                    height: h * 0.38,
                },
                aspects[1],
            );
            Self {
                devices: [primary, back],
                chat: Rectangle {
                    x: width + 16.,
                    y: 80.,
                    width: w - width - 48.,
                    height: h - 104.,
                },
                angles: [0., -0.18],
            }
        }
    }
    pub fn blend(self, target: Self, t: f32) -> Self {
        fn rect(a: Rectangle, b: Rectangle, t: f32) -> Rectangle {
            Rectangle {
                x: a.x + (b.x - a.x) * t,
                y: a.y + (b.y - a.y) * t,
                width: a.width + (b.width - a.width) * t,
                height: a.height + (b.height - a.height) * t,
            }
        }
        Self {
            devices: [
                rect(self.devices[0], target.devices[0], t),
                rect(self.devices[1], target.devices[1], t),
            ],
            chat: rect(self.chat, target.chat, t),
            angles: [
                self.angles[0] + (target.angles[0] - self.angles[0]) * t,
                self.angles[1] + (target.angles[1] - self.angles[1]) * t,
            ],
        }
    }
    pub fn local(&self, index: usize, p: Point) -> Option<Point> {
        let r = self.devices[index];
        let sx = p.x - r.center_x();
        let sy = p.y - r.center_y();
        let a = self.angles[index];
        let x = sx / (a.cos() - sx * a.sin() / 1000.);
        let y = sy * (1. + x * a.sin() / 1000.);
        let q = Point::new(x / r.width + 0.5, y / r.height + 0.5);
        (q.x >= 0. && q.x <= 1. && q.y >= 0. && q.y <= 1.).then_some(q)
    }
}
