use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Default, Deserialize)]
pub(super) struct DesktopInput {
    pub(super) kind: String,
    #[serde(default)]
    pub(super) device_id: String,
    #[serde(default)]
    observation_id: String,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    #[serde(default)]
    to_x: i32,
    #[serde(default)]
    to_y: i32,
    #[serde(default)]
    amount: i32,
    #[serde(default)]
    text: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    strokes: Vec<Vec<[i32; 2]>>,
}

impl DesktopInput {
    pub(super) fn observe(device: &str) -> Self {
        Self {
            device_id: device.into(),
            kind: "observe".into(),
            ..Default::default()
        }
    }
    pub(super) fn request(self, id: &str, run: &str) -> wire::DesktopActionRequest {
        wire::DesktopActionRequest {
            action_id: id.into(),
            control_session: run.into(),
            epoch: 1,
            device_id: self.device_id,
            kind: self.kind,
            observation_id: self.observation_id,
            x: self.x,
            y: self.y,
            to_x: self.to_x,
            to_y: self.to_y,
            amount: self.amount,
            text: self.text,
            key: self.key,
            strokes: self
                .strokes
                .into_iter()
                .map(|path| wire::BrushStroke {
                    points: path
                        .into_iter()
                        .map(|[x, y]| wire::ScreenPoint { x, y })
                        .collect(),
                })
                .collect(),
            ..Default::default()
        }
    }
}

pub(super) fn add_desktop_actions(tool: &mut Value) {
    let props = &mut tool["parameters"]["properties"];
    props["kind"]["enum"].as_array_mut().unwrap().extend([
        json!("right_click"),
        json!("drag"),
        json!("stroke"),
    ]);
    props["to_x"] = json!({"type":"integer"});
    props["to_y"] = json!({"type":"integer"});
    props["strokes"] = json!({"type":"array","description":"On a computer, draw each path with the active brush held down, in observed screenshot pixels. Lift between paths.","minItems":1,"maxItems":32,"items":{"type":"array","minItems":2,"maxItems":128,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"integer"}}}});
}

pub(super) fn device_info(device: &wire::RegisteredDevice) -> Value {
    json!({"id":device.id,"name":device.name,"kind":device.kind,"system":device.system_name(),"source":if device.paired {"remote"}else{"virtual"},"connected":crate::connection::reachable(device)})
}

pub(super) fn device_output(info: &Value, reply: wire::HostDesktopReply) -> Value {
    let mut info = info.clone();
    if let Some(point) = reply.location.and_then(|fix| fix.point) {
        info["location"] = json!({"scope":reply.location_scope,"latitude":point.latitude,"longitude":point.longitude});
    }
    if reply.jpeg.is_empty() {
        return Value::String(info.to_string());
    }
    json!([{"type":"input_text","text":info.to_string()},{"type":"input_image","image_url":format!("data:image/jpeg;base64,{}",STANDARD.encode(reply.jpeg))}])
}

#[derive(Default)]
pub(super) struct DeviceRun {
    pub(super) devices: Vec<wire::RegisteredDevice>,
    sessions: HashMap<String, CancellationToken>,
    observations: HashMap<String, (String, u32, u32)>,
    pub(super) images: HashMap<String, usize>,
    pub(super) uncertain: Vec<String>,
}

