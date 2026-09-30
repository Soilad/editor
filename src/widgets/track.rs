//! One track of the [`Timeline`](super::timeline::Timeline): its header in the
//! label column (an editable name and an enable toggle) and the lane its clips
//! sit on.

use std::rc::Rc;

use iced::{
    Alignment, Border, Color, Element, Event, Length, Point, Rectangle, Shadow, Size, Theme,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse,
        renderer::Quad,
        text,
        widget::{Operation, Tree},
    },
    widget::{button, row, text_input},
};

use avio::TrackId;

use crate::widgets::theme::{CORNER_RADIUS, button_style};

const PADDING: f32 = 8.0;
const TOGGLE_WIDTH: f32 = 34.0;

/// The header of one timeline track: a box holding the track's name input and a
/// button that enables or disables the track.
///
/// A disabled track contributes nothing to the preview or the export; its lane
/// is drawn dimmed by the timeline (see [`draw_lane`] and [`draw_disabled`]).
pub struct TrackRow<'a, Message, Renderer = iced::Renderer> {
    enabled: bool,
    content: Element<'a, Message, Theme, Renderer>,
}

impl<'a, Message, Renderer> TrackRow<'a, Message, Renderer>
where
    Message: Clone + 'a,
    Renderer: iced::advanced::Renderer + text::Renderer + 'a,
{
    /// `placeholder` is shown while `name` is empty. Without `on_rename` the name
    /// is read-only, and without `on_toggle` the toggle is inert.
    pub fn new(
        track: &'a avio::Track,
        name: &'a str,
        placeholder: &str,
        on_rename: Option<Rc<dyn Fn(TrackId, String) -> Message + 'a>>,
        on_toggle: Option<Rc<dyn Fn(TrackId, bool) -> Message + 'a>>,
    ) -> Self {
        let id = track.id;
        let enabled = track.enabled;

        let input = text_input(placeholder, name)
            .size(15)
            .padding([4, 6])
            .width(Length::Fill)
            .style(label_style);
        let input: Element<'a, Message, Theme, Renderer> = match on_rename {
            Some(on_rename) => input.on_input(move |name| on_rename(id, name)).into(),
            None => input.into(),
        };

        let toggle = button(
            iced::widget::text(if enabled { "On" } else { "Off" })
                .size(11)
                .center(),
        )
        .style(move |theme, status| toggle_style(theme, status, enabled))
        .padding([4, 0])
        .width(TOGGLE_WIDTH)
        .on_press_maybe(on_toggle.map(|f| f(id, !enabled)));

        Self {
            enabled,
            content: row![input, toggle]
                .spacing(4)
                .align_y(Alignment::Center)
                .into(),
        }
    }
}

impl<'a, Message, Renderer> Widget<Message, Theme, Renderer> for TrackRow<'a, Message, Renderer>
where
    Renderer: iced::advanced::Renderer + text::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.max();
        let node = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(
                Size::ZERO,
                Size::new(size.width - PADDING * 2.0, size.height),
            ),
        );
        // Center the content vertically in the header box.
        let y = (size.height - node.size().height) / 2.0;
        layout::Node::with_children(size, vec![node.move_to(Point::new(PADDING, y))])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                content_layout(layout),
                renderer,
                operation,
            );
        });
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let palette = theme.palette();
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(bounds.x, bounds.y + PADDING / 2.0),
                    Size::new(bounds.width - PADDING / 2.0, bounds.height - PADDING),
                ),
                border: Border {
                    color: Color {
                        a: if self.enabled { 1.0 } else { 0.4 },
                        ..palette.primary
                    },
                    width: 0.5,
                    radius: CORNER_RADIUS,
                },
                shadow: Shadow::default(),
                snap: false,
            },
            palette.background,
        );
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            content_layout(layout),
            cursor,
            viewport,
        );
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
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            content_layout(layout),
            cursor,
            renderer,
            clipboard,
            shell,
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
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            content_layout(layout),
            cursor,
            viewport,
            renderer,
        )
    }
}

impl<'a, Message, Renderer> From<TrackRow<'a, Message, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Renderer: iced::advanced::Renderer + text::Renderer + 'a,
{
    fn from(widget: TrackRow<'a, Message, Renderer>) -> Self {
        Self::new(widget)
    }
}

fn content_layout(layout: Layout<'_>) -> Layout<'_> {
    layout
        .children()
        .next()
        .expect("a track row lays out its content")
}

/// Draw the background of a track's lane, with a thin outline.
pub fn draw_lane<Renderer: iced::advanced::Renderer>(
    renderer: &mut Renderer,
    theme: &Theme,
    bounds: Rectangle,
) {
    let palette = theme.palette();
    renderer.fill_quad(
        Quad {
            bounds,
            border: Border {
                color: Color {
                    a: 0.20,
                    ..palette.primary
                },
                width: 0.5,
                radius: 0.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
        },
        palette.background,
    );
}

/// Dim a disabled track's lane, clips included. Draw it after the clips.
pub fn draw_disabled<Renderer: iced::advanced::Renderer>(
    renderer: &mut Renderer,
    theme: &Theme,
    bounds: Rectangle,
) {
    renderer.fill_quad(
        Quad {
            bounds,
            border: Border::default(),
            shadow: Shadow::default(),
            snap: false,
        },
        Color {
            a: 0.6,
            ..theme.palette().background
        },
    );
}

/// Filled with the primary color while the track is enabled, hollow otherwise.
fn toggle_style(theme: &Theme, status: button::Status, enabled: bool) -> button::Style {
    let base = button_style(theme, status);
    let palette = theme.palette();
    if enabled {
        button::Style {
            text_color: palette.background,
            background: Some(palette.primary.into()),
            ..base
        }
    } else {
        button::Style {
            text_color: Color {
                a: 0.5,
                ..palette.text
            },
            ..base
        }
    }
}

/// A borderless input that blends into the label column until focused.
fn label_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let default = text_input::default(theme, status);
    let border = match status {
        text_input::Status::Focused { .. } => Border {
            radius: CORNER_RADIUS,
            ..default.border
        },

        text_input::Status::Hovered => Border {
            color: Color {
                a: 0.4,
                ..theme.palette().text
            },
            ..Border {
                radius: CORNER_RADIUS,
                ..default.border
            }
        },
        _ => Border {
            width: 0.0,
            ..default.border
        },
    };
    text_input::Style {
        background: theme.palette().background.into(),
        border,
        ..default
    }
}
