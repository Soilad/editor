use std::{path::PathBuf, time::Duration};

use avio::TrackKind;

use crate::clip_components::{ClipComponent, open_preview};
use crate::preview::Preview;
use crate::widgets::clip_timeline::SOURCE_DURATION_KEY;

/// An audio file, wrapped as an [`avio::Clip`] trimmed to the file's full length.
#[derive(Debug, Clone)]
pub struct AudioComponent {
    pub clip: avio::Clip,
}

impl AudioComponent {
    pub fn new(path: PathBuf, duration: Duration) -> Self {
        let mut clip = avio::Clip::new(path).trim(Duration::ZERO, duration);
        clip.metadata.insert(
            SOURCE_DURATION_KEY.to_owned(),
            duration.as_secs_f64().to_string(),
        );
        Self { clip }
    }
}

impl ClipComponent for AudioComponent {
    fn preview(&self) -> Result<Preview, String> {
        // The player refuses a timeline without video, so the audio plays under
        // a black frame spanning the same length.
        let clip = self.clip.clone().offset(Duration::ZERO);
        let length = clip
            .duration()
            .ok_or("audio clip has no length")?
            .div_f64(clip.speed.max(0.01));
        let backdrop = avio::Clip::solid(avio::Color::BLACK).trim(Duration::ZERO, length);
        open_preview(vec![backdrop], vec![clip])
    }

    fn avio_clip(&self) -> &avio::Clip {
        &self.clip
    }

    fn track_kind(&self) -> TrackKind {
        TrackKind::Audio
    }
}
