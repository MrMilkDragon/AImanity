use crate::{Error, Installation, Result, android};
use ai_phone_api::wire::VirtualDeviceProfile;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tokio::sync::{oneshot, watch};

pub struct Instance {
    pub home: PathBuf,
    pub bundle: PathBuf,
    pub inventory: BTreeMap<String, String>,
    pub profile: VirtualDeviceProfile,
    pub slot: u16,
    pub locale: String,
}

impl Instance {
    pub(crate) fn identity(&self, name: &str) -> Result<&str> {
        self.inventory
            .get(name)
            .map(String::as_str)
            .ok_or(Error::Invalid("device_component_missing"))
    }

    pub(crate) fn desktop_profile(&self) -> serde_json::Value {
        serde_json::json!({
            "locale": self.locale,
            "desktop": {
                "cpus": self.profile.cpus,
                "memory_mb": self.profile.memory_mb,
                "storage_gb": self.profile.storage_gb,
            },
        })
    }

    pub async fn run(
        self,
        kind: &str,
        stop: watch::Receiver<bool>,
        ready: oneshot::Sender<()>,
    ) -> Result<()> {
        if kind == "phone" && self.slot >= 65 {
            return Err(Error::Invalid("device_ports_exhausted"));
        }
        if kind == "phone" {
            let mut install = Installation::lock(&self.home)?;
            let identity = self.identity("android.payload")?;
            let resources = android::Resources {
                cpus: self.profile.cpus,
                memory_mb: self.profile.memory_mb,
                storage_gb: self.profile.storage_gb,
            };
            let phone = match install.resume_android(identity)? {
                Some(prepared) => {
                    prepared.configure(&resources)?;
                    prepared
                }
                None => install.prepare_android(
                    &self.bundle.join("android.payload"),
                    identity,
                    resources,
                    &|p| fs2::available_space(p).unwrap_or(0),
                    &|| *stop.borrow(),
                )?,
            };
            let root = self.home.join("user");
            phone_apps(&root, &self.bundle, self.identity("source.payload")?, &stop)?;
            drop(install);
            return android::session::workspace(phone, &root, self.slot, stop, ready).await;
        }
        if kind != "desktop" {
            return Err(Error::Invalid("device_kind"));
        }
        #[cfg(target_os = "macos")]
        return crate::unix_initializer::workspace_desktop(&self, stop, ready).await;
        #[cfg(any(windows, all(target_os = "linux", target_arch = "x86_64")))]
        {
            let bundle = self.bundle.join("qemu");
            let home = self.home.join("user/qemu");
            crate::qemu_native::verify_manifest(&self.bundle, self.identity("qemu/manifest.json")?)
                .map_err(|_| Error::Invalid("vm_bundle_invalid"))?;
            let profile = serde_json::to_vec(&self.desktop_profile())
                .map_err(|_| Error::Invalid("host_profile"))?;
            crate::filesystem::directory(&home, true)?;
            let port = crate::qemu_host::running_control(&home)?.unwrap_or(19061 + self.slot);
            let result = crate::qemu_host::ensure(
                &bundle,
                &home,
                self.profile
                    .cpus
                    .try_into()
                    .map_err(|_| Error::Invalid("invalid_device_resources"))?,
                crate::desktop_host_memory_mib(self.profile.memory_mb),
                port,
                &profile,
                &|| *stop.borrow(),
            )
            .await;
            if result.is_ok() {
                let _ = ready.send(());
                let mut stop = stop;
                let _ = stop.wait_for(|end| *end).await;
            }
            crate::qemu_host::stop_desktop(&self.home).await.and(result)
        }
    }
}

fn phone_apps(
    root: &Path,
    bundle: &Path,
    identity: &str,
    stop: &watch::Receiver<bool>,
) -> Result<()> {
    let runtime = root.join("runtime");
    let dest = runtime.join("phone-apps");
    let stamp = dest.join("bundle-version");
    if fs::read_to_string(&stamp).is_ok_and(|s| s == identity) {
        return Ok(());
    }
    fs::create_dir_all(&runtime)?;
    let dir = tempfile::tempdir_in(&runtime)?;
    let mut archive = fs::File::open(bundle.join("source.payload"))?;
    let cancel = || *stop.borrow();
    let control = crate::control::Control {
        available: &|_| 0,
        cancelled: &cancel,
    };
    let mut count = 0;
    crate::archive::visit(&mut archive, &control, None, |path, _, source| {
        match path.strip_prefix("runtime/phone-apps/") {
            Some(name @ ("input.apk" | "browser.apk" | "home.apk")) => {
                std::io::copy(source, &mut fs::File::create_new(dir.path().join(name))?)?;
                count += 1;
            }
            _ => crate::archive::drain(source)?,
        }
        Ok(())
    })?;
    if count != 3 {
        return Err(Error::Invalid("phone_apps_missing"));
    }
    fs::create_dir_all(&dest)?;
    for name in ["input.apk", "browser.apk", "home.apk"] {
        fs::rename(dir.path().join(name), dest.join(name))?;
    }
    fs::write(stamp, identity)?;
    Ok(())
}
