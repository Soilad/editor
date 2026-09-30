use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

use iced::{
    Border, Color, Element, Event, Length, Point, Rectangle, Shadow, Size, Theme,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse,
        renderer::Quad,
        text,
        widget::{Operation, Tree, tree},
    },
    alignment, keyboard,
    widget::{button, row},
};

use crate::helper_funcs;
use crate::thumbnails::SourcePreview;
use crate::widgets::theme::button_style;

use avio::{ClipId, Command, Track, TrackId, TrackKind};

use crate::widgets::clip_timeline::{self, ClipTimeline, Hit};
use crate::widgets::theme::CORNER_RADIUS;
use crate::widgets::track::{self, TrackRow};

const LABEL_WIDTH: f32 = 132.0;
const TRACK_HEIGHT: f32 = 68.0;
const TIMELINE_WIDTH: f32 = 760.0;
const CLIP_INSET: f32 = 8.0;
const PIXELS_PER_SECOND: f64 = 60.0;
const MIN_ZOOM: f32 = 0.5;
const MAX_ZOOM: f32 = 3.0;
/// Minimum scrollable length of the timeline, in seconds.
const BASE_CONTENT_SECS: f64 = 30.0;
/// Extra room after the last clip, in seconds.
const TRAILING_SECS: f64 = 6.0;
const MINOR_TICK_SECS: f64 = 0.2;
const SCROLLBAR_HEIGHT: f32 = 14.0;
/// Height of the "add track" strip under the last track.
const FOOTER_HEIGHT: f32 = 36.0;
const LABEL_PADDING: f32 = 8.0;
/// Zoom factor applied by one press of a zoom button.
const ZOOM_STEP: f32 = 1.25;
const ZOOM_BUTTON_SIZE: f32 = 24.0;
/// Width of the zoom percentage between the two zoom buttons.
const ZOOM_LABEL_WIDTH: f32 = 48.0;

/// Where a clip sits on the timeline, in seconds. Read from the [`avio::Clip`]
/// itself, or from the in-progress drag while a gesture is active.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placement {
    row: usize,
    start: f64,
    in_point: f64,
    footprint: f64,
}

#[derive(Debug)]
struct State {
    drag: Option<Drag>,
    scroll: f64,
    zoom: f32,
    modifiers: keyboard::Modifiers,
    /// Zoom button under the cursor, so hovering redraws only on change.
    zoom_hover: Option<ZoomButton>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            drag: None,
            scroll: 0.0,
            zoom: 1.0,
            modifiers: keyboard::Modifiers::default(),
            zoom_hover: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZoomButton {
    Out,
    In,
}

#[derive(Debug)]
enum Drag {
    Playhead,
    Clip {
        id: ClipId,
        mode: ClipDrag,
        origin: Placement,
        preview: Placement,
        /// Other selected clips carried along by a move, at their original
        /// placements. They follow the grabbed clip by the same offset.
        group: Vec<(ClipId, Placement)>,
    },
    /// Rubber-band selection between two screen points.
    Marquee {
        from: Point,
        to: Point,
    },
}

#[derive(Debug, Clone, Copy)]
enum ClipDrag {
    Move { grab: f64 },
    ResizeStart,
    ResizeEnd,
}

/// A compact editor-style timeline over an [`avio::Timeline`], with track labels,
/// movable clips, resize handles, a draggable playhead, horizontal scrolling, and
/// cursor-centered zooming.
///
/// The widget never mutates the timeline: finished gestures are published as
/// [`avio::Command`]s through [`on_edit`](Self::on_edit) for the host's
/// [`avio::Editor`] to apply. The playhead is owned by the host too; moving it
/// publishes [`on_seek`](Self::on_seek).
///
/// Each track's header is a [`TrackRow`]: an editable name
/// ([`on_rename`](Self::on_rename)) and an enable toggle
/// ([`on_toggle_track`](Self::on_toggle_track)). A footer under the tracks offers buttons to append new video or audio tracks
/// ([`on_add_track`](Self::on_add_track)).
pub struct Timeline<'a, Message, Renderer = iced::Renderer> {
    timeline: &'a avio::Timeline,
    playhead: f64,
    cursor_tracks: &'a [usize],
    track_names: Option<&'a HashMap<TrackId, String>>,
    previews: Option<&'a HashMap<PathBuf, SourcePreview>>,
    on_seek: Option<Box<dyn Fn(Duration) -> Message + 'a>>,
    on_edit: Option<Box<dyn Fn(Vec<Command>) -> Message + 'a>>,
    on_drop: Option<Box<dyn Fn(TrackId, Duration) -> Message + 'a>>,
    on_context: Option<Box<dyn Fn(Option<ClipId>, Duration) -> Message + 'a>>,
    on_rename: Option<Rc<dyn Fn(TrackId, String) -> Message + 'a>>,
    on_add_track: Option<Rc<dyn Fn(TrackKind) -> Message + 'a>>,
    on_toggle_track: Option<Rc<dyn Fn(TrackId, bool) -> Message + 'a>>,
    selected: Option<&'a HashSet<ClipId>>,
    on_select: Option<Box<dyn Fn(HashSet<ClipId>) -> Message + 'a>>,
    /// One [`TrackRow`] per row, then the footer. Built when the widget is turned
    /// into an [`Element`], once every callback is known.
    children: Vec<Element<'a, Message, Theme, Renderer>>,
}

