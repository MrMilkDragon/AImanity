use ai_phone_api::wire;
use prost::Message;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition, WriteTransaction};
use std::{error::Error, fs, path::Path, sync::Arc};
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;

mod channels;
mod conversation;
mod device_run;
mod documents;
mod files;
mod lifecycle;
mod location;
mod modes;
mod network;
#[cfg(any(
    target_os = "macos",
    windows,
    all(target_os = "android", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "x86_64")
))]
mod owned;
#[cfg(all(test, any(target_os = "macos", windows)))]
#[path = "../tests/installed/overlay.rs"]
mod fluent_overlay_qualification;
mod peers;
#[cfg(any(
    target_os = "macos",
    windows,
    all(target_os = "android", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "x86_64")
))]
mod power;
mod records;
mod scheduling;
mod skills;
mod worker;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const RECORDS: TableDefinition<&str, &[u8]> = TableDefinition::new("workspace");
const MESSAGES: TableDefinition<u64, &[u8]> = TableDefinition::new("messages");
const IDS: TableDefinition<&str, u64> = TableDefinition::new("message_ids");
const JOBS: TableDefinition<u64, &[u8]> = TableDefinition::new("jobs");
const PENDING: TableDefinition<&str, &[u8]> = TableDefinition::new("pending");
const INBOX: TableDefinition<&str, &[u8]> = TableDefinition::new("notice_inbox");
const BLOBS: TableDefinition<&str, &[u8]> = TableDefinition::new("attachments");

pub(crate) struct Workspace {
    pub(crate) host: std::sync::Mutex<Option<(String, Arc<crate::host_desktop::Grant>)>>,
    pub(crate) host_microphone: Arc<tokio::sync::Semaphore>,
    channels: channels::Channels,
    db: Database,
    peers: Arc<peers::Peers>,
    #[cfg(any(
        target_os = "macos",
        windows,
        all(target_os = "android", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "x86_64")
    ))]
    power: std::sync::Mutex<std::collections::BTreeMap<String, power::Owner>>,
    files: std::path::PathBuf,
    changes: watch::Sender<u64>,
    live: std::sync::Mutex<Option<conversation::LiveReply>>,
    ready: Arc<Notify>,
    location: tokio::sync::Mutex<std::collections::HashMap<String, CancellationToken>>,
    location_changed: Arc<Notify>,
    shutdown: CancellationToken,
    run: std::sync::Mutex<std::collections::BTreeMap<String, scheduling::Active>>,
    verified: std::sync::Mutex<std::collections::HashMap<String, (String, u64, wire::ModelDraft)>>,
    worker: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    clients: std::sync::Mutex<std::collections::HashMap<String, reqwest::Client>>,
    proxy: std::sync::OnceLock<aimanity_workbench_proxy::HarnessProxy>,
}

impl std::fmt::Debug for Workspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workspace").finish_non_exhaustive()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

