use ai_phone_adb::{Client, Limits};
use ai_phone_api::wire::{DesktopActionRequest, HostDesktopReply};
use image::codecs::jpeg::JpegEncoder;
use std::time::{Duration, Instant};

pub(super) const ADB_PORT: u16 = 5037;
pub(super) const DEADLINE: Duration = Duration::from_secs(8);
pub(super) struct Phone {
    client: Client,
    observations: crate::host_desktop::Views<(u32, u32, Instant)>,
}
impl Phone {
    pub async fn open(serial: &str) -> Result<Self, String> {
        let device = ai_phone_adb::devices(ADB_PORT, DEADLINE)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|device| device.serial == serial)
            .ok_or("请在手机上授权 ADB 并重新选择设备")?;
        Ok(Self {
            client: Client::attached(ADB_PORT, device).map_err(|e| e.to_string())?,
            observations: Default::default(),
        })
    }
    pub async fn perform(
        &mut self,
        action: &DesktopActionRequest,
    ) -> Result<HostDesktopReply, String> {
        if action.kind == "foreground" {
            let focused = self
                .client
                .shell(
                    "dumpsys window displays | grep -m1 mCurrentFocus=",
                    Limits {
                        stdout: 1024,
                        stderr: 1024,
                    },
                    DEADLINE,
                )
                .await
                .map_err(|e| e.to_string())?;
            let foreground = String::from_utf8_lossy(&focused.stdout)
                .split_whitespace()
                .find(|s| s.contains('/'))
                .unwrap_or_default()
                .trim_end_matches('}')
                .to_owned();
            return Ok(HostDesktopReply {
                foreground,
                ..Default::default()
            });
        }
        if action.kind == "observe" {
            let png = self
                .client
                .raw("exec:screencap -p", 24 * 1024 * 1024, DEADLINE)
                .await
                .map_err(|e| e.to_string())?;
            let (jpeg, width, height) =
                tokio::task::spawn_blocking(move || -> Result<_, String> {
                    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                        .map_err(|e| e.to_string())?
                        .into_rgb8();
                    let mut jpeg = Vec::new();
                    JpegEncoder::new_with_quality(&mut jpeg, 78)
                        .encode_image(&image)
                        .map_err(|e| e.to_string())?;
                    Ok((jpeg, image.width(), image.height()))
                })
                .await
                .map_err(|e| e.to_string())??;
            let observation_id = uuid::Uuid::now_v7().to_string();
            self.observations.remember(
                action,
                observation_id.clone(),
                (width, height, Instant::now()),
            );
            return Ok(HostDesktopReply {
                observation_id,
                jpeg,
                width,
                height,
                ..Default::default()
            });
        }
        let (width, height, _) = self
            .observations
            .take(action)
            .filter(|(_, _, at)| at.elapsed() < Duration::from_secs(60))
            .ok_or("请先观察这台手机")?;
        let point = |x: i32, y: i32| -> Result<(), String> {
            if (0..i64::from(width)).contains(&i64::from(x))
                && (0..i64::from(height)).contains(&i64::from(y))
            {
                Ok(())
            } else {
                Err("操作点在屏幕外".into())
            }
        };
        let (x, y) = (action.x, action.y);
        let command = match action.kind.as_str() {
            "click" | "double_click" => {
                point(x, y)?;
                let tap = format!("input tap {x} {y}");
                if action.kind == "double_click" {
                    format!("{tap} && {tap}")
                } else {
                    tap
                }
            }
            "long_press" => {
                point(x, y)?;
                format!("input swipe {x} {y} {x} {y} 650")
            }
            "swipe" => {
                point(x, y)?;
                point(action.to_x, action.to_y)?;
                format!("input swipe {x} {y} {} {} 240", action.to_x, action.to_y)
            }
            "scroll" => {
                point(x, y)?;
                let bottom = i64::from(height) - 1;
                let to = (i64::from(y) - i64::from(action.amount.clamp(-20, 20)) * bottom / 10)
                    .clamp(0, bottom);
                format!("input swipe {x} {y} {x} {to} 240")
            }
            "type" => {
                if !action
                    .text
                    .bytes()
                    .all(|b| b == b' ' || b.is_ascii_graphic())
                    || action.text.contains("%s")
                {
                    return Err("ADB 文本输入支持可打印 ASCII".into());
                }
                let literal = action.text.replace(' ', "%s").replace('\'', "'\\''");
                format!("input text '{literal}'")
            }
            "key" => {
                let key = match action.key.as_str() {
                    "enter" => 66,
                    "tab" => 61,
                    "escape" => 111,
                    "backspace" => 67,
                    "left" => 21,
                    "right" => 22,
                    "up" => 19,
                    "down" => 20,
                    "back" => 4,
                    "home" => 3,
                    _ => return Err("手机不支持此按键".into()),
                };
                format!("input keyevent {key}")
            }
            _ => return Err("手机不支持此操作".into()),
        };
        let output = self
            .client
            .shell(
                &command,
                Limits {
                    stdout: 4096,
                    stderr: 4096,
                },
                DEADLINE,
            )
            .await
            .map_err(|e| e.to_string())?;
        if output.code != 0 {
            return Err(format!(
                "Android 输入失败 ({}) {}",
                output.code,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(HostDesktopReply::default())
    }
}
