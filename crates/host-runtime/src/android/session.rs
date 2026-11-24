//! Fixed native SDK lifecycle, independent of device focus and model execution.
mod adb;
#[cfg(target_os = "macos")]
mod avo;
mod cli;
#[cfg(any(windows, all(target_os = "linux", target_arch = "x86_64")))]
mod cuttlefish;
mod link;
mod plan;
mod process;
mod readiness;
#[cfg(target_os = "macos")]
pub use avo::runtime as avo_runtime;
pub use cli::run;
use std::time::Duration;
pub const CONTRACT: &str = "ai-phone-native-android-session-v1";

pub fn attached_client(home: &std::path::Path) -> crate::Result<Option<ai_phone_adb::Client>> {
    use crate::{Error, filesystem};
    let dir = home.join("user/data/android");
    let native = dir.join("native");
    let lock = native.join("session.lock");
    if !lock.try_exists()? || filesystem::lease(&lock)?.is_some() {
        return Ok(None);
    }
    let owner = process::session_record(&native.join("session.pb"))?
        .ok_or(Error::Invalid("android_session_owner"))?;
    let sdk = std::path::Path::new(&owner.sdk_root);
    let engine = super::Engine::installed(sdk);
    if owner.version != 1
        || !sdk.starts_with(home.join("components/android"))
        || std::path::Path::new(&owner.avd_home) != dir.join(engine.avd_directory())
    {
        return Err(Error::Invalid("android_session_owner"));
    }
    let port =
        u16::try_from(owner.adb_port).map_err(|_| Error::Invalid("android_session_ports"))?;
    let console =
        u16::try_from(owner.console_port).map_err(|_| Error::Invalid("android_session_ports"))?;
    ai_phone_adb::Client::new(port, engine.serial(console))
        .map(Some)
        .map_err(|_| Error::Invalid("android_session_ports"))
}
#[cfg(any(windows, all(target_os = "linux", target_arch = "x86_64")))]
pub async fn stop_owned(home: &std::path::Path) -> crate::Result<()> {
    use crate::{Error, filesystem};
    let Some(adb) = attached_client(home)? else {
        return Ok(());
    };
    let _shutdown = adb
        .open_stream("shell:sync && reboot -p", Duration::from_secs(5))
        .await
        .map_err(|_| Error::Invalid("android_poweroff_request"))?;
    let lock = home.join("user/data/android/native/session.lock");
    tokio::time::timeout(Duration::from_secs(30), async {
        while filesystem::lease(&lock)?.is_none() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(())
    })
    .await
    .map_err(|_| Error::Invalid("android_emulator_stop_unknown"))?
}
#[cfg(any(
    target_os = "macos",
    windows,
    all(target_os = "linux", target_arch = "x86_64")
))]
pub(crate) async fn flush_attached(home: &std::path::Path) -> crate::Result<()> {
    if let Some(adb) = attached_client(home)? {
        adb::sync(&adb).await?;
    }
    Ok(())
}
pub(crate) async fn wait_serial(
    serial: String,
    observe_only: bool,
    stop: &mut tokio::sync::watch::Receiver<bool>,
) -> crate::Result<ai_phone_api::wire::NativeAndroidBooted> {
    let adb = adb::Adb {
        port: 15037,
        serial,
    };
    if observe_only {
        return readiness::existing(&adb, stop, Duration::from_secs(120)).await;
    }
    let until = tokio::time::Instant::now() + Duration::from_secs(120);
    let mut root_requested = false;
    loop {
        readiness::cancelled(stop)?;
        if let Some(value) = process::boot_probe(&adb, &mut root_requested).await? {
            return Ok(value);
        }
        if tokio::time::Instant::now() >= until {
            return Err(crate::Error::Invalid("android_session_unavailable"));
        }
        readiness::pause(stop).await?;
    }
}

#[cfg(any(test, target_os = "linux"))]
pub(crate) async fn backend_observation(
    serial: &str,
) -> crate::Result<ai_phone_api::wire::NativeAndroidBooted> {
    let (_owner, mut stop) = tokio::sync::watch::channel(false);
    readiness::existing(
        &adb::Adb {
            port: 15037,
            serial: serial.into(),
        },
        &mut stop,
        Duration::from_secs(10),
    )
    .await
}

#[cfg(test)]
mod tests;
