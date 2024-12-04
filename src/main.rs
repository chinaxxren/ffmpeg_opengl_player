mod config;
mod renderer;
mod player;
mod audio;
mod video;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ffmpeg_next as ffmpeg;
use ffmpeg::util::frame::Video as VideoFrame;

use glium::glutin::event::{Event, WindowEvent, KeyboardInput, ElementState, VirtualKeyCode};
use glium::glutin::event_loop::{ControlFlow, EventLoop};

use config::Config;
use renderer::Renderer;
use player::Player;

const TARGET_FPS: u32 = 30;
const FRAME_DURATION: Duration = Duration::from_micros((1_000_000f32 / TARGET_FPS as f32) as u64);

fn main() {
    println!("[Main] 程序启动");

    let config = Config::default();

    println!("[Main] 创建事件循环");
    let event_loop = EventLoop::new();

    // 创建一个帧缓冲区来存储最新的视频帧
    let frame_buffer = Arc::new(Mutex::new(None::<VideoFrame>));
    let frame_buffer_clone = frame_buffer.clone();

    println!("[Main] 创建播放器");
    let player = Player::start(
        config.video_path.clone(),
        Box::new(move |frame: &VideoFrame| {
            if let Ok(mut buffer) = frame_buffer_clone.lock() {
                *buffer = Some(frame.clone());
            }
        }),
        Box::new(|playing| {
            println!("[Player] 播放状态改变: {}", if playing { "播放" } else { "暂停" });
        }),
    ).expect("Failed to start player");

    let player = Arc::new(Mutex::new(player));

    // 等待第一帧
    println!("[Main] 等待第一帧");
    let mut first_frame = None;
    while first_frame.is_none() {
        if let Ok(buffer) = frame_buffer.lock() {
            if let Some(frame) = &*buffer {
                first_frame = Some(frame.clone());
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let first_frame = first_frame.unwrap();
    println!("[Main] 收到第一帧，尺寸: {}x{}", first_frame.width(), first_frame.height());

    println!("[Main] 创建渲染器");
    let mut renderer = Renderer::new(&config, &event_loop, first_frame.width() as u32, first_frame.height() as u32);

    let mut frame_count = 0;
    let mut last_fps_update = Instant::now();
    let mut last_frame_time = Instant::now();

    println!("[Main] 进入主事件循环");
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                println!("[Main] 接收到退出事件");
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::KeyboardInput {
                    input: KeyboardInput {
                        virtual_keycode: Some(keycode),
                        state: ElementState::Pressed,
                        ..
                    },
                    ..
                },
                ..
            } => {
                match keycode {
                    VirtualKeyCode::Space => {
                        println!("[Main] 空格键按下，切换播放状态");
                        if let Ok(mut player) = player.lock() {
                            player.toggle_pause_playing();
                        }
                    }
                    VirtualKeyCode::M => {
                        println!("[Main] M键按下，切换缩放模式");
                        renderer.toggle_scale_mode();
                    }
                    _ => (),
                }
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(physical_size),
                ..
            } => {
                // 检查窗口大小是否有效
                if physical_size.width == 0 || physical_size.height == 0 || 
                   physical_size.width == u32::MAX || physical_size.height == u32::MAX {
                    println!("[Main] 忽略无效的窗口大小: {:?}", physical_size);
                    return;
                }
                println!("[Main] 窗口大小改变: {:?}", physical_size);
                renderer.handle_resize(physical_size);
            }
            Event::MainEventsCleared => {
                let now = Instant::now();
                let elapsed = now.duration_since(last_frame_time);
                
                if elapsed >= FRAME_DURATION {
                    if let Ok(buffer) = frame_buffer.lock() {
                        if let Some(frame) = &*buffer {
                            frame_count += 1;
                            renderer.render_frame(frame);
                            last_frame_time = now;

                            if now.duration_since(last_fps_update) >= Duration::from_secs(1) {
                                println!("[Main] FPS: {}", frame_count);
                                frame_count = 0;
                                last_fps_update = now;
                            }
                        }
                    }
                } else {
                    std::thread::sleep(FRAME_DURATION - elapsed);
                }
            }
            _ => (),
        }
    });
}
