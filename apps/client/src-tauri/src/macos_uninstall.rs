use crate::{Exit, ExitPhase, updates};
use std::os::unix::process::CommandExt;
use std::{
    ffi::OsString,
    fs, io,
    path::Path,
    process::{self, Command, Stdio},
};
use tauri::{AppHandle, Manager};

pub(crate) const ID: &str = "uninstall-aimanity";
const HELPER: &str = "--uninstall-macos";
const BUNDLE_ID: &str = "dev.aiphone.client";
const ERROR_TITLE: &str = "AImanity uninstall failed";

pub(crate) fn select(app: &AppHandle) {
    let exit = app.state::<Exit>();
    let mut phase = exit.0.lock().unwrap();
    if !matches!(*phase, ExitPhase::Open) {
        return;
    }
    *phase = ExitPhase::Stopping;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = async {
            if !tauri::async_runtime::spawn_blocking(confirm)
                .await
                .map_err(|e| e.to_string())??
            {
                *app.state::<Exit>().0.lock().unwrap() = ExitPhase::Open;
                return Ok::<_, String>(());
            }
            let _update = updates::idle_guard(&app)?;
            let mut command = helper(&app)?;
            crate::bootstrap::stop_installing(&app).await?;
            command.spawn().map_err(|error| error.to_string())?;
            *app.state::<Exit>().0.lock().unwrap() = ExitPhase::Ready;
            app.exit(0);
            Ok(())
        }
        .await;
        if let Err(error) = result {
            *app.state::<Exit>().0.lock().unwrap() = ExitPhase::Open;
            updates::macos::alert(ERROR_TITLE, &error);
        }
    });
}

fn confirm() -> Result<bool, String> {
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", "display alert \"Uninstall AImanity?\" message \"This permanently deletes local chats, settings, and both virtual devices.\" as critical buttons {\"Cancel\", \"Uninstall\"} default button \"Cancel\" cancel button \"Cancel\""])
        .output()
        .map_err(|error| error.to_string())?;
    Ok(output.status.success() && output.stdout.trim_ascii() == b"button returned:Uninstall")
}

fn helper(app: &AppHandle) -> Result<Command, String> {
    let bundle = updates::macos::application(BUNDLE_ID)?;
    let parent = bundle.parent().ok_or("AImanity is not installed.")?;
    let _writable =
        tempfile::tempdir_in(parent).map_err(|_| "AImanity cannot be removed from this folder.")?;
    let user = app.path().home_dir().map_err(|error| error.to_string())?;
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    command
        .arg(HELPER)
        .arg(process::id().to_string())
        .arg(user)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    Ok(command)
}

fn preserve_backups(data: &Path) -> io::Result<()> {
    let backups = data.join("host/user/data/backups");
    match fs::symlink_metadata(&backups) {
        Ok(metadata) if metadata.is_dir() && fs::read_dir(&backups)?.next().is_some() => {
            let destination = tempfile::Builder::new()
                .prefix("AImanity Backups ")
                .tempdir_in(data.parent().unwrap())?;
            fs::rename(&backups, destination.path().join("backups"))?;
            let _ = destination.keep();
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn uninstall(user: &Path, bundle: &Path) -> Result<(), String> {
    let library = user.join("Library");
    let data = library.join("Application Support").join(BUNDLE_ID);
    if fs::symlink_metadata(&data).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("AImanity data directory is a symbolic link.".into());
    }
    tauri::async_runtime::block_on(ai_phone_host_runtime::remove_macos_devices(
        &data.join("host"),
        user,
    ))
    .map_err(|error| error.to_string())?;
    preserve_backups(&data).map_err(|error| error.to_string())?;
    for path in [
        data,
        library.join("Caches").join(BUNDLE_ID),
        library.join("WebKit").join(BUNDLE_ID),
        library.join("HTTPStorages").join(BUNDLE_ID),
        library.join("Logs").join(BUNDLE_ID),
        library
            .join("Preferences")
            .join(format!("{BUNDLE_ID}.plist")),
        library
            .join("Saved Application State")
            .join(format!("{BUNDLE_ID}.savedState")),
    ] {
        remove(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    remove(bundle).map_err(|error| error.to_string())
}

pub(crate) fn run(args: &[OsString]) -> Option<i32> {
    if args.first().is_none_or(|arg| arg != HELPER) {
        return None;
    }
    let result = (|| {
        let [_, pid, user] = args else {
            return Err("Invalid macOS uninstall arguments.".into());
        };
        let pid = pid
            .to_str()
            .and_then(|pid| pid.parse().ok())
            .ok_or("Invalid AImanity process.")?;
        let bundle = updates::macos::application(BUNDLE_ID)?;
        updates::macos::wait_for_parent(pid)?;
        uninstall(Path::new(user), &bundle)
    })();
    Some(match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("uninstall: {error}");
            updates::macos::alert(ERROR_TITLE, &error);
            1
        }
    })
}
