use super::{STANDARD, latest_phone_observation, tool_output, wire};
use base64::Engine;
use image::GenericImageView;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Condition {
    region: [u32; 4],
    clear_region: Option<[u32; 4]>,
    #[serde(default)]
    colors: Vec<String>,
    #[serde(default)]
    sample_points: Vec<[u32; 2]>,
    #[serde(default)]
    avoid_points: Vec<[u32; 2]>,
    #[serde(default)]
    any_color: bool,
    #[serde(default)]
    on_appearance: bool,
    coverage_percent: u32,
    timeout_ms: u32,
}

pub(super) async fn visual_condition(
    value: &Value,
    context: &[Value],
) -> Result<wire::ActionCueRequest, String> {
    let cue = Condition::deserialize(value).map_err(|_| "视觉触发条件格式无效")?;
    if !(1..=4).contains(&(cue.colors.len() + cue.sample_points.len())) {
        return Err("请从当前画面选一至四个取色点或颜色".into());
    }
    if cue.avoid_points.len() > 4 || (!cue.avoid_points.is_empty() && cue.clear_region.is_none()) {
        return Err("目标区域取色需要 clear_region，最多四点".into());
    }
    let mut rgb = cue
        .colors
        .iter()
        .map(|s| {
            s.strip_prefix('#')
                .filter(|v| v.len() == 6)
                .and_then(|v| u32::from_str_radix(v, 16).ok())
                .ok_or("颜色格式应为 #RRGGBB")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut avoid_rgb = Vec::new();
    if !cue.sample_points.is_empty() || !cue.avoid_points.is_empty() {
        let frame = latest_phone_observation(context)
            .and_then(|call| tool_output(context, call))
            .and_then(Value::as_array)
            .and_then(|parts| {
                parts.iter().find_map(|p| {
                    if p["type"] != "input_image" {
                        return None;
                    }
                    p["image_url"]
                        .as_str()?
                        .strip_prefix("data:image/png;base64,")
                })
            })
            .ok_or("当前手机实景帧不可用，请重新观察后取色")?;
        let png = STANDARD.decode(frame).map_err(|_| "实景帧编码无效")?;
        let (target, avoid) = tokio::task::spawn_blocking(move || {
            let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                .map_err(|_| "实景帧读取失败")?;
            let sample = |points: &[[u32; 2]]| {
                points
                    .iter()
                    .map(|&[x, y]| {
                        if !image.in_bounds(x, y) {
                            return Err("取色点超出当前截图");
                        }
                        let [r, g, b, _] = image.get_pixel(x, y).0;
                        Ok(u32::from_be_bytes([0, r, g, b]))
                    })
                    .collect::<Result<Vec<_>, _>>()
            };
            Ok::<_, &str>((sample(&cue.sample_points)?, sample(&cue.avoid_points)?))
        })
        .await
        .map_err(|e| e.to_string())??;
        rgb.extend(target);
        avoid_rgb = avoid;
    }
    let bounds = |[left, top, right, bottom]: [u32; 4]| wire::PhoneBounds {
        left,
        top,
        right,
        bottom,
    };
    Ok(wire::ActionCueRequest {
        region: Some(bounds(cue.region)),
        clear_region: cue.clear_region.map(bounds),
        rgb,
        avoid_rgb,
        any_color: cue.any_color,
        on_appearance: cue.on_appearance,
        coverage_percent: cue.coverage_percent,
        timeout_ms: cue.timeout_ms,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn sampling_reads_the_returned_phone_frame_in_screen_coordinates() {
        let mut image = image::RgbImage::new(3, 2);
        image.put_pixel(2, 1, image::Rgb([0x17, 0x86, 0xc1]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let history = [
            json!({"type":"function_call","name":"phone_observe","call_id":"view"}),
            json!({"type":"function_call_output","call_id":"view","output":[{"type":"input_image","image_url":format!("data:image/png;base64,{}",STANDARD.encode(png.into_inner()))}]}),
        ];
        let mut cue = json!({"region":[0,0,3,2],"colors":["#010203"],"sample_points":[[2,1]],"clear_region":[0,0,1,1],"avoid_points":[[0,0]],"coverage_percent":10,"timeout_ms":300});
        let request = visual_condition(&cue, &history).await.unwrap();
        assert_eq!(request.avoid_rgb, [0]);
        assert_eq!(request.rgb, [0x010203, 0x1786c1]);
        assert!(visual_condition(&cue, &[]).await.is_err());
        cue["sample_points"] = json!([[3, 2]]);
        assert!(visual_condition(&cue, &history).await.is_err());
    }
}
