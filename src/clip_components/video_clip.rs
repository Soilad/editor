use std::{path::PathBuf, time::Duration};

use crate::clip_components::{ClipComponent, open_preview};
use crate::preview::Preview;
use crate::widgets::clip_timeline::SOURCE_DURATION_KEY;

/// A video file, wrapped as an [`avio::Clip`] trimmed to the file's full length.
#[derive(Debug, Clone)]
pub struct VideoComponent {
    pub clip: avio::Clip,
}

impl VideoComponent {
    pub fn new(path: PathBuf, duration: Duration) -> Self {
        let mut clip = avio::Clip::new(path)
            .trim(Duration::ZERO, duration)
            .with_fit(avio::FitMode::Fit);
        clip.metadata.insert(
            SOURCE_DURATION_KEY.to_owned(),
            duration.as_secs_f64().to_string(),
        );
        Self { clip }
    }
}

impl ClipComponent for VideoComponent {
    fn preview(&self) -> Result<Preview, String> {
        open_preview(vec![self.clip.clone().offset(Duration::ZERO)], Vec::new())
    }

    fn avio_clip(&self) -> &avio::Clip {
        &self.clip
    }
}