impl<'a, Message, Renderer> Timeline<'a, Message, Renderer> {
    pub fn new(timeline: &'a avio::Timeline, playhead: Duration) -> Self {
        Self {
            timeline,
            playhead: playhead.as_secs_f64(),
            cursor_tracks: &[],
            track_names: None,
            previews: None,
            on_seek: None,
            on_edit: None,
            on_drop: None,
            on_context: None,
            on_rename: None,
            on_add_track: None,
            on_toggle_track: None,
            selected: None,
            on_select: None,
            children: Vec::new(),
        }
    }

    /// Rows (in display order) that show a track cursor at the playhead.
    pub fn cursor_tracks(mut self, rows: &'a [usize]) -> Self {
        self.cursor_tracks = rows;
        self
    }

    /// Names to show for tracks, overriding [`Track::name`]. avio has no command
    /// to rename a track, so the host keeps edited names alongside the timeline.
    pub fn track_names(mut self, names: &'a HashMap<TrackId, String>) -> Self {
        self.track_names = Some(names);
        self
    }

    /// First frames and waveforms of source files, keyed by path, drawn on the
    /// clips cut from them.
    pub fn previews(mut self, previews: &'a HashMap<PathBuf, SourcePreview>) -> Self {
        self.previews = Some(previews);
        self
    }

    /// Called on every keystroke in a track's label with the label's new text.
    /// Without it the labels are read-only.
    pub fn on_rename(mut self, f: impl Fn(TrackId, String) -> Message + 'a) -> Self {
        self.on_rename = Some(Rc::new(f));
        self
    }

    /// Called when one of the "add track" buttons is pressed.
    pub fn on_add_track(mut self, f: impl Fn(TrackKind) -> Message + 'a) -> Self {
        self.on_add_track = Some(Rc::new(f));
        self
    }

    /// Called with a track and its requested enabled state when the track's
    /// toggle is pressed. Without it the toggles are inert.
    pub fn on_toggle_track(mut self, f: impl Fn(TrackId, bool) -> Message + 'a) -> Self {
        self.on_toggle_track = Some(Rc::new(f));
        self
    }

    /// Clips drawn as selected. Moving one of them moves them all.
    pub fn selected(mut self, selected: &'a HashSet<ClipId>) -> Self {
        self.selected = Some(selected);
        self
    }

    /// Called with the new selection whenever the user changes it: a click on a
    /// clip selects it alone, Shift/Ctrl-click toggles it, Shift/Ctrl-dragging
    /// over empty lanes adds every clip the box touches, and a plain click on
    /// empty lanes clears it.
    pub fn on_select(mut self, f: impl Fn(HashSet<ClipId>) -> Message + 'a) -> Self {
        self.on_select = Some(Box::new(f));
        self
    }

    /// Called with the new playhead position whenever the user moves it.
    pub fn on_seek(mut self, f: impl Fn(Duration) -> Message + 'a) -> Self {
        self.on_seek = Some(Box::new(f));
        self
    }

    /// Called with the commands that finish a move or trim gesture.
    pub fn on_edit(mut self, f: impl Fn(Vec<Command>) -> Message + 'a) -> Self {
        self.on_edit = Some(Box::new(f));
        self
    }

    /// Called when an external clip is released over a track. Only set this while
    /// a clip is actually being dragged in from elsewhere.
    pub fn on_drop(mut self, f: impl Fn(TrackId, Duration) -> Message + 'a) -> Self {
        self.on_drop = Some(Box::new(f));
        self
    }

    /// Called on a right-click in the lanes with the clip under the cursor (if
    /// any) and the snapped timeline position of the click.
    pub fn on_context(mut self, f: impl Fn(Option<ClipId>, Duration) -> Message + 'a) -> Self {
        self.on_context = Some(Box::new(f));
        self
    }

    /// Video tracks followed by audio tracks, one row each. Video runs from the
    /// last track down to track 0, so rows stack the way the preview draws them:
    /// the top row in front.
    fn rows(&self) -> impl Iterator<Item = &'a Track> + 'a {
        self.timeline
            .video_tracks()
            .iter()
            .rev()
            .chain(self.timeline.audio_tracks())
    }

    fn row_count(&self) -> usize {
        self.timeline.video_tracks().len() + self.timeline.audio_tracks().len()
    }

    fn track_id(&self, row: usize) -> Option<TrackId> {
        self.rows().nth(row).map(|track| track.id)
    }

    /// Rows holding tracks of the same kind as `row`, so video clips stay on
    /// video tracks and audio clips on audio tracks.
    fn rows_of_kind(&self, row: usize) -> std::ops::Range<usize> {
        let video = self.timeline.video_tracks().len();
        if row < video {
            0..video
        } else {
            video..self.row_count()
        }
    }

    /// The name the user gave the track, which may be empty.
    fn track_name(&self, track: &'a Track) -> &'a str {
        self.track_names
            .and_then(|names| names.get(&track.id))
            .map_or(track.name.as_str(), String::as_str)
    }

    /// Fallback label for an unnamed track, shown as the input's placeholder.
    fn default_label(&self, row: usize) -> String {
        let video = self.timeline.video_tracks().len();
        if row < video {
            format!("Video {}", video - row)
        } else {
            format!("Audio {}", row - video + 1)
        }
    }

    fn row_kind(&self, row: usize) -> TrackKind {
        if row < self.timeline.video_tracks().len() {
            TrackKind::Video
        } else {
            TrackKind::Audio
        }
    }

    fn preview_of(&self, clip: &avio::Clip) -> Option<&'a SourcePreview> {
        self.previews?.get(clip.source_path()?)
    }

