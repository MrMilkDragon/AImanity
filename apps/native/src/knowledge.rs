mod graph;

use crate::{
    connection::Connection,
    controls::{Icon, field, icon_button},
    locale::Locale,
    style,
};
use ai_phone_api::wire;
use iced::{
    Element, Length, Point, Subscription, Task, keyboard,
    widget::{
        button, canvas, column, container, image as picture, markdown, row, rule, scrollable,
        space, text, text_editor, text_input,
    },
};
use std::{collections::HashMap, sync::Arc, time::Duration};

const IMAGE_ROUTE: &str = "/api/workbench/knowledge-image/";
const MAX_IMAGE: usize = 6 * 1024 * 1024;

#[derive(Clone, Debug)]
pub enum Message {
    Load,
    Query(String),
    Listed(String, Result<wire::KnowledgeList, String>),
    New,
    Select(String, Option<Point>),
    CloseDocument,
    Read(
        String,
        Result<(wire::KnowledgeDocument, wire::KnowledgeList), String>,
    ),
    Edit,
    Title(String),
    Body(text_editor::Action),
    Save,
    Delete,
    Reload,
    Saved(String, Result<wire::KnowledgeDocument, String>),
    Deleted(String, Result<wire::KnowledgeList, String>),
    Related(String, u64, Result<wire::KnowledgeList, String>),
    TogglePicker,
    RelationQuery(String),
    Options(String, Result<wire::KnowledgeList, String>),
    Link(String),
    PickImage,
    Paste,
    Insert(String, text_editor::Cursor, Result<Option<String>, String>),
    Image(String, Result<picture::Handle, String>),
    OpenLink(String),
}

struct Draft {
    document: wire::KnowledgeDocument,
    body: text_editor::Content,
    related: Vec<wire::KnowledgeDocument>,
    rendered: markdown::Content,
    dirty: bool,
}
impl Draft {
    fn new(document: wire::KnowledgeDocument, related: Vec<wire::KnowledgeDocument>) -> Self {
        Self {
            body: text_editor::Content::with_text(&document.content),
            rendered: markdown::Content::parse(&document.content),
            document,
            related,
            dirty: false,
        }
    }
    fn savable(&self) -> bool {
        !self.document.title.trim().is_empty() && (self.document.id.is_empty() || self.dirty)
    }
}

