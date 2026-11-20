use crate::{connection::Connection, controls::Icon, locale::Locale, style};
use ai_phone_api::wire;
use iced::{
    Element, Length, Task, mouse,
    widget::{button, column, container, mouse_area, row, text},
};

const ROW: f32 = 40.;
const GAP: f32 = 2.;

#[derive(Clone, Debug)]
pub enum Message {
    Load,
    Loaded(u64, Result<wire::TodoList, String>),
    Select(String),
    Drag(String),
    Over(usize),
    Drop,
    Cancel,
    Step(bool),
}

#[derive(Default)]
pub struct Todos {
    items: Vec<wire::TodoItem>,
    selected: String,
    dragging: bool,
    slot: Option<usize>,
    request: u64,
    reading: bool,
    moving: bool,
    error: String,
}

impl Todos {
    pub fn rows_height(&self) -> f32 {
        let notice = !self.error.is_empty() || self.items.is_empty();
        let rows = self.items.len() + usize::from(notice);
        self.items.len() as f32 * (ROW + 2.)
            + if notice { ROW } else { 0. }
            + rows.saturating_sub(1) as f32 * GAP
    }

    pub fn cancel_drag(&mut self) {
        self.dragging = false;
        self.slot = None;
    }
    pub fn update(&mut self, message: Message, client: &Connection) -> Task<Message> {
        match message {
            Message::Load if !self.reading && !self.moving && !self.dragging => {
                self.reading = true;
                self.request += 1;
                let seq = self.request;
                let client = client.clone();
                return Task::perform(
                    async move {
                        client
                            .rpc("proactive/todos", wire::ProactiveRequest {})
                            .await
                    },
                    move |r| Message::Loaded(seq, r),
                );
            }
            Message::Loaded(seq, result) if seq == self.request => {
                self.reading = false;
                self.moving = false;
                match result {
                    Ok(list) => {
                        self.items = list.items;
                        self.error.clear();
                    }
                    Err(e) => self.error = e,
                }
            }
            Message::Select(id) => self.selected = id,
            Message::Drag(id) if !self.moving => {
                self.selected = id;
                self.dragging = true;
                self.slot = None;
            }
            Message::Over(slot) if self.dragging => self.slot = Some(slot),
            Message::Cancel => self.cancel_drag(),
            Message::Drop if self.dragging => {
                self.dragging = false;
                if let Some(slot) = self.slot.take() {
                    return self.move_to(slot, client);
                }
            }
            Message::Step(down) if !self.moving => {
                if let Some(i) = self.items.iter().position(|item| item.id == self.selected) {
                    return self.move_to(
                        if down {
                            (i + 2).min(self.items.len())
                        } else {
                            i.saturating_sub(1)
                        },
                        client,
                    );
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn move_to(&mut self, slot: usize, client: &Connection) -> Task<Message> {
        let Some(from) = self.items.iter().position(|item| item.id == self.selected) else {
            return Task::none();
        };
        if slot == from || slot == from + 1 {
            return Task::none();
        }
        let before_id = self
            .items
            .get(slot)
            .map(|item| item.id.clone())
            .unwrap_or_default();
        let input = wire::TodoMove {
            id: self.selected.clone(),
            before_id,
        };
        self.request += 1;
        let seq = self.request;
        self.moving = true;
        let client = client.clone();
        Task::perform(
            async move { client.rpc("proactive/move-todo", input).await },
            move |r| Message::Loaded(seq, r),
        )
    }

    pub fn view(&self, locale: Locale) -> Element<'_, Message> {
        let mut rows = column![].spacing(GAP);
        if !self.error.is_empty() {
            rows = rows.push(
                button(
                    text(locale.tr(&self.error))
                        .size(13)
                        .style(style::muted_text),
                )
                .height(ROW)
                .on_press(Message::Load)
                .style(style::quiet),
            );
        } else if self.items.is_empty() {
            rows = rows.push(
                container(
                    text(locale.tr(if self.reading {
                        "读取中…"
                    } else {
                        "暂无待办"
                    }))
                    .size(13)
                    .style(style::muted_text),
                )
                .center_y(ROW),
            );
        }
        for (i, item) in self.items.iter().enumerate() {
            let source = match item.source.as_str() {
                "copilot" => "Copilot",
                "user" => "主线任务",
                "notification" => "通知",
                "device-handoff" => "设备协作",
                _ => "Liberty",
            };
            let name = item
                .title
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or(source);
            let selected = self.selected == item.id;
            let grip = mouse_area(container(Icon::Grip.view()).center(Length::Fixed(ROW)))
                .on_press(Message::Drag(item.id.clone()))
                .interaction(mouse::Interaction::Grab);
            let title = button(
                container(
                    text(locale.tr(name))
                        .size(14)
                        .wrapping(text::Wrapping::None),
                )
                .center_y(ROW),
            )
            .on_press(Message::Select(item.id.clone()))
            .width(Length::Fill)
            .clip(true)
            .padding([0, 6])
            .style(style::quiet);
            let detail = if item.status == "running" {
                "执行中"
            } else if item.status == "paused" {
                "已暂停"
            } else {
                source
            };
            let content = container(
                row![
                    title,
                    text(locale.tr(detail)).size(12).style(style::muted_text),
                    grip
                ]
                .align_y(iced::Center),
            )
            .style(move |theme: &iced::Theme| container::Style {
                background: selected.then(|| theme.palette().text.scale_alpha(0.055).into()),
                ..Default::default()
            });
            let marker = |active| {
                iced::widget::rule::horizontal(1).style(move |theme: &iced::Theme| {
                    iced::widget::rule::Style {
                        color: if active {
                            theme.palette().text
                        } else {
                            iced::Color::TRANSPARENT
                        },
                        radius: 0.into(),
                        fill_mode: iced::widget::rule::FillMode::Full,
                        snap: true,
                    }
                })
            };
            let mut target = mouse_area(column![
                marker(self.slot == Some(i)),
                content,
                marker(i + 1 == self.items.len() && self.slot == Some(i + 1))
            ]);
            if self.dragging {
                target = target
                    .on_move(move |p| Message::Over(i + usize::from(p.y >= ROW / 2.)))
                    .on_enter(Message::Over(i))
                    .on_release(Message::Drop);
            }
            rows = rows.push(target);
        }
        rows.into()
    }
}
