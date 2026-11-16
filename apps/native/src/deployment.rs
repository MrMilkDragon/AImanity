use crate::{
    controls::{Icon, action_button, icon_button},
    host::Host,
    locale::Locale,
    style,
};
use iced::{
    Element, Length,
    widget::{button, column, container, pick_list, row, space, text},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[cfg(not(windows))]
const ID: &str = "dev.aiphone.client";

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
                storage_gb: Some(16),
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
    base.map(|p| p.join(ID).join("host"))
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
            "source.payload" | "android.payload" | "images.payload" | "windows/wsl.msi"
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
        if format!("{:x}", hash.finalize()) != expected {
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
    pub fn host(&self, request: &Request) -> Result<Host, String> {
        let installed = self.installed()?;
        let volume = self
            .home
            .ancestors()
            .find(|p| p.is_dir())
            .ok_or("找不到安装磁盘")?;
        let free = fs2::available_space(volume).map_err(|e| e.to_string())?;
        #[derive(Deserialize)]
        struct Policy {
            minimum_free_gb: u64,
            minimum_resume_free_gb: u64,
        }
        let policy: Policy =
            serde_json::from_str(include_str!("../../../config/host-bootstrap.json"))
                .expect("bundled host policy");
        let limit = if installed {
            policy.minimum_resume_free_gb
        } else {
            policy.minimum_free_gb
        };
        if free < limit * 1024_u64.pow(3) {
            return Err(format!(
                "磁盘空间不足 · 需要 {limit} GiB，可用 {:.1} GiB",
                free as f64 / 1024_f64.powi(3)
            ));
        }
        let path = self.home.join("request.json");
        fs::create_dir_all(&self.home).map_err(|e| e.to_string())?;
        if !installed {
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
            resume: installed,
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
    Back,
    Set(bool, Field, u16),
    Start,
}
pub struct Setup {
    pub bundle: Bundle,
    pub request: Request,
    pub locale: Option<Locale>,
}
impl Setup {
    pub fn new(bundle: Bundle, request: Option<Request>) -> Self {
        let locale = request.as_ref().map(|r| Locale::parse(&r.locale));
        Self {
            bundle,
            request: request.unwrap_or_default(),
            locale,
        }
    }
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Choose(locale) => {
                self.locale = Some(locale);
                self.request.locale = locale.code().into();
            }
            Message::Back => self.locale = None,
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
            Message::Start => {}
        }
    }
    pub fn view<'a>(&self, error: &'a str) -> Element<'a, Message> {
        let content = if let Some(locale) = self.locale {
            let tr = |key: &'static str| locale.tr(key);
            let mut body = column![
                row![
                    icon_button(Icon::Back, tr("返回"), Some(Message::Back)),
                    text(tr("设备配置")).size(22)
                ]
                .spacing(8)
                .align_y(iced::Center),
                Self::guest(false, self.request.phone, locale),
                Self::guest(true, self.request.desktop, locale),
                action_button(tr("安装"), Some(Message::Start))
            ]
            .spacing(22);
            if !error.is_empty() {
                body = body.push(text(error).size(13).style(style::error_text));
            }
            body
        } else {
            let options: Vec<_> = Locale::all().collect();
            column![
                text("Language").size(22),
                column(options.chunks(2).map(|pair| {
                    row(pair.iter().map(|locale| {
                        button(
                            row![
                                text(locale.glyph()).size(12).width(33),
                                text(locale.name()).size(14)
                            ]
                            .spacing(8)
                            .align_y(iced::Center),
                        )
                        .width(Length::Fill)
                        .height(style::CONTROL_HEIGHT)
                        .style(style::quiet)
                        .on_press(Message::Choose(*locale))
                        .into()
                    }))
                    .spacing(8)
                    .into()
                }))
                .spacing(8)
            ]
            .spacing(18)
        };
        container(container(content).max_width(590).padding(24))
            .center(Length::Fill)
            .into()
    }
    fn guest(desktop: bool, guest: Guest, locale: Locale) -> Element<'static, Message> {
        let tr = |key: &'static str| locale.tr(key);
        let specs = [
            (tr("处理器"), Field::Cpu, vec![1, 2, 4], guest.cpus.into()),
            (
                tr("内存"),
                Field::Memory,
                if desktop {
                    vec![2048, 2560, 3072, 4096]
                } else {
                    vec![1536, 2048, 3072, 4096]
                },
                guest.memory_mb,
            ),
            (
                tr("存储"),
                Field::Storage,
                if desktop {
                    vec![16, 32, 64]
                } else {
                    vec![16, 24, 32]
                },
                guest.storage_gb.unwrap_or(16).into(),
            ),
        ];
        let fields = row(specs.into_iter().map(|(label, field, values, current)| {
            let options: Vec<_> = values
                .into_iter()
                .map(|value| Value {
                    value,
                    label: match field {
                        Field::Cpu => format!("{value} {}", tr("核")),
                        Field::Memory => format!("{} GB", f32::from(value) / 1024.),
                        Field::Storage => format!("{value} GB"),
                    },
                })
                .collect();
            let selected = options.iter().find(|v| v.value == current).cloned();
            column![
                text(label).size(13).style(style::muted_text),
                pick_list(options, selected, move |v| Message::Set(
                    desktop, field, v.value
                ))
                .width(Length::Fill)
                .padding([10, 12])
                .style(style::select)
                .menu_style(style::menu)
            ]
            .spacing(6)
            .into()
        }))
        .spacing(10);
        column![
            row![
                if desktop {
                    Icon::Screen.view()
                } else {
                    Icon::Camera.view()
                },
                text(tr(if desktop { "电脑" } else { "手机" })).size(15),
                space::horizontal()
            ]
            .spacing(8)
            .align_y(iced::Center),
            fields
        ]
        .spacing(10)
        .into()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Value {
    value: u16,
    label: String,
}
impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

pub fn loading<M: 'static>(elapsed: std::time::Duration) -> iced::widget::canvas::Canvas<Orb, M> {
    iced::widget::canvas::Canvas::new(Orb(elapsed.as_secs_f32()))
        .width(64)
        .height(64)
}
pub struct Orb(f32);
impl<M> iced::widget::canvas::Program<M> for Orb {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, Path};
        let mut frame = Frame::new(renderer, bounds.size());
        let (s, c) = (self.0 * 1.5).sin_cos();
        for point in 0..150 {
            let y = 1. - 2. * (point as f32 + 0.5) / 150.;
            let r = (1. - y * y).sqrt();
            let (u, v) = (point as f32 * 2.3999631).sin_cos();
            let x = r * v;
            let z = r * u;
            let depth = z * c - x * s;
            let x = x * c + z * s;
            let p = iced::Point::new(32. + x * 24., 32. + y * 24.);
            frame.fill(
                &Path::circle(p, 0.7 + (depth + 1.) * 0.35),
                iced::Color::from_rgba(0.09, 0.09, 0.09, 0.25 + (depth + 1.) * 0.34),
            );
        }
        vec![frame.into_geometry()]
    }
}
