use ai_phone_api::wire;
use futures_util::SinkExt;
use prost::Message;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug)]
pub struct Connection {
    pub(crate) workspace: Arc<crate::workspace::Workspace>,
    devices_changed: tokio::sync::watch::Receiver<u64>,
}
#[derive(Clone, Debug)]
pub struct Session {
    pub client: Connection,
    pub registry: wire::RegistryState,
}
#[derive(Clone, Debug)]
pub struct Frame {
    pub cursor: Option<Arc<wire::DeviceCursor>>,
    pub observation_id: String,
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<Vec<u8>>,
    pub sequence: u64,
    pub captured_ms: f64,
    pub received: Instant,
}

#[derive(Clone, Debug, Default)]
pub struct LatestFrame(Arc<Mutex<Option<Result<Frame, String>>>>);

impl LatestFrame {
    fn replace(&self, frame: Result<Frame, String>) -> bool {
        let previous = self.0.lock().unwrap().replace(frame);
        previous.is_none()
    }

    pub fn take(&self) -> Option<Result<Frame, String>> {
        self.0.lock().unwrap().take()
    }
}

const RECONNECT_DELAY: Duration = Duration::from_secs(1);
#[derive(Clone, Copy, Debug)]
pub enum Press {
    Left,
    Right,
    Hold,
    Second,
}