#[derive(Default)]
pub struct Knowledge {
    documents: Vec<wire::KnowledgeDocument>,
    graph: graph::Graph,
    query: String,
    search: Option<iced::task::Handle>,
    loaded: bool,
    error: String,
    document_error: String,
    selected: Option<String>,
    anchor: Option<Point>,
    document_open: bool,
    editing: bool,
    busy: bool,
    reading: Option<String>,
    drafts: HashMap<String, Draft>,
    images: HashMap<String, Option<picture::Handle>>,
    picking: bool,
    relation_query: String,
    relation_search: Option<iced::task::Handle>,
    options: Vec<wire::KnowledgeDocument>,
}
impl Knowledge {
    pub fn update(&mut self, message: Message, connection: &Connection) -> Task<Message> {
        match message {
            Message::Load => {
                if self.search.is_none() {
                    return self.search(connection, Duration::ZERO);
                }
            }
            Message::Query(query) => {
                self.query = query;
                return self.search(connection, Duration::from_millis(180));
            }
            Message::Listed(query, result) => {
                if query != self.query {
                    return Task::none();
                }
                self.search = None;
                self.loaded = true;
                match result {
                    Ok(list) => {
                        self.error.clear();
                        let graph_changed =
                            list.documents.len() != self.documents.len()
                                || list.documents.iter().zip(&self.documents).any(
                                    |(next, current)| {
                                        next.id != current.id
                                            || next.title != current.title
                                            || next.related_ids != current.related_ids
                                    },
                                );
                        if graph_changed {
                            self.graph.rebuild(&list.documents);
                        }
                        self.documents = list.documents;
                        return self.load_document(connection);
                    }
                    Err(error) => self.error = error,
                }
            }
            Message::New => {
                self.drafts
                    .entry(String::new())
                    .or_insert_with(|| Draft::new(Default::default(), Vec::new()));
                self.editing = true;
                self.select(String::new(), None);
            }
            Message::Select(id, origin) => {
                if self.document_open && self.selected.as_ref() == Some(&id) {
                    return Task::none();
                }
                self.editing = self.drafts.get(&id).is_some_and(|draft| draft.dirty);
                let anchor = origin.or(self.anchor);
                self.select(id, anchor);
                return self.load_document(connection);
            }
            Message::CloseDocument => {
                self.document_open = false;
                self.close_picker();
            }
            Message::Read(id, result) => {
                if self.reading.as_ref() != Some(&id) {
                    return Task::none();
                }
                self.reading = None;
                match result {
                    Ok((document, _)) if document.trashed => {
                        self.document_error = "文档已移除".into();
                    }
                    Ok((document, related)) => {
                        if self.drafts.get(&id).is_none_or(|draft| {
                            !draft.dirty && document.revision > draft.document.revision
                        }) {
                            self.drafts
                                .insert(id, Draft::new(document, related.documents));
                        }
                        return self.load_images(connection);
                    }
                    Err(error) => self.document_error = error,
                }
            }
            Message::Edit => {
                self.editing = true;
                self.close_picker();
            }
            Message::Title(title) => {
                if let Some(draft) = self.draft_mut() {
                    draft.document.title = title;
                    draft.dirty = true;
                }
            }
            Message::Body(action) => {
                if !self.busy
                    && let Some(draft) = self.draft_mut()
                {
                    draft.dirty |= action.is_edit();
                    draft.body.perform(action);
                }
            }
            Message::Reload => {
                if let Some(id) = &self.selected {
                    self.drafts.remove(id);
                }
                self.editing = false;
                self.document_error.clear();
                return self.load_document(connection);
            }
            Message::Save => {
                let Some(draft) = self.draft().filter(|draft| !self.busy && draft.savable()) else {
                    return Task::none();
                };
                let request = wire::KnowledgeSaveRequest {
                    id: draft.document.id.clone(),
                    title: draft.document.title.clone(),
                    content: draft.body.text(),
                    expected_revision: draft.document.revision,
                    related_ids: draft.document.related_ids.clone(),
                };
                let id = request.id.clone();
                self.busy = true;
                self.document_error.clear();
                let connection = connection.clone();
                return Task::perform(
                    async move { connection.rpc("workbench/knowledge-save", request).await },
                    move |result| Message::Saved(id.clone(), result),
                );
            }
            Message::Delete => {
                let Some(draft) = self
                    .draft()
                    .filter(|draft| !self.busy && !draft.document.id.is_empty())
                else {
                    return Task::none();
                };
                let id = draft.document.id.clone();
                let request = wire::KnowledgeDeleteRequest {
                    id: id.clone(),
                    expected_revision: draft.document.revision,
                };
                let connection = connection.clone();
                self.busy = true;
                self.document_error.clear();
                return Task::perform(
                    async move { connection.rpc("workbench/knowledge-delete", request).await },
                    move |result| Message::Deleted(id.clone(), result),
                );
            }
            Message::Saved(old_id, result) => {
                self.busy = false;
                match result {
                    Ok(document) => {
                        let id = document.id.clone();
                        let revision = document.revision;
                        let related = self
                            .drafts
                            .remove(&old_id)
                            .map(|draft| draft.related)
                            .unwrap_or_default();
                        self.drafts
                            .insert(id.clone(), Draft::new(document, related));
                        if self.selected.as_ref() == Some(&old_id) {
                            self.selected = Some(id.clone());
                            self.editing = false;
                        }
                        let related_connection = connection.clone();
                        let related_id = id.clone();
                        return Task::batch([
                            self.search(connection, Duration::ZERO),
                            self.load_images(connection),
                            Task::perform(
                                async move {
                                    related_connection
                                        .rpc(
                                            "workbench/knowledge-related",
                                            wire::WorkbenchRequest {
                                                id: related_id,
                                                ..Default::default()
                                            },
                                        )
                                        .await
                                },
                                move |result| Message::Related(id.clone(), revision, result),
                            ),
                        ]);
                    }
                    Err(error) => self.document_error = error,
                }
            }
            Message::Deleted(id, result) => {
                self.busy = false;
                match result {
                    Ok(_) => {
                        self.drafts.remove(&id);
                        if self.selected.as_ref() == Some(&id) {
                            self.document_open = false;
                            self.close_picker();
                        }
                        return self.search(connection, Duration::ZERO);
                    }
                    Err(error) => self.document_error = error,
                }
            }
            Message::Related(id, revision, result) => match result {
                Ok(list) => {
                    if let Some(draft) = self
                        .drafts
                        .get_mut(&id)
                        .filter(|draft| draft.document.revision == revision)
                    {
                        draft.related = list.documents;
                    }
                }
                Err(error) if self.selected.as_ref() == Some(&id) => self.document_error = error,
                Err(_) => {}
            },
            Message::TogglePicker => {
                if self.picking {
                    self.close_picker();
                } else {
                    self.picking = true;
                    return Task::batch([
                        self.search_relations(connection, Duration::ZERO),
                        iced::widget::operation::focus("knowledge-relation-search"),
                    ]);
                }
            }
            Message::RelationQuery(query) => {
                self.relation_query = query;
                return self.search_relations(connection, Duration::from_millis(180));
            }
            Message::Options(query, result) => {
                if self.picking && query == self.relation_query {
                    self.relation_search = None;
                    match result {
                        Ok(list) => self.options = list.documents,
                        Err(error) => self.document_error = error,
                    }
                }
            }
            Message::Link(target_id) => {
                let Some(draft) = self.draft().filter(|_| !self.busy) else {
                    return Task::none();
                };
                let id = draft.document.id.clone();
                let request = wire::KnowledgeLinkRequest {
                    id: id.clone(),
                    present: !draft.document.related_ids.contains(&target_id),
                    target_id,
                    expected_revision: draft.document.revision,
                };
                self.busy = true;
                self.document_error.clear();
                let connection = connection.clone();
                return Task::perform(
                    async move { connection.rpc("workbench/knowledge-link", request).await },
                    move |result| Message::Saved(id.clone(), result),
                );
            }
            Message::PickImage | Message::Paste => {
                let Some(draft) = self.draft().filter(|_| !self.busy) else {
                    return Task::none();
                };
                let id = draft.document.id.clone();
                let cursor = draft.body.cursor();
                let paste = matches!(message, Message::Paste);
                self.busy = true;
                self.document_error.clear();
                let connection = connection.clone();
                return Task::perform(
                    async move {
                        if paste {
                            use crate::platform::ClipboardContent;
                            return match crate::platform::read_clipboard().await? {
                                Some(ClipboardContent::Text(text)) => Ok(Some(text)),
                                Some(ClipboardContent::Png(png)) => {
                                    upload_image(&connection, png, "图片").await.map(Some)
                                }
                                None => Ok(None),
                            };
                        }
                        let Some(file) = rfd::AsyncFileDialog::new()
                            .add_filter("图片", &["png", "jpg", "jpeg", "webp"])
                            .pick_file()
                            .await
                        else {
                            return Ok(None);
                        };
                        let path = file.path();
                        if tokio::fs::metadata(path)
                            .await
                            .map_err(|e| e.to_string())?
                            .len()
                            > MAX_IMAGE as u64
                        {
                            return Err("请使用 6 MB 以内的 PNG、JPEG 或 WebP 图片".into());
                        }
                        let bytes = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
                        let name = path.file_stem().unwrap_or_default().to_string_lossy();
                        upload_image(&connection, bytes, &name).await.map(Some)
                    },
                    move |result| Message::Insert(id.clone(), cursor, result),
                );
            }
            Message::Insert(id, cursor, result) => {
                self.busy = false;
                match result {
                    Ok(Some(text)) => {
                        if let Some(draft) = self.drafts.get_mut(&id) {
                            draft.dirty = true;
                            draft.body.move_to(cursor);
                            draft.body.perform(text_editor::Action::Edit(
                                text_editor::Edit::Paste(Arc::new(text)),
                            ));
                        }
                    }
                    Ok(None) => {}
                    Err(error) => self.document_error = error,
                }
            }
            Message::Image(id, result) => match result {
                Ok(handle) => {
                    self.images.insert(id, Some(handle));
                }
                Err(error) => {
                    self.images.remove(&id);
                    self.document_error = error;
                }
            },
            Message::OpenLink(url) => {
                if (url.starts_with("https://") || url.starts_with("http://"))
                    && let Err(error) = open::that(url)
                {
                    self.document_error = error.to_string();
                }
            }
        }
        Task::none()
    }

