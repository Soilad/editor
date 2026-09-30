use std::{collections::HashSet, time::Duration};

use avio::{ClipId, TrackKind};
use iced::{
    Border, Color, Element, Length, Point, Rectangle, Shadow, Size, Theme,
    advanced::{
        mouse,
        renderer::Quad,
        text::{self, Text},
    },
    alignment,
    keyboard::key::Named::Delete,
    widget::{button, column},
    window::position,
};

use crate::helper_funcs::{self, find_clip};
use crate::thumbnails::SourcePreview;
use crate::widgets::clip_preview;
use crate::widgets::theme::{CORNER_RADIUS, button_style};
use crate::{Message, component_for};

/// Metadata key under which the full length of a clip's source media is stored
/// (in seconds), so trims can be clamped without re-probing the file.
pub const SOURCE_DURATION_KEY: &str = "source_duration";

/// Footprint used for clips whose out-point is unset (they run to end-of-file).
const OPEN_ENDED_SECS: f64 = 5.0;

pub const MIN_WIDTH: f32 = 16.0;

const HANDLE_WIDTH: f32 = 8.0;
const LABEL_PADDING: f32 = 13.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    StartHandle,
    EndHandle,
    Body,
}

/// Read-only view over an [`avio::Clip`] for the [`Timeline`](super::timeline::Timeline)
/// widget. Placement comes from the clip itself; hit-testing and rendering live here.
#[derive(Clone, Copy)]
pub struct ClipTimeline<'a> {
    pub clip: &'a avio::Clip,
}

impl<'a> ClipTimeline<'a> {
    pub fn new(clip: &'a avio::Clip) -> Self {
        Self { clip }
    }