impl DeviceRun {
    pub(super) fn for_devices(
        peers: &super::peers::Peers,
        devices: impl Iterator<Item = wire::RegisteredDevice>,
    ) -> Self {
        let (sessions, devices) = devices
            .filter_map(|device| {
                let stop = peers.active_session(&device.id).ok()?;
                Some(((device.id.clone(), stop), device))
            })
            .unzip();
        Self {
            devices,
            sessions,
            ..Default::default()
        }
    }
    pub(super) fn session(&self, id: &str) -> Result<CancellationToken> {
        self.sessions
            .get(id)
            .cloned()
            .ok_or_else(|| "这台设备不在当前任务的授权范围".into())
    }
    pub(super) fn tool(&self) -> Value {
        let ids: Vec<_> = self.devices.iter().map(|d| &d.id).collect();
        let roster: Vec<_> = self.devices.iter().map(device_info).collect();
        let mut tool = json!({"type":"function","name":"device_interact","description":format!("Operate a connected device. For system navigation or a named key use kind=key and the key parameter. For a visible control observe, then use screenshot pixels. Devices: {}",json!(roster)),
            "parameters":{"type":"object","properties":{"device_id":{"type":"string","enum":ids},"kind":{"type":"string","enum":["foreground","observe","key","click","double_click","scroll","type"]},"key":{"type":"string","enum":["home","back","enter","tab","escape","backspace","left","right","up","down","save","select_all","undo","redo"],"description":"Phone navigation includes home and back. Desktop shortcuts include save, select_all, undo, redo."},"x":{"type":"integer"},"y":{"type":"integer"},"to_x":{"type":"integer"},"to_y":{"type":"integer"},"amount":{"type":"integer"},"text":{"type":"string"}},"required":["device_id","kind"]}});
        if self.devices.iter().any(|d| d.kind == "phone") {
            tool["parameters"]["properties"]["kind"]["enum"]
                .as_array_mut()
                .unwrap()
                .extend([json!("long_press"), json!("swipe")]);
        }
        if self.devices.iter().any(|d| d.kind == "desktop") {
            add_desktop_actions(&mut tool);
        }
        tool
    }
    pub(super) async fn perform(
        &mut self,
        workspace: &Workspace,
        n: u64,
        run: &str,
        data: DesktopInput,
        event: &mut wire::HarnessEvent,
        events: &mut Vec<wire::HarnessEvent>,
    ) -> Result<Value> {
        let id = data.device_id.clone();
        let session = self.session(&id)?;
        workspace.claim_device(run, &id, &data.kind)?;
        if session.is_cancelled() {
            return Err("设备已断开".into());
        }
        let mut action = data.request(&event.id, run);
        let input = !matches!(action.kind.as_str(), "observe" | "foreground");
        let mut info = json!({"device_id":id,"kind":action.kind,"action_id":event.id});
        if input {
            if self.uncertain.contains(&id) {
                return Err(
                    "上次操作中断时未取得设备回执，请先核对当前画面，再由用户确认是否重试".into(),
                );
            }
            if let Some((view, width, height)) = self.observations.get(&id) {
                let inside = |x: i32, y: i32| {
                    (0..i64::from(*width)).contains(&i64::from(x))
                        && (0..i64::from(*height)).contains(&i64::from(y))
                };
                if (matches!(
                    action.kind.as_str(),
                    "click"
                        | "double_click"
                        | "right_click"
                        | "long_press"
                        | "drag"
                        | "swipe"
                        | "scroll"
                ) && !inside(action.x, action.y))
                    || (matches!(action.kind.as_str(), "drag" | "swipe")
                        && !inside(action.to_x, action.to_y))
                {
                    info["executed"] = json!(false);
                    info["reason"] = json!(format!(
                        "Coordinates outside the {width}×{height} observation. Use pixel coordinates or a named key for system navigation."
                    ));
                    event.status = "not_executed".into();
                    event.content = info.to_string();
                    return Ok(Value::String(event.content.clone()));
                }
                action.observation_id = view.clone();
                self.observations.remove(&id);
                if action.kind == "key" {
                    info["key"] = json!(action.key);
                }
                event.status = "dispatched".into();
                event.kind = "device_input".into();
                event.content = info.to_string();
                events.push(event.clone());
                workspace.record_tools(n, run, std::mem::take(events))?;
            } else {
                info["requested_kind"] = json!(action.kind);
                info["input_executed"] = json!(false);
                action.kind = "observe".into();
                info["kind"] = json!("observe");
            }
        }
        let reply = workspace.peers.perform(action, Some(&session)).await?;
        if !reply.observation_id.is_empty() {
            info["observation_id"] = json!(reply.observation_id);
            info["width"] = json!(reply.width);
            info["height"] = json!(reply.height);
            if !reply.jpeg.is_empty() {
                self.observations.insert(
                    id,
                    (reply.observation_id.clone(), reply.width, reply.height),
                );
            }
        }
        if !reply.foreground.is_empty() {
            info["foreground"] = json!(reply.foreground);
        }
        event.content = info.to_string();
        Ok(device_output(&info, reply))
    }
}

pub(super) fn retire_image(index: Option<usize>, input: &mut [Value]) {
    if let Some(index) = index {
        let item = &mut input[index];
        let key = if item["type"] == "function_call_output" {
            "output"
        } else {
            "content"
        };
        if let Some(parts) = item[key].as_array_mut()
            && parts.last().is_some_and(|p| p["type"] == "input_image")
        {
            parts.pop();
        }
    }
}
