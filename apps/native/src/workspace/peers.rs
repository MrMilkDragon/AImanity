use super::*;
use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message as Frame, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Mutex, Weak},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

pub(super) const PORT: u16 = 17863;
type Call = (
    wire::DesktopActionRequest,
    oneshot::Sender<wire::HostDesktopReply>,
);
enum Peer {
    Offline(String),
    Invitation {
        code: Uuid,
        kind: String,
        issued: Instant,
    },
    Connected {
        session: Uuid,
        lanes: Arc<Mutex<HashMap<Uuid, oneshot::Sender<WebSocket>>>>,
        guest_audio: bool,
        closed: CancellationToken,
        tx: mpsc::Sender<Call>,
        stop: CancellationToken,
        frames: watch::Sender<()>,
    },
}
struct Listener {
    address: SocketAddr,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}
pub(super) struct Peers {
    id: Uuid,
    entries: Mutex<HashMap<String, Peer>>,
    pub changes: watch::Sender<u64>,
    listener: tokio::sync::Mutex<Option<Listener>>,
    stop: CancellationToken,
}
impl Peers {
    pub(super) fn new(stop: CancellationToken) -> Arc<Self> {
        Arc::new(Self {
            id: Uuid::new_v4(),
            entries: Default::default(),
            changes: watch::channel(0).0,
            listener: Default::default(),
            stop,
        })
    }

