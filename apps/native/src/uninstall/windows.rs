use super::Plan;
use std::{io, os::windows::ffi::OsStrExt, ptr::null_mut};
use windows_sys::{
    Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    core::w,
};

pub(super) fn plan() -> Result<Plan, String> {
    let program = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("unins000.exe");
    if !program.is_file() {
        return Err("AImanity uninstaller is missing".into());
    }
    Ok(Plan { program })
}

impl Plan {
    pub fn launch(self) -> Result<(), String> {
        let program: Vec<u16> = self.program.as_os_str().encode_wide().chain([0]).collect();
        let result = unsafe {
            ShellExecuteW(
                null_mut(),
                w!("runas"),
                program.as_ptr(),
                w!("/SILENT /NORESTART"),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if result as isize <= 32 {
            return Err(io::Error::last_os_error().to_string());
        }
        Ok(())
    }
}
