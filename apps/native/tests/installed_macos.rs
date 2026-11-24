#![cfg(target_os = "macos")]

#[path = "installed/window.rs"]
mod window;

use std::{env, fs, os::unix::fs::MetadataExt, path::PathBuf, time::Instant};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[ignore = "installs and opens the signed DMG in a separate desktop profile"]
async fn signed_mac_opens_offline_and_reopens_the_same_workspace() -> Result<()> {
    let executable = PathBuf::from(env::var("AIMANITY_QA_CLIENT")?);
    let evidence = PathBuf::from(env::var("AIMANITY_QA_ROOT")?);
    let profile = tempfile::tempdir_in(&evidence)?;
    let home = profile.path().join("dev.aimanity.workspace/host");
    let mut app = window::Client::launch(
        &executable,
        profile.path(),
        &evidence,
        &evidence.join("fresh.log"),
    )
    .await?;
    assert!(!home.join("user/native-locale").exists());
    app.choose_english().await?;
    app.workspace().await?;
    app.capture("first-workbench.png")?;
    assert_eq!(fs::read_to_string(home.join("user/native-locale"))?, "en");
    let store = home.join("workspace.redb");
    let identity = fs::metadata(&store)?.ino();
    app.close().await?;
    let started = Instant::now();
    let mut app = window::Client::launch(
        &executable,
        profile.path(),
        &evidence,
        &evidence.join("reopen.log"),
    )
    .await?;
    app.workspace().await?;
    println!(
        "Installed Mac workspace reopened offline in {} ms",
        started.elapsed().as_millis()
    );
    app.capture("reopened-workbench.png")?;
    assert_eq!(fs::metadata(store)?.ino(), identity);
    app.close().await
}
