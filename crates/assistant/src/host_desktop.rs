use crate::*;
use axum::extract::{
    Path, State,
    ws::{Message as Frame, WebSocket, WebSocketUpgrade},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::sync::Mutex;
use tokio::sync::{mpsc, oneshot};

pub(crate) type Call = (
    wire::DesktopActionRequest,
    oneshot::Sender<wire::HostDesktopReply>,
);

struct Grant {
    key: Uuid,
    request: Uuid,
    run: Option<Uuid>,
    calls: mpsc::Sender<Call>,
}

#[derive(Default)]
pub(crate) struct DesktopSession(Mutex<Option<Grant>>);

pub fn receipt_in_session(output: &Value, session: Option<&str>) -> bool {
    session.is_some_and(|id| {
        crate::worker::tool_value(output).is_some_and(|v| v["host_session"] == id)
    })
}

impl DesktopSession {
    pub fn bind(&self, request: Uuid, run: Uuid, requested: bool) {
        let mut active = self.0.lock().unwrap();
        if let Some(grant) = active.as_mut() {
            if requested && grant.request == request {
                grant.run = Some(run);
            } else if grant.run == Some(run) || grant.request == request {
                active.take();
            }
        }
    }

    pub fn revoke(&self, run: Uuid) {
        self.0.lock().unwrap().take_if(|g| g.run == Some(run));
    }

    fn sender(&self, run: Uuid) -> Option<(Uuid, mpsc::Sender<Call>)> {
        self.0
            .lock()
            .unwrap()
            .as_ref()
            .filter(|g| g.run == Some(run))
            .map(|g| (g.key, g.calls.clone()))
    }

    pub fn session(&self, run: Uuid) -> Option<String> {
        self.0
            .lock()
            .unwrap()
            .as_ref()
            .filter(|g| g.run == Some(run))
            .map(|g| g.key.to_string())
    }

    pub async fn perform(&self, run: Uuid, call: &Value, args: &Value) -> Result<Value, String> {
        let (session, sender) = self.sender(run).ok_or("本机桌面尚未为本次任务授权")?;
        let (send, reply) = oneshot::channel();
        let kind = args["kind"].as_str().ok_or("缺少桌面操作类型")?;
        let action = action_request(call, args)?;
        let id = action.action_id.clone();
        sender
            .send((action, send))
            .await
            .map_err(|_| "本机桌面授权已结束")?;
        drop(sender);
        let receipt = reply.await.map_err(|_| "本机连接已关闭，操作结果未知")?;
        if !self
            .0
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|g| g.key == session && g.run == Some(run))
        {
            return Err("本机授权已结束".into());
        }
        if receipt.action_id != id {
            return Err("本机操作回执不匹配".into());
        }
        if !receipt.error.is_empty() {
            return Err(receipt.error);
        }
        let value = json!({"success":true,"host_session":session,"device_id":format!("host-display-{}",receipt.display),"action_id":id,"kind":kind,
            "display":receipt.display,"displays":receipt.displays,"observation_id":receipt.observation_id,
            "width":receipt.width,"height":receipt.height});
        let mut output = vec![json!({"type":"input_text","text":value.to_string()})];
        if !receipt.jpeg.is_empty() {
            output.push(json!({"type":"input_image","image_url":format!("data:image/jpeg;base64,{}", STANDARD.encode(receipt.jpeg))}));
        }
        Ok(Value::Array(output))
    }
}

pub(crate) fn action_request(
    call: &Value,
    args: &Value,
) -> Result<wire::DesktopActionRequest, String> {
    let pixel = |v: &Value| match v.as_i64() {
        Some(n) => i32::try_from(n).map_err(|_| "无效的坐标或滚动距离"),
        None if v.is_null() => Ok(-1),
        _ => Err("无效的坐标或滚动距离"),
    };
    let kind = args["kind"].as_str().ok_or("缺少桌面操作类型")?;
    Ok(wire::DesktopActionRequest {
        action_id: call["local_action_id"]
            .as_str()
            .ok_or("缺少操作编号")?
            .into(),
        device_id: args["display"].as_str().unwrap_or_default().into(),
        observation_id: args["observation_id"].as_str().unwrap_or_default().into(),
        kind: kind.into(),
        x: pixel(&args["x"])?,
        y: pixel(&args["y"])?,
        to_x: pixel(&args["to_x"])?,
        to_y: pixel(&args["to_y"])?,
        amount: pixel(&args["amount"])?,
        text: args["text"].as_str().unwrap_or_default().into(),
        key: args["key"].as_str().unwrap_or_default().into(),
        ..Default::default()
    })
}

