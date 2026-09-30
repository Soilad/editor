//! The editor's document: avio's timeline for media, plus the DAW-side data
//! avio has no model for (MIDI, free-standing automation), under one undo history.

use std::{mem, path::PathBuf};

use avio::{Command, EditError, Editor, EncoderConfig, Progress, TimelineError};

/// Musical time. Ticks stay correct across tempo changes, unlike `Duration`.
pub type Ticks = u64;
/// Ticks per quarter note.
pub const PPQ: Ticks = 960;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MidiTrackId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LaneId(pub u64);

#[derive(Debug, Clone)]
pub struct Note {
    /// Start, relative to the clip's start.
    pub start: Ticks,
    pub length: Ticks,
    pub pitch: u8,
    pub velocity: u8,
}

#[derive(Debug, Clone)]
pub struct MidiClip {
    pub start: Ticks,
    pub length: Ticks,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone)]
pub struct MidiTrack {
    pub id: MidiTrackId,
    pub name: String,
    pub clips: Vec<MidiClip>,
}

/// What an automation lane drives.
#[derive(Debug, Clone)]
pub enum AutomationTarget {
    /// A media track's volume/pan; compiled into avio's `TrackAutomation` at render.
    MediaTrack(avio::TrackId, avio::AudioProperty),
    /// A synth parameter on a MIDI track; consumed by the MIDI renderer, never by avio.
    MidiParam(MidiTrackId, String),
}

#[derive(Debug, Clone)]
pub struct AutomationLane {
    pub id: LaneId,
    pub target: AutomationTarget,
    /// `(time, value)` breakpoints, sorted by time.
    pub points: Vec<(Ticks, f64)>,
}

/// Everything in the project that avio doesn't own. Snapshotted whole for undo.
#[derive(Debug, Clone, Default)]
pub struct OwnState {
    pub midi_tracks: Vec<MidiTrack>,
    pub automation: Vec<AutomationLane>,
}

/// One undo step. `media` says whether avio's `Editor` also moved a step;
/// `own` holds the *other* version of [`OwnState`], swapped in on undo/redo.
#[derive(Debug)]
struct Step {
    media: bool,
    own: Option<OwnState>,
}

/// An in-progress gesture folding several edits into one [`Step`].
#[derive(Debug)]
struct Group {
    /// Whether any media command succeeded inside the group.
    media_dirty: bool,
    /// [`OwnState`] as it was before the group's first own-state edit.
    own_before: Option<OwnState>,
}

pub struct Project {
    /// Kept (rather than bare `avio::apply`) because it guards avio's id
    /// high-water: its counters are crate-private, so only `Editor` can stop an
    /// edit after undo from re-minting a discarded `ClipId`.
    media: Editor,
    own: OwnState,
    undo: Vec<Step>,
    redo: Vec<Step>,
    group: Option<Group>,
}

impl Project {
    pub fn new(media: avio::Timeline) -> Self {
        Self {
            media: Editor::new(media),
            own: OwnState::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            group: None,
        }
    }

    pub fn media(&self) -> &avio::Timeline {
        self.media.current()
    }

    pub fn own(&self) -> &OwnState {
        &self.own
    }

    /// Encodes the project, as it is now, to `output`. `on_progress` runs after
    /// each video frame; returning `false` cancels the export.
    ///
    /// Only the media timeline is rendered: MIDI tracks and automation lanes
    /// have no renderer yet, so they are left out of the export.
    ///
    /// The encode runs on a blocking thread over a snapshot of the timeline,
    /// so edits made while it runs don't reach the file.
    pub fn render(
        &self,
        output: PathBuf,
        config: EncoderConfig,
        on_progress: impl Fn(&Progress) -> bool + Send + 'static,
    ) -> impl Future<Output = Result<(), String>> + Send + 'static {
        let timeline = self.media().clone();
        async move {
            if timeline.video_tracks().iter().all(|track| track.clips.is_empty()) {
                return Err("nothing to export: the timeline has no video clips".to_owned());
            }
            tokio::task::spawn_blocking(move || {
                timeline.render_with_progress(output, config, on_progress)
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e: TimelineError| e.to_string())
        }
    }

