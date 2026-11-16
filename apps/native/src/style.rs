use iced::{Color, Theme, theme::Palette, widget::button};

pub const FOREGROUND: Color = Color::from_rgb8(0x17, 0x17, 0x17);
pub const BACKGROUND: Color = Color::from_rgb8(0xfa, 0xfa, 0xfa);
pub const MUTED: Color = Color::from_rgb8(0x59, 0x59, 0x59);
pub const ERROR: Color = Color::from_rgb8(0xb5, 0x25, 0x25);
pub const CONTROL_RADIUS: f32 = 8.;
pub const PANEL_RADIUS: f32 = 14.;
pub const GLASS_OPACITY: f32 = 0.35;
pub const GLASS_BLUR: f32 = 11.;
pub const BORDER_WIDTH: f32 = 0.75;
pub const CONTROL_HEIGHT: f32 = 40.;
pub const MOTION_ENTER: f32 = 0.28;
pub const MOTION_EXIT: f32 = 0.18;
pub const MOTION_DEVICE: f32 = 0.58;

pub fn theme() -> Theme {
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
}

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::primary(theme, status);
    style.border.radius = CONTROL_RADIUS.into();
    style
}

pub fn quiet(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::text(theme, status);
    style.text_color = FOREGROUND;
    style.border.radius = CONTROL_RADIUS.into();
    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        style.background = Some(Color::from_rgba(0., 0., 0., 0.045).into());
    }
    style
}

pub fn input(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let mut s = iced::widget::text_input::default(theme, status);
    s.background = Color::from_rgba(1., 1., 1., 0.35).into();
    s.border.radius = CONTROL_RADIUS.into();
    s.border.width = BORDER_WIDTH;
    s.border.color = if matches!(status, iced::widget::text_input::Status::Focused { .. }) {
        FOREGROUND
    } else {
        Color::from_rgba(0., 0., 0., 0.16)
    };
    s
}
pub fn select(
    theme: &Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    let mut s = iced::widget::pick_list::default(theme, status);
    s.background = Color::from_rgba(1., 1., 1., 0.35).into();
    s.border.radius = CONTROL_RADIUS.into();
    s.border.width = BORDER_WIDTH;
    s.border.color = Color::from_rgba(0., 0., 0., 0.16);
    s
}
pub fn menu(theme: &Theme) -> iced::overlay::menu::Style {
    let mut s = iced::overlay::menu::default(theme);
    s.background = Color::from_rgba(0.98, 0.98, 0.98, 0.98).into();
    s.border.radius = CONTROL_RADIUS.into();
    s.border.width = BORDER_WIDTH;
    s.border.color = Color::from_rgba(0., 0., 0., 0.16);
    s
}
