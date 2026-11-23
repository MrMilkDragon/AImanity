use std::{io, path::Path};
use tokio::sync::watch;

// The phone's own Android desktop is outside the VM control channel.
const MOBILE: &str = "Host desktop control requires a desktop host";
pub enum Permission {
    Disabled,
    Off,
    Confirm,
    Armed,
    Active {
        request: String,
        _stop: watch::Sender<()>,
    },
}
impl Permission {
    pub fn load(_: &Path) -> Self {
        Self::Disabled
    }
    pub fn enabled(&self) -> bool {
        false
    }
    pub fn configure(&mut self, _: &Path, enabled: bool) -> io::Result<()> {
        if enabled {
            Err(io::Error::new(io::ErrorKind::Unsupported, MOBILE))
        } else {
            Ok(())
        }
    }
    pub fn end(&mut self, _: &str) -> bool {
        false
    }
    pub fn close(&mut self) {}
}
pub fn permission() -> Result<(), String> {
    Err(MOBILE.into())
}
pub(crate) struct Grant;
impl Grant {
    pub(crate) fn new(_: watch::Receiver<()>) -> Self {
        Self
    }
    pub(crate) fn active(&self) -> bool {
        false
    }
    pub(crate) fn perform(
        &self,
        _: &ai_phone_api::wire::DesktopActionRequest,
    ) -> Result<ai_phone_api::wire::HostDesktopReply, String> {
        Err(MOBILE.into())
    }
}
