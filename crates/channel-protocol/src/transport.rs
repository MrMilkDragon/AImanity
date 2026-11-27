use super::*;
use futures_util::future::BoxFuture;
use serde_json::Value;
use std::sync::{Arc, RwLock};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub enum Payload {
    Text(String),
    Image(Vec<u8>),
}
pub trait Incoming: Send + Sync {
    fn message<'a>(
        &'a self,
        sender: String,
        id: String,
        text: String,
        target: Value,
        direct: bool,
    ) -> BoxFuture<'a, Result<()>>;
    fn connected(&self);
    fn offset(&self, offset: i64) -> Result<()>;
    fn identify(&self, identity: String) -> Result<()>;
}
#[derive(Clone, Default)]
pub struct Live {
    pub state: String,
    pub error: String,
    pub qr_png: Vec<u8>,
}

#[derive(Clone)]
pub(super) struct Context {
    sink: Arc<dyn Incoming>,
    pub live: Arc<RwLock<Live>>,
    pub cancel: CancellationToken,
    pub tasks: TaskTracker,
    api_base: String,
    dir: std::path::PathBuf,
}
impl Context {
    pub(super) fn telegram_api(&self) -> &str {
        if self.api_base.is_empty() {
            "https://api.telegram.org"
        } else {
            &self.api_base
        }
    }
    pub async fn request(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        Ok(
            tokio::select! { _=self.cancel.cancelled()=>return Err("channel_stopped".into()), r=request.send()=>r.map_err(reqwest::Error::without_url)? },
        )
    }
    pub fn status(&self, state: &str, error: &str) {
        let mut live = self.live.write().unwrap();
        let established = state == "connected" && live.state != state;
        live.state = state.into();
        live.error = error.into();
        if state == "connected" {
            live.qr_png.clear();
        }
        drop(live);
        if established {
            self.sink.connected();
        }
    }
    pub async fn message(
        &self,
        sender: String,
        id: String,
        text: String,
        target: Value,
        direct: bool,
        bot: bool,
    ) -> Result<()> {
        if bot || sender.is_empty() {
            return Ok(());
        }
        self.sink.message(sender, id, text, target, direct).await
    }
    pub async fn offset(&self, n: i64) -> Result<()> {
        self.sink.offset(n)
    }
    pub async fn identify(&self, id: String) -> Result<()> {
        self.sink.identify(id)
    }
    pub fn whatsapp_store(&self, binding: uuid::Uuid) -> Result<linked_device::Store> {
        linked_device::Store::open(&self.dir, binding)
    }
}

enum Transport {
    Telegram(Arc<telegram::Telegram>),
    Wecom(Arc<wecom::Wecom>),
    Feishu(Arc<feishu::Feishu>),
    Qq(Arc<qq::Qq>),
    WhatsApp(Arc<whatsapp::WhatsApp>),
}
#[derive(Clone)]
pub struct Channel {
    transport: Arc<Transport>,
    context: Context,
}
impl Channel {
    pub fn start(
        provider: &str,
        config: Config,
        sink: Arc<dyn Incoming>,
        dir: std::path::PathBuf,
        cancel: CancellationToken,
    ) -> Result<Self> {
        let context = Context {
            sink,
            dir,
            cancel,
            tasks: TaskTracker::new(),
            api_base: config.api_base.clone(),
            live: Arc::new(RwLock::new(Live {
                state: "connecting".into(),
                ..Default::default()
            })),
        };
        let transport = match provider {
            "telegram" => Transport::Telegram(telegram::start(context.clone(), config)?),
            "wecom" => Transport::Wecom(wecom::start(context.clone(), config)?),
            "feishu" => Transport::Feishu(feishu::start(context.clone(), config)?),
            "qq" => Transport::Qq(qq::start(context.clone(), config)?),
            "whatsapp" => Transport::WhatsApp(whatsapp::start(context.clone(), config)?),
            _ => return Err("invalid_channel_provider".into()),
        };
        Ok(Self {
            context,
            transport: Arc::new(transport),
        })
    }
    pub fn live(&self) -> Live {
        self.context.live.read().unwrap().clone()
    }
    pub async fn send(&self, target: &Value, payload: Payload, id: &str) -> Result<()> {
        if self.context.cancel.is_cancelled() {
            return Err("channel_stopped".into());
        }
        match &*self.transport {
            Transport::Telegram(c) => c.send(target, payload, id).await,
            Transport::Wecom(c) => c.send(target, payload, id).await,
            Transport::Feishu(c) => c.send(target, payload, id).await,
            Transport::Qq(c) => c.send(target, payload, id).await,
            Transport::WhatsApp(c) => c.send(target, payload, id).await,
        }
    }
    pub async fn stop(&self, unlink: bool) -> Result<()> {
        let result = if unlink && let Transport::WhatsApp(c) = &*self.transport {
            c.logout().await.map(|_| ())
        } else {
            Ok(())
        };
        self.context.cancel.cancel();
        self.context.tasks.close();
        self.context.tasks.wait().await;
        result
    }
}
