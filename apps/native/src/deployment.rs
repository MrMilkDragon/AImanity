use crate::{controls::Icon, host::Host, locale::Locale, spring_slider::SpringSlider, style};
use iced::{
    Element, Length,
    widget::{button, column, container, float, image, row, rule, space, stack, text},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::LazyLock,
    time::{Duration, Instant},
};

#[path = "host_memory.rs"]
mod host_memory;

const GIB: u64 = 1024_u64.pow(3);
const SETUP_CONTROL: f32 = 44.;
const SETUP_LINE: iced::Color = iced::Color::from_rgb8(222, 222, 222);
const SETUP_TITLE: iced::Font = iced::Font {
    weight: iced::font::Weight::Semibold,
    ..iced::Font::DEFAULT
};

fn line() -> rule::Rule<'static> {
    rule::horizontal(1).style(|_| rule::Style {
        color: SETUP_LINE,
        radius: 0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    })
}

pub fn glass_background<'a, M: 'a>(content: Element<'a, M>) -> Element<'a, M> {
    static BACKDROP: LazyLock<image::Handle> = LazyLock::new(|| {
        let pixels: Vec<u8> = (0..64)
            .flat_map(|y: u8| {
                (0..64).flat_map(move |x: u8| {
                    let dx = (f32::from(x) / 63. - 0.15) / 0.85;
                    let dy = (f32::from(y) / 63. - 0.05) / 0.95;
                    let opacity = style::GLASS_OPACITY
                        + (1. - style::GLASS_OPACITY)
                            * (1. - dx.hypot(dy) / (std::f32::consts::SQRT_2 * 0.68)).max(0.);
                    [233_u8, 236, 238]
                        .map(|c| f32::from(c).mul_add(1. - opacity, 255. * opacity).round() as u8)
                        .into_iter()
                        .chain([255])
                })
            })
            .collect();
        image::Handle::from_rgba(64, 64, pixels)
    });
    stack![
        image(BACKDROP.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(iced::ContentFit::Fill),
        content
    ]
    .into()
}

#[derive(Deserialize)]
struct CapacityPolicy {
    minimum_ram_gb: u64,
    minimum_free_gb: u64,
    minimum_resume_free_gb: u64,
}

static CAPACITY_POLICY: LazyLock<CapacityPolicy> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../config/host-bootstrap.json"))
        .expect("bundled host policy")
});

#[derive(Clone, Copy)]
struct Capacity {
    memory: u64,
    disk: u64,
    disk_required: u64,
}

impl Capacity {
    fn rows(self) -> [(&'static str, u64, u64); 2] {
        [
            ("主机内存", self.memory, CAPACITY_POLICY.minimum_ram_gb),
            ("可用磁盘", self.disk, self.disk_required),
        ]
    }

    fn eligible(self) -> bool {
        self.rows()
            .into_iter()
            .all(|(_, available, required)| available >= required)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct Guest {
    cpus: u8,
    memory_mb: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    storage_gb: Option<u8>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Request {
    locale: String,
    phone: Guest,
    desktop: Guest,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            locale: "zh-CN".into(),
            phone: Guest {
                cpus: 2,
                memory_mb: 2048,
                storage_gb: Some(16),
            },
            desktop: Guest {
                cpus: 2,
                memory_mb: 2560,
                storage_gb: Some(32),
            },
        }
    }
}

pub struct Bundle {
    pub home: PathBuf,
    pub resource: PathBuf,
    inventory: BTreeMap<String, String>,
}
pub fn data_home() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("AIMANITY_DATA_DIR") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err("AIMANITY_DATA_DIR must be absolute".into());
        }
        return Ok(path.join("host"));
    }
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library/Application Support"));
    #[cfg(windows)]
    return crate::host::windows_data_home();
    #[cfg(not(any(target_os = "macos", windows)))]
    let base: Option<PathBuf> = None;
    #[cfg(not(windows))]
    base.map(|p| p.join(crate::APP_ID).join("host"))
        .ok_or_else(|| "Application data directory unavailable".into())
}

pub fn bundle_inventory(resource: &Path) -> Result<BTreeMap<String, String>, String> {
    let inventory: BTreeMap<String, String> =
        serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/integrity.json")))
            .map_err(|e| e.to_string())?;
    let platform = if cfg!(windows) {
        "qemu/manifest.json"
    } else {
        "images.payload"
    };
    for name in [
        "source.payload",
        "android.payload",
        platform,
        "speech/ggml-tiny-q5_1.bin",
    ] {
        if !inventory.contains_key(name) || !resource.join(name).is_file() {
            return Err(format!("离线组件缺失 · {name}"));
        }
    }
    Ok(inventory)
}