#[derive(Clone, Debug)]
pub enum DeviceInput {
    Tap {
        x: u32,
        y: u32,
        press: Press,
    },
    Drag {
        x: u32,
        y: u32,
        to_x: u32,
        to_y: u32,
        duration: u32,
        hold: bool,
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
pub(crate) fn reachable(device: &wire::RegisteredDevice) -> bool {
    device.connected
        && !matches!(
            device.runtime_state.as_str(),
            "stopping" | "deleting" | "stopped"
        )
}
impl Connection {
    pub async fn connect(root: String) -> Result<Session, String> {
        let workspace = tokio::task::spawn_blocking(move || {
            crate::workspace::Workspace::open(std::path::Path::new(&root))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        workspace.run();
        let client = Self {
            devices_changed: workspace.registry_changes(),
            workspace,
        };
        client.session().await
    }
    pub async fn session(mut self) -> Result<Session, String> {
        self.devices_changed.borrow_and_update();
        let registry = self.workspace.registry().map_err(|e| e.to_string())?;
        Ok(Session {
            client: self,
            registry,
        })
    }
    pub async fn rpc<I: Message, O: Message + Default>(
        &self,
        route: &str,
        input: I,
    ) -> Result<O, String> {
        let started = Instant::now();
        let bytes = self.workspace.rpc(route, input.encode_to_vec()).await?;
        eprintln!(
            "native_metric rpc route={route} ms={:.3} bytes={}",
            started.elapsed().as_secs_f64() * 1000.,
            bytes.len()
        );
        O::decode(bytes.as_slice()).map_err(|e| e.to_string())
    }
    pub async fn history(&self) -> Result<wire::ChatState, String> {
        let workspace = self.workspace.clone();
        tokio::task::spawn_blocking(move || {
            workspace
                .history(wire::ChatHistoryRequest {
                    limit: 80,
                    ..Default::default()
                })
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
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
        let workspace = self.workspace.clone();
        tokio::task::spawn_blocking(move || workspace.upload(name, bytes))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }
    pub async fn get_bytes(&self, route: &str) -> Result<bytes::Bytes, String> {
        let id = route
            .strip_prefix("workbench/knowledge-image/")
            .ok_or("文件地址无效")?;
        self.attachment(id).await
    }
    pub async fn attachment(&self, id: &str) -> Result<bytes::Bytes, String> {
        let workspace = self.workspace.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || workspace.attachment(&id))
            .await
            .map_err(|e| e.to_string())?
            .map(Into::into)
            .map_err(|e| e.to_string())
    }
    pub async fn download_file(
        &self,
        source: &str,
        target: &std::path::Path,
    ) -> Result<(), String> {
        self.workspace
            .export_file(source, target)
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn upload_file(
        &self,
        source: &std::path::Path,
        directory: String,
    ) -> Result<wire::FileLocation, String> {
        self.workspace
            .import_file(source, &directory)
            .await
            .map_err(|e| e.to_string())
    }
    pub fn chat_events(self) -> impl futures_util::Stream<Item = Result<wire::ChatState, String>> {
        iced::stream::channel(1, async move |mut output| {
            let mut changes = self.workspace.subscribe();
            loop {
                if output.send(self.history().await).await.is_err()
                    || changes.changed().await.is_err()
                {
                    break;
                }
            }
        })
    }
    pub fn device_events(
        mut self,
    ) -> impl futures_util::Stream<Item = Result<wire::RegistryState, String>> {
        iced::stream::channel(1, async move |mut output| {
            while self.devices_changed.changed().await.is_ok() {
                let state = self.workspace.registry().map_err(|e| e.to_string());
                if output.send(state).await.is_err() {
                    break;
                }
            }
        })
    }
    pub fn frames(
        self,
        device: wire::RegisteredDevice,
        mut visible: tokio::sync::watch::Receiver<bool>,
    ) -> impl futures_util::Stream<Item = LatestFrame> {
        iced::stream::channel::<LatestFrame>(1, async move |mut output| {
            let pending = LatestFrame::default();
            let Some(mut input) = self.workspace.frame_updates(&device.id) else {
                return;
            };
            let mut previous: Option<(Vec<u8>, Arc<Vec<u8>>)> = None;
            let mut sequence = 0;
            loop {
                if visible.wait_for(|on| *on).await.is_err() {
                    return;
                }
                let received = Instant::now();
                let mut delay = RECONNECT_DELAY;
                let result = async {
                    let reply = self
                        .workspace
                        .manual_input(wire::DesktopActionRequest {
                            device_id: device.id.clone(),
                            action_id: uuid::Uuid::now_v7().to_string(),
                            kind: "observe".into(),
                            ..Default::default()
                        })
                        .await?;
                    let pixels = match &previous {
                        Some((jpeg, pixels)) if *jpeg == reply.jpeg => {
                            delay = Duration::from_millis(350);
                            pixels.clone()
                        }
                        _ => {
                            let jpeg = reply.jpeg;
                            let (jpeg, pixels) = tokio::task::spawn_blocking(move || {
                                let image = image::load_from_memory_with_format(
                                    &jpeg,
                                    image::ImageFormat::Jpeg,
                                )
                                .map_err(|e| e.to_string())?
                                .into_rgba8();
                                if image.dimensions() != (reply.width, reply.height) {
                                    return Err("画面尺寸与设备回执不一致".to_string());
                                }
                                Ok((jpeg, Arc::new(image.into_raw())))
                            })
                            .await
                            .map_err(|e| e.to_string())??;
                            previous = Some((jpeg, pixels.clone()));
                            sequence += 1;
                            delay = Duration::from_millis(65);
                            pixels
                        }
                    };
                    Ok(Frame {
                        cursor: input.borrow().clone(),
                        observation_id: reply.observation_id,
                        width: reply.width,
                        height: reply.height,
                        pixels,
                        sequence,
                        captured_ms: 0.,
                        received,
                    })
                }
                .await;
                if pending.replace(result) && output.send(pending.clone()).await.is_err() {
                    return;
                }
                tokio::select! {
                    changed=visible.changed()=>if changed.is_err(){return;},
                    result = input.changed() => if result.is_err() { return; },
                    _ = tokio::time::sleep_until((received + delay).into()) => {}
                }
            }
        })
    }
    pub async fn input(
        &self,
        device: &wire::RegisteredDevice,
        input: DeviceInput,
        observation: String,
    ) -> Result<(), String> {
        if observation.is_empty() {
            return Err("请先获取设备画面".into());
        }
        let mut action = wire::DesktopActionRequest {
            device_id: device.id.clone(),
            action_id: uuid::Uuid::now_v7().to_string(),
            observation_id: observation,
            ..Default::default()
        };
        action.kind = match input {
            DeviceInput::Tap { x, y, press } => {
                action.x = x as i32;
                action.y = y as i32;
                match press {
                    Press::Left => "click",
                    Press::Second => "double_click",
                    _ if device.kind == "phone" => "long_press",
                    _ => "right_click",
                }
            }
            DeviceInput::Drag {
                x,
                y,
                to_x,
                to_y,
                duration,
                hold,
            } => {
                action.x = x as i32;
                action.y = y as i32;
                action.to_x = to_x as i32;
                action.to_y = to_y as i32;
                action.duration_ms = duration;
                if device.kind == "phone" && !hold {
                    "swipe"
                } else {
                    "drag"
                }
            }
            DeviceInput::Text(v) => {
                action.text = v;
                "type"
            }
            DeviceInput::Key(v) => {
                action.key = v.to_ascii_lowercase();
                "key"
            }
            DeviceInput::Scroll { x, y, amount } => {
                action.x = x as i32;
                action.y = y as i32;
                action.amount = if device.kind == "phone" {
                    amount.signum() * 2
                } else {
                    amount
                };
                "scroll"
            }
            DeviceInput::Rotate(rotation) => {
                action.amount = rotation as i32;
                "rotate"
            }
        }
        .into();
        self.workspace.manual_input(action).await?;
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
        let workspace = crate::workspace::Workspace::from_db(
            redb::Database::builder()
                .create_with_backend(redb::backends::InMemoryBackend::new())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
        Self {
            devices_changed: workspace.registry_changes(),
            workspace,
        }
    }
}

#[cfg(test)]
#[test]
fn stalled_renderer_keeps_the_latest_frame_and_one_wakeup() {
    let slot = LatestFrame::default();
    let mut previous = std::sync::Weak::new();
    let mut wakes = 0;
    for sequence in 1..=120 {
        let pixels = Arc::new(vec![0; 16]);
        let live = Arc::downgrade(&pixels);
        wakes += usize::from(slot.replace(Ok(Frame {
            cursor: None,
            observation_id: String::new(),
            width: 2,
            height: 2,
            pixels,
            sequence,
            captured_ms: 0.,
            received: Instant::now(),
        })));
        assert!(previous.upgrade().is_none());
        previous = live;
    }
    assert_eq!(wakes, 1);
    assert_eq!(slot.take().unwrap().unwrap().sequence, 120);
    assert!(slot.replace(Err("disconnected".into())));
    assert_eq!(slot.take().unwrap().unwrap_err(), "disconnected");
}
