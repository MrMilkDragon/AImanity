use crate::{
    controls::{action_button, field, form_input, form_select, text_button},
    locale::Locale,
    paired_phone,
    peer_connection::share,
};
use iced::{
    Element, Length, Task,
    widget::{column, container, row, text},
};
use tokio::sync::{oneshot, watch};
#[derive(Clone, Debug)]
enum Msg {
    Target(String),
    Code(String),
    Select(String),
    Scan,
    Found(std::result::Result<Vec<(String, String)>, String>),
    Start,
    Ready(u64),
    Stop,
    Ended(u64, std::result::Result<(), String>),
}
struct Sharing {
    locale: Locale,
    target: String,
    code: String,
    serial: Option<String>,
    phones: Vec<(String, String)>,
    stop: Option<watch::Sender<()>>,
    generation: u64,
    active: bool,
    error: String,
}
pub(super) fn window(locale: Locale) -> iced::Result {
    iced::application(
        move || {
            (
                Sharing {
                    locale,
                    target: String::new(),
                    code: String::new(),
                    serial: None,
                    phones: vec![],
                    stop: None,
                    generation: 0,
                    active: false,
                    error: String::new(),
                },
                Task::none(),
            )
        },
        Sharing::update,
        Sharing::view,
    )
    .title(move |_s: &Sharing| locale.tr("共享设备").to_owned())
    .window_size((510., 370.))
    .centered()
    .theme(|_: &Sharing| iced::Theme::Light)
    .run()
}
impl Sharing {
    fn update(&mut self, message: Msg) -> Task<Msg> {
        match message {
            Msg::Target(s) => self.target = s,
            Msg::Code(s) => self.code = s,
            Msg::Select(label) => {
                self.serial = self
                    .phones
                    .iter()
                    .find(|(_, l)| l == &label)
                    .map(|(s, _)| s.clone())
            }
            Msg::Scan => {
                return Task::perform(
                    async {
                        ai_phone_adb::devices(paired_phone::ADB_PORT, paired_phone::DEADLINE)
                            .await
                            .map(|ds| {
                                ds.into_iter()
                                    .map(|d| {
                                        let label = format!("{} · {}", d.model, d.serial);
                                        (d.serial, label)
                                    })
                                    .collect()
                            })
                            .map_err(|e| e.to_string())
                    },
                    Msg::Found,
                );
            }
            Msg::Found(Ok(phones)) => {
                if self
                    .serial
                    .as_ref()
                    .is_some_and(|s| !phones.iter().any(|(id, _)| id == s))
                {
                    self.serial = None;
                }
                self.phones = phones;
                self.error.clear();
            }
            Msg::Found(Err(e)) => self.error = e,
            Msg::Start => {
                if self.stop.is_some() {
                    return Task::none();
                }
                self.error.clear();
                let (stop, rx) = watch::channel(());
                self.stop = Some(stop);
                self.generation += 1;
                let generation = self.generation;
                let (ready, wait) = oneshot::channel();
                let (target, code, serial) = (
                    self.target.trim().to_owned(),
                    self.code.trim().to_owned(),
                    self.serial.clone(),
                );
                return Task::batch([
                    Task::perform(
                        async move {
                            share(&target, &code, serial.as_deref(), ready, rx)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        move |r| Msg::Ended(generation, r),
                    ),
                    Task::future(async move { wait.await.ok() })
                        .and_then(move |_| Task::done(Msg::Ready(generation))),
                ]);
            }
            Msg::Ready(g) if g == self.generation => self.active = self.stop.is_some(),
            Msg::Ready(_) => {}
            Msg::Stop => {
                self.stop = None;
                self.active = false;
                self.generation += 1;
            }
            Msg::Ended(g, r) if g == self.generation => {
                self.stop = None;
                self.active = false;
                self.error = r.err().unwrap_or_default();
            }
            Msg::Ended(..) => {}
        }
        Task::none()
    }
    fn view(&self) -> Element<'_, Msg> {
        let tr = |s: &'static str| self.locale.tr(s);
        let idle = self.stop.is_none();
        let os = std::env::consts::OS;
        let local = format!("{} · {}", tr("这台电脑"), ai_phone_api::platform_name(os).unwrap_or(os));
        let choices = std::iter::once(local.clone())
            .chain(self.phones.iter().map(|(_, l)| l.clone()))
            .collect::<Vec<_>>();
        let chosen = self
            .phones
            .iter()
            .find(|(s, _)| Some(s) == self.serial.as_ref())
            .map(|(_, s)| s.clone())
            .unwrap_or(local);
        let source: Element<'_, Msg> = if idle {
            form_select(choices, Some(chosen), Msg::Select)
                .width(Length::Fill)
                .into()
        } else {
            container(text(chosen))
                .center_y(crate::style::CONTROL_HEIGHT)
                .width(Length::Fill)
                .into()
        };
        let mut form = column![
            text(tr("共享设备")).size(22),
            row![
                source,
                text_button(tr("查找手机")).on_press_maybe(idle.then_some(Msg::Scan))
            ]
            .align_y(iced::Center)
            .spacing(8),
            field(
                tr("工作区 SSH 主机或本地转发端口"),
                &self.target,
                Msg::Target,
                idle
            ),
            column![
                text(tr("配对码")).size(12),
                form_input("", &self.code)
                    .secure(true)
                    .on_input_maybe(idle.then_some(Msg::Code))
            ]
            .spacing(5)
        ]
        .spacing(12);
        if !self.error.is_empty() {
            form = form.push(text(&self.error).size(13).color(crate::style::ERROR));
        }
        let submit = if idle {
            action_button(
                tr("共享屏幕与文件"),
                (!self.target.trim().is_empty() && uuid::Uuid::parse_str(self.code.trim()).is_ok())
                    .then_some(Msg::Start),
            )
        } else {
            action_button(
                if self.active {
                    tr("停止共享")
                } else {
                    tr("取消")
                },
                Some(Msg::Stop),
            )
        };
        iced::widget::scrollable(container(form.push(submit)).padding(24).width(Length::Fill))
            .width(Length::Fill)
            .into()
    }
}
