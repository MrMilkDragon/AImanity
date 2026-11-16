//! Deployment of the offline Windows workbench on the existing owned QEMU disks.
use crate::{Installation, android, desktop_host_memory_mib, profile, qemu_host, vm_control};
use ai_phone_api::wire::NativeAndroidVmEndpoint;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

pub fn verify_manifest(resource: &Path, identity: &str) -> Result<(), String> {
    if !crate::manifest::identity(identity) {
        return Err("vm_bundle_invalid".into());
    }
    let bytes = fs::read(resource.join("qemu/manifest.json")).map_err(|e| e.to_string())?;
    if hex::encode(Sha256::digest(bytes)) != identity {
        return Err("vm_bundle_changed".into());
    }
    Ok(())
}

pub struct Deployment<'a> {
    pub resource: &'a Path,
    pub home: &'a Path,
    pub request: &'a Path,
    pub android: &'a str,
    pub vm_manifest: &'a str,
    pub resume: bool,
}

impl Deployment<'_> {
    pub async fn start(
        &self,
        cancel: &dyn Fn() -> bool,
        phase: impl Fn(&str),
    ) -> Result<(), String> {
        let error = |e: crate::Error| e.code().to_owned();
        verify_manifest(self.resource, self.vm_manifest)?;
        let bytes = fs::read(self.request).map_err(|e| e.to_string())?;
        let request = profile::parse_request(&bytes).map_err(error)?;
        let mut installation = Installation::lock(self.home).map_err(error)?;
        if self
            .home
            .join("wsl-owner.json")
            .try_exists()
            .map_err(|e| e.to_string())?
        {
            return Err("windows_backend_migration_required".into());
        }
        if cancel() {
            return Err("installation_cancelled".into());
        }
        phase("android");
        let cached = if self.resume {
            installation.resume_android(self.android).map_err(error)?
        } else {
            None
        };
        let phone = match cached {
            Some(prepared) => prepared,
            None => installation
                .prepare_android(
                    &self.resource.join("android.payload"),
                    self.android,
                    android::Resources {
                        cpus: request.phone.cpus,
                        memory_mb: request.phone.memory_mb,
                        storage_gb: request.phone.storage_gb,
                    },
                    &|path| fs2::available_space(path).unwrap_or(0),
                    cancel,
                )
                .map_err(error)?,
        };
        let mut guest_profile: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "invalid_device_resources")?;
        guest_profile["android_qemu"] = serde_json::Value::Bool(
            android::Engine::installed(&phone.sdk_root) == android::Engine::Whpx,
        );
        let guest_profile =
            serde_json::to_vec(&guest_profile).map_err(|_| "invalid_device_resources")?;
        let vm = self.home.join("user/qemu");
        phase("engine");
        qemu_host::ensure(
            &self.resource.join("qemu"),
            &vm,
            request
                .desktop
                .cpus
                .try_into()
                .map_err(|_| "invalid_device_resources")?,
            desktop_host_memory_mib(request.desktop.memory_mb),
            &guest_profile,
            cancel,
            |_| {},
        )
        .await
        .map_err(error)?;
        let key = qemu_host::key_path(&vm);
        phase("android");
        let boot = android::launch::start(
            &phone,
            "",
            Some(NativeAndroidVmEndpoint {
                port: vm_control::GUEST_PORT.into(),
                key_file: key.to_str().ok_or("vm_control_key_path")?.into(),
            }),
            cancel,
        )
        .await?;
        phase("devices");
        tokio::select! { biased;
            _ = android::launch::cancelled(cancel) => Err("installation_cancelled".into()),
            result = vm_control::complete_devices(vm_control::GUEST_PORT, &key, &boot.backend_bridge_id) =>
                result.map(|_| ()).map_err(error)
        }
    }
}
