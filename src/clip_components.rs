use std::path::PathBuf;

pub mod video_clip;


pub trait ClipComponent {
    fn at_position(&self, position: u32) {}
    fn split(&self, position: u32) {}
    fn clip(&self, left_position: u32, right_position: u32) {}
    fn set_speed(&self, speed: u32) {}
    fn preview(&self) {}
    fn get_path(&self) -> PathBuf;
}
