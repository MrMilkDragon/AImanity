use super::{
    STANDARD, latest_grounding, latest_phone_observation, output_position, tool_output, tool_value,
    wire,
};
use base64::Engine;
use image::GenericImageView;
use serde::Deserialize;
use serde_json::{Value, json};

pub(super) fn follows_tap(call: &Value) -> bool {
    matches!(call["name"].as_str(), Some("phone_tap" | "phone_swipe"))
        && call["arguments"]
            .as_str()
            .and_then(|args| serde_json::from_str::<Value>(args).ok())
            .is_some_and(|args| args["when"].get("start_tap").is_some())
}

pub(super) async fn starting_tap(call: &Value, context: &[Value]) -> Option<Value> {
    if !matches!(call["name"].as_str(), Some("phone_tap" | "phone_swipe")) {
        return None;
    }
    let args: Value = serde_json::from_str(call["arguments"].as_str()?).ok()?;
    let when = &args["when"];
    let start = when.get("start_tap")?;
    if when["on_appearance"] != true || visual_condition(when, &[]).await.is_err() {
        return None;
    }
    let (name, arguments) = if let Some(element) = start["element_id"].as_str() {
        let observed = latest_grounding(context)?;
        (
            "phone_tap_element",
            json!({"element_id":element,"observation_id":observed["observation_id"]}),
        )
    } else {
        (
            "phone_tap",
            json!({"x":start["x"].as_u64()?,"y":start["y"].as_u64()?,"fallback_reason":start["fallback_reason"].as_str()?}),
        )
    };
    Some(
        json!({"type":"function_call","name":name,"arguments":arguments.to_string(),
        "call_id":uuid::Uuid::now_v7().to_string(),"local_action_id":uuid::Uuid::now_v7().to_string(),
        "local_start_for":call["local_action_id"],"local_planned_at_ms":super::now_ms()}),
    )
}

pub(super) fn validate_followup(context: &[Value], generated: usize) -> Result<(), &'static str> {
    let earlier = &context[..generated];
    let tap = earlier
        .last()
        .filter(|tap| {
            matches!(
                tap["name"].as_str(),
                Some("phone_tap" | "phone_tap_element")
            )
        })
        .ok_or("同批预判需要紧随启动点击")?;
    let read = latest_phone_observation(context).ok_or("启动后缺少实景")?;
    if !tap["local_start_for"].is_string()
        || tap["local_start_for"] != context[generated]["local_action_id"]
        || read["local_after_action"] != tap["call_id"]
        || !output_position(context, tap).is_some_and(|n| n > generated)
        || !tool_output(context, tap)
            .and_then(tool_value)
            .is_some_and(|r| r["success"] == true)
    {
        return Err("启动点击与当前画面的回执不匹配");
    }
    let before = latest_grounding(earlier).ok_or("启动点击之前缺少实景")?;
    let now = latest_grounding(context).ok_or("启动后缺少实景")?;
    if !before["foreground_app"]
        .as_str()
        .is_some_and(|app| !app.is_empty())
        || before["foreground_app"] != now["foreground_app"]
        || before["device_id"] != now["device_id"]
        || before["width"].as_u64().is_none()
        || before["height"].as_u64().is_none()
        || before["width"] != now["width"]
        || before["height"] != now["height"]
    {
        return Err("启动后的设备、应用或画面尺寸已改变，请重新观察");
    }
    Ok(())
}

#[derive(Deserialize)]
struct Condition {
    region: [u32; 4],
    track_from: Option<[u32; 4]>,
    #[serde(default)]
    lead_ms: u32,
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
    #[serde(default)]
    motion_direction: String,
    #[serde(default)]
    coverage_percent: u32,
    timeout_ms: u32,
}

pub(super) async fn visual_condition(
    value: &Value,
    context: &[Value],
) -> Result<wire::ActionCueRequest, String> {
    let cue = Condition::deserialize(value).map_err(|_| "视觉触发条件格式无效")?;
    let motion = !cue.motion_direction.is_empty();
    let colors = cue.colors.len() + cue.sample_points.len();
    if colors > 4 || (colors == 0 && (!motion || cue.clear_region.is_some())) {
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
    if cue.lead_ms > 500
        || (cue.lead_ms > 0 && cue.track_from.is_none())
        || (cue.track_from.is_some() && !motion)
    {
        return Err("跟踪需选择方向，提前量为 0–500 毫秒".into());
    }
    let mut avoid_rgb = Vec::new();
    let mut target_rgb = Vec::new();
    if cue.track_from.is_some() || !cue.sample_points.is_empty() || !cue.avoid_points.is_empty() {
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
        let (target, avoid, patch) = tokio::task::spawn_blocking(move || {
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
            let mut patch = Vec::new();
            if let Some([l, t, r, b]) = cue.track_from {
                if r <= l || b <= t || r > image.width() || b > image.height() {
                    return Err("目标框超出当前截图");
                }
                for j in 0..5 {
                    for i in 0..5 {
                        patch.extend_from_slice(
                            &image
                                .get_pixel(
                                    l + (2 * i + 1) * (r - l) / 10,
                                    t + (2 * j + 1) * (b - t) / 10,
                                )
                                .0[..3],
                        );
                    }
                }
            }
            Ok::<_, &str>((
                sample(&cue.sample_points)?,
                sample(&cue.avoid_points)?,
                patch,
            ))
        })
        .await
        .map_err(|e| e.to_string())??;
        target_rgb = patch;
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
        track_from: cue.track_from.map(bounds),
        target_rgb,
        lead_ms: cue.lead_ms,
        clear_region: cue.clear_region.map(bounds),
        rgb,
        avoid_rgb,
        any_color: cue.any_color,
        on_appearance: motion || cue.on_appearance,
        motion_direction: cue.motion_direction,
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
        cue["motion_direction"] = json!("right");
        let moving = visual_condition(&cue, &history).await.unwrap();
        assert_eq!(moving.motion_direction, "right");
        assert_eq!(moving.rgb, request.rgb);
        assert!(visual_condition(&cue, &[]).await.is_err());
        cue["sample_points"] = json!([[3, 2]]);
        assert!(visual_condition(&cue, &history).await.is_err());
    }
}
