use std::path::PathBuf;

use crate::clip_components::ClipComponent;

#[derive(Default, Debug)]
pub struct VideoComponent {
    pub path: PathBuf,
}

impl ClipComponent for VideoComponent {
    fn get_path(&self) -> PathBuf {
        self.path.clone()
    }
}
