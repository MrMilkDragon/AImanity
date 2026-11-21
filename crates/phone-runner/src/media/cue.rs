use super::{Bytes, Media, State, StatusCode, hub, wire};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, stream::BoxStream};
use prost::Message;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::timeout;

pub(super) struct Cue {
    source: (u32, u32),
    region: wire::PhoneBounds,
    clear_region: Option<(wire::PhoneBounds, Vec<[u8; 3]>)>,
    palette: Vec<[u8; 3]>,
    percent: u32,
    any_color: bool,
}

impl Cue {
    pub(super) fn matches(&self, size: (u32, u32), pixel: impl Fn(u32, u32) -> [u8; 3]) -> bool {
        let (w, h) = size;
        if u64::from(w) * u64::from(self.source.1) != u64::from(h) * u64::from(self.source.0) {
            return false;
        }
        self.occupied(self.region, size, &pixel, &self.palette, self.any_color)
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
        let mut hits = [0; 8];
        let mut total = 0;
        for row in y.step_by(dy) {
            for col in x.clone().step_by(dx) {
                let p = pixel(col, row);
                for (i, color) in colors.iter().enumerate() {
                    if p.iter().zip(color).all(|(a, b)| a.abs_diff(*b) <= 24) {
                        hits[i] += 1;
                    }
                }
                total += 1;
            }
        }
        let mut matches = hits[..colors.len()]
            .iter()
            .map(|h| h * 100 >= total * self.percent);
        if any {
            matches.any(|found| found)
        } else {
            matches.all(|found| found)
        }
    }
}

pub(super) type Samples<'a> = BoxStream<'a, super::Result<(bool, Instant)>>;

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
                            cue.matches(image.dimensions(), |x, y| image.get_pixel(x, y).0),
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
        || [Some(region), request.clear_region]
            .into_iter()
            .flatten()
            .any(|r| {
                r.left >= r.right
                    || r.top >= r.bottom
                    || r.right > request.source_width
                    || r.bottom > request.source_height
            })
        || !(1..=4).contains(&request.rgb.len())
        || request.avoid_rgb.len() > 4
        || (!request.avoid_rgb.is_empty() && request.clear_region.is_none())
        || request
            .rgb
            .iter()
            .chain(&request.avoid_rgb)
            .any(|v| *v > 0xffffff)
        || !(1..=100).contains(&request.coverage_percent)
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
    let cue = Arc::new(Cue {
        source: (request.source_width, request.source_height),
        region,
        clear_region,
        palette,
        percent: request.coverage_percent,
        any_color: request.any_color,
    });
    let started = Instant::now();
    let mut result = wire::ActionCueMatch::default();
    let mut frames = samples(&media, cue, request.on_appearance);
    let observe = async {
        let mut previous = None;
        let max_age = crate::grounding::CUE_MAX_AGE;
        while let Some(sample) = frames.next().await {
            let (matched, at) = sample?;
            if at.elapsed() > max_age {
                previous = None;
                continue;
            }
            result.frames += 1;
            if request.on_appearance && result.frames == 1 && matched {
                result.already_present = true;
                return Ok(());
            }
            let first = if request.on_appearance {
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
    fn alternate_colors_still_require_coverage_inside_the_lane() {
        let mut cue = Cue {
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
        let maroon_car = |x, y| if x < 2 { [125, 43, 61] } else { window(x, y) };
        assert!(cue.matches((8, 8), maroon_car));
        cue.clear_region.as_mut().unwrap().1.push([125, 43, 61]);
        assert!(!cue.matches((8, 8), maroon_car));
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
        cue.palette = vec![[125, 43, 61]];
        assert!(!cue.matches((8, 8), |_, _| [125, 76, 82]));
    }
}
