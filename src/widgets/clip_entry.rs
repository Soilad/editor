use std::{path::PathBuf, rc::Rc};

use iced::{
    Border, Color, Element, Event, Length, Point, Rectangle, Shadow, Size, Theme,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse,
        renderer::Quad,
        text::{self, Text},
        widget::{Tree, tree},
    },
    alignment,
};

use crate::clip_components::{ClipComponent, video_clip::VideoComponent};

pub const MIN_LENGTH: f32 = 48.0;
pub const HEIGHT: f32 = 52.0;

const LABEL_PADDING: f32 = 13.0;

#[derive(Debug, Default, Clone, Copy)]
struct DragState {
    dragging: bool,
    grab_offset: iced::Vector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Body,
}

/// An editor-style clip widget. Timeline placement is still owned by
/// [`Timeline`](super::timeline::Timeline), while clip sizing, hit-testing, and
/// rendering live here.
#[derive(Clone)]
pub struct ClipEntry<Message> {
    pub name: String,
    pub position: Point,
    /// ClipEntry duration, measured in timeline pixels for this small example widget.
    pub length: f32,
    pub component: Option<Rc<dyn ClipComponent>>,
    pub avio_component: Option<avio::Clip>,
    pub on_press: Option<Message>,
    pub on_drag: Option<Rc<dyn Fn(Point) -> Message>>,
    pub on_drop: Option<Message>,
}

impl<Message> ClipEntry<Message> {
    pub fn new(name: impl Into<String>, length: f32, path: PathBuf) -> Self {
        Self {
            name: name.into(),
            position: Point { x: 100.0, y: 0.0 },
            length: length.max(MIN_LENGTH),
            component: Some(
                Rc::new(
                    VideoComponent{
                        path: path.clone()
                    }
                )
            ),
            avio_component: Some(
                avio::Clip::new(
                    path,
                ),
            ),
            on_press: None,
            on_drag: None,
            on_drop: None,
        }
    }

    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    pub fn on_drag(mut self, f: impl Fn(Point) -> Message + 'static) -> Self {
        self.on_drag = Some(Rc::new(f));
        self
    }

    pub fn on_drop(mut self, message: Message) -> Self {
        self.on_drop = Some(message);
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

    fn moved_bounds(&self, bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: self.position.x,
            y: self.position.y,
            ..bounds
        }
    }
}

// impl<Message> Default for ClipEntry<Message> {
//     fn default() -> Self {
//         Self::new("Untitled clip", 180.0)
//     }
// }

impl<Message, Renderer> Widget<Message, Theme, Renderer> for ClipEntry<Message>
where
    Renderer: iced::advanced::Renderer + text::Renderer,
    Message: Clone,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<DragState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(DragState::default())
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
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<DragState>();
        let palette = theme.palette();
        let bounds = if let Some(position) = _cursor.position()
            && state.dragging
        {
            let bounds = layout.bounds();
            Rectangle {
                x: position.x - state.grab_offset.x,
                y: position.y - state.grab_offset.y,
                ..bounds
            }
        } else {
            layout.bounds()
        };

        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    color: Color {
                        a: 0.9,
                        ..palette.primary
                    },
                    width: 1.0,
                    radius: 4.0.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            palette.background,
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

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<DragState>();
        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position() {
                    if Self::hit_test(bounds, pos).is_some() {
                        state.dragging = true;
                        state.grab_offset = pos - bounds.position();
                        if let Some(m) = &self.on_press {
                            shell.publish(m.clone());
                        }
                        shell.capture_event();
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
                if let Some(f) = &self.on_drag {
                    let new_pos = *position - state.grab_offset;
                    shell.publish(f(Point::new(new_pos.x, new_pos.y)));
                }
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                if let Some(m) = &self.on_drop {
                    shell.publish(m.clone());
                }
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<DragState>();
        if state.dragging {
            mouse::Interaction::Grabbing
        } else {
            Self::mouse_interaction_at(layout.bounds(), cursor)
        }
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
