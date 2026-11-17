use crate::{
    connection::Connection,
    controls::{Icon, action_button, icon_button},
    style,
};
use ai_phone_api::wire;
use iced::{
    Element, Length, Task,
    widget::{checkbox, column, pick_list, row, space, text, text_input},
};

#[derive(Clone, Debug)]
pub enum Message {
    Load,
    Edit(String),
    Import,
    Refresh,
    Enable(bool),
    Select(String, String),
    Received(bool, Result<wire::HarnessProxyState, String>),
}
#[derive(Default)]
pub struct Proxy {
    state: Option<wire::HarnessProxyState>,
    url: String,
    busy: bool,
    error: String,
}
impl Proxy {
    pub fn enabled(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.enabled)
    }
    pub fn update(&mut self, message: Message, client: &Connection) -> Task<Message> {
        let mut request = wire::HarnessProxyRequest::default();
        let route = match message {
            Message::Load => "proxy/state",
            Message::Refresh => "proxy/refresh",
            Message::Enable(enabled) => {
                request.enabled = enabled;
                "proxy/toggle"
            }
            Message::Import => {
                request.subscription_url = self.url.trim().to_owned();
                "proxy/import"
            }
            Message::Select(group, node) => {
                request.group = group;
                request.node = node;
                "proxy/select"
            }
            Message::Edit(url) => {
                self.url = url;
                return Task::none();
            }
            Message::Received(imported, result) => {
                self.busy = false;
                match result {
                    Ok(state) => {
                        self.state = Some(state);
                        if imported {
                            self.url.clear();
                        }
                    }
                    Err(e) => self.error = e,
                }
                return Task::none();
            }
        };
        if self.busy {
            return Task::none();
        }
        self.error.clear();
        self.busy = true;
        let client = client.clone();
        Task::perform(async move { client.rpc(route, request).await }, move |r| {
            Message::Received(route == "proxy/import", r)
        })
    }
    pub fn view(&self, locale: crate::locale::Locale) -> Element<'_, Message> {
        let mut body = column![].spacing(12);
        if let Some(state) = &self.state {
            let import = (!self.busy && !self.url.trim().is_empty()).then_some(Message::Import);
            let mut subscription = row![
                text_input(
                    if state.has_subscription {
                        locale.tr("已保存")
                    } else {
                        "https://"
                    },
                    &self.url
                )
                .secure(true)
                .style(style::input)
                .size(14)
                .line_height(iced::Pixels(20.))
                .padding([10, 12])
                .on_input_maybe((!self.busy).then_some(Message::Edit))
                .on_submit_maybe(import.clone()),
                action_button(locale.tr("导入"), import).width(60),
            ]
            .spacing(8)
            .align_y(iced::Center);
            if state.has_subscription {
                subscription = subscription.push(icon_button(
                    Icon::Refresh,
                    (!self.busy).then_some(Message::Refresh),
                ));
            }
            body = body
                .push(
                    row![
                        text(locale.tr("启用")).size(14),
                        space::horizontal(),
                        checkbox(state.enabled).size(17).on_toggle_maybe(
                            (!self.busy && state.has_subscription).then_some(Message::Enable),
                        ),
                    ]
                    .spacing(8)
                    .align_y(iced::Center),
                )
                .push(
                    column![
                        text(locale.tr("订阅地址")).size(12).color(style::MUTED),
                        subscription,
                    ]
                    .spacing(5),
                );
            for group in &state.groups {
                body = body.push(
                    column![
                        text(&group.name).size(12).color(style::MUTED),
                        pick_list(&group.nodes[..], Some(&group.selected), |node| {
                            Message::Select(group.name.clone(), node)
                        })
                        .width(Length::Fill)
                        .style(style::select)
                        .menu_style(style::menu)
                        .text_size(14)
                        .text_line_height(iced::Pixels(20.))
                        .padding([10, 12])
                        .menu_height(260)
                    ]
                    .spacing(6),
                );
            }
        }
        if self.busy || !self.error.is_empty() {
            body = body.push(
                text(if self.busy {
                    locale.tr("正在处理…")
                } else {
                    &self.error
                })
                .size(12)
                .color(if self.busy {
                    style::MUTED
                } else {
                    style::ERROR
                }),
            );
        }
        body.into()
    }
}
