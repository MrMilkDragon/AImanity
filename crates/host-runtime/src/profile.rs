//! Installed device profiles and explicit resource changes.
use crate::{Error, Result, filesystem};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{ffi::OsString, fs, io::Read, path::Path};

pub(crate) const CONTRACT: &str = "ai-phone-host-profile-v1";
pub const DESKTOP_STORAGE_GB: [u32; 3] = [16, 32, 64];
const LOCALES: [&str; 10] = [
    "zh-CN", "en", "zh-TW", "ja", "ko", "de", "fr", "es", "ru", "ar",
];

pub fn desktop_host_memory_mib(desktop_mib: u32) -> u32 {
    (desktop_mib.div_ceil(1024) + 2).max(4) * 1024
}
pub(crate) fn checked_launch_path(value: Option<&str>) -> Result<&str> {
    value
        .filter(|path| !path.trim().is_empty() && !path.chars().any(char::is_control))
        .ok_or(Error::Invalid("host_profile_launch_path"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Phone {
    pub(crate) cpus: u32,
    pub(crate) memory_mb: u32,
    pub(crate) storage_gb: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Desktop {
    pub(crate) cpus: u32,
    pub(crate) memory_mb: u32,
    pub(crate) storage_gb: Option<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    #[serde(default)]
    pub(crate) reconfigure: bool,
    locale: String,
    pub(crate) phone: Phone,
    pub(crate) desktop: Desktop,
    #[cfg(any(all(test, unix), target_os = "linux"))]
    #[serde(default)]
    pub(crate) android_qemu: bool,
    #[cfg(not(any(all(test, unix), target_os = "linux")))]
    #[serde(default, rename = "android_qemu")]
    _android_qemu: bool,
}

#[cfg(unix)]
pub(crate) fn phone_resources(
    root: &Path,
    template: &str,
    request: &Request,
) -> Result<crate::android::Resources> {
    let path = root.join("data/deployment/profile.json");
    if !request.reconfigure && path.try_exists()? {
        let value: Value =
            serde_json::from_slice(&fs::read(path)?).map_err(|_| Error::Invalid("host_profile"))?;
        validate(&value, template)?;
        return Ok(crate::android::Resources {
            cpus: value["phone"]["cpus"].as_u64().unwrap() as u32,
            memory_mb: value["phone"]["memory_mb"].as_u64().unwrap() as u32,
            storage_gb: value["phone"]["storage_gb"].as_u64().unwrap() as u32,
        });
    }
    Ok(crate::android::Resources {
        cpus: request.phone.cpus,
        memory_mb: request.phone.memory_mb,
        storage_gb: request.phone.storage_gb,
    })
}

fn resources(phone: &Phone, desktop: &Desktop) -> bool {
    [phone.cpus, desktop.cpus]
        .iter()
        .all(|cpus| [1, 2, 4].contains(cpus))
        && [1536, 2048, 3072, 4096].contains(&phone.memory_mb)
        && [16, 24, 32].contains(&phone.storage_gb)
        && [2048, 2560, 3072, 4096].contains(&desktop.memory_mb)
        && desktop
            .storage_gb
            .is_none_or(|storage| DESKTOP_STORAGE_GB.contains(&storage))
}

fn absolute(text: &str) -> bool {
    Path::new(text).is_absolute() && !text.chars().any(|value| value < ' ')
}

fn proxy(text: &str) -> bool {
    text.is_empty()
        || url::Url::parse(text).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && matches!(url.path(), "" | "/")
                && url.query().is_none()
                && url.fragment().is_none()
        })
}

fn validate(value: &Value, template: &str) -> Result<()> {
    #[derive(Deserialize)]
    struct Profile {
        schema_version: u32,
        locale: String,
        sdk_root: String,
        host_proxy: String,
        container_proxy: String,
    }
    #[derive(Deserialize)]
    struct Device {
        template: String,
        cpus: u32,
        memory_mb: u32,
        storage_gb: Option<u32>,
    }
    let header: Profile =
        serde_json::from_value(value.clone()).map_err(|_| Error::Invalid("host_profile"))?;
    let phone: Device = serde_json::from_value(value["phone"].clone())
        .map_err(|_| Error::Invalid("host_profile"))?;
    let desktop: Device = serde_json::from_value(value["desktop"].clone())
        .map_err(|_| Error::Invalid("host_profile"))?;
    let storage = phone.storage_gb.ok_or(Error::Invalid("host_profile"))?;
    if header.schema_version != 1
        || !LOCALES.contains(&header.locale.as_str())
        || phone.template != template
        || !matches!(template, "android35-arm64" | "android35-x86_64")
        || desktop.template != "debian13-openbox"
        || !resources(
            &Phone {
                cpus: phone.cpus,
                memory_mb: phone.memory_mb,
                storage_gb: storage,
            },
            &Desktop {
                cpus: desktop.cpus,
                memory_mb: desktop.memory_mb,
                storage_gb: desktop.storage_gb,
            },
        )
        || !absolute(&header.sdk_root)
        || !proxy(&header.host_proxy)
        || !proxy(&header.container_proxy)
        || value
            .get("local_entry")
            .is_some_and(|entry| !entry.is_boolean())
    {
        return Err(Error::Invalid("host_profile"));
    }
    if let Some(path) = value.get("launch_path") {
        checked_launch_path(path.as_str())?;
    }
    Ok(())
}

pub(crate) fn prepare(
    root: &Path,
    sdk: &Path,
    template: &str,
    bytes: &[u8],
    launch_path: &str,
) -> Result<()> {
    prepare_request(root, sdk, template, parse_request(bytes)?, launch_path)
}

pub(crate) fn parse_request(bytes: &[u8]) -> Result<Request> {
    if bytes.len() > 8192 {
        return Err(Error::Invalid("host_profile_request"));
    }
    let request: Request =
        serde_json::from_slice(bytes).map_err(|_| Error::Invalid("host_profile_request"))?;
    if !LOCALES.contains(&request.locale.as_str()) || !resources(&request.phone, &request.desktop) {
        return Err(Error::Invalid("host_profile_request"));
    }
    Ok(request)
}

pub(crate) fn prepare_request(
    root: &Path,
    sdk: &Path,
    template: &str,
    request: Request,
    launch_path: &str,
) -> Result<()> {
    if !matches!(template, "android35-arm64" | "android35-x86_64") {
        return Err(Error::Invalid("host_profile_request"));
    }
    let destination = root.join("data/deployment/profile.json");
    if destination.exists() {
        let mut value: Value = serde_json::from_slice(&fs::read(destination)?)
            .map_err(|_| Error::Invalid("host_profile"))?;
        validate(&value, template)?;
        if !request.reconfigure {
            return Ok(());
        }
        if value["phone"]["storage_gb"] != request.phone.storage_gb
            || request.desktop.storage_gb.is_some_and(|gb| {
                u64::from(gb) < value["desktop"]["storage_gb"].as_u64().unwrap_or(64)
            })
        {
            return Err(Error::Invalid("installed_device_storage"));
        }
        value["phone"]["cpus"] = request.phone.cpus.into();
        value["phone"]["memory_mb"] = request.phone.memory_mb.into();
        value["desktop"]["cpus"] = request.desktop.cpus.into();
        value["desktop"]["memory_mb"] = request.desktop.memory_mb.into();
        if let Some(gb) = request.desktop.storage_gb {
            value["desktop"]["storage_gb"] = gb.into();
        }
        return persist(root, &value);
    }
    if !sdk.is_absolute()
        || sdk
            .as_os_str()
            .to_string_lossy()
            .chars()
            .any(|value| value < ' ')
    {
        return Err(Error::Invalid("host_profile"));
    }
    checked_launch_path(Some(launch_path))?;
    let mut value = json!({"schema_version":1,"local_entry":true,"locale":request.locale,
        "launch_path":launch_path,
        "sdk_root":sdk,
        "host_proxy":"","container_proxy":"",
        "phone":{"template":template,"cpus":request.phone.cpus,"memory_mb":request.phone.memory_mb,"storage_gb":request.phone.storage_gb},
        "desktop":{"template":"debian13-openbox","cpus":request.desktop.cpus,"memory_mb":request.desktop.memory_mb}});
    if let Some(storage) = request.desktop.storage_gb {
        value["desktop"]["storage_gb"] = json!(storage);
    }
    persist(root, &value)
}

fn persist(root: &Path, value: &Value) -> Result<()> {
    let destination = root.join("data/deployment/profile.json");
    let folder = destination.parent().ok_or(Error::Invalid("host_profile"))?;
    filesystem::directory(folder, true)?;
    filesystem::publish_private(
        folder,
        "profile.json",
        &serde_json::to_vec_pretty(value).map_err(|_| Error::Invalid("host_profile"))?,
        false,
    )
}

fn document() -> Result<Value> {
    let value: Value =
        serde_json::from_reader(std::io::stdin()).map_err(|_| Error::Invalid("host_profile"))?;
    validate_document(&value)?;
    Ok(value)
}

fn validate_document(value: &Value) -> Result<()> {
    let template = value
        .pointer("/phone/template")
        .and_then(Value::as_str)
        .ok_or(Error::Invalid("host_profile"))?;
    validate(value, template)
}

#[cfg(unix)]
pub(crate) fn save_user_profile(root: &Path, value: &Value) -> Result<()> {
    validate_document(value)?;
    persist(root, value)
}

pub(crate) fn check(arguments: &[OsString]) -> Result<()> {
    if !arguments.is_empty() {
        return Err(Error::Invalid("host_profile_arguments"));
    }
    document()?;
    println!("{{\"valid\":true}}");
    Ok(())
}

pub(crate) fn write(arguments: &[OsString]) -> Result<()> {
    let [root] = arguments else {
        return Err(Error::Invalid("host_profile_arguments"));
    };
    persist(Path::new(root), &document()?)?;
    println!("{{\"prepared\":true}}");
    Ok(())
}

pub(crate) fn run(arguments: &[OsString]) -> Result<()> {
    let [root, sdk, template] = arguments else {
        return Err(Error::Invalid("host_profile_arguments"));
    };
    let template = template
        .to_str()
        .ok_or(Error::Invalid("host_profile_arguments"))?;
    let mut bytes = Vec::new();
    std::io::stdin().take(8193).read_to_end(&mut bytes)?;
    if bytes.len() > 8192 {
        return Err(Error::Invalid("host_profile_request"));
    }
    prepare(
        Path::new(root),
        Path::new(sdk),
        template,
        &bytes,
        &std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".into()),
    )?;
    println!("{{\"prepared\":true}}");
    Ok(())
}

#[cfg(all(test, unix))]
mod resume_resources_tests {
    use super::*;
    #[test]
    fn desktop_host_has_room_for_the_container_and_system_services() {
        for desktop_mib in [2048, 2560, 3072, 4096] {
            assert!(desktop_host_memory_mib(desktop_mib) >= desktop_mib + 2048);
        }
    }
    fn request(cpus: u32, memory: u32) -> Request {
        parse_request(&serde_json::to_vec(&json!({"locale":"en","phone":{"cpus":cpus,"memory_mb":memory,"storage_gb":16},"desktop":{"cpus":2,"memory_mb":2048}})).unwrap()).unwrap()
    }
    #[test]
    fn first_run_defaults_keep_existing_phone_resources() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        prepare_request(
            root,
            Path::new("/owned/sdk"),
            "android35-arm64",
            request(4, 4096),
            "/usr/bin:/bin",
        )
        .unwrap();
        let before = fs::read(root.join("data/deployment/profile.json")).unwrap();
        let selected = phone_resources(root, "android35-arm64", &request(2, 2048)).unwrap();
        assert_eq!(
            (selected.cpus, selected.memory_mb, selected.storage_gb),
            (4, 4096, 16)
        );
        assert_eq!(
            fs::read(root.join("data/deployment/profile.json")).unwrap(),
            before
        );
    }
    #[test]
    fn invalid_saved_resources_are_rejected_instead_of_being_replaced_by_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        prepare_request(
            root,
            Path::new("/owned/sdk"),
            "android35-arm64",
            request(4, 4096),
            "/usr/bin:/bin",
        )
        .unwrap();
        let path = root.join("data/deployment/profile.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["phone"]["cpus"] = json!(9);
        let body = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &body).unwrap();
        assert!(phone_resources(root, "android35-arm64", &request(2, 2048)).is_err());
        assert_eq!(fs::read(path).unwrap(), body);
    }

    #[test]
    fn explicit_reconfiguration_changes_resources_and_keeps_the_installed_identity() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let install =
            |r| prepare_request(root, Path::new("/owned/sdk"), "android35-arm64", r, "/bin");
        let mut initial = request(2, 2048);
        initial.desktop.storage_gb = Some(16);
        install(initial).unwrap();
        let profile = || {
            serde_json::from_slice::<Value>(
                &fs::read(root.join("data/deployment/profile.json")).unwrap(),
            )
            .unwrap()
        };
        let before = profile();
        let mut next = request(4, 3072);
        next.reconfigure = true;
        next.desktop.memory_mb = 2560;
        next.desktop.storage_gb = Some(32);
        install(next).unwrap();
        let updated = profile();
        assert_eq!(updated["phone"]["cpus"], 4);
        assert_eq!(updated["desktop"]["memory_mb"], 2560);
        assert_eq!(updated["desktop"]["storage_gb"], 32);
        assert_eq!(updated["sdk_root"], before["sdk_root"]);
        let mut shrink = request(1, 1536);
        shrink.reconfigure = true;
        shrink.desktop.storage_gb = Some(16);
        assert!(install(shrink).is_err());
        assert_eq!(profile(), updated);
    }
}
