use std::path::PathBuf;

pub struct Config {
    pub video_path: PathBuf,
    pub window_width: u32,
    pub window_height: u32,
    pub window_title: String,
}

impl Config {
    pub fn new(video_path: PathBuf) -> Self {
        Self {
            video_path,
            window_width: 800,
            window_height: 600,
            window_title: String::from("视频播放器"),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new(PathBuf::from("/Users/chinaxxren/Desktop/a.mp4"))
    }
}
