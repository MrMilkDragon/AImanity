use ai_phone_api::wire;
use serde::{Deserialize, Serialize};
use std::time::Duration;
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default)]
    pub verified_at_ms: u64,
}
fn default_provider() -> String {
    "deepseek".into()
}
fn default_protocol() -> String {
    "responses".into()
}
impl ModelConfig {
    pub fn same_replay_identity(&self, other: &Self) -> bool {
        self.provider == other.provider
            && self.protocol == other.protocol
            && self.base_url == other.base_url
            && self.model == other.model
    }
    pub fn endpoint(&self) -> Result<String, String> {
        let suffix = match self.protocol.as_str() {
            "responses" => "/responses",
            "chat-completions" => "/chat/completions",
            "anthropic-messages" => "/messages",
            _ => return Err("请选择有效的模型服务商与 API 协议".into()),
        };
        let base = self.base_url.trim_end_matches('/');
        let url = reqwest::Url::parse(base).map_err(|_| "请输入完整的 http(s) 地址")?;
        let path = url.path().trim_end_matches('/');
        if path.ends_with(suffix) {
            return Ok(base.into());
        }
        if path.ends_with("/responses")
            || path.ends_with("/chat/completions")
            || path.ends_with("/messages")
        {
            return Err("API 地址的末尾与所选协议不一致".into());
        }
        let version = if self.protocol == "anthropic-messages" && !path.ends_with("/v1") {
            "/v1"
        } else {
            ""
        };
        Ok(format!("{base}{version}{suffix}"))
    }
    pub fn client(&self) -> Result<reqwest::Client, String> {
        client_for_proxy(&self.proxy_url)
    }
    pub fn profile(&self, revision: u64) -> wire::ModelProfile {
        wire::ModelProfile {
            revision,
            provider: self.provider.clone(),
            protocol: self.protocol.clone(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            proxy_url: self.proxy_url.clone(),
            has_api_key: !self.api_key.is_empty(),
            verified_at_ms: self.verified_at_ms,
        }
    }
    pub fn unverified(mut self) -> Self {
        self.verified_at_ms = 0;
        self
    }
}
pub fn client_for_proxy(proxy_url: &str) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(180));
    if !proxy_url.is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url).map_err(|_| "代理地址无效")?);
    }
    builder.build().map_err(|_| "无法创建模型连接".into())
}