pub fn verify_bundle(resource: &Path) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    for (name, expected) in bundle_inventory(resource)? {
        // The installer verifies its large payloads while extracting them.
        if matches!(
            name.as_str(),
            "source.payload" | "android.payload" | "images.payload"
        ) {
            continue;
        }
        let mut file = fs::File::open(resource.join(&name)).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut bytes = [0; 65536];
        loop {
            let count = file.read(&mut bytes).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            hash.update(&bytes[..count]);
        }
        if hex::encode(hash.finalize()) != expected {
            return Err(format!("离线组件损坏 · {name}"));
        }
    }
    Ok(())
}
impl Bundle {
    pub fn resource_dir() -> Result<PathBuf, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let folder = exe.parent().ok_or("离线资源目录不存在")?;
        #[cfg(target_os = "macos")]
        {
            let contents = folder.parent().ok_or("离线资源目录不存在")?;
            Ok(contents.join("Resources/host"))
        }
        #[cfg(not(target_os = "macos"))]
        Ok(folder.join("host"))
    }
    pub fn load() -> Result<Self, String> {
        let resource = Self::resource_dir()?;
        Ok(Self {
            home: data_home()?,
            inventory: bundle_inventory(&resource)?,
            resource,
        })
    }
    pub fn request(&self) -> Result<Option<Request>, String> {
        match fs::read(self.home.join("request.json")) {
            Ok(data) => serde_json::from_slice(&data)
                .map(Some)
                .map_err(|e| e.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
    pub fn installed(&self) -> Result<bool, String> {
        let value = match fs::read(self.home.join("deployment.json")) {
            Ok(bytes) => {
                serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| e.to_string())?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.to_string()),
        };
        Ok(value["format"].as_u64() == Some(1)
            && (value["progress"]["state"].as_str() == Some("ready")
                || value["installed_payload"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty())))
    }
    fn capacity(&self, admitted: bool) -> Result<Capacity, String> {
        let volume = self
            .home
            .ancestors()
            .find(|p| p.is_dir())
            .ok_or("找不到安装磁盘")?;
        Ok(Capacity {
            memory: host_memory::total_memory_bytes() / GIB,
            disk: fs2::available_space(volume).map_err(|error| error.to_string())? / GIB,
            disk_required: if admitted {
                CAPACITY_POLICY.minimum_resume_free_gb
            } else {
                CAPACITY_POLICY.minimum_free_gb
            },
        })
    }
    pub fn host(&self, request: &Request) -> Result<Host, String> {
        let path = self.home.join("request.json");
        let admitted = self.installed()? || path.is_file();
        if let Some((label, available, required)) = self
            .capacity(admitted)?
            .rows()
            .into_iter()
            .find(|(_, available, required)| available < required)
        {
            return Err(format!(
                "{} · {available} / {required} GiB",
                Locale::parse(&request.locale).tr(label)
            ));
        }
        fs::create_dir_all(&self.home).map_err(|e| e.to_string())?;
        if !admitted {
            let bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
            write_atomic(&path, &bytes)?;
        }
        let images = if cfg!(windows) {
            self.inventory.get("qemu/manifest.json")
        } else {
            self.inventory.get("images.payload")
        };
        Ok(Host {
            home: self.home.clone(),
            resource: self.resource.clone(),
            request: path,
            #[cfg(target_os = "macos")]
            source: self.inventory["source.payload"].clone(),
            android: self.inventory["android.payload"].clone(),
            images: images.ok_or("找不到设备镜像")?.clone(),
            resume: admitted,
        })
    }
    pub fn mark_installed(&self) -> Result<(), String> {
        let payload = &self.inventory["source.payload"];
        let bytes = serde_json::to_vec(&serde_json::json!({"format":1,"payload":payload,
            "installed_payload":payload,"progress":{"phase":"devices","state":"ready","error":""}}))
        .map_err(|e| e.to_string())?;
        write_atomic(&self.home.join("deployment.json"), &bytes)
    }
}
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("安装目录不存在")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temp.write_all(bytes).map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub enum Field {
    Cpu,
    Memory,
    Storage,
}
#[derive(Clone, Copy, Debug)]
pub enum Message {
    Choose(Locale),
    Continue,
    Set(bool, Field, u16),
    RefreshCapacity,
    Start,
    Stop,
}
pub struct Setup {
    pub bundle: Bundle,
    pub request: Request,
    pub locale: Option<Locale>,
    language: Locale,
    language_entry: iced::Animation<bool>,
    pub attempted: bool,
    pub installed: bool,
    capacity: Result<Capacity, String>,
    #[cfg(target_os = "macos")]
    pub activities: Vec<crate::host::Activity>,
}
impl Setup {
    pub fn new(bundle: Bundle, request: Option<Request>, installed: bool) -> Self {
        let locale = request.as_ref().map(|r| Locale::parse(&r.locale));
        let attempted = request.is_some();
        let capacity = bundle.capacity(installed || attempted);
        Self {
            bundle,
            attempted,
            request: request.unwrap_or_default(),
            locale,
            language: locale.unwrap_or_default(),
            language_entry: iced::Animation::new(false)
                .duration(Duration::from_millis(480))
                .easing(iced::animation::Easing::EaseOutQuart),
            installed,
            capacity,
            #[cfg(target_os = "macos")]
            activities: Vec::new(),
        }
    }
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Choose(locale) => self.language = locale,
            Message::Continue => {
                self.locale = Some(self.language);
                self.request.locale = self.language.code().into();
                self.capacity = self.bundle.capacity(false);
                self.language_entry.go_mut(true, Instant::now());
            }
            Message::RefreshCapacity => self.capacity = self.bundle.capacity(false),
            Message::Set(desktop, field, value) => {
                let guest = if desktop {
                    &mut self.request.desktop
                } else {
                    &mut self.request.phone
                };
                match field {
                    Field::Cpu => guest.cpus = value as u8,
                    Field::Memory => guest.memory_mb = value,
                    Field::Storage => guest.storage_gb = Some(value as u8),
                }
            }
            Message::Start | Message::Stop => {}
        }
    }

    pub fn animating_language(&self) -> bool {
        self.language_entry.is_animating(Instant::now())
    }
    #[cfg(target_os = "macos")]
    pub fn activity(&mut self, activity: crate::host::Activity) {
        use crate::host::ActivityState::Running;
        if activity.state != Running
            && let Some(running) = self
                .activities
                .iter_mut()
                .rev()
                .find(|entry| entry.command == activity.command && entry.state == Running)
        {
            *running = activity;
            return;
        }
        if self.activities.len() == 3 {
            self.activities.remove(0);
        }
        self.activities.push(activity);
    }

    pub fn progress<'a>(
        &'a self,
        locale: Locale,
        phase: &'a str,
        error: &'a str,
        can_stop: bool,
        reduced_motion: bool,
    ) -> Element<'a, Message> {
        let title = locale.tr(if phase.is_empty() {
            if error.is_empty() {
                "已停止"
            } else {
                "安装失败"
            }
        } else {
            phase
        });
        let mut content = column![].spacing(20).align_x(iced::Center);
        if !phase.is_empty() {
            content = content.push(crate::loading::orb(180., reduced_motion));
        }
        content = content.push(text(title).size(22).font(SETUP_TITLE));
        if !error.is_empty() {
            content = content.push(text(error).size(13).style(style::error_text));
        }
        let centered = container(content).center(Length::Fill);
        let mut page = column![].spacing(28);
        if !self.installed {
            page = page.push(text(locale.tr("安装")).size(34).font(SETUP_TITLE));
        }
        let details = column![centered].spacing(12);
        #[cfg(target_os = "macos")]
        let details = if self.activities.is_empty() {
            details
        } else {
            use crate::host::ActivityState::{Completed, Failed, Running};
            let activity = column(self.activities.iter().map(|entry| {
                let label = match entry.state {
                    Running => "运行中",
                    Completed => "已完成",
                    Failed => "失败",
                };
                row![
                    text(&entry.command).size(12),
                    text(locale.tr(label)).size(12).style(style::muted_text)
                ]
                .spacing(12)
                .into()
            }))
            .spacing(6);
            details.push(activity)
        };
        page = page.push(details.height(Length::Fill));
        if phase.is_empty() || can_stop {
            let (label, message) = if can_stop {
                ("停止", Message::Stop)
            } else if error.is_empty() {
                ("继续安装", Message::Start)
            } else {
                ("重试", Message::Start)
            };
            page = page.push(
                column![
                    line(),
                    row![
                        space::horizontal(),
                        button(container(text(locale.tr(label)).size(14)).center_y(Length::Fill))
                            .padding([0, 13])
                            .height(SETUP_CONTROL)
                            .style(if can_stop {
                                style::quiet
                            } else {
                                style::primary
                            })
                            .on_press(message)
                    ]
                ]
                .spacing(14),
            );
        }
        page.padding(iced::Padding {
            top: 46.,
            right: 44.,
            bottom: 26.,
            left: 44.,
        })
        .into()
    }

    pub fn view<'a>(&'a self, error: &'a str, reduced_motion: bool) -> Element<'a, Message> {
        if let Some(locale) = self.locale {
            let tr = |key: &'static str| locale.tr(key);
            let mut content = column![text(tr("配置设备")).size(34).font(SETUP_TITLE)].spacing(28);
            let capacity_error = match (&self.capacity, error) {
                (_, error) if !error.is_empty() => error,
                (Ok(capacity), _) if !capacity.eligible() => {
                    tr("本机资源不足，请检查内存和可用空间。")
                }
                _ => "",
            };
            if !capacity_error.is_empty() {
                content = content.push(text(capacity_error).size(14).style(style::error_text));
            }
            content = content.push(column![
                line(),
                Self::guest(false, self.request.phone, locale, reduced_motion),
                line(),
                Self::guest(true, self.request.desktop, locale, reduced_motion),
                line(),
            ]);
            let (checks, eligible): (Element<'_, Message>, _) = match &self.capacity {
                Ok(capacity) => (
                    row(capacity.rows().map(|(label, available, required)| {
                        let status = if available < required {
                            format!("{} {available} GB / {required} GB", tr(label))
                        } else {
                            format!("{} {available} GB", tr(label))
                        };
                        text(status)
                            .size(12)
                            .style(move |theme| {
                                if available < required {
                                    style::error_text(theme)
                                } else {
                                    style::muted_text(theme)
                                }
                            })
                            .into()
                    }))
                    .spacing(11)
                    .into(),
                    capacity.eligible(),
                ),
                Err(error) => (text(error).size(13).style(style::error_text).into(), false),
            };
            let panel = column![
                container(content)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_y(iced::Center),
                space::vertical().height(16),
                line(),
                row![
                    column![
                        text(if cfg!(target_os = "macos") {
                            "macOS · Apple Silicon"
                        } else {
                            "Windows 11"
                        })
                        .size(12)
                        .font(SETUP_TITLE),
                        checks
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    button(container(text(tr("开始安装")).size(14)).center_y(Length::Fill))
                        .height(SETUP_CONTROL)
                        .padding([0, 17])
                        .style(style::primary)
                        .on_press_maybe(eligible.then_some(Message::Start))
                ]
                .spacing(10)
                .padding(iced::Padding::ZERO.top(14))
                .align_y(iced::Center)
            ]
            .padding(iced::Padding {
                top: 46.,
                right: 44.,
                bottom: 26.,
                left: 44.,
            });
            let now = Instant::now();
            if !reduced_motion && self.language_entry.is_animating(now) {
                let entry = self.language_entry.interpolate(0., 1., now);
                float(panel)
                    .scale(0.96 + 0.04 * entry)
                    .translate(move |_, _| iced::Vector::new(0., 12. * (1. - entry)))
                    .into()
            } else {
                panel.into()
            }
        } else {
            let options: Vec<_> = Locale::all().collect();
            let locale = self.language;
            let rtl = locale.code() == "ar";
            let bold = iced::Font {
                weight: iced::font::Weight::Bold,
                ..locale.font()
            };
            column![
                container(
                    column![
                        text(locale.tr("选择语言")).size(33).font(bold),
                        container(
                            column(options.chunks(2).map(|pair| {
                                row(pair.iter().enumerate().map(|(index, &option)| {
                                    let option = if rtl {
                                        pair[pair.len() - 1 - index]
                                    } else {
                                        option
                                    };
                                    let selected = option == locale;
                                    let radio = container(space()).center(12).style(move |_| {
                                        container::Style::default()
                                            .background(if selected {
                                                iced::Color::WHITE
                                            } else {
                                                iced::Color::TRANSPARENT
                                            })
                                            .border(iced::Border {
                                                color: if selected {
                                                    style::FOREGROUND
                                                } else {
                                                    iced::Color::from_rgb8(150, 150, 150)
                                                },
                                                width: if selected { 4. } else { 1.5 },
                                                radius: 6.into(),
                                            })
                                    });
                                    let label = text(option.name()).size(14).font(iced::Font {
                                        weight: if selected {
                                            iced::font::Weight::Bold
                                        } else {
                                            iced::font::Weight::Normal
                                        },
                                        ..option.font()
                                    });
                                    button(
                                        if rtl {
                                            row![space::horizontal(), label, radio]
                                        } else {
                                            row![radio, label]
                                        }
                                        .height(Length::Fill)
                                        .spacing(10)
                                        .align_y(iced::Center),
                                    )
                                    .width(Length::Fill)
                                    .height(SETUP_CONTROL)
                                    .padding([0, 13])
                                    .style(move |_, status| button::Style {
                                        background: Some(style::CONTROL_FILL.into()),
                                        text_color: if selected
                                            || matches!(
                                                status,
                                                button::Status::Hovered | button::Status::Pressed
                                            ) {
                                            style::FOREGROUND
                                        } else {
                                            style::MUTED
                                        },
                                        border: iced::Border {
                                            color: if selected {
                                                style::FOREGROUND
                                            } else {
                                                SETUP_LINE
                                            },
                                            width: 1.,
                                            radius: style::CONTROL_RADIUS.into(),
                                        },
                                        ..Default::default()
                                    })
                                    .on_press(Message::Choose(option))
                                    .into()
                                }))
                                .spacing(8)
                                .into()
                            }))
                            .spacing(8)
                        )
                        .width(434),
                    ]
                    .spacing(28)
                    .align_x(iced::Center)
                )
                .center(Length::Fill),
                button(
                    container(text(locale.tr("继续")).size(14).font(SETUP_TITLE))
                        .center(Length::Fill)
                )
                .width(142)
                .height(SETUP_CONTROL)
                .style(style::primary)
                .on_press(Message::Continue)
            ]
            .align_x(iced::Center)
            .padding(iced::Padding {
                top: 20.,
                right: 44.,
                bottom: 33.,
                left: 44.,
            })
            .into()
        }
    }
    fn guest(
        desktop: bool,
        guest: Guest,
        locale: Locale,
        reduced_motion: bool,
    ) -> Element<'static, Message> {
        let tr = |key: &'static str| locale.tr(key);
        let specs: [(&str, Field, &'static [u16], u16); 3] = [
            (tr("处理器"), Field::Cpu, &[1, 2, 4], guest.cpus.into()),
            (
                tr("内存"),
                Field::Memory,
                if desktop {
                    &[2048, 2560, 3072, 4096]
                } else {
                    &[1536, 2048, 3072, 4096]
                },
                guest.memory_mb,
            ),
            (
                tr("存储"),
                Field::Storage,
                if desktop {
                    &[16, 32, 64]
                } else {
                    &[16, 24, 32]
                },
                guest
                    .storage_gb
                    .unwrap_or(if desktop { 32 } else { 16 })
                    .into(),
            ),
        ];
        let fields = row(specs.into_iter().map(|(label, field, values, current)| {
            let value = match field {
                Field::Cpu => current.to_string(),
                Field::Memory => format!("{} GB", f32::from(current) / 1024.),
                Field::Storage => format!("{current} GB"),
            };
            let selected = values
                .iter()
                .position(|&value| value == current)
                .unwrap_or(0);
            column![
                container(
                    row![
                        text(label).size(12).style(style::muted_text),
                        space::horizontal(),
                        text(value).size(14).font(SETUP_TITLE)
                    ]
                    .spacing(6)
                )
                .height(21)
                .align_y(iced::Center),
                SpringSlider::new(values.len() as u8 - 1, selected as u8, move |index| {
                    Message::Set(desktop, field, values[usize::from(index)])
                })
                .reduce_motion(reduced_motion),
            ]
            .into()
        }))
        .spacing(18);
        row![
            row![
                if desktop {
                    Icon::Screen.view()
                } else {
                    Icon::PhonePortrait.view()
                },
                text(tr(if desktop { "电脑" } else { "手机" }))
                    .size(16)
                    .font(SETUP_TITLE),
            ]
            .spacing(8)
            .width(120)
            .align_y(iced::Center),
            fields
        ]
        .spacing(16)
        .padding([21, 0])
        .align_y(iced::Center)
        .into()
    }
}
