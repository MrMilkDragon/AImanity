use crate::{Error, Result, filesystem};
use ai_phone_api::wire::DeviceBridgePeerOwner;
use prost::Message;
use std::path::Path;

pub const RUNTIME: &str = "/var/lib/aiphone/installation/user/data/android/user";

#[cfg(any(test, target_os = "linux"))]
pub(crate) async fn observe(
    expected: &DeviceBridgePeerOwner,
) -> Result<ai_phone_api::wire::NativeAndroidBooted> {
    let home = Path::new(RUNTIME);
    let actual = owner(home)?;
    if expected.version != 1 || expected != &actual {
        return Err(Error::Invalid("phone_peer_identity"));
    }
    let serial = advertised_serial(home, expected)?;
    let mut value = crate::android::session::backend_observation(&serial).await?;
    value.backend_paired = true;
    value.backend_bridge_id = expected.bridge_id.clone();
    Ok(value)
}
fn owner(home: &Path) -> Result<DeviceBridgePeerOwner> {
    DeviceBridgePeerOwner::decode(
        filesystem::bounded_file_with_limit(&home.join(".phone-peer.pb"), 512)?.as_slice(),
    )
    .map_err(|_| Error::Invalid("phone_peer_identity"))
}
fn advertised_serial(home: &Path, expected: &DeviceBridgePeerOwner) -> Result<String> {
    let directory = home.join("avd/running");
    filesystem::existing_directory(&directory)?;
    let rows = super::discovery::values(filesystem::bounded_file_with_limit(
        &directory.join("pid_0.ini"),
        16384,
    )?)?;
    match rows.get("aiphone.serial") {
        Some(serial)
            if serial == super::peer::CUTTLEFISH_SERIAL
                && rows.get("aiphone.bridge") == Some(&expected.bridge_id) =>
        {
            Ok(serial.clone())
        }
        None if super::discovery::advertised_token(
            &rows,
            5564,
            8558,
            Some(&expected.bridge_id),
        )?
        .is_some() =>
        {
            Ok("emulator-5564".into())
        }
        _ => Err(Error::Invalid("phone_peer_unavailable")),
    }
}

/// Returns the serial advertised by the expected bridge owner.
pub fn paired_phone_serial(bridge_id: &str) -> Result<String> {
    let home = Path::new(RUNTIME);
    let owner = owner(home)?;
    if owner.version != 1 || owner.bridge_id != bridge_id || owner.secret.len() != 32 {
        return Err(Error::Invalid("phone_peer_identity"));
    }
    advertised_serial(home, &owner)
}
