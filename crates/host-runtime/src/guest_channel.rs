#[cfg(any(target_os = "macos", target_os = "linux", test))]
use crate::Error;
#[cfg(any(target_os = "macos", target_os = "linux", windows, test))]
use crate::Result;
#[cfg(any(
    target_os = "macos",
    windows,
    all(target_os = "linux", target_arch = "x86_64")
))]
use std::path::Path;
#[cfg(any(target_os = "macos", target_os = "linux", test))]
use std::process::Stdio;
#[cfg(any(target_os = "macos", target_os = "linux", test))]
use tokio::process::Command;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    process::Child,
};

#[derive(Clone, Copy)]
pub enum Service {
    Control,
    FilesAndAudio,
}

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
pub type Channel = (Box<dyn Stream>, Option<Child>);

#[cfg(any(target_os = "macos", target_os = "linux", test))]
pub(crate) fn command(mut cmd: Command, service: Service) -> Result<Channel> {
    let arg = match service {
        Service::Control => "workspace-control",
        Service::FilesAndAudio => "workspace-io",
    };
    let mut child = cmd
        .arg(arg)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let read = child
        .stdout
        .take()
        .ok_or(Error::Invalid("guest_channel_output"))?;
    let write = child
        .stdin
        .take()
        .ok_or(Error::Invalid("guest_channel_input"))?;
    Ok((Box::new(tokio::io::join(read, write)), Some(child)))
}

#[cfg(target_os = "macos")]
pub async fn open(home: &Path, service: Service) -> Result<Option<Channel>> {
    crate::managed_engine::desktop_command(home)?
        .map(|cmd| command(cmd, service))
        .transpose()
}

#[cfg(any(windows, all(target_os = "linux", target_arch = "x86_64")))]
pub async fn open(home: &Path, service: Service) -> Result<Option<Channel>> {
    use ai_phone_api::wire::LinuxVmOperation;
    let vm = home.join("user/qemu");
    let Some(port) = crate::qemu_host::running_control(&vm)? else {
        return Ok(None);
    };
    let op = match service {
        Service::Control => LinuxVmOperation::DesktopSession,
        Service::FilesAndAudio => LinuxVmOperation::DesktopIo,
    };
    let (stream, _) =
        crate::vm_control::open(port, &crate::qemu_host::key_path(&vm), op, Vec::new()).await?;
    Ok(Some((Box::new(stream), None)))
}
