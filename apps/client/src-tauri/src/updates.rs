use crate::client_update::{self, Offer, Platform, Result, check, platform};
use semver::Version;
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::{
    AppHandle, Manager, Wry,
    menu::MenuItem,
    window::{ProgressBarState, ProgressBarStatus},
};
use tokio::sync::Mutex;

#[cfg(target_os = "macos")]
pub(crate) use crate::client_update::macos;
pub(crate) use crate::client_update::run;

pub const ID: &str = "client-updates";
pub const CHECK: &str = "Check for Updates…";

pub struct Menu {
    item: MenuItem<Wry>,
    state: Mutex<UpdateState>,
}

#[derive(Default)]
struct UpdateState {
    offer: Option<Offer>,
    checked: Option<(Instant, Duration)>,
}

impl Menu {
    pub fn new(item: MenuItem<Wry>) -> Self {
        Self {
            item,
            state: Mutex::new(UpdateState::default()),
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn idle_guard(app: &AppHandle) -> std::result::Result<impl Drop + '_, &'static str> {
    app.state::<Menu>()
        .inner()
        .state
        .try_lock()
        .map_err(|_| "An update is in progress. Try again after it finishes.")
}

pub fn cleanup_current(app: &AppHandle) {
    if let (Some(platform), Ok(cache)) = (platform(), app.path().app_cache_dir()) {
        client_update::remove_current(&cache, &app.package_info().version, platform);
    }
}

#[derive(Serialize)]
pub struct UpdateDetails {
    version: String,
    name: String,
    notes: String,
}

impl From<&Offer> for UpdateDetails {
    fn from(offer: &Offer) -> Self {
        Self {
            version: offer.version.to_string(),
            name: offer.name.clone(),
            notes: offer.notes.clone(),
        }
    }
}

fn progress(app: &AppHandle, status: ProgressBarStatus, value: Option<u64>) {
    if let Some(window) = app
        .get_webview_window("workbench")
        .or_else(|| app.get_webview_window("setup"))
    {
        let _ = window.set_progress_bar(ProgressBarState {
            status: Some(status),
            progress: value,
        });
    }
}

pub async fn refresh(app: &AppHandle, force: bool) -> Result<Option<UpdateDetails>> {
    let Some(platform) = platform() else {
        return Ok(None);
    };
    let menu = app.state::<Menu>();
    let mut state = menu.state.lock().await;
    if !force
        && state
            .checked
            .is_some_and(|(at, lifetime)| at.elapsed() < lifetime)
    {
        return Ok(state.offer.as_ref().map(UpdateDetails::from));
    }
    let _ = menu.item.set_enabled(false);
    let _ = menu.item.set_text("Checking for Updates…");
    match check(&app.package_info().version, platform).await {
        Ok(next) => {
            state.offer = next;
            state.checked = Some((Instant::now(), Duration::from_secs(30 * 60)));
            show_menu(&menu, state.offer.as_ref(), "Up to Date · Check Again");
            Ok(state.offer.as_ref().map(UpdateDetails::from))
        }
        Err(error) => {
            state.checked = Some((Instant::now(), Duration::from_secs(60)));
            show_menu(
                &menu,
                state.offer.as_ref(),
                "Could Not Check for Updates · Retry",
            );
            Err(error)
        }
    }
}

pub fn check_on_launch(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = refresh(&app, false).await {
            eprintln!("Client update: {error}");
        }
    });
}

fn show_menu(menu: &Menu, offer: Option<&Offer>, no_offer_label: &str) {
    let label = offer.map_or_else(
        || no_offer_label.to_owned(),
        |offer| format!("Install Update {}…", offer.version),
    );
    let _ = menu.item.set_text(label);
    let _ = menu.item.set_enabled(true);
}

pub async fn apply(app: &AppHandle, requested: Option<Version>) -> Result<()> {
    let platform = platform().ok_or("Updates are unavailable on this platform")?;
    let menu = app.state::<Menu>();
    let state = menu
        .state
        .try_lock()
        .map_err(|_| "An update operation is already in progress")?;
    let offer = state.offer.as_ref().ok_or("Check for an update first")?;
    if requested.is_some_and(|version| version != offer.version) {
        return Err("The available update has changed".into());
    }
    let _ = menu.item.set_enabled(false);
    let _ = menu
        .item
        .set_text(format!("Downloading Update {}…", offer.version));
    match install(app, offer, platform).await {
        Ok(()) => {
            app.exit(0);
            Ok(())
        }
        Err(error) => {
            progress(app, ProgressBarStatus::None, None);
            show_menu(&menu, Some(offer), CHECK);
            Err(error)
        }
    }
}

pub fn select(app: &AppHandle) {
    if platform().is_none() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let menu = app.state::<Menu>();
        let ready = match menu.state.try_lock() {
            Ok(state) => state.offer.is_some(),
            Err(_) => return,
        };
        let result = if ready {
            apply(&app, None).await
        } else {
            refresh(&app, true).await.map(|_| ())
        };
        if let Err(error) = result {
            eprintln!("Client update: {error}");
            if ready {
                let _ = menu.item.set_text("Update Interrupted · Retry");
            }
        }
    });
}

async fn install(app: &AppHandle, offer: &Offer, platform: Platform) -> Result<()> {
    let cache = app.path().app_cache_dir()?;
    let downloaded = client_update::download(&cache, offer, platform, |percent| {
        progress(app, ProgressBarStatus::Normal, Some(percent));
    })
    .await?;
    progress(app, ProgressBarStatus::Indeterminate, None);
    let _ = app
        .state::<Menu>()
        .item
        .set_text(format!("Installing Update {}…", offer.version));
    client_update::launch(
        &cache,
        &downloaded,
        &offer.version,
        platform,
        &app.config().identifier,
    )
}
