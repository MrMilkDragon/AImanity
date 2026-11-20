use crate::{
    connection::Connection,
    controls::{Icon, action_button, icon_button, sized_icon_button},
    style,
};
use ai_phone_api::wire::{self, FileLocation, FileOperation};
use iced::{
    Color, Element, Length, Padding, Task,
    widget::{
        button, checkbox, column, container, mouse_area, row, rule, scrollable, space, text,
        text_input,
    },
};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
};

const DEVICE_GAP: f32 = 16.;
const PANE_PADDING: f32 = 8.;
const TOOL_GAP: f32 = 4.;
const SCROLLBAR_WIDTH: f32 = 4.;
const SCROLLBAR_GAP: f32 = 2.;
const SIZE_COLUMN: f32 = 80.;
const MODIFIED_COLUMN: f32 = 112.;
const CHECK_COLUMN: f32 = 36.;
const GRIP_COLUMN: f32 = 24.;
const FILE_BORDER: Color = Color::from_rgb8(222, 222, 222);

fn file_divider(strength: f32) -> rule::Rule<'static> {
    rule::horizontal(style::BORDER_WIDTH).style(move |theme| {
        let mut appearance = rule::default(theme);
        appearance.color = FILE_BORDER.scale_alpha(strength);
        appearance
    })
}

