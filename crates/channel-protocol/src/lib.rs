mod feishu;
mod linked_device;
mod qq;
mod socket;
mod telegram;
mod transport;
mod wecom;
mod whatsapp;

use std::time::Duration;
pub use store::{Config, PROVIDERS, Pairing, Result, event_id};
pub use telegram::identity as telegram_identity;
pub use transport::{Channel, Incoming, Live, Payload};

pub mod store {
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha512};
    use uuid::Uuid;
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
    pub const PROVIDERS: [&str; 5] = ["wecom", "feishu", "qq", "telegram", "whatsapp"];
    pub fn event_id(provider: &str, app: &str, id: &str) -> String {
        let mut bytes: [u8; 16] = Sha512::digest(serde_json::to_vec(&(provider, app, id)).unwrap())
            [..16]
            .try_into()
            .unwrap();
        bytes[6] = (bytes[6] & 15) | 128;
        bytes[8] = (bytes[8] & 63) | 128;
        Uuid::from_bytes(bytes).to_string()
    }
    #[derive(Clone, Default, Serialize, Deserialize)]
    pub struct Config {
        pub app_id: String,
        pub secret: String,
        pub version: String,
        pub offset: i64,
        pub proxy_url: String,
        pub api_base: String,
        pub owner: String,
        pub pairing: Option<Pairing>,
        pub identity: String,
    }
    #[derive(Clone, Serialize, Deserialize)]
    pub struct Pairing {
        pub code: String,
        pub expires: u64,
        attempts: u8,
    }
    impl Pairing {
        pub fn new(now: u64) -> Self {
            Self {
                code: format!("{:06}", Uuid::new_v4().as_u128() % 1_000_000),
                expires: now + 600_000,
                attempts: 0,
            }
        }
        pub fn accept(&mut self, code: &str, now: u64) -> bool {
            if now >= self.expires || self.attempts >= 10 {
                return false;
            }
            self.attempts += 1;
            self.code == code
        }
    }
}
struct State;
impl State {
    fn external_client(proxy: &str, timeout: Duration) -> Result<reqwest::Client> {
        let mut b = reqwest::Client::builder()
            .no_proxy()
            .http1_only()
            .tcp_nodelay(true)
            .timeout(timeout);
        if !proxy.is_empty() {
            b = b.proxy(reqwest::Proxy::all(proxy)?);
        }
        Ok(b.build()?)
    }
}
