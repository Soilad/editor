use std::rc::Rc;

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

use crate::clip_components::{ ClipComponent, video_clip::VideoComponent };

pub const MIN_LENGTH: f32 = 48.0;
pub const HEIGHT: f32 = 52.0;

const HANDLE_WIDTH: f32 = 8.0;
const LABEL_PADDING: f32 = 13.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    StartHandle,
    EndHandle,
    Body,
}

#[derive(Debug, Clone, Copy)]
pub struct Appearance {
    pub active: bool,
}

/// An editor-style clip widget. Timeline placement is still owned by
/// [`Timeline`](super::timeline::Timeline), while clip sizing, hit-testing, and
/// rendering live here.
#[derive(Clone)]
pub struct ClipTimeline {
    pub name: String,
    /// Clip duration, measured in timeline pixels for this small example widget.
    pub length: f32,
    pub component: Rc<dyn ClipComponent>,
}

impl ClipTimeline {
    pub fn new(
        name: impl Into<String>,
        length: f32,
        component: Rc<dyn ClipComponent>,
    ) -> Self {
        Self {
            name: name.into(),
            length: length.max(MIN_LENGTH),
            component,
        }
    }

    pub fn left_handle_bounds(bounds: Rectangle) -> Rectangle {
        Rectangle::new(
            bounds.position(),
            Size::new(HANDLE_WIDTH.min(bounds.width), bounds.height),
        )
    }

    pub fn right_handle_bounds(bounds: Rectangle) -> Rectangle {
        Rectangle::new(
            Point::new(bounds.x + (bounds.width - HANDLE_WIDTH).max(0.0), bounds.y),
            Size::new(HANDLE_WIDTH.min(bounds.width), bounds.height),
        )
    }

    pub fn resize_from_start(start: &mut f32, length: &mut f32, new_start: f32) {
        let end = *start + *length;
        *start = new_start.min(end - MIN_LENGTH).max(0.0);
        *length = (end - *start).max(MIN_LENGTH);
    }

    pub fn resize_from_end(start: f32, length: &mut f32, new_end: f32) {
        *length = (new_end - start).max(MIN_LENGTH);
    }

    pub fn hit_test(bounds: Rectangle, position: Point) -> Option<Hit> {
        if Self::left_handle_bounds(bounds).contains(position) {
            Some(Hit::StartHandle)
        } else if Self::right_handle_bounds(bounds).contains(position) {
            Some(Hit::EndHandle)
        } else if bounds.contains(position) {
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
            Some(Hit::StartHandle | Hit::EndHandle) => mouse::Interaction::ResizingHorizontally,
            Some(Hit::Body) => mouse::Interaction::Grab,
            None => mouse::Interaction::None,
        }
    }

    pub fn draw_at<Renderer>(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        bounds: Rectangle,
        viewport: &Rectangle,
    ) where
        Renderer: iced::advanced::Renderer + text::Renderer,
    {
        let palette = theme.palette();
        let fill = palette.background;

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
        renderer.fill_quad(
            Quad {
                bounds: Self::left_handle_bounds(bounds),
                border: Border::default(),
                shadow: Shadow::default(),
                snap: false,
            },
            palette.text,
        );
        renderer.fill_quad(
            Quad {
                bounds: Self::right_handle_bounds(bounds),
                border: Border::default(),
                shadow: Shadow::default(),
                snap: false,
            },
            palette.text,
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

impl Default for ClipTimeline {
    fn default() -> Self {
        Self::new(
            "Untitled clip",
            180.0,
            Rc::new(VideoComponent::default()),
        )
    }
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for ClipTimeline
where
    Renderer: iced::advanced::Renderer + text::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.length), Length::Fixed(HEIGHT))
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

impl<'a, Message: 'a, Renderer> From<ClipTimeline> for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer + text::Renderer + 'a,
{
    fn from(widget: ClipTimeline) -> Self {
        Self::new(widget)
    }
}

