use crate::{
    connection::Connection,
    controls::{Icon, action_button, form_input, form_select, icon_button, sized_icon_button},
    style,
};
use ai_phone_api::wire::{self, FileLocation, FileOperation};
use iced::{
    Color, Element, Length, Padding, Rectangle, Task,
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
    Choose(usize, String),
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceChoice {
    id: String,
    label: String,
    phone: bool,
}
impl std::fmt::Display for DeviceChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
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
    devices: Vec<DeviceChoice>,
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
            devices: Vec::new(),
            panes: [
                Pane::new("工作文件", Some(String::new())),
                Pane::new("", None),
                Pane::new("", None),
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
    pub fn set_devices(&mut self, devices: &[wire::RegisteredDevice]) -> Vec<usize> {
        let mut names = std::collections::HashMap::<&str, usize>::new();
        self.devices = devices
            .iter()
            .filter(|d| crate::connection::reachable(d))
            .map(|d| {
                let ordinal = names.entry(&d.name).or_default();
                *ordinal += 1;
                let label = if *ordinal > 1 {
                    format!("{} · {ordinal}", d.name)
                } else {
                    d.name.clone()
                };
                DeviceChoice {
                    id: d.id.clone(),
                    label,
                    phone: d.kind == "phone",
                }
            })
            .collect();
        let current = |i: usize| {
            self.devices
                .iter()
                .find(|d| Some(&d.id) == self.panes[i].device.as_ref())
                .map(|d| d.id.clone())
        };
        let mut left = current(1);
        let mut right = current(2);
        if left.is_none() {
            left = self
                .devices
                .iter()
                .find(|d| Some(&d.id) != right.as_ref())
                .map(|d| d.id.clone())
                .or_else(|| right.take());
        }
        if right.is_none() {
            right = self
                .devices
                .iter()
                .find(|d| Some(&d.id) != left.as_ref())
                .map(|d| d.id.clone());
        }
        [(1, left), (2, right)]
            .into_iter()
            .filter_map(|(i, id)| self.bind(i, id).then_some(i))
            .collect()
    }
    fn bind(&mut self, i: usize, id: Option<String>) -> bool {
        let name = self
            .devices
            .iter()
            .find(|d| Some(&d.id) == id.as_ref())
            .map_or("", |d| &d.label);
        let pane = &mut self.panes[i];
        if pane.device == id {
            pane.name = name.into();
            return false;
        }
        let revision = pane.revision + 1;
        *pane = Pane {
            revision,
            ..Pane::new(name, id)
        };
        if self.edit.as_ref().is_some_and(|e| e.index == i) {
            self.edit = None;
        }
        if self.drag.as_ref().is_some_and(|(from, _)| *from == i) {
            self.drag = None;
        }
        true
    }
    pub(super) fn list(&mut self, i: usize, client: &Connection) -> Task<Message> {
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
    fn pane_regions(&self, body: Rectangle) -> [Rectangle; 3] {
        let notice = if self.busy || !self.error.is_empty() {
            style::FIELD_LINE.0 + DEVICE_GAP
        } else {
            0.
        };
        let footer = if self.edit.is_some() {
            style::CONTROL_HEIGHT + DEVICE_GAP
        } else {
            0.
        };
        let count = self.panes[1..]
            .iter()
            .filter(|p| p.device.is_some())
            .count();
        let gap = if count == 0 { 0. } else { DEVICE_GAP };
        let available = (body.height - notice - footer - gap).max(0.);
        let workspace = Rectangle {
            y: body.y + notice,
            height: if count == 0 {
                available
            } else {
                (available * 0.36).round()
            },
            ..body
        };
        let device = Rectangle {
            x: body.x,
            y: workspace.y + workspace.height + DEVICE_GAP,
            width: if count == 2 {
                (body.width - DEVICE_GAP) / 2.
            } else {
                body.width
            },
            height: available - workspace.height,
        };
        [
            workspace,
            device,
            Rectangle {
                x: device.x + device.width + DEVICE_GAP,
                ..device
            },
        ]
    }

    pub fn local_drop(
        &self,
        point: iced::Point,
        body: Rectangle,
        path: PathBuf,
    ) -> Option<Message> {
        let index = self
            .pane_regions(body)
            .iter()
            .position(|region| region.contains(point))?;
        self.panes[index].device.as_ref()?;
        Some(Message::Upload(
            self.panes[index].location(&self.panes[index].path),
            vec![path],
        ))
    }
    pub fn update(&mut self, message: Message, client: &Connection) -> Task<Message> {
        match message {
            Message::Load => return Task::batch((0..3).map(|i| self.list(i, client))),
            Message::Choose(i, id) => {
                if !(1..=2).contains(&i) || !self.devices.iter().any(|d| d.id == id) || self.busy {
                    return Task::none();
                }
                let other = 3 - i;
                if self.panes[other].device.as_ref() == Some(&id) {
                    let revision = self.panes[i].revision.max(self.panes[other].revision);
                    self.panes.swap(i, other);
                    for slot in [i, other] {
                        self.panes[slot].revision = revision;
                    }
                    self.edit = None;
                    self.drag = None;
                    return Task::batch([self.list(i, client), self.list(other, client)]);
                }
                if self.bind(i, Some(id)) {
                    return self.list(i, client);
                }
            }
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
                return Task::perform(
                    crate::documents::AsyncFileDialog::new().pick_files(),
                    move |files| {
                        Message::Upload(
                            target.clone(),
                            files
                                .unwrap_or_default()
                                .iter()
                                .map(|f| f.path().to_owned())
                                .collect(),
                        )
                    },
                );
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
                        let names = sources
                            .iter()
                            .map(|source| leaf(&source.path).to_owned())
                            .collect::<Vec<_>>();
                        let targets = crate::documents::download_targets(&names).await?;
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
                            crate::documents::download(&client, &path, &target).await?;
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
    pub fn view(&self, bounds: Rectangle, locale: crate::locale::Locale) -> Element<'_, Message> {
        let mut body = column![].spacing(DEVICE_GAP);
        if self.busy || !self.error.is_empty() {
            body = body.push(
                text(if self.busy {
                    locale.tr("正在处理文件…")
                } else {
                    &self.error
                })
                .size(12)
                .line_height(style::FIELD_LINE)
                .wrapping(text::Wrapping::None)
                .color(if self.busy {
                    style::MUTED
                } else {
                    style::ERROR
                }),
            );
        }
        let regions = self.pane_regions(bounds);
        body = body.push(self.pane(0, regions[0].height, locale));
        if self.panes[1].device.is_some() {
            let mut devices = row![self.pane(1, regions[1].height, locale)].spacing(DEVICE_GAP);
            if self.panes[2].device.is_some() {
                devices = devices.push(self.pane(2, regions[2].height, locale));
            }
            body = body.push(devices);
        }
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
                    form_input(locale.tr("名称"), &edit.name)
                        .on_input(Message::Name)
                        .on_submit(Message::Confirm),
                );
            }
            body = body.push(
                form.push(action_button(
                    locale.tr("确认"),
                    (!self.busy && (delete || !edit.name.trim().is_empty()))
                        .then_some(Message::Confirm),
                ))
                .push(icon_button(Icon::Close, Some(Message::Cancel))),
            );
        }
        body.into()
    }
    fn pane(&self, i: usize, height: f32, locale: crate::locale::Locale) -> Element<'_, Message> {
        let pane = &self.panes[i];
        let enabled = !self.busy && pane.device.is_some();
        let selected = !pane.selected.is_empty();
        let control = |icon, message| {
            sized_icon_button(
                icon,
                message,
                style::COMPACT_CONTROL,
                style::COMPACT_CONTROL,
            )
        };
        let op = |icon, message| control(icon, enabled.then_some(message));
        let selected_device = self
            .devices
            .iter()
            .find(|d| Some(&d.id) == pane.device.as_ref());
        let icon = match selected_device {
            Some(d) if d.phone => Icon::PhonePortrait,
            Some(_) => Icon::Screen,
            None => Icon::Folder,
        };
        let title: Element<'_, Message> = if i != 0 && self.devices.len() >= 3 && !self.busy {
            form_select(
                self.devices.as_slice(),
                selected_device,
                move |d: DeviceChoice| Message::Choose(i, d.id),
            )
            .width(Length::Fill)
            .into()
        } else {
            style::section_title(if i == 0 {
                locale.tr("工作文件")
            } else {
                &pane.name
            })
            .wrapping(text::Wrapping::None)
            .into()
        };
        let header = row![
            icon.view().width(16).height(16),
            container(title).width(Length::Fill).clip(true),
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
                        .line_height(style::FIELD_LINE)
                        .padding([8, 4]),
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

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Point;

    #[tokio::test]
    async fn selecting_a_device_keeps_panes_distinct_and_discards_its_previous_listing() {
        let root = tempfile::tempdir().unwrap();
        let client = Connection::connect(root.path().to_str().unwrap().to_owned())
            .await
            .unwrap()
            .client;
        let mut files = Files::default();
        let devices = ["a", "b", "c"].map(|id| wire::RegisteredDevice {
            id: id.into(),
            name: "Studio".into(),
            paired: true,
            connected: true,
            ..Default::default()
        });
        let ids = |f: &Files| [1, 2].map(|i| f.panes[i].device.clone());
        assert_eq!(ids(&files), [None, None]);
        files.set_devices(&devices[..1]);
        assert_eq!(ids(&files), [Some("a".into()), None]);
        files.set_devices(&devices[..2]);
        assert_eq!(ids(&files), [Some("a".into()), Some("b".into())]);
        files.set_devices(&devices);
        assert_eq!(files.devices[2].to_string(), "Studio · 3");
        let old = files.panes[1].revision;
        let _ = files.update(Message::Choose(1, "c".into()), &client);
        let _ = files.update(Message::Listed(1, old, Err("stale".into())), &client);
        assert!(files.panes[1].error.is_empty());
        assert_eq!(ids(&files), [Some("c".into()), Some("b".into())]);
        files.panes[1].path = "/Projects".into();
        let _ = files.update(Message::Choose(2, "c".into()), &client);
        assert_eq!(files.panes[2].path, "/Projects");
        assert_eq!(ids(&files), [Some("b".into()), Some("c".into())]);
        assert!(
            files
                .set_devices(&[devices[1].clone(), devices[2].clone()])
                .is_empty()
        );
        files.set_devices(&[]);
        assert_eq!(ids(&files), [None, None]);
        assert_eq!(files.panes[0].device.as_deref(), Some(""));
        client.workspace.stop().await;
    }

    #[test]
    fn editing_reserves_the_footer_and_keeps_drops_in_the_displayed_pane() {
        let mut files = Files::default();
        files.panes[1].device = Some("phone".into());
        files.panes[2].device = Some("desktop".into());
        files.edit = Some(Edit {
            index: 0,
            operation: FileOperation::Mkdir,
            name: "Example".into(),
            sources: vec![],
        });
        let body = Rectangle {
            x: 24.,
            y: 80.,
            width: 640.,
            height: 420.,
        };
        let [work, phone, computer] = files.pane_regions(body);
        assert_eq!(work.y, body.y);
        assert_eq!(work.y + work.height + DEVICE_GAP, phone.y);
        assert_eq!(phone.y, computer.y);
        assert_eq!(phone.height, computer.height);
        assert_eq!(computer.x + computer.width, body.x + body.width);
        assert_eq!(
            phone.y + phone.height + DEVICE_GAP + style::CONTROL_HEIGHT,
            body.y + body.height
        );

        let drop_at = |point| files.local_drop(point, body, PathBuf::from("example.txt"));
        for (point, device) in [(work.center(), ""), (phone.center(), "phone")] {
            let Some(Message::Upload(target, _)) = drop_at(point) else {
                panic!("visible pane did not receive the drop")
            };
            assert_eq!(target.device_id, device);
        }
        assert!(
            matches!(drop_at(computer.center()), Some(Message::Upload(loc,_)) if loc.device_id=="desktop")
        );
        assert!(
            drop_at(Point::new(
                phone.x + phone.width + DEVICE_GAP / 2.,
                phone.center_y()
            ))
            .is_none()
        );
        assert!(drop_at(Point::new(work.center_x(), body.y + body.height - 1.)).is_none());
    }
}
