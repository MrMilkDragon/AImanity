use super::Plan;
use crate::{APP_ID, deployment};
use std::{
    ffi::OsString,
    fs, io,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
};

const HELPER: &str = "--uninstall-macos";
const ERROR_TITLE: &str = "AImanity uninstall failed";

impl Plan {
    pub fn launch(self) -> Result<(), String> {
        Command::new(self.program)
            .arg(HELPER)
            .arg(process::id().to_string())
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

fn application() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bundle = exe.ancestors().nth(3).ok_or("Install AImanity first.")?;
    if bundle.extension().is_none_or(|ext| ext != "app")
        || !exe.starts_with(bundle.join("Contents/MacOS"))
    {
        return Err("Install AImanity first.".into());
    }
    let info = Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
        .arg(bundle.join("Contents/Info.plist"))
        .output()
        .map_err(|e| e.to_string())?;
    if !info.status.success() || String::from_utf8_lossy(&info.stdout).trim() != APP_ID {
        return Err("The installed app has a different bundle identifier.".into());
    }
    Ok(bundle.to_owned())
}
fn wait_for_parent(pid: u32) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while unsafe { libc::getppid() } as u32 == pid {
        if std::time::Instant::now() >= deadline {
            return Err("AImanity did not quit.".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(())
}
fn user_home() -> Result<PathBuf, String> {
    let user = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("AImanity cannot locate its user profile.")?;
    if deployment::data_home()? != profile(&user).join("host") {
        return Err("Uninstall requires the standard AImanity data directory.".into());
    }
    Ok(user)
}

fn profile(user: &Path) -> PathBuf {
    user.join("Library/Application Support").join(APP_ID)
}

pub(super) fn plan() -> Result<Plan, String> {
    user_home()?;
    let bundle = application()?;
    let parent = bundle.parent().ok_or("AImanity is not installed.")?;
    tempfile::tempdir_in(parent).map_err(|_| "AImanity cannot be removed from this folder.")?;
    Ok(Plan {
        program: std::env::current_exe().map_err(|error| error.to_string())?,
    })
}

fn preserve_backups(data: &Path) -> io::Result<()> {
    let backups = data.join("host/user/data/backups");
    match fs::symlink_metadata(&backups) {
        Ok(metadata) if metadata.is_dir() && fs::read_dir(&backups)?.next().is_some() => {
            let destination = tempfile::Builder::new()
                .prefix("AImanity Backups ")
                .tempdir_in(data.parent().expect("AImanity profile parent"))?;
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
    let data = profile(user);
    if fs::symlink_metadata(&data).is_ok_and(|metadata| metadata.is_symlink()) {
        return Err("AImanity data directory is a symbolic link.".into());
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?
        .block_on(ai_phone_host_runtime::remove_macos_devices(
            &data.join("host"),
            user,
        ))
        .map_err(|error| error.to_string())?;
    preserve_backups(&data).map_err(|error| error.to_string())?;
    for path in [
        data,
        library.join("Caches").join(APP_ID),
        library.join("WebKit").join(APP_ID),
        library.join("HTTPStorages").join(APP_ID),
        library.join("Logs").join(APP_ID),
        library.join("Preferences").join(format!("{APP_ID}.plist")),
        library
            .join("Saved Application State")
            .join(format!("{APP_ID}.savedState")),
    ] {
        remove(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    remove(bundle).map_err(|error| error.to_string())
}

pub fn run(args: &[OsString]) -> Option<i32> {
    if args.first().is_none_or(|argument| argument != HELPER) {
        return None;
    }
    let result = (|| {
        let [_, pid] = args else {
            return Err("Invalid macOS uninstall arguments.".into());
        };
        let pid = pid
            .to_str()
            .and_then(|pid| pid.parse().ok())
            .ok_or("Invalid AImanity process.")?;
        let bundle = application()?;
        wait_for_parent(pid)?;
        uninstall(&user_home()?, &bundle)
    })();
    Some(match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("uninstall: {error}");
            let _ = rfd::MessageDialog::new()
                .set_title(ERROR_TITLE)
                .set_description(&error)
                .show();
            1
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_the_profile_preserves_its_backups_and_other_data() {
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path().join(APP_ID);
        let backups = data.join("host/user/data/backups");
        fs::create_dir_all(&backups).unwrap();
        fs::write(backups.join("snapshot.tar"), "backup").unwrap();
        let external = directory.path().join("other-project");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("device"), "preserved").unwrap();
        std::os::unix::fs::symlink(&external, data.join("external")).unwrap();

        preserve_backups(&data).unwrap();
        remove(&data).unwrap();

        assert!(!data.exists());
        assert_eq!(fs::read(external.join("device")).unwrap(), b"preserved");
        let backup = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .as_encoded_bytes()
                    .starts_with(b"AImanity Backups ")
            })
            .unwrap();
        assert_eq!(
            fs::read(backup.join("backups/snapshot.tar")).unwrap(),
            b"backup"
        );
    }
}
