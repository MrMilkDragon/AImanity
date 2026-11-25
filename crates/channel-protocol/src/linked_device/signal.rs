use super::*;
use bytes::Bytes;
use whatsapp_rust::wacore::store::traits::{
    MsgSecretEntry, MsgSecretStore, SignalStore, merge_msg_secret_expiry,
    merge_msg_secret_message_ts,
};
#[async_trait]
impl SignalStore for Store {
    async fn put_identity(&self, address: &str, key: [u8; 32]) -> Result<()> {
        self.save_bytes("identity", address, &key)
    }
    async fn load_identity(&self, address: &str) -> Result<Option<[u8; 32]>> {
        self.get("identity", address)?
            .map(|v| {
                v.try_into()
                    .map_err(|_| StoreError::Validation("identity length".into()))
            })
            .transpose()
    }
    async fn delete_identity(&self, address: &str) -> Result<()> {
        self.delete("identity", address)
    }
    async fn get_session(&self, address: &str) -> Result<Option<Bytes>> {
        Ok(self.get("session", address)?.map(Bytes::from))
    }
    async fn put_session(&self, address: &str, session: &[u8]) -> Result<()> {
        self.save_bytes("session", address, session)
    }
    async fn delete_session(&self, address: &str) -> Result<()> {
        self.delete("session", address)
    }
    async fn has_signal_state_for_user(&self, user: &str) -> Result<bool> {
        let t = self
            .0
            .begin_read()
            .map_err(error)?
            .open_table(DATA)
            .map_err(error)?;
        for group in ["identity", "session"] {
            for r in t.range((group, user)..).map_err(error)? {
                let (k, _) = r.map_err(error)?;
                let (g, id) = k.value();
                if g != group || !id.starts_with(user) {
                    break;
                }
                if id.split(['@', ':']).next() == Some(user) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    async fn store_prekey(&self, id: u32, record: &[u8], uploaded: bool) -> Result<()> {
        self.store_prekeys_batch(&[(id, Bytes::copy_from_slice(record))], uploaded)
            .await
    }
    async fn store_prekeys_batch(&self, keys: &[(u32, Bytes)], uploaded: bool) -> Result<()> {
        self.edit(|t| {
            for (id, b) in keys {
                let key = id.to_string();
                t.insert(("prekey", key.as_str()), b.as_ref())
                    .map_err(error)?;
                if uploaded {
                    t.insert(("uploaded", key.as_str()), &[][..])
                        .map_err(error)?;
                }
            }
            Ok(())
        })
    }
    async fn load_prekey(&self, id: u32) -> Result<Option<Bytes>> {
        Ok(self.get("prekey", &id.to_string())?.map(Bytes::from))
    }
    async fn mark_prekeys_uploaded(&self, ids: &[u32]) -> Result<()> {
        self.edit(|t| {
            for id in ids {
                let key = id.to_string();
                if t.get(("prekey", key.as_str())).map_err(error)?.is_some() {
                    t.insert(("uploaded", key.as_str()), &[][..])
                        .map_err(error)?;
                }
            }
            Ok(())
        })
    }
    async fn remove_prekey(&self, id: u32) -> Result<()> {
        let key = id.to_string();
        self.edit(|t| {
            for group in ["prekey", "uploaded"] {
                t.remove((group, key.as_str())).map_err(error)?;
            }
            Ok(())
        })
    }
    async fn get_max_prekey_id(&self) -> Result<u32> {
        self.items("prekey")?
            .iter()
            .map(|(k, _)| k.parse().map_err(error))
            .try_fold(0, |max, i: Result<u32>| Ok(max.max(i?)))
    }
    async fn store_signed_prekey(&self, id: u32, record: &[u8]) -> Result<()> {
        self.save_bytes("signed", &id.to_string(), record)
    }
    async fn load_signed_prekey(&self, id: u32) -> Result<Option<Vec<u8>>> {
        self.get("signed", &id.to_string())
    }
    async fn load_all_signed_prekeys(&self) -> Result<Vec<(u32, Vec<u8>)>> {
        self.items("signed")?
            .into_iter()
            .map(|(id, b)| Ok((id.parse().map_err(error)?, b)))
            .collect()
    }
    async fn remove_signed_prekey(&self, id: u32) -> Result<()> {
        self.delete("signed", &id.to_string())
    }
    async fn put_sender_key(&self, address: &str, record: &[u8]) -> Result<()> {
        self.save_bytes("sender_key", address, record)
    }
    async fn get_sender_key(&self, address: &str) -> Result<Option<Vec<u8>>> {
        self.get("sender_key", address)
    }
    async fn delete_sender_key(&self, address: &str) -> Result<()> {
        self.delete("sender_key", address)
    }
}
#[async_trait]
impl MsgSecretStore for Store {
    async fn put_msg_secrets(&self, entries: Vec<MsgSecretEntry>) -> Result<usize> {
        self.edit(|t| {
            let count = entries.len();
            for mut e in entries {
                let id = key(&[&e.chat, &e.sender, &e.msg_id]);
                if let Some(prev) = t.get(("msg_secret", id.as_str())).map_err(error)? {
                    let p: MsgSecretEntry = decode(prev.value())?;
                    e.expires_at = merge_msg_secret_expiry(p.expires_at, e.expires_at);
                    e.message_ts = merge_msg_secret_message_ts(p.message_ts, e.message_ts);
                }
                t.insert(("msg_secret", id.as_str()), encode(&e)?.as_slice())
                    .map_err(error)?;
            }
            Ok(count)
        })
    }
    async fn get_msg_secret(&self, chat: &str, sender: &str, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .get_msg_secret_with_ts(chat, sender, id)
            .await?
            .map(|p| p.0))
    }
    async fn get_msg_secret_with_ts(
        &self,
        chat: &str,
        sender: &str,
        id: &str,
    ) -> Result<Option<(Vec<u8>, i64)>> {
        Ok(self
            .object::<MsgSecretEntry>("msg_secret", &key(&[chat, sender, id]))?
            .map(|e| (e.secret.to_vec(), e.message_ts)))
    }
    async fn delete_expired_msg_secrets(&self, cutoff: i64) -> Result<u32> {
        self.prune("msg_secret", |_, v| {
            let e: MsgSecretEntry = decode(v)?;
            Ok(e.expires_at != 0 && e.expires_at <= cutoff)
        })
    }
}
