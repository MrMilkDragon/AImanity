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
    fn finished(&self, id: &str, session: Uuid) {
        let mut map = self.entries.lock().unwrap();
        if matches!(map.get(id), Some(Peer::Connected { session: active, .. }) if *active == session)
        {
            map.remove(id);
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
        .on_failed_upgrade(move |_| failed.finished(&reserved, session))
        .on_upgrade(move |socket| async move {
            peers.signal();
            exchange(socket, rx, stop).await;
            peers.finished(&id, session);
        })
}
async fn exchange(mut socket: WebSocket, mut calls: mpsc::Receiver<Call>, stop: CancellationToken) {
    if socket
        .send(Frame::Binary(bytes::Bytes::new()))
        .await
        .is_err()
    {
        return;
    }
    loop {
        let call = tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            inbound = socket.recv() => match inbound {
                Some(Ok(Frame::Ping(_) | Frame::Pong(_))) => continue,
                _ => break,
            },
            item = calls.recv() => item,
        };
        let Some((request, answer)) = call else {
            break;
        };
        if answer.is_closed() {
            continue;
        }
        if socket
            .send(Frame::Binary(request.encode_to_vec().into()))
            .await
            .is_err()
        {
            break;
        }
        let received = tokio::select! { biased;
            _ = stop.cancelled() => break,
            received = tokio::time::timeout(Duration::from_secs(20), socket.recv()) => received,
        };
        let Ok(Some(Ok(Frame::Binary(bytes)))) = received else {
            break;
        };
        let Ok(reply) = wire::HostDesktopReply::decode(bytes) else {
            break;
        };
        let _ = answer.send(reply);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

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
}
