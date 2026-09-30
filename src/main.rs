mod clip_components;
mod helper_funcs;
mod overwrite;
mod preview;
mod project;
mod thumbnails;
mod widgets;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use iced::{
    Color, Element, Length, Point, Renderer, Theme,
    advanced::image,
    keyboard,
    widget::{
        button, center, column, container, grid, opaque,
        pane_grid::{self},
        row, scrollable, stack, text, text_input,
    },
};
use iced_aw::{ContextMenu, Menu, MenuBar, menu::Item, menu_items};

use crate::clip_components::{
    ClipComponent, ClipProperties, audio_clip::AudioComponent, solid_clip::SolidComponent,
    text_clip::TextComponent, video_clip::VideoComponent,
};
use crate::helper_funcs::to_avio_color;
use crate::preview::Preview;
use crate::project::Project;
use crate::thumbnails::SourcePreview;
use crate::widgets::clip_entry::ClipEntry;
use crate::widgets::clip_timeline;
use crate::widgets::theme::{THEME, button_style};
use crate::widgets::timeline::Timeline;

use avio::{ClipId, Command, MediaInfo, TrackId, TrackKind, open};
use std::fs;

fn browse(dir: &str) -> Vec<(String, MediaInfo)> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| {
            let path: PathBuf = entry.ok()?.path();
            let info = open(&path).ok()?; // reads container/stream metadata, no decode
            Some((path.to_string_lossy().into_owned(), info))
        })
        .collect()
}

#[derive(Debug)]
enum PaneType {
    Clips,
    Timeline,
    Viewer,
    Browser,
    ColorGrading,
}

struct App {
    panes: pane_grid::State<PaneType>,
    project: Project,
    /// User-given track names. avio has no command to rename a track, so these
    /// live beside the timeline rather than in its undo history.
    track_names: HashMap<TrackId, String>,
    clips_entries: Vec<ClipEntry<Message>>,
    /// First frames and waveforms of the source files in the clip browser.
    previews: HashMap<PathBuf, SourcePreview>,
    dragging_clip_index: Option<usize>,
    /// What the timeline's context menu acts on: the right-clicked clip (if any)
    /// and the timeline position of the click.
    clip_menu: (Option<ClipId>, Duration),
    /// Clips selected on the timeline. May hold ids of clips an undo removed.
    selected: HashSet<ClipId>,
    /// The last copied clip, with the track it was copied from.
    clipboard: Option<(TrackId, avio::Clip)>,
    /// Player for the editor's current timeline, positioned at `playhead`.
    preview: Option<Preview>,
    /// Bumped on every reload so results from superseded loads are discarded.
    preview_generation: u64,
    playhead: Duration,
    playing: bool,
    /// The frame on screen. Held as an allocation so the renderer keeps it
    /// uploaded and draws it immediately, instead of blanking while it loads.
    frame: Option<iced::widget::image::Allocation>,
    /// Whether an export is running; blocks starting a second one.
    exporting: bool,
    /// Contents of the Insert menu's text field, used by the next text clip.
    new_text: String,
    /// The open Properties dialog, if any.
    properties: Option<PropertiesDialog>,
    /// Timeline row (in display order) of the track cursor that `j`/`k` move.
    cursor_row: usize,
    /// Row where visual mode (`v`) started, while it is on. Visual mode puts a
    /// cursor on every row from here to `cursor_row`.
    visual_anchor: Option<usize>,
    /// Rows holding a track cursor, derived from `cursor_row` and
    /// `visual_anchor`. `i` splits the clips under the playhead on each row.
    cursor_tracks: Vec<usize>,
}

/// A clip's Properties dialog: the values being edited, applied on OK.
struct PropertiesDialog {
    clip: ClipId,
    values: ClipProperties,
    /// Whether the colour picker overlay is open.
    picking_color: bool,
}

impl Default for App {
    fn default() -> Self {
        let (mut panes, browser) = pane_grid::State::new(PaneType::Browser);
        let (clip, split) = panes
            .split(pane_grid::Axis::Vertical, browser, PaneType::Clips)
            .unwrap();
        panes.resize(split, 0.15);
        let (timeline, _) = panes
            .split(pane_grid::Axis::Horizontal, clip, PaneType::Timeline)
            .unwrap();
        panes.split(pane_grid::Axis::Vertical, clip, PaneType::Viewer);
        panes.split(pane_grid::Axis::Vertical, timeline, PaneType::ColorGrading);

        let timeline = avio::Timeline::builder()
            .canvas(1920, 1080)
            .frame_rate(30.0)
            .video_track(vec![]) // one empty video track to start
            .audio_track(vec![]) // one empty audio track to start
            .build()
            .expect("valid timeline");

        Self {
            panes,
            clips_entries: browse("./src/media/")
                .into_iter()
                .enumerate()
                .map(|(i, (name, info))| wire_entry(i, ClipEntry::new(name, &info)))
                .collect(),
            previews: HashMap::new(),
            dragging_clip_index: None,
            clip_menu: (None, Duration::ZERO),
            selected: HashSet::new(),
            cursor_row: 0,
            visual_anchor: None,
            cursor_tracks: vec![0],
            clipboard: None,
            project: Project::new(timeline),
            track_names: HashMap::new(),
            frame: None,
            playing: false,
            playhead: Duration::ZERO,
            preview: None,
            preview_generation: 0,
            exporting: false,
            new_text: String::new(),
            properties: None,
        }
    }
}

