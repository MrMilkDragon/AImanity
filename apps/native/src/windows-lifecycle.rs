use std::{
    ffi::OsString,
    fs, io,
    os::windows::{
        ffi::OsStringExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::PathBuf,
    ptr::{null, null_mut},
};
use windows_sys::{
    Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
        Security::{TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY},
        Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT,
        System::{
            Com::CoTaskMemFree,
            Threading::{
                CreateMutexW, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
        UI::{
            Shell::{
                FOLDERID_LocalAppData, FOLDERID_RoamingAppData, KF_FLAG_DONT_VERIFY,
                SHGetKnownFolderPath,
            },
            WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId},
        },
    },
    core::GUID,
};

pub(super) fn open_guard() -> io::Result<OwnedHandle> {
    let name: Vec<u16> = format!("Local\\{}.running\0", crate::APP_ID)
        .encode_utf16()
        .collect();
    let mutex = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
    if mutex.is_null() {
        return Err(io::Error::last_os_error());
    }
    let already_open = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let mutex = unsafe { OwnedHandle::from_raw_handle(mutex) };
    if already_open {
        return Err(io::Error::from_raw_os_error(ERROR_ALREADY_EXISTS as i32));
    }
    Ok(mutex)
}

pub(super) fn roaming_host() -> Result<PathBuf, String> {
    Ok(app_directory(&FOLDERID_RoamingAppData, null_mut())?.join("host"))
}

fn desktop_user() -> io::Result<Option<OwnedHandle>> {
    let shell = unsafe { GetShellWindow() };
    if shell.is_null() {
        return Ok(None);
    }
    let mut pid = 0;
    if unsafe { GetWindowThreadProcessId(shell, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut token = null_mut();
    let access = TOKEN_QUERY | TOKEN_IMPERSONATE | TOKEN_DUPLICATE;
    if unsafe { OpenProcessToken(process.as_raw_handle(), access, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Some(unsafe { OwnedHandle::from_raw_handle(token) }))
}

fn app_directory(folder: &GUID, token: HANDLE) -> Result<PathBuf, String> {
    let mut raw = null_mut();
    let result =
        unsafe { SHGetKnownFolderPath(folder, KF_FLAG_DONT_VERIFY as u32, token, &mut raw) };
    if result < 0 {
        unsafe { CoTaskMemFree(raw.cast()) };
        return Err(format!(
            "Could not locate the Windows profile ({result:#x})."
        ));
    }
    let folder = unsafe {
        let length = (0..).find(|i| *raw.add(*i) == 0).unwrap();
        let path = OsString::from_wide(std::slice::from_raw_parts(raw, length));
        CoTaskMemFree(raw.cast());
        PathBuf::from(path)
    };
    Ok(folder.join(crate::APP_ID))
}

pub(super) fn remove() -> Result<(), String> {
    let _guard = open_guard().map_err(|error| error.to_string())?;
    // The elevated setup account can differ from the explorer session's user.
    let profile = desktop_user().map_err(|error| error.to_string())?;
    let token = profile
        .as_ref()
        .map_or(null_mut(), AsRawHandle::as_raw_handle);
    let roaming = app_directory(&FOLDERID_RoamingAppData, token)?;
    let local = app_directory(&FOLDERID_LocalAppData, token)?;
    let host = roaming.join("host");
    if [roaming.as_path(), host.as_path()].iter().any(|path| {
        fs::symlink_metadata(path)
            .is_ok_and(|info| info.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }) {
        return Err("AImanity’s device folder is a Windows reparse point.".into());
    }
    stop_host(&host)?;
    for folder in [local, roaming] {
        if let Err(error) = fs::remove_dir_all(&folder)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(format!("{}: {error}", folder.display()));
        }
    }
    Ok(())
}

pub(super) fn stop_host(home: &std::path::Path) -> Result<(), String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?
        .block_on(ai_phone_host_runtime::qemu_host::stop_installed(home))
        .map_err(|error| error.code().into())
}
