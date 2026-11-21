use crate::wire::{MotionTrack, PhoneBounds};

pub(super) struct Target {
    area: PhoneBounds,
    pixels: [u8; 75],
    last: std::sync::Mutex<Option<PhoneBounds>>,
}

impl Target {
    pub fn new(area: PhoneBounds, pixels: [u8; 75]) -> Self {
        Self {
            area,
            pixels,
            last: Default::default(),
        }
    }

    pub fn locate(
        &self,
        destination: PhoneBounds,
        screen: (u32, u32),
        pixel: impl Fn(u32, u32) -> [u8; 3],
    ) -> Option<PhoneBounds> {
        let prior = *self.last.lock().unwrap();
        let anchor = prior.unwrap_or(self.area);
        let (w, h) = (anchor.right - anchor.left, anchor.bottom - anchor.top);
        let destination = prior.unwrap_or(destination);
        let (cx, cy) = (
            (anchor.left + anchor.right) / 2,
            (anchor.top + anchor.bottom) / 2,
        );
        let stride = if prior.is_some() {
            (w.min(h) / 12).clamp(2, 5)
        } else {
            (w.min(h) / 6).clamp(2, 12)
        };
        let mut best = u32::MAX;
        let mut found = None;
        for scale in [100, 75, 133, 56, 178, 38, 260] {
            let (sw, sh) = ((w * scale / 100).max(1), (h * scale / 100).max(1));
            if sw > screen.0 || sh > screen.1 {
                continue;
            }
            let xs = cx.min(destination.left).saturating_sub(w).max(sw / 2)
                ..=cx
                    .max(destination.right)
                    .saturating_add(w)
                    .min(screen.0 - sw + sw / 2);
            let ys = cy.min(destination.top).saturating_sub(h).max(sh / 2)
                ..=cy
                    .max(destination.bottom)
                    .saturating_add(h)
                    .min(screen.1 - sh + sh / 2);
            let evaluate = |x, y, limit| {
                let (l, t) = (x - sw / 2, y - sh / 2);
                let mut cost = 0;
                // Centre and corners reject unrelated texture before the other samples are read.
                for k in [
                    12, 0, 24, 4, 20, 7, 17, 10, 14, 1, 2, 3, 5, 6, 8, 9, 11, 13, 15, 16, 18, 19,
                    21, 22, 23,
                ] {
                    let p = pixel(
                        l + ((2 * (k % 5) + 1) * sw / 10),
                        t + ((2 * (k / 5) + 1) * sh / 10),
                    );
                    let i = k as usize * 3;
                    cost += u32::from(p[0].abs_diff(self.pixels[i]))
                        + u32::from(p[1].abs_diff(self.pixels[i + 1]))
                        + u32::from(p[2].abs_diff(self.pixels[i + 2]));
                    if cost >= limit {
                        break;
                    }
                }
                cost
            };
            if xs.contains(&cx) && ys.contains(&cy) && evaluate(cx, cy, 1) == 0 {
                best = 0;
                found = Some(PhoneBounds {
                    left: cx - sw / 2,
                    top: cy - sh / 2,
                    right: cx - sw / 2 + sw,
                    bottom: cy - sh / 2 + sh,
                });
                break;
            }
            let xstep = stride.max(xs.end().saturating_sub(*xs.start()).div_ceil(64)) as usize;
            let ystep = stride.max(ys.end().saturating_sub(*ys.start()).div_ceil(96)) as usize;
            let mut coarse = None;
            'coarse: for y in ys.clone().step_by(ystep) {
                for x in xs.clone().step_by(xstep) {
                    let cost = evaluate(x, y, best);
                    if cost < best {
                        best = cost;
                        coarse = Some((x, y));
                        if best == 0 {
                            break 'coarse;
                        }
                    }
                }
            }
            if let Some((x, y)) = coarse {
                let (mut bx, mut by) = (x, y);
                for ry in y.saturating_sub(stride).max(*ys.start())
                    ..=y.saturating_add(stride).min(*ys.end())
                {
                    for rx in x.saturating_sub(stride).max(*xs.start())
                        ..=x.saturating_add(stride).min(*xs.end())
                    {
                        let cost = evaluate(rx, ry, best);
                        if cost < best {
                            best = cost;
                            bx = rx;
                            by = ry;
                        }
                    }
                }
                let (left, top) = (bx - sw / 2, by - sh / 2);
                found = Some(PhoneBounds {
                    left,
                    top,
                    right: left + sw,
                    bottom: top + sh,
                });
                if best == 0 {
                    break;
                }
            }
        }
        if best > 25 * 3 * 29 {
            found = None;
        }
        *self.last.lock().unwrap() = found;
        found
    }
}