    fn frame_secs(&self) -> f64 {
        1.0 / self.timeline.frame_rate().max(1.0)
    }

    fn snap(&self, secs: f64) -> f64 {
        let frame = self.frame_secs();
        (secs / frame).round() * frame
    }

    fn placement_of(&self, row: usize, clip: &avio::Clip) -> Placement {
        let view = ClipTimeline::new(clip);
        Placement {
            row,
            start: view.start(),
            in_point: view.in_point(),
            footprint: view.footprint(),
        }
    }

    /// Placement to draw `clip` at, taking an in-progress drag into account.
    fn displayed_placement(&self, state: &State, row: usize, clip: &avio::Clip) -> Placement {
        match &state.drag {
            Some(Drag::Clip { id, preview, .. }) if *id == clip.id => *preview,
            Some(Drag::Clip {
                origin,
                preview,
                group,
                ..
            }) => group
                .iter()
                .find(|(id, _)| *id == clip.id)
                .map(|(_, placement)| follow(*placement, *origin, *preview))
                .unwrap_or_else(|| self.placement_of(row, clip)),
            _ => self.placement_of(row, clip),
        }
    }

    fn is_selected(&self, id: ClipId) -> bool {
        self.selected.is_some_and(|selected| selected.contains(&id))
    }

    fn publish_selection(&self, selection: HashSet<ClipId>, shell: &mut Shell<'_, Message>) {
        if let Some(on_select) = &self.on_select
            && self.selected != Some(&selection)
        {
            shell.publish(on_select(selection));
        }
    }

    /// Every selected clip except `except`, with its current placement.
    fn selected_placements(&self, except: ClipId) -> Vec<(ClipId, Placement)> {
        self.rows()
            .enumerate()
            .flat_map(|(row, track)| track.clips.iter().map(move |clip| (row, clip)))
            .filter(|(_, clip)| clip.id != except && self.is_selected(clip.id))
            .map(|(row, clip)| (clip.id, self.placement_of(row, clip)))
            .collect()
    }

    /// Clips whose bounds touch the screen rectangle `area`.
    fn clips_in(&self, layout: Layout<'_>, state: &State, area: Rectangle) -> Vec<ClipId> {
        self.rows()
            .enumerate()
            .flat_map(|(row, track)| track.clips.iter().map(move |clip| (row, clip)))
            .filter(|(row, clip)| {
                let placement = self.placement_of(*row, clip);
                self.clip_bounds(layout, state, &placement)
                    .intersects(&area)
            })
            .map(|(_, clip)| clip.id)
            .collect()
    }

    /// Limit a group move so no clip starts before zero, and drop the row change
    /// if it would put any clip on a track of the other kind (or off the end).
    fn constrain_group(
        &self,
        origin: Placement,
        preview: Placement,
        group: &[(ClipId, Placement)],
    ) -> Placement {
        let earliest = group
            .iter()
            .map(|(_, placement)| placement.start)
            .fold(origin.start, f64::min);
        let start = origin.start + (preview.start - origin.start).max(-earliest);
        let rows_fit = group.iter().all(|(_, placement)| {
            let row = follow(*placement, origin, preview).row;
            self.rows_of_kind(placement.row).contains(&row)
                && preview.row as isize - origin.row as isize + placement.row as isize >= 0
        });
        Placement {
            start,
            row: if rows_fit { preview.row } else { origin.row },
            ..preview
        }
    }

    fn lane_width(&self) -> f32 {
        TIMELINE_WIDTH - LABEL_WIDTH
    }

    fn tracks_height(&self) -> f32 {
        self.row_count().max(1) as f32 * TRACK_HEIGHT
    }

    fn visible_secs(&self, state: &State) -> f64 {
        f64::from(self.lane_width() / state.zoom) / PIXELS_PER_SECOND
    }

    fn content_secs(&self, state: &State) -> f64 {
        self.rows()
            .enumerate()
            .flat_map(|(row, track)| track.clips.iter().map(move |clip| (row, clip)))
            .map(|(row, clip)| {
                let placement = self.displayed_placement(state, row, clip);
                placement.start + placement.footprint + TRAILING_SECS
            })
            .fold(BASE_CONTENT_SECS, f64::max)
            .max(self.playhead)
            .max(self.visible_secs(state))
    }

    fn max_scroll(&self, state: &State) -> f64 {
        (self.content_secs(state) - self.visible_secs(state)).max(0.0)
    }

    fn clamp_view(&self, state: &mut State) {
        state.zoom = state.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        state.scroll = state.scroll.clamp(0.0, self.max_scroll(state));
    }

    /// Publish a seek to the snapped time under screen position `x`.
    fn seek_to(&self, layout: Layout<'_>, state: &State, x: f32, shell: &mut Shell<'_, Message>) {
        if let Some(on_seek) = &self.on_seek {
            let secs = self
                .snap(self.screen_x_to_time(layout, state, x))
                .clamp(0.0, self.content_secs(state));
            shell.publish(on_seek(Duration::from_secs_f64(secs)));
        }
    }

    fn lane_bounds(&self, layout: Layout<'_>) -> Rectangle {
        let bounds = layout.bounds();
        Rectangle::new(
            Point::new(bounds.x + LABEL_WIDTH, bounds.y),
            Size::new(self.lane_width(), self.tracks_height()),
        )
    }

