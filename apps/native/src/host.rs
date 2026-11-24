#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
mod lifecycle {
    include!("windows-lifecycle.rs");
}

#[cfg(windows)]
pub type InstanceGuard = std::os::windows::io::OwnedHandle;

#[cfg(windows)]
pub fn instance_guard() -> std::io::Result<InstanceGuard> {
    lifecycle::open_guard()
}

#[cfg(target_os = "linux")]
pub type InstanceGuard = std::os::unix::net::UnixListener;

#[cfg(target_os = "linux")]
pub fn instance_guard() -> std::io::Result<InstanceGuard> {
    use std::os::{linux::net::SocketAddrExt, unix::net::SocketAddr};
    InstanceGuard::bind_addr(&SocketAddr::from_abstract_name(crate::APP_ID)?)
}

#[cfg(windows)]
pub fn windows_data_home() -> Result<PathBuf, String> {
    lifecycle::roaming_host()
}

#[cfg(windows)]
pub fn windows_command(arguments: &[std::ffi::OsString]) -> Option<i32> {
    let [argument] = arguments else { return None };
    let result = match argument.to_str()? {
        "--uninstall-windows-user-data" => lifecycle::remove().map(|_| 0),
        "--stop-windows-backend" => crate::deployment::data_home()
            .and_then(|home| lifecycle::stop_host(&home))
            .map(|_| 0),
        "--prepare-windows-host" => prepare_windows_host(),
        _ => return None,
    };
    Some(match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    })
}

#[cfg(windows)]
fn prepare_windows_host() -> Result<i32, String> {
    use std::{
        os::windows::process::CommandExt,
        process::{Command, Stdio},
    };

    let resource = crate::deployment::Setup::resource_dir()?;
    crate::deployment::bundle_inventory(&resource)?;
    let system = PathBuf::from(std::env::var_os("SystemRoot").ok_or("windows_system_path")?)
        .join("System32");
    let code = Command::new(system.join("dism.exe"))
        .args([
            "/Online",
            "/Enable-Feature",
            "/FeatureName:HypervisorPlatform",
            "/All",
            "/NoRestart",
            "/Quiet",
        ])
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("Windows Hypervisor Platform: {error}"))?
        .code();
    match code {
        Some(value @ (0 | 3010)) => Ok(value),
        _ => Err(format!("Windows Hypervisor Platform: exit {code:?}")),
    }
}