/// Length given to new text and solid clips; neither has one of its own.
const GENERATED_CLIP_LENGTH: Duration = Duration::from_secs(5);

/// Colours offered by the Insert menu for new solid clips.
const SOLID_PRESETS: [(&str, avio::Color); 5] = [
    ("Black", avio::Color::BLACK),
    ("White", avio::Color::WHITE),
    ("Red", avio::Color::rgb(255, 0, 0)),
    ("Green", avio::Color::rgb(0, 255, 0)),
    ("Blue", avio::Color::rgb(0, 0, 255)),
];

/// Wires a clip-browser entry at index `i` to the drag-and-drop messages.
fn wire_entry(i: usize, entry: ClipEntry<Message>) -> ClipEntry<Message> {
    entry
        .on_press(Message::ClipPressed(i))
        .on_drag(move |p| Message::ClipMoved(i, p))
        .on_drop(Message::ClipDropped(i))
}

/// Asks for media files and probes each one; unreadable files are skipped.
async fn pick_media() -> Vec<MediaInfo> {
    let Some(files) = rfd::AsyncFileDialog::new()
        .set_title("Import Media")
        .pick_files()
        .await
    else {
        return Vec::new();
    };
    let paths: Vec<PathBuf> = files.iter().map(|f| f.path().to_path_buf()).collect();
    // Probing reads container headers from disk, so keep it off the UI thread.
    tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .filter_map(|path| match open(&path) {
                Ok(info) => Some(info),
                Err(e) => {
                    eprintln!("import {}: {e}", path.display());
                    None
                }
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[derive(Clone)]
pub enum Message {
    NoOp,
    Resized(pane_grid::ResizeEvent),
    Dragged(pane_grid::DragEvent),
    MouseReleased,
    ClipPressed(usize),
    ClipMoved(usize, Point),
    /// Carries the index of the clip entry being dropped.
    ClipDropped(usize),
    /// Fired by the Timeline when a dragged clip is released over it.
    /// Carries the target track, the timeline offset, and the clip-entry index.
    ClipDroppedToTimeline(TrackId, Duration, usize),
    /// A finished move/trim gesture on the Timeline, applied as one undo step.
    TimelineEdited(Vec<Command>),
    /// The timeline was right-clicked, over this clip if any, at this position.
    ClipMenu(Option<ClipId>, Duration),
    /// Put a copy of this clip, from this track, on the clipboard.
    CopyClip(TrackId, avio::Clip),
    /// Place the clipboard's clip at the playhead on the track it came from.
    PasteClip,
    // Delete the clip from its track, as one undo step.
    RemoveClip(TrackId, ClipId),
    /// The timeline selection changed to exactly these clips.
    SelectClips(HashSet<ClipId>),
    /// Select every clip on the timeline.
    SelectAll,
    /// Delete every selected clip, as one undo step.
    DeleteSelected,
    /// Cut every selected clip the playhead is over, as one undo step.
    SplitAtCursors,
    ToggleVisualMode,
    ExitVisualMode,
    /// Append an empty track of this kind.
    AddTrack(TrackKind),
    /// A track's label was edited; carries its full new text.
    RenameTrack(TrackId, String),
    /// A track's toggle was pressed; carries the requested enabled state.
    SetTrackEnabled(TrackId, bool),
    Undo,
    Redo,
    /// The playhead was moved on the Timeline.
    Seek(Duration),
    PreviewReady(u64, Result<Preview, String>),
    /// A decoded frame finished uploading; carries the preview generation.
    FrameAllocated(
        u64,
        Result<iced::widget::image::Allocation, iced::widget::image::Error>,
    ),
    Tick,
    TogglePlay,
    /// Pause and move the playhead by this many frames (negative steps back).
    StepFrames(i64),
    SeekToStart,
    /// Seek to the timeline's last frame.
    SeekToEnd,
    /// Seek to the start of the clip under the playhead on the cursor's track,
    /// or of the previous clip there when already at a start.
    SeekToClipStart,
    /// Seek to the last frame of the clip under the playhead on the cursor's
    /// track, or of the next clip there when already at an end.
    SeekToClipEnd,
    /// Open a file picker and add the chosen files to the clip browser.
    ImportMedia,
    /// Probed files from the picker, with their durations.
    MediaImported(Vec<MediaInfo>),
    /// A source file's first frame and/or waveform finished decoding.
    SourcePreviewLoaded(PathBuf, SourcePreview),
    /// Ask where to save, then render the project there.
    Export,
    /// The save dialog closed, with the chosen path unless cancelled.
    ExportTo(Option<PathBuf>),
    /// The Insert menu's text field was edited; carries its full new text.
    NewTextChanged(String),
    /// Add a text clip showing the text field's contents to the clip browser.
    AddTextClip,
    /// Add a solid clip of this named colour to the clip browser.
    AddSolidClip(&'static str, avio::Color),
    /// The export finished, writing this file, or failed.
    ExportFinished(Result<PathBuf, String>),
    /// Open the Properties dialog for this clip.
    OpenProperties(ClipId),
    /// The Properties dialog's text field was edited; carries its full new text.
    PropertiesTextChanged(String),
    /// Open the Properties dialog's colour picker.
    PickColor,
    /// The colour picker was closed without choosing.
    CancelColor,
    /// The colour picker submitted this colour.
    ColorPicked(Color),
    /// Apply the Properties dialog's values to its clip, as one undo step.
    ApplyProperties,
    /// Close the Properties dialog without applying.
    CloseProperties,
    TimelineCursorMovedVertical(i32),
}

impl App {
    /// The Properties dialog, centred over a dimmed backdrop, with the fields
    /// the clip's component provides.
    fn properties_view<'a>(&self, dialog: &'a PropertiesDialog) -> Element<'a, Message> {
        let mut form = column![text("Properties").size(18)].spacing(10);
        let timeline = self.project.media();
        if let Some(fields) = find_clip(timeline, dialog.clip)
            .and_then(|(_, clip)| component_for(timeline, clip).properties_view(dialog))
        {
            form = form.push(fields);
        }
        form = form.push(
            row![
                button("OK")
                    .style(button_style)
                    .on_press(Message::ApplyProperties),
                button("Cancel")
                    .style(button_style)
                    .on_press(Message::CloseProperties),
            ]
            .spacing(10),
        );
        center(
            container(form)
                .padding(16)
                .width(320)
                .style(container::bordered_box),
        )
        .style(|_| {
            container::background(Color {
                a: 0.6,
                ..Color::BLACK
            })
        })
        .into()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let file_menu: Menu<'_, Message, Theme, Renderer> = Menu::new(menu_items!(
            (button("New")
                .style(button_style)
                .on_press(Message::NoOp)
                .width(Length::Fill)),
            (button("Open")
                .style(button_style)
                .on_press(Message::NoOp)
                .width(Length::Fill)),
            (button("Import Media")
                .style(button_style)
                .on_press(Message::ImportMedia)
                .width(Length::Fill)),
            (button(if self.exporting {
                "Exporting..."
            } else {
                "Export"
            })
            .style(button_style)
            .on_press_maybe((!self.exporting).then_some(Message::Export))
            .width(Length::Fill)),
            (button("Exit")
                .style(button_style)
                .on_press(Message::NoOp)
                .width(Length::Fill)),
        ))
        .spacing(5.0)
        .width(120);

        let edit_menu: Menu<'_, Message, _, _> = Menu::new(menu_items!(
            (button("Undo")
                .style(button_style)
                .on_press_maybe(self.project.can_undo().then_some(Message::Undo))
                .width(Length::Fill)),
            (button("Redo")
                .style(button_style)
                .on_press_maybe(self.project.can_redo().then_some(Message::Redo))
                .width(Length::Fill)),
            (button("Paste")
                .style(button_style)
                .on_press_maybe(self.clipboard.is_some().then_some(Message::PasteClip))
                .width(Length::Fill)),
        ))
        .spacing(5.0)
        .width(100);

        let has_text = !self.new_text.trim().is_empty();
        let mut insert_items: Vec<Element<'_, Message>> = vec![
            text_input("Title text", &self.new_text)
                .on_input(Message::NewTextChanged)
                .on_submit_maybe(has_text.then_some(Message::AddTextClip))
                .into(),
            button("Add Text")
                .style(button_style)
                .on_press_maybe(has_text.then_some(Message::AddTextClip))
                .width(Length::Fill)
                .into(),
        ];
        insert_items.extend(SOLID_PRESETS.into_iter().map(|(name, color)| {
            button(text(format!("Solid: {name}")))
                .style(button_style)
                .on_press(Message::AddSolidClip(name, color))
                .width(Length::Fill)
                .into()
        }));
        let insert_menu: Menu<'_, Message, _, _> =
            Menu::new(insert_items.into_iter().map(Item::new).collect())
                .spacing(5.0)
                .width(160);

        let head: MenuBar<'_, Message, _, _> = MenuBar::new(menu_items!(
            (container(text("File")), file_menu),
            (container(text("Edit")), edit_menu),
            (container(text("Insert")), insert_menu),
        ))
        .spacing(10);

        let body = pane_grid::PaneGrid::new(&self.panes, |_pane, state, _is_maximized| {
            let (title, content): (&str, Element<'_, Message>) = match state {
                PaneType::Clips => (
                    "Clips",
                    grid(self.clips_entries.iter().cloned().map(Element::from))
                        .height(2000.0)
                        .spacing(10)
                        .into(),
                ),
                PaneType::Timeline => ("Timeline", {
                    let mut timeline = Timeline::new(self.project.media(), self.playhead)
                        .on_seek(Message::Seek)
                        .on_edit(Message::TimelineEdited)
                        .track_names(&self.track_names)
                        .previews(&self.previews)
                        .on_rename(Message::RenameTrack)
                        .on_add_track(Message::AddTrack)
                        .on_toggle_track(Message::SetTrackEnabled)
                        .on_context(Message::ClipMenu)
                        .selected(&self.selected)
                        .cursor_tracks(&self.cursor_tracks)
                        .on_select(Message::SelectClips);
                    if let Some(index) = self.dragging_clip_index {
                        timeline = timeline.on_drop(move |track, offset| {
                            Message::ClipDroppedToTimeline(track, offset, index)
                        });
                    }
                    let scrolled = scrollable(timeline)
                        .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new()));
                    ContextMenu::new(scrolled, move || {
                        let (clip, _) = self.clip_menu;
                        clip_timeline::context_menu(
                            self.project.media(),
                            clip,
                            self.playhead,
                            &self.selected,
                        )
                    })
                    .into()
                }),
                PaneType::Viewer => {
                    let picture: Element<'_, Message> = match &self.frame {
                        // Every frame comes composited onto the canvas.
                        Some(frame) => iced::widget::image(frame.handle().clone())
                            .content_fit(iced::ContentFit::Contain)
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .into(),
                        None => container(text("no preview")).height(Length::Fill).into(),
                    };
                    let has_clips = preview::timeline_end(self.project.media()) > Duration::ZERO;
                    let toggle = button(if self.playing { "Pause" } else { "Play" })
                        .style(button_style)
                        .on_press_maybe(has_clips.then_some(Message::TogglePlay));
                    ("Viewer", column![picture, toggle].into())
                }
                PaneType::Browser => {
                    let file_entry: Vec<Element<_>> = fs::read_dir(".")
                        .unwrap()
                        .map(|x| {
                            let name = x.unwrap().file_name();
                            text(name.to_string_lossy().into_owned()).into()
                        })
                        .collect::<Vec<Element<_>>>();
                    ("Browser", column(file_entry).width(Length::Shrink).into())
                }
                PaneType::ColorGrading => ("ColorGrading", text("ColorGrading").into()),
            };
            pane_grid::Content::new(content).title_bar(pane_grid::TitleBar::new(text(title)))
        })
        .on_resize(10, Message::Resized)
        .on_drag(Message::Dragged);
        let page = column![head, body];
        match &self.properties {
            Some(dialog) => stack![page, opaque(self.properties_view(dialog))].into(),
            None => page.into(),
        }
    }

    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::NoOp => {
                todo!()
            }
            Message::Resized(pane_grid::ResizeEvent { split, ratio, .. }) => {
                self.panes.resize(split, ratio);
            }
            Message::Dragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                self.panes.drop(pane, target);
            }
            Message::Dragged(x) => {
                dbg!(x);
            }
            Message::ClipPressed(x) => {
                dbg!(x);
            }
            Message::ClipDropped(index) => {
                // Mark which clip entry is being dragged so the Timeline knows
                self.dragging_clip_index = Some(index);
            }
            Message::ClipMoved(i, p) => {
                self.dragging_clip_index = Some(i);
                if let Some(clip) = self.clips_entries.get_mut(i) {
                    clip.position = p;
                }
            }
            Message::ClipDroppedToTimeline(track, offset, index) => {
                if let Some(entry) = self.clips_entries.get(index) {
                    let components = entry.components.clone();
                    if let Err(e) = self.place_components(&components, track, offset) {
                        eprintln!("add clip: {e}");
                    }
                }
                self.dragging_clip_index = None;
                return self.reload_preview();
            }
            Message::TimelineEdited(commands) => {
                if let Err(e) = self.apply_overwriting(&commands) {
                    eprintln!("timeline edit: {e}");
                    return iced::Task::none();
                }
                return self.reload_preview();
            }
            Message::ClipMenu(clip, at) => {
                self.clip_menu = (clip, at);
            }
            Message::CopyClip(track, clip) => {
                self.clipboard = Some((track, clip));
            }
            Message::RemoveClip(_, clip) => {
                let command = Command::RemoveClip { clip };
                if let Err(e) = self.apply_overwriting(&[command]) {
                    eprintln!("delete clip: {e}");
                }
                return self.reload_preview();
            }
            Message::SelectClips(selection) => {
                self.selected = selection;
            }
            Message::SelectAll => {
                self.selected = helper_funcs::tracks(self.project.media())
                    .flat_map(|track| &track.clips)
                    .map(|clip| clip.id)
                    .collect();
            }
            Message::DeleteSelected => {
                let commands: Vec<Command> = std::mem::take(&mut self.selected)
                    .into_iter()
                    .filter(|&clip| find_clip(self.project.media(), clip).is_some())
                    .map(|clip| Command::RemoveClip { clip })
                    .collect();
                if commands.is_empty() {
                    return iced::Task::none();
                }
                if let Err(e) = self.apply_overwriting(&commands) {
                    eprintln!("delete clips: {e}");
                }
                return self.reload_preview();
            }
            Message::SplitAtCursors => {
                let timeline = self.project.media();
                let commands: Vec<Command> = self
                    .cursor_tracks
                    .iter()
                    .filter_map(|&row| helper_funcs::track_at_row(timeline, row))
                    .flat_map(|track| &track.clips)
                    .filter_map(|clip| component_for(timeline, clip).split(self.playhead))
                    .collect();
                if commands.is_empty() {
                    return iced::Task::none();
                }
                return self.update(Message::TimelineEdited(commands));
            }
            Message::PasteClip => {
                if let Some((track, clip)) = &self.clipboard {
                    let command = Command::AddClip {
                        track: *track,
                        clip: Box::new(clip.clone().offset(self.playhead)),
                    };
                    if let Err(e) = self.apply_overwriting(&[command]) {
                        eprintln!("paste clip: {e}");
                    }
                    return self.reload_preview();
                }
            }
            Message::AddTrack(kind) => {
                // An empty track renders nothing, so the preview stays valid.
                if let Err(e) = self.project.apply_media(&Command::AddTrack { kind }) {
                    eprintln!("add track: {e}");
                }
            }
            Message::RenameTrack(track, name) => {
                self.track_names.insert(track, name);
            }
            Message::SetTrackEnabled(track, enabled) => {
                if let Err(e) = self.project.set_track_enabled(track, enabled) {
                    eprintln!("toggle track: {e}");
                }
                return self.reload_preview();
            }
            Message::Undo => {
                self.project.undo();
                return self.reload_preview();
            }
            Message::Redo => {
                self.project.redo();
                return self.reload_preview();
            }
            Message::MouseReleased => {
                self.dragging_clip_index = None;
            }
            Message::Seek(at) => {
                let timeline = self.project.media();
                self.playhead = at;
                if !preview::is_covered(timeline, at) {
                    // Nothing on screen here; don't show the next clip's first frame.
                    self.frame = None;
                }
                match &self.preview {
                    // Seeking past the end would stop the player, so leave it be.
                    Some(p) if p.is_running() => {
                        if at < preview::timeline_end(timeline) {
                            p.seek(at, self.playing);
                        }
                    }
                    _ => return self.reload_preview(),
                }
            }
            Message::PreviewReady(generation, Ok(p)) => {
                if generation != self.preview_generation {
                    p.stop();
                } else {
                    self.preview = Some(p);
                }
            }
            Message::PreviewReady(_, Err(e)) => eprintln!("preview: {e}"),
            Message::FrameAllocated(generation, Ok(frame)) => {
                if generation == self.preview_generation
                    && (self.playing || preview::is_covered(self.project.media(), self.playhead))
                {
                    self.frame = Some(frame);
                }
            }
            Message::FrameAllocated(_, Err(e)) => eprintln!("preview frame: {e}"),
            Message::TogglePlay => {
                if self.playing {
                    self.playing = false;
                    if let Some(p) = &self.preview {
                        p.pause();
                    }
                } else {
                    if self.playhead >= preview::timeline_end(self.project.media()) {
                        self.playhead = Duration::ZERO;
                    }
                    self.playing = true;
                    match &self.preview {
                        Some(p) if p.is_running() => p.play(),
                        _ => return self.reload_preview(),
                    }
                }
            }
            Message::StepFrames(frames) => {
                dbg!(frames);
                let timeline = self.project.media();
                let frame = Duration::from_secs_f64(1.0 / timeline.frame_rate());
                let last = preview::timeline_end(timeline).saturating_sub(frame);
                let delta = frame * frames.unsigned_abs() as u32;
                let target = if frames < 0 {
                    self.playhead.saturating_sub(delta)
                } else {
                    (self.playhead + delta).min(last)
                };
                dbg!(target);
                if self.playing {
                    self.playing = false;
                    if let Some(p) = &self.preview {
                        p.pause();
                    }
                }
                return self.update(Message::Seek(target));
            }
            Message::SeekToStart => return self.update(Message::Seek(Duration::ZERO)),
            Message::SeekToEnd => {
                let timeline = self.project.media();
                let frame = Duration::from_secs_f64(1.0 / timeline.frame_rate());
                let last = preview::timeline_end(timeline).saturating_sub(frame);
                return self.update(Message::Seek(last));
            }
            Message::SeekToClipStart => {
                let Some(track) = helper_funcs::track_at_row(self.project.media(), self.cursor_row)
                else {
                    return iced::Task::none();
                };
                let start = track
                    .clips
                    .iter()
                    .map(|clip| clip.offset)
                    .filter(|&start| start < self.playhead)
                    .max();
                if let Some(start) = start {
                    return self.update(Message::Seek(start));
                }
            }
            Message::SeekToClipEnd => {
                let timeline = self.project.media();
                let Some(track) = helper_funcs::track_at_row(timeline, self.cursor_row) else {
                    return iced::Task::none();
                };
                let frame = Duration::from_secs_f64(1.0 / timeline.frame_rate());
                let end = track
                    .clips
                    .iter()
                    .filter_map(helper_funcs::span)
                    .map(|(start, end)| end.saturating_sub(frame).max(start))
                    .filter(|&last| last > self.playhead)
                    .min();
                if let Some(end) = end {
                    return self.update(Message::Seek(end));
                }
            }
            Message::ImportMedia => {
                return iced::Task::perform(pick_media(), Message::MediaImported);
            }
            Message::MediaImported(files) => {
                return iced::Task::batch(files.into_iter().map(|info| {
                    let name = info.path().to_string_lossy().into_owned();
                    self.push_entry(ClipEntry::new(name, &info))
                }));
            }
            Message::SourcePreviewLoaded(path, preview) => {
                for entry in &mut self.clips_entries {
                    if entry
                        .media_source()
                        .is_some_and(|(source, ..)| source == path)
                    {
                        entry.preview = Some(preview.clone());
                    }
                }
                self.previews.insert(path, preview);
            }
            Message::NewTextChanged(text) => self.new_text = text,
            Message::AddTextClip => {
                let text = std::mem::take(&mut self.new_text);
                let name = format!("Text: {text}");
                let component =
                    TextComponent::new(avio::TextSpec::new(text), GENERATED_CLIP_LENGTH);
                return self.push_entry(ClipEntry::from_component(name, Arc::new(component)));
            }
            Message::AddSolidClip(name, color) => {
                let component = SolidComponent::new(color, GENERATED_CLIP_LENGTH);
                return self.push_entry(ClipEntry::from_component(
                    format!("Solid: {name}"),
                    Arc::new(component),
                ));
            }
            Message::Export => {
                if self.exporting {
                    return iced::Task::none();
                }
                self.exporting = true;
                let dialog = rfd::AsyncFileDialog::new()
                    .set_title("Export")
                    .set_file_name("export.mp4")
                    .add_filter("MP4 video", &["mp4"])
                    .save_file();
                return iced::Task::perform(
                    async move { dialog.await.map(|file| file.path().to_path_buf()) },
                    Message::ExportTo,
                );
            }
            Message::ExportTo(None) => self.exporting = false,
            Message::ExportTo(Some(path)) => {
                let timeline = self.project.media();
                let config = avio::EncoderConfig::builder()
                    .resolution(timeline.canvas_width(), timeline.canvas_height())
                    .framerate(timeline.frame_rate())
                    .build();
                let render = self.project.render(path.clone(), config, |_| true);
                return iced::Task::perform(
                    async move { render.await.map(|()| path) },
                    Message::ExportFinished,
                );
            }
            Message::ExportFinished(result) => {
                self.exporting = false;
                match result {
                    Ok(path) => println!("exported {}", path.display()),
                    Err(e) => eprintln!("export: {e}"),
                }
            }
            Message::OpenProperties(id) => {
                if let Some((_, clip)) = find_clip(self.project.media(), id) {
                    let values = component_for(self.project.media(), clip).properties();
                    self.properties = Some(PropertiesDialog {
                        clip: id,
                        values,
                        picking_color: false,
                    });
                }
            }
            Message::PropertiesTextChanged(text) => {
                if let Some(dialog) = &mut self.properties {
                    dialog.values.text = Some(text);
                }
            }
            Message::PickColor => {
                if let Some(dialog) = &mut self.properties {
                    dialog.picking_color = true;
                }
            }
            Message::CancelColor => {
                if let Some(dialog) = &mut self.properties {
                    dialog.picking_color = false;
                }
            }
            Message::ColorPicked(color) => {
                if let Some(dialog) = &mut self.properties {
                    dialog.picking_color = false;
                    dialog.values.color = Some(to_avio_color(color));
                }
            }
            Message::ApplyProperties => {
                let Some(dialog) = self.properties.take() else {
                    return iced::Task::none();
                };
                let command = find_clip(self.project.media(), dialog.clip).and_then(|(_, clip)| {
                    component_for(self.project.media(), clip).set_properties(&dialog.values)
                });
                if let Some(command) = command {
                    if let Err(e) = self.project.apply_media(&command) {
                        eprintln!("clip properties: {e}");
                        return iced::Task::none();
                    }
                    return self.reload_preview();
                }
            }
            Message::CloseProperties => self.properties = None,
            Message::TimelineCursorMovedVertical(delta) => {
                let last = helper_funcs::tracks(self.project.media())
                    .count()
                    .saturating_sub(1);
                self.cursor_row = self
                    .cursor_row
                    .saturating_add_signed(delta as isize)
                    .min(last);
                self.update_cursor_tracks();
            }
            Message::ToggleVisualMode => {
                self.visual_anchor = match self.visual_anchor {
                    Some(_) => None,
                    None => Some(self.cursor_row),
                };
                self.update_cursor_tracks();
            }
            Message::ExitVisualMode => {
                self.visual_anchor = None;
                self.update_cursor_tracks();
                self.selected.clear();
            }
            Message::Tick => {
                if let Some(p) = &self.preview {
                    if self.playing && !p.is_running() {
                        // Reached the end of the timeline.
                        self.playing = false;
                    }
                    // While parked in a gap, keep the viewer blank.
                    let show =
                        self.playing || preview::is_covered(self.project.media(), self.playhead);
                    // Taken even while hidden, so a frame from a paused seek into
                    // a gap doesn't count as new once play is pressed. Taking
                    // also hands over the pixels without copying them.
                    let frame = p.frames.lock().unwrap().take();
                    if let Some(f) = frame {
                        if show {
                            if self.playing {
                                // Follow playback with the frame actually on screen.
                                self.playhead = f.pts;
                            }
                            // Swap it in only once it's uploaded; a fresh handle
                            // otherwise draws blank for a frame and flickers.
                            let handle = image::Handle::from_rgba(f.width, f.height, f.data);
                            let generation = self.preview_generation;
                            return iced::widget::image::allocate(handle)
                                .map(move |result| Message::FrameAllocated(generation, result));
                        }
                    }
                }
            }
        }
        iced::Task::none()
    }

    /// Adds an entry to the clip browser, ready to drag onto the timeline, and
    /// starts decoding its source's previews if they aren't loaded yet.
    fn push_entry(&mut self, mut entry: ClipEntry<Message>) -> iced::Task<Message> {
        let source = entry.media_source();
        let task = match &source {
            Some((path, ..)) if self.previews.contains_key(path) => {
                entry.preview = self.previews.get(path).cloned();
                iced::Task::none()
            }
            Some((path, video, audio)) => iced::Task::perform(
                thumbnails::load(path.clone(), *video, *audio),
                |(path, preview)| Message::SourcePreviewLoaded(path, preview),
            ),
            None => iced::Task::none(),
        };
        let i = self.clips_entries.len();
        self.clips_entries.push(wire_entry(i, entry));
        task
    }

    /// The app at startup, decoding the previews of the media found in
    /// `./src/media/`.
    fn boot() -> (Self, iced::Task<Message>) {
        let app = Self::default();
        let loads = app.clips_entries.iter().filter_map(|entry| {
            let (path, video, audio) = entry.media_source()?;
            Some(iced::Task::perform(
                thumbnails::load(path, video, audio),
                |(path, preview)| Message::SourcePreviewLoaded(path, preview),
            ))
        });
        let task = iced::Task::batch(loads);
        (app, task)
    }

    /// Places every component of a clip entry at `offset`, as one undo step.
    /// Each goes on the track of its own kind matching `dropped` (see
    /// [`helper_funcs::matching_track`]); a track is added for any kind the
    /// timeline has none of. On error nothing changes.
    fn place_components(
        &mut self,
        components: &[Arc<dyn ClipComponent>],
        dropped: TrackId,
        offset: Duration,
    ) -> Result<(), avio::EditError> {
        // `apply_overwriting` commits (or cancels) this group, so added
        // tracks share the clips' undo step.
        self.project.begin_group();
        let mut commands = Vec::with_capacity(components.len());
        for component in components {
            let kind = component.track_kind();
            let track = match helper_funcs::matching_track(self.project.media(), kind, dropped) {
                Some(track) => track,
                None => {
                    if let Err(e) = self.project.apply_media(&Command::AddTrack { kind }) {
                        self.project.cancel_group();
                        return Err(e);
                    }
                    helper_funcs::matching_track(self.project.media(), kind, dropped)
                        .expect("a track of this kind was just added")
                }
            };
            commands.push(Command::AddClip {
                track,
                clip: Box::new(component.avio_clip().clone().offset(offset)),
            });
        }
        self.apply_overwriting(&commands)
    }

    /// Applies `commands` as one undo step. Every clip they place takes over its
    /// span, trimming whatever it landed on. On error nothing changes.
    fn apply_overwriting(&mut self, commands: &[Command]) -> Result<(), avio::EditError> {
        self.project.begin_group();
        let result = commands.iter().try_for_each(|command| {
            self.project.apply_media(command)?;
            let Some(placed) = overwrite::placed_clip(self.project.media(), command) else {
                return Ok(());
            };
            overwrite::clear_under(self.project.media(), placed)
                .iter()
                .try_for_each(|clear| self.project.apply_media(clear))
        });
        match result {
            Ok(()) => self.project.commit_group(),
            Err(_) => self.project.cancel_group(),
        }
        result
    }

    /// Replaces the player with a fresh one for the current timeline, positioned
    /// at the playhead. The player can't take structural edits in place, and it
    /// can't be restarted once it has run to the end.
    fn reload_preview(&mut self) -> iced::Task<Message> {
        if let Some(old) = self.preview.take() {
            old.stop();
        }
        self.preview_generation += 1;

        let timeline = self.project.media().clone();
        if preview::timeline_end(&timeline) == Duration::ZERO {
            self.frame = None;
            self.playing = false;
            return iced::Task::none();
        }
        if !preview::is_covered(&timeline, self.playhead) {
            self.frame = None;
        }

        let generation = self.preview_generation;
        let (start, play) = (self.playhead, self.playing);
        iced::Task::perform(
            async move {
                tokio::task::spawn_blocking(move || Preview::open(&timeline, start, play))
                    .await
                    .map_err(|e| e.to_string())?
            },
            move |result| Message::PreviewReady(generation, result),
        )
    }

    /// Rebuild `cursor_tracks`: the rows between the visual anchor and the
    /// cursor in visual mode, otherwise the cursor's row alone.
    fn update_cursor_tracks(&mut self) {
        let anchor = self.visual_anchor.unwrap_or(self.cursor_row);
        let (from, to) = (anchor.min(self.cursor_row), anchor.max(self.cursor_row));
        self.cursor_tracks = (from..=to).collect();
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        let mouse = iced::event::listen_with(|event, _status, _window_id| match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(Message::MouseReleased)
            }
            _ => None,
        });
        // Only keys no widget consumed, so typing in a track label stays typing.
        let keys = iced::event::listen_with(|event, status, _window_id| match (event, status) {
            (
                iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }),
                iced::event::Status::Ignored,
            ) => keybinding(key.as_ref(), modifiers),
            _ => None,
        });
        let tick = if self.preview.is_some() {
            iced::time::every(std::time::Duration::from_millis(33)).map(|_| Message::Tick)
        } else {
            iced::Subscription::none()
        };
        iced::Subscription::batch([mouse, keys, tick])
    }
}