    fn row_lane_bounds(&self, layout: Layout<'_>, row: usize) -> Rectangle {
        let bounds = layout.bounds();
        Rectangle::new(
            Point::new(bounds.x + LABEL_WIDTH, bounds.y + row as f32 * TRACK_HEIGHT),
            Size::new(self.lane_width(), TRACK_HEIGHT),
        )
    }

    fn time_to_screen_x(&self, layout: Layout<'_>, state: &State, secs: f64) -> f32 {
        layout.bounds().x
            + LABEL_WIDTH
            + ((secs - state.scroll) * PIXELS_PER_SECOND) as f32 * state.zoom
    }

    fn screen_x_to_time(&self, layout: Layout<'_>, state: &State, x: f32) -> f64 {
        state.scroll
            + f64::from((x - layout.bounds().x - LABEL_WIDTH) / state.zoom) / PIXELS_PER_SECOND
    }

    fn row_at(&self, layout: Layout<'_>, y: f32) -> usize {
        (((y - layout.bounds().y) / TRACK_HEIGHT).floor() as isize)
            .clamp(0, self.row_count().max(1) as isize - 1) as usize
    }

    fn clip_bounds(&self, layout: Layout<'_>, state: &State, placement: &Placement) -> Rectangle {
        Rectangle::new(
            Point::new(
                self.time_to_screen_x(layout, state, placement.start),
                layout.bounds().y + placement.row as f32 * TRACK_HEIGHT + CLIP_INSET,
            ),
            Size::new(
                ((placement.footprint * PIXELS_PER_SECOND) as f32 * state.zoom)
                    .max(clip_timeline::MIN_WIDTH),
                TRACK_HEIGHT - CLIP_INSET * 2.0,
            ),
        )
    }

    /// Set the zoom, keeping the time under screen position `x` in place.
    fn zoom_around(&self, layout: Layout<'_>, state: &mut State, zoom: f32, x: f32) {
        let anchor = self.screen_x_to_time(layout, state, x);
        state.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let screen_offset =
            f64::from((x - layout.bounds().x - LABEL_WIDTH) / state.zoom) / PIXELS_PER_SECOND;
        state.scroll = anchor - screen_offset;
        self.clamp_view(state);
    }

    /// Bounds of the zoom out button, the zoom label, and the zoom in button,
    /// right-aligned in the footer under the lanes.
    fn zoom_controls_bounds(&self, layout: Layout<'_>) -> [Rectangle; 3] {
        let bounds = layout.bounds();
        let y = bounds.y + self.tracks_height() + (FOOTER_HEIGHT - ZOOM_BUTTON_SIZE) / 2.0;
        let right = bounds.x + TIMELINE_WIDTH - LABEL_PADDING;
        let button = Size::new(ZOOM_BUTTON_SIZE, ZOOM_BUTTON_SIZE);
        let zoom_in = Rectangle::new(Point::new(right - ZOOM_BUTTON_SIZE, y), button);
        let label = Rectangle::new(
            Point::new(zoom_in.x - ZOOM_LABEL_WIDTH, y),
            Size::new(ZOOM_LABEL_WIDTH, ZOOM_BUTTON_SIZE),
        );
        let zoom_out = Rectangle::new(Point::new(label.x - ZOOM_BUTTON_SIZE, y), button);
        [zoom_out, label, zoom_in]
    }

    fn zoom_button_at(&self, layout: Layout<'_>, position: Point) -> Option<ZoomButton> {
        let [zoom_out, _, zoom_in] = self.zoom_controls_bounds(layout);
        if zoom_out.contains(position) {
            Some(ZoomButton::Out)
        } else if zoom_in.contains(position) {
            Some(ZoomButton::In)
        } else {
            None
        }
    }

    fn playhead_hit_bounds(&self, layout: Layout<'_>, state: &State) -> Rectangle {
        let x = self.time_to_screen_x(layout, state, self.playhead);
        Rectangle::new(
            Point::new(x - 5.0, layout.bounds().y),
            Size::new(10.0, self.tracks_height()),
        )
    }

    /// Topmost clip under `position`, with the part of it that was hit.
    fn clip_at(
        &self,
        layout: Layout<'_>,
        state: &State,
        position: Point,
    ) -> Option<(&'a avio::Clip, Placement, Hit)> {
        let rows: Vec<&'a Track> = self.rows().collect();
        rows.into_iter().enumerate().rev().find_map(|(row, track)| {
            track.clips.iter().rev().find_map(|clip| {
                let placement = self.placement_of(row, clip);
                let bounds = self.clip_bounds(layout, state, &placement);
                ClipTimeline::hit_test(bounds, position).map(|hit| (clip, placement, hit))
            })
        })
    }

