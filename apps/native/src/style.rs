use iced::{
    Color, Size, Theme,
    theme::Palette,
    widget::{button, text},
};
use std::sync::OnceLock;

pub const FOREGROUND: Color = Color::from_rgb8(0x17, 0x17, 0x17);
pub const BACKGROUND: Color = Color::from_rgb8(0xfa, 0xfa, 0xfa);
pub const MUTED: Color = Color::from_rgb8(0x59, 0x59, 0x59);
pub const ERROR: Color = Color::from_rgb8(0xb5, 0x25, 0x25);
const COPILOT_BACKGROUND: Color = Color::from_rgb8(34, 34, 34);
pub const CONTROL_RADIUS: f32 = 8.;
pub const PANEL_RADIUS: f32 = 14.;
pub const GLASS_OPACITY: f32 = 0.35;
pub const GLASS_BLUR: f32 = 18.;
pub const BORDER_WIDTH: f32 = 0.75;
pub const CONTROL_HEIGHT: f32 = 40.;
pub const MOTION_DEVICE: f32 = 0.56;
pub const WINDOW_ENTER: f32 = 0.46;
pub const WINDOW_EXIT: f32 = 0.24;
pub const NAV_ICON_WIDTH: f32 = 36.;
pub const REGISTRY_CONTROL: f32 = 44.;

pub fn navigation(size: Size) -> (f32, f32, f32, f32) {
    if size.width <= 760. {
        (10., 6., 4., 42.)
    } else {
        (24., 11., 5., 38.)
    }
}

pub fn navigation_group(size: Size) -> (f32, f32) {
    if size.width <= 760. {
        (0., 8.)
    } else {
        (3., 19.)
    }
}

pub fn window_insets(width: f32) -> (f32, f32, f32) {
    if width <= 640. {
        (18., 12., 52.)
    } else {
        (24., 16., 56.)
    }
}

pub fn registry_insets(width: f32, adding: bool) -> (f32, f32, f32, f32) {
    if adding {
        (16., 18., 16., 16.)
    } else if width <= 540. {
        (32., 20., 14., 18.)
    } else {
        (46., 44., 26., 28.)
    }
}

pub fn theme() -> Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME
        .get_or_init(|| {
            Theme::custom(
                "AImanity",
                Palette {
                    background: BACKGROUND,
                    text: FOREGROUND,
                    primary: FOREGROUND,
                    danger: ERROR,
                    ..Theme::Light.palette()
                },
            )
        })
        .clone()
}

pub fn copilot_theme() -> Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME
        .get_or_init(|| {
            Theme::custom(
                "AImanity · Copilot",
                Palette {
                    background: COPILOT_BACKGROUND,
                    text: Color::from_rgb8(244, 244, 244),
                    primary: Color::from_rgb8(242, 242, 242),
                    danger: Color::from_rgb8(248, 128, 128),
                    ..Theme::Dark.palette()
                },
            )
        })
        .clone()
}

pub fn muted_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(muted_color(theme)),
    }
}
pub fn muted_color(theme: &Theme) -> Color {
    if dark(theme) {
        Color::from_rgb8(188, 188, 188)
    } else {
        MUTED
    }
}

pub fn error_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().danger),
    }
}

pub fn dark(theme: &Theme) -> bool {
    theme.palette().background == COPILOT_BACKGROUND
}

pub fn control_fill(theme: &Theme) -> Color {
    let gray = if dark(theme) { 0.13 } else { 1. };
    Color::from_rgba(gray, gray, gray, GLASS_OPACITY)
}

pub fn accent(theme: &Theme) -> Color {
    let gray = if dark(theme) { 0x42 } else { 0xea };
    Color::from_rgb8(gray, gray, gray)
}

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::primary(theme, status);
    style.border.radius = CONTROL_RADIUS.into();
    style
}

pub fn quiet(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::text(theme, status);
    style.text_color = theme.palette().text;
    style.border.radius = CONTROL_RADIUS.into();
    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        let ink = if dark(theme) { 1. } else { 0. };
        style.background = Some(Color::from_rgba(ink, ink, ink, 0.055).into());
    }
    style
}

pub fn input(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let mut s = iced::widget::text_input::default(theme, status);
    s.background = control_fill(theme).into();
    s.border.radius = CONTROL_RADIUS.into();
    s.border.width = BORDER_WIDTH;
    s.border.color = if matches!(status, iced::widget::text_input::Status::Focused { .. }) {
        theme.palette().text
    } else {
        theme.palette().text.scale_alpha(0.16)
    };
    s
}
pub fn select(
    theme: &Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    let mut s = iced::widget::pick_list::default(theme, status);
    s.background = control_fill(theme).into();
    s.border.radius = CONTROL_RADIUS.into();
    s.border.width = BORDER_WIDTH;
    s.border.color = theme.palette().text.scale_alpha(0.16);
    s
}

pub fn registry_input(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let mut style = input(theme, status);
    style.background = if dark(theme) {
        accent(theme)
    } else {
        Color::WHITE
    }
    .into();
    style
}

pub fn registry_select(
    theme: &Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    let mut style = select(theme, status);
    style.background = if dark(theme) {
        accent(theme)
    } else {
        Color::WHITE
    }
    .into();
    style
}
pub fn menu(theme: &Theme) -> iced::overlay::menu::Style {
    let mut s = iced::overlay::menu::default(theme);
    s.background = theme.palette().background.scale_alpha(0.98).into();
    s.border.radius = CONTROL_RADIUS.into();
    s.border.width = BORDER_WIDTH;
    s.border.color = theme.palette().text.scale_alpha(0.16);
    s
}
