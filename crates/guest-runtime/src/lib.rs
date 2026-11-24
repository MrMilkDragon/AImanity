//! Native Linux guest control. The assistant owns authorization and action intent.
pub mod accessibility;
pub mod actions;
pub mod audio;
pub mod cursor;
pub mod files;
pub mod home_identity;
pub mod imaging;
#[cfg(target_os = "linux")]
pub mod linux_runtime;
pub mod location;
pub mod media;
pub mod notifications;
mod paint;
pub mod process;
pub mod runtime;
pub mod screen;
pub mod settings;
pub mod workspace;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;
pub use runtime::{DesktopRuntime, Response};
pub use settings::Settings;