pub async fn connect(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(request): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> Response {
    if let Err(e) = auth(&app, &headers) {
        return e.into();
    }
    if headers.contains_key("origin") {
        return error(StatusCode::FORBIDDEN, "native_only", "请在本机客户端授权");
    }
    ws.max_message_size(5 * 1024 * 1024)
        .on_upgrade(move |socket| serve(app, request, socket))
}

async fn serve(app: Shared, request: Uuid, socket: WebSocket) {
    let key = Uuid::now_v7();
    let (tx, calls) = mpsc::channel(1);
    {
        let mut active = app.run_signals.host.0.lock().unwrap();
        if active.is_some() {
            return;
        }
        *active = Some(Grant {
            key,
            request,
            run: None,
            calls: tx,
        });
    }
    exchange(socket, calls, &CancellationToken::new()).await;
    app.run_signals
        .host
        .0
        .lock()
        .unwrap()
        .take_if(|g| g.key == key);
}

pub(crate) async fn exchange(
    mut socket: WebSocket,
    mut calls: mpsc::Receiver<Call>,
    stop: &CancellationToken,
) {
    if socket
        .send(Frame::Binary(
            wire::HostDesktopReply::default().encode_to_vec().into(),
        ))
        .await
        .is_ok()
    {
        loop {
            let call = tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                _ = socket.recv() => break,
                item = calls.recv() => item,
            };
            let Some((action, answer)) = call else {
                let _ = socket.send(Frame::Close(None)).await;
                break;
            };
            if answer.is_closed() {
                continue;
            }
            if socket
                .send(Frame::Binary(action.encode_to_vec().into()))
                .await
                .is_err()
            {
                break;
            }
            let response = tokio::select! { biased; _ = stop.cancelled() => break, reply = tokio::time::timeout(Duration::from_secs(20), socket.recv()) => reply };
            let Ok(Some(Ok(Frame::Binary(bytes)))) = response else {
                break;
            };
            let Ok(receipt) = wire::HostDesktopReply::decode(bytes) else {
                break;
            };
            let _ = answer.send(receipt);
        }
    }
}

pub(crate) fn schema(paired: bool) -> Value {
    let mut kinds = vec!["observe", "click", "double_click", "type", "key", "scroll"];
    let mut keys = vec![
        "enter",
        "tab",
        "escape",
        "backspace",
        "left",
        "right",
        "up",
        "down",
        "select_all",
        "save",
        "undo",
        "redo",
    ];
    let mut props = json!({
        "observation_id":{"type":"string"}, "x":{"type":"integer"},"y":{"type":"integer"},
        "amount":{"type":"integer","description":"滚动行数，正值向下"},"text":{"type":"string"}
    });
    let mut required = vec!["kind"];
    let (name, description) = if paired {
        kinds.extend(["long_press", "swipe"]);
        keys.extend(["back", "home"]);
        required.push("device_id");
        props["device_id"] = json!({"type":"string"});
        props["to_x"] = json!({"type":"integer"});
        props["to_y"] = json!({"type":"integer"});
        (
            "device_interact",
            "读取画面时只填 device_id 和 kind=observe。点击、输入、滚动和按键使用这台设备刚返回的画面。操作后再次 observe 核对结果。",
        )
    } else {
        props["intent"] = json!({"type":"string"});
        required.push("intent");
        props["display"] = json!({"type":"string"});
        (
            "host_desktop",
            "本次获准使用的用户本机桌面。observe 得到屏幕、显示器列表、图像坐标和 observation_id。点击、双击、输入、按键和滚动使用刚取得的本机观察。文件交接继续用工作文件。操作后观察并核对应用内实际结果。",
        )
    };
    props["key"] = json!({"type":"string","enum":keys});
    props["kind"] = json!({"type":"string","enum":kinds});
    json!({"type":"function","name":name,"description":description,"parameters":{
        "type":"object","properties":props,"required":required,"additionalProperties":false
    }})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_follows_only_the_accepted_request_until_revocation() {
        let key = Uuid::now_v7();
        let request = Uuid::now_v7();
        let task = Uuid::now_v7();
        let background = Uuid::now_v7();
        let (calls, mut receiver) = mpsc::channel(1);
        let relay = DesktopSession(Mutex::new(Some(Grant {
            key,
            request,
            run: None,
            calls,
        })));
        relay.bind(Uuid::now_v7(), background, true);
        assert!(relay.sender(background).is_none());
        relay.bind(request, task, true);
        relay.revoke(background);
        assert_eq!(relay.session(task), Some(key.to_string()));
        relay.bind(Uuid::now_v7(), task, false);
        assert!(relay.sender(task).is_none());
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
    }
}
