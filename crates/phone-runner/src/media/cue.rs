use super::{Bytes, Media, State, StatusCode, hub, wire};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, stream::BoxStream};
use prost::Message;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::timeout;

mod target;
use target::{Target, crossing};

pub(super) struct Cue {
    target: Option<Target>,
    source: (u32, u32),
    region: wire::PhoneBounds,
    clear_region: Option<(wire::PhoneBounds, Vec<[u8; 3]>)>,
    palette: Vec<[u8; 3]>,
    percent: u32,
    any_color: bool,
    moving: bool,
}

pub(super) enum Sample {
    Color(bool),
    Target(Option<wire::PhoneBounds>, bool),
    Motion(crate::motion::Grid, bool),
}

impl Cue {
    pub(super) fn sample(&self, size: (u32, u32), pixel: impl Fn(u32, u32) -> [u8; 3]) -> Sample {
        if u64::from(size.0) * u64::from(self.source.1)
            != u64::from(size.1) * u64::from(self.source.0)
        {
            return Sample::Color(false);
        }
        let appearance = self.matches(size, &pixel);
        if let Some(target) = &self.target {
            return Sample::Target(
                target.locate(self.region, self.source, |x, y| {
                    pixel(
                        (u64::from(x) * u64::from(size.0) / u64::from(self.source.0)) as u32,
                        (u64::from(y) * u64::from(size.1) / u64::from(self.source.1)) as u32,
                    )
                }),
                appearance,
            );
        }
        if self.moving {
            let grid = crate::motion::Grid::sample(
                self.region,
                self.source.0.max(self.source.1),
                |x, y| {
                    pixel(
                        (u64::from(x) * u64::from(size.0) / u64::from(self.source.0)) as u32,
                        (u64::from(y) * u64::from(size.1) / u64::from(self.source.1)) as u32,
                    )
                },
            );
            Sample::Motion(grid, appearance)
        } else {
            Sample::Color(appearance)
        }
    }
    pub(super) fn matches(&self, size: (u32, u32), pixel: impl Fn(u32, u32) -> [u8; 3]) -> bool {
        (self.palette.is_empty()
            || self.occupied(self.region, size, &pixel, &self.palette, self.any_color))
            && self
                .clear_region
                .as_ref()
                .is_none_or(|(area, colors)| !self.occupied(*area, size, &pixel, colors, true))
    }

    fn occupied(
        &self,
        area: wire::PhoneBounds,
        (w, h): (u32, u32),
        pixel: &impl Fn(u32, u32) -> [u8; 3],
        colors: &[[u8; 3]],
        any: bool,
    ) -> bool {
        let scale =
            |v: u32, to: u32, from: u32| (u64::from(v) * u64::from(to) / u64::from(from)) as u32;
        let x = scale(area.left, w, self.source.0)..scale(area.right, w, self.source.0);
        let y = scale(area.top, h, self.source.1)..scale(area.bottom, h, self.source.1);
        if x.is_empty() || y.is_empty() {
            return false;
        }
        let dx = (x.end - x.start).div_ceil(40) as usize;
        let dy = (y.end - y.start).div_ceil(40) as usize;
        let per_row = x.len().div_ceil(dx);
        let mut remaining = per_row * y.len().div_ceil(dy);
        let required = (remaining * self.percent as usize).div_ceil(100);
        let mut hits = [0usize; 8];
        for row in y.step_by(dy) {
            for col in x.clone().step_by(dx) {
                let p = pixel(col, row);
                for (i, color) in colors.iter().enumerate() {
                    if p.iter().zip(color).all(|(a, b)| a.abs_diff(*b) <= 24) {
                        hits[i] += 1;
                    }
                }
            }
            remaining -= per_row;
            let coverage = hits[1..colors.len()].iter().fold(hits[0], |n, &count| {
                if any { n.max(count) } else { n.min(count) }
            });
            if coverage >= required || coverage + remaining < required {
                return coverage >= required;
            }
        }
        false
    }
}

pub(super) type Samples<'a> = BoxStream<'a, super::Result<(Sample, Instant)>>;

