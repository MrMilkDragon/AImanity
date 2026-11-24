//! Commands for the installation-owned Linux desktop and workspace.
use crate::{Error, Result};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tokio::process::Command;

pub(crate) fn require_rootfs(root: &Path, expected: Option<&str>) -> Result<()> {
    let data = std::fs::read(root.join("runtime/services/manifest.json"))?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&data).map_err(|_| Error::Invalid("native_services_manifest"))?;
    let identity = manifest["linux_rootfs"]["sha256"]
        .as_str()
        .filter(|value| crate::manifest::identity(value))
        .ok_or(Error::Invalid("linux_rootfs_missing"))?;
    if expected.is_some_and(|expected| expected != identity) {
        return Err(Error::Invalid("linux_rootfs_identity"));
    }
    Ok(())
}

pub fn native_arguments(root: &Path, operation: &str, lima: Option<&str>) -> Result<Vec<String>> {
    let text = |path: &Path| {
        path.to_str()
            .map(str::to_owned)
            .ok_or(Error::Invalid("linux_runtime_path"))
    };
    let mut command = Vec::new();
    if let Some(lima) = lima {
        command.extend(
            [
                lima,
                "shell",
                "--tty=false",
                "--workdir",
                "/",
                "ai-phone",
                "sudo",
                "-n",
                "--",
            ]
            .map(str::to_owned),
        );
    }
    command.extend([
        text(&root.join("runtime/services/bin/desktop-bridge"))?,
        "native-linux".into(),
        text(root)?,
        operation.into(),
    ]);
    Ok(command)
}

pub(crate) async fn apply_location(
    mut command: Command,
    fix: &ai_phone_api::wire::LocationFix,
) -> Result<ai_phone_api::wire::LocationDevice> {
    use prost::Message;
    use std::{process::Stdio, time::Duration};
    use tokio::io::AsyncWriteExt;
    let mut child = command
        .arg("location")
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut input = child.stdin.take().ok_or(Error::Invalid("location_input"))?;
    input.write_all(&fix.encode_to_vec()).await?;
    drop(input);
    let output = tokio::time::timeout(Duration::from_secs(12), child.wait_with_output())
        .await
        .map_err(|_| Error::Invalid("location_timeout"))??;
    if !output.status.success() {
        return Err(Error::Invalid("location_guest_failed"));
    }
    ai_phone_api::wire::LocationDevice::decode(output.stdout.as_slice())
        .map_err(|_| Error::Invalid("location_receipt"))
}

pub fn native_command(root: &Path, operation: &str) -> Result<Command> {
    let lima = std::env::var("AI_PHONE_LIMA_COMMAND").ok();
    let args = native_arguments(root, operation, lima.as_deref())?;
    let mut command = Command::new(&args[0]);
    command.args(&args[1..]);
    Ok(command)
}

#[derive(serde::Deserialize, serde::Serialize)]
struct Desktop {
    cpus: u32,
    memory_mb: u32,
}

pub fn configure(root: &Path, cpus: u32, memory_mb: u32) -> Result<()> {
    let folder = root.join("data/deployment");
    let mut profile: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("profile.json"))?)
            .map_err(|_| Error::Invalid("desktop_resources"))?;
    profile["desktop"]["cpus"] = cpus.into();
    profile["desktop"]["memory_mb"] = memory_mb.into();
    crate::filesystem::publish_private(
        &folder,
        "profile.json",
        &serde_json::to_vec(&profile).map_err(|_| Error::Invalid("desktop_resources"))?,
        false,
    )
}

pub fn desktop_arguments(
    root: &Path,
    lima: Option<&str>,
    proxy: &str,
) -> Result<Option<Vec<String>>> {
    let file = root.join("data/deployment/profile.json");
    let data = match std::fs::read(file) {
        Ok(data) => data,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    #[derive(serde::Deserialize)]
    struct Profile {
        desktop: Desktop,
    }
    let Profile { desktop } =
        serde_json::from_slice(&data).map_err(|_| Error::Invalid("desktop_resources"))?;
    let mut args = native_arguments(root, "desktop", lima)?;
    args.extend([
        desktop.cpus.to_string(),
        desktop.memory_mb.to_string(),
        proxy.into(),
    ]);
    Ok(Some(args))
}

pub fn operation(arguments: &[&str]) -> Result<Command> {
    let root =
        std::env::var_os("AI_PHONE_PROJECT_ROOT").ok_or(Error::Invalid("linux_runtime_path"))?;
    let mut command = native_command(&PathBuf::from(root), "exec")?;
    command.args(arguments);
    Ok(command)
}

pub fn administration(root: &Path, arguments: &[&str]) -> Result<Command> {
    let mut command = native_command(root, "admin")?;
    command.args(arguments).current_dir(root);
    Ok(command)
}
