use crate::style;
use iced::{
    Element, Length,
    widget::{button, column, container, pick_list, rule, svg, text, text_input},
};
use std::borrow::Borrow;

#[derive(Clone, Copy)]
pub enum Icon {
    Network,
    Pin,
    Book,
    Wrench,
    Folder,
    Target,
    Screen,
    PhonePortrait,
    PhoneLandscape,
    Camera,
    Copy,
    Cut,
    Up,
    Grip,
    Plus,
    Minus,
    Back,
    Edit,
    Trash,
    Eye,
    EyeOff,
    Paperclip,
    Todos,
    Send,
    Stop,
    Pause,
    Play,
    Down,
    Right,
    Download,
    Upload,
    Search,
    Link,
    Undo,
    Close,
    Image,
    Check,
    Settings,
    Cable,
    Compare,
    Refresh,
    Volume,
    Muted,
    Compass,
    LocateFixed,
    Mic,
    MicOff,
}
pub fn content_button<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    height: f32,
) -> button::Button<'a, M> {
    button(container(content).center_y(Length::Fill))
        .height(height)
        .padding([0, 12])
        .style(style::quiet)
}

fn button_label(label: &str) -> text::Text<'_> {
    text(label)
        .size(13)
        .line_height(style::FIELD_LINE)
        .wrapping(text::Wrapping::None)
}

pub fn text_button<'a, M: Clone + 'a>(label: &'a str) -> button::Button<'a, M> {
    content_button(button_label(label), style::CONTROL_HEIGHT)
}

pub fn action_button<'a, M: Clone + 'a>(
    label: &'a str,
    action: Option<M>,
) -> button::Button<'a, M> {
    content_button(
        container(button_label(label)).center_x(Length::Fill),
        style::CONTROL_HEIGHT,
    )
    .width(Length::Shrink)
    .style(style::primary)
    .on_press_maybe(action)
}

pub fn tab_button<'a, M: Clone + 'a>(
    label: &'a str,
    selected: bool,
    action: Option<M>,
) -> Element<'a, M> {
    button(column![
        container(text(label).size(13).color(if selected {
            style::FOREGROUND
        } else {
            style::MUTED
        }))
        .center(Length::Fill),
        rule::horizontal(2).style(move |theme| {
            let mut s = rule::default(theme);
            s.color = if selected {
                style::FOREGROUND
            } else {
                iced::Color::TRANSPARENT
            };
            s
        })
    ])
    .width(Length::Fill)
    .height(style::CONTROL_HEIGHT)
    .padding([0, 6])
    .style(style::quiet)
    .on_press_maybe(action)
    .into()
}
pub const ICON_SIZE: f32 = 18.;

