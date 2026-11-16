use ai_phone_api::wire;
use bytes::BytesMut;
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use std::{
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue};

#[derive(Clone, Debug)]
pub struct Connection {
    http: reqwest::Client,
    assistant: HeaderValue,
    phone: HeaderValue,
    session: HeaderValue,
}
#[derive(Clone, Debug)]
pub struct Session {
    pub client: Connection,
    pub registry: wire::RegistryState,
    pub chat: wire::ChatState,
    pub links: wire::DeviceLinkState,
}
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<Vec<u8>>,
    pub sequence: u64,
    pub captured_ms: f64,
    pub received: Instant,
}
#[derive(Clone, Debug)]
pub enum DeviceInput {
    Tap {
        x: u32,
        y: u32,
        right: bool,
    },
    Drag {
        x: u32,
        y: u32,
        to_x: u32,
        to_y: u32,
        duration: u32,
    },
    Scroll {
        x: u32,
        y: u32,
        amount: i32,
    },
    Text(String),
    Key(String),
    Rotate(u32),
}
impl Connection {
    pub async fn connect(root: String) -> Result<Session, String> {
        let root = std::path::Path::new(&root);
        let (assistant, phone) = tokio::try_join!(
            tokio::fs::read_to_string(root.join("data/assistant/token")),
            tokio::fs::read_to_string(root.join("data/phone-runner/token")),
        )
        .map_err(|e| e.to_string())?;
        let bearer = |token: &str| -> Result<HeaderValue, String> {
            let mut header = HeaderValue::from_str(&format!("Bearer {}", token.trim()))
                .map_err(|e| e.to_string())?;
            header.set_sensitive(true);
            Ok(header)
        };
        let http = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?;
        let mut client = Self {
            http,
            assistant: bearer(&assistant)?,
            phone: bearer(&phone)?,
            session: HeaderValue::from_static(""),
        };
        let session: wire::LoginResult = client.rpc("enter", wire::LocalEntryRequest {}).await?;
        client.session = HeaderValue::from_str(&format!("ai_phone_session={}", session.token))
            .map_err(|e| e.to_string())?;
        client.session.set_sensitive(true);
        let (registry, chat, links) = tokio::try_join!(
            client.rpc("registry/state", wire::RegistryRequest {}),
            client.history(),
            client.rpc("desktop/state", wire::RegistryRequest {}),
        )?;
        Ok(Session {
            client,
            registry,
            chat,
            links,
        })
    }
    pub async fn rpc<I: Message, O: Message + Default>(
        &self,
        route: &str,
        input: I,
    ) -> Result<O, String> {
        let started = Instant::now();
        let (port, path, token) = if let Some(path) = route.strip_prefix("phone/") {
            (17862, path, &self.phone)
        } else if let Some(path) = route.strip_prefix("channels/") {
            (17864, path, &self.assistant)
        } else {
            (
                17863,
                route.strip_prefix("assistant/").unwrap_or(route),
                &self.assistant,
            )
        };
        let response = self
            .http
            .post(format!("http://127.0.0.1:{port}/v1/{path}"))
            .header("authorization", token)
            .header("content-type", "application/x-protobuf")
            .body(input.encode_to_vec())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        eprintln!(
            "native_metric rpc route={route} ms={:.3} status={} bytes={}",
            started.elapsed().as_secs_f64() * 1000.,
            status.as_u16(),
            bytes.len()
        );
        if !status.is_success() {
            let error = wire::ApiError::decode(bytes).map_err(|_| format!("HTTP {status}"))?;
            return Err(error.message);
        }
        O::decode(bytes).map_err(|e| e.to_string())
    }
    pub async fn history(&self) -> Result<wire::ChatState, String> {
        self.rpc(
            "assistant/history",
            wire::ChatHistoryRequest {
                limit: 80,
                ..Default::default()
            },
        )
        .await
    }
    pub async fn audio_stream(&self, index: usize) -> Result<reqwest::Response, String> {
        let route = if index == 0 { "audio" } else { "desktop/audio" };
        self.http
            .get(format!("http://127.0.0.1:17865/{route}"))
            .header("authorization", &self.phone)
            .send()
            .await
            .map_err(|error| error.to_string())
    }
    pub async fn upload(&self, path: &std::path::Path) -> Result<wire::ChatAttachment, String> {
        let name = path
            .file_name()
            .ok_or("文件名为空")?
            .to_string_lossy()
            .into_owned();
        let size = tokio::fs::metadata(path)
            .await
            .map_err(|e| e.to_string())?
            .len();
        if size == 0 || size > 8 * 1024 * 1024 {
            return Err("单个附件须为 1 字节到 8 MiB".into());
        }
        let bytes = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
        self.upload_bytes(name, bytes).await
    }
    pub async fn upload_bytes(
        &self,
        name: String,
        bytes: Vec<u8>,
    ) -> Result<wire::ChatAttachment, String> {
        if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
            return Err("单个附件须为 1 字节到 8 MiB".into());
        }
        let form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(bytes).file_name(name),
        );
        let response = self
            .http
            .post("http://127.0.0.1:17863/v1/upload")
            .header("authorization", &self.assistant)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(wire::ApiError::decode(bytes)
                .map_err(|_| format!("HTTP {status}"))?
                .message);
        }
        wire::ChatAttachment::decode(bytes).map_err(|e| e.to_string())
    }
    pub async fn get_bytes(&self, route: &str) -> Result<bytes::Bytes, String> {
        self.get(route, &[])
            .await?
            .bytes()
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn attachment(&self, id: &str) -> Result<bytes::Bytes, String> {
        self.get_bytes(&format!("attachment/{id}")).await
    }
    async fn get(&self, route: &str, query: &[(&str, &str)]) -> Result<reqwest::Response, String> {
        let response = self
            .http
            .get(format!("http://127.0.0.1:17863/v1/{route}"))
            .query(query)
            .header("authorization", &self.assistant)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        if !status.is_success() {
            let bytes = response.bytes().await.map_err(|e| e.to_string())?;
            return Err(wire::ApiError::decode(bytes)
                .map_err(|_| format!("HTTP {status}"))?
                .message);
        }
        Ok(response)
    }
    pub async fn download_file(
        &self,
        source: &str,
        target: &std::path::Path,
    ) -> Result<(), String> {
        use tokio::io::AsyncWriteExt;
        let mut response = self
            .get(
                "workbench/file",
                &[("path", source.trim_start_matches('/'))],
            )
            .await?;
        let temporary = target.with_extension(format!("{}.part", uuid::Uuid::now_v7()));
        let result = async {
            let mut file = tokio::fs::File::create(&temporary)
                .await
                .map_err(|e| e.to_string())?;
            while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
                file.write_all(&chunk).await.map_err(|e| e.to_string())?;
            }
            file.flush().await.map_err(|e| e.to_string())?;
            drop(file);
            tokio::fs::rename(&temporary, target)
                .await
                .map_err(|e| e.to_string())
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        result
    }
    pub async fn upload_file(
        &self,
        source: &std::path::Path,
        directory: String,
    ) -> Result<wire::FileLocation, String> {
        let location = wire::FileLocation {
            path: directory,
            ..Default::default()
        }
        .encode_to_vec();
        let file = reqwest::multipart::Part::file(source)
            .await
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .part(
                "location",
                reqwest::multipart::Part::bytes(location)
                    .mime_str("application/x-protobuf")
                    .map_err(|e| e.to_string())?,
            )
            .part("file", file);
        let response = self
            .http
            .post("http://127.0.0.1:17863/v1/files/upload")
            .header("authorization", &self.assistant)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(wire::ApiError::decode(bytes)
                .map_err(|_| format!("HTTP {status}"))?
                .message);
        }
        wire::FileLocation::decode(bytes).map_err(|e| e.to_string())
    }
    pub fn chat_events(self) -> impl futures_util::Stream<Item = Result<wire::ChatState, String>> {
        iced::stream::channel::<Result<wire::ChatState, String>>(1, async move |mut output| {
            let result: Result<(), String> = async {
                let mut request = "ws://127.0.0.1:17863/v1/chat/events"
                    .into_client_request()
                    .map_err(|e| e.to_string())?;
                request
                    .headers_mut()
                    .insert("origin", HeaderValue::from_static("http://127.0.0.1:17860"));
                request.headers_mut().insert("cookie", self.session.clone());
                let (mut socket, _) = tokio_tungstenite::connect_async(request)
                    .await
                    .map_err(|e| e.to_string())?;
                while let Some(event) = socket.next().await {
                    match event.map_err(|e| e.to_string())? {
                        tokio_tungstenite::tungstenite::Message::Binary(bytes) => {
                            wire::ChatChanged::decode(bytes).map_err(|e| e.to_string())?;
                            output
                                .send(self.history().await)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                        tokio_tungstenite::tungstenite::Message::Close(_) => {
                            return Err("对话连接已关闭".into());
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            .await;
            if let Err(error) = result {
                let _ = output.send(Err(error)).await;
            }
        })
    }
    pub fn frames(
        self,
        device: wire::RegisteredDevice,
    ) -> impl futures_util::Stream<Item = Result<Frame, String>> {
        iced::stream::channel::<Result<Frame, String>>(1, async move |mut output| {
            let result: Result<(), String> = async {
                let route = if device.kind == "phone" {
                    "screen"
                } else {
                    "desktop/screen"
                };
                let response = self
                    .http
                    .get(format!("http://127.0.0.1:17865/{route}"))
                    .header("authorization", &self.phone)
                    .send()
                    .await
                    .map_err(|e| e.to_string())?
                    .error_for_status()
                    .map_err(|e| e.to_string())?;
                let mut stream = response.bytes_stream();
                let mut bytes = BytesMut::new();
                let mut sequence = 0;
                while let Some(chunk) = stream.next().await {
                    bytes.extend_from_slice(&chunk.map_err(|e| e.to_string())?);
                    let mut latest = None;
                    while let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header =
                            std::str::from_utf8(&bytes[..end]).map_err(|e| e.to_string())?;
                        let length = header
                            .lines()
                            .find_map(|line| line.strip_prefix("Content-Length: "))
                            .ok_or("画面缺少长度")?
                            .trim()
                            .parse::<usize>()
                            .map_err(|e| e.to_string())?;
                        if bytes.len() < end + 4 + length {
                            break;
                        }
                        let captured_ms = header
                            .lines()
                            .find_map(|line| line.strip_prefix("X-Capture-Time: "))
                            .map(str::parse::<f64>)
                            .transpose()
                            .map_err(|e| e.to_string())?
                            .unwrap_or(0.);
                        let packet = bytes.split_to(end + 4 + length).freeze();
                        latest = Some((packet.slice(end + 4..), captured_ms));
                    }
                    if let Some((jpeg, captured_ms)) = latest {
                        let received = Instant::now();
                        let decoded = image::load_from_memory(&jpeg)
                            .map_err(|e| e.to_string())?
                            .into_rgba8();
                        sequence += 1;
                        output
                            .send(Ok(Frame {
                                width: decoded.width(),
                                height: decoded.height(),
                                pixels: Arc::new(decoded.into_raw()),
                                sequence,
                                captured_ms,
                                received,
                            }))
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                }
                Err("设备画面连接已关闭".into())
            }
            .await;
            if let Err(error) = result {
                let _ = output.send(Err(error)).await;
            }
        })
    }
    pub async fn input(
        &self,
        device: &wire::RegisteredDevice,
        input: DeviceInput,
    ) -> Result<(), String> {
        if device.kind == "phone" {
            let scope = wire::TakeControlRequest {
                phone_id: device.id.clone(),
                owner_id: "local-user".into(),
                controller: "manual".into(),
            };
            let control: wire::ControlState = self.rpc("phone/control", scope).await?;
            use wire::device_action_request::Action;
            let action = match input {
                DeviceInput::Tap { x, y, right: false } => Action::Tap(wire::TapAction { x, y }),
                DeviceInput::Tap { x, y, right: true } => {
                    Action::LongPress(wire::LongPressAction {
                        x,
                        y,
                        duration_ms: 650,
                    })
                }
                DeviceInput::Drag {
                    x,
                    y,
                    to_x,
                    to_y,
                    duration,
                } => Action::Drag(wire::DragAction {
                    x1: x,
                    y1: y,
                    x2: to_x,
                    y2: to_y,
                    duration_ms: duration,
                }),
                DeviceInput::Scroll { x, y, amount } => Action::Swipe(wire::SwipeAction {
                    x1: x,
                    y1: y,
                    x2: x,
                    y2: y.saturating_add_signed(-amount),
                    duration_ms: 240,
                }),
                DeviceInput::Text(text) => Action::Input(wire::InputAction { text }),
                DeviceInput::Key(key) => Action::Key(wire::KeyAction { key }),
                DeviceInput::Rotate(rotation) => Action::Orientation(wire::OrientationAction {
                    automatic: false,
                    rotation,
                }),
            };
            let receipt: wire::DeviceActionResponse = self
                .rpc(
                    "phone/action",
                    wire::DeviceActionRequest {
                        phone_id: device.id.clone(),
                        owner_id: "local-user".into(),
                        run_id: "manual".into(),
                        controller: "manual".into(),
                        control_epoch: control.control_epoch,
                        action_id: uuid::Uuid::now_v7().to_string(),
                        action: Some(action),
                        ..Default::default()
                    },
                )
                .await?;
            if !receipt.success {
                return Err(receipt.output);
            }
        } else {
            let control: wire::DesktopObservation = self
                .rpc(
                    "desktop/control",
                    wire::DesktopRequest {
                        device_id: device.id.clone(),
                    },
                )
                .await?;
            let mut request = wire::DesktopActionRequest {
                device_id: device.id.clone(),
                action_id: uuid::Uuid::now_v7().to_string(),
                observation_id: control.observation_id,
                epoch: control.epoch,
                ..Default::default()
            };
            match input {
                DeviceInput::Tap { x, y, right } => {
                    request.kind = "click".into();
                    request.x = x as i32;
                    request.y = y as i32;
                    request.button = if right { 3 } else { 1 };
                    request.clicks = 1;
                }
                DeviceInput::Drag {
                    x, y, to_x, to_y, ..
                } => {
                    request.kind = "drag".into();
                    request.x = x as i32;
                    request.y = y as i32;
                    request.to_x = to_x as i32;
                    request.to_y = to_y as i32;
                    request.button = 1;
                }
                DeviceInput::Scroll { x, y, amount } => {
                    request.kind = "scroll".into();
                    request.x = x as i32;
                    request.y = y as i32;
                    request.amount = amount;
                }
                DeviceInput::Text(text) => {
                    request.kind = "text".into();
                    request.text = text;
                }
                DeviceInput::Key(key) => {
                    request.kind = "key".into();
                    request.key = key;
                }
                DeviceInput::Rotate(_) => return Ok(()),
            }
            let _: wire::DesktopObservation = self.rpc("desktop/action", request).await?;
        }
        Ok(())
    }
}
pub fn epoch_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        * 1000.
}

#[cfg(test)]
impl Connection {
    pub fn for_state_test() -> Self {
        Self {
            http: reqwest::Client::new(),
            assistant: HeaderValue::from_static(""),
            phone: HeaderValue::from_static(""),
            session: HeaderValue::from_static(""),
        }
    }
}