    /// Applies an avio command. On error nothing changes.
    pub fn apply_media(&mut self, command: &Command) -> Result<(), EditError> {
        self.media.apply(command)?;
        match &mut self.group {
            Some(g) => g.media_dirty = true,
            None => self.push(Step {
                media: true,
                own: None,
            }),
        }
        Ok(())
    }

    /// Enables or disables a media track, as one undo step (or as part of the
    /// open group). A disabled track contributes nothing to preview or export.
    ///
    /// avio has no command for this and keeps a timeline's tracks private, so
    /// the flag is flipped on a serde round-trip of the current timeline.
    pub fn set_track_enabled(
        &mut self,
        track: avio::TrackId,
        enabled: bool,
    ) -> Result<(), String> {
        let timeline = self.media();
        let (list, index, current) = [
            ("video_tracks", timeline.video_tracks()),
            ("audio_tracks", timeline.audio_tracks()),
        ]
        .into_iter()
        .find_map(|(list, tracks)| {
            let index = tracks.iter().position(|t| t.id == track)?;
            Some((list, index, tracks[index].enabled))
        })
        .ok_or_else(|| format!("track {track:?} not found"))?;
        if current == enabled {
            return Ok(());
        }

        let mut value = serde_json::to_value(timeline).map_err(|e| e.to_string())?;
        *value
            .get_mut(list)
            .and_then(|tracks| tracks.get_mut(index))
            .and_then(|track| track.get_mut("enabled"))
            .ok_or("timeline serialized without a track's enabled flag")? = enabled.into();
        let next: avio::Timeline = serde_json::from_value(value).map_err(|e| e.to_string())?;

        match &mut self.group {
            Some(g) => {
                self.media.replace_current(next);
                g.media_dirty = true;
            }
            None => {
                self.media.begin_group();
                self.media.replace_current(next);
                self.media.commit_group();
                self.push(Step {
                    media: true,
                    own: None,
                });
            }
        }
        Ok(())
    }

    /// Edits the non-avio state as one undo step (or as part of the open group).
    pub fn edit_own(&mut self, edit: impl FnOnce(&mut OwnState)) {
        match &mut self.group {
            Some(g) => {
                if g.own_before.is_none() {
                    g.own_before = Some(self.own.clone());
                }
                edit(&mut self.own);
            }
            None => {
                let before = self.own.clone();
                edit(&mut self.own);
                self.push(Step {
                    media: false,
                    own: Some(before),
                });
            }
        }
    }

    fn push(&mut self, step: Step) {
        self.undo.push(step);
        self.redo.clear();
    }

    /// Starts folding edits into one undo step. Groups don't nest.
    pub fn begin_group(&mut self) {
        if self.group.is_none() {
            self.media.begin_group();
            self.group = Some(Group {
                media_dirty: false,
                own_before: None,
            });
        }
    }

    /// Drops the open group's edits, restoring the state from before it.
    pub fn cancel_group(&mut self) {
        if let Some(g) = self.group.take() {
            self.media.cancel_group();
            if let Some(before) = g.own_before {
                self.own = before;
            }
        }
    }

    /// Closes the open group, recording its edits as a single undo step.
    pub fn commit_group(&mut self) {
        let Some(g) = self.group.take() else {
            return;
        };
        self.media.commit_group();
        if g.media_dirty || g.own_before.is_some() {
            self.push(Step { media: g.media_dirty, own: g.own_before });
        }
    }

    pub fn can_undo(&self) -> bool {
        self.group.is_none() && !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        self.group.is_none() && !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        if self.group.is_some() {
            return false;
        }
        let Some(mut step) = self.undo.pop() else {
            return false;
        };
        if step.media {
            self.media.undo();
        }
        if let Some(other) = &mut step.own {
            mem::swap(&mut self.own, other);
        }
        self.redo.push(step);
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.group.is_some() {
            return false;
        }
        let Some(mut step) = self.redo.pop() else {
            return false;
        };
        if step.media {
            self.media.redo();
        }
        if let Some(other) = &mut step.own {
            mem::swap(&mut self.own, other);
        }
        self.undo.push(step);
        true
    }
}