impl Icon {
    pub fn view<'a>(self) -> svg::Svg<'a> {
        let path = match self {
            Self::Network => {
                "<rect x='9' y='2' width='6' height='6'/><rect x='2' y='16' width='6' height='6'/><rect x='16' y='16' width='6' height='6'/><path d='M12 8v4M5 16v-4h14v4'/>"
            }
            Self::Pin => {
                "<path d='M20 10c0 5-8 11-8 11S4 15 4 10a8 8 0 1 1 16 0Z'/><circle cx='12' cy='10' r='2.6'/>"
            }
            Self::LocateFixed => {
                "<g stroke-width='2'><circle cx='12' cy='12' r='7'/><circle cx='12' cy='12' r='2'/><path d='M12 2v3m0 14v3M2 12h3m14 0h3'/></g>"
            }
            Self::Book => {
                "<path d='M12 6C9 3 5 3 2 4v15c4-1 7-1 10 2 3-3 6-3 10-2V4c-3-1-7-1-10 2Zm0 0v15'/>"
            }
            Self::Wrench => {
                "<path d='M13 9l4 4 4-4a6 6 0 0 1-8 7l-6 6a2 2 0 0 1-3-3l6-6a6 6 0 0 1 7-8Z'/>"
            }
            Self::Folder => {
                "<path d='M3 6a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z'/>"
            }
            Self::Target => {
                "<circle cx='12' cy='12' r='9'/><circle cx='12' cy='12' r='5'/><circle cx='12' cy='12' r='1'/>"
            }
            Self::Screen => {
                "<rect x='2' y='4' width='20' height='14' rx='2'/><path d='M8 22h8m-4-4v4'/>"
            }
            Self::PhonePortrait => {
                "<rect x='5' y='2' width='14' height='20' rx='2'/><path d='M12 18h.01'/>"
            }
            Self::PhoneLandscape => {
                "<rect x='2' y='5' width='20' height='14' rx='2'/><path d='M6 12h.01'/>"
            }
            Self::Camera => "<path d='M4 9V5h4m8 0h4v4m0 6v4h-4m-8 0H4v-4M3 12h18'/>",
            Self::Copy => {
                "<rect x='8' y='8' width='12' height='12' rx='2'/><path d='M16 4H6a2 2 0 0 0-2 2v10'/>"
            }
            Self::Cut => {
                "<circle cx='6' cy='6' r='3'/><circle cx='6' cy='18' r='3'/><path d='m8 8 12 12M8 16 20 4'/>"
            }
            Self::Up => "<path d='m6 10 6-6 6 6m-6-6v16'/>",
            Self::Grip => {
                "<path d='M9 5h.1M15 5h.1M9 12h.1m6 0h.1M9 19h.1m6 0h.1' stroke-width='3'/>"
            }
            Self::Plus => "<path d='M12 5v14M5 12h14'/>",
            Self::Minus => "<path d='M5 12h14'/>",
            Self::Back => "<path d='m12 19-7-7 7-7M5 12h14'/>",
            Self::Edit => "<path d='m16 3 5 5L8 21H3v-5ZM14 5l5 5'/>",
            Self::Trash => "<path d='M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7'/>",
            Self::Eye => {
                "<path d='M2 12s3-7 10-7 10 7 10 7-3 7-10 7S2 12 2 12Z'/><circle cx='12' cy='12' r='3'/>"
            }
            Self::EyeOff => {
                "<path d='m3 3 18 18M10.6 10.6a2 2 0 0 0 2.8 2.8M9 5.5A10 10 0 0 1 12 5c7 0 10 7 10 7a18 18 0 0 1-3 4M6 6a18 18 0 0 0-4 6s3 7 10 7a12 12 0 0 0 5-1'/>"
            }
            Self::Paperclip => {
                "<path d='m21 11-9 9a6 6 0 0 1-8-8L14 2a4 4 0 0 1 6 6L10 18a2 2 0 0 1-3-3l9-9'/>"
            }
            Self::Todos => "<path d='m3 6 1.5 1.5L7 4m3 2h11M3 12h3m4 0h11M3 18h3m4 0h11'/>",
            Self::Send => "<path d='m5 12 7-7 7 7M12 5v14'/>",
            Self::Stop => "<rect x='6' y='6' width='12' height='12' rx='2'/>",
            Self::Pause => "<path d='M8 5v14M16 5v14'/>",
            Self::Play => "<path d='m8 5 11 7-11 7Z'/>",
            Self::Down => "<path d='m6 9 6 6 6-6'/>",
            Self::Right => "<path d='m9 6 6 6-6 6'/>",
            Self::Download => "<path d='M12 3v12m-5-5 5 5 5-5M5 16v5h14v-5'/>",
            Self::Upload => "<path d='M12 15V3m-5 5 5-5 5 5M5 16v5h14v-5'/>",
            Self::Search => "<circle cx='11' cy='11' r='7'/><path d='m16 16 4 4'/>",
            Self::Link => {
                "<path d='M10 13a4 4 0 0 0 6 0l3-3a4 4 0 0 0-6-6l-2 2m3 5a4 4 0 0 0-6 0l-3 3a4 4 0 0 0 6 6l2-2'/>"
            }
            Self::Undo => "<path d='M9 14l-5-5 5-5M4 9h10a6 6 0 0 1 0 12h-2'/>",
            Self::Close => "<path d='m6 6 12 12M18 6 6 18'/>",
            Self::Image => {
                "<rect x='3' y='3' width='18' height='18' rx='2'/><circle cx='9' cy='9' r='2'/><path d='m3 18 5-5 3 3 4-4 6 6'/>"
            }
            Self::Check => "<path d='m5 12 4 4L19 6'/>",
            Self::Settings => {
                "<path d='m10 2-.7 3-2 .9L4.5 5 2 9.3l2.2 2.1v2L2 15.5 4.5 20l2.8-.9 2 .9.7 3h5l.7-3 2-.9 2.8.9 2.5-4.5-2.2-2.1v-2L23 9.3 20.5 5l-2.8.9-2-.9L15 2Z'/><circle cx='12.5' cy='12.5' r='3'/>"
            }
            Self::Compare => {
                r#"<rect x="2" y="5" width="9" height="14" rx="2"/><rect x="14" y="5" width="8" height="14" rx="2"/>"#
            }
            Self::Cable => {
                "<path d='M7 2v4M11 2v4M5 6h8v4a4 4 0 0 1-8 0ZM9 14v3a4 4 0 0 0 8 0v-3M15 10h4v4h-4Z'/>"
            }
            Self::Refresh => {
                "<path d='M20 7v5h-5M4 17v-5h5M6 6a8 8 0 0 1 13 3M18 18a8 8 0 0 1-13-3'/>"
            }
            Self::Volume => {
                "<path d='M11 5 6 9H3v6h3l5 4zM15.5 8.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13'/>"
            }
            Self::Muted => "<path d='M11 5 6 9H3v6h3l5 4zM16 9l6 6m0-6-6 6'/>",
            Self::Compass => {
                "<circle cx='12' cy='12' r='9'/><path d='m15.8 8.2-2.3 5.3-5.3 2.3 2.3-5.3Z'/>"
            }
            Self::Mic => {
                "<rect x='9' y='2' width='6' height='12' rx='3'/><path d='M5 10v2a7 7 0 0 0 14 0v-2m-7 9v3'/>"
            }
            Self::MicOff => {
                "<path d='m2 2 20 20M9 9v2a3 3 0 0 0 5 2m1-4V5a3 3 0 0 0-5.6-1.5M5 10v2a7 7 0 0 0 12 4.9m2-4.9v-2m-7 9v3'/>"
            }
        };
        use std::sync::OnceLock;
        static HANDLES: [OnceLock<svg::Handle>; Icon::MicOff as usize + 1] =
            [const { OnceLock::new() }; Icon::MicOff as usize + 1];
        let handle = HANDLES[self as usize].get_or_init(|| svg::Handle::from_memory(format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#171717' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'>{path}</svg>").into_bytes()));
        svg(handle.clone())
            .width(ICON_SIZE)
            .height(ICON_SIZE)
            .style(|theme: &iced::Theme, _| svg::Style {
                color: Some(theme.palette().text),
            })
    }
}
pub fn icon_button<'a, M: Clone + 'a>(icon: Icon, action: Option<M>) -> Element<'a, M> {
    sized_icon_button(icon, action, style::CONTROL_HEIGHT, style::CONTROL_HEIGHT)
}

pub fn sized_icon_button<'a, M: Clone + 'a>(
    icon: Icon,
    action: Option<M>,
    width: f32,
    height: f32,
) -> Element<'a, M> {
    button(
        container(icon.view().opacity(if action.is_some() { 1. } else { 0.5 }))
            .center(Length::Fill),
    )
    .width(width)
    .height(height)
    .padding(0)
    .on_press_maybe(action)
    .style(style::quiet)
    .into()
}
const FIELD_PADDING: [f32; 2] = [(style::CONTROL_HEIGHT - style::FIELD_LINE.0) / 2., 12.];

