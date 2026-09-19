use iced::{
    Border, Color, Element, Event, Length, Point, Rectangle, Shadow, Size, Theme,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse,
        renderer::Quad,
        text::{self, Text},
        widget::Tree,
    },
    alignment,
};

use crate::clip_components::{ClipComponent, video_clip::VideoComponent};


pub const MIN_LENGTH: f32 = 48.0;
pub const HEIGHT: f32 = 52.0;

const LABEL_PADDING: f32 = 13.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Body,
}

#[derive(Debug, Clone, Copy)]
pub struct Appearance {
    pub active: bool,
}

/// An editor-style clip widget. Timeline placement is still owned by
/// [`Timeline`](super::timeline::Timeline), while clip sizing, hit-testing, and
/// rendering live here.
pub struct ClipEntry<Message> {
    pub name: String,
    pub position: Point,
    /// ClipEntry duration, measured in timeline pixels for this small example widget.
    pub length: f32,
    pub component: Box< dyn ClipComponent >,
    pub on_press: Option<Message>,
}

impl<Message> ClipEntry<Message> {
    pub fn new(name: impl Into<String>, length: f32) -> Self {
        Self {
            name: name.into(),
            position: Point { x: 0.0, y: 0.0 },
            length: length.max(MIN_LENGTH),
            component: Box::new(VideoComponent::default()),
            on_press: None,
        }
    }

    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    pub fn hit_test(bounds: Rectangle, position: Point) -> Option<Hit> {
        if bounds.contains(position) {
            Some(Hit::Body)
        } else {
            None
        }
    }

    pub fn mouse_interaction_at(bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        let Some(position) = cursor.position() else {
            return mouse::Interaction::None;
        };

        match Self::hit_test(bounds, position) {
            Some(Hit::Body) => mouse::Interaction::Grab,
            None => mouse::Interaction::None,
        }
    }

    pub fn draw_at<Renderer>(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        bounds: Rectangle,
        appearance: Appearance,
        viewport: &Rectangle,
    ) where
        Renderer: iced::advanced::Renderer + text::Renderer,
    {
        let palette = theme.palette();
        let fill = if appearance.active {
            palette.primary
        } else {
            palette.background
        };

        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    color: Color {
                        a: 0.9,
                        ..palette.text
                    },
                    width: 1.0,
                    radius: 4.0.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            fill,
        );
        renderer.fill_text(
            Text {
                content: self.name.clone(),
                size: 14.0.into(),
                line_height: text::LineHeight::default(),
                font: renderer.default_font(),
                bounds: Size::new((bounds.width - LABEL_PADDING * 2.0).max(1.0), 18.0),
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Center,
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::None,
            },
            Point::new(bounds.x + LABEL_PADDING, bounds.center_y()),
            Color::WHITE,
            *viewport,
        );
    }
}

impl<Message> Default for ClipEntry<Message> {
    fn default() -> Self {
        Self::new("Untitled clip", 180.0)
    }
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for ClipEntry<Message>
where
    Renderer: iced::advanced::Renderer + text::Renderer,
    Message: Clone,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(self.length).height(HEIGHT);
        layout::Node::new(limits.resolve(
            Length::Fixed(self.length),
            Length::Fixed(HEIGHT),
            Size::ZERO,
        ))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.draw_at(
            renderer,
            theme,
            layout.bounds(),
            Appearance { active: false },
            viewport,
        );
    }

    fn update(
        &mut self,
        _tree: &mut Tree,
        _event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        _shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = _event {
            if Self::hit_test(
                _layout.bounds(),
                _cursor.position().unwrap_or(Point { x: 0.0, y: 0.0 }),
            )
            .is_some()
            {
                if let Some(message) = &self.on_press {
                    _shell.publish(message.clone());
                }
            }
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        Self::mouse_interaction_at(layout.bounds(), cursor)
    }
}

impl<'a, Message: 'a, Renderer> From<ClipEntry<Message>> for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer + text::Renderer + 'a,
    Message: Clone,
{
    fn from(widget: ClipEntry<Message>) -> Self {
        Self::new(widget)
    }
}
