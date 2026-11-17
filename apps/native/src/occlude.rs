use iced::advanced::Renderer as _;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{Operation, Tree, tree},
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, mouse};

pub fn behind<'a, M: 'a>(content: Element<'a, M>, panel: Rectangle) -> Element<'a, M> {
    Element::new(Behind {
        content,
        occlusion: Occlusion::Panel(panel),
    })
}

pub fn scroll_around_devices<'a, M: 'a>(
    content: Element<'a, M>,
    layout: crate::layout::Layout,
    focus: usize,
    linked: bool,
) -> Element<'a, M> {
    Element::new(Behind {
        content,
        occlusion: Occlusion::DeviceInput {
            layout,
            focus,
            linked,
        },
    })
}

enum Occlusion {
    Panel(Rectangle),
    DeviceInput {
        layout: crate::layout::Layout,
        focus: usize,
        linked: bool,
    },
}
struct Behind<'a, M> {
    content: Element<'a, M>,
    occlusion: Occlusion,
}
impl<M> Widget<M, Theme, Renderer> for Behind<'_, M> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
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
        let blocked = match (&self.occlusion, event) {
            (Occlusion::Panel(panel), Event::Mouse(_)) => {
                cursor.position().is_some_and(|p| panel.contains(p))
            }
            (
                Occlusion::DeviceInput {
                    layout,
                    focus,
                    linked,
                },
                Event::Mouse(mouse::Event::WheelScrolled { .. }),
            ) => cursor
                .position()
                .is_some_and(|point| layout.device_at(*focus, *linked, point).is_some()),
            (
                Occlusion::DeviceInput { layout, focus, .. },
                Event::Touch(iced::touch::Event::FingerPressed { position, .. }),
            ) => layout.device_at(*focus, true, *position).is_some(),
            _ => false,
        };
        if blocked {
            return;
        }
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
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
        let Some(bounds) = viewport.intersection(&layout.bounds()) else {
            return;
        };
        let viewport = &bounds;
        let hole = match self.occlusion {
            Occlusion::Panel(panel) => viewport.intersection(&panel),
            Occlusion::DeviceInput { .. } => None,
        };
        let Some(hole) = hole else {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
            return;
        };
        let regions = [
            Rectangle {
                x: viewport.x,
                y: viewport.y,
                width: viewport.width,
                height: hole.y - viewport.y,
            },
            Rectangle {
                x: viewport.x,
                y: hole.y + hole.height,
                width: viewport.width,
                height: viewport.y + viewport.height - hole.y - hole.height,
            },
            Rectangle {
                x: viewport.x,
                y: hole.y,
                width: hole.x - viewport.x,
                height: hole.height,
            },
            Rectangle {
                x: hole.x + hole.width,
                y: hole.y,
                width: viewport.x + viewport.width - hole.x - hole.width,
                height: hole.height,
            },
        ];
        for region in regions
            .into_iter()
            .filter(|r| r.width > 0. && r.height > 0.)
        {
            renderer.with_layer(region, |renderer| {
                self.content
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, &region)
            });
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if matches!(self.occlusion, Occlusion::Panel(panel) if cursor.position().is_some_and(|p| panel.contains(p)))
        {
            return mouse::Interaction::None;
        }
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: iced::Vector,
    ) -> Option<overlay::Element<'a, M, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}
