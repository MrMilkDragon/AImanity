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
        tx: mpsc::Sender<Call>,
        stop: CancellationToken,
        frames: watch::Sender<()>,
    },
}
pub(super) struct Peers {
    id: Uuid,
    entries: Mutex<HashMap<String, Peer>>,
    pub changes: watch::Sender<u64>,
    listener: tokio::sync::Mutex<Option<(SocketAddr, tokio::task::JoinHandle<()>)>>,
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

    pub(super) async fn listen(self: &Arc<Self>, port: u16) -> Result<SocketAddr> {
        let mut listener = self.listener.lock().await;
        if let Some((addr, _)) = listener.as_ref() {
            return Ok(*addr);
        }
        let socket = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
        let addr = socket.local_addr()?;
        let app = Router::new()
            .route("/v1/peer/share", get(accept).head(identity))
            .with_state(Arc::downgrade(self));
        let stop = self.stop.clone();
        *listener = Some((
            addr,
            tokio::spawn(async move {
                if let Err(e) = axum::serve(socket, app)
                    .with_graceful_shutdown(stop.cancelled_owned())
                    .await
                {
                    eprintln!("Device sharing: {e}");
                }
            }),
        ));
        Ok(addr)
    }
    pub(super) async fn close(&self) {
        self.entries.lock().unwrap().clear();
        self.signal();
        if let Some((_, task)) = self.listener.lock().await.take() {
            let _ = task.await;
        }
    }
    pub(super) fn signal(&self) {
        self.changes.send_modify(|n| *n = n.wrapping_add(1));
    }
    pub(super) fn available(&self, mut registry: wire::RegistryState) -> wire::RegistryState {
        let entries = self.entries.lock().unwrap();
        for device in &mut registry.devices {
            if device.paired {
                device.connected = matches!(entries.get(&device.id), Some(Peer::Connected { .. }));
            }
        }
        registry
    }
    pub(super) fn remove<T>(&self, id: &str, change: impl FnOnce() -> Result<T>) -> Result<T> {
        let mut entries = self.entries.lock().unwrap();
        let result = change()?;
        if let Some(Peer::Connected { stop, .. }) = entries.remove(id) {
            stop.cancel();
        }
        drop(entries);
        self.signal();
        Ok(result)
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
            match registry.devices.iter().find(|d| d.id == q.id) {
                Some(d) if d.paired && d.kind == q.kind => {}
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
            map.insert(id.to_owned(), Peer::Offline(reason));
            self.signal();
        }
    }
    pub(super) fn frames(&self, id: &str) -> Option<watch::Receiver<()>> {
        match self.entries.lock().unwrap().get(id) {
            Some(Peer::Connected { frames, .. }) => Some(frames.subscribe()),
            _ => None,
        }
    }
    pub(super) async fn perform(
        &self,
        mut action: wire::DesktopActionRequest,
    ) -> Result<wire::HostDesktopReply> {
        let id = std::mem::take(&mut action.device_id);
        let (tx, stop, frames) = match self.entries.lock().unwrap().get(&id) {
            Some(Peer::Connected {
                tx, stop, frames, ..
            }) => (tx.clone(), stop.clone(), frames.clone()),
            _ => return Err("设备已断开".into()),
        };
        let expected = action.action_id.clone();
        let observe = action.kind == "observe";
        let input = !observe && action.kind != "foreground";
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
async fn identity(State(state): State<Weak<Peers>>) -> Response {
    match state.upgrade() {
        Some(peers) => (
            StatusCode::NO_CONTENT,
            [("x-aimanity-workspace", peers.id.to_string())],
        )
            .into_response(),
        None => StatusCode::GONE.into_response(),
    }
}
async fn accept(
    State(state): State<Weak<Peers>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(peers) = state.upgrade() else {
        return StatusCode::GONE.into_response();
    };
    let header = |name| headers.get(name).and_then(|v| v.to_str().ok());
    let Some(code) = header("authorization")
        .and_then(|h| h.strip_prefix("Bearer "))
        .and_then(|s| s.parse::<Uuid>().ok())
    else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let kind = header("x-aimanity-device");
    if headers.contains_key("origin")
        || (kind == Some("desktop")
            && header("x-aimanity-origin").and_then(|s| s.parse::<Uuid>().ok()) == Some(peers.id))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (tx, rx) = mpsc::channel(1);
    let stop = peers.stop.child_token();
    let session = Uuid::new_v4();
    let id = {
        let mut entries = peers.entries.lock().unwrap();
        if peers.stop.is_cancelled() {
            return StatusCode::GONE.into_response();
        }
        let Some((id, slot)) = entries.iter_mut().find(|(_, slot)| matches!(slot, Peer::Invitation {code: expected, kind: k, issued}
            if *expected == code && kind == Some(k.as_str()) && issued.elapsed() < Duration::from_secs(300))) else { return StatusCode::FORBIDDEN.into_response(); };
        *slot = Peer::Connected {
            session,
            tx,
            stop: stop.clone(),
            frames: watch::channel(()).0,
        };
        id.clone()
    };
    let failed = peers.clone();
    let reserved = id.clone();
    ws.max_message_size(5 * 1024 * 1024)
        .on_failed_upgrade(move |e: axum::Error| failed.finished(&reserved, session, e.to_string()))
        .on_upgrade(move |socket| async move {
            peers.signal();
            if let Err(error) = exchange(socket, rx, stop).await {
                peers.finished(&id, session, error.to_string());
            }
        })
}
async fn exchange(
    mut socket: WebSocket,
    mut calls: mpsc::Receiver<Call>,
    stop: CancellationToken,
) -> Result<()> {
    socket.send(Frame::Binary(bytes::Bytes::new())).await?;
    loop {
        let call = tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            inbound = socket.recv() => match inbound.transpose()? {
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
            _ = stop.cancelled() => return Ok(()),
            received = tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    match socket.recv().await.transpose()? {
                        Some(Frame::Binary(bytes)) => return Result::Ok(wire::HostDesktopReply::decode(bytes)?),
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
        let (mut socket, _) = tokio_tungstenite::connect_async(req).await?;
        socket.next().await.unwrap()?;
        let registry = work.registry()?;
        assert_eq!(registry.devices.len(), 1);
        assert!(registry.devices[0].connected);
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
        let addr = w.peers.listen(0).await?;
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
        let address = work.peers.listen(0).await?;
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
                    .perform(action(source, "type", token.clone()))
                    .await?;
                let got = work
                    .peers
                    .perform(action(source, "foreground", String::new()))
                    .await?
                    .foreground;
                work.peers.perform(action(target, "type", got)).await?;
                assert_eq!(
                    work.peers
                        .perform(action(target, "foreground", String::new()))
                        .await?
                        .foreground,
                    token
                );
            }
        }
        work.dispatch(
            "registry/device-disconnect",
            &wire::DesktopRequest {
                device_id: "phone-b".into(),
            }
            .encode_to_vec(),
        )?;
        assert_eq!(state()?.links.len(), 1);
        assert!(
            work.peers
                .perform(action("phone-b", "type", String::new()))
                .await
                .is_err()
        );
        work.stop().await;
        for client in clients {
            client.await?;
        }
        Ok(())
    }
}
