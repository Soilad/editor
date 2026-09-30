use std::{path::PathBuf, rc::Rc, sync::Arc};

use avio::{MediaInfo, TrackKind};

use iced::{
    Border, Color, Element, Event, Length, Point, Rectangle, Shadow, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay,
        renderer::{self, Quad},
        text::{self, Text},
        widget::{Tree, tree},
    },
    alignment,
};

use crate::{
    clip_components::{
        ClipComponent, audio_clip::AudioComponent, video_clip::VideoComponent,
    },
    thumbnails::SourcePreview,
    widgets::{clip_preview, theme::CORNER_RADIUS},
};

pub const MIN_LENGTH: f32 = 48.0;
pub const HEIGHT: f32 = 52.0;

const LABEL_PADDING: f32 = 13.0;
/// Gap between the entry's outline and its preview.
const PREVIEW_INSET: f32 = 4.0;

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
    /// Wraps the avio clip that gets added to the timeline when this entry is dropped on it.
    pub components: Vec<Arc<dyn ClipComponent>>,
    /// First frame and/or waveform of the entry's source file, once loaded.
    pub preview: Option<SourcePreview>,
    pub on_press: Option<Message>,
    pub on_drag: Option<Rc<dyn Fn(Point) -> Message>>,
    pub on_drop: Option<Message>,
}

impl<Message> ClipEntry<Message> {
    /// An entry for a media file, with a video and/or audio component for each
    /// kind of stream it has.
    pub fn new(name: impl Into<String>, info: &MediaInfo) -> Self {
        let (path, duration) = (info.path().to_path_buf(), info.duration());
        let mut components: Vec<Arc<dyn ClipComponent>> = Vec::new();
        // Files with no recognised streams still get a video clip, as before.
        if info.has_video() || !info.has_audio() {
            components.push(Arc::new(VideoComponent::new(path.clone(), duration)));
        }
        if info.has_audio() {
            components.push(Arc::new(AudioComponent::new(path, duration)));
        }
        Self::from_components(name, components)
    }

    /// An entry for any component, sized by its clip's duration.
    pub fn from_component(name: impl Into<String>, component: Arc<dyn ClipComponent>) -> Self {
        Self::from_components(name, vec![component])
    }

    /// An entry placing all of `components` together, sized by the longest.
    pub fn from_components(
        name: impl Into<String>,
        components: Vec<Arc<dyn ClipComponent>>,
    ) -> Self {
        let duration = components
            .iter()
            .filter_map(|component| component.avio_clip().duration())
            .max()
            .unwrap_or_default();
        Self {
            name: name.into(),
            position: Point { x: 100.0, y: 0.0 },
            length: duration.as_secs_f32().max(MIN_LENGTH),
            components,
            preview: None,
            on_press: None,
            on_drag: None,
            on_drop: None,
        }
    }

    /// The source file of this entry, if it has one, with whether a video and
    /// an audio preview should be loaded for it.
    pub fn media_source(&self) -> Option<(PathBuf, bool, bool)> {
        let path = self.components.iter().find_map(|c| c.get_path())?;
        let has = |kind| self.components.iter().any(|c| c.track_kind() == kind);
        Some((path, has(TrackKind::Video), has(TrackKind::Audio)))
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
    Renderer: iced::advanced::Renderer
        + text::Renderer
        + iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>,
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
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        // While dragging, the moving copy is drawn by the overlay so it stays on
        // top of the other panes; the entry itself stays put.
        draw_entry(
            renderer,
            theme,
            &self.name,
            self.preview.as_ref(),
            layout.bounds(),
            *viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        _translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_ref::<DragState>();
        if !state.dragging {
            return None;
        }
        Some(overlay::Element::new(Box::new(Ghost {
            name: &self.name,
            preview: self.preview.as_ref(),
            size: layout.bounds().size(),
            grab_offset: state.grab_offset,
        })))
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

/// Draw an entry: its outline, then its preview (the first frame at the left,
/// the waveform over the rest), then its name on top.
fn draw_entry<Renderer>(
    renderer: &mut Renderer,
    theme: &Theme,
    name: &str,
    preview: Option<&SourcePreview>,
    bounds: Rectangle,
    viewport: Rectangle,
) where
    Renderer: iced::advanced::Renderer
        + text::Renderer
        + iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>,
{
    let palette = theme.palette();
    renderer.fill_quad(
        Quad {
            bounds,
            border: Border {
                color: Color {
                    a: 0.9,
                    ..palette.primary
                },
                width: 1.0,
                radius: CORNER_RADIUS.into(),
            },
            shadow: Shadow::default(),
            snap: false,
        },
        palette.background,
    );
    if let Some(preview) = preview {
        // Keep clear of the rounded corners.
        let area = bounds.shrink([PREVIEW_INSET, PREVIEW_INSET + LABEL_PADDING]);
        if let Some(visible) = area.intersection(&viewport) {
            let mut waveform_area = area;
            if let Some(frame) = &preview.frame {
                let width = clip_preview::draw_frame(renderer, frame, area, visible);
                waveform_area.x += width + PREVIEW_INSET;
                waveform_area.width -= width + PREVIEW_INSET;
            }
            if let Some(waveform) = &preview.waveform {
                clip_preview::draw_waveform(
                    renderer,
                    waveform,
                    waveform_area,
                    visible,
                    0.0,
                    waveform.duration(),
                    Color {
                        a: 0.5,
                        ..palette.primary
                    },
                );
            }
        }
    }
    renderer.fill_text(
        Text {
            content: name.to_owned(),
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
        viewport,
    );
}

/// The copy of a [`ClipEntry`] that follows the cursor while it is dragged.
/// Drawn as an overlay so it paints above every pane, including the timeline.
struct Ghost<'a> {
    name: &'a str,
    preview: Option<&'a SourcePreview>,
    size: Size,
    /// Cursor position relative to the entry's top-left.
    grab_offset: Vector,
}

impl<Message, Renderer> overlay::Overlay<Message, Theme, Renderer> for Ghost<'_>
where
    Renderer: iced::advanced::Renderer
        + text::Renderer
        + iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>,
{
    fn layout(&mut self, _renderer: &Renderer, bounds: Size) -> layout::Node {
        layout::Node::new(bounds)
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let Some(position) = cursor.position() else {
            return;
        };
        let bounds = Rectangle::new(position - self.grab_offset, self.size);
        draw_entry(
            renderer,
            theme,
            self.name,
            self.preview,
            bounds,
            layout.bounds(),
        );
    }

    // The default `mouse::Interaction::None` keeps the cursor available to the
    // widgets underneath, so the timeline still sees the drop.
}

impl<'a, Message: 'a, Renderer> From<ClipEntry<Message>> for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer
        + text::Renderer
        + iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>
        + 'a,
    Message: Clone,
{
    fn from(widget: ClipEntry<Message>) -> Self {
        Self::new(widget)
    }
}