    fn find_clip(&self, id: ClipId) -> Option<&'a avio::Clip> {
        helper_funcs::find_clip(self.timeline, id).map(|(_, clip)| clip)
    }

    /// Update the preview of a clip drag for the cursor at `position`.
    fn drag_clip(
        &self,
        layout: Layout<'_>,
        state: &State,
        clip: &avio::Clip,
        mode: ClipDrag,
        origin: Placement,
        position: Point,
    ) -> Placement {
        let view = ClipTimeline::new(clip);
        let speed = view.speed();
        let min_footprint = self.frame_secs();
        let cursor = self.screen_x_to_time(layout, state, position.x);

        match mode {
            ClipDrag::Move { grab } => {
                let kind = self.rows_of_kind(origin.row);
                let row = self
                    .row_at(layout, position.y)
                    .clamp(kind.start, kind.end.saturating_sub(1));
                Placement {
                    row,
                    start: self.snap(cursor - grab).max(0.0),
                    ..origin
                }
            }
            ClipDrag::ResizeStart => {
                let end = origin.start + origin.footprint;
                // Can't reveal media before the source's first frame.
                let earliest = (origin.start - origin.in_point / speed).max(0.0);
                let start = self
                    .snap(cursor)
                    .clamp(earliest, (end - min_footprint).max(earliest));
                Placement {
                    start,
                    in_point: (origin.in_point + (start - origin.start) * speed).max(0.0),
                    footprint: end - start,
                    ..origin
                }
            }
            ClipDrag::ResizeEnd => {
                // Can't extend past the source's last frame, when that is known.
                let latest = view
                    .source_duration()
                    .map(|source| origin.start + (source - origin.in_point) / speed)
                    .unwrap_or(f64::INFINITY);
                let min_end = origin.start + min_footprint;
                let end = self.snap(cursor).clamp(min_end, latest.max(min_end));
                Placement {
                    footprint: end - origin.start,
                    ..origin
                }
            }
        }
    }

    /// Commands that turn `origin` into `preview` for `clip`.
    fn commands_for(
        &self,
        clip: &avio::Clip,
        mode: ClipDrag,
        origin: Placement,
        preview: Placement,
    ) -> Vec<Command> {
        let id = clip.id;
        let offset = Duration::from_secs_f64(preview.start.max(0.0));
        let mut commands = Vec::new();

        match mode {
            ClipDrag::Move { .. } => {
                if preview.row != origin.row {
                    if let Some(to) = self.track_id(preview.row) {
                        commands.push(Command::MoveClipToTrack {
                            clip: id,
                            to,
                            offset,
                        });
                    }
                } else if preview.start != origin.start {
                    commands.push(Command::MoveClip { clip: id, offset });
                }
            }
            ClipDrag::ResizeStart | ClipDrag::ResizeEnd => {
                if preview.in_point != origin.in_point || preview.footprint != origin.footprint {
                    let speed = ClipTimeline::new(clip).speed();
                    commands.push(Command::TrimClip {
                        clip: id,
                        in_point: Some(Duration::from_secs_f64(preview.in_point)),
                        out_point: Some(Duration::from_secs_f64(
                            preview.in_point + preview.footprint * speed,
                        )),
                    });
                }
                if preview.start != origin.start {
                    commands.push(Command::MoveClip { clip: id, offset });
                }
            }
        }

        commands
    }
}

impl<'a, Message, Renderer> Timeline<'a, Message, Renderer>
where
    Message: Clone + 'a,
    Renderer: iced::advanced::Renderer + text::Renderer + 'a,
{
    fn build_children(&mut self) {
        let headers = self.rows().enumerate().map(|(row, track)| {
            TrackRow::new(
                track,
                self.track_name(track),
                &self.default_label(row),
                self.on_rename.clone(),
                self.on_toggle_track.clone(),
            )
            .into()
        });

        let add = |label: &'a str, kind: TrackKind| {
            button(iced::widget::text(label).size(12).center())
                .style(button_style)
                .padding([4, 0])
                .width(Length::Fill)
                .on_press_maybe(self.on_add_track.as_ref().map(|f| f(kind)))
        };
        let footer = row![
            add("+ Video", TrackKind::Video),
            add("+ Audio", TrackKind::Audio)
        ]
        .spacing(4)
        .into();

        self.children = headers.chain(std::iter::once(footer)).collect();
    }
}

/// Where a group member moved along with a clip dragged from `origin` to
/// `preview` ends up.
fn follow(placement: Placement, origin: Placement, preview: Placement) -> Placement {
    Placement {
        row: (placement.row as isize + preview.row as isize - origin.row as isize).max(0) as usize,
        start: placement.start + preview.start - origin.start,
        ..placement
    }
}