fn table_cell(label: text::Text<'_>, width: Length) -> container::Container<'_, Message> {
    container(
        label
            .size(12)
            .style(style::muted_text)
            .wrapping(text::Wrapping::None),
    )
    .padding([0., PANE_PADDING])
    .width(width)
    .clip(true)
}

#[derive(Clone, Debug)]
pub enum Message {
    Load,
    Go(usize, String),
    Address(usize, String),
    Listed(usize, u64, Result<wire::FileListing, String>),
    Select(usize, String, bool),
    All(usize, bool),
    Clip(usize, FileOperation),
    Paste(usize),
    Drag(usize, String),
    Drop(usize, Option<String>),
    EndDrag,
    Edit(usize, FileOperation),
    Name(String),
    Confirm,
    Cancel,
    Pick(usize),
    Upload(FileLocation, Vec<PathBuf>),
    Download(Vec<FileLocation>),
    Finished(Option<FileOperation>, Result<Vec<wire::FileResult>, String>),
}

struct Pane {
    name: String,
    device: Option<String>,
    address: String,
    path: String,
    entries: Vec<wire::FileEntry>,
    selected: HashSet<String>,
    revision: u64,
    loading: bool,
    error: String,
}
impl Pane {
    fn new(name: &str, device: Option<String>) -> Self {
        Self {
            name: name.into(),
            device,
            path: "/".into(),
            address: "/".into(),
            entries: Vec::new(),
            selected: HashSet::new(),
            revision: 0,
            loading: false,
            error: String::new(),
        }
    }
    fn location(&self, path: &str) -> FileLocation {
        FileLocation {
            device_id: self.device.clone().unwrap_or_default(),
            path: path.into(),
        }
    }
    fn selection(&self) -> Vec<FileLocation> {
        self.entries
            .iter()
            .filter(|f| self.selected.contains(&f.path))
            .map(|f| self.location(&f.path))
            .collect()
    }
}
struct Edit {
    index: usize,
    operation: FileOperation,
    name: String,
    sources: Vec<FileLocation>,
}
pub struct Files {
    panes: [Pane; 3],
    clipboard: Option<(FileOperation, Vec<FileLocation>)>,
    drag: Option<(usize, Vec<FileLocation>)>,
    edit: Option<Edit>,
    queued_uploads: VecDeque<(FileLocation, Vec<PathBuf>)>,
    busy: bool,
    error: String,
}
impl Default for Files {
    fn default() -> Self {
        Self {
            panes: [
                Pane::new("工作文件", Some(String::new())),
                Pane::new("手机", None),
                Pane::new("电脑", None),
            ],
            clipboard: None,
            drag: None,
            edit: None,
            queued_uploads: VecDeque::new(),
            busy: false,
            error: String::new(),
        }
    }
}
impl Files {
    pub fn set_devices(&mut self, devices: &[Option<wire::RegisteredDevice>; 2]) {
        for (index, device) in devices.iter().enumerate() {
            let pane = &mut self.panes[index + 1];
            if let Some(device) = device {
                if pane.device.as_deref() != Some(device.id.as_str()) {
                    *pane = Pane::new(&device.name, Some(device.id.clone()));
                }
            } else {
                *pane = Pane::new(if index == 0 { "手机" } else { "电脑" }, None);
            }
        }
    }
    fn list(&mut self, i: usize, client: &Connection) -> Task<Message> {
        let pane = &mut self.panes[i];
        if pane.device.is_none() {
            return Task::none();
        }
        pane.loading = true;
        pane.error.clear();
        pane.revision += 1;
        let revision = pane.revision;
        let request = pane.location(&pane.path);
        let client = client.clone();
        Task::perform(
            async move { client.rpc("files/list", request).await },
            move |r| Message::Listed(i, revision, r),
        )
    }
    fn manage(
        &mut self,
        client: &Connection,
        operation: FileOperation,
        sources: Vec<FileLocation>,
        target: FileLocation,
        name: String,
    ) -> Task<Message> {
        if self.busy {
            return Task::none();
        }
        self.busy = true;
        self.error.clear();
        let client = client.clone();
        let request = wire::FileManageRequest {
            operation: operation.into(),
            sources,
            target: Some(target),
            name,
        };
        Task::perform(
            async move {
                client
                    .rpc::<_, wire::FileManageResult>("files/manage", request)
                    .await
                    .map(|reply| reply.results)
            },
            move |r| Message::Finished(Some(operation), r),
        )
    }
    pub fn local_drop(
        &self,
        point: iced::Point,
        bounds: iced::Rectangle,
        path: PathBuf,
    ) -> Message {
        let index = if point.y < bounds.y + 74. + ((bounds.height - 120.) / 2.).clamp(170., 300.) {
            0
        } else if point.x < bounds.center_x() {
            1
        } else {
            2
        };
        Message::Upload(
            self.panes[index].location(&self.panes[index].path),
            vec![path],
        )
    }
    pub fn update(&mut self, message: Message, client: &Connection) -> Task<Message> {
        match message {
            Message::Load => return Task::batch((0..3).map(|i| self.list(i, client))),
            Message::Go(i, path) => {
                self.panes[i].address = path.clone();
                self.panes[i].path = path;
                self.panes[i].selected.clear();
                return self.list(i, client);
            }
            Message::Address(i, value) => self.panes[i].address = value,
            Message::Listed(i, revision, result) => {
                let pane = &mut self.panes[i];
                if revision != pane.revision {
                    return Task::none();
                }
                pane.loading = false;
                match result {
                    Ok(result) => {
                        if let Some(location) = result.location {
                            pane.address = location.path.clone();
                            pane.path = location.path;
                        }
                        pane.entries = result.entries;
                        pane.selected
                            .retain(|path| pane.entries.iter().any(|e| &e.path == path));
                    }
                    Err(e) => {
                        pane.entries.clear();
                        pane.selected.clear();
                        pane.error = e;
                    }
                }
            }
            Message::Select(i, path, on) => {
                if on {
                    self.panes[i].selected.insert(path);
                } else {
                    self.panes[i].selected.remove(&path);
                }
            }
            Message::All(i, on) => {
                let pane = &mut self.panes[i];
                pane.selected = if on {
                    pane.entries.iter().map(|f| f.path.clone()).collect()
                } else {
                    HashSet::new()
                };
            }
            Message::Clip(i, op) => self.clipboard = Some((op, self.panes[i].selection())),
            Message::Paste(i) => {
                if let Some((op, sources)) = self.clipboard.clone() {
                    return self.manage(
                        client,
                        op,
                        sources,
                        self.panes[i].location(&self.panes[i].path),
                        String::new(),
                    );
                }
            }
            Message::Drag(i, path) => {
                let pane = &self.panes[i];
                let sources = if pane.selected.contains(&path) {
                    pane.selection()
                } else {
                    vec![pane.location(&path)]
                };
                self.drag = Some((i, sources));
            }
            Message::Drop(i, path) => {
                if let Some((from, sources)) = self.drag.take()
                    && (from != i || path.is_some())
                {
                    let pane = &self.panes[i];
                    if pane.device.is_some() {
                        let target = pane.location(path.as_deref().unwrap_or(&pane.path));
                        return self.manage(
                            client,
                            FileOperation::Copy,
                            sources,
                            target,
                            String::new(),
                        );
                    }
                }
            }
            Message::EndDrag => self.drag = None,
            Message::Edit(i, operation) => {
                let sources = if operation == FileOperation::Mkdir {
                    Vec::new()
                } else {
                    self.panes[i].selection()
                };
                let name = if operation == FileOperation::Rename {
                    leaf(&sources[0].path).into()
                } else {
                    String::new()
                };
                self.edit = Some(Edit {
                    index: i,
                    operation,
                    name,
                    sources,
                });
            }
            Message::Name(name) => {
                if let Some(edit) = &mut self.edit {
                    edit.name = name;
                }
            }
            Message::Cancel => self.edit = None,
            Message::Confirm => {
                if let Some(edit) = &self.edit {
                    let pane = &self.panes[edit.index];
                    return self.manage(
                        client,
                        edit.operation,
                        edit.sources.clone(),
                        pane.location(&pane.path),
                        edit.name.clone(),
                    );
                }
            }
            Message::Pick(i) => {
                let target = self.panes[i].location(&self.panes[i].path);
                return Task::perform(rfd::AsyncFileDialog::new().pick_files(), move |files| {
                    Message::Upload(
                        target.clone(),
                        files
                            .unwrap_or_default()
                            .iter()
                            .map(|f| f.path().to_owned())
                            .collect(),
                    )
                });
            }
            Message::Upload(target, paths) => {
                if paths.is_empty() {
                    return Task::none();
                }
                if self.busy {
                    self.queued_uploads.push_back((target, paths));
                    return Task::none();
                }
                self.busy = true;
                self.error.clear();
                let client = client.clone();
                return Task::perform(
                    async move {
                        for path in paths {
                            let source = client
                                .upload_file(
                                    &path,
                                    if target.device_id.is_empty() {
                                        target.path.clone()
                                    } else {
                                        "/".into()
                                    },
                                )
                                .await?;
                            if !target.device_id.is_empty() {
                                copy_file(
                                    &client,
                                    source,
                                    target.clone(),
                                    path.file_name()
                                        .ok_or("文件名为空")?
                                        .to_string_lossy()
                                        .into_owned(),
                                )
                                .await?;
                            }
                        }
                        Ok(Vec::new())
                    },
                    |r| Message::Finished(None, r),
                );
            }
            Message::Download(sources) => {
                if self.busy {
                    return Task::none();
                }
                self.busy = true;
                self.error.clear();
                let client = client.clone();
                return Task::perform(
                    async move {
                        let dialog = rfd::AsyncFileDialog::new();
                        let targets = if sources.len() == 1 {
                            let Some(file) = dialog
                                .set_file_name(leaf(&sources[0].path))
                                .save_file()
                                .await
                            else {
                                return Ok(vec![]);
                            };
                            vec![file.path().to_owned()]
                        } else {
                            let Some(folder) = dialog.pick_folder().await else {
                                return Ok(vec![]);
                            };
                            sources
                                .iter()
                                .map(|source| folder.path().join(leaf(&source.path)))
                                .collect()
                        };
                        for (source, target) in sources.into_iter().zip(targets) {
                            let path = if source.device_id.is_empty() {
                                source.path
                            } else {
                                let name = leaf(&source.path);
                                let suffix = uuid::Uuid::now_v7().to_string();
                                let name = match name.rsplit_once('.') {
                                    Some((stem, ext)) => {
                                        format!("{stem} ({}).{ext}", &suffix[24..])
                                    }
                                    None => format!("{name} ({})", &suffix[24..]),
                                };
                                copy_file(
                                    &client,
                                    source,
                                    FileLocation {
                                        path: "/".into(),
                                        ..Default::default()
                                    },
                                    name,
                                )
                                .await?
                                .path
                            };
                            client.download_file(&path, &target).await?;
                        }
                        Ok(Vec::new())
                    },
                    |r| Message::Finished(None, r),
                );
            }
            Message::Finished(operation, result) => {
                self.busy = false;
                match result {
                    Ok(results) => {
                        let failures: Vec<_> = results
                            .into_iter()
                            .filter(|r| !r.error.is_empty())
                            .collect();
                        self.error = failures
                            .iter()
                            .map(|r| r.error.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        if operation == Some(FileOperation::Move) {
                            self.clipboard = (!failures.is_empty()).then(|| {
                                (
                                    FileOperation::Move,
                                    failures.into_iter().filter_map(|r| r.source).collect(),
                                )
                            });
                        }
                        if self.error.is_empty() {
                            self.edit = None;
                        }
                    }
                    Err(e) => self.error = e,
                }
                if self.error.is_empty()
                    && let Some((target, paths)) = self.queued_uploads.pop_front()
                {
                    return self.update(Message::Upload(target, paths), client);
                }
                self.queued_uploads.clear();
                return self.update(Message::Load, client);
            }
        }
        Task::none()
    }
    pub fn view(&self, height: f32, locale: crate::locale::Locale) -> Element<'_, Message> {
        let mut body = column![].spacing(16);
        if self.busy || !self.error.is_empty() {
            body = body.push(
                text(if self.busy {
                    locale.tr("正在处理文件…")
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
        let workspace_height = (height * 0.36).max(150.);
        let devices_height = (height - workspace_height - DEVICE_GAP).max(150.);
        body = body.push(self.pane(0, workspace_height, locale)).push(
            row![
                self.pane(1, devices_height, locale),
                self.pane(2, devices_height, locale)
            ]
            .spacing(DEVICE_GAP),
        );
        if let Some(edit) = &self.edit {
            let title = match edit.operation {
                FileOperation::Mkdir => locale.tr("新建文件夹"),
                FileOperation::Rename => locale.tr("重命名"),
                _ => locale.tr("删除选中的文件？"),
            };
            let delete = edit.operation == FileOperation::Delete;
            let mut form = row![text(title).size(13)].spacing(8).align_y(iced::Center);
            if !delete {
                form = form.push(
                    text_input(locale.tr("名称"), &edit.name)
                        .style(style::input)
                        .on_input(Message::Name)
                        .on_submit(Message::Confirm),
                );
            }
            body = body.push(
                form.push(
                    action_button(
                        locale.tr("确认"),
                        (!self.busy && (delete || !edit.name.trim().is_empty()))
                            .then_some(Message::Confirm),
                    )
                    .width(70),
                )
                .push(icon_button(Icon::Close, Some(Message::Cancel))),
            );
        }
        body.into()
    }
    fn pane(&self, i: usize, height: f32, locale: crate::locale::Locale) -> Element<'_, Message> {
        let pane = &self.panes[i];
        let enabled = !self.busy && pane.device.is_some();
        let selected = !pane.selected.is_empty();
        let control = |icon, message| sized_icon_button(icon, message, 36., 36.);
        let op = |icon, message| control(icon, enabled.then_some(message));
        let device_icon = [Icon::Folder, Icon::PhonePortrait, Icon::Screen][i];
        let header = row![
            device_icon.view().width(16).height(16),
            container(style::section_title(locale.tr(&pane.name)).wrapping(text::Wrapping::None))
                .clip(true),
            space::horizontal(),
            op(Icon::Refresh, Message::Go(i, pane.path.clone())),
            op(Icon::Upload, Message::Pick(i)),
            op(Icon::Plus, Message::Edit(i, FileOperation::Mkdir))
        ]
        .spacing(PANE_PADDING)
        .align_y(iced::Center);
        let mut tools = row![].spacing(TOOL_GAP).align_y(iced::Center);
        if selected {
            let sources = pane.selection();
            let download = pane
                .entries
                .iter()
                .filter(|e| pane.selected.contains(&e.path))
                .all(|e| !e.directory);
            tools = tools
                .push(container(text(pane.selected.len().to_string()).size(12)).padding([0, 4]))
                .push(op(Icon::Copy, Message::Clip(i, FileOperation::Copy)))
                .push(op(Icon::Cut, Message::Clip(i, FileOperation::Move)))
                .push(control(
                    Icon::Download,
                    (enabled && download).then_some(Message::Download(sources)),
                ))
                .push(control(
                    Icon::Edit,
                    (enabled && pane.selected.len() == 1)
                        .then_some(Message::Edit(i, FileOperation::Rename)),
                ))
                .push(op(Icon::Trash, Message::Edit(i, FileOperation::Delete)))
                .push(op(Icon::Close, Message::All(i, false)));
        } else {
            let parent = pane.path.rsplit_once('/').map_or("/", |(parent, _)| {
                if parent.is_empty() { "/" } else { parent }
            });
            tools = tools
                .push(control(
                    Icon::Up,
                    (enabled && pane.path != "/").then(|| Message::Go(i, parent.into())),
                ))
                .push(
                    text_input(locale.tr("路径"), &pane.address)
                        .on_input_maybe(enabled.then_some(move |v| Message::Address(i, v)))
                        .on_submit_maybe(enabled.then(|| Message::Go(i, pane.address.clone())))
                        .style(|theme, status| {
                            let mut appearance = style::input(theme, status);
                            appearance.background = Color::TRANSPARENT.into();
                            appearance.border.width = 0.;
                            appearance
                        })
                        .size(12)
                        .padding([9, 4]),
                );
        }
        if self.clipboard.is_some() {
            tools = tools.push(op(Icon::Link, Message::Paste(i)));
        }
        let table_header = row![
            container(
                checkbox(!pane.entries.is_empty() && pane.entries.len() == pane.selected.len())
                    .size(15)
                    .on_toggle_maybe(
                        (enabled && !pane.entries.is_empty())
                            .then_some(move |b| Message::All(i, b))
                    )
            )
            .center_x(CHECK_COLUMN),
            table_cell(text(locale.tr("名称")), Length::Fill),
            table_cell(text(locale.tr("大小")), Length::Fixed(SIZE_COLUMN)),
            table_cell(text(locale.tr("修改时间")), Length::Fixed(MODIFIED_COLUMN)),
            space::horizontal().width(GRIP_COLUMN + SCROLLBAR_WIDTH + SCROLLBAR_GAP)
        ]
        .align_y(iced::Center);
        let mut listing = column![];
        if pane.loading || !pane.error.is_empty() || pane.entries.is_empty() {
            listing = listing.push(
                container(
                    text(if pane.loading {
                        locale.tr("读取中…")
                    } else if !pane.error.is_empty() {
                        &pane.error
                    } else if pane.device.is_none() {
                        locale.tr("尚未安装设备")
                    } else {
                        locale.tr("暂无文件")
                    })
                    .size(12)
                    .style(style::muted_text),
                )
                .padding([20, 12])
                .center_x(Length::Fill),
            );
        } else {
            for entry in &pane.entries {
                let path = entry.path.clone();
                let browse = entry.directory || entry.symlink;
                let checked = pane.selected.contains(&path);
                let activate = if browse {
                    Message::Go(i, path.clone())
                } else {
                    Message::Download(vec![pane.location(&path)])
                };
                let icon = if browse { Icon::Folder } else { Icon::Right };
                let name = button(
                    row![
                        icon.view().width(16).height(16),
                        text(&entry.name).size(12).wrapping(text::Wrapping::None)
                    ]
                    .height(style::CONTROL_HEIGHT - style::BORDER_WIDTH)
                    .align_y(iced::Center)
                    .spacing(PANE_PADDING),
                )
                .width(Length::Fill)
                .padding([0., PANE_PADDING])
                .clip(true)
                .on_press_maybe(enabled.then_some(activate))
                .style(style::quiet);
                let handle =
                    mouse_area(container(Icon::Grip.view()).center(Length::Fixed(GRIP_COLUMN)))
                        .on_press(Message::Drag(i, path.clone()))
                        .interaction(iced::mouse::Interaction::Grab);
                let entry_row = row![
                    container(checkbox(checked).size(15).on_toggle_maybe(enabled.then(
                        move || {
                            let path = path.clone();
                            move |b| Message::Select(i, path.clone(), b)
                        }
                    )))
                    .center_x(CHECK_COLUMN),
                    name,
                    table_cell(
                        text(if browse {
                            "—".into()
                        } else {
                            size_label(entry.size_bytes)
                        }),
                        Length::Fixed(SIZE_COLUMN)
                    ),
                    table_cell(
                        text(timestamp(entry.modified_at_ms)),
                        Length::Fixed(MODIFIED_COLUMN)
                    ),
                    handle
                ]
                .align_y(iced::Center);
                let entry_row =
                    container(column![entry_row, file_divider(0.55)]).style(move |_| {
                        let appearance = container::Style::default();
                        if checked {
                            appearance.background(style::ACCENT)
                        } else {
                            appearance
                        }
                    });
                if browse && enabled && self.drag.is_some() {
                    listing = listing.push(
                        mouse_area(entry_row)
                            .on_release(Message::Drop(i, Some(entry.path.clone()))),
                    );
                } else {
                    listing = listing.push(entry_row);
                }
            }
        }
        let content = container(column![
            container(header)
                .padding([0., PANE_PADDING])
                .height(style::CONTROL_HEIGHT - style::BORDER_WIDTH)
                .align_y(iced::Center),
            file_divider(1.),
            container(tools)
                .padding([0., PANE_PADDING])
                .height(style::CONTROL_HEIGHT - style::BORDER_WIDTH)
                .align_y(iced::Center),
            file_divider(1.),
            container(table_header)
                .width(Length::Fill)
                .height(32)
                .align_y(iced::Center)
                .style(|_| container::Style::default().background(Color::from_rgb8(242, 242, 242))),
            scrollable(
                container(listing).padding(Padding::ZERO.right(SCROLLBAR_WIDTH + SCROLLBAR_GAP)),
            )
            .height(Length::Fill)
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new()
                    .width(SCROLLBAR_WIDTH)
                    .scroller_width(SCROLLBAR_WIDTH),
            ))
        ])
        .width(Length::Fill)
        .height(height)
        .clip(true)
        .style(|_| {
            container::Style::default()
                .background(Color::WHITE.scale_alpha(0.18))
                .border(iced::Border {
                    width: style::BORDER_WIDTH,
                    color: FILE_BORDER,
                    radius: style::CONTROL_RADIUS.into(),
                })
        });
        if enabled && self.drag.is_some() {
            mouse_area(content)
                .on_release(Message::Drop(i, None))
                .into()
        } else {
            content.into()
        }
    }
}
fn leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or_default()
}
fn size_label(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / 1073741824.)
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1048576.)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{bytes} B")
    }
}
fn timestamp(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .filter(|_| ms > 0)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "—".into())
}
async fn copy_file(
    client: &Connection,
    source: FileLocation,
    target: FileLocation,
    name: String,
) -> Result<FileLocation, String> {
    let response: wire::FileManageResult = client
        .rpc(
            "files/manage",
            wire::FileManageRequest {
                operation: FileOperation::Copy.into(),
                sources: vec![source],
                target: Some(target),
                name,
            },
        )
        .await?;
    let item = response
        .results
        .into_iter()
        .next()
        .ok_or("服务未返回文件回执")?;
    if !item.error.is_empty() {
        return Err(item.error);
    }
    item.target.ok_or_else(|| "服务未返回目标文件".into())
}
