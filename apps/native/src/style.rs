use iced::{
    Color, Theme,
    theme::Palette,
    widget::{button, text},
};
use std::sync::OnceLock;

pub const FOREGROUND: Color = Color::from_rgb8(0x17, 0x17, 0x17);
pub const BACKGROUND: Color = Color::from_rgb8(0xfa, 0xfa, 0xfa);
pub const MUTED: Color = Color::from_rgb8(0x59, 0x59, 0x59);
pub const ERROR: Color = Color::from_rgb8(0xb5, 0x25, 0x25);
pub const LATIN_FONT: iced::Font = iced::Font::with_name(if cfg!(target_os = "macos") {
    "Helvetica Neue"
} else {
    "Segoe UI"
});
pub const CJK_FONT: iced::Font = iced::Font::with_name(if cfg!(target_os = "macos") {
    "PingFang SC"
} else {
    "Microsoft YaHei"
});
pub const ACCENT: Color = Color::from_rgb8(0xea, 0xea, 0xea);
pub const CONTROL_FILL: Color = Color {
    a: GLASS_OPACITY,
    ..Color::WHITE
};
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
pub const NAVIGATION: [f32; 3] = [11., 5., 38.];
pub const NAV_TOOL_GAP: f32 = 3.;
pub const NAV_SEPARATOR: f32 = 19.;
pub const REGISTRY_CONTROL: f32 = 44.;

pub fn tool_title(label: &str) -> iced::widget::Text<'_> {
    heading(label, 15., 22., iced::font::Weight::Semibold)
}

pub fn section_title(label: &str) -> iced::widget::Text<'_> {
    heading(label, 14., 20., iced::font::Weight::Medium)
}

fn heading(
    label: &str,
    size: f32,
    line: f32,
    weight: iced::font::Weight,
) -> iced::widget::Text<'_> {
    let cjk = label.chars().any(|c| {
        matches!(c,
        '\u{3000}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}')
    });
    text(label)
        .size(size)
        .line_height(iced::Pixels(line))
        .font(iced::Font {
            weight,
            ..if cjk { CJK_FONT } else { LATIN_FONT }
        })
        .wrapping(text::Wrapping::None)
}

pub fn bezier(progress: f32, [x1, y1, x2, y2]: [f32; 4]) -> f32 {
    if progress <= 0. || progress >= 1. {
        return progress;
    }
    let cubic = |t: f32, a, b| {
        let u = 1. - t;
        3. * u * t * (u * a + t * b) + t * t * t
    };
    let (mut low, mut high) = (0., 1.);
    for _ in 0..12 {
        let t = (low + high) * 0.5;
        if cubic(t, x1, x2) < progress {
            low = t;
        } else {
            high = t;
        }
    }
    cubic((low + high) * 0.5, y1, y2)
}

pub fn window_insets(width: f32) -> (f32, f32, f32) {
    if width <= 640. {
        (18., 12., 52.)
    } else {
        (24., 16., 56.)
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

pub fn muted_text(_: &Theme) -> text::Style {
    text::Style { color: Some(MUTED) }
}

pub fn error_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().danger),
    }
}

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let opacity = match status {
        button::Status::Active => 1.,
        button::Status::Hovered | button::Status::Pressed => 0.9,
        button::Status::Disabled => 0.5,
    };
    button::Style {
        background: Some(theme.palette().primary.scale_alpha(opacity).into()),
        text_color: Color::WHITE.scale_alpha(if status == button::Status::Disabled {
            0.5
        } else {
            1.
        }),
        border: iced::border::rounded(CONTROL_RADIUS),
        ..Default::default()
    }
}

pub fn quiet(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::text(theme, status);
    style.text_color = theme
        .palette()
        .text
        .scale_alpha(if status == button::Status::Disabled {
            0.5
        } else {
            1.
        });
    style.border.radius = CONTROL_RADIUS.into();
    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        style.background = Some(ACCENT.into());
    }
    style
}

pub fn input(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let mut s = iced::widget::text_input::default(theme, status);
    s.background = CONTROL_FILL.into();
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
    s.background = CONTROL_FILL.into();
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
    style.background = Color::WHITE.into();
    style
}

pub fn registry_select(
    theme: &Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    let mut style = select(theme, status);
    style.background = Color::WHITE.into();
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
