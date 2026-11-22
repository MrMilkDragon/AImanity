#[cfg(target_os = "windows")]
pub(crate) fn total_memory_bytes() -> u64 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut state = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: the initialized structure has the length required by the Windows API.
    if unsafe { GlobalMemoryStatusEx(&mut state) } == 0 {
        0
    } else {
        state.ullTotalPhys
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn total_memory_bytes() -> u64 {
    let mut bytes = 0u64;
    let mut size = std::mem::size_of_val(&bytes);
    // hw.memsize is a read-only 64-bit value.
    let status = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut bytes as *mut u64).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status == 0 && size == std::mem::size_of_val(&bytes) {
        bytes
    } else {
        0
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn total_memory_bytes() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|info| {
            info.lines().find_map(|line| {
                line.strip_prefix("MemTotal:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
        })
        .unwrap_or(0)
        * 1024
}
