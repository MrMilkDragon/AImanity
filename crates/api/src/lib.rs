// Generated wire types are public so later API and runner consumers share the contract.
pub const LINUX_ROOTFS: &str = "/opt/aiphone/bundle/images.payload";
pub const LINUX_DESKTOP_TEMPLATE: &str = "debian13-openbox";

pub fn guest_system(kind: &str) -> Option<&'static str> {
    match kind {
        "desktop" => Some("Linux"),
        "phone" => Some("Android"),
        _ => None,
    }
}

pub fn platform_name(os: &str) -> Option<&'static str> {
    match os {
        "macos" => Some("macOS"),
        "windows" => Some("Windows"),
        "linux" => Some("Linux"),
        "android" => Some("Android"),
        _ => None,
    }
}

impl wire::RegisteredDevice {
    pub fn system_name(&self) -> &str {
        if self.system.is_empty() && !self.paired {
            return guest_system(&self.kind).unwrap_or("");
        }
        &self.system
    }
}

pub mod location_motion;
pub mod observations;
pub mod wire {
    include!(concat!(env!("OUT_DIR"), "/ai_phone.v1.rs"));
}
