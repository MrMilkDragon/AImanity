//! The native phone worker holds the link even after the UI detaches.
use super::plan::Plan;
use crate::{
    Error, Result,
    device_bridge::{self, Connection},
    filesystem, vm_control,
};
use ai_phone_api::wire::{
    DeviceBridgeConfig, DeviceBridgePeerOwner, DeviceBridgePhone, DeviceBridgeRole,
    LinuxVmOperation, NativeAndroidBooted,
};
use prost::Message;
use std::{io::Write, time::Duration};
use tokio::sync::watch;

fn identity(plan: &Plan) -> Result<DeviceBridgePeerOwner> {
    let path = plan.home.join("phone-link.pb");
    if path.try_exists()? {
        return DeviceBridgePeerOwner::decode(
            filesystem::bounded_file_with_limit(&path, 512)?.as_slice(),
        )
        .map_err(|_| Error::Invalid("phone_link_identity"));
    }
    let mut random = [0u8; 48];
    getrandom::fill(&mut random).map_err(|_| Error::Invalid("phone_link_entropy"))?;
    let value = DeviceBridgePeerOwner {
        version: 1,
        bridge_id: random[..16].iter().map(|b| format!("{b:02x}")).collect(),
        secret: random[16..].to_vec(),
    };
    let mut output = tempfile::NamedTempFile::new_in(&plan.home)?;
    output.write_all(&value.encode_to_vec())?;
    output.as_file().sync_all()?;
    output
        .persist_noclobber(path)
        .map_err(|e| Error::Io(e.error))?;
    filesystem::sync_directory(&plan.home)?;
    Ok(value)
}
fn token(plan: &Plan) -> Result<String> {
    let mut result = None;
    for root in [
        plan.home.join("profile/local/Temp"),
        plan.home.join("profile/Library/Caches/TemporaryItems"),
        plan.home.join("android"),
    ] {
        if let Some(value) = device_bridge::discovery::token(
            &root.join("avd/running"),
            plan.ports.console,
            plan.ports.grpc,
            None,
        )? && result.replace(value).is_some()
        {
            return Err(Error::Invalid("emulator_discovery_invalid"));
        }
    }
    result.ok_or(Error::Invalid("emulator_auth_unavailable"))
}

pub(super) async fn observe(
    plan: &Plan,
    stop: &mut watch::Receiver<bool>,
) -> Result<NativeAndroidBooted> {
    observe_owner(plan, &identity(plan)?, stop).await
}
async fn observe_owner(
    plan: &Plan,
    owner: &DeviceBridgePeerOwner,
    stop: &mut watch::Receiver<bool>,
) -> Result<NativeAndroidBooted> {
    let backend = plan
        .backend
        .as_ref()
        .ok_or(Error::Invalid("phone_link_backend"))?;
    let request = vm_control::open(
        backend.port,
        &backend.key,
        LinuxVmOperation::PhoneStatus,
        owner.encode_to_vec(),
    );
    let (_, bytes) = tokio::select! { biased;
        _=stop.changed()=>Err(Error::Cancelled),
        result=tokio::time::timeout(Duration::from_secs(15),request)=>result.map_err(|_| Error::Invalid("phone_link_observation"))?,
    }?;
    let value = NativeAndroidBooted::decode(bytes.as_slice())
        .map_err(|_| Error::Invalid("phone_link_observation"))?;
    if value.version != 1
        || value.serial != plan.serial()
        || !value.root_verified
        || !value.backend_paired
        || value.backend_bridge_id != owner.bridge_id
        || ![value.width, value.height]
            .into_iter()
            .all(|n| (1..=8192).contains(&n))
    {
        return Err(Error::Invalid("phone_link_observation"));
    }
    Ok(value)
}
pub(super) async fn start(
    plan: &Plan,
    stop: &mut watch::Receiver<bool>,
) -> Result<(Connection, NativeAndroidBooted)> {
    let token = if plan.engine == crate::android::Engine::Cuttlefish {
        String::new()
    } else {
        token(plan)?
    };
    let backend = plan
        .backend
        .as_ref()
        .ok_or(Error::Invalid("phone_link_backend"))?;
    let owner = identity(plan)?;
    let request = DeviceBridgePhone {
        transport: Some(DeviceBridgeConfig {
            version: owner.version,
            role: DeviceBridgeRole::Guest as i32,
            bridge_id: owner.bridge_id.clone(),
            secret: owner.secret.clone(),
            endpoint: String::new(),
            adb_port: 0,
            emulator_port: 0,
        }),
        runtime_directory: device_bridge::guest::RUNTIME.into(),
        serial: plan.serial(),
        grpc_token: token,
        owner_pipe: true,
    };
    let mut connection = Connection::start(request, backend.port, &backend.key, stop).await?;
    match observe_owner(plan, &owner, stop).await {
        Ok(receipt) => Ok((connection, receipt)),
        Err(error) => {
            connection.shutdown().await?;
            Err(error)
        }
    }
}
