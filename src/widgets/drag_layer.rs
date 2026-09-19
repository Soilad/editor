use iced::{
    advanced::{
        layout, mouse, renderer, widget::{Tree, Widget}, Clipboard, Layout, Shell,
    },
    Element, Event, Length, Point, Rectangle, Size, Theme,
};

pub struct DragLayer<'a, Message, Renderer> {
    base: Element<'a, Message, Theme, Renderer>,
    cursor_position: Point,
    dragging_clip: Option<&'a str>,
}

impl<'a, Message, Renderer> DragLayer<'a, Message, Renderer> {
    pub fn new(
        base: impl Into<Element<'a, Message, Theme, Renderer>>,
        cursor_position: Point,
        dragging_clip: Option<&'a str>,
    ) -> Self {
        Self {
            base: base.into(),
            cursor_position,
            dragging_clip,
        }
    }
}

impl<'a, Message, Renderer> Widget<Message, Theme, Renderer> for DragLayer<'a, Message, Renderer>
where
    Renderer: iced::advanced::Renderer + iced::advanced::text::Renderer,
{
    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.base.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
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
        self.base.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);

        if let Some(name) = self.dragging_clip {
            let width = 100.0;
            let height = 52.0;
            let bounds = Rectangle::new(
                Point::new(self.cursor_position.x - width / 2.0, self.cursor_position.y - height / 2.0),
                Size::new(width, height),
            );

            // Draw a semi-transparent clip background
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border {
                        color: iced::Color::from_rgba(1.0, 1.0, 1.0, 0.8),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    shadow: iced::Shadow::default(),
                    snap: false,
                },
                theme.palette().primary,
            );
            
            renderer.fill_text(
                iced::advanced::text::Text {
                    content: name.to_string(),
                    size: 14.0.into(),
                    line_height: iced::advanced::text::LineHeight::default(),
                    font: renderer.default_font(),
                    bounds: Size::new(width - 26.0, 18.0),
                    align_x: iced::alignment::Horizontal::Left,
                    align_y: iced::alignment::Vertical::Center,
                    shaping: iced::advanced::text::Shaping::Basic,
                    wrapping: iced::advanced::text::Wrapping::None,
                },
                Point::new(bounds.x + 13.0, bounds.center_y()),
                iced::Color::WHITE,
                *viewport,
            );
        }
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.base)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.base))
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
        self.base.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.base.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
}

impl<'a, Message: 'a, Renderer> From<DragLayer<'a, Message, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer + iced::advanced::text::Renderer + 'a,
{
    fn from(widget: DragLayer<'a, Message, Renderer>) -> Self {
        Self::new(widget)
    }
}
