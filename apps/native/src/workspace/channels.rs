use super::*;
use aimanity_channels::{Channel, Config, Incoming, PROVIDERS, Pairing, Payload};
use futures_util::{FutureExt, future::BoxFuture};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Mutex, Weak},
};

pub(super) const BINDINGS: TableDefinition<&str, &[u8]> = TableDefinition::new("channel_bindings");
pub(super) const OUTBOX: TableDefinition<&str, &[u8]> = TableDefinition::new("channel_outbox");
#[derive(Default)]
pub(super) struct Channels {
    sessions: Mutex<BTreeMap<String, Channel>>,
    mutation: tokio::sync::Mutex<()>,
    pub(super) ready: Arc<Notify>,
}
#[derive(Serialize, Deserialize)]
struct Delivery {
    binding: String,
    version: String,
    owner: String,
    target: Value,
    text: String,
    attempted: bool,
}
struct Sink {
    workspace: Weak<Workspace>,
    key: String,
    version: String,
}
fn now() -> u64 {
    crate::connection::epoch_ms() as u64
}
impl Sink {
    fn workspace(&self) -> Result<Arc<Workspace>> {
        self.workspace
            .upgrade()
            .ok_or_else(|| "channel_stopped".into())
    }
    fn edit(&self, f: impl FnOnce(&mut Config) -> Result<()>) -> Result<()> {
        let w = self.workspace()?;
        let tx = w.db.begin_write()?;
        {
            let mut table = tx.open_table(BINDINGS)?;
            let mut c: Config = serde_json::from_slice(
                table
                    .get(self.key.as_str())?
                    .ok_or("channel_unbound")?
                    .value(),
            )?;
            if c.version != self.version {
                return Err("channel_binding_changed".into());
            }
            f(&mut c)?;
            table.insert(self.key.as_str(), serde_json::to_vec(&c)?.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }
    async fn receive(
        &self,
        sender: String,
        id: String,
        text: String,
        target: Value,
        direct: bool,
    ) -> Result<()> {
        if id.is_empty() || text.trim().is_empty() || text.len() > 24_000 {
            return Ok(());
        }
        let w = self.workspace()?;
        let pair = text.split_whitespace().collect::<Vec<_>>();
        if matches!(pair.first().copied(), Some("/pair" | "配对")) {
            if !direct || pair.len() != 2 {
                return Ok(());
            }
            let mut approved = false;
            self.edit(|c| {
                if c.pairing.as_mut().is_some_and(|p| p.accept(pair[1], now())) {
                    c.owner = sender.clone();
                    c.pairing = None;
                    approved = true;
                }
                Ok(())
            })?;
            if approved {
                let session = w.channels.sessions.lock().unwrap().get(&self.key).cloned();
                if let Some(session) = session {
                    session
                        .send(
                            &target,
                            Payload::Text("Paired.".into()),
                            &uuid::Uuid::new_v4().to_string(),
                        )
                        .await?;
                }
            }
            return Ok(());
        }
        let tx = w.db.begin_write()?;
        {
            let configs = tx.open_table(BINDINGS)?;
            let Some(bytes) = configs.get(self.key.as_str())? else {
                return Ok(());
            };
            let c: Config = serde_json::from_slice(bytes.value())?;
            if c.version != self.version || c.owner != sender {
                return Ok(());
            }
            let (_, provider) = self.key.rsplit_once('/').ok_or("channel_scope")?;
            let event = aimanity_channels::event_id(provider, &c.app_id, &id);
            let participant = aimanity_channels::event_id(provider, &c.app_id, &sender);
            let run = format!("channel/{}/{}/{participant}/{event}", self.key, c.version);
            let mut inbox = tx.open_table(OUTBOX)?;
            if inbox.get(run.as_str())?.is_some()
                || tx.open_table(IDS)?.get(run.as_str())?.is_some()
            {
                return Ok(());
            }
            let d = Delivery {
                binding: self.key.clone(),
                version: c.version,
                owner: sender,
                target,
                text,
                attempted: false,
            };
            inbox.insert(run.as_str(), serde_json::to_vec(&d)?.as_slice())?;
        }
        tx.commit()?;
        w.channels.ready.notify_one();
        Ok(())
    }
}
impl Incoming for Sink {
    fn message<'a>(
        &'a self,
        sender: String,
        id: String,
        text: String,
        target: Value,
        direct: bool,
    ) -> BoxFuture<'a, Result<()>> {
        self.receive(sender, id, text, target, direct).boxed()
    }
    fn connected(&self) {
        if let Some(w) = self.workspace.upgrade() {
            w.channels.ready.notify_one();
        }
    }
    fn offset(&self, n: i64) -> Result<()> {
        self.edit(|c| {
            c.offset = n;
            Ok(())
        })
    }
    fn identify(&self, id: String) -> Result<()> {
        self.edit(|c| {
            c.identity = id;
            Ok(())
        })
    }
}
impl Workspace {
    fn channel_configs(&self) -> Result<BTreeMap<String, Config>> {
        self.db
            .begin_read()?
            .open_table(BINDINGS)?
            .iter()?
            .map(|r| {
                let (k, v) = r?;
                Ok((k.value().to_owned(), serde_json::from_slice(v.value())?))
            })
            .collect()
    }
    fn channel_list(&self, agent: &str) -> Result<wire::ChannelListResponse> {
        let configs = self.channel_configs()?;
        let sessions = self.channels.sessions.lock().unwrap();
        let bindings = PROVIDERS
            .iter()
            .map(|p| {
                let key = format!("{agent}/{p}");
                let Some(c) = configs.get(&key) else {
                    return wire::ChannelBinding {
                        provider: (*p).into(),
                        status: "unbound".into(),
                        ..Default::default()
                    };
                };
                let live = sessions.get(&key).map(Channel::live).unwrap_or_default();
                let pair = c.pairing.as_ref().filter(|x| x.expires > now());
                wire::ChannelBinding {
                    provider: (*p).into(),
                    app_id: c.app_id.clone(),
                    configured: true,
                    paired: !c.owner.is_empty(),
                    status: live.state,
                    error: live.error,
                    qr_png: live.qr_png,
                    proxy_url: c.proxy_url.clone(),
                    api_base: c.api_base.clone(),
                    pairing_command: pair
                        .map(|x| format!("/pair {}", x.code))
                        .unwrap_or_default(),
                    pairing_expires_at_ms: pair.map_or(0, |x| x.expires),
                }
            })
            .collect();
        Ok(wire::ChannelListResponse { bindings })
    }
    fn connect_channel(self: &Arc<Self>, key: String, mut config: Config) -> Result<()> {
        let (_, p) = key.rsplit_once('/').ok_or("channel_scope")?;
        if config.proxy_url.is_empty() && self.proxy.get().is_some_and(|p| p.enabled()) {
            config.proxy_url = aimanity_workbench_proxy::ADDRESS.into();
        }
        let sink = Arc::new(Sink {
            workspace: Arc::downgrade(self),
            version: config.version.clone(),
            key: key.clone(),
        });
        let channel = Channel::start(
            p,
            config,
            sink,
            self.files.with_file_name("channels"),
            self.shutdown.child_token(),
        )?;
        self.channels.sessions.lock().unwrap().insert(key, channel);
        Ok(())
    }
    pub(super) async fn channel_request(
        self: &Arc<Self>,
        op: &str,
        bytes: &[u8],
    ) -> Result<wire::ChannelListResponse> {
        let (agent, provider, bind) = match op {
            "list" => {
                let q = wire::ChannelListRequest::decode(bytes)?;
                let agent = if q.agent_id.is_empty() {
                    self.registry()?.active_agent_id
                } else {
                    q.agent_id
                };
                return self.channel_list(&agent);
            }
            "bind" => {
                let q = wire::ChannelBindRequest::decode(bytes)?;
                (q.agent_id.clone(), q.provider.clone(), Some(q))
            }
            "pair" => {
                let q = wire::ChannelPairRequest::decode(bytes)?;
                (q.agent_id, q.provider, None)
            }
            "unbind" => {
                let q = wire::ChannelUnbindRequest::decode(bytes)?;
                (q.agent_id, q.provider, None)
            }
            _ => return Err("unknown channel operation".into()),
        };
        if !PROVIDERS.contains(&provider.as_str()) {
            return Err("未知渠道".into());
        }
        let registry = self.registry()?;
        let agent = if agent.is_empty() {
            registry.active_agent_id
        } else {
            agent
        };
        if !registry.agents.iter().any(|a| a.id == agent) {
            return Err("请先配置 Agent".into());
        }
        let _lock = self.channels.mutation.lock().await;
        let key = format!("{agent}/{provider}");
        let mut configs = self.channel_configs()?;
        let old = configs.remove(&key);
        let config = if let Some(q) = bind {
            let mut c = old.clone().unwrap_or_default();
            let secret = if q.secret.is_empty() {
                c.secret.clone()
            } else {
                q.secret
            };
            let app = match provider.as_str() {
                "whatsapp" if !q.reset_session && !c.app_id.is_empty() => c.app_id.clone(),
                "whatsapp" => uuid::Uuid::new_v4().to_string(),
                "telegram" => aimanity_channels::telegram_identity(&secret)
                    .ok_or("Bot Token 无效")?
                    .to_string(),
                _ => {
                    if q.app_id.trim().is_empty() || secret.trim().is_empty() {
                        return Err("请填写 Bot ID 和 Secret".into());
                    }
                    q.app_id
                }
            };
            let base = q.api_base.trim_end_matches('/').to_string();
            if !base.is_empty() {
                let url = reqwest::Url::parse(&base)?;
                let local = url.host_str().is_some_and(|h| {
                    h == "localhost"
                        || h.trim_matches(['[', ']'])
                            .parse::<std::net::IpAddr>()
                            .is_ok_and(|ip| ip.is_loopback())
                });
                if provider != "telegram"
                    || !(url.scheme() == "https" || url.scheme() == "http" && local)
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.fragment().is_some()
                    || url.query().is_some()
                {
                    return Err("Bot API URL 需要 HTTPS 或本机 HTTP".into());
                }
            }
            if !q.proxy_url.trim().is_empty() {
                reqwest::Proxy::all(&q.proxy_url)?;
            }
            if configs.iter().any(|(key, c)| {
                key.ends_with(&format!("/{provider}")) && c.app_id == app && c.api_base == base
            }) {
                return Err("此机器人已绑定其他 Agent".into());
            }
            if c.app_id != app || c.api_base != base || q.reset_session {
                c = Config::default();
            }
            c.app_id = app;
            c.secret = secret;
            c.proxy_url = q.proxy_url;
            c.api_base = base;
            if c.version.is_empty() {
                c.version = uuid::Uuid::new_v4().to_string();
            }
            if c.owner.is_empty() {
                c.pairing = Some(Pairing::new(now()));
            }
            Some(c)
        } else if op == "pair" {
            let mut c = old.clone().ok_or("请先绑定渠道")?;
            c.pairing = Some(Pairing::new(now()));
            Some(c)
        } else {
            None
        };
        if op != "pair" {
            let session = self.channels.sessions.lock().unwrap().remove(&key);
            if let Some(s) = session {
                let unlink = old.as_ref().is_some_and(|o| {
                    provider == "whatsapp" && config.as_ref().is_none_or(|c| c.app_id != o.app_id)
                });
                let result = s.stop(unlink).await;
                if let Err(e) = result {
                    eprintln!("Channel unlink: {e}");
                }
            }
        }
        let tx = self.db.begin_write()?;
        {
            let mut records = tx.open_table(BINDINGS)?;
            match &config {
                Some(c) => {
                    records.insert(key.as_str(), serde_json::to_vec(c)?.as_slice())?;
                }
                None => {
                    records.remove(key.as_str())?;
                }
            }
        }
        tx.commit()?;
        if op != "pair"
            && let Some(old) = old
            && provider == "whatsapp"
            && config.as_ref().is_none_or(|c| c.app_id != old.app_id)
        {
            let path = self
                .files
                .with_file_name("channels")
                .join(format!("{}.redb", uuid::Uuid::parse_str(&old.app_id)?));
            if path.exists() {
                std::fs::remove_file(path)?;
            }
        }
        if op != "pair"
            && let Some(c) = config
        {
            self.connect_channel(key, c)?;
        }
        self.channels.ready.notify_one();
        self.channel_list(&agent)
    }
    async fn deliver_channels(&self) -> Result<()> {
        let rows = self
            .db
            .begin_read()?
            .open_table(OUTBOX)?
            .iter()?
            .map(|r| {
                let (k, v) = r?;
                Ok((
                    k.value().to_owned(),
                    serde_json::from_slice::<Delivery>(v.value())?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let configs = self.channel_configs()?;
        for (run, mut d) in rows {
            let config = configs.get(&d.binding);
            if config.is_none_or(|c| c.version != d.version || c.owner != d.owner) {
                self.finish_delivery(&run)?;
                continue;
            }
            if d.attempted {
                continue;
            }
            let ordinal = self
                .db
                .begin_read()?
                .open_table(IDS)?
                .get(run.as_str())?
                .map(|v| v.value());
            let Some(n) = ordinal else {
                let (agent, _) = d.binding.rsplit_once('/').ok_or("channel_scope")?;
                self.submit_job(
                    wire::ChatSendRequest {
                        request_id: run.clone(),
                        agent_id: agent.into(),
                        text: d.text,
                        ..Default::default()
                    },
                    "channel",
                    "",
                )?;
                continue;
            };
            let table = self.db.begin_read()?.open_table(MESSAGES)?;
            let reply = wire::ChatMessage::decode(table.get(n + 1)?.ok_or("消息不存在")?.value())?;
            if reply.active {
                continue;
            }
            let text = if reply.system_notice.is_empty() {
                reply.text
            } else {
                "The task stopped. Check AImanity for details.".into()
            };
            if text.is_empty() {
                continue;
            }
            let session = self
                .channels
                .sessions
                .lock()
                .unwrap()
                .get(&d.binding)
                .cloned();
            let Some(channel) = session.filter(|s| s.live().state == "connected") else {
                continue;
            };
            d.attempted = true;
            let tx = self.db.begin_write()?;
            tx.open_table(OUTBOX)?
                .insert(run.as_str(), serde_json::to_vec(&d)?.as_slice())?;
            tx.commit()?;
            let (_, id) = run.rsplit_once('/').ok_or("channel_scope")?;
            match channel.send(&d.target, Payload::Text(text), id).await {
                Ok(()) => self.finish_delivery(&run)?,
                Err(e) => eprintln!("Channel delivery: {e}"),
            }
        }
        Ok(())
    }
    fn finish_delivery(&self, run: &str) -> Result<()> {
        let tx = self.db.begin_write()?;
        tx.open_table(OUTBOX)?.remove(run)?;
        tx.commit()?;
        Ok(())
    }
}
pub(super) async fn serve(weak: Weak<Workspace>, shutdown: CancellationToken, ready: Arc<Notify>) {
    if let Some(w) = weak.upgrade() {
        let _lock = w.channels.mutation.lock().await;
        if let Ok(configs) = w.channel_configs() {
            for (k, c) in configs {
                if !w.channels.sessions.lock().unwrap().contains_key(&k)
                    && let Err(e) = w.connect_channel(k, c)
                {
                    eprintln!("Channel restore: {e}");
                }
            }
        }
    }
    ready.notify_one();
    loop {
        tokio::select! {_ = shutdown.cancelled()=>break,_=ready.notified()=>{if let Some(w)=weak.upgrade()&&let Err(e)=w.deliver_channels().await{eprintln!("Channel queue: {e}");}}}
    }
    if let Some(w) = weak.upgrade() {
        let sessions = std::mem::take(&mut *w.channels.sessions.lock().unwrap());
        for s in sessions.into_values() {
            let _ = s.stop(false).await;
        }
    }
}