impl Workspace {
    pub(crate) fn open(root: &Path) -> Result<Arc<Self>> {
        let mut dir = fs::DirBuilder::new();
        let mut file = fs::OpenOptions::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
            dir.mode(0o700);
            file.mode(0o600);
        }
        dir.recursive(true).create(root)?;
        let handle = file
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("workspace.redb"))?;
        let files = root.join("files");
        dir.create(&files)?;
        Self::from_db(
            Database::builder()
                .set_cache_size(4 * 1024 * 1024)
                .create_with_backend(redb::backends::FileBackend::new(handle))?,
            files,
        )
    }

    pub(crate) fn from_db(db: Database, files: std::path::PathBuf) -> Result<Arc<Self>> {
        let tx = db.begin_write()?;
        tx.open_table(MESSAGES)?;
        tx.open_table(IDS)?;
        tx.open_table(JOBS)?;
        tx.open_table(PENDING)?;
        tx.open_table(BLOBS)?;
        tx.open_table(INBOX)?;
        tx.open_table(documents::DOCUMENTS)?;
        tx.open_table(skills::ENABLED)?;
        tx.open_table(skills::CUSTOM)?;
        tx.open_table(channels::BINDINGS)?;
        tx.open_table(channels::OUTBOX)?;
        {
            let mut data = tx.open_table(RECORDS)?;
            if data.get("chat")?.is_none() {
                data.insert(
                    "chat",
                    wire::ChatState {
                        conversation_id: uuid::Uuid::now_v7().to_string(),
                        ..Default::default()
                    }
                    .encode_to_vec()
                    .as_slice(),
                )?;
            }
        }
        tx.commit()?;
        let shutdown = CancellationToken::new();
        let this = Arc::new(Self {
            db,
            peers: peers::Peers::new(shutdown.clone()),
            #[cfg(any(
                target_os = "macos",
                windows,
                all(target_os = "android", target_arch = "aarch64"),
                all(target_os = "linux", target_arch = "x86_64")
            ))]
            power: Default::default(),
            files,
            host: Default::default(),
            host_microphone: Arc::new(tokio::sync::Semaphore::new(1)),
            changes: watch::channel(0).0,
            live: Default::default(),
            ready: Default::default(),
            location: Default::default(),
            location_changed: Default::default(),
            shutdown,
            run: Default::default(),
            clients: Default::default(),
            proxy: Default::default(),
            verified: Default::default(),
            worker: Default::default(),
            channels: Default::default(),
        });
        if this.files.with_file_name("proxy").join("profile.json").is_file() { this.network_proxy()?; }
        Ok(this)
    }

    pub(crate) fn run(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let stop = self.shutdown.clone();
        let ready = self.ready.clone();
        ready.notify_one();
        let channels = tokio::spawn(channels::serve(weak.clone(), stop.clone(), self.channels.ready.clone()));
        let watch_location = tokio::spawn(Self::watch_location(
            weak.clone(),
            stop.clone(),
            self.location_changed.clone(),
            self.registry_changes(),
        ));
        #[cfg(any(
            target_os = "macos",
            windows,
            all(target_os = "android", target_arch = "aarch64"),
            all(target_os = "linux", target_arch = "x86_64")
        ))]
        let watch_owned = tokio::spawn(owned::watch(
            weak.clone(),
            self.registry_changes(),
            stop.clone(),
        ));
        let worker = tokio::spawn(async move {
            if let Some(workspace) = weak.upgrade() {
                if let Some(proxy) = workspace.proxy.get()
                    && proxy.enabled() && let Err(e) = proxy.restore().await {
                    eprintln!("Workspace proxy: {e}");
                }
                workspace.finish_device_changes().await;
                #[cfg(any(
                    target_os = "macos",
                    windows,
                    all(target_os = "android", target_arch = "aarch64"),
                    all(target_os = "linux", target_arch = "x86_64")
                ))]
                workspace.resume_devices();
            }
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                let delay = weak.upgrade().and_then(|w| w.mode_wait());
                let timer = async {
                    match delay {
                        Some(d) => tokio::time::sleep(d).await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {biased;
                    _ = stop.cancelled() => break,
                    _ = timer => {},
                    Some(done) = tasks.join_next(), if !tasks.is_empty() => { if let Err(e) = done { eprintln!("Workspace task: {e}"); } }
                    _ = ready.notified() => {}
                }
                let Some(workspace) = weak.upgrade() else {
                    break;
                };
                if let Err(e) = workspace.schedule_mode() {
                    eprintln!("Workspace mode: {e}");
                }
                if let Err(e) = workspace.flush_notices() {
                    eprintln!("Workspace inbox: {e}");
                }
                match workspace.claim_jobs() {
                    Ok(jobs) => {
                        for (n, job, id, token, child) in jobs {
                            let w = workspace.clone();
                            tasks.spawn(async move {
                                if let Err(e) = w.execute(n, job, &token, child).await {
                                    eprintln!("Workspace task: {e}");
                                }
                                w.run.lock().unwrap().remove(&id);
                                w.changed();
                            });
                        }
                    }
                    Err(e) => eprintln!("Workspace queue: {e}"),
                }
            }
            while tasks.join_next().await.is_some() {}
            let _ = channels.await;
            let _ = watch_location.await;
            #[cfg(any(
                target_os = "macos",
                windows,
                all(target_os = "android", target_arch = "aarch64"),
                all(target_os = "linux", target_arch = "x86_64")
            ))]
            let _ = watch_owned.await;
        });
        *self.worker.lock().unwrap() = Some(worker);
    }

    pub(crate) async fn stop(&self) {
        self.shutdown.cancel();
        #[cfg(any(
            target_os = "macos",
            windows,
            all(target_os = "android", target_arch = "aarch64"),
            all(target_os = "linux", target_arch = "x86_64")
        ))]
        self.stop_power(None).await;
        self.peers.close().await;
        if let Some(proxy) = self.proxy.get() { proxy.stop().await; }
        let worker = self.worker.lock().unwrap().take();
        if let Some(worker) = worker {
            let _ = worker.await;
        }
    }
    pub(crate) fn disconnect_reason(&self, id: &str) -> Option<String> {
        self.peers.disconnect_reason(id)
    }
    pub(crate) fn frame_updates(
        &self,
        id: &str,
    ) -> Option<watch::Receiver<Option<Arc<wire::DeviceCursor>>>> {
        self.peers.frames(id)
    }
    pub(crate) async fn manual_input(
        &self,
        mut q: wire::DesktopActionRequest,
    ) -> std::result::Result<wire::HostDesktopReply, String> {
        q.epoch = 2;
        q.control_session.clear();
        self.peers.perform(q, None).await.map_err(|e| e.to_string())
    }
    pub(super) async fn host_action(
        &self,
        action: wire::DesktopActionRequest,
    ) -> Result<wire::HostDesktopReply> {
        if action.action_id.is_empty() {
            return Err("操作缺少回执编号".into());
        }
        let grant = self
            .host
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(owner, grant)| *owner == action.control_session && grant.active())
            .map(|(_, grant)| grant.clone())
            .ok_or("本任务未授权使用宿主机")?;
        Ok(tokio::task::spawn_blocking(move || grant.perform(&action)).await??)
    }
    pub(crate) async fn device_io(
        &self,
        id: &str,
        op: wire::DeviceIoOperation,
        path: &str,
        destination: &str,
    ) -> Result<crate::device_io::Lane> {
        self.peers
            .io(
                id,
                wire::DeviceIoRequest {
                    operation: op.into(),
                    path: path.into(),
                    destination: destination.into(),
                    ..Default::default()
                },
                None,
            )
            .await
    }
    pub(crate) fn registry_changes(&self) -> watch::Receiver<u64> {
        self.peers.changes.subscribe()
    }
    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }
    fn changed(&self) {
        self.changes.send_modify(|rev| *rev = rev.wrapping_add(1));
    }
    fn client(&self, proxy: &str) -> Result<reqwest::Client> {
        let proxy = if proxy.is_empty() && self.proxy.get().is_some_and(|p| p.enabled()) {
            aimanity_workbench_proxy::ADDRESS
        } else { proxy };
        let mut pool = self.clients.lock().unwrap();
        if let Some(client) = pool.get(proxy) {
            return Ok(client.clone());
        }
        let client = aimanity_model::client_for_proxy(proxy)?;
        pool.insert(proxy.into(), client.clone());
        Ok(client)
    }

    fn get<T: Message + Default>(&self, key: &str) -> Result<T> {
        read(&self.db.begin_read()?.open_table(RECORDS)?, key)
    }

    pub(super) async fn start_pairing(
        self: &Arc<Self>,
        request: wire::DevicePairRequest,
        port: u16,
    ) -> Result<wire::DeviceInvitation> {
        let invitation = self.peers.invite(self, request)?;
        self.peers.listen(self, port).await?;
        Ok(invitation)
    }
    pub(crate) async fn rpc(
        self: &Arc<Self>,
        route: &str,
        bytes: Vec<u8>,
    ) -> std::result::Result<Vec<u8>, String> {
        let name = route.strip_prefix("assistant/").unwrap_or(route);
        if let Some(change) = lifecycle::Change::route(name) {
            let q = wire::DesktopRequest::decode(bytes.as_slice()).map_err(|e| e.to_string())?;
            return self
                .end_device(&q.device_id, change)
                .await
                .map(|r| r.encode_to_vec())
                .map_err(|e| e.to_string());
        }
        if let Some(op) = name.strip_prefix("channels/") {
            return self.channel_request(op, &bytes).await.map(|list| list.encode_to_vec()).map_err(|e| e.to_string());
        }
        if name == "workbench/skill-import" {
            let q = wire::SkillImportRequest::decode(bytes.as_slice()).map_err(|e| e.to_string())?;
            return self.import_skill(q).await.map(|s| s.encode_to_vec()).map_err(|e| e.to_string());
        }
        if let Some(operation) = name.strip_prefix("proxy/") {
            return self.proxy_request(operation, bytes).await.map_err(|e| e.to_string());
        }
        if name == "files/list" || name == "files/manage" {
            let result: Result<Vec<u8>> = async {
                if name == "files/list" {
                    return Ok(self
                        .file_list(wire::FileLocation::decode(bytes.as_slice())?)
                        .await?
                        .encode_to_vec());
                }
                Ok(self
                    .file_manage(wire::FileManageRequest::decode(bytes.as_slice())?)
                    .await?
                    .encode_to_vec())
            }
            .await;
            return result.map_err(|e| e.to_string());
        }
        if name == "location/update" {
            let q = wire::LocationUpdate::decode(bytes.as_slice()).map_err(|e| e.to_string())?;
            return self
                .set_location(q)
                .await
                .map(|s| s.encode_to_vec())
                .map_err(|e| e.to_string());
        }
        if name == "host-desktop/action" {
            let action =
                wire::DesktopActionRequest::decode(bytes.as_slice()).map_err(|e| e.to_string())?;
            return self
                .host_action(action)
                .await
                .map(|reply| reply.encode_to_vec())
                .map_err(|e| e.to_string());
        }
        #[cfg(any(
            target_os = "macos",
            windows,
            all(target_os = "android", target_arch = "aarch64"),
            all(target_os = "linux", target_arch = "x86_64")
        ))]
        if name == "registry/device-start" {
            let q =
                wire::DeviceStartRequest::decode(bytes.as_slice()).map_err(|e| e.to_string())?;
            return self
                .start_device(q)
                .await
                .map(|s| s.encode_to_vec())
                .map_err(|e| e.to_string());
        }
        if name == "registry/device-invite" {
            let q = wire::DevicePairRequest::decode(bytes.as_slice()).map_err(|e| e.to_string())?;
            return self
                .start_pairing(q, peers::PORT)
                .await
                .map(|invite| invite.encode_to_vec())
                .map_err(|e| e.to_string());
        }
        if name == "registry/model-test" {
            return self
                .probe(
                    wire::ConnectionTestRequest::decode(bytes.as_slice())
                        .map_err(|e| e.to_string())?,
                )
                .await
                .map(|m| m.encode_to_vec())
                .map_err(|e| e.to_string());
        }
        let store = self.clone();
        let name = name.to_owned();
        tokio::task::spawn_blocking(move || store.dispatch(&name, &bytes))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    pub(crate) fn stash(&self, state: &wire::ChatState, id: &str) -> Result<()> {
        let tx = self.db.begin_write()?;
        tx.open_table(PENDING)?
            .insert(id, state.encode_to_vec().as_slice())?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn pending(&self) -> Result<Vec<wire::ChatState>> {
        self.db
            .begin_read()?
            .open_table(PENDING)?
            .iter()?
            .map(|row| Ok(wire::ChatState::decode(row?.1.value())?))
            .collect()
    }
    pub(crate) fn attachment(&self, id: &str) -> Result<Vec<u8>> {
        Ok(self
            .db
            .begin_read()?
            .open_table(BLOBS)?
            .get(id)?
            .ok_or("附件不存在")?
            .value()
            .to_vec())
    }
    fn store_attachment(&self, name: String, bytes: &[u8]) -> Result<wire::ChatAttachment> {
        let media = match image::guess_format(bytes) {
            Ok(image::ImageFormat::Png) => "image/png",
            Ok(image::ImageFormat::Jpeg) => "image/jpeg",
            Ok(image::ImageFormat::WebP) => "image/webp",
            Ok(image::ImageFormat::Gif) => "image/gif",
            _ if std::str::from_utf8(bytes).is_ok() => "text/plain",
            _ => "application/octet-stream",
        };
        let item = wire::ChatAttachment {
            id: uuid::Uuid::now_v7().to_string(),
            name,
            media_type: media.into(),
            size_bytes: bytes.len() as u64,
        };
        let tx = self.db.begin_write()?;
        tx.open_table(BLOBS)?.insert(item.id.as_str(), bytes)?;
        put(&tx, &format!("attachment/{}", item.id), &item)?;
        tx.commit()?;
        Ok(item)
    }
}
fn read<T: Message + Default>(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
) -> Result<T> {
    match table.get(key)? {
        Some(v) => Ok(T::decode(v.value())?),
        None => Ok(T::default()),
    }
}
fn put(tx: &WriteTransaction, key: &str, value: &impl Message) -> Result<()> {
    tx.open_table(RECORDS)?
        .insert(key, value.encode_to_vec().as_slice())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use redb::ReadableTableMetadata;

    #[tokio::test]
    async fn unconfigured_proxy_opens_without_starting_a_service() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let workspace = Workspace::open(dir.path())?;
        let bytes = workspace.rpc("proxy/state", Vec::new()).await?;
        assert_eq!(wire::HarnessProxyState::decode(bytes.as_slice())?, wire::HarnessProxyState::default());
        assert!(!dir.path().join("proxy").exists());
        Ok(())
    }

    #[tokio::test]
    async fn host_action_only_accepts_the_authorized_task() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let workspace = Workspace::open(dir.path())?;
        let (_stop, live) = watch::channel(());
        *workspace.host.lock().unwrap() = Some((
            "drawing".into(),
            Arc::new(crate::host_desktop::Grant::new(live)),
        ));
        let request = wire::DesktopActionRequest {
            kind: "stroke".into(),
            action_id: "a".into(),
            control_session: "another-task".into(),
            ..Default::default()
        };
        let result = workspace
            .rpc("host-desktop/action", request.encode_to_vec())
            .await;
        assert!(result.unwrap_err().contains("未授权"));
        Ok(())
    }

    #[tokio::test]
    async fn devices_of_the_same_kind_keep_their_own_identity_after_reopen() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let workspace = Workspace::open(dir.path())?;
        for (id, kind) in [
            ("a", "phone"),
            ("b", "phone"),
            ("c", "desktop"),
            ("d", "desktop"),
        ] {
            let draft = wire::DeviceSaveRequest {
                id: id.into(),
                name: format!("{kind} {id}"),
                kind: kind.into(),
                ..Default::default()
            };
            workspace.dispatch("registry/device-save", &draft.encode_to_vec())?;
        }
        let remove = wire::DesktopRequest {
            device_id: "b".into(),
        };
        workspace
            .rpc("registry/device-delete", remove.encode_to_vec())
            .await?;
        drop(workspace);
        let workspace = Workspace::open(dir.path())?;
        let state = workspace.dispatch("registry/state", &[])?;
        let ids: Vec<_> = wire::RegistryState::decode(state.as_slice())?
            .devices
            .into_iter()
            .map(|device| device.id)
            .collect();
        assert_eq!(ids, ["a", "c", "d"]);
        Ok(())
    }

    #[tokio::test]
    async fn reopening_keeps_queued_work_once_and_attachments_immutable() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        let endpoint = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let workspace = Workspace::open(root)?;
        let tx = workspace.db.begin_write()?;
        put(
            &tx,
            "registry",
            &wire::RegistryState {
                active_agent_id: "local".into(),
                agents: vec![wire::RegisteredAgent {
                    id: "local".into(),
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
                base_url: format!("http://{}/v1", endpoint.local_addr()?),
                model: "local".into(),
                ..Default::default()
            },
        )?;
        tx.commit()?;
        let attachment = workspace.upload("sheet.txt".into(), b"original".to_vec())?;
        fs::write(workspace.files.join("sheet.txt"), b"edited")?;
        let q = wire::ChatSendRequest {
            text: "Read the attachment".into(),
            request_id: "request".into(),
            attachment_ids: vec![attachment.id.clone()],
            ..Default::default()
        };
        workspace.stash(&wire::ChatState::default(), &q.request_id)?;
        let conversation = workspace.submit(q.clone())?.conversation_id;
        assert!(workspace.pending()?.is_empty());
        drop(workspace);
        let workspace = Workspace::open(root)?;
        assert_eq!(workspace.submit(q.clone())?.conversation_id, conversation);
        let page = workspace.history(wire::ChatHistoryRequest {
            limit: 80,
            ..Default::default()
        })?;
        assert_eq!(page.messages.len(), 2);
        assert_eq!(page.messages[0].attachments[0].id, attachment.id);
        assert_eq!(workspace.attachment(&attachment.id)?, b"original");
        assert_eq!(fs::read(workspace.files.join("sheet.txt"))?, b"edited");
        let q = wire::ChatSendRequest {
            agent_id: "local".into(),
            ..q
        };
        assert_eq!(workspace.next_job()?.unwrap().1.request, Some(q.clone()));
        workspace.control(
            wire::ChatRunRequest {
                run_id: q.request_id.clone(),
                pause: true,
            },
            false,
        )?;
        drop(workspace);
        let workspace = Workspace::open(root)?;
        assert!(workspace.next_job()?.is_none());
        workspace.control(
            wire::ChatRunRequest {
                run_id: q.request_id.clone(),
                ..Default::default()
            },
            true,
        )?;
        workspace.run();
        let timeout = std::time::Duration::from_secs(5);
        let (_socket, _) = tokio::time::timeout(timeout, endpoint.accept()).await??;
        tokio::time::timeout(timeout, workspace.stop()).await?;
        drop(workspace);
        let workspace = Workspace::open(root)?;
        assert_eq!(workspace.next_job()?.unwrap().1.request, Some(q));
        assert_eq!(workspace.db.begin_read()?.open_table(JOBS)?.len()?, 1);
        Ok(())
    }
}