/// The clip with this id, with the track holding it.
fn find_clip(timeline: &avio::Timeline, id: ClipId) -> Option<(TrackId, &avio::Clip)> {
    timeline
        .video_tracks()
        .iter()
        .chain(timeline.audio_tracks())
        .find_map(|track| {
            let clip = track.clips.iter().find(|clip| clip.id == id)?;
            Some((track.id, clip))
        })
}

/// The component behind a clip on the timeline: text by its source, audio by
/// the track it sits on, video otherwise.
fn component_for(timeline: &avio::Timeline, clip: &avio::Clip) -> Box<dyn ClipComponent> {
    let clip = clip.clone();
    let on_audio_track = timeline
        .audio_tracks()
        .iter()
        .any(|track| track.clips.iter().any(|c| c.id == clip.id));
    match clip.source {
        avio::ClipSource::Text(_) => Box::new(TextComponent { clip }),
        avio::ClipSource::Solid(_) => Box::new(SolidComponent { clip }),
        _ if on_audio_track => Box::new(AudioComponent { clip }),
        _ => Box::new(VideoComponent { clip }),
    }
}

/// Maps a key press to the app action it triggers, if any.
fn keybinding(key: keyboard::Key<&str>, modifiers: keyboard::Modifiers) -> Option<Message> {
    use keyboard::{Key, key::Named};
    // `command()` is Ctrl on Linux/Windows and Cmd on macOS.
    match (key, modifiers.command(), modifiers.shift()) {
        (Key::Named(Named::Space), false, _) => Some(Message::TogglePlay),
        (Key::Character("u"), _, _) | (Key::Character("z"), true, false) => Some(Message::Undo),
        (Key::Character("r"), _, true) | (Key::Character("z"), true, true) => Some(Message::Redo),
        (Key::Character("a"), true, false) => Some(Message::SelectAll),
        (Key::Named(Named::Delete | Named::Backspace) | Key::Character("d"), false, _) => {
            Some(Message::DeleteSelected)
        }
        (Key::Named(Named::Escape), false, _) => Some(Message::ExitVisualMode),
        (Key::Character("v"), false, _) => Some(Message::ToggleVisualMode),
        // TODO(human): redo, frame stepping, and jumping to start/end.
        (Key::Character("i"), _, _) => Some(Message::SplitAtCursors),
        (Key::Character("b"), _, _) => Some(Message::SeekToClipStart),
        (Key::Character("e"), _, _) => Some(Message::SeekToClipEnd),
        (Key::Named(Named::Home), _, _) => Some(Message::SeekToStart),
        (Key::Named(Named::End), _, _) => Some(Message::SeekToEnd),

        (Key::Character("h"), _, _) => Some(Message::StepFrames(-1)),
        (Key::Character("l"), _, _) => Some(Message::StepFrames(1)),
        (Key::Character("j"), _, _) | (Key::Named(Named::ArrowDown), _, _) => {
            Some(Message::TimelineCursorMovedVertical(1))
        }
        (Key::Character("k"), _, _) | (Key::Named(Named::ArrowUp), _, _) => {
            Some(Message::TimelineCursorMovedVertical(-1))
        }
        _ => None,
    }
}

fn theme(_app: &App) -> Theme {
    THEME.clone()
}

fn main() -> Result<(), iced::Error> {
    iced::application(App::boot, App::update, App::view)
        .theme(theme)
        .subscription(App::subscription)
        .run()
}
