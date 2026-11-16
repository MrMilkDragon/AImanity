use crate::style;
use iced::{
    Element, Length,
    widget::{button, column, container, rule, svg, text, text_input, tooltip},
};

#[derive(Clone, Copy)]
pub enum Icon {
    Plus,
    Back,
    Edit,
    Trash,
    Eye,
    EyeOff,
    Paperclip,
    Send,
    Stop,
    Pause,
    Play,
    Down,
    Right,
    Download,
    Check,
    Settings,
    Cable,
    Refresh,
}
pub fn action_button<'a, M: Clone + 'a>(
    label: &'a str,
    action: Option<M>,
) -> button::Button<'a, M> {
    button(container(text(label).size(13)).center(Length::Fill))
        .width(Length::Fill)
        .height(style::CONTROL_HEIGHT)
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
impl Icon {
    pub fn view<'a>(self) -> svg::Svg<'a> {
        let path = match self {
            Self::Plus => "<path d='M12 5v14M5 12h14'/>",
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
            Self::Send => "<path d='m5 12 7-7 7 7M12 5v14'/>",
            Self::Stop => "<rect x='6' y='6' width='12' height='12' rx='2'/>",
            Self::Pause => "<path d='M8 5v14M16 5v14'/>",
            Self::Play => "<path d='m8 5 11 7-11 7Z'/>",
            Self::Down => "<path d='m6 9 6 6 6-6'/>",
            Self::Right => "<path d='m9 6 6 6-6 6'/>",
            Self::Download => "<path d='M12 3v12m-5-5 5 5 5-5M5 16v5h14v-5'/>",
            Self::Check => "<path d='m5 12 4 4L19 6'/>",
            Self::Settings => {
                "<path d='m10 2-.7 3-2 .9L4.5 5 2 9.3l2.2 2.1v2L2 15.5 4.5 20l2.8-.9 2 .9.7 3h5l.7-3 2-.9 2.8.9 2.5-4.5-2.2-2.1v-2L23 9.3 20.5 5l-2.8.9-2-.9L15 2Z'/><circle cx='12.5' cy='12.5' r='3'/>"
            }
            Self::Cable => {
                "<path d='M7 2v4M11 2v4M5 6h8v4a4 4 0 0 1-8 0ZM9 14v3a4 4 0 0 0 8 0v-3M15 10h4v4h-4Z'/>"
            }
            Self::Refresh => {
                "<path d='M20 7v5h-5M4 17v-5h5M6 6a8 8 0 0 1 13 3M18 18a8 8 0 0 1-13-3'/>"
            }
        };
        svg(svg::Handle::from_memory(format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#171717' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'>{path}</svg>").into_bytes())).width(18).height(18)
    }
}
pub fn icon_button<'a, M: Clone + 'a>(
    icon: Icon,
    label: &'a str,
    action: Option<M>,
) -> Element<'a, M> {
    tooltip(
        button(
            container(
                icon.view()
                    .opacity(if action.is_some() { 1. } else { 0.35 }),
            )
            .center(Length::Fill),
        )
        .width(style::CONTROL_HEIGHT)
        .height(style::CONTROL_HEIGHT)
        .on_press_maybe(action)
        .style(style::quiet),
        text(label).size(12),
        tooltip::Position::Bottom,
    )
    .into()
}
pub fn field<'a, M: Clone + 'a>(
    label: &'a str,
    value: &'a str,
    on_input: impl Fn(String) -> M + 'a,
    enabled: bool,
) -> Element<'a, M> {
    column![
        text(label).size(12).color(style::MUTED),
        text_input("", value)
            .on_input_maybe(enabled.then_some(on_input))
            .size(14)
            .line_height(iced::Pixels(20.))
            .padding([10, 12])
            .style(style::input)
    ]
    .spacing(6)
    .into()
}