impl<'a, Message, Renderer> Widget<Message, Theme, Renderer> for Timeline<'a, Message, Renderer>
where
    Renderer: iced::advanced::Renderer
        + text::Renderer
        + iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>,
{
    fn size(&self) -> iced::Size<Length> {
        iced::Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
        let state = tree.state.downcast_mut::<State>();
        // The dragged clip may have vanished (e.g. an undo mid-gesture).
        if let Some(Drag::Clip { id, .. }) = &state.drag
            && self.find_clip(*id).is_none()
        {
            state.drag = None;
        }
        self.clamp_view(state);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        let tracks_height = self.tracks_height();
        let footer_row = self.children.len().saturating_sub(1);

        let children = self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .enumerate()
            .map(|(row, (child, state))| {
                if row == footer_row {
                    let node = child.as_widget_mut().layout(
                        state,
                        renderer,
                        &layout::Limits::new(
                            Size::ZERO,
                            Size::new(LABEL_WIDTH - LABEL_PADDING * 2.0, FOOTER_HEIGHT),
                        ),
                    );
                    // Center the footer vertically under the tracks.
                    let y = tracks_height + (FOOTER_HEIGHT - node.size().height) / 2.0;
                    node.move_to(Point::new(LABEL_PADDING, y))
                } else {
                    // A track row fills its whole slot in the label column.
                    let slot = Size::new(LABEL_WIDTH, TRACK_HEIGHT);
                    child
                        .as_widget_mut()
                        .layout(state, renderer, &layout::Limits::new(slot, slot))
                        .move_to(Point::new(0.0, row as f32 * TRACK_HEIGHT))
                }
            })
            .collect();

        layout::Node::with_children(
            Size::new(
                TIMELINE_WIDTH,
                tracks_height + FOOTER_HEIGHT + SCROLLBAR_HEIGHT,
            ),
            children,
        )
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
            self.children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
                .for_each(|((child, state), layout)| {
                    child
                        .as_widget_mut()
                        .operate(state, layout, renderer, operation);
                });
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
        let state = tree.state.downcast_ref::<State>();
        let lane_bounds = self.lane_bounds(layout);
        let tracks_height = self.tracks_height();

        renderer.fill_quad(
            Quad {
                bounds,
                border: Border::default(),
                shadow: Shadow::default(),
                snap: false,
            },
            palette.background,
        );

        // Draws tracks
        for row in 0..self.row_count().max(1) {
            track::draw_lane(renderer, theme, self.row_lane_bounds(layout, row));
        }

        let first_tick = (state.scroll / MINOR_TICK_SECS).floor() as i64 - 1;
        let last_tick =
            ((state.scroll + self.visible_secs(state)) / MINOR_TICK_SECS).ceil() as i64 + 1;
        let ticks_per_second = (1.0 / MINOR_TICK_SECS).round() as i64;
        for tick in first_tick..=last_tick {
            let x = self.time_to_screen_x(layout, state, tick as f64 * MINOR_TICK_SECS);
            if x < lane_bounds.x || x > lane_bounds.x + lane_bounds.width {
                continue;
            }
            let major = tick % ticks_per_second == 0;
            let height = if major { 12.0 } else { 6.0 };
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle::new(Point::new(x, bounds.y), Size::new(1.0, height)),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    snap: false,
                },
                palette.primary,
            );
        }

        let dragged = match &state.drag {
            Some(Drag::Clip { id, .. }) => Some(*id),
            _ => None,
        };
        for (row, track) in self.rows().enumerate() {
            for clip in &track.clips {
                let placement = self.displayed_placement(state, row, clip);
                let clip_bounds = self.clip_bounds(layout, state, &placement);
                if clip_bounds.x > lane_bounds.x + lane_bounds.width
                    || clip_bounds.x + clip_bounds.width < lane_bounds.x
                {
                    continue;
                }
                // Text draws above every quad in the layer, so the label boxes
                // drawn last can't cover a clip's name; clip it to the lane instead.
                let Some(visible) = lane_bounds.intersection(viewport) else {
                    continue;
                };
                ClipTimeline::new(clip).draw_at(
                    renderer,
                    theme,
                    clip_bounds,
                    &visible,
                    dragged == Some(clip.id) || self.is_selected(clip.id),
                    self.row_kind(placement.row),
                    self.preview_of(clip),
                );
            }
        }

        for (row, track) in self.rows().enumerate() {
            if !track.enabled {
                track::draw_disabled(renderer, theme, self.row_lane_bounds(layout, row));
            }
        }

        if let Some(Drag::Marquee { from, to }) = &state.drag {
            let area = Rectangle::new(
                Point::new(from.x.min(to.x), from.y.min(to.y)),
                Size::new((from.x - to.x).abs(), (from.y - to.y).abs()),
            );
            if let Some(area) = area.intersection(&lane_bounds) {
                renderer.fill_quad(
                    Quad {
                        bounds: area,
                        border: Border {
                            color: palette.primary,
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        shadow: Shadow::default(),
                        snap: false,
                    },
                    Color {
                        a: 0.15,
                        ..palette.primary
                    },
                );
            }
        }

        let playhead_x = self.time_to_screen_x(layout, state, self.playhead);
        if playhead_x >= lane_bounds.x && playhead_x <= lane_bounds.x + lane_bounds.width {
            for &row in self
                .cursor_tracks
                .iter()
                .filter(|&&row| row < self.row_count())
            {
                let lane = self.row_lane_bounds(layout, row);
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle::new(
                            Point::new(playhead_x - 1.0, lane.y),
                            Size::new(2.0, TRACK_HEIGHT),
                        ),
                        border: Border::default(),
                        shadow: Shadow::default(),
                        snap: false,
                    },
                    Color {
                        a: 0.8,
                        ..palette.text
                    },
                );
            }
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle::new(
                        Point::new(playhead_x - 1.0, bounds.y),
                        Size::new(2.0, tracks_height),
                    ),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    snap: false,
                },
                Color {
                    a: 0.5,
                    ..palette.primary
                },
            );
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle::new(
                        Point::new(playhead_x - 6.0, bounds.y),
                        Size::new(12.0, 8.0),
                    ),
                    border: Border {
                        color: Color {
                            a: 0.5,
                            ..palette.primary
                        },
                        width: 1.0,
                        radius: CORNER_RADIUS,
                    },
                    shadow: Shadow::default(),
                    snap: false,
                },
                Color {
                    a: 0.5,
                    ..palette.primary
                },
            );
        }

        let [zoom_out, zoom_label, zoom_in] = self.zoom_controls_bounds(layout);
        let hovered = state.zoom_hover;
        let controls = [
            (zoom_out, "-", Some(ZoomButton::Out), state.zoom > MIN_ZOOM),
            (zoom_label, "", None, true),
            (zoom_in, "+", Some(ZoomButton::In), state.zoom < MAX_ZOOM),
        ];
        for (bounds, label, button, enabled) in controls {
            let label = match button {
                Some(_) => label.to_owned(),
                None => format!("{:.0}%", state.zoom * 100.0),
            };
            let color = Color {
                a: if enabled { 1.0 } else { 0.35 },
                ..palette.text
            };
            if button.is_some() {
                let hover = enabled && button == hovered;
                renderer.fill_quad(
                    Quad {
                        bounds,
                        border: Border {
                            color: Color { a: 0.6, ..color },
                            width: 1.0,
                            radius: CORNER_RADIUS,
                        },
                        shadow: Shadow::default(),
                        snap: false,
                    },
                    if hover {
                        Color {
                            a: 0.25,
                            ..palette.primary
                        }
                    } else {
                        palette.background
                    },
                );
            }
            renderer.fill_text(
                text::Text {
                    content: label,
                    size: 13.0.into(),
                    line_height: text::LineHeight::default(),
                    font: renderer.default_font(),
                    bounds: bounds.size(),
                    align_x: text::Alignment::Center,
                    align_y: alignment::Vertical::Center,
                    shaping: text::Shaping::Basic,
                    wrapping: text::Wrapping::None,
                },
                bounds.center(),
                color,
                *viewport,
            );
        }

        for ((child, state), layout) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            child
                .as_widget()
                .draw(state, renderer, theme, style, layout, cursor, viewport);
        }
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
        for ((child, state), layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                state, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }

        let state = tree.state.downcast_mut::<State>();
        // Handled before the capture check: a wrapping context menu captures the
        // right-click it opens on, but still needs to know what was clicked.
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event
            && state.drag.is_none()
            && let Some(on_context) = &self.on_context
            && let Some(position) = cursor.position()
            && self.lane_bounds(layout).contains(position)
        {
            let clip = self
                .clip_at(layout, state, position)
                .map(|(clip, _, _)| clip.id);
            let at = self
                .snap(self.screen_x_to_time(layout, state, position.x))
                .max(0.0);
            shell.publish(on_context(clip, Duration::from_secs_f64(at)));
        }
        // Let an in-progress gesture finish even if a child claimed the release.
        if shell.is_event_captured() && state.drag.is_none() {
            return;
        }
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(position) = cursor.position() else {
                    return;
                };

                if let Some(button) = self.zoom_button_at(layout, position) {
                    let zoom = match button {
                        ZoomButton::In => state.zoom * ZOOM_STEP,
                        ZoomButton::Out => state.zoom / ZOOM_STEP,
                    };
                    // Keep the playhead in place, or the lane's center when the
                    // playhead is off screen.
                    let lane = self.lane_bounds(layout);
                    let playhead = self.time_to_screen_x(layout, state, self.playhead);
                    let anchor = if (lane.x..=lane.x + lane.width).contains(&playhead) {
                        playhead
                    } else {
                        lane.center_x()
                    };
                    self.zoom_around(layout, state, zoom, anchor);
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }

                if self.playhead_hit_bounds(layout, state).contains(position) {
                    state.drag = Some(Drag::Playhead);
                    shell.request_redraw();
                    return;
                }

                let toggle = state.modifiers.shift() || state.modifiers.command();

                if let Some((clip, placement, hit)) = self.clip_at(layout, state, position) {
                    if toggle {
                        let mut selection = self.selected.cloned().unwrap_or_default();
                        if !selection.remove(&clip.id) {
                            selection.insert(clip.id);
                        }
                        self.publish_selection(selection, shell);
                        shell.request_redraw();
                        return;
                    }
                    // Grabbing a selected clip's body carries the rest of the
                    // selection along; anything else selects just this clip.
                    let group = if hit == Hit::Body && self.is_selected(clip.id) {
                        self.selected_placements(clip.id)
                    } else {
                        self.publish_selection(HashSet::from([clip.id]), shell);
                        Vec::new()
                    };
                    let mode = match hit {
                        Hit::StartHandle => ClipDrag::ResizeStart,
                        Hit::EndHandle => ClipDrag::ResizeEnd,
                        Hit::Body => ClipDrag::Move {
                            grab: self.screen_x_to_time(layout, state, position.x)
                                - placement.start,
                        },
                    };
                    state.drag = Some(Drag::Clip {
                        id: clip.id,
                        mode,
                        origin: placement,
                        preview: placement,
                        group,
                    });
                    shell.request_redraw();
                    return;
                }

                if self.lane_bounds(layout).contains(position) {
                    if toggle {
                        state.drag = Some(Drag::Marquee {
                            from: position,
                            to: position,
                        });
                    } else {
                        self.publish_selection(HashSet::new(), shell);
                        state.drag = Some(Drag::Playhead);
                        self.seek_to(layout, state, position.x, shell);
                    }
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let Some(position) = cursor.position() else {
                    return;
                };
                let hover = self.zoom_button_at(layout, position);
                if hover != state.zoom_hover {
                    state.zoom_hover = hover;
                    shell.request_redraw();
                }
                match state.drag.take() {
                    None => return,
                    Some(Drag::Playhead) => {
                        self.seek_to(layout, state, position.x, shell);
                        state.drag = Some(Drag::Playhead);
                    }
                    Some(Drag::Clip {
                        id,
                        mode,
                        origin,
                        group,
                        ..
                    }) => {
                        let Some(clip) = self.find_clip(id) else {
                            return;
                        };
                        let mut preview =
                            self.drag_clip(layout, state, clip, mode, origin, position);
                        if !group.is_empty() {
                            preview = self.constrain_group(origin, preview, &group);
                        }
                        state.drag = Some(Drag::Clip {
                            id,
                            mode,
                            origin,
                            preview,
                            group,
                        });
                    }
                    Some(Drag::Marquee { from, .. }) => {
                        state.drag = Some(Drag::Marquee { from, to: position });
                    }
                }
                self.clamp_view(state);
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                match state.drag.take() {
                    Some(Drag::Clip {
                        id,
                        mode,
                        origin,
                        preview,
                        group,
                    }) => {
                        // A click (no move) on one clip of a group selects it alone.
                        if preview == origin && !group.is_empty() {
                            self.publish_selection(HashSet::from([id]), shell);
                        }
                        if let Some(on_edit) = &self.on_edit {
                            let mut moves: Vec<(&avio::Clip, Placement, Placement)> =
                                std::iter::once((id, origin, preview))
                                    .chain(group.iter().map(|&(other, placement)| {
                                        (other, placement, follow(placement, origin, preview))
                                    }))
                                    .filter_map(|(id, from, to)| {
                                        Some((self.find_clip(id)?, from, to))
                                    })
                                    .collect();
                            // Every move overwrites what it lands on, so move the
                            // clips leading in the direction of travel first, before
                            // a group member lands on one still waiting to move.
                            let down = preview.row > origin.row;
                            let right = preview.start > origin.start;
                            moves.sort_by(|(_, a, _), (_, b, _)| {
                                let rows = if down {
                                    b.row.cmp(&a.row)
                                } else {
                                    a.row.cmp(&b.row)
                                };
                                let starts = if right {
                                    b.start.total_cmp(&a.start)
                                } else {
                                    a.start.total_cmp(&b.start)
                                };
                                rows.then(starts)
                            });
                            let commands: Vec<Command> = moves
                                .into_iter()
                                .flat_map(|(clip, from, to)| {
                                    self.commands_for(clip, mode, from, to)
                                })
                                .collect();
                            if !commands.is_empty() {
                                shell.publish(on_edit(commands));
                            }
                        }
                        shell.request_redraw();
                    }
                    Some(Drag::Marquee { from, to }) => {
                        let area = Rectangle::new(
                            Point::new(from.x.min(to.x), from.y.min(to.y)),
                            Size::new((from.x - to.x).abs(), (from.y - to.y).abs()),
                        );
                        let mut selection = self.selected.cloned().unwrap_or_default();
                        selection.extend(self.clips_in(layout, state, area));
                        self.publish_selection(selection, shell);
                        shell.request_redraw();
                    }
                    Some(Drag::Playhead) => shell.request_redraw(),
                    None => {}
                }

                if let Some(on_drop) = &self.on_drop
                    && let Some(position) = cursor.position()
                    && self.lane_bounds(layout).contains(position)
                    && let Some(track) = self.track_id(self.row_at(layout, position.y))
                {
                    let start = self
                        .snap(self.screen_x_to_time(layout, state, position.x))
                        .max(0.0);
                    shell.publish(on_drop(track, Duration::from_secs_f64(start)));
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let Some(position) = cursor.position() else {
                    return;
                };
                if !self.lane_bounds(layout).contains(position) {
                    return;
                }

                let (x, y) = match delta {
                    mouse::ScrollDelta::Lines { x, y } => (*x * 48.0, *y * 48.0),
                    mouse::ScrollDelta::Pixels { x, y } => (*x, *y),
                };

                if state.modifiers.command() {
                    self.zoom_around(layout, state, state.zoom * (1.0 + y * 0.01), position.x);
                } else {
                    state.scroll += f64::from((x - y) / state.zoom) / PIXELS_PER_SECOND;
                    self.clamp_view(state);
                }

                // Keep the enclosing scrollable from also scrolling.
                shell.capture_event();
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
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.drag.is_none()
            && let Some(interaction) = self
                .children
                .iter()
                .zip(&tree.children)
                .zip(layout.children())
                .map(|((child, state), layout)| {
                    child
                        .as_widget()
                        .mouse_interaction(state, layout, cursor, viewport, renderer)
                })
                .find(|interaction| *interaction != mouse::Interaction::None)
        {
            return interaction;
        }

        match &state.drag {
            Some(Drag::Clip {
                mode: ClipDrag::ResizeStart | ClipDrag::ResizeEnd,
                ..
            }) => return mouse::Interaction::ResizingHorizontally,
            Some(Drag::Clip { .. } | Drag::Playhead) => return mouse::Interaction::Grabbing,
            Some(Drag::Marquee { .. }) => return mouse::Interaction::Crosshair,
            None => {}
        }

        let Some(position) = cursor.position() else {
            return mouse::Interaction::None;
        };

        if self.zoom_button_at(layout, position).is_some() {
            return mouse::Interaction::Pointer;
        }

        if self.playhead_hit_bounds(layout, state).contains(position) {
            return mouse::Interaction::Grab;
        }

        self.clip_at(layout, state, position)
            .map(|(_, placement, _)| {
                ClipTimeline::mouse_interaction_at(
                    self.clip_bounds(layout, state, &placement),
                    cursor,
                )
            })
            .unwrap_or(mouse::Interaction::None)
    }
}

impl<'a, Message, Renderer> From<Timeline<'a, Message, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Renderer: iced::advanced::Renderer
        + text::Renderer
        + iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>
        + 'a,
{
    fn from(mut widget: Timeline<'a, Message, Renderer>) -> Self {
        widget.build_children();
        Self::new(widget)
    }
}
