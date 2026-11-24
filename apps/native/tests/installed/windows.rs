#[path = "guests.rs"]
mod guests;
#[path = "window.rs"]
mod window;

use ai_phone_host_runtime::{
    android::session::attached_client,
    guest_channel::{self, Service},
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};
use window::Client;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn onboarding(client: &mut Client, home: &Path) -> Result<()> {
    client.word("English", true).await?;
    client.word("Continue", true).await?;
    client.word("Message", false).await?;
    assert_eq!(fs::read_to_string(home.join("user/native-locale"))?, "en");
    assert!(home.join("workspace.redb").is_file());
    let devices = home.join("devices");
    assert!(!devices.exists() || devices.read_dir()?.next().is_none());
    client.capture("empty-workbench.png")?;
    Ok(())
}

#[tokio::test]
#[ignore = "opens the native workbench on a disposable Windows desktop"]
async fn windows_empty_workbench() -> Result<()> {
    let evidence = PathBuf::from(env::var("AIMANITY_QA_ROOT")?);
    fs::create_dir_all(&evidence)?;
    let root = tempfile::tempdir()?;
    let dir = root.path();
    let exe =
        PathBuf::from(env::var_os("AIMANITY_QA_CLIENT").ok_or("Missing workbench executable")?);
    let home = dir.join("dev.aimanity.workspace/host");
    let mut client = Client::launch(&exe, dir, &evidence, &evidence.join("fresh.log")).await?;
    onboarding(&mut client, &home).await?;
    client.close().await?;
    let mut reopened = Client::launch(&exe, dir, &evidence, &evidence.join("reopened.log")).await?;
    reopened.word("Message", false).await?;
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires the signed offline EXE and WHPX"]
async fn windows_installed_guests() -> Result<()> {
    let root = PathBuf::from(env::var("AIMANITY_QA_ROOT")?);
    assert_eq!(env::var("RUNNER_ENVIRONMENT")?, "github-hosted");
    let profile = root.join("profile");
    let home = profile.join("dev.aimanity.workspace/host");
    assert!(!home.exists());
    let exe = root.join("installed/ai-phone-client.exe");
    let log = root.join("native.log");
    let mut app = Client::launch(&exe, &profile, &root, &log).await?;
    let result = async {
        onboarding(&mut app, &home).await?;
        app.word("Add", true).await?;
        app.create_guest("Phone").await?;
        app.frame("phone", &log).await?;
        app.create_guest("Desktop").await?;
        app.frame("desktop", &log).await?;
        app.capture("installed-guests.png")?;
        let dirs = home
            .join("devices")
            .read_dir()?
            .map(|d| d.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        assert_eq!(dirs.len(), 2);
        let mut phone = None;
        let mut linux = None;
        for dir in dirs {
            if let Some(adb) = attached_client(&dir)? {
                phone = Some((adb, dir));
            } else if dir.join("user/qemu/control.key").is_file() {
                linux = Some(dir);
            }
        }
        let (adb, phone) = phone.ok_or("Owned Android session missing")?;
        let linux = linux.ok_or("Owned Linux session missing")?;
        tokio::try_join!(guests::android(adb), guests::desktop(&linux))?;
        Result::Ok((phone, linux))
    }
    .await;
    let closed = app.close().await;
    let (phone, linux) = result?;
    closed?;
    assert!(attached_client(&phone)?.is_none());
    assert!(
        guest_channel::open(&linux, Service::Control)
            .await?
            .is_none()
    );
    let log = root.join("reopen.log");
    let mut app = Client::launch(&exe, &profile, &root, &log).await?;
    let resumed = tokio::time::timeout(Duration::from_secs(1500), async {
        app.word("Message", false).await?;
        app.frame("phone", &log).await?;
        app.frame("desktop", &log).await
    })
    .await;
    let closed = app.close().await;
    resumed??;
    closed
}
