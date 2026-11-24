use crate::{DesktopRuntime, Result, Settings, screen::Screen};
use ai_phone_api::{
    observations::Views,
    wire::{DesktopActionRequest, DeviceIoRequest, HostDesktopReply, LocationDevice},
};
use aimanity_device_io::{StorageMedia, files::Endpoint};
use futures_util::{SinkExt, StreamExt};
use image::codecs::jpeg::JpegEncoder;
use prost::Message;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message as Frame, protocol::Role},
};

fn key(name: &str) -> Result<&'static str> {
    Ok(match name {
        "enter" => "Return",
        "escape" | "back" => "Escape",
        "tab" => "Tab",
        "backspace" => "BackSpace",
        "left" => "Left",
        "right" => "Right",
        "up" => "Up",
        "down" => "Down",
        "home" => "Home",
        "save" => "ctrl+s",
        "select_all" => "ctrl+a",
        "undo" => "ctrl+z",
        "redo" => "ctrl+shift+z",
        "volumeup" => "XF86AudioRaiseVolume",
        "volumedown" => "XF86AudioLowerVolume",
        "volumemute" => "XF86AudioMute",
        _ => return Err("Linux 不支持此按键".into()),
    })
}
struct Session {
    desktop: DesktopRuntime,
    views: Views<(Instant, crate::screen::WindowIdentity)>,
    location: reqwest::Client,
}
impl Session {
    async fn perform(&mut self, q: &DesktopActionRequest) -> Result<HostDesktopReply> {
        if let Some(fix) = q.location {
            let bytes = self
                .location
                .post("http://127.0.0.1:17867/fix")
                .header("Content-Type", "application/x-protobuf")
                .body(fix.encode_to_vec())
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            let receipt = LocationDevice::decode(bytes)?;
            if receipt.status != if fix.enabled { "applied" } else { "disabled" } {
                return Err(if receipt.error.is_empty() {
                    "定位未生效".into()
                } else {
                    receipt.error.into()
                });
            }
            return Ok(HostDesktopReply {
                location: Some(fix),
                location_scope: "browser".into(),
                ..Default::default()
            });
        }
        let window = self.desktop.screen.window()?;
        let mut reply = HostDesktopReply {
            foreground: window.title.clone(),
            ..Default::default()
        };
        match q.kind.as_str() {
            "foreground" => return Ok(reply),
            "observe" => {
                let pixels = self.desktop.screen.capture()?;
                reply.width = pixels.width();
                reply.height = pixels.height();
                JpegEncoder::new_with_quality(&mut reply.jpeg, 78).encode_image(&pixels)?;
                reply.observation_id = uuid::Uuid::new_v4().to_string();
                self.views
                    .remember(q, reply.observation_id.clone(), (Instant::now(), window));
                return Ok(reply);
            }
            _ => {}
        }
        let (_, observed) = self
            .views
            .take(q)
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(60))
            .ok_or("请先观察这台电脑")?;
        if observed.window_id != window.window_id || observed.process_id != window.process_id {
            return Err("前台窗口已改变，请重新观察".into());
        }
        let mut input = match q.kind.as_str() {
            "click" | "double_click" | "right_click" => {
                let button = if q.kind == "right_click" { 3 } else { 1 };
                let clicks = if q.kind == "double_click" {
                    2
                } else {
                    q.clicks.max(1)
                };
                json!({"kind": "click", "x": q.x, "y": q.y, "button": button, "clicks": clicks})
            }
            "drag" => json!({"kind":"drag","x":q.x,"y":q.y,"to_x":q.to_x,"to_y":q.to_y}),
            "scroll" => json!({"kind":"scroll","x":q.x,"y":q.y,"amount":q.amount}),
            "stroke" => {
                let strokes: Vec<Vec<_>> = q
                    .strokes
                    .iter()
                    .map(|stroke| stroke.points.iter().map(|p| [p.x, p.y]).collect())
                    .collect();
                json!({"kind": "stroke", "strokes": strokes})
            }
            "type" => {
                json!({"kind":"text","text":q.text,"window_id":observed.window_id,"process_id":observed.process_id})
            }
            "key" => json!({"kind":"key","key":key(&q.key)?}),
            _ => return Err("Linux 不支持此操作".into()),
        };
        input["action_id"] = json!(q.action_id);
        input["wait_for_paint"] = json!(q.kind == "type");
        input["receipt_only"] = json!(true);
        self.desktop.perform_action(input, || Ok(())).await?;
        Ok(reply)
    }
}

pub async fn serve(operation: &str) -> Result<()> {
    let stream = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
    let mut ws = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
    if operation == "workspace-io" {
        let Some(Ok(Frame::Binary(bytes))) = ws.next().await else {
            return Err("缺少设备文件或音频请求".into());
        };
        let q = DeviceIoRequest::decode(bytes)?;
        return StorageMedia {
            files: Endpoint::Desktop,
            guest_audio: true,
            pulse_server: Some("unix:/tmp/ai-runtime/pulse/native".into()),
        }
        .serve(&mut ws, q)
        .await;
    }
    let desktop = DesktopRuntime::new(Settings::from_env());
    desktop.screen.window()?;
    ws.send(Frame::Binary(
        HostDesktopReply::default().encode_to_vec().into(),
    ))
    .await?;
    let (out, mut notices) = tokio::sync::mpsc::channel(16);
    let watch = aimanity_device_io::notices::watch(
        desktop.settings.home.join(".aiphone/notifications.json"),
        out,
    );
    tokio::pin!(watch);
    let mut session = Session {
        desktop,
        views: Default::default(),
        location: reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(8))
            .build()?,
    };
    loop {
        let reply = tokio::select! {
            _ = &mut watch => break,
            notice = notices.recv() => {
                let Some(notification) = notice else { break };
                HostDesktopReply { notification: Some(notification), ..Default::default() }
            }
            message = ws.next() => match message.transpose()? {
                Some(Frame::Binary(bytes)) => {
                    let request = DesktopActionRequest::decode(bytes)?;
                    let mut receipt = session.perform(&request).await.unwrap_or_else(|error| {
                        HostDesktopReply { error: error.to_string(), ..Default::default() }
                    });
                    receipt.action_id = request.action_id;
                    receipt
                }
                Some(Frame::Close(_)) | None => break,
                _ => continue,
            }
        };
        ws.send(Frame::Binary(reply.encode_to_vec().into())).await?;
    }
    Ok(())
}