    fn select(&mut self, id: String, anchor: Option<Point>) {
        self.selected = Some(id);
        self.anchor = anchor;
        self.document_open = true;
        self.document_error.clear();
        self.close_picker();
    }
    fn close_picker(&mut self) {
        self.picking = false;
        self.relation_query.clear();
        self.options.clear();
        if let Some(task) = self.relation_search.take() {
            task.abort();
        }
    }
    fn draft(&self) -> Option<&Draft> {
        self.drafts.get(self.selected.as_deref()?)
    }
    fn draft_mut(&mut self) -> Option<&mut Draft> {
        self.drafts.get_mut(self.selected.as_deref()?)
    }
    fn search(&mut self, connection: &Connection, delay: Duration) -> Task<Message> {
        if let Some(task) = self.search.take() {
            task.abort();
        }
        let query = self.query.clone();
        let connection = connection.clone();
        let (task, handle) = Task::perform(
            async move {
                let result = list(&connection, &query, delay).await;
                Message::Listed(query, result)
            },
            std::convert::identity,
        )
        .abortable();
        self.search = Some(handle);
        task
    }
    fn search_relations(&mut self, connection: &Connection, delay: Duration) -> Task<Message> {
        if let Some(task) = self.relation_search.take() {
            task.abort();
        }
        let query = self.relation_query.clone();
        let connection = connection.clone();
        let (task, handle) = Task::perform(
            async move {
                let result = list(&connection, &query, delay).await;
                Message::Options(query, result)
            },
            std::convert::identity,
        )
        .abortable();
        self.relation_search = Some(handle);
        task
    }