    pub(super) async fn listen(&self, workspace: &Arc<Workspace>, port: u16) -> Result<SocketAddr> {
        let mut listener = self.listener.lock().await;
        if let Some(server) = listener.as_ref() {
            return Ok(server.address);
        }
        let socket = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
        let addr = socket.local_addr()?;
        let app = Router::new()
            .route("/v1/peer/share", get(accept).head(identity))
            .route("/v1/peer/io", get(accept_io))
            .with_state(Arc::downgrade(workspace));
        let stop = self.stop.child_token();
        *listener = Some(Listener {
            address: addr,
            stop: stop.clone(),
            task: tokio::spawn(async move {
                if let Err(e) = axum::serve(socket, app)
                    .with_graceful_shutdown(stop.cancelled_owned())
                    .await
                {
                    eprintln!("Device sharing: {e}");
                }
            }),
        });
        Ok(addr)
    }
    pub(super) async fn close(&self) {
        self.entries.lock().unwrap().clear();
        self.signal();
        self.release_idle_listener().await;
    }
    pub(super) async fn release_idle_listener(&self) {
        let mut listener = self.listener.lock().await;
        let idle = self
            .entries
            .lock()
            .unwrap()
            .values()
            .all(|p| matches!(p, Peer::Offline(_)));
        if idle && let Some(server) = listener.take() {
            server.stop.cancel();
            let _ = server.task.await;
        }
    }
    pub(super) fn signal(&self) {
        self.changes.send_modify(|n| *n = n.wrapping_add(1));
    }
    pub(super) fn available(&self, mut registry: wire::RegistryState) -> wire::RegistryState {
        let entries = self.entries.lock().unwrap();
        for device in &mut registry.devices {
            if device.paired {
                let route = entries.get(&device.id);
                device.connected = matches!(route, Some(Peer::Connected { .. }));
                device.guest_audio = matches!(
                    route,
                    Some(Peer::Connected {
                        guest_audio: true,
                        ..
                    })
                );
            }
        }
        registry
    }
    pub(super) fn remove<T>(
        &self,
        id: &str,
        change: impl FnOnce() -> Result<T>,
    ) -> Result<(T, Option<CancellationToken>)> {
        let mut entries = self.entries.lock().unwrap();
        let result = change()?;
        let closed = match entries.remove(id) {
            Some(Peer::Connected { stop, closed, .. }) => {
                stop.cancel();
                Some(closed)
            }
            _ => None,
        };
        drop(entries);
        self.signal();
        Ok((result, closed))
    }
    pub(super) fn invite(
        &self,
        workspace: &Workspace,
        q: wire::DevicePairRequest,
    ) -> Result<wire::DeviceInvitation> {
        if q.id.is_empty()
            || q.name.trim().is_empty()
            || !matches!(q.kind.as_str(), "phone" | "desktop")
        {
            return Err("设备名称或类型无效".into());
        }
        let mut entries = self.entries.lock().unwrap();
        if matches!(entries.get(&q.id), Some(Peer::Connected { .. })) {
            return Err("设备已经连接".into());
        }
        workspace.write_registry(|registry, _| {
            match registry.devices.iter_mut().find(|d| d.id == q.id) {
                Some(d)
                    if d.paired
                        && d.kind == q.kind
                        && !matches!(d.runtime_state.as_str(), "stopping" | "deleting") =>
                {
                    d.runtime_state = "registered".into();
                }
                Some(_) => return Err("该设备不能重新配对为另一种设备".into()),
                None => registry.devices.push(wire::RegisteredDevice {
                    id: q.id.clone(),
                    name: q.name,
                    kind: q.kind.clone(),
                    revision: 1,
                    runtime_state: "registered".into(),
                    paired: true,
                    ..Default::default()
                }),
            }
            Ok(())
        })?;
        let code = Uuid::new_v4();
        entries.insert(
            q.id.clone(),
            Peer::Invitation {
                code,
                kind: q.kind,
                issued: Instant::now(),
            },
        );
        self.signal();
        Ok(wire::DeviceInvitation {
            device_id: q.id,
            code: code.to_string(),
        })
    }
    pub(super) fn disconnect_reason(&self, id: &str) -> Option<String> {
        match self.entries.lock().unwrap().get(id) {
            Some(Peer::Offline(reason)) => Some(reason.clone()),
            _ => None,
        }
    }
    fn finished(&self, id: &str, session: Uuid, reason: String) {
        let mut map = self.entries.lock().unwrap();
        if matches!(map.get(id), Some(Peer::Connected { session: active, .. }) if *active == session)
        {
            if let Some(Peer::Connected { stop, .. }) =
                map.insert(id.to_owned(), Peer::Offline(reason))
            {
                stop.cancel();
            }
            self.signal();
        }
    }
    pub(super) fn active_session(&self, id: &str) -> Result<CancellationToken> {
        match self.entries.lock().unwrap().get(id) {
            Some(Peer::Connected { stop, .. }) => Ok(stop.clone()),
            _ => Err("设备已断开".into()),
        }
    }
    pub(super) fn frames(&self, id: &str) -> Option<watch::Receiver<()>> {
        match self.entries.lock().unwrap().get(id) {
            Some(Peer::Connected { frames, .. }) => Some(frames.subscribe()),
            _ => None,
        }
    }
    pub(super) async fn io(
        &self,
        id: &str,
        request: wire::DeviceIoRequest,
        expected: Option<&CancellationToken>,
    ) -> Result<crate::device_io::Lane> {
        let (lanes, stop) = match self.entries.lock().unwrap().get(id) {
            Some(Peer::Connected { lanes, stop, .. }) if expected.is_none_or(|s| s == stop) => {
                (lanes.clone(), stop.clone())
            }
            _ => return Err("设备已断开".into()),
        };
        let key = Uuid::new_v4();
        let (give, take) = oneshot::channel();
        lanes.lock().unwrap().insert(key, give);
        let _pending = PendingLane { lanes: &lanes, key };
        let open = async {
            self.perform(
                wire::DesktopActionRequest {
                    device_id: id.into(),
                    action_id: key.to_string(),
                    io: Some(request),
                    ..Default::default()
                },
                Some(&stop),
            )
            .await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(take.await?)
        };
        let socket = tokio::select! { biased;
            _ = stop.cancelled() => return Err("设备已断开".into()),
            result = tokio::time::timeout(Duration::from_secs(10), open) => result??,
        };
        Ok(crate::device_io::Lane { socket, stop })
    }
    pub(super) async fn perform(
        &self,
        mut action: wire::DesktopActionRequest,
        expected_session: Option<&CancellationToken>,
    ) -> Result<wire::HostDesktopReply> {
        let id = std::mem::take(&mut action.device_id);
        let (tx, stop, frames) = match self.entries.lock().unwrap().get(&id) {
            Some(Peer::Connected {
                tx, stop, frames, ..
            }) if expected_session.is_none_or(|expected| expected == stop) => {
                (tx.clone(), stop.clone(), frames.clone())
            }
            _ => return Err("设备已断开".into()),
        };
        let expected = action.action_id.clone();
        let observe = action.kind == "observe";
        let input = !observe
            && action.kind != "foreground"
            && action.io.is_none()
            && action.location.is_none();
        let (sender, receiver) = oneshot::channel();
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err("共享已结束，设备未确认操作结果".into()),
            receipt = async {
                tx.send((action, sender)).await.map_err(|_| "设备已断开")?;
                receiver.await.map_err(|_| "设备未确认操作结果")
            } => {
                let reply = receipt?;
                if expected.is_empty() || reply.action_id != expected { return Err("设备回执不匹配".into()); }
                if !reply.error.is_empty() { return Err(reply.error.into()); }
                if observe && (reply.observation_id.is_empty() || reply.width == 0 || reply.height == 0 || !reply.jpeg.starts_with(&[0xff,0xd8])) {
                    return Err("设备未返回完整画面".into());
                }
                if input { frames.send_replace(()); }
                Ok(reply)
            }
        }
    }
}
struct PendingLane<'a> {
    lanes: &'a Mutex<HashMap<Uuid, oneshot::Sender<WebSocket>>>,
    key: Uuid,
}
impl Drop for PendingLane<'_> {
    fn drop(&mut self) {
        self.lanes.lock().unwrap().remove(&self.key);
    }
}
async fn accept_io(
    State(state): State<Weak<Workspace>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let key = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .and_then(|v| Uuid::parse_str(v).ok());
    if headers.contains_key("origin") {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some((workspace, key)) = state.upgrade().zip(key) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let grant = workspace
        .peers
        .entries
        .lock()
        .unwrap()
        .values()
        .find_map(|peer| match peer {
            Peer::Connected { lanes, .. } => lanes.lock().unwrap().remove(&key),
            _ => None,
        });
    match grant {
        Some(give) => ws
            .max_message_size(4 * 1024 * 1024)
            .on_upgrade(move |socket| async move {
                let _ = give.send(socket);
            }),
        None => StatusCode::FORBIDDEN.into_response(),
    }
}
async fn identity(State(state): State<Weak<Workspace>>) -> Response {
    match state.upgrade() {
        Some(workspace) => (
            StatusCode::NO_CONTENT,
            [("x-aimanity-workspace", workspace.peers.id.to_string())],
        )
            .into_response(),
        None => StatusCode::GONE.into_response(),
    }
}
async fn accept(
    State(state): State<Weak<Workspace>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(workspace) = state.upgrade() else {
        return StatusCode::GONE.into_response();
    };
    let peers = workspace.peers.clone();
    let header = |name| headers.get(name).and_then(|v| v.to_str().ok());
    let Some(code) = header("authorization")
        .and_then(|h| h.strip_prefix("Bearer "))
        .and_then(|s| s.parse::<Uuid>().ok())
    else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let kind = header("x-aimanity-device");
    let Some(system) = header("x-aimanity-system").and_then(ai_phone_api::platform_name) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if kind
        != Some(if system == "Android" {
            "phone"
        } else {
            "desktop"
        })
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if headers.contains_key("origin")
        || (kind == Some("desktop")
            && header("x-aimanity-origin").and_then(|s| s.parse::<Uuid>().ok()) == Some(peers.id))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (tx, rx) = mpsc::channel(1);
    let stop = peers.stop.child_token();
    let session = Uuid::new_v4();
    let closed = CancellationToken::new();
    let id = {
        let mut entries = peers.entries.lock().unwrap();
        if peers.stop.is_cancelled() {
            return StatusCode::GONE.into_response();
        }
        let Some((id, slot)) = entries.iter_mut().find(|(_, slot)| matches!(slot, Peer::Invitation {code: expected, kind: k, issued}
            if *expected == code && kind == Some(k.as_str()) && issued.elapsed() < Duration::from_secs(300))) else { return StatusCode::FORBIDDEN.into_response(); };
        if let Err(error) = workspace.write_registry(|state, _| {
            let device = state
                .devices
                .iter_mut()
                .find(|device| device.id == *id)
                .ok_or("设备不存在")?;
            device.system = system.into();
            Ok(())
        }) {
            eprintln!("Device registration: {error}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        *slot = Peer::Connected {
            session,
            closed: closed.clone(),
            guest_audio: kind == Some("desktop") && header("x-aimanity-audio") == Some("guest-pcm"),
            lanes: Default::default(),
            tx,
            stop: stop.clone(),
            frames: watch::channel(()).0,
        };
        id.clone()
    };
    let failed = peers.clone();
    let reserved = id.clone();
    let done = closed.drop_guard();
    let workspace = Arc::downgrade(&workspace);
    ws.max_message_size(5 * 1024 * 1024)
        .on_failed_upgrade(move |e: axum::Error| failed.finished(&reserved, session, e.to_string()))
        .on_upgrade(move |socket| async move {
            let _done = done;
            peers.signal();
            if let Some(w) = workspace.upgrade() {
                w.ready.notify_one();
            }
            if let Err(error) = exchange(socket, rx, stop, &workspace, &id).await {
                peers.finished(&id, session, error.to_string());
            }
        })
}
async fn close_session(socket: &mut WebSocket) -> Result<()> {
    socket.send(Frame::Close(None)).await?;
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(Ok(msg)) = socket.recv().await {
            if matches!(msg, Frame::Close(_)) {
                break;
            }
        }
    })
    .await;
    Ok(())
}
async fn exchange(
    mut socket: WebSocket,
    mut calls: mpsc::Receiver<Call>,
    stop: CancellationToken,
    workspace: &Weak<Workspace>,
    device: &str,
) -> Result<()> {
    socket.send(Frame::Binary(bytes::Bytes::new())).await?;
    loop {
        let call = tokio::select! {
            biased;
            _ = stop.cancelled() => return close_session(&mut socket).await,
            inbound = socket.recv() => match inbound.transpose()? {
                Some(Frame::Binary(bytes)) => {
                    if let Some(notice)=wire::HostDesktopReply::decode(bytes)?.notification{workspace.upgrade().ok_or("工作区已关闭")?.receive_notice(device,notice)?;continue;}
                    return Err("设备返回了未请求的回执".into());
                }
                Some(Frame::Ping(_) | Frame::Pong(_)) => continue,
                Some(Frame::Close(Some(close))) if !close.reason.is_empty() => return Err(close.reason.to_string().into()),
                _ => return Err("共享端已关闭连接".into()),
            },
            item = calls.recv() => item,
        };
        let Some((request, answer)) = call else {
            return Ok(());
        };
        if answer.is_closed() {
            continue;
        }
        socket
            .send(Frame::Binary(request.encode_to_vec().into()))
            .await?;
        let reply = tokio::select! { biased;
            _ = stop.cancelled() => return close_session(&mut socket).await,
            received = tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    match socket.recv().await.transpose()? {
                        Some(Frame::Binary(bytes)) => {
                            let reply=wire::HostDesktopReply::decode(bytes)?;
                            if let Some(notice)=reply.notification {workspace.upgrade().ok_or("工作区已关闭")?.receive_notice(device,notice)?; continue;}
                            return Result::Ok(reply);
                        },
                        Some(Frame::Ping(_) | Frame::Pong(_)) => continue,
                        Some(Frame::Close(Some(close))) if !close.reason.is_empty() => return Err(close.reason.to_string().into()),
                        _ => return Err("设备在返回回执前断开连接".into()),
                    }
                }
            }) => received.map_err(|_| "设备回执超时")?,
        };
        let _ = answer.send(reply?);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    #[tokio::test]
    async fn failed_listener_start_retries_the_same_device_and_accepts_a_real_connection()
    -> Result<()> {
        let dir = tempfile::tempdir()?;
        let work = Workspace::open(dir.path())?;
        let held = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = held.local_addr()?;
        let device = wire::DevicePairRequest {
            id: "repair".into(),
            name: "Phone".into(),
            kind: "phone".into(),
        };
        let error = work
            .start_pairing(device.clone(), addr.port())
            .await
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::AddrInUse
        );
        assert_eq!(work.registry()?.devices.len(), 1);
        drop(held);
        let invite = work.start_pairing(device, addr.port()).await?;
        let mut req = format!("ws://{addr}/v1/peer/share").into_client_request()?;
        req.headers_mut()
            .insert("authorization", format!("Bearer {}", invite.code).parse()?);
        req.headers_mut()
            .insert("x-aimanity-device", "phone".parse()?);
        req.headers_mut()
            .insert("x-aimanity-system", "android".parse()?);
        let mut mismatched = req.clone();
        mismatched
            .headers_mut()
            .insert("x-aimanity-system", "windows".parse()?);
        let err = tokio_tungstenite::connect_async(mismatched)
            .await
            .unwrap_err();
        assert!(
            matches!(err,tokio_tungstenite::tungstenite::Error::Http(response) if response.status()==StatusCode::BAD_REQUEST)
        );
        let (mut socket, _) = tokio_tungstenite::connect_async(req).await?;
        socket.next().await.unwrap()?;
        let registry = work.registry()?;
        assert_eq!(registry.devices.len(), 1);
        assert!(registry.devices[0].connected);
        assert_eq!(registry.devices[0].system_name(), "Android");
        let mut changed = work.registry_changes();
        socket
            .close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
                reason: "peer ended session".into(),
            }))
            .await?;
        changed.changed().await?;
        assert!(!work.registry()?.devices[0].connected);
        assert_eq!(
            work.disconnect_reason("repair").as_deref(),
            Some("peer ended session")
        );
        work.stop().await;
        Ok(())
    }
    #[tokio::test]
    async fn closing_an_open_workspace_revokes_the_upgraded_socket() -> Result<()> {
        let root = tempfile::tempdir()?;
        let w = Workspace::open(root.path())?;
        let addr = w.peers.listen(&w, 0).await?;
        let invite = w.peers.invite(
            &w,
            wire::DevicePairRequest {
                id: "a".into(),
                name: "A".into(),
                kind: "phone".into(),
            },
        )?;
        let mut req = format!("ws://{addr}/v1/peer/share").into_client_request()?;
        req.headers_mut()
            .insert("authorization", format!("Bearer {}", invite.code).parse()?);
        req.headers_mut()
            .insert("x-aimanity-device", "phone".parse()?);
        req.headers_mut()
            .insert("x-aimanity-system", "android".parse()?);
        let (mut socket, _) = tokio_tungstenite::connect_async(req).await?;
        assert!(socket.next().await.unwrap()?.is_binary());
        assert!(w.registry()?.devices[0].connected);
        w.stop().await;
        assert!(!w.registry()?.devices[0].connected);
        assert!(w.peers.frames("a").is_none());
        Ok(())
    }

    #[tokio::test]
    async fn three_authorized_peers_share_routes_without_a_link_command() -> Result<()> {
        let root = tempfile::tempdir()?;
        let work = Workspace::open(root.path())?;
        let address = work.peers.listen(&work, 0).await?;
        let ids = ["phone-a", "phone-b", "desktop"];
        let mut clients = Vec::new();
        for id in ids {
            let kind = if id == "desktop" { "desktop" } else { "phone" };
            let invitation = work.peers.invite(
                &work,
                wire::DevicePairRequest {
                    id: id.into(),
                    name: id.into(),
                    kind: kind.into(),
                },
            )?;
            let mut req = format!("ws://{address}/v1/peer/share").into_client_request()?;
            req.headers_mut().insert(
                "authorization",
                format!("Bearer {}", invitation.code).parse()?,
            );
            req.headers_mut().insert("x-aimanity-device", kind.parse()?);
            req.headers_mut().insert(
                "x-aimanity-system",
                if kind == "phone" {
                    "android"
                } else {
                    "windows"
                }
                .parse()?,
            );
            let (mut stream, _) = tokio_tungstenite::connect_async(req).await?;
            assert!(stream.next().await.unwrap()?.is_binary());
            clients.push(tokio::spawn(async move {
                use futures_util::SinkExt;
                let mut text = id.to_owned();
                while let Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes))) =
                    stream.next().await
                {
                    let action = wire::DesktopActionRequest::decode(bytes).unwrap();
                    if action.kind == "type" {
                        text = action.text;
                    }
                    let reply = wire::HostDesktopReply {
                        action_id: action.action_id,
                        foreground: text.clone(),
                        ..Default::default()
                    };
                    if stream
                        .send(tokio_tungstenite::tungstenite::Message::Binary(
                            reply.encode_to_vec().into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        let state = || {
            wire::DeviceLinkState::decode(work.dispatch("desktop/state", &[])?.as_slice())
                .map_err(Into::into)
        };
        let mesh: Result<wire::DeviceLinkState> = state();
        assert_eq!(mesh?.links.len(), 3);
        let action = |id: &str, kind: &str, value: String| wire::DesktopActionRequest {
            device_id: id.into(),
            kind: kind.into(),
            text: value,
            action_id: Uuid::now_v7().to_string(),
            ..Default::default()
        };
        for source in ids {
            for target in ids.into_iter().filter(|t| *t != source) {
                let token = format!("{source} to {target}");
                work.peers
                    .perform(action(source, "type", token.clone()), None)
                    .await?;
                let got = work
                    .peers
                    .perform(action(source, "foreground", String::new()), None)
                    .await?
                    .foreground;
                work.peers
                    .perform(action(target, "type", got), None)
                    .await?;
                assert_eq!(
                    work.peers
                        .perform(action(target, "foreground", String::new()), None)
                        .await?
                        .foreground,
                    token
                );
            }
        }
        work.rpc(
            "registry/device-disconnect",
            wire::DesktopRequest {
                device_id: "phone-b".into(),
            }
            .encode_to_vec(),
        )
        .await?;
        assert_eq!(state()?.links.len(), 1);
        assert!(
            work.peers
                .perform(action("phone-b", "type", String::new()), None)
                .await
                .is_err()
        );
        work.stop().await;
        for client in clients {
            client.await?;
        }
        Ok(())
    }
    #[cfg(not(target_os = "android"))]
    #[tokio::test]
    async fn file_transfers_use_selected_peers_and_lifecycle_preserves_the_other_devices()
    -> Result<()> {
        use crate::{device_files::Endpoint, device_io::Services, host_desktop, peer_connection};
        let root = tempfile::tempdir()?;
        let work = Workspace::open(root.path())?;
        let address = work.peers.listen(&work, 0).await?;
        let mut shares = Vec::new();
        let share = async |id: &str| {
            let dir = root.path().join(id);
            let invite = work.peers.invite(
                &work,
                wire::DevicePairRequest {
                    id: id.into(),
                    name: "Studio".into(),
                    kind: "desktop".into(),
                },
            )?;
            let mut req = format!("ws://{address}/v1/peer/share").into_client_request()?;
            req.headers_mut()
                .insert("authorization", format!("Bearer {}", invite.code).parse()?);
            req.headers_mut()
                .insert("x-aimanity-device", "desktop".parse()?);
            let os = if id == "a" {
                "windows"
            } else if id == "b" {
                "macos"
            } else {
                "linux"
            };
            req.headers_mut().insert("x-aimanity-system", os.parse()?);
            let target = address.port().to_string();
            let (ws, _) = peer_connection::dial(&target, req).await?;
            let services = Services {
                target,
                files: Endpoint::Directory(dir),
                guest_audio: false,
                pulse_server: None,
            };
            let (ready, opened) = oneshot::channel();
            let (keep, stop) = watch::channel(());
            let task = tokio::spawn(host_desktop::serve_socket(ws, None, services, ready, stop));
            opened.await?;
            Result::Ok((keep, task))
        };
        for id in ["a", "b", "c"] {
            let dir = root.path().join(id);
            std::fs::create_dir(&dir)?;
            std::fs::write(dir.join("origin"), id)?;
            shares.push(share(id).await?);
        }
        let devices = work.registry()?.devices;
        for (id, system) in [("a", "Windows"), ("b", "macOS"), ("c", "Linux")] {
            let device = devices.iter().find(|d| d.id == id).unwrap();
            assert!(device.connected);
            assert_eq!(device.system_name(), system);
        }
        let loc = |device: &str, path: &str| wire::FileLocation {
            device_id: device.into(),
            path: path.into(),
        };
        let manage = async |op: wire::FileOperation,
                            src: wire::FileLocation,
                            to: wire::FileLocation,
                            name: &str| {
            let result = work
                .rpc(
                    "files/manage",
                    wire::FileManageRequest {
                        operation: op.into(),
                        sources: vec![src],
                        target: Some(to),
                        name: name.into(),
                    }
                    .encode_to_vec(),
                )
                .await?;
            Result::Ok(
                wire::FileManageResult::decode(result.as_slice())?
                    .results
                    .remove(0),
            )
        };
        let data: Vec<u8> = (0..670_313).map(|i| (i % 227) as u8).collect();
        std::fs::write(root.path().join("a/blob"), &data)?;
        for (src, to, path, new_name) in [
            ("a", "c", "/blob", "result"),
            ("c", "b", "/result", "same"),
            ("b", "", "/same", "receipt"),
        ] {
            assert!(
                manage(
                    wire::FileOperation::Copy,
                    loc(src, path),
                    loc(to, "/"),
                    new_name
                )
                .await?
                .error
                .is_empty()
            );
        }
        assert_eq!(std::fs::read(work.files.join("receipt"))?, data);
        std::fs::create_dir(root.path().join("a/folder"))?;
        std::fs::write(root.path().join("a/folder/note"), "图纸")?;
        assert!(
            manage(
                wire::FileOperation::Move,
                loc("a", "/folder"),
                loc("b", "/"),
                "folder"
            )
            .await?
            .error
            .is_empty()
        );
        assert!(!root.path().join("a/folder").exists());
        assert_eq!(
            std::fs::read_to_string(root.path().join("b/folder/note"))?,
            "图纸"
        );
        assert!(
            !manage(
                wire::FileOperation::Move,
                loc("a", "/origin"),
                loc("b", "/"),
                "origin"
            )
            .await?
            .error
            .is_empty()
        );
        assert_eq!(std::fs::read(root.path().join("a/origin"))?, b"a");
        assert_eq!(std::fs::read(root.path().join("b/origin"))?, b"b");
        let original_session = work.peers.active_session("a")?;
        work.end_device("a", super::super::lifecycle::Change::Disconnect)
            .await?;
        assert!(work.file_list(loc("a", "/")).await.is_err());
        assert!(
            work.registry()?
                .devices
                .iter()
                .any(|d| d.id == "a" && !d.connected && d.runtime_state == "stopped")
        );
        let (a, task) = shares.remove(0);
        task.await??;
        drop(a);
        shares.push(share("a").await?);
        assert!(original_session.is_cancelled());
        let stale = work
            .peers
            .perform(
                wire::DesktopActionRequest {
                    device_id: "a".into(),
                    kind: "foreground".into(),
                    ..Default::default()
                },
                Some(&original_session),
            )
            .await;
        assert_eq!(stale.unwrap_err().to_string(), "设备已断开");
        assert!(
            work.peers
                .io(
                    "a",
                    wire::DeviceIoRequest::default(),
                    Some(&original_session)
                )
                .await
                .is_err()
        );
        assert!(
            work.file_list(loc("a", "/"))
                .await?
                .entries
                .iter()
                .any(|e| e.name == "origin")
        );
        assert!(
            work.file_list(loc("b", "/"))
                .await?
                .entries
                .iter()
                .any(|e| e.name == "same")
        );
        work.end_device("c", super::super::lifecycle::Change::Delete)
            .await?;
        assert!(work.registry()?.devices.iter().all(|d| d.id != "c"));
        assert_eq!(std::fs::read(root.path().join("c/result"))?, data);
        let guest = "owned";
        work.dispatch(
            "registry/device-save",
            &wire::DeviceSaveRequest {
                id: guest.into(),
                name: "Linux".into(),
                kind: "desktop".into(),
                ..Default::default()
            }
            .encode_to_vec(),
        )?;
        let disk = work.device_home(guest).join("disk");
        std::fs::write(&disk, b"guest data")?;
        work.end_device(guest, super::super::lifecycle::Change::Disconnect)
            .await?;
        assert_eq!(std::fs::read(&disk)?, b"guest data");
        work.end_device(guest, super::super::lifecycle::Change::Delete)
            .await?;
        assert!(!work.device_home(guest).exists());
        assert_eq!(std::fs::read(work.files.join("receipt"))?, data);
        for id in ["a", "b"] {
            work.end_device(id, super::super::lifecycle::Change::Disconnect)
                .await?;
        }
        assert!(work.peers.listener.lock().await.is_none());
        work.stop().await;
        for (sender, task) in shares {
            drop(sender);
            let _ = task.await?;
        }
        Ok(())
    }
}
