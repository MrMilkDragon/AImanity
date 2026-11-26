use super::{records::revision, *};
use ai_phone_api::location_motion as motion;
use futures_util::{StreamExt, stream};
use std::time::Duration;

impl Workspace {
    pub(super) async fn set_location(
        &self,
        q: wire::LocationUpdate,
    ) -> Result<wire::LocationState> {
        let mut sessions = self.location.lock().await;
        let mut s: wire::LocationState = self.get("location")?;
        if !q.request_id.is_empty() && s.request_id == q.request_id {
            return Ok(s);
        }
        revision(s.revision, q.expected_revision)?;
        if q.enabled {
            if !(0.5..=500.).contains(&q.speed_kph)
                || q.route.len() > 200
                || q.route.iter().any(|p| !motion::valid(p))
                || (q.moving && (q.route.len() < 2 || motion::length(&q.route) < 1.))
            {
                return Err("路线或速度无效".into());
            }
            let point = if q.use_host {
                let response = self
                    .client("")?
                    .get("https://ipwho.is/")
                    .timeout(Duration::from_secs(8))
                    .send()
                    .await?
                    .error_for_status()?;
                let raw: serde_json::Value = serde_json::from_slice(&response.bytes().await?)?;
                let p = wire::GeoPoint {
                    latitude: raw["latitude"].as_f64().ok_or("请在地图选点")?,
                    longitude: raw["longitude"].as_f64().ok_or("请在地图选点")?,
                    accuracy_m: 50000.,
                };
                s.host = Some(p);
                p
            } else {
                q.point.ok_or("请在地图选点")?
            };
            if !motion::valid(&point) {
                return Err("定位坐标无效".into());
            }
            if s.route != q.route
                || (!s.moving && q.moving && s.distance_m >= motion::length(&q.route))
            {
                s.distance_m = 0.;
            }
            s.point = if q.moving {
                motion::at(&q.route, s.distance_m, q.repeat).map(|v| v.0)
            } else {
                Some(point)
            };
            s.source = if q.use_host { "network" } else { "manual" }.into();
        }
        s.revision += 1;
        s.request_id = q.request_id;
        s.enabled = q.enabled;
        s.moving = q.enabled && q.moving;
        s.speed_kph = q.speed_kph;
        s.route = q.route;
        s.repeat = q.repeat;
        s.updated_at_ms = crate::connection::epoch_ms() as u64;
        s.devices = self
            .registry()?
            .devices
            .into_iter()
            .map(|device| wire::LocationDevice {
                device_id: device.id,
                kind: device.kind,
                status: "pending".into(),
                ..Default::default()
            })
            .collect();
        self.store_location(&s)?;
        self.location_changed.notify_one();
        sessions.clear();
        Ok(s)
    }
    fn store_location(&self, s: &wire::LocationState) -> Result<()> {
        let tx = self.db.begin_write()?;
        put(&tx, "location", s)?;
        tx.commit()?;
        Ok(())
    }
    async fn deliver_location(
        &self,
        mut state: wire::LocationState,
        sessions: &mut std::collections::HashMap<String, CancellationToken>,
        send_all: bool,
    ) -> Result<wire::LocationState> {
        let fix = wire::LocationFix {
            enabled: state.enabled,
            point: state.point,
            speed_mps: if state.moving {
                state.speed_kph / 3.6
            } else {
                0.
            },
        };
        let devices = self.registry()?.devices;
        let mut old = std::mem::take(sessions);
        let mut last: std::collections::HashMap<_, _> = std::mem::take(&mut state.devices)
            .into_iter()
            .map(|d| (d.device_id.clone(), d))
            .collect();
        let delivery = devices
            .into_iter()
            .enumerate()
            .map(|(i, device)| {
                let session = self.peers.active_session(&device.id).ok();
                let already_sent = session.is_some() && old.remove(&device.id) == session;
                if let Some(token) = &session {
                    sessions.insert(device.id.clone(), token.clone());
                }
                let previous = last
                    .remove(&device.id)
                    .filter(|_| !send_all && already_sent);
                (i, device, session, previous)
            })
            .collect::<Vec<_>>();
        let mut receipts = stream::iter(delivery)
            .map(|(i, device, session, previous)| async move {
                if let Some(receipt) = previous {
                    return (i, receipt);
                }
                let mut receipt = wire::LocationDevice {
                    device_id: device.id.clone(),
                    kind: device.kind.clone(),
                    status: "pending".into(),
                    ..Default::default()
                };
                let result = if let Some(session) = session {
                    let req = wire::DesktopActionRequest {
                        device_id: device.id.clone(),
                        action_id: uuid::Uuid::now_v7().to_string(),
                        kind: "location".into(),
                        location: Some(fix),
                        ..Default::default()
                    };
                    tokio::time::timeout(
                        Duration::from_secs(2),
                        self.peers.perform(req, Some(&session)),
                    )
                    .await
                    .map_err(|e| e.into())
                    .and_then(|result| result.map(Some))
                } else if !device.paired
                    && !matches!(
                        device.runtime_state.as_str(),
                        "stopped" | "stopping" | "deleting" | "failed" | "starting"
                    )
                {
                    self.owned_location(&device, &fix).await
                } else {
                    Ok(None)
                };
                {
                    match result {
                        Ok(None) => {}
                        Ok(Some(reply))
                            if reply.location == Some(fix)
                                && matches!(
                                    reply.location_scope.as_str(),
                                    "system" | "browser" | "workspace"
                                ) =>
                        {
                            receipt.status = if !fix.enabled {
                                "disabled"
                            } else {
                                match reply.location_scope.as_str() {
                                    "system" => "applied",
                                    "browser" => "browser",
                                    _ => "workspace",
                                }
                            }
                            .into();
                            receipt.applied_at_ms = crate::connection::epoch_ms() as u64;
                        }
                        Ok(_) => {
                            receipt.status = "unavailable".into();
                            receipt.error = "设备未确认定位".into();
                        }
                        Err(e) => {
                            receipt.status = "unavailable".into();
                            receipt.error = e.to_string();
                        }
                    }
                }
                (i, receipt)
            })
            .buffer_unordered(8)
            .collect::<Vec<_>>()
            .await;
        receipts.sort_by_key(|i| i.0);
        state.devices = receipts.into_iter().map(|v| v.1).collect();
        self.store_location(&state)?;
        self.changed();
        Ok(state)
    }
    async fn owned_location(
        &self,
        device: &wire::RegisteredDevice,
        fix: &wire::LocationFix,
    ) -> Result<Option<wire::HostDesktopReply>> {
        let home = self.device_home(&device.id);
        let scope = if device.kind == "phone" {
            let Some(client) = ai_phone_host_runtime::android::session::attached_client(&home)?
            else {
                return Ok(None);
            };
            ai_phone_host_runtime::android::location::apply(&client, fix).await?;
            "system"
        } else {
            #[cfg(any(
                target_os = "macos",
                windows,
                all(target_os = "linux", target_arch = "x86_64")
            ))]
            {
                let Some(ack) = ai_phone_host_runtime::desktop_location(&home, fix).await? else {
                    return Ok(None);
                };
                if !ack.error.is_empty() {
                    return Err(ack.error.into());
                }
                if ack.status != if fix.enabled { "applied" } else { "disabled" } {
                    return Err("设备未确认定位".into());
                }
                "browser"
            }
            #[cfg(not(any(
                target_os = "macos",
                windows,
                all(target_os = "linux", target_arch = "x86_64")
            )))]
            return Ok(None);
        };
        Ok(Some(wire::HostDesktopReply {
            location: Some(*fix),
            location_scope: scope.into(),
            ..Default::default()
        }))
    }
    pub(super) async fn watch_location(
        weak: std::sync::Weak<Self>,
        stop: CancellationToken,
        wake: Arc<Notify>,
        mut devices: watch::Receiver<u64>,
    ) {
        loop {
            let moving = weak
                .upgrade()
                .and_then(|w| w.get::<wire::LocationState>("location").ok())
                .is_some_and(|s| s.moving);
            let tick = tokio::select! {
                _=stop.cancelled()=>break,
                _=wake.notified()=>false,
                result=devices.changed()=>{if result.is_err(){break;}false},
                _=tokio::time::sleep(Duration::from_secs(1)),if moving=>true,
            };
            let Some(w) = weak.upgrade() else {
                break;
            };
            let mut sessions = w.location.lock().await;
            let Ok(mut state) = w.get::<wire::LocationState>("location") else {
                continue;
            };
            if state.revision == 0 {
                continue;
            }
            if tick && state.moving {
                let now = crate::connection::epoch_ms() as u64;
                state.distance_m +=
                    state.speed_kph / 3.6 * now.saturating_sub(state.updated_at_ms) as f64 / 1000.;
                if let Some((p, end)) = motion::at(&state.route, state.distance_m, state.repeat) {
                    state.point = Some(p);
                    state.moving = !end;
                }
                state.updated_at_ms = now;
            }
            if let Err(e) = w.deliver_location(state, &mut sessions, tick).await {
                eprintln!("Workspace location: {e}");
            }
        }
    }
}