fn samples<'a>(media: &'a Media, cue: Arc<Cue>, on_appearance: bool) -> Samples<'a> {
    #[cfg(target_os = "macos")]
    if let Some(avo) = &media.avo {
        return avo.cue_samples(media, cue, on_appearance);
    }
    let mut last = match &*media.screen.frames.borrow() {
        hub::Frame::Image(frame) if !on_appearance => Some(frame.captured),
        _ => None,
    };
    media
        .screen
        .pictures()
        .filter_map(move |frame| {
            let fresh = last.replace(frame.captured) != Some(frame.captured);
            let cue = Arc::clone(&cue);
            async move {
                if !fresh {
                    return None;
                }
                Some(
                    tokio::task::spawn_blocking(move || -> super::Result<_> {
                        let image = image::load_from_memory_with_format(
                            &frame.jpeg,
                            image::ImageFormat::Jpeg,
                        )?
                        .into_rgb8();
                        Ok((
                            cue.sample(image.dimensions(), |x, y| image.get_pixel(x, y).0),
                            frame.published,
                        ))
                    })
                    .await
                    .map_err(Into::into)
                    .and_then(|sample| sample),
                )
            }
        })
        .boxed()
}

pub(super) async fn watch(State(media): State<Arc<Media>>, body: Bytes) -> Response {
    let Ok(request) = wire::ActionCueRequest::decode(body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(region) = request.region else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if request.phone_id != media.config.device
        || [Some(region), request.clear_region, request.track_from]
            .into_iter()
            .flatten()
            .any(|r| {
                r.left >= r.right
                    || r.top >= r.bottom
                    || r.right > request.source_width
                    || r.bottom > request.source_height
            })
        || !matches!(
            request.motion_direction.as_str(),
            "" | "left" | "right" | "up" | "down"
        )
        || request.rgb.len() > 4
        || (request.rgb.is_empty() && request.motion_direction.is_empty())
        || (!request.rgb.is_empty() && !(1..=100).contains(&request.coverage_percent))
        || (request.clear_region.is_some() && request.rgb.is_empty())
        || request.avoid_rgb.len() > 4
        || (!request.avoid_rgb.is_empty() && request.clear_region.is_none())
        || request
            .rgb
            .iter()
            .chain(&request.avoid_rgb)
            .any(|v| *v > 0xffffff)
        || !(100..=15000).contains(&request.timeout_ms)
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let colors = |values: &[u32]| {
        values
            .iter()
            .map(|v| {
                let [_, r, g, b] = v.to_be_bytes();
                [r, g, b]
            })
            .collect::<Vec<_>>()
    };
    let palette = colors(&request.rgb);
    let clear_region = request.clear_region.map(|area| {
        let mut blocked = palette.clone();
        blocked.extend(colors(&request.avoid_rgb));
        (area, blocked)
    });
    let target = if let Some(area) = request.track_from {
        let Ok(pixels) = <[u8; 75]>::try_from(request.target_rgb.as_slice()) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if request.motion_direction.is_empty()
            || request.lead_ms > 500
            || !(0..3).any(|c| {
                let mut channel = pixels[c..].iter().step_by(3);
                let first = *channel.next().unwrap();
                let (low, high) =
                    channel.fold((first, first), |(lo, hi), &v| (lo.min(v), hi.max(v)));
                high - low >= 24
            })
        {
            return StatusCode::BAD_REQUEST.into_response();
        }
        Some(Target::new(area, pixels))
    } else {
        None
    };
    let cue = Arc::new(Cue {
        target,
        source: (request.source_width, request.source_height),
        region,
        clear_region,
        palette,
        percent: request.coverage_percent,
        any_color: request.any_color,
        moving: !request.motion_direction.is_empty(),
    });
    let started = Instant::now();
    let mut result = wire::ActionCueMatch::default();
    let edge = request.on_appearance || !request.motion_direction.is_empty();
    let mut frames = samples(&media, cue, edge);
    let observe = async {
        let mut previous = None;
        let mut missing_since = None;
        let mut prior_target: Option<(wire::PhoneBounds, Instant)> = None;
        let mut reference: Option<(crate::motion::Grid, Instant)> = None;
        let max_age = crate::grounding::CUE_MAX_AGE;
        while let Some(sample) = frames.next().await {
            let (sample, at) = sample?;
            if at.elapsed() > max_age {
                previous = None;
                continue;
            }
            result.frames += 1;
            let matched = match sample {
                Sample::Color(matched) => matched,
                Sample::Target(bounds, appearance) => {
                    let Some(bounds) = bounds else {
                        if prior_target.is_none()
                            || at.saturating_duration_since(*missing_since.get_or_insert(at))
                                >= Duration::from_millis(120)
                        {
                            result.target_lost = true;
                            return Ok(());
                        }
                        continue;
                    };
                    missing_since = None;
                    if let Some((old, prior)) = prior_target {
                        let dt = at.saturating_duration_since(prior);
                        if dt < Duration::from_millis(40) {
                            continue;
                        }
                        if appearance
                            && dt <= max_age
                            && let Some((track, arrival)) = crossing(
                                old,
                                bounds,
                                dt.as_millis() as u32,
                                region,
                                &request.motion_direction,
                                request.lead_ms,
                            )
                        {
                            result.motion = Some(track);
                            result.arrival_ms = arrival;
                        }
                    }
                    prior_target = Some((bounds, at));
                    result.motion.is_some()
                }
                Sample::Motion(grid, appearance) => {
                    if reference.as_ref().is_some_and(|(_, prior)| {
                        at.saturating_duration_since(*prior) < Duration::from_millis(65)
                    }) {
                        continue;
                    }
                    let track = reference
                        .as_ref()
                        .filter(|(_, prior)| {
                            appearance && at.saturating_duration_since(*prior) <= max_age
                        })
                        .and_then(|(old, _)| {
                            grid.tracks(old)
                                .into_iter()
                                .next()
                                .filter(|t| crate::motion::direction(t) == request.motion_direction)
                        });
                    reference = Some((grid, at));
                    result.motion = track;
                    result.motion.is_some()
                }
            };
            if request.on_appearance && result.frames == 1 && matched {
                result.already_present = true;
                return Ok(());
            }
            let first = if edge {
                Some(at)
            } else {
                previous.filter(|&prior| at.saturating_duration_since(prior) <= max_age)
            };
            if matched && let Some(first) = first {
                result.matched = true;
                (result.width, result.height) = (request.source_width, request.source_height);
                result.observed_at_ms =
                    super::now().saturating_sub(at.elapsed().as_millis() as u64);
                result.first_match_at_ms = result
                    .observed_at_ms
                    .saturating_sub(at.saturating_duration_since(first).as_millis() as u64);
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            previous = matched.then_some(at);
        }
        Err("设备画面已断开".into())
    };
    if let Ok(Err(error)) = timeout(Duration::from_millis(request.timeout_ms.into()), observe).await
    {
        return super::failure(error);
    }
    result.elapsed_ms = started.elapsed().as_millis() as u32;
    ([("content-type", crate::MEDIA)], result.encode_to_vec()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_visible_state_finishes_sampling_at_its_requested_coverage() {
        let cue = Cue {
            target: None,
            source: (40, 40),
            region: wire::PhoneBounds {
                left: 0,
                top: 0,
                right: 40,
                bottom: 40,
            },
            clear_region: None,
            palette: vec![[0, 120, 0]],
            percent: 10,
            any_color: false,
            moving: false,
        };
        let reads = std::cell::Cell::new(0);
        assert!(cue.matches((40, 40), |_, _| {
            reads.set(reads.get() + 1);
            [0, 120, 0]
        }));
        assert_eq!(reads.get(), 160);
    }

    #[test]
    fn color_coverage_and_exclusion_use_their_own_regions() {
        let mut cue = Cue {
            target: None,
            source: (8, 8),
            region: wire::PhoneBounds {
                left: 2,
                top: 5,
                right: 6,
                bottom: 7,
            },
            clear_region: None,
            palette: vec![[255, 0, 0], [0, 0, 255]],
            percent: 50,
            any_color: true,
            moving: false,
        };
        let window = |x, _| if x < 4 { [255, 0, 0] } else { [0, 0, 0] };
        assert!(cue.matches((8, 8), window));
        let destination = wire::PhoneBounds {
            left: 0,
            top: 5,
            right: 2,
            bottom: 7,
        };
        cue.clear_region = Some((destination, cue.palette.clone()));
        assert!(!cue.matches((8, 8), window));
        let overlay = |x, y| if x < 2 { [0, 200, 0] } else { window(x, y) };
        assert!(cue.matches((8, 8), overlay));
        cue.clear_region.as_mut().unwrap().1.push([0, 200, 0]);
        assert!(!cue.matches((8, 8), overlay));
        assert!(!cue.matches((8, 8), window));
        assert!(cue.matches((8, 8), |x, y| if x < 2 {
            [50, 80, 60]
        } else {
            window(x, y)
        }));
        cue.clear_region = None;
        cue.percent = 51;
        assert!(!cue.matches((8, 8), window));
        cue.percent = 50;
        cue.any_color = false;
        assert!(!cue.matches((8, 8), window));
        cue.palette = vec![[0, 200, 0]];
        assert!(!cue.matches((8, 8), |_, _| [0, 225, 0]));
    }
}
