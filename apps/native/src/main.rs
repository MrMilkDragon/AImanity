#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(target_os = "android"))]
include!("workbench.rs");

#[cfg(target_os = "android")]
fn main() {}

#[cfg(not(target_os = "android"))]
fn main() -> iced::Result {
    run_workbench()
}
