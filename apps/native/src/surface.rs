use crate::{
    Message,
    connection::DeviceInput,
    controls::{Icon, sized_icon_button},
    scene::{Pointer, Scene},
    style,
};
use iced::advanced::{
    Clipboard, InputMethod, Layout, Shell, Widget, clipboard, input_method, layout, renderer,
    widget::{Tree, tree},
};
use iced::{
    Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{Shader, container, row, scrollable, shader},
};

pub fn tool_rect(view: Size, anchor: Rectangle, preferred: Size, beside: bool) -> Rectangle {
    let available_height = if beside {
        view.height - 88.
    } else {
        view.height - anchor.y - anchor.height - 26.
    };
    let width = preferred.width.min((view.width - 32.).max(0.));
    let height = preferred.height.min(available_height.max(80.));
    let right = anchor.x + anchor.width;
    let (left, top) = if beside {
        (right + 10., anchor.y)
    } else {
        (right - width, anchor.y + anchor.height + 10.)
    };
    Rectangle::new(
        Point::new(
            left.clamp(16., (view.width - width - 16.).max(16.)),
            top.clamp(16., (view.height - height - 16.).max(16.)),
        ),
        Size::new(width, height),
    )
}

pub fn tool_motion(anchor: Rectangle, panel: Rectangle, amount: f32) -> Rectangle {
    if amount == 1. {
        return panel;
    }
    let center = anchor.center();
    let factor = |trigger: f32, extent: f32| {
        let folded = (trigger / extent).max(0.035);
        folded + (1. - folded) * amount
    };
    let x = factor(anchor.width, panel.width);
    let y = factor(anchor.height, panel.height);
    Rectangle::new(
        Point::new(
            center.x + (panel.x - center.x) * x,
            center.y + (panel.y - center.y) * y,
        ),
        Size::new(panel.width * x, panel.height * y),
    )
}

pub fn window_progress(elapsed: std::time::Duration, open: bool) -> (f32, bool) {
    let (duration, curve) = if open {
        (style::WINDOW_ENTER, [0.2, 1.15, 0.3, 1.])
    } else {
        (style::WINDOW_EXIT, [0.32, 0., 0.65, 1.])
    };
    let time = (elapsed.as_secs_f32() / duration).min(1.);
    (style::bezier(time, curve), time == 1.)
}

pub fn tool_layer<'a, M: Clone + 'a>(
    content: Element<'a, M>,
    bounds: Rectangle,
    frame: Rectangle,
    focus: M,
) -> Element<'a, M> {
    let scale = (frame.width / bounds.width).min(frame.height / bounds.height);
    let offset = frame.center() - bounds.center();
    iced::widget::pin(
        iced::widget::float(
            iced::widget::mouse_area(
                container(iced::widget::themer(Some(style::theme()), content))
                    .width(bounds.width)
                    .height(bounds.height)
                    .clip(true),
            )
            .on_press(focus),
        )
        .scale(scale)
        .translate(move |_, _| offset),
    )
    .x(bounds.x)
    .y(bounds.y)
    .into()
}

pub fn tool_header<'a, M: 'a>(
    title: &'a str,
    viewport_width: f32,
    actions: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let (inset, trailing, height) = style::window_insets(viewport_width);
    let label = style::tool_title(title);
    container(
        row![
            container(label).width(Length::Fill).clip(true),
            actions.into()
        ]
        .align_y(iced::Center)
        .spacing(12),
    )
    .height(height)
    .width(Length::Fill)
    .padding(iced::Padding::ZERO.left(inset).right(trailing))
    .align_y(iced::Center)
    .into()
}

pub fn tool_padding(viewport_width: f32) -> iced::Padding {
    let (inset, _, _) = style::window_insets(viewport_width);
    iced::Padding::ZERO
        .top(8)
        .right(inset)
        .bottom(24)
        .left(inset)
}

pub fn tool_action<'a, M: Clone + 'a>(icon: Icon, message: Option<M>) -> Element<'a, M> {
    sized_icon_button(icon, message, 32., 32.)
}

pub fn tool_scroll() -> scrollable::Direction {
    scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(4).scroller_width(4))
}

pub struct Surface {
    shader: Shader<Message, Scene>,
    keyboard_target: Option<usize>,
}
impl Surface {
    pub fn new(scene: Scene) -> Self {
        Self {
            keyboard_target: scene.keyboard_target,
            shader: shader(scene).width(Length::Fill).height(Length::Fill),
        }
    }
}
impl Widget<Message, Theme, Renderer> for Surface {
    fn tag(&self) -> tree::Tag {
        <Shader<Message, Scene> as Widget<Message, Theme, Renderer>>::tag(&self.shader)
    }
    fn state(&self) -> tree::State {
        <Shader<Message, Scene> as Widget<Message, Theme, Renderer>>::state(&self.shader)
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        <Shader<Message, Scene> as Widget<Message, Theme, Renderer>>::layout(
            &mut self.shader,
            tree,
            renderer,
            limits,
        )
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Some(target) = self.keyboard_target {
            let state = tree.state.downcast_mut::<Pointer>();
            match event {
                Event::InputMethod(input_method::Event::Preedit(content, selection)) => {
                    state.preedit = (!content.is_empty()).then(|| input_method::Preedit {
                        content: content.clone(),
                        selection: selection.clone(),
                        text_size: Some(14.into()),
                    });
                    shell.request_redraw();
                    shell.capture_event();
                }
                Event::InputMethod(input_method::Event::Commit(text)) => {
                    state.preedit = None;
                    shell.publish(Message::Input(target, DeviceInput::Text(text.clone())));
                    shell.capture_event();
                }
                Event::InputMethod(input_method::Event::Closed) => state.preedit = None,
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                    if modifiers.command() && key.as_ref() == keyboard::Key::Character("v") =>
                {
                    if let Some(text) = clipboard.read(clipboard::Kind::Standard) {
                        shell.publish(Message::Input(target, DeviceInput::Text(text)));
                    }
                    shell.capture_event();
                }
                Event::Keyboard(_) if state.preedit.is_some() => shell.capture_event(),
                _ => {}
            }
            let origin = layout.bounds();
            shell.request_input_method(&InputMethod::Enabled {
                cursor: Rectangle::new(
                    state.anchor + iced::Vector::new(origin.x, origin.y),
                    Size::new(1., 20.),
                ),
                purpose: input_method::Purpose::Normal,
                preedit: state.preedit.clone(),
            });
        }
        if !shell.is_event_captured() {
            <Shader<Message, Scene> as Widget<Message, Theme, Renderer>>::update(
                &mut self.shader,
                tree,
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.shader
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        <Shader<Message, Scene> as Widget<Message, Theme, Renderer>>::mouse_interaction(
            &self.shader,
            tree,
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
}
impl From<Surface> for Element<'_, Message> {
    fn from(surface: Surface) -> Self {
        Element::new(surface)
    }
}
