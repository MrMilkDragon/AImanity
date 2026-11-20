use super::Resources;
use crate::{Error, Installation, Result};
use ai_phone_api::wire::NativeAndroidPrepare;
use prost::Message;
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
};

pub const CONTRACT: &str = "ai-phone-native-android-v1";
fn read_request() -> Result<NativeAndroidPrepare> {
    let mut bytes = Vec::new();
    std::io::stdin().take(8193).read_to_end(&mut bytes)?;
    if bytes.len() > 8192 {
        return Err(Error::Invalid("android_prepare_input"));
    }
    let request = NativeAndroidPrepare::decode(bytes.as_slice())
        .map_err(|_| Error::Invalid("android_prepare_input"))?;
    if request.version != 1 {
        return Err(Error::Invalid("android_prepare_input"));
    }
    Ok(request)
}
pub fn run(arguments: &[OsString]) -> Result<()> {
    let (home, request, factory) = match arguments {
        [home] => (home, read_request()?, false),
        [home, payload, identity] => (
            home,
            NativeAndroidPrepare {
                version: 1,
                payload_path: payload
                    .to_str()
                    .ok_or(Error::Invalid("android_prepare_arguments"))?
                    .into(),
                payload_sha256: identity
                    .to_str()
                    .ok_or(Error::Invalid("android_prepare_arguments"))?
                    .into(),
                cpus: 2,
                memory_mb: 2048,
                storage_gb: 16,
            },
            true,
        ),
        _ => return Err(Error::Invalid("android_prepare_arguments")),
    };
    let resources = Resources {
        cpus: request.cpus,
        memory_mb: request.memory_mb,
        storage_gb: request.storage_gb,
    };
    resources.validate()?;
    let prepared = Installation::lock(Path::new(home))?.prepare_android(
        Path::new(&request.payload_path),
        &request.payload_sha256,
        resources,
        &|path| fs2::available_space(path).unwrap_or(0),
        &|| false,
    )?;
    let receipt = prepared.wire()?;
    if factory {
        println!(
            "{}",
            serde_json::json!({
                "version": receipt.version,
                "sdkRoot": receipt.sdk_root,
                "avdHome": receipt.avd_home,
                "avdName": receipt.avd_name,
                "reusedSdk": receipt.reused_sdk,
                "reusedDevice": receipt.reused_device,
            })
        );
    } else {
        std::io::stdout().write_all(&receipt.encode_to_vec())?;
    }
    Ok(())
}
