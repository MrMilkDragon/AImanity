use crate::{client_update, controls::action_button, locale::Locale, style};
use futures_util::SinkExt;
use iced::{
    Element, Font, Task, font,
    widget::{column, row, text},
};
use semver::Version;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Message {
    Checked(Result<Option<client_update::Offer>, String>),
    Install,
    Progress(u64),
    Finished(Result<(), String>),
}

#[derive(Default)]
pub struct State {
    pub offer: Option<client_update::Offer>,
    pub progress: Option<u64>,
    pub error: String,
}

pub fn check() -> Task<Message> {
    let Some(platform) = client_update::platform() else {
        return Task::none();
    };
    Task::perform(
        async move {
            let version = Version::parse(env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
            if let Ok(cache) = cache() {
                client_update::remove_current(&cache, &version, platform);
            }
            client_update::check(&version, platform)
                .await
                .map_err(|e| e.to_string())
        },
        Message::Checked,
    )
}

fn cache() -> Result<PathBuf, String> {
    let base = if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "HOME"
    };
    let directory = PathBuf::from(std::env::var_os(base).ok_or("找不到更新缓存目录")?);
    let directory = if cfg!(target_os = "macos") {
        directory.join("Library/Caches")
    } else {
        directory
    };
    Ok(directory.join("dev.aiphone.client"))
}

impl State {
    pub fn install(&mut self) -> Task<Message> {
        let Some((offer, platform)) = self.offer.clone().zip(client_update::platform()) else {
            return Task::none();
        };
        if self.progress.is_some() {
            return Task::none();
        }
        self.progress = Some(0);
        self.error.clear();
        Task::run(
            iced::stream::channel(4, async move |mut output| {
                let result = async {
                    let cache = cache()?;
                    let downloaded =
                        client_update::download(&cache, &offer, platform, |progress| {
                            let _ = output.try_send(Message::Progress(progress));
                        })
                        .await
                        .map_err(|e| e.to_string())?;
                    tokio::task::spawn_blocking(move || {
                        client_update::launch(
                            &cache,
                            &downloaded,
                            &offer.version,
                            platform,
                            "dev.aiphone.client",
                        )
                        .map_err(|e| e.to_string())
                    })
                    .await
                    .map_err(|e| e.to_string())?
                }
                .await;
                let _ = output.send(Message::Finished(result)).await;
            }),
            |message| message,
        )
    }

    pub fn view(&self, locale: Locale) -> Element<'_, Message> {
        let Some(offer) = self.offer.as_ref() else {
            return column![].into();
        };
        let mut body = column![
            row![
                text(offer.version.to_string()).size(14),
                text(&offer.name).size(14).font(Font {
                    weight: font::Weight::Bold,
                    ..Font::DEFAULT
                })
            ]
            .spacing(8)
            .align_y(iced::Center),
            text(&offer.notes).size(14)
        ]
        .spacing(12);
        if let Some(progress) = self.progress {
            body = body.push(text(format!("{} {progress}%", locale.tr("下载更新"))).size(13));
        }
        if !self.error.is_empty() {
            body = body.push(text(&self.error).size(12).color(style::ERROR));
        }
        body.push(action_button(
            locale.tr(if self.progress.is_some() {
                "正在更新…"
            } else {
                "确认更新"
            }),
            self.progress.is_none().then_some(Message::Install),
        ))
        .into()
    }
}
