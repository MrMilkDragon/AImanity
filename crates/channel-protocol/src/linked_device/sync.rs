use super::*;
use whatsapp_rust::wacore::{
    appstate::{hash::HashState, processor::AppStateMutationMAC},
    store::traits::{AppStateSyncKey, AppSyncStore},
};
#[async_trait]
impl AppSyncStore for Store {
    async fn get_sync_key(&self, id: &[u8]) -> Result<Option<AppStateSyncKey>> {
        self.object("sync", &hex::encode(id))
    }
    async fn set_sync_key(&self, id: &[u8], v: AppStateSyncKey) -> Result<()> {
        self.edit(|t| {
            t.insert(("sync", hex::encode(id).as_str()), encode(&v)?.as_slice())
                .map_err(error)?;
            let newer = t
                .get(("meta", "latest"))
                .map_err(error)?
                .is_none_or(|current| id > current.value());
            if newer {
                t.insert(("meta", "latest"), id).map_err(error)?;
            }
            Ok(())
        })
    }
    async fn get_latest_sync_key_id(&self) -> Result<Option<Vec<u8>>> {
        self.get("meta", "latest")
    }
    async fn get_version(&self, name: &str) -> Result<HashState> {
        Ok(self.object("version", name)?.unwrap_or_default())
    }
    async fn set_version(&self, name: &str, v: HashState) -> Result<()> {
        self.save_bytes("version", name, &encode(&v)?)
    }
    async fn put_mutation_macs(
        &self,
        name: &str,
        _version: u64,
        values: &[AppStateMutationMAC],
    ) -> Result<()> {
        let group = format!("mac/{name}");
        self.edit(|t| {
            for v in values {
                t.insert(
                    (group.as_str(), hex::encode(&v.index_mac).as_str()),
                    v.value_mac.as_slice(),
                )
                .map_err(error)?;
            }
            Ok(())
        })
    }
    async fn get_mutation_mac(&self, name: &str, index: &[u8]) -> Result<Option<Vec<u8>>> {
        self.get(&format!("mac/{name}"), &hex::encode(index))
    }
    async fn delete_mutation_macs(&self, name: &str, indices: &[Vec<u8>]) -> Result<()> {
        let group = format!("mac/{name}");
        self.edit(|t| {
            for v in indices {
                t.remove((group.as_str(), hex::encode(v).as_str()))
                    .map_err(error)?;
            }
            Ok(())
        })
    }
    async fn clear_mutation_macs(&self, name: &str) -> Result<()> {
        self.prune(&format!("mac/{name}"), |_, _| Ok(true))?;
        Ok(())
    }
}
