//! Serial-bound installer input/notification capabilities, separate from model tools.
mod applications;
mod device;
mod policy;
mod state;
mod windows_migration;
use crate::{Error, Result, filesystem, native_ipc};
use ai_phone_api::wire::{NativeAndroidInputReady, NativeAndroidInputSetup};
use device::Device;
use state::{Stage, State};
use std::{ffi::OsString, path::Path, time::Duration};
use tokio::sync::watch;
const PACKAGE: &str = "dev.aiphone.input";
const VERSION: u64 = include!("../../../../apps/android-input/version-code.txt");
const IME: &str = "dev.aiphone.input/.UnicodeIme";
const ATTENTION: &str = "dev.aiphone.input/.PhoneAttention";
const ACCESSIBILITY_SERVICES: &str = "enabled_accessibility_services";
const NOTIFICATION: &str = "dev.aiphone.input/.PhoneNotifications";
pub const CONTRACT: &str = "ai-phone-input-setup-v2";

pub async fn run(arguments: &[OsString]) -> Result<()> {
    if !arguments.is_empty() {
        return Err(Error::Invalid("phone_input_arguments"));
    }
    let request: NativeAndroidInputSetup =
        native_ipc::read_bounded(&mut tokio::io::stdin(), 16384).await?;
    if request.version != 1 {
        return Err(Error::Invalid("phone_input_request"));
    }
    let (end, stop) = watch::channel(false);
    let signal = tokio::spawn(async move {
        let _ = crate::component::interrupt().await;
        let _ = end.send(true);
    });
    let result = setup(request, stop).await;
    signal.abort();
    let _ = signal.await;
    let changed = result?;
    native_ipc::write_bounded(
        &mut tokio::io::stdout(),
        &NativeAndroidInputReady {
            version: 1,
            changed,
        },
        512,
    )
    .await?;
    Ok(())
}
pub async fn setup(request: NativeAndroidInputSetup, stop: watch::Receiver<bool>) -> Result<bool> {
    setup_at(15037, request, stop).await
}
pub async fn setup_at(
    port: u16,
    request: NativeAndroidInputSetup,
    stop: watch::Receiver<bool>,
) -> Result<bool> {
    if request.version != 1 {
        return Err(Error::Invalid("phone_input_request"));
    }
    if *stop.borrow() {
        return Err(Error::Cancelled);
    }
    let client = ai_phone_adb::Client::new(port, request.serial.clone())
        .map_err(|_| Error::Invalid("phone_input_serial"))?;
    let state = State::lock(Path::new(&request.project_root), &request.serial)?;
    let pending = state.load()?;
    let mut adb = device::Adb { client, stop };
    let apps = if request.offline_apps {
        Some(applications::Applications::load(
            Path::new(&request.project_root),
            &state,
        )?)
    } else {
        None
    };
    let mut changed = configure(
        &state,
        &mut adb,
        pending,
        Path::new(&request.apk_path),
        request.observe_only,
    )
    .await?;
    if let Some(apps) = apps {
        changed |= apps
            .configure(&state, &mut adb, request.observe_only)
            .await?;
    }
    Ok(changed)
}
async fn package_version(adb: &mut impl Device) -> Result<Option<u64>> {
    let packages = adb
        .execute(
            &[
                "shell",
                "pm",
                "list",
                "packages",
                "--show-versioncode",
                "--user",
                "0",
                PACKAGE,
            ],
            10,
        )
        .await?;
    for row in packages.lines() {
        let (package, version) = row
            .split_once(" versionCode:")
            .ok_or(Error::Invalid("phone_input_observation"))?;
        if package
            .strip_prefix("package:")
            .ok_or(Error::Invalid("phone_input_observation"))?
            == PACKAGE
        {
            return version
                .parse()
                .map(Some)
                .map_err(|_| Error::Invalid("phone_input_observation"));
        }
    }
    Ok(None)
}
async fn setting(adb: &mut impl Device, key: &str) -> Result<String> {
    adb.execute(
        &["shell", "settings", "--user", "0", "get", "secure", key],
        10,
    )
    .await
}
async fn write_setting(adb: &mut impl Device, key: &str, value: &str) -> Result<()> {
    adb.execute(
        &[
            "shell", "settings", "--user", "0", "put", "secure", key, value,
        ],
        10,
    )
    .await
    .map(|_| ())
}
fn resource(path: &Path, maximum: u64) -> Result<&str> {
    let file = filesystem::regular(path)?;
    if file.metadata()?.len() == 0 || file.metadata()?.len() > maximum {
        return Err(Error::Invalid("phone_input_resource"));
    }
    path.to_str().ok_or(Error::Invalid("phone_input_resource"))
}
async fn registration(adb: &mut impl Device) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if adb.cancelled() {
            return Err(Error::Cancelled);
        }
        let value = tokio::time::timeout_at(
            deadline,
            adb.execute(&["shell", "ime", "list", "-a", "-s", "--user", "0"], 5),
        )
        .await
        .map_err(|_| Error::Invalid("phone_input_registration"))??;
        let rows = value.lines().collect::<Vec<_>>();
        if !rows.iter().all(|row| policy::component(row)) {
            return Err(Error::Invalid("phone_input_observation"));
        }
        if policy::contains(&rows, IME) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(Error::Invalid("phone_input_registration"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
async fn ime(adb: &mut impl Device, observe: bool) -> Result<bool> {
    let mut changed = false;
    let value = setting(adb, "enabled_input_methods").await?;
    if !policy::contains(&policy::components(&value, true)?, IME) {
        if observe {
            return Err(Error::Invalid("phone_input_not_ready"));
        }
        adb.execute(&["shell", "ime", "enable", "--user", "0", IME], 10)
            .await?;
        changed = true;
        if !policy::contains(
            &policy::components(&setting(adb, "enabled_input_methods").await?, true)?,
            IME,
        ) {
            return Err(Error::Invalid("phone_input_enable_unknown"));
        }
    }
    let value = setting(adb, "default_input_method").await?;
    let rows = policy::components(&value, false)?;
    if rows.len() > 1 {
        return Err(Error::Invalid("phone_input_observation"));
    }
    if !policy::contains(&rows, IME) {
        if observe {
            return Err(Error::Invalid("phone_input_not_ready"));
        }
        adb.execute(&["shell", "ime", "set", "--user", "0", IME], 10)
            .await?;
        changed = true;
        if !policy::contains(
            &policy::components(&setting(adb, "default_input_method").await?, false)?,
            IME,
        ) {
            return Err(Error::Invalid("phone_input_selection_unknown"));
        }
    }
    Ok(changed)
}
async fn enabled(adb: &mut impl Device, key: &str, observe: bool) -> Result<bool> {
    match setting(adb, key).await?.as_str() {
        "1" => Ok(false),
        "0" | "null" => {
            if observe {
                return Err(Error::Invalid("phone_input_not_ready"));
            }
            write_setting(adb, key, "1").await?;
            if setting(adb, key).await? != "1" {
                return Err(Error::Invalid("phone_input_setting_unknown"));
            }
            Ok(true)
        }
        _ => Err(Error::Invalid("phone_input_observation")),
    }
}
async fn service_access(adb: &mut impl Device, service: &str) -> Result<bool> {
    let response = adb
        .execute(
            &[
                "shell",
                "am",
                "broadcast",
                "--receiver-foreground",
                "--user",
                "0",
                "-a",
                "dev.aiphone.input.READ_ACCESS",
                "-n",
                "dev.aiphone.input/.UiTreeReceiver",
                "--es",
                "service",
                service,
            ],
            10,
        )
        .await?;
    let prefix = format!("Broadcast completed: result=-1, data=\"{service}:");
    match response
        .lines()
        .last()
        .and_then(|line| line.strip_prefix(&prefix))
    {
        Some("true\"") => Ok(true),
        Some("false\"") => Ok(false),
        _ => Err(Error::Invalid("phone_input_service_observation")),
    }
}
async fn attention_bound(adb: &mut impl Device) -> Result<bool> {
    let state = adb
        .execute(
            &[
                "shell", "dumpsys", "-t", "2", "activity", "services", ATTENTION,
            ],
            3,
        )
        .await?;
    Ok(state.lines().any(|line| {
        line.trim_start()
            .starts_with("requested=true received=true hasBound=true ")
    }))
}
async fn attention(adb: &mut impl Device, observe: bool) -> Result<bool> {
    let value = setting(adb, ACCESSIBILITY_SERVICES).await?;
    let listed = policy::contains(&policy::components(&value, false)?, ATTENTION);
    if observe {
        return if listed && attention_bound(adb).await? {
            Ok(false)
        } else {
            Err(Error::Invalid("phone_input_not_ready"))
        };
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut listed = listed;
        let mut changed = false;
        if !listed {
            // Android removes enabled components until its own service registry sees them.
            while !service_access(adb, "attention").await? {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            let value = setting(adb, ACCESSIBILITY_SERVICES).await?;
            let mut rows = policy::components(&value, false)?;
            listed = policy::contains(&rows, ATTENTION);
            if !listed {
                rows.push(ATTENTION);
                write_setting(adb, ACCESSIBILITY_SERVICES, &rows.join(":")).await?;
                changed = true;
            }
        }
        loop {
            if !listed {
                let value = setting(adb, ACCESSIBILITY_SERVICES).await?;
                listed = policy::contains(&policy::components(&value, false)?, ATTENTION);
            }
            if listed && attention_bound(adb).await? {
                return Ok(changed);
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    })
    .await
    .map_err(|_| Error::Invalid("phone_input_accessibility_unknown"))?
}
async fn services(adb: &mut impl Device, observe: bool) -> Result<bool> {
    let mut changed = enabled(adb, "show_ime_with_hard_keyboard", observe).await?;
    if !service_access(adb, "notification").await? {
        if observe {
            return Err(Error::Invalid("phone_input_not_ready"));
        }
        adb.execute(
            &[
                "shell",
                "cmd",
                "notification",
                "allow_listener",
                NOTIFICATION,
                "0",
            ],
            10,
        )
        .await?;
        changed = true;
        if !service_access(adb, "notification").await? {
            return Err(Error::Invalid("phone_input_notification_unknown"));
        }
    }
    changed |= attention(adb, observe).await?;
    Ok(changed)
}
async fn configure(
    state: &State,
    adb: &mut impl Device,
    pending: Option<Stage>,
    apk: &Path,
    observe: bool,
) -> Result<bool> {
    let installed = package_version(adb).await?;
    let current = installed.is_some_and(|version| version >= VERSION);
    if adb
        .execute(&["shell", "am", "get-current-user"], 10)
        .await?
        != "0"
    {
        return Err(Error::Invalid("phone_input_user"));
    }
    let mut changed = false;
    let migrate = pending == Some(Stage::MigrateWindows)
        || (!observe && installed == Some(0) && windows_migration::matches(adb).await?);
    let restore_roles = migrate || pending == Some(Stage::RestoreWindowsRoles);
    if migrate {
        if observe {
            return Err(Error::Invalid("phone_input_not_ready"));
        }
        let apk = resource(apk, 16 * 1024 * 1024)?;
        state.save(Stage::MigrateWindows)?;
        windows_migration::transfer(adb, installed, apk).await?;
        state.save(Stage::RestoreWindowsRoles)?;
        changed = true;
    } else if !current {
        if observe {
            return Err(Error::Invalid("phone_input_not_ready"));
        }
        let apk = resource(apk, 16 * 1024 * 1024)?;
        state.save(if installed.is_some() {
            Stage::ReplaceUnknown
        } else {
            Stage::InstallUnknown
        })?;
        // The guest package version is the receipt. A later explicit repair may retry the same -r install.
        let installation = adb
            .execute(
                &["install", "--no-incremental", "-r", "--user", "0", apk],
                60,
            )
            .await;
        if package_version(adb)
            .await?
            .is_none_or(|version| version < VERSION)
        {
            installation?;
            return Err(Error::Invalid("phone_input_install_unknown"));
        }
        changed = true;
    }
    if !observe && !restore_roles && pending != Some(Stage::Configure) {
        state.save(Stage::Configure)?;
    }
    registration(adb).await?;
    changed |= ime(adb, observe).await?;
    changed |= services(adb, observe).await?;
    if !observe {
        if restore_roles {
            windows_migration::finish(adb).await?;
        }
        state.finish()?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests;
