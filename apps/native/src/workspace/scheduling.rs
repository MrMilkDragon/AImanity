use super::{conversation::Job, *};
use redb::ReadableTableMetadata;
use std::collections::BTreeSet;

pub(super) struct Active {
    pub token: CancellationToken,
    pub child: bool,
    pub devices: BTreeSet<String>,
    pub detail: wire::RunningAgent,
}
type Work = (u64, Job, String, CancellationToken, bool);

impl Job {
    fn priority(&self) -> u8 {
        match self.source.as_str() {
            "copilot" => 0,
            "user" | "goal" | "" => 1,
            "liberty" => 3,
            _ => 2,
        }
    }
}

impl Workspace {
    pub(super) fn receive_notice(
        &self,
        device: &str,
        notice: wire::PhoneNotification,
    ) -> Result<()> {
        if notice.event_id.is_empty() {
            return Err("通知缺少事件编号".into());
        }
        let id = format!("notice/{}/{}/{}", device.len(), device, notice.event_id);
        let tx = self.db.begin_write()?;
        if tx.open_table(IDS)?.get(id.as_str())?.is_some() {
            return Ok(());
        }
        let mut table = tx.open_table(INBOX)?;
        if table.get(id.as_str())?.is_some() {
            return Ok(());
        }
        let q = wire::ChatSendRequest {
            request_id: id.clone(),
            device_id: device.into(),
            text: format!("{} · {}\n{}", notice.app_name, notice.title, notice.text),
            ..Default::default()
        };
        table.insert(id.as_str(), q.encode_to_vec().as_slice())?;
        drop(table);
        tx.commit()?;
        self.ready.notify_one();
        Ok(())
    }
    pub(super) fn flush_notices(&self) -> Result<()> {
        let tx = self.db.begin_read()?;
        let inbox = tx.open_table(INBOX)?;
        if inbox.is_empty()? {
            return Ok(());
        }
        if !self.proactive_state()?.notifications_enabled {
            return Ok(());
        }
        let registry = self.registry()?;
        if registry.active_agent_id.is_empty() {
            return Ok(());
        }
        let pending = inbox
            .iter()?
            .map(|r| Ok(wire::ChatSendRequest::decode(r?.1.value())?))
            .collect::<Result<Vec<_>>>()?;
        drop(inbox);
        drop(tx);
        for q in pending {
            if !registry
                .devices
                .iter()
                .any(|d| d.connected && d.id == q.device_id)
            {
                continue;
            }
            self.submit_job(q, "notification", "")?;
        }
        Ok(())
    }
    pub(super) fn queued(&self) -> Result<Vec<(u64, Job)>> {
        let tx = self.db.begin_read()?;
        let mut jobs = tx
            .open_table(JOBS)?
            .iter()?
            .map(|row| {
                let (n, bytes) = row?;
                Ok((n.value(), Job::decode(bytes.value())?))
            })
            .collect::<Result<Vec<_>>>()?;
        let order: wire::TodoList = read(&tx.open_table(RECORDS)?, "todo-order")?;
        let positions: std::collections::HashMap<_, _> = order
            .items
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.as_str(), i))
            .collect();
        jobs.sort_by_key(|(n, j)| {
            let placed = j
                .request
                .as_ref()
                .and_then(|q| positions.get(q.request_id.as_str()).copied());
            (
                j.priority() != 0,
                placed.is_none(),
                placed.unwrap_or(0),
                j.priority(),
                *n,
            )
        });
        Ok(jobs)
    }
    pub(super) fn claim_jobs(&self) -> Result<Vec<Work>> {
        let jobs = self.queued()?;
        if jobs.is_empty() {
            return Ok(vec![]);
        }
        let registry = self.registry()?;
        let mut running = self.run.lock().unwrap();
        let mut selected = Vec::new();
        for (n, job) in jobs {
            let Some(q) = job.request.as_ref() else {
                continue;
            };
            if job.paused || running.contains_key(&q.request_id) || running.len() >= 4 {
                continue;
            }
            let main_busy = running.values().any(|a| !a.child);
            let child = !job.parent.is_empty() || main_busy;
            if child && q.device_id.is_empty() && job.parent.is_empty() {
                continue;
            }
            if !q.device_id.is_empty() && running.values().any(|a| a.devices.contains(&q.device_id))
            {
                continue;
            }
            let token = running
                .get(&job.parent)
                .map_or_else(|| self.shutdown.child_token(), |a| a.token.child_token());
            let id = q.request_id.clone();
            let device = registry
                .devices
                .iter()
                .find(|d| d.id == q.device_id)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            let detail = wire::RunningAgent {
                run_id: id.clone(),
                title: q
                    .text
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(120)
                    .collect(),
                source: job.source.clone(),
                device,
                step: "执行中".into(),
                started_at_ms: crate::connection::epoch_ms() as u64,
            };
            let devices = (!q.device_id.is_empty())
                .then(|| q.device_id.clone())
                .into_iter()
                .collect();
            running.insert(
                id.clone(),
                Active {
                    token: token.clone(),
                    child,
                    devices,
                    detail,
                },
            );
            selected.push((n, job, id, token, child));
        }
        drop(running);
        if !selected.is_empty() {
            self.changed();
        }
        Ok(selected)
    }
    pub(super) fn claim_device(&self, run: &str, id: &str, step: &str) -> Result<()> {
        let mut active = self.run.lock().unwrap();
        if active
            .iter()
            .any(|(owner, a)| owner != run && a.devices.contains(id))
        {
            return Err("设备正在执行另一项任务".into());
        }
        if let Some(a) = active.get_mut(run) {
            a.devices.insert(id.to_owned());
            if a.detail.step != step && a.child {
                a.detail.step = step.into();
                self.changed();
            }
        }
        Ok(())
    }
    pub(super) async fn delegate(
        &self,
        parent: &str,
        call: &str,
        tasks: Vec<(String, String)>,
        token: &CancellationToken,
    ) -> Result<serde_json::Value> {
        if !(1..=8).contains(&tasks.len()) {
            return Err("每次可委派一至八项独立工作".into());
        }
        let mut seen = BTreeSet::new();
        let registry = self.registry()?;
        for (device, task) in &tasks {
            if task.trim().is_empty() {
                return Err("子任务缺少目标".into());
            }
            if !device.is_empty() {
                if !registry
                    .devices
                    .iter()
                    .any(|d| d.id == *device && d.paired && d.connected)
                {
                    return Err("设备未连接".into());
                }
                if !seen.insert(device)
                    || self
                        .run
                        .lock()
                        .unwrap()
                        .get(parent)
                        .is_some_and(|a| a.devices.contains(device))
                {
                    return Err("同一设备的操作需要顺序执行".into());
                }
            }
        }
        let mut changes = self.subscribe();
        let mut ids = Vec::new();
        for (index, (device_id, text)) in tasks.into_iter().enumerate() {
            let request_id = format!("{parent}/{call}/{index}");
            self.submit_job(
                wire::ChatSendRequest {
                    request_id: request_id.clone(),
                    device_id,
                    text,
                    ..Default::default()
                },
                "delegate",
                parent,
            )?;
            ids.push(request_id);
        }
        loop {
            let tx = self.db.begin_read()?;
            let index = tx.open_table(IDS)?;
            let messages = tx.open_table(MESSAGES)?;
            let mut results = Vec::new();
            for id in &ids {
                let n = index.get(id.as_str())?.ok_or("子任务不存在")?.value();
                let reply = wire::ChatMessage::decode(
                    messages.get(n + 1)?.ok_or("子任务缺少结果")?.value(),
                )?;
                if !reply.active {
                    results.push(serde_json::json!({"task":id,"result":reply.text,"error":reply.system_notice,"receipts":self.get::<wire::HarnessEventList>(&format!("events/{id}"))?.events.iter().filter(|e|e.kind=="device_input" && e.status=="succeeded").count()}));
                }
            }
            if results.len() == ids.len() {
                return Ok(serde_json::Value::Array(results));
            }
            drop(messages);
            drop(index);
            drop(tx);
            tokio::select! {_=token.cancelled()=>return Err("任务已停止".into()), r=changes.changed()=>r?}
        }
    }
    pub(super) fn todos(&self) -> Result<wire::TodoList> {
        let registry = self.registry()?;
        let jobs = self.queued()?;
        let running = self.run.lock().unwrap();
        Ok(wire::TodoList {
            items: jobs
                .into_iter()
                .filter_map(|(_, job)| {
                    let q = job.request?;
                    Some(wire::TodoItem {
                        id: q.request_id.clone(),
                        title: q.text,
                        source: job.source,
                        device: registry
                            .devices
                            .iter()
                            .find(|d| d.id == q.device_id)
                            .map_or_else(String::new, |d| d.name.clone()),
                        status: if job.paused {
                            "paused"
                        } else if running.contains_key(&q.request_id) {
                            "running"
                        } else {
                            "queued"
                        }
                        .into(),
                    })
                })
                .collect(),
        })
    }
    pub(super) fn move_todo(&self, q: wire::TodoMove) -> Result<wire::TodoList> {
        let mut list = self.todos()?;
        if q.id == q.before_id {
            return Ok(list);
        }
        let index = list
            .items
            .iter()
            .position(|item| item.id == q.id)
            .ok_or("待办不存在")?;
        let item = list.items.remove(index);
        let next = if q.before_id.is_empty() {
            list.items.len()
        } else {
            list.items
                .iter()
                .position(|i| i.id == q.before_id)
                .ok_or("待办不存在")?
        };
        list.items.insert(next, item);
        let tx = self.db.begin_write()?;
        let ids = wire::TodoList {
            items: list
                .items
                .iter()
                .map(|t| wire::TodoItem {
                    id: t.id.clone(),
                    ..Default::default()
                })
                .collect(),
        };
        put(&tx, "todo-order", &ids)?;
        tx.commit()?;
        self.ready.notify_one();
        self.todos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::post};
    use futures_util::{SinkExt, StreamExt};
    use std::{net::Ipv4Addr, time::Duration};
    use tokio::sync::{Semaphore, mpsc};
    use tokio_tungstenite::tungstenite::{Message as Frame, client::IntoClientRequest};

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn busy_primary_delegates_another_devices_notice_and_location_reaches_both() -> Result<()>
    {
        let root = tempfile::tempdir()?;
        let w = Workspace::open(root.path())?;
        let model = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = model.local_addr()?;
        let arrivals = Arc::new(Semaphore::new(0));
        let answers = Arc::new(Semaphore::new(0));
        let exercise_delegation = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler = {
            let exercise_delegation = exercise_delegation.clone();
            let hits = arrivals.clone();
            let release = answers.clone();
            move |bytes: axum::body::Bytes| {
                let drill = exercise_delegation.load(std::sync::atomic::Ordering::Relaxed);
                let hits = hits.clone();
                let release = release.clone();
                async move {
                    use serde_json::json;
                    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    let offers = |name| {
                        body["tools"]
                            .as_array()
                            .is_some_and(|ts| ts.iter().any(|t| t["function"]["name"] == name))
                    };
                    let from_tool = body["messages"]
                        .as_array()
                        .is_some_and(|ms| ms.iter().any(|m| m["role"] == "tool"));
                    let call = if !drill {
                        hits.add_permits(1);
                        release.acquire().await.unwrap().forget();
                        None
                    } else if !from_tool && offers("delegate_tasks") {
                        Some((
                            "delegate_tasks",
                            json!({"tasks":[{"device_id":"a","task":"On A"},{"device_id":"b","task":"On B"}]}),
                        ))
                    } else if !from_tool && offers("device_interact") {
                        let id = if body["messages"].to_string().contains("On A") {
                            "a"
                        } else {
                            "b"
                        };
                        Some((
                            "device_interact",
                            json!({"device_id":id,"kind":"key","key":"home"}),
                        ))
                    } else if from_tool && offers("complete_goal") {
                        Some(("complete_goal", json!({"outcome":"Both devices confirmed"})))
                    } else {
                        None
                    };
                    let (delta, reason) = match call {
                        Some((name, args)) => (
                            json!({"tool_calls":[{"index":0,"id":name,"function":{"name":name,"arguments":args.to_string()}}]}),
                            "tool_calls",
                        ),
                        None => (json!({"content":"received"}), "stop"),
                    };
                    let response =
                        json!({"choices":[{"index":0,"delta":delta,"finish_reason":reason}]});
                    (
                        [("content-type", "text/event-stream")],
                        format!("data: {response}\n\ndata: [DONE]\n\n"),
                    )
                }
            }
        };
        let server = tokio::spawn(async move {
            axum::serve(
                model,
                Router::new().route("/v1/chat/completions", post(handler)),
            )
            .await
        });
        let tx = w.db.begin_write()?;
        put(
            &tx,
            "registry",
            &wire::RegistryState {
                active_agent_id: "agent".into(),
                agents: vec![wire::RegisteredAgent {
                    id: "agent".into(),
                    model_connection_id: "model".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        )?;
        put(
            &tx,
            "model/model",
            &wire::ModelDraft {
                provider: "compatible".into(),
                protocol: "chat-completions".into(),
                base_url: format!("http://{address}/v1"),
                model: "isolated".into(),
                ..Default::default()
            },
        )?;
        tx.commit()?;
        let address = w.peers.listen(&w, 0).await?;
        let mut senders = vec![];
        let mut peers = vec![];
        let (fixes, mut received) = mpsc::unbounded_channel();
        for id in ["a", "b"] {
            let inv = w
                .start_pairing(
                    wire::DevicePairRequest {
                        id: id.into(),
                        name: format!("Phone {id}"),
                        kind: "phone".into(),
                    },
                    address.port(),
                )
                .await?;
            let mut req = format!("ws://{address}/v1/peer/share").into_client_request()?;
            req.headers_mut()
                .insert("authorization", format!("Bearer {}", inv.code).parse()?);
            req.headers_mut()
                .insert("x-aimanity-device", "phone".parse()?);
            req.headers_mut()
                .insert("x-aimanity-system", "android".parse()?);
            let (mut sock, _) = tokio_tungstenite::connect_async(req).await?;
            sock.next().await.unwrap()?;
            let (send, mut notices) = mpsc::unbounded_channel::<wire::PhoneNotification>();
            senders.push(send);
            let fixes = fixes.clone();
            let mut image = Vec::new();
            image::codecs::jpeg::JpegEncoder::new(&mut image)
                .encode_image(&image::RgbImage::new(2, 2))?;
            peers.push(tokio::spawn(async move {
                loop {
                    let reply=tokio::select!{
                        Some(n)=notices.recv()=>wire::HostDesktopReply{notification:Some(n),..Default::default()},
                        msg=sock.next()=>{
                            let Some(Ok(Frame::Binary(bytes)))=msg else{break;};
                            let a=wire::DesktopActionRequest::decode(bytes)?;
                            let mut r=wire::HostDesktopReply{action_id:a.action_id,..Default::default()};
                            if let Some(fix)=a.location { r.location=Some(fix);r.location_scope="system".into();fixes.send((id,fix))?; }
                            else {r.observation_id="frame".into();r.width=2;r.height=2;r.jpeg=image.clone();}
                            r
                        }
                    };
                    sock.send(Frame::Binary(reply.encode_to_vec().into())).await?;
                }
                Result::Ok(())
            }));
        }
        w.run();
        w.submit(wire::ChatSendRequest {
            request_id: "main".into(),
            text: "Main task".into(),
            device_id: "a".into(),
            ..Default::default()
        })?;
        let limit = Duration::from_secs(6);
        tokio::time::timeout(limit, arrivals.acquire())
            .await??
            .forget();
        let notice = wire::PhoneNotification {
            event_id: "same-os-id".into(),
            app_name: "Inbox".into(),
            title: "New work".into(),
            ..Default::default()
        };
        senders[1].send(notice.clone())?;
        senders[1].send(notice.clone())?;
        senders[0].send(notice)?;
        tokio::time::timeout(limit, arrivals.acquire())
            .await??
            .forget();
        let mut changes = w.subscribe();
        tokio::time::timeout(limit, async {
            loop {
                if w.todos()?.items.len() == 3 {
                    break;
                }
                changes.changed().await?;
            }
            Result::Ok(())
        })
        .await??;
        let snapshot = w.history(wire::ChatHistoryRequest {
            limit: 40,
            ..Default::default()
        })?;
        assert_eq!(snapshot.run_id, "main");
        assert_eq!(snapshot.children.len(), 1);
        assert_eq!(snapshot.children[0].device, "Phone b");
        let todos = w.todos()?;
        assert_eq!(
            todos.items.iter().filter(|t| t.status == "queued").count(),
            1
        );
        assert_eq!(
            todos
                .items
                .iter()
                .filter(|t| t.source == "notification")
                .count(),
            2
        );

        let point = wire::GeoPoint {
            latitude: 31.,
            longitude: 121.,
            accuracy_m: 8.,
        };
        let s = w
            .set_location(wire::LocationUpdate {
                enabled: true,
                point: Some(point),
                speed_kph: 5.,
                request_id: "loc".into(),
                ..Default::default()
            })
            .await?;
        assert!(s.devices.len() == 2 && s.devices.iter().all(|r| r.status == "applied"));
        let mut ids = BTreeSet::new();
        for _ in 0..2 {
            let (id, fix) = tokio::time::timeout(limit, received.recv()).await?.unwrap();
            assert_eq!(fix.point, Some(point));
            ids.insert(id);
        }
        assert_eq!(ids, BTreeSet::from(["a", "b"]));
        answers.add_permits(6);
        tokio::time::timeout(limit, async {
            loop {
                if w.todos()?.items.is_empty() && w.run.lock().unwrap().is_empty() {
                    break;
                }
                changes.changed().await?;
            }
            Result::Ok(())
        })
        .await??;
        assert!(w.history(Default::default())?.children.is_empty());
        exercise_delegation.store(true, std::sync::atomic::Ordering::Relaxed);
        for source in ["user", "liberty", "goal"] {
            let state = w.proactive_state()?;
            let id = match source {
                "user" => {
                    w.submit(wire::ChatSendRequest {
                        request_id: "many".into(),
                        text: "Inspect both devices".into(),
                        ..Default::default()
                    })?
                    .run_id
                }
                "liberty" => {
                    let s = w.set_mode(
                        "proactive/update",
                        &wire::ProactiveUpdateRequest {
                            exploring: true,
                            notifications_enabled: true,
                            interval_seconds: 60,
                            expected_revision: state.revision,
                            ..Default::default()
                        }
                        .encode_to_vec(),
                    )?;
                    format!("mode/liberty/{}/0", s.revision)
                }
                _ => {
                    let s = w.set_mode(
                        "proactive/goal",
                        &wire::GoalUpdateRequest {
                            goal: "Inspect both devices".into(),
                            expected_revision: state.revision,
                            ..Default::default()
                        }
                        .encode_to_vec(),
                    )?;
                    format!("mode/goal/{}/0", s.revision)
                }
            };
            tokio::time::timeout(limit, async {
                loop {
                    let log: wire::HarnessEventList = w.get(&format!("events/{id}"))?;
                    if let Some(e) = log.events.iter().find(|e| e.title == "delegate_tasks") {
                        let results: Vec<serde_json::Value> = serde_json::from_str(&e.content)?;
                        assert_eq!(results.len(), 2, "{source}");
                        assert!(
                            results
                                .iter()
                                .all(|r| r["receipts"] == 1 && r["error"] == ""),
                            "{source}: {results:?}"
                        );
                        if w.todos()?.items.is_empty() {
                            break;
                        }
                    }
                    changes.changed().await?;
                }
                Result::Ok(())
            })
            .await
            .map_err(|_| format!("missing delegation for {source}"))??;
        }
        assert_eq!(w.proactive_state()?.goal_status, "completed");
        w.stop().await;
        for peer in peers {
            peer.await??;
        }
        server.abort();
        Ok(())
    }
}
