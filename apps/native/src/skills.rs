use crate::{
    connection::Connection,
    controls::{Icon, action_button, form_input, sized_icon_button},
    locale::Locale,
    style,
    surface::tool_action,
};
use ai_phone_api::wire;
use iced::{
    Element, Font, Length, Padding, Task,
    widget::{button, checkbox, column, container, row, rule, text},
};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Message {
    Load,
    Loaded(Result<wire::SkillList, String>),
    ToggleImport,
    Search(String),
    Source(String),
    ChooseFile,
    FileChosen(Option<PathBuf>),
    ImportUrl,
    Imported(Result<wire::RuntimeSkill, String>),
    Expand(String),
    Enable(String, bool),
    Remove(String),
    Undo,
    Changed(Result<wire::SkillList, String>, Option<String>),
    OpenSource(String),
}

#[derive(Default)]
pub struct Skills {
    items: Vec<wire::RuntimeSkill>,
    source: String,
    query: String,
    adding: bool,
    busy: bool,
    loaded: bool,
    error: String,
    expanded: Option<String>,
    removed: Option<String>,
}

impl Skills {
    pub fn update(&mut self, message: Message, connection: &Connection) -> Task<Message> {
        match message {
            Message::Load => {
                if self.busy {
                    return Task::none();
                }
                self.busy = true;
                let connection = connection.clone();
                return Task::perform(
                    async move {
                        connection
                            .rpc("workbench/skills", wire::WorkbenchRequest::default())
                            .await
                    },
                    Message::Loaded,
                );
            }
            Message::Loaded(result) => {
                self.busy = false;
                self.loaded = true;
                match result {
                    Ok(value) => {
                        self.items = value.skills;
                        self.error.clear();
                    }
                    Err(error) => self.error = error,
                }
            }
            Message::ToggleImport => self.adding = !self.adding,
            Message::Search(query) => self.query = query,
            Message::Source(source) => self.source = source,
            Message::ChooseFile => {
                return Task::perform(
                    async {
                        crate::documents::AsyncFileDialog::new()
                            .add_filter("SKILL.md", &["md", "txt"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_owned())
                    },
                    Message::FileChosen,
                );
            }
            Message::FileChosen(None) => {}
            Message::FileChosen(Some(path)) => {
                self.busy = true;
                self.error.clear();
                let connection = connection.clone();
                return Task::perform(
                    async move {
                        let content = tokio::fs::read_to_string(&path)
                            .await
                            .map_err(|error| error.to_string())?;
                        let name = path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        connection
                            .rpc(
                                "workbench/skill-import",
                                wire::SkillImportRequest {
                                    source: String::new(),
                                    content,
                                    name,
                                },
                            )
                            .await
                    },
                    Message::Imported,
                );
            }
            Message::ImportUrl => {
                if self.busy || self.source.trim().is_empty() {
                    return Task::none();
                }
                self.busy = true;
                self.error.clear();
                let connection = connection.clone();
                let source = self.source.trim().to_owned();
                return Task::perform(
                    async move {
                        connection
                            .rpc(
                                "workbench/skill-import",
                                wire::SkillImportRequest {
                                    source,
                                    ..Default::default()
                                },
                            )
                            .await
                    },
                    Message::Imported,
                );
            }
            Message::Imported(result) => {
                self.busy = false;
                match result {
                    Ok(skill) => {
                        self.source.clear();
                        self.adding = false;
                        self.expanded = Some(skill.id);
                        return self.update(Message::Load, connection);
                    }
                    Err(error) => self.error = error,
                }
            }
            Message::Expand(id) => {
                self.expanded = (self.expanded.as_ref() != Some(&id)).then_some(id);
            }
            Message::Enable(id, enabled) => return self.change(connection, id, enabled, false),
            Message::Remove(id) => return self.change(connection, id, false, true),
            Message::Undo => {
                if let Some(id) = self.removed.clone() {
                    return self.change(connection, id, true, false);
                }
            }
            Message::Changed(result, removed) => {
                self.busy = false;
                match result {
                    Ok(value) => {
                        self.items = value.skills;
                        self.removed = removed;
                        self.error.clear();
                    }
                    Err(error) => self.error = error,
                }
            }
            Message::OpenSource(source) => {
                if let Err(error) = crate::platform::open_link(source) {
                    self.error = error.to_string();
                }
            }
        }
        Task::none()
    }

    fn change(
        &mut self,
        connection: &Connection,
        id: String,
        enabled: bool,
        remove: bool,
    ) -> Task<Message> {
        self.busy = true;
        self.error.clear();
        let connection = connection.clone();
        let removed = if remove {
            Some(id.clone())
        } else {
            self.removed.clone().filter(|removed| removed != &id)
        };
        Task::perform(
            async move {
                connection
                    .rpc(
                        "workbench/skill-update",
                        wire::SkillUpdateRequest {
                            id,
                            enabled,
                            remove,
                        },
                    )
                    .await
            },
            move |result| Message::Changed(result, removed.clone()),
        )
    }

    pub fn actions(&self) -> Element<'_, Message> {
        row![
            tool_action(Icon::Upload, (!self.busy).then_some(Message::ChooseFile)),
            tool_action(Icon::Plus, Some(Message::ToggleImport)),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(2)
        .into()
    }

    pub fn view(&self, locale: Locale) -> Element<'_, Message> {
        let mut content =
            column![form_input(locale.tr("搜索"), &self.query).on_input(Message::Search)]
                .spacing(10);
        if self.adding {
            let submit =
                (!self.busy && !self.source.trim().is_empty()).then_some(Message::ImportUrl);
            content = content.push(
                row![
                    Icon::Link.view(),
                    form_input(locale.tr("GitHub / SKILL.md 网址"), &self.source)
                        .on_input_maybe((!self.busy).then_some(Message::Source))
                        .on_submit_maybe(submit.clone())
                        .width(Length::Fill),
                    action_button(
                        if self.busy {
                            locale.tr("导入中…")
                        } else {
                            locale.tr("导入")
                        },
                        submit
                    ),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            );
        }
        if !self.error.is_empty() {
            content = content.push(
                text(locale.tr(&self.error))
                    .size(13)
                    .style(style::error_text),
            );
        }
        if !self.loaded && self.error.is_empty() {
            content = content.push(text(locale.tr("读取中…")).size(13).style(style::muted_text));
        }
        let mut list = column![].spacing(4);
        let query = self.query.trim().to_lowercase();
        for skill in self.items.iter().filter(|skill| {
            query.is_empty()
                || skill.name.to_lowercase().contains(&query)
                || skill.description.to_lowercase().contains(&query)
        }) {
            let expanded = self.expanded.as_ref() == Some(&skill.id);
            let label = row![
                if expanded { Icon::Down } else { Icon::Right }
                    .view()
                    .width(15)
                    .height(15),
                container(style::section_title(&skill.name))
                    .width(Length::Fill)
                    .clip(true),
            ]
            .spacing(8)
            .height(style::COMPACT_CONTROL)
            .align_y(iced::Alignment::Center);
            let mut heading = row![
                button(label)
                    .style(style::quiet)
                    .padding(0)
                    .width(Length::Fill)
                    .on_press(Message::Expand(skill.id.clone()))
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center);
            if skill.source == "builtin" {
                heading = heading.push(text(locale.tr("内置")).size(12).style(style::muted_text));
            } else {
                let id = skill.id.clone();
                heading = heading.push(checkbox(skill.enabled).size(16).on_toggle_maybe(
                    (!self.busy).then_some(move |enabled| Message::Enable(id.clone(), enabled)),
                ));
            }
            if !skill.builtin {
                heading = heading.push(sized_icon_button(
                    Icon::Trash,
                    (!self.busy).then(|| Message::Remove(skill.id.clone())),
                    style::COMPACT_CONTROL,
                    style::COMPACT_CONTROL,
                ));
            }
            let mut article = column![heading];
            if expanded {
                let mut details = column![].spacing(10);
                if !skill.description.is_empty() {
                    details = details.push(
                        text(&skill.description)
                            .size(13)
                            .line_height(iced::Pixels(21.45))
                            .style(style::muted_text),
                    );
                }
                if !skill.builtin {
                    let source = text(&skill.source).size(12).style(style::muted_text);
                    details = if skill.source.starts_with("https://") {
                        details.push(
                            button(source)
                                .style(style::quiet)
                                .padding(0)
                                .on_press(Message::OpenSource(skill.source.clone())),
                        )
                    } else {
                        details.push(source)
                    };
                }
                details = details.push(text(&skill.content).size(12).font(Font::MONOSPACE));
                article = article.push(container(details).padding(Padding::ZERO.top(16).left(23)));
            }
            list = list.push(column![
                container(article).padding([14, 0]),
                rule::horizontal(1).style(|theme| rule::Style {
                    color: style::FOREGROUND.scale_alpha(16. / 255.),
                    ..rule::default(theme)
                }),
            ]);
        }
        content = content.push(list);
        if self.removed.is_some() {
            content = content.push(
                crate::controls::content_button(
                    row![Icon::Undo.view(), text(locale.tr("撤销移除")).size(13)]
                        .spacing(8)
                        .align_y(iced::Alignment::Center),
                    style::CONTROL_HEIGHT,
                )
                .on_press_maybe((!self.busy).then_some(Message::Undo)),
            );
        }
        content.into()
    }
}
