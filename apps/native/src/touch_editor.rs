use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, renderer,
    text::highlighter::PlainText,
    widget::{Operation, Tree, tree},
};
use iced::{
    Element, Event, Length, Rectangle, Renderer, Size, Theme, mouse, touch, widget::TextEditor,
};

pub fn editor<'a, M: 'a>(editor: TextEditor<'a, PlainText, M>) -> Element<'a, M> {
    Element::new(TouchEditor(editor.into()))
}

struct TouchEditor<'a, M>(Element<'a, M>);

impl<M> Widget<M, Theme, Renderer> for TouchEditor<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Option<touch::Finger>>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(None::<touch::Finger>)
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.0)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.children[0].diff(&self.0);
    }
    fn size(&self) -> Size<Length> {
        self.0.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.0
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.0
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let active = tree.state.downcast_mut::<Option<touch::Finger>>();
        let mapped;
        let (event, cursor) = if let Event::Touch(touch) = event {
            let (position, button) = match *touch {
                touch::Event::FingerPressed { id, position } if active.is_none() => {
                    if layout.bounds().contains(position) && viewport.contains(position) {
                        *active = Some(id);
                    }
                    (position, mouse::Event::ButtonPressed(mouse::Button::Left))
                }
                touch::Event::FingerMoved { id, position } if *active == Some(id) => {
                    (position, mouse::Event::CursorMoved { position })
                }
                touch::Event::FingerLifted { id, position }
                | touch::Event::FingerLost { id, position }
                    if *active == Some(id) =>
                {
                    *active = None;
                    (position, mouse::Event::ButtonReleased(mouse::Button::Left))
                }
                _ => return,
            };
            mapped = Event::Mouse(button);
            (&mapped, mouse::Cursor::Available(position))
        } else {
            (event, cursor)
        };
        self.0.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
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
        self.0.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.0
            .as_widget()
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }
}