    fn load_document(&mut self, connection: &Connection) -> Task<Message> {
        let Some(id) = self
            .selected
            .as_ref()
            .filter(|id| self.document_open && !id.is_empty() && !self.busy)
        else {
            return Task::none();
        };
        if self.draft().is_some_and(|draft| {
            draft.dirty
                || !self
                    .documents
                    .iter()
                    .any(|doc| doc.id == *id && doc.revision > draft.document.revision)
        }) {
            return self.load_images(connection);
        }
        if self.reading.as_ref() == Some(id) {
            return Task::none();
        }
        self.reading = Some(id.clone());
        let id = id.clone();
        let connection = connection.clone();
        Task::perform(
            async move {
                let request = wire::WorkbenchRequest {
                    id: id.clone(),
                    ..Default::default()
                };
                let result = tokio::try_join!(
                    connection.rpc("workbench/knowledge-read", request.clone()),
                    connection.rpc("workbench/knowledge-related", request),
                );
                Message::Read(id, result)
            },
            std::convert::identity,
        )
    }
    fn load_images(&mut self, connection: &Connection) -> Task<Message> {
        let ids = self
            .draft()
            .into_iter()
            .flat_map(|draft| draft.rendered.images())
            .filter_map(|src| image_id(src))
            .filter(|id| !self.images.contains_key(*id))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        Task::batch(ids.into_iter().map(|id| {
            let connection = connection.clone();
            self.images.insert(id.clone(), None);
            Task::perform(
                async move {
                    let result = async {
                        let bytes = connection
                            .get_bytes(&format!("workbench/knowledge-image/{id}"))
                            .await?;
                        let rgba = ::image::load_from_memory(&bytes)
                            .map_err(|e| e.to_string())?
                            .into_rgba8();
                        Ok(picture::Handle::from_rgba(
                            rgba.width(),
                            rgba.height(),
                            rgba.into_raw(),
                        ))
                    }
                    .await;
                    Message::Image(id, result)
                },
                std::convert::identity,
            )
        }))
    }
    pub fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_secs(4)).map(|_| Message::Load)
    }
    pub fn document_anchor(&self) -> Option<Point> {
        self.anchor
    }

    pub fn document_open(&self) -> bool {
        self.document_open
    }

    pub fn escape(&self) -> Option<Message> {
        if self.picking {
            Some(Message::TogglePicker)
        } else {
            self.document_open.then_some(Message::CloseDocument)
        }
    }

    pub fn view(&self, locale: Locale) -> Element<'_, Message> {
        let search = row![
            Icon::Search.view(),
            text_input(locale.tr("搜索"), &self.query)
                .size(14)
                .padding([6, 5])
                .on_input(Message::Query)
                .on_submit_maybe(
                    self.search
                        .is_none()
                        .then(|| self.documents.first())
                        .flatten()
                        .map(|doc| Message::Select(doc.id.clone(), None))
                )
                .style(transparent_search),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let graph: Element<'_, Message> = if !self.loaded {
            container(text(locale.tr("读取中…")).size(13).style(style::muted_text))
                .center(Length::Fill)
                .into()
        } else if self.documents.is_empty() {
            container(
                text(if self.query.is_empty() {
                    locale.tr("知识库为空")
                } else {
                    locale.tr("没有匹配的文档")
                })
                .size(14)
                .style(style::muted_text),
            )
            .center(Length::Fill)
            .into()
        } else {
            canvas(graph::Network {
                graph: &self.graph,
                selected: self.selected.as_deref(),
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        };
        let mut body = column![
            row![
                text(locale.tr("知识库")).size(14),
                space::horizontal(),
                icon_button(
                    Icon::Plus,
                    locale.tr("新建知识文档"),
                    (!self.busy).then_some(Message::New)
                )
            ]
            .align_y(iced::Alignment::Center),
            search,
            rule::horizontal(1),
        ]
        .spacing(8);
        if !self.error.is_empty() {
            body = body.push(
                text(locale.tr(&self.error))
                    .size(13)
                    .style(style::error_text),
            );
        }
        container(body.push(graph))
            .padding(24)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub fn document_view(&self, locale: Locale) -> Option<Element<'_, Message>> {
        if !self.document_open {
            return None;
        }
        let draft = self.draft();
        let title = draft
            .map(|d| d.document.title.as_str())
            .filter(|title| !title.is_empty())
            .or_else(|| {
                self.documents
                    .iter()
                    .find(|d| Some(&d.id) == self.selected.as_ref())
                    .map(|doc| doc.title.as_str())
            })
            .unwrap_or(locale.tr("新建文档"));
        let mut actions = row![container(text(title).size(14)).width(Length::Fill)]
            .align_y(iced::Alignment::Center);
        if let Some(draft) = draft {
            if self.editing {
                actions = actions
                    .push(icon_button(
                        Icon::Image,
                        locale.tr("插入图片"),
                        (!self.busy).then_some(Message::PickImage),
                    ))
                    .push(icon_button(
                        Icon::Check,
                        locale.tr("保存文档"),
                        (!self.busy && draft.savable()).then_some(Message::Save),
                    ));
            } else {
                actions = actions.push(icon_button(
                    Icon::Edit,
                    locale.tr("编辑文档"),
                    Some(Message::Edit),
                ));
            }
            if !draft.document.id.is_empty() {
                actions = actions.push(icon_button(
                    Icon::Trash,
                    locale.tr("移除当前文档"),
                    (!self.busy).then_some(Message::Delete),
                ));
            }
        }
        let actions = actions.push(icon_button(
            Icon::Close,
            locale.tr("关闭文档"),
            Some(Message::CloseDocument),
        ));
        let mut body = column![].spacing(14);
        if !self.document_error.is_empty() {
            body = body.push(
                text(locale.tr(&self.document_error))
                    .size(13)
                    .style(style::error_text),
            );
        }
        if let Some(draft) = draft {
            if self.editing {
                if draft.dirty
                    && self
                        .documents
                        .iter()
                        .any(|d| d.id == draft.document.id && d.revision > draft.document.revision)
                {
                    body = body.push(
                        row![
                            text(locale.tr("文档已更新，当前草稿已保留"))
                                .size(13)
                                .style(style::muted_text),
                            space::horizontal(),
                            button(text(locale.tr("放弃草稿")).size(13))
                                .style(style::quiet)
                                .on_press_maybe((!self.busy).then_some(Message::Reload))
                        ]
                        .align_y(iced::Alignment::Center),
                    );
                }
                let can_save = !self.busy && draft.savable();
                let can_paste = !self.busy;
                body = body
                    .push(field(
                        locale.tr("标题"),
                        &draft.document.title,
                        Message::Title,
                        !self.busy,
                    ))
                    .push(
                        column![
                            text(locale.tr("正文")).size(12).style(style::muted_text),
                            text_editor(&draft.body)
                                .size(14)
                                .padding(12)
                                .height(272)
                                .on_action(Message::Body)
                                .key_binding(move |key| {
                                    let binding = text_editor::Binding::<Message>::from_key_press(
                                        key.clone(),
                                    );
                                    if key.modifiers.command()
                                        && key.key
                                            == keyboard::Key::Named(keyboard::key::Named::Enter)
                                    {
                                        return (can_save
                                            && matches!(
                                                key.status,
                                                text_editor::Status::Focused { .. }
                                            ))
                                        .then_some(text_editor::Binding::Custom(Message::Save));
                                    }
                                    if matches!(binding, Some(text_editor::Binding::Paste)) {
                                        return can_paste.then_some(text_editor::Binding::Custom(
                                            Message::Paste,
                                        ));
                                    }
                                    binding
                                })
                                .style(|theme, status| {
                                    let mut appearance = text_editor::default(theme, status);
                                    appearance.background = style::control_fill(theme).into();
                                    appearance.border.width = style::BORDER_WIDTH;
                                    appearance.border.radius = style::CONTROL_RADIUS.into();
                                    appearance
                                }),
                        ]
                        .spacing(6),
                    );
            } else {
                body = body.push(markdown::view_with(
                    draft.rendered.items(),
                    markdown::Settings::with_text_size(15, style::theme()),
                    &DocumentImages(&self.images, locale),
                ));
                body = body.push(self.relations(draft, locale));
            }
        } else if self.document_error.is_empty() {
            body = body.push(
                container(text(locale.tr("读取中…")).size(13).style(style::muted_text))
                    .center_x(Length::Fill)
                    .height(160),
            );
        }
        Some(
            container(column![actions, scrollable(body).height(Length::Fill)].spacing(16))
                .padding(24)
                .into(),
        )
    }

    fn relations<'a>(&'a self, draft: &'a Draft, locale: Locale) -> Element<'a, Message> {
        let mut content = column![
            rule::horizontal(1),
            row![
                text(locale.tr("关联文档")).size(14),
                space::horizontal(),
                icon_button(
                    if self.picking {
                        Icon::Close
                    } else {
                        Icon::Link
                    },
                    if self.picking {
                        locale.tr("收起关联选择")
                    } else {
                        locale.tr("关联文档")
                    },
                    (!self.busy).then_some(Message::TogglePicker)
                )
            ]
            .align_y(iced::Alignment::Center)
        ]
        .spacing(6);
        for related in &draft.related {
            content = content.push(
                button(
                    row![
                        text(&related.title).size(13),
                        space::horizontal(),
                        Icon::Right.view()
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .width(Length::Fill)
                .height(40)
                .style(style::quiet)
                .on_press(Message::Select(related.id.clone(), None)),
            );
        }
        if self.picking {
            content = content.push(
                row![
                    Icon::Search.view(),
                    text_input(locale.tr("搜索文档"), &self.relation_query)
                        .id("knowledge-relation-search")
                        .size(13)
                        .padding([8, 4])
                        .on_input(Message::RelationQuery)
                        .style(transparent_search)
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            );
            let mut options = column![].spacing(3);
            let mut count = 0;
            for option in self
                .options
                .iter()
                .filter(|option| option.id != draft.document.id)
            {
                count += 1;
                let linked = draft.document.related_ids.contains(&option.id);
                options = options.push(
                    button(
                        row![
                            text(&option.title).size(13),
                            space::horizontal(),
                            if linked { Icon::Check } else { Icon::Link }.view()
                        ]
                        .align_y(iced::Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(40)
                    .style(style::quiet)
                    .on_press_maybe((!self.busy).then(|| Message::Link(option.id.clone()))),
                );
            }
            if count == 0 {
                options = options.push(
                    text(if self.relation_search.is_some() {
                        locale.tr("读取中…")
                    } else {
                        locale.tr("没有可关联的文档")
                    })
                    .size(13)
                    .style(style::muted_text),
                );
            }
            content = content.push(options);
        }
        content.into()
    }
}

async fn list(
    connection: &Connection,
    query: &str,
    delay: Duration,
) -> Result<wire::KnowledgeList, String> {
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    connection
        .rpc(
            "workbench/knowledge",
            wire::WorkbenchRequest {
                query: query.into(),
                ..Default::default()
            },
        )
        .await
}

async fn upload_image(
    connection: &Connection,
    image: Vec<u8>,
    name: &str,
) -> Result<String, String> {
    if image.is_empty() || image.len() > MAX_IMAGE {
        return Err("请使用 6 MB 以内的 PNG、JPEG 或 WebP 图片".into());
    }
    let result: wire::KnowledgeImage = connection
        .rpc(
            "workbench/knowledge-image-upload",
            wire::KnowledgeImageUploadRequest { image },
        )
        .await?;
    let name = name.replace(['[', ']', '\\', '\r', '\n'], " ");
    let name = name.trim();
    Ok(format!(
        "\n\n![{}]({IMAGE_ROUTE}{})\n\n",
        if name.is_empty() { "图片" } else { name },
        result.id
    ))
}

fn image_id(url: &str) -> Option<&str> {
    let id = url.strip_prefix(IMAGE_ROUTE)?;
    uuid::Uuid::parse_str(id).ok()?;
    Some(id)
}
struct DocumentImages<'a>(&'a HashMap<String, Option<picture::Handle>>, Locale);
impl<'a> markdown::Viewer<'a, Message> for DocumentImages<'a> {
    fn on_link_click(url: markdown::Uri) -> Message {
        Message::OpenLink(url)
    }
    fn image(
        &self,
        _: markdown::Settings,
        url: &'a markdown::Uri,
        _: &'a str,
        _: &markdown::Text,
    ) -> Element<'a, Message> {
        match image_id(url).and_then(|id| self.0.get(id)) {
            Some(Some(handle)) => container(
                picture::Image::new(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Shrink)
                    .expand(false),
            )
            .max_height(390)
            .into(),
            Some(None) => text(self.1.tr("读取中…"))
                .size(13)
                .style(style::muted_text)
                .into(),
            None => space::vertical().height(0).into(),
        }
    }
}
fn transparent_search(theme: &iced::Theme, status: text_input::Status) -> text_input::Style {
    let mut appearance = style::input(theme, status);
    appearance.background = iced::Color::TRANSPARENT.into();
    appearance.border.width = 0.;
    appearance
}
