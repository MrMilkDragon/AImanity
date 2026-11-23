use crate::{host_desktop, *};
use axum::extract::{State, ws::WebSocketUpgrade};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Mutex, time::Instant};
use tokio::sync::{mpsc, oneshot};

enum Session {
    Invitation {
        code: Uuid,
        kind: String,
        issued: Instant,
    },
    Connected {
        key: Uuid,
        kind: String,
        tx: mpsc::Sender<host_desktop::Call>,
        stop: CancellationToken,
    },
}
impl Session {
    fn accepts(&self, candidate: Uuid, kind: &str) -> bool {
        matches!(self, Self::Invitation {code, kind: wanted, issued} if *code == candidate && wanted == kind && issued.elapsed() < Duration::from_secs(300))
    }
}

pub(crate) struct Sessions(Mutex<HashMap<String, Session>>, Uuid);
impl Default for Sessions {
    fn default() -> Self {
        Self(Default::default(), Uuid::new_v4())
    }
}

impl Sessions {
    pub fn contains_output(&self, output: &Value) -> bool {
        crate::worker::tool_value(output).is_some_and(|receipt| self.current_receipt(&receipt))
    }
    pub fn current_receipt(&self, receipt: &Value) -> bool {
        let Some(id) = receipt["device_id"].as_str() else {
            return false;
        };
        let Some(session) = receipt["peer_session"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
        else {
            return false;
        };
        matches!(self.0.lock().unwrap().get(id), Some(Session::Connected {key, ..}) if *key == session)
    }
    pub fn invite(&self, id: &str, kind: &str) -> Result<wire::DeviceInvitation, String> {
        let mut entries = self.0.lock().unwrap();
        if matches!(entries.get(id), Some(Session::Connected { .. })) {
            return Err("设备已经连接".into());
        }
        let code = Uuid::new_v4();
        entries.insert(
            id.into(),
            Session::Invitation {
                code,
                kind: kind.into(),
                issued: Instant::now(),
            },
        );
        Ok(wire::DeviceInvitation {
            device_id: id.into(),
            code: code.to_string(),
        })
    }

    pub fn any_connected(&self) -> bool {
        self.0
            .lock()
            .unwrap()
            .values()
            .any(|entry| matches!(entry, Session::Connected { .. }))
    }
    pub fn connected(&self, id: &str) -> bool {
        matches!(
            self.0.lock().unwrap().get(id),
            Some(Session::Connected { .. })
        )
    }
    pub async fn foreground(&self, device: &str) -> Option<String> {
        let request = wire::DesktopActionRequest {
            device_id: device.into(),
            action_id: Uuid::now_v7().to_string(),
            kind: "foreground".into(),
            ..Default::default()
        };
        let (_, _, reply) = tokio::time::timeout(Duration::from_millis(500), self.perform(request))
            .await
            .ok()?
            .ok()?;
        (!reply.foreground.is_empty()).then_some(reply.foreground)
    }
    pub fn disconnect(&self, id: &str) {
        if let Some(Session::Connected { stop, .. }) = self.0.lock().unwrap().remove(id) {
            stop.cancel();
        }
    }
    fn finish(&self, id: &str, key: Uuid) {
        let mut entries = self.0.lock().unwrap();
        if matches!(entries.get(id), Some(Session::Connected { key: active, .. }) if *active == key)
        {
            entries.remove(id);
        }
    }
    async fn perform(
        &self,
        mut action: wire::DesktopActionRequest,
    ) -> Result<(Uuid, String, wire::HostDesktopReply), String> {
        let id = std::mem::take(&mut action.device_id);
        let (key, kind, tx, stop) = match self.0.lock().unwrap().get(&id) {
            Some(Session::Connected {
                key,
                kind,
                tx,
                stop,
            }) => (*key, kind.clone(), tx.clone(), stop.clone()),
            _ => return Err("设备已断开".into()),
        };
        let expected = Uuid::parse_str(&action.action_id).map_err(|_| "缺少操作编号")?;
        let observe = action.kind == "observe";
        let (reply, result) = oneshot::channel();
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err("设备共享已结束，操作结果未知".into()),
            response = async {
                tx.send((action,reply)).await.map_err(|_| "设备已断开")?;
                let receipt = result.await.map_err(|_| "连接中断，操作结果未知")?;
                if Uuid::parse_str(&receipt.action_id).ok() != Some(expected) {return Err("设备回执不匹配".into());}
                if !receipt.error.is_empty() { return Err(receipt.error); }
                if observe && (receipt.observation_id.is_empty() || receipt.width==0 || receipt.height==0 || !receipt.jpeg.starts_with(&[0xff,0xd8])) {return Err("共享设备没有返回完整画面".into());}
                Ok((key, kind, receipt))
            } => response
        }
    }
}
fn invitation_code(headers: &HeaderMap) -> Option<Uuid> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?
        .parse()
        .ok()
}
pub async fn identity(State(app): State<Shared>) -> impl axum::response::IntoResponse {
    (
        StatusCode::NO_CONTENT,
        [("x-aimanity-workspace", app.run_signals.peers.1.to_string())],
    )
}
pub async fn connect(
    State(app): State<Shared>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let rejected = || {
        error(
            StatusCode::FORBIDDEN,
            "invalid_invitation",
            "配对码无效或已过期",
        )
    };
    let (Some(code), Some(kind)) = (
        invitation_code(&headers),
        headers
            .get("x-aimanity-device")
            .and_then(|v| v.to_str().ok()),
    ) else {
        return rejected();
    };
    if kind == "desktop"
        && headers
            .get("x-aimanity-origin")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<Uuid>().ok())
            == Some(app.run_signals.peers.1)
    {
        return error(
            StatusCode::FORBIDDEN,
            "local_desktop",
            "请使用本机协作授权访问当前工作区的桌面",
        );
    }
    if headers.contains_key("origin") {
        return rejected();
    }
    let (tx, rx) = mpsc::channel(1);
    let stop = CancellationToken::new();
    let key = Uuid::new_v4();
    let id = {
        let mut entries = app.run_signals.peers.0.lock().unwrap();
        let Some((id, session)) = entries.iter_mut().find(|(_, s)| s.accepts(code, kind)) else {
            return rejected();
        };
        *session = Session::Connected {
            key,
            kind: kind.into(),
            tx,
            stop: stop.clone(),
        };
        id.clone()
    };
    let registered = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM devices WHERE id=$1 AND paired AND runtime_state='registered')").bind(&id).fetch_one(&app.db).await;
    if !matches!(registered, Ok(true)) {
        app.run_signals.peers.finish(&id, key);
        return rejected();
    }
    let failed = app.clone();
    let reserved = id.clone();
    ws.max_message_size(5 * 1024 * 1024)
        .on_failed_upgrade(move |_| failed.run_signals.peers.finish(&reserved, key))
        .on_upgrade(move |socket| async move {
            host_desktop::exchange(socket, rx, &stop).await;
            app.run_signals.peers.finish(&id, key);
        })
}
pub async fn manual(
    State(app): State<Shared>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let mut action = match decode::<wire::DesktopActionRequest>(&app, &headers, body) {
        Ok(action) => action,
        Err(e) => return e.into(),
    };
    action.epoch = 2;
    action.control_session.clear();
    match app.run_signals.peers.perform(action).await {
        Ok((_, _, reply)) => pb(StatusCode::OK, reply),
        Err(e) => error(StatusCode::CONFLICT, "peer_action", &e),
    }
}
pub async fn tool(
    app: &App,
    run: Uuid,
    call: &Value,
    args: &Value,
    frame: Option<&Value>,
) -> Result<Value, String> {
    let id = args["device_id"].as_str().ok_or("缺少设备编号")?;
    crate::desktop::authorized(app, run, id).await?;
    let mut action = host_desktop::action_request(call, args)?;
    if action.kind == "foreground" {
        return Err("请用 device_list 查看前台应用".into());
    }
    if action.kind != "observe" {
        let frame = frame.ok_or("请先用 device_interact 观察这台设备")?;
        let observation = frame["observation_id"]
            .as_str()
            .ok_or("这次观察没有画面编号")?;
        if !action.observation_id.is_empty() && action.observation_id != observation {
            return Err("观察已经更新，请使用这台设备的最新画面".into());
        }
        action.observation_id = observation.into();
        if action.kind == "scroll" && args["x"].is_null() && args["y"].is_null() {
            let center = |field: &str| {
                frame[field]
                    .as_u64()
                    .and_then(|v| i32::try_from(v / 2).ok())
                    .ok_or("设备画面尺寸缺失")
            };
            action.x = center("width")?;
            action.y = center("height")?;
        }
    }
    action.device_id = id.into();
    action.epoch = 1;
    action.control_session = run.to_string();
    let (session, device_kind, reply) = app.run_signals.peers.perform(action).await?;
    let value = json!({"success":true,"peer_session":session,"device_id":id,"device_kind":device_kind,"action_id":reply.action_id,"observation_id":reply.observation_id,
        "kind":args["kind"],"width":reply.width,"height":reply.height});
    let mut output = vec![json!({"type":"input_text","text":value.to_string()})];
    if !reply.jpeg.is_empty() {
        output.push(json!({"type":"input_image","image_url":format!("data:image/jpeg;base64,{}",STANDARD.encode(reply.jpeg))}));
    }
    Ok(Value::Array(output))
}
