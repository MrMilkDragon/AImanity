use super::*;
use whatsapp_rust::wacore::store::traits::{
    DeviceListRecord, LidPnMappingEntry, ProtocolStore, TcTokenEntry,
};
fn packed(bytes: &[u8]) -> Vec<u8> {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut p = t.to_le_bytes().to_vec();
    p.extend_from_slice(bytes);
    p
}
fn older(record: &[u8], cutoff: i64) -> Result<bool> {
    let ts = record
        .get(..8)
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| StoreError::Validation("message timestamp".into()))?;
    Ok(i64::from_le_bytes(ts) < cutoff)
}
impl Store {
    fn token_update(&self, jid: &str, f: impl FnOnce(&mut TcTokenEntry)) -> Result<()> {
        self.edit(|t| {
            let mut entry = t
                .get(("token", jid))
                .map_err(error)?
                .map(|b| decode(b.value()))
                .transpose()?
                .unwrap_or(TcTokenEntry {
                    token: Vec::new(),
                    token_timestamp: 0,
                    sender_timestamp: None,
                });
            f(&mut entry);
            t.insert(("token", jid), encode(&entry)?.as_slice())
                .map_err(error)?;
            Ok(())
        })
    }
}
#[async_trait]
impl ProtocolStore for Store {
    async fn get_sender_key_devices(&self, g: &str) -> Result<Vec<(String, bool)>> {
        let rows = self.items(&format!("group/{g}"))?;
        Ok(rows
            .into_iter()
            .map(|(s, b)| (s, b.first() == Some(&1)))
            .collect())
    }
    async fn set_sender_key_status(&self, g: &str, entries: &[(&str, bool)]) -> Result<()> {
        let group = format!("group/{g}");
        self.edit(|t| {
            for (id, yes) in entries {
                t.insert((group.as_str(), *id), &[u8::from(*yes)][..])
                    .map_err(error)?;
            }
            Ok(())
        })
    }
    async fn clear_sender_key_devices(&self, g: &str) -> Result<()> {
        self.prune(&format!("group/{g}"), |_, _| Ok(true))?;
        Ok(())
    }
    async fn delete_sender_key_device_rows(&self, jids: &[&str]) -> Result<()> {
        self.edit(|t| {
            t.retain(|(g, id), _| !g.starts_with("group/") || !jids.contains(&id))
                .map_err(error)?;
            Ok(())
        })
    }
    async fn clear_all_sender_key_devices(&self) -> Result<()> {
        self.edit(|t| {
            t.retain(|(g, _), _| !g.starts_with("group/"))
                .map_err(error)?;
            Ok(())
        })
    }
    async fn get_lid_mapping(&self, lid: &str) -> Result<Option<LidPnMappingEntry>> {
        self.object("lid", lid)
    }
    async fn get_pn_mapping(&self, phone: &str) -> Result<Option<LidPnMappingEntry>> {
        let Some(lid) = self.object::<String>("pn", phone)? else {
            return Ok(None);
        };
        self.get_lid_mapping(&lid).await
    }
    async fn put_lid_mapping(&self, e: &LidPnMappingEntry) -> Result<()> {
        self.put_lid_mappings(std::slice::from_ref(e)).await
    }
    async fn put_lid_mappings(&self, entries: &[LidPnMappingEntry]) -> Result<()> {
        self.edit(|t| {
            for e in entries {
                let previous = t
                    .get(("lid", e.lid.as_str()))
                    .map_err(error)?
                    .map(|b| decode::<LidPnMappingEntry>(b.value()))
                    .transpose()?;
                let mut entry = e.clone();
                if let Some(p) = previous {
                    entry.created_at = p.created_at;
                    if p.phone_number != e.phone_number {
                        t.remove(("pn", p.phone_number.as_str())).map_err(error)?;
                    }
                }
                t.insert(("lid", e.lid.as_str()), encode(&entry)?.as_slice())
                    .map_err(error)?;
                t.insert(("pn", e.phone_number.as_str()), encode(&e.lid)?.as_slice())
                    .map_err(error)?;
            }
            Ok(())
        })
    }
    async fn get_all_lid_mappings(&self) -> Result<Vec<LidPnMappingEntry>> {
        self.items("lid")?.iter().map(|(_, v)| decode(v)).collect()
    }
    async fn save_base_key(&self, a: &str, id: &str, b: &[u8]) -> Result<()> {
        self.save_bytes("base", &key(&[a, id]), b)
    }
    async fn has_same_base_key(&self, a: &str, id: &str, b: &[u8]) -> Result<bool> {
        Ok(self.get("base", &key(&[a, id]))?.as_deref() == Some(b))
    }
    async fn delete_base_key(&self, a: &str, id: &str) -> Result<()> {
        self.delete("base", &key(&[a, id]))
    }
    async fn update_device_list(&self, r: DeviceListRecord) -> Result<()> {
        self.save_bytes("devices", &r.user, &encode(&r)?)
    }
    async fn get_devices(&self, user: &str) -> Result<Option<DeviceListRecord>> {
        self.object("devices", user)
    }
    async fn delete_devices(&self, user: &str) -> Result<()> {
        self.delete("devices", user)
    }
    async fn get_group_metadata(&self, g: &str) -> Result<Option<Vec<u8>>> {
        self.get("metadata", g)
    }
    async fn put_group_metadata(&self, g: &str, b: &[u8]) -> Result<()> {
        self.save_bytes("metadata", g, b)
    }
    async fn delete_group_metadata(&self, g: &str) -> Result<()> {
        self.delete("metadata", g)
    }
    async fn get_tc_token(&self, j: &str) -> Result<Option<TcTokenEntry>> {
        self.object("token", j)
    }
    async fn put_tc_token(&self, j: &str, e: &TcTokenEntry) -> Result<()> {
        self.save_bytes("token", j, &encode(e)?)
    }
    async fn delete_tc_token(&self, j: &str) -> Result<()> {
        self.delete("token", j)
    }
    async fn get_all_tc_token_jids(&self) -> Result<Vec<String>> {
        Ok(self.items("token")?.into_iter().map(|(j, _)| j).collect())
    }
    async fn delete_expired_tc_tokens(&self, t: i64, s: i64) -> Result<u32> {
        self.prune("token", |_, v| {
            let e: TcTokenEntry = decode(v)?;
            Ok((e.token.is_empty() || e.token_timestamp < t)
                && e.sender_timestamp.is_none_or(|v| v < s))
        })
    }
    async fn touch_tc_token_sender_timestamp(&self, j: &str, ts: i64) -> Result<()> {
        self.token_update(j, |e| e.sender_timestamp = Some(ts))
    }
    async fn store_received_tc_token(&self, j: &str, b: &[u8], ts: i64) -> Result<()> {
        self.token_update(j, |e| {
            if e.token.is_empty() || ts >= e.token_timestamp {
                e.token = b.to_vec();
                e.token_timestamp = ts;
            }
        })
    }
    async fn store_sent_message(&self, chat: &str, id: &str, payload: &[u8]) -> Result<()> {
        self.save_bytes("sent", &key(&[chat, id]), &packed(payload))
    }
    async fn take_sent_message(&self, chat: &str, id: &str) -> Result<Option<Vec<u8>>> {
        self.edit(|t| {
            Ok(t.remove(("sent", key(&[chat, id]).as_str()))
                .map_err(error)?
                .map(|b| b.value()[8..].to_vec()))
        })
    }
    async fn delete_expired_sent_messages(&self, cutoff: i64) -> Result<u32> {
        self.prune("sent", |_, b| older(b, cutoff))
    }
    async fn store_pending_inbound(
        &self,
        chat: &str,
        sender: &str,
        id: &str,
        b: &[u8],
    ) -> Result<()> {
        self.save_bytes("in", &key(&[chat, sender, id]), &packed(b))
    }
    async fn get_pending_inbound(
        &self,
        chat: &str,
        sender: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>> {
        Ok(self
            .get("in", &key(&[chat, sender, id]))?
            .map(|b| b[8..].to_vec()))
    }
    async fn delete_pending_inbound(&self, chat: &str, sender: &str, id: &str) -> Result<()> {
        self.delete("in", &key(&[chat, sender, id]))
    }
    async fn delete_expired_pending_inbound(&self, t: i64) -> Result<u32> {
        self.prune("in", |_, b| older(b, t))
    }
}
