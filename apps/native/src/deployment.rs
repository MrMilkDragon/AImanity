use crate::{locale::Locale, spring_slider::SpringSlider, style};
use iced::{
    Element, Length,
    widget::{button, column, container, image, row, space, stack, text},
};
#[cfg(not(target_os = "android"))]
use std::io::Read;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::LazyLock,
};

#[cfg(target_os = "android")]
#[path = "android_assets.rs"]
pub(crate) mod android_assets;

#[cfg(target_os = "android")]
pub(crate) fn initialize_android(app: android_activity::AndroidApp) -> Result<(), String> {
    android_assets::initialize(app)
}

const SETUP_CONTROL: f32 = 44.;
const SETUP_LINE: iced::Color = iced::Color::from_rgb8(222, 222, 222);
const SETUP_TITLE: iced::Font = iced::Font {
    weight: iced::font::Weight::Semibold,
    ..iced::Font::DEFAULT
};

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

#[cfg(target_os = "android")]
pub fn data_home() -> Result<PathBuf, String> {
    android_assets::home()
}

#[cfg(not(target_os = "android"))]
pub fn data_home() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("AIMANITY_DATA_DIR") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err("AIMANITY_DATA_DIR must be absolute".into());
        }
        return Ok(path.join(crate::APP_ID).join("host"));
    }
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library/Application Support"));
    #[cfg(windows)]
    return crate::host::windows_data_home();
    #[cfg(target_os = "linux")]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    #[cfg(not(windows))]
    base.map(|p| p.join(crate::APP_ID).join("host"))
        .ok_or_else(|| "Application data directory unavailable".into())
}

#[cfg(not(target_os = "android"))]
pub fn component_versions() -> Result<BTreeMap<String, String>, String> {
    serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/integrity.json")))
        .map_err(|e| e.to_string())
}
#[cfg(not(target_os = "android"))]
pub fn bundle_inventory(resource: &Path) -> Result<BTreeMap<String, String>, String> {
    let inventory = component_versions()?;
    let platform = if cfg!(any(windows, target_os = "linux")) {
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

#[cfg(not(target_os = "android"))]
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
pub struct Setup {
    pub home: PathBuf,
    pub resource: PathBuf,
    pub language: Locale,
    pub saved: bool,
}
impl Setup {
    #[cfg(target_os = "android")]
    pub fn resource_dir() -> Result<PathBuf, String> {
        let id = android_assets::manifest()
            .map_err(|e| e.to_string())?
            .map_or_else(|| String::from("missing"), |(id, _)| id);
        Ok(data_home()?.join("resources").join(id))
    }
    #[cfg(not(target_os = "android"))]
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
        let home = data_home()?;
        let resource = Self::resource_dir()?;
        let language = match fs::read_to_string(home.join("user/native-locale")) {
            Ok(value) => Some(Locale::parse(value.trim())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        Ok(Self {
            home,
            resource,
            saved: language.is_some(),
            language: language.unwrap_or_default(),
        })
    }
    pub fn save(&self) -> Result<(), String> {
        if self.saved {
            return Ok(());
        }
        let user = self.home.join("user");
        fs::create_dir_all(&user).map_err(|e| e.to_string())?;
        write_atomic(&user.join("native-locale"), self.language.code().as_bytes())
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
#[derive(Clone, Debug)]
pub enum Message {
    Choose(Locale),
    Continue,
}
impl Setup {
    pub fn view<'a>(&'a self, error: &'a str) -> Element<'a, Message> {
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
            text(error).size(13).style(style::error_text),
            button(
                container(text(locale.tr("继续")).size(14).font(SETUP_TITLE)).center(Length::Fill)
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

const CPU_CHOICES: &[u32] = &[1, 2, 4];

pub(crate) fn resource_controls<M: Clone + 'static>(
    desktop: bool,
    guest: &ai_phone_api::wire::VirtualDeviceProfile,
    existing: Option<u32>,
    locale: Locale,
    reduced_motion: bool,
    #[cfg(target_os = "android")] mut cpu_input: Option<Element<'static, M>>,
    change: impl Fn(Field, u32) -> M + Copy + 'static,
) -> Element<'static, M> {
    let tr = |key: &'static str| locale.tr(key);
    let specs: [(&str, Field, &'static [u32], u32); 3] = [
        (tr("处理器"), Field::Cpu, CPU_CHOICES, guest.cpus),
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
            guest.storage_gb,
        ),
    ];
    row(specs.into_iter().map(|(label, field, values, current)| {
        let value = match field {
            Field::Cpu => current.to_string(),
            Field::Memory => format!("{} GB", f64::from(current) / 1024.),
            Field::Storage => format!("{current} GB"),
        };
        let values = match (field, existing) {
            (Field::Storage, Some(_)) if !desktop => &values[..0],
            (Field::Storage, Some(minimum)) => &values[values.partition_point(|n| *n < minimum)..],
            _ => values,
        };
        let selected = values
            .iter()
            .position(|&value| value == current)
            .unwrap_or(0);
        let display_value: Element<'static, M> = text(value).size(14).font(SETUP_TITLE).into();
        #[cfg(target_os = "android")]
        let display_value = if matches!(field, Field::Cpu) {
            cpu_input.take().unwrap_or(display_value)
        } else {
            display_value
        };
        let mut control = column![
            container(
                row![
                    text(label).size(12).style(style::muted_text),
                    space::horizontal(),
                    display_value
                ]
                .spacing(6)
            )
            .height(21)
            .align_y(iced::Center)
        ];
        if values.len() > 1 {
            control = control.push(
                SpringSlider::new(values.len() as u8 - 1, selected as u8, move |index| {
                    change(field, values[usize::from(index)])
                })
                .reduce_motion(reduced_motion),
            );
        }
        control.into()
    }))
    .spacing(18)
    .into()
}
