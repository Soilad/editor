use std::{path::PathBuf, time::Duration};

use avio::{Command, TrackKind};
use iced::Element;

use crate::{Message, PropertiesDialog, preview::Preview};

pub mod audio_clip;
pub mod midi_clip;
pub mod solid_clip;
pub mod text_clip;
pub mod video_clip;

/// The user-editable settings of a clip, as shown in its Properties dialog.
/// A field is `None` when the clip has no such setting.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClipProperties {
    pub text: Option<String>,
    pub color: Option<avio::Color>,
}

impl ClipProperties {
    pub fn is_empty(&self) -> bool {
        self.text.is_none() && self.color.is_none()
    }
}

pub trait ClipComponent: Send + Sync {
    fn at_position(&self, position: u32) {}
    /// The razor cut that splits this clip at timeline position `at`, or `None`
    /// when `at` is not strictly inside the clip.
    fn split(&self, at: Duration) -> Option<Command> {
        let clip = self.avio_clip();
        let inside = match clip.duration() {
            Some(duration) => {
                let end = clip.offset + duration.div_f64(clip.speed.max(0.01));
                at > clip.offset && at < end
            }
            // Open-ended: the end is unknown here, so let avio range-check it.
            None => at > clip.offset,
        };
        inside.then_some(Command::SplitClip { clip: clip.id, at })
    }
    /// A detached copy of this clip, ready to be placed with [`Command::AddClip`]
    /// (which gives it a fresh id).
    fn copy(&self) -> avio::Clip {
        self.avio_clip().clone()
    }
    /// The settings this clip lets the user edit; empty by default.
    fn properties(&self) -> ClipProperties {
        ClipProperties::default()
    }
    /// The edit that gives this clip `properties`, or `None` when it has
    /// nothing editable.
    fn set_properties(&self, _properties: &ClipProperties) -> Option<Command> {
        None
    }
    fn clip(&self, left_position: u32, right_position: u32) {}
    fn set_speed(&self, speed: u32) {}
    /// Opens a player for just this clip. Blocks while the source is opened.
    fn preview(&self) -> Result<Preview, String>;
    /// The avio clip this component wraps; this is what gets placed on the timeline.
    fn avio_clip(&self) -> &avio::Clip;
    /// The kind of track this component's clip goes on.
    fn track_kind(&self) -> TrackKind {
        TrackKind::Video
    }
    fn get_path(&self) -> Option<PathBuf> {
        self.avio_clip()
            .source_path()
            .map(|path| path.to_path_buf())
    }
    /// Fields for this clip's section of the Properties dialog, or `None` when
    /// it has nothing to show.
    fn properties_view<'a>(&self, _dialog: &'a PropertiesDialog) -> Option<Element<'a, Message>> {
        None
    }
}

/// Opens a player, playing from the start, over a throwaway timeline holding
/// just these clips.
pub(crate) fn open_preview(
    video: Vec<avio::Clip>,
    audio: Vec<avio::Clip>,
) -> Result<Preview, String> {
    let mut builder = avio::Timeline::builder()
        .canvas(1920, 1080)
        .frame_rate(30.0)
        .video_track(video);
    if !audio.is_empty() {
        builder = builder.audio_track(audio);
    }
    let timeline = builder.build().map_err(|e| e.to_string())?;
    Preview::open(&timeline, Duration::ZERO, true)
}