pub(super) fn crossing(
    old: PhoneBounds,
    now: PhoneBounds,
    dt_ms: u32,
    zone: PhoneBounds,
    direction: &str,
    lead_ms: u32,
) -> Option<(MotionTrack, u32)> {
    let center = |b: PhoneBounds| {
        (
            ((b.left + b.right) / 2) as i32,
            ((b.top + b.bottom) / 2) as i32,
        )
    };
    let (x, y) = center(now);
    let (ox, oy) = center(old);
    let (dx, dy) = (x - ox, y - oy);
    let track = MotionTrack {
        bounds: Some(now),
        dx,
        dy,
    };
    if (dx.abs().max(dy.abs()) < 2) || crate::motion::direction(&track) != direction || dt_ms == 0 {
        return None;
    }
    // Intersect the measured trajectory with both axes of the acceptance rectangle.
    let (mut enter, mut leave) = (0.0_f64, f64::from(lead_ms));
    for (pos, delta, lo, hi) in [
        (x, dx, zone.left, zone.right),
        (y, dy, zone.top, zone.bottom),
    ] {
        if delta == 0 {
            if pos < lo as i32 || pos >= hi as i32 {
                return None;
            }
        } else {
            let time = |boundary: u32| {
                (f64::from(boundary) - f64::from(pos)) * f64::from(dt_ms) / f64::from(delta)
            };
            let (a, b) = (time(lo), time(hi));
            enter = enter.max(a.min(b));
            leave = leave.min(a.max(b));
        }
    }
    (enter <= leave).then_some((track, enter.ceil() as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_a_resizing_selection_in_both_directions() {
        let bounds = |side: u32| PhoneBounds {
            left: 48 - side / 2,
            right: 48 - side / 2 + side,
            top: 48 - side / 2,
            bottom: 48 - side / 2 + side,
        };
        let ink = |x: u32, y: u32| [(x * 53) as u8, (y * 53) as u8, ((x + y) % 2 * 245) as u8];
        let patch = std::array::from_fn(|i| ink((i / 3 % 5) as u32, (i / 15) as u32)[i % 3]);
        for sizes in [[32, 24, 18], [32, 42, 56]] {
            let target = Target::new(bounds(32), patch);
            for side in sizes {
                let area = bounds(side);
                let found = target
                    .locate(bounds(64), (96, 96), |x, y| {
                        if (area.left..area.right).contains(&x)
                            && (area.top..area.bottom).contains(&y)
                        {
                            ink((x - area.left) * 5 / side, (y - area.top) * 5 / side)
                        } else {
                            [220, 220, 220]
                        }
                    })
                    .unwrap();
                assert!((found.right - found.left).abs_diff(side) <= 2);
            }
        }
    }

    #[test]
    fn forecast_requires_the_selected_texture_and_an_intersecting_trajectory() {
        let bounds = |x, y| PhoneBounds {
            left: x,
            top: y,
            right: x + 10,
            bottom: y + 10,
        };
        let pattern = |x: u32, y: u32| {
            [
                if (x / 2 + y / 2).is_multiple_of(2) {
                    180
                } else {
                    22
                },
                (x * 22) as u8,
                (y * 18) as u8,
            ]
        };
        let mut pixels = [0; 75];
        for (n, sample) in pixels.chunks_exact_mut(3).enumerate() {
            sample.copy_from_slice(&pattern((n % 5 * 2 + 1) as u32, (n / 5 * 2 + 1) as u32));
        }
        let target = Target::new(bounds(24, 64), pixels);
        let zone = bounds(24, 28);
        let frame = |y0: u32| {
            target.locate(zone, (80, 100), |x, y| {
                if (24..34).contains(&x) && (y0..y0 + 10).contains(&y) {
                    pattern(x - 24, y - y0)
                } else {
                    [180, 0, 0]
                }
            })
        };
        let old = frame(58).unwrap();
        let now = frame(50).unwrap();
        assert_eq!(now, bounds(24, 50));
        let (_, time) = crossing(old, now, 50, zone, "up", 120).unwrap();
        assert!((105..=110).contains(&time));
        assert!(crossing(old, now, 50, zone, "up", 80).is_none());
        assert!(crossing(now, old, 50, zone, "up", 200).is_none());
        assert!(crossing(old, now, 50, bounds(45, 28), "up", 200).is_none());
        assert!(target.locate(zone, (80, 100), |_, _| [180, 0, 0]).is_none());
    }
}