pub fn form_input<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
) -> text_input::TextInput<'a, M> {
    text_input(placeholder, value)
        .size(14)
        .line_height(style::FIELD_LINE)
        .padding(FIELD_PADDING)
        .style(style::input)
}

pub fn form_select<'a, T, L, V, M>(
    options: L,
    selected: Option<V>,
    on_select: impl Fn(T) -> M + 'a,
) -> pick_list::PickList<'a, T, L, V, M>
where
    T: ToString + PartialEq + Clone + 'a,
    L: Borrow<[T]> + 'a,
    V: Borrow<T> + 'a,
    M: Clone + 'a,
{
    pick_list(options, selected, on_select)
        .text_size(14)
        .text_line_height(style::FIELD_LINE)
        .padding(FIELD_PADDING)
        .style(style::select)
        .menu_style(style::menu)
}

pub fn field<'a, M: Clone + 'a>(
    label: &'a str,
    value: &'a str,
    on_input: impl Fn(String) -> M + 'a,
    enabled: bool,
) -> Element<'a, M> {
    column![
        text(label).size(12).style(style::muted_text),
        form_input("", value).on_input_maybe(enabled.then_some(on_input))
    ]
    .spacing(6)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workbench_actions_keep_the_same_height_for_long_and_short_labels() {
        let actions: [Element<'_, ()>; 5] = [
            text_button("设备").into(),
            action_button("Save and reconnect", Some(())).into(),
            action_button("Speichern und erneut verbinden", None).into(),
            icon_button(Icon::Plus, Some(())),
            tab_button("Model", true, Some(())),
        ];
        for action in actions {
            assert_eq!(
                action.as_widget().size().height,
                Length::Fixed(style::CONTROL_HEIGHT)
            );
        }
    }
}
