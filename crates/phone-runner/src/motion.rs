use crate::wire::{MotionTrack, PhoneBounds};

/// A sparse luminance image in screenshot coordinates, shared by observation and gesture cues.
pub(crate) struct Grid {
    area: PhoneBounds,
    step: u32,
    width: usize,
    height: usize,
    luma: Vec<u8>,
}

impl Grid {
    pub(crate) fn sample(
        area: PhoneBounds,
        screen_long_side: u32,
        pixel: impl Fn(u32, u32) -> [u8; 3],
    ) -> Self {
        let shorter = (area.right - area.left).min(area.bottom - area.top);
        let step = screen_long_side.div_ceil(160).min((shorter / 12).max(1));
        let width = (area.right - area.left).div_ceil(step) as usize;
        let height = (area.bottom - area.top).div_ceil(step) as usize;
        let mut luma = Vec::with_capacity(width * height);
        for y in (area.top..area.bottom).step_by(step as usize) {
            for x in (area.left..area.right).step_by(step as usize) {
                let [r, g, b] = pixel(x, y);
                luma.push(((u16::from(r) + 2 * u16::from(g) + u16::from(b)) / 4) as u8);
            }
        }
        Self {
            area,
            step,
            width,
            height,
            luma,
        }
    }

    pub(crate) fn tracks(&self, old: &Self) -> Vec<MotionTrack> {
        if self.area != old.area || self.width < 9 || self.height < 9 {
            return Vec::new();
        }
        let stride = self.width;
        let offsets = [
            0,
            2,
            4,
            2 * stride,
            2 * stride + 2,
            2 * stride + 4,
            4 * stride,
            4 * stride + 2,
            4 * stride + 4,
        ];
        let cols = (self.width - 4).div_ceil(4);
        let mut flow = vec![None; cols * (self.height - 4).div_ceil(4)];
        for y in (0..self.height - 4).step_by(4) {
            for x in (0..self.width - 4).step_by(4) {
                let at = y * stride + x;
                let patch = offsets.map(|i| self.luma[at + i]);
                let contrast = patch.iter().max().unwrap() - patch.iter().min().unwrap();
                let cost = |at: usize| -> u16 {
                    offsets
                        .iter()
                        .zip(patch)
                        .map(|(&i, v)| u16::from(old.luma[at + i].abs_diff(v)))
                        .sum()
                };
                let still = cost(at);
                if contrast < 24 || still < 95 {
                    continue;
                }
                let mut best = still;
                let mut next = still;
                let mut shift = (0, 0);
                for sy in y.saturating_sub(10)..=(y + 10).min(self.height - 5) {
                    for sx in x.saturating_sub(10)..=(x + 10).min(self.width - 5) {
                        let score = cost(sy * stride + sx);
                        if score < best {
                            next = best;
                            best = score;
                            shift = (x as i32 - sx as i32, y as i32 - sy as i32);
                        } else if score < next {
                            next = score;
                        }
                    }
                }
                if best < 160 && best * 2 < still && best + 8 < next {
                    flow[y / 4 * cols + x / 4] = Some(shift);
                }
            }
        }
        let mut groups = Vec::new();
        let mut stack = Vec::new();
        for start in 0..flow.len() {
            let Some(seed) = flow[start].take() else {
                continue;
            };
            stack.push((start, seed));
            let (mut left, mut top, mut right, mut bottom) = (self.width, self.height, 0, 0);
            let (mut n, mut dx, mut dy) = (0, 0, 0);
            while let Some((i, (vx, vy))) = stack.pop() {
                let (x, y) = (i % cols, i / cols);
                left = left.min(x * 4);
                top = top.min(y * 4);
                right = right.max(x * 4 + 5);
                bottom = bottom.max(y * 4 + 5);
                n += 1;
                dx += vx;
                dy += vy;
                for row in y.saturating_sub(1)..=(y + 1).min(flow.len() / cols - 1) {
                    for col in x.saturating_sub(1)..=(x + 1).min(cols - 1) {
                        let j = row * cols + col;
                        if let Some((bx, by)) = flow[j]
                            && (bx - seed.0).abs() <= 2
                            && (by - seed.1).abs() <= 2
                            && bx * seed.0 + by * seed.1 > 0
                        {
                            flow[j] = None;
                            stack.push((j, (bx, by)));
                        }
                    }
                }
            }
            if n >= 3 {
                let area = self.area;
                let bounds = PhoneBounds {
                    left: area.left + left as u32 * self.step,
                    top: area.top + top as u32 * self.step,
                    right: (area.left + right as u32 * self.step).min(area.right),
                    bottom: (area.top + bottom as u32 * self.step).min(area.bottom),
                };
                groups.push((
                    n,
                    MotionTrack {
                        bounds: Some(bounds),
                        dx: dx * self.step as i32 / n,
                        dy: dy * self.step as i32 / n,
                    },
                ));
            }
        }
        groups.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
        groups.into_iter().take(8).map(|(_, track)| track).collect()
    }
}

pub(crate) fn direction(track: &MotionTrack) -> &'static str {
    if track.dx.abs() > track.dy.abs() {
        if track.dx > 0 { "right" } else { "left" }
    } else if track.dy > 0 {
        "down"
    } else {
        "up"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_a_textured_object_and_ignores_static_background_and_global_dimming() {
        let frame = |left: u32, drop: u8| {
            Grid::sample(
                PhoneBounds {
                    left: 0,
                    top: 0,
                    right: 90,
                    bottom: 100,
                },
                100,
                |x, y| {
                    let v = if (left..left + 26).contains(&x) && (40..68).contains(&y) {
                        (50 + (((x - left) * 31 + y * 19 + (x - left) * y * 7) % 195)) as u8
                    } else {
                        40
                    };
                    [v.saturating_sub(drop); 3]
                },
            )
        };
        let before = frame(20, 0);
        let after = frame(25, 0);
        let tracks = after.tracks(&before);
        let object = tracks.iter().find(|t| direction(t) == "right").unwrap();
        let b = object.bounds.unwrap();
        assert!(b.left < 51 && b.right > 25 && b.top < 68 && b.bottom > 40);
        assert_eq!((object.dx, object.dy), (5, 0));
        assert!(before.tracks(&before).is_empty());
        assert!(frame(20, 12).tracks(&before).is_empty());
        assert_eq!(direction(&before.tracks(&after)[0]), "left");
    }
}