    pub fn name(&self) -> String {
        match &self.clip.source {
            avio::ClipSource::File(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
            avio::ClipSource::Text(_) => "Text".to_owned(),
            avio::ClipSource::Solid(_) => "Solid".to_owned(),
        }
    }

    /// Timeline start, in seconds.
    pub fn start(&self) -> f64 {
        self.clip.offset.as_secs_f64()
    }

    /// Source in-point, in seconds.
    pub fn in_point(&self) -> f64 {
        self.clip.in_point.unwrap_or(Duration::ZERO).as_secs_f64()
    }

    pub fn speed(&self) -> f64 {
        helper_funcs::clip_speed(self.clip)
    }

    /// Timeline footprint (source duration divided by speed), in seconds.
    pub fn footprint(&self) -> f64 {
        helper_funcs::footprint(self.clip)
            .map(|d| d.as_secs_f64())
            .unwrap_or(OPEN_ENDED_SECS)
    }

    /// Length of the underlying source media, in seconds, if known.
    pub fn source_duration(&self) -> Option<f64> {
        self.clip
            .metadata
            .get(SOURCE_DURATION_KEY)
            .and_then(|value| value.parse().ok())
    }

    pub fn body_bounds(bounds: Rectangle) -> Rectangle {
        let position = Point {
            x: bounds.position().x + (HANDLE_WIDTH.min(bounds.width) / 2.0),
            y: bounds.position().y,
        };
        Rectangle::new(
            position,
            Size::new(bounds.width - HANDLE_WIDTH, bounds.height),
        )
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

    /// Draw the clip in `bounds`. `preview` holds what was decoded from the
    /// clip's source file: a video track shows its first frame, an audio track
    /// its waveform over the trimmed range.
    pub fn draw_at<Renderer>(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        bounds: Rectangle,
        viewport: &Rectangle,
        active: bool,
        kind: TrackKind,
        preview: Option<&SourcePreview>,
    ) where
        Renderer: iced::advanced::Renderer
            + text::Renderer
            + iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>,
    {
        let palette = theme.palette();
        let border = if active {
            palette.primary
        } else {
            palette.text
        };

        renderer.fill_quad(
            Quad {
                bounds: Self::body_bounds(bounds),
                border: Border {
                    color: Color { a: 0.9, ..border },
                    width: 1.0,
                    radius: 0.0.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            palette.background,
        );
        if let Some(preview) = preview {
            self.draw_preview(renderer, preview, bounds, viewport, kind, border);
        }
        renderer.fill_quad(
            Quad {
                bounds: Self::left_handle_bounds(bounds),
                border: Border {
                    radius: CORNER_RADIUS,
                    ..Border::default()
                },
                shadow: Shadow::default(),
                snap: false,
            },
            border,
        );
        renderer.fill_quad(
            Quad {
                bounds: Self::right_handle_bounds(bounds),
                border: Border {
                    radius: CORNER_RADIUS,
                    ..Border::default()
                },
                shadow: Shadow::default(),
                snap: false,
            },
            border,
        );
        // Keep the name inside the clip's body and the visible area.
        let Some(label_clip) = Self::body_bounds(bounds).intersection(viewport) else {
            return;
        };
        renderer.fill_text(
            Text {
                content: self.name(),
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
            label_clip,
        );
    }
}

impl ClipTimeline<'_> {
    fn draw_preview<Renderer>(
        &self,
        renderer: &mut Renderer,
        preview: &SourcePreview,
        bounds: Rectangle,
        viewport: &Rectangle,
        kind: TrackKind,
        color: Color,
    ) where
        Renderer: iced::advanced::Renderer
            + iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>,
    {
        let body = Self::body_bounds(bounds).shrink(1.0);
        let Some(visible) = body.intersection(viewport) else {
            return;
        };
        match kind {
            TrackKind::Video => {
                if let Some(frame) = &preview.frame {
                    clip_preview::draw_frame(renderer, frame, body, visible);
                }
            }
            TrackKind::Audio => {
                if let Some(waveform) = &preview.waveform {
                    let from = self.in_point();
                    let to = from + self.footprint() * self.speed();
                    clip_preview::draw_waveform(
                        renderer,
                        waveform,
                        body,
                        visible,
                        from,
                        to,
                        Color { a: 0.5, ..color },
                    );
                }
            }
        }
    }
}

/// Items of the timeline's right-click menu, for the right-clicked `clip` (if
/// any). Split cuts the clip at `playhead`, and is disabled when the playhead
/// isn't over the clip. Split and Delete act on the whole `selection` when the
/// right-clicked clip is part of it; Split then cuts every selected clip the
/// playhead is over.
pub fn context_menu<'a>(
    timeline: &'a avio::Timeline,
    clip: Option<ClipId>,
    playhead: Duration,
    selection: &HashSet<ClipId>,
) -> Element<'a, Message> {
    let target = clip.and_then(|id| find_clip(timeline, id));
    let in_selection = target.is_some_and(|(_, clip)| selection.contains(&clip.id));
    // A split's left half keeps the clip's id, so later cuts stay valid.
    let splits: Vec<avio::Command> = if in_selection {
        selection.iter().copied().collect::<Vec<ClipId>>()
    } else {
        target
            .map(|(_, clip)| clip.id)
            .into_iter()
            .collect::<Vec<ClipId>>()
    }
    .into_iter()
    .filter_map(|id| find_clip(timeline, id))
    .filter_map(|(_, clip)| component_for(timeline, clip).split(playhead))
    .collect();
    let split = (!splits.is_empty()).then(|| Message::TimelineEdited(splits));
    let copy =
        target.map(|(track, clip)| Message::CopyClip(track, component_for(timeline, clip).copy()));
    let properties = target
        .filter(|(_, clip)| !component_for(timeline, clip).properties().is_empty())
        .map(|(_, clip)| Message::OpenProperties(clip.id));
    let delete = target.map(|(track, clip)| {
        if selection.len() > 1 && selection.contains(&clip.id) {
            Message::DeleteSelected
        } else {
            Message::RemoveClip(track, clip.id)
        }
    });
    column![
        button("Split")
            .style(button_style)
            .on_press_maybe(split)
            .width(Length::Fill),
        button("Copy")
            .style(button_style)
            .on_press_maybe(copy)
            .width(Length::Fill),
        button("Delete")
            .style(button_style)
            .on_press_maybe(delete)
            .width(Length::Fill),
        button("Properties")
            .style(button_style)
            .on_press_maybe(properties)
            .width(Length::Fill),
    ]
    .spacing(10)
    .width(100)
    .into()
}
