use super::Plan;
use std::{ffi::OsString, fs, io, path::Path, process::Command};

pub(super) fn plan() -> Result<Plan, String> {
    Ok(Plan {
        program: std::env::current_exe().map_err(|e| e.to_string())?,
    })
}

impl Plan {
    pub fn launch(self) -> Result<(), String> {
        Command::new(self.program)
            .arg("--uninstall-linux")
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

fn uninstall() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let data = crate::deployment::data_home()?;
    let owns = |query: &str, args: &[&str]| -> Result<bool, String> {
        if !Path::new(query).is_file() {
            return Ok(false);
        }
        let owner = Command::new(query)
            .args(args)
            .arg(&exe)
            .output()
            .map_err(|e| e.to_string())?;
        Ok(owner.status.success()
            && String::from_utf8_lossy(&owner.stdout)
                .trim()
                .split(':')
                .next()
                == Some("aimanity"))
    };
    let (manager, remove) = if owns("/usr/bin/dpkg-query", &["--search"])? {
        ("/usr/bin/dpkg", "--purge")
    } else if owns("/usr/bin/rpm", &["-qf", "--queryformat", "%{NAME}"])? {
        ("/usr/bin/rpm", "-e")
    } else {
        return Err(
            "The running application does not belong to the installed AImanity package.".into(),
        );
    };
    let status = Command::new("/usr/bin/pkexec")
        .args([manager, remove, "aimanity"])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("{manager}: {status}"));
    }
    match fs::remove_dir_all(data) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result.map_err(|e| e.to_string()),
    }
}

pub fn run(args: &[OsString]) -> Option<i32> {
    if args != [OsString::from("--uninstall-linux")] {
        return None;
    }
    Some(match uninstall() {
        Ok(()) => 0,
        Err(error) => {
            rfd::MessageDialog::new()
                .set_title("AImanity")
                .set_description(&error)
                .show();
            eprintln!("{error}");
            1
        }
    })
}
