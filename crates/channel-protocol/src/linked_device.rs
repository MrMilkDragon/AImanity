use async_trait::async_trait;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Serialize, de::DeserializeOwned};
use std::{path::Path, sync::Arc};
use whatsapp_rust::wacore::store::{
    Device,
    device::DEVICE_PROPS,
    error::{Result, StoreError},
    traits::DeviceStore,
};
mod protocol;
mod signal;
mod sync;

const DATA: TableDefinition<(&str, &str), &[u8]> = TableDefinition::new("linked_device");
type Table<'a> = redb::Table<'a, (&'static str, &'static str), &'static [u8]>;
pub(super) struct Store(Database);
fn error(e: impl std::error::Error + Send + Sync + 'static) -> StoreError {
    StoreError::Database(Box::new(e))
}
fn encode<T: Serialize + ?Sized>(x: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(x).map_err(|e| StoreError::Serialization(Box::new(e)))
}
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|e| StoreError::Serialization(Box::new(e)))
}
fn key(parts: &[&str]) -> String {
    serde_json::to_string(parts).unwrap()
}
impl Store {
    pub(super) fn open(dir: &Path, binding: uuid::Uuid) -> super::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let mut options = std::fs::OpenOptions::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(format!("{binding}.redb")))?;
        let db = Database::builder()
            .set_cache_size(2 * 1024 * 1024)
            .create_with_backend(redb::backends::FileBackend::new(file))?;
        let store = Self(db);
        store.edit(|_| Ok(()))?;
        Ok(store)
    }
    fn edit<T>(&self, f: impl FnOnce(&mut Table<'_>) -> Result<T>) -> Result<T> {
        let tx = self.0.begin_write().map_err(error)?;
        let value = {
            let mut t = tx.open_table(DATA).map_err(error)?;
            f(&mut t)?
        };
        tx.commit().map_err(error)?;
        Ok(value)
    }
    fn get(&self, group: &str, id: &str) -> Result<Option<Vec<u8>>> {
        let t = self
            .0
            .begin_read()
            .map_err(error)?
            .open_table(DATA)
            .map_err(error)?;
        Ok(t.get((group, id))
            .map_err(error)?
            .map(|v| v.value().to_owned()))
    }
    fn object<T: DeserializeOwned>(&self, group: &str, id: &str) -> Result<Option<T>> {
        self.get(group, id)?.as_deref().map(decode).transpose()
    }
    fn save_bytes(&self, g: &str, id: &str, v: &[u8]) -> Result<()> {
        self.edit(|t| {
            t.insert((g, id), v).map_err(error)?;
            Ok(())
        })
    }
    fn delete(&self, g: &str, id: &str) -> Result<()> {
        self.edit(|t| {
            t.remove((g, id)).map_err(error)?;
            Ok(())
        })
    }
    fn items(&self, g: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let table = self
            .0
            .begin_read()
            .map_err(error)?
            .open_table(DATA)
            .map_err(error)?;
        let mut rows = Vec::new();
        for r in table.range((g, "")..).map_err(error)? {
            let (k, v) = r.map_err(error)?;
            let (group, id) = k.value();
            if group != g {
                break;
            }
            rows.push((id.to_owned(), v.value().to_vec()));
        }
        Ok(rows)
    }
    fn prune(&self, group: &str, mut del: impl FnMut(&str, &[u8]) -> Result<bool>) -> Result<u32> {
        self.edit(|t| {
            let mut ids = Vec::new();
            for row in t.range((group, "")..).map_err(error)? {
                let (k, v) = row.map_err(error)?;
                let (g, id) = k.value();
                if g != group {
                    break;
                }
                if del(id, v.value())? {
                    ids.push(id.to_owned());
                }
            }
            for id in &ids {
                t.remove((group, id.as_str())).map_err(error)?;
            }
            Ok(ids.len() as u32)
        })
    }
}
#[async_trait]
impl DeviceStore for Store {
    async fn save(&self, device: &Device) -> Result<()> {
        self.save_bytes("device", "one", &encode(device)?)
    }
    async fn load(&self) -> Result<Option<Device>> {
        let mut v: Option<Device> = self.object("device", "one")?;
        if let Some(d) = v.as_mut() {
            d.device_props = Arc::new(DEVICE_PROPS.clone());
        }
        Ok(v)
    }
    async fn exists(&self) -> Result<bool> {
        Ok(self.get("device", "one")?.is_some())
    }
    async fn create(&self) -> Result<i32> {
        self.edit(|t| {
            if t.get(("device", "one")).map_err(error)?.is_none() {
                t.insert(("device", "one"), encode(&Device::new())?.as_slice())
                    .map_err(error)?;
            }
            Ok(1)
        })
    }
}
