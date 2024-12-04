use std::path::PathBuf;
use crate::renderer::ScaleMode;

pub struct Config {
    pub video_path: PathBuf,
    /// 窗口初始宽度，之后的窗口尺寸由用户通过拖拽等操作来控制
    pub window_width: u32,
    /// 窗口初始高度，之后的窗口尺寸由用户通过拖拽等操作来控制
    pub window_height: u32,
    pub window_title: String,
    /// 视频缩放模式：
    /// - Fit: 按原视频比例显示，可能有黑边
    /// - Fill: 按原比例拉伸占满窗口，可能裁剪
    pub scale_mode: ScaleMode,
}

impl Config {
    pub fn new(video_path: PathBuf) -> Self {
        Self {
            video_path,
            window_width: 800,    // 初始窗口宽度
            window_height: 600,   // 初始窗口高度
            window_title: String::from("视频播放器"),
            scale_mode: ScaleMode::Fill,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new(PathBuf::from("/Users/chinaxxren/Desktop/a.mp4"))
    }
}
