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
            ..Default::default()
        }
    }
}

pub(super) fn device_info(device: &wire::RegisteredDevice) -> Value {
    json!({"id":device.id,"name":device.name,"kind":device.kind,"connected":device.connected})
}

pub(super) fn device_output(info: &Value, reply: wire::HostDesktopReply) -> Value {
    if reply.jpeg.is_empty() {
        return Value::String(info.to_string());
    }
    json!([{"type":"input_text","text":info.to_string()},{"type":"input_image","image_url":format!("data:image/jpeg;base64,{}",STANDARD.encode(reply.jpeg))}])
}

#[derive(Default)]
pub(super) struct DeviceRun {
    pub(super) devices: Vec<wire::RegisteredDevice>,
    observations: HashMap<String, (String, u32, u32)>,
    pub(super) images: HashMap<String, usize>,
    pub(super) uncertain: Vec<String>,
}

impl DeviceRun {
    pub(super) fn for_devices(devices: Vec<wire::RegisteredDevice>) -> Self {
        Self {
            devices,
            ..Default::default()
        }
    }
    pub(super) fn tool(&self) -> Value {
        let ids: Vec<_> = self.devices.iter().map(|d| &d.id).collect();
        let roster: Vec<_> = self.devices.iter().map(device_info).collect();
        json!({"type":"function","name":"device_interact","description":format!("Operate a connected device. For system navigation or a named key use kind=key and the key parameter. For a visible control observe, then use screenshot pixels. Devices: {}",json!(roster)),
            "parameters":{"type":"object","properties":{"device_id":{"type":"string","enum":ids},"kind":{"type":"string","enum":["foreground","observe","key","click","double_click","scroll","long_press","swipe","type"]},"key":{"type":"string","enum":["home","back","enter","tab","escape","backspace","left","right","up","down","save","select_all","undo","redo"],"description":"Phone navigation includes home and back. Desktop shortcuts include save, select_all, undo, redo."},"x":{"type":"integer"},"y":{"type":"integer"},"to_x":{"type":"integer"},"to_y":{"type":"integer"},"amount":{"type":"integer"},"text":{"type":"string"}},"required":["device_id","kind"]}})
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
        if !self.devices.iter().any(|d| d.id == id) {
            return Err("这台设备不在当前任务的授权范围".into());
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
                    "click" | "double_click" | "long_press" | "swipe" | "scroll"
                ) && !inside(action.x, action.y))
                    || (action.kind == "swipe" && !inside(action.to_x, action.to_y))
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
        let reply = workspace.peers.perform(action).await?;
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

    pub(super) fn retire_image(&mut self, id: &str, input: &mut [Value]) {
        if let Some(index) = self.images.remove(id) {
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
}
