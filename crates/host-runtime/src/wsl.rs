//! Linux backend startup and device pairing.
#[cfg(unix)]
pub(crate) mod backend;
pub(crate) mod devices;

pub const DEVICE_SETUP_CONTRACT: &str = "ai-phone-device-setup-v2";
