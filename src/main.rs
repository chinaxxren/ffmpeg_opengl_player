extern crate ffmpeg_next as ffmpeg;
use ffmpeg::format::Pixel;
use ffmpeg::util::frame::Video;
use glium::glutin::dpi::LogicalSize;
use glium::glutin::event::{ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent};
use glium::glutin::event_loop::{ControlFlow, EventLoop};
use glium::glutin::window::WindowBuilder;
use glium::glutin::ContextBuilder;
use glium::{implement_vertex, Display};
use glium::uniform;
use glium::Surface;
use std::borrow::Cow;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

mod audio;
mod player;
mod video;

use crate::player::Player;

#[derive(Copy, Clone)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

implement_vertex!(Vertex, position, tex_coords);

const SHOW_WIDTH: u32 = 800;
const SHOW_HEIGHT: u32 = 600;

fn main() {
    // 创建带缓冲的通道，避免阻塞
    let (frame_sender, frame_receiver) = mpsc::channel::<Video>();

    let path = "/Users/chinaxxren/Desktop/a.mp4";
    println!("开始播放视频: {}", path);

    // 保持对 Player 的引用
    let player = Arc::new(Mutex::new(
        Player::start(
            path.into(),
            {
                let sender = frame_sender.clone();
                move |frame| {
                    if let Err(e) = sender.send(frame.clone()) {
                        eprintln!("发送帧失败: {}", e);
                    }
                }
            },
            move |playing| {
                println!("播放状态: {}", playing);
            },
        )
        .expect("Failed to start player"),
    ));

    // 创建事件循环和窗口
    let event_loop = EventLoop::new();
    let window_builder = WindowBuilder::new()
        .with_title("视频播放器")
        .with_inner_size(LogicalSize::new(SHOW_WIDTH, SHOW_HEIGHT));
    
    let context_builder = ContextBuilder::new();
    let display = glium::Display::new(window_builder, context_builder, &event_loop)
        .expect("Failed to create display");

    // 创建顶点缓冲
    let vertex_buffer = {
        let vertices = vec![
            Vertex {
                position: [-1.0, -1.0],
                tex_coords: [0.0, 1.0],
            },
            Vertex {
                position: [1.0, -1.0],
                tex_coords: [1.0, 1.0],
            },
            Vertex {
                position: [1.0, 1.0],
                tex_coords: [1.0, 0.0],
            },
            Vertex {
                position: [-1.0, 1.0],
                tex_coords: [0.0, 0.0],
            },
        ];
        glium::VertexBuffer::new(&display, &vertices).expect("Failed to create vertex buffer")
    };

    let index_buffer = glium::IndexBuffer::new(
        &display,
        glium::index::PrimitiveType::TrianglesList,
        &[0u16, 1, 2, 0, 2, 3],
    )
    .expect("Failed to create index buffer");

    // Load shaders and create program
    let vertex_shader_src = include_str!("vertex_shader.glsl");
    let fragment_shader_src = include_str!("fragment_shader.glsl");

    let program =
        glium::Program::from_source(&display, vertex_shader_src, fragment_shader_src, None)
            .expect("Failed to create shader program");

    let mut frame_count = 0;
    let mut last_fps_update = Instant::now();
    let last_frame_time: Instant = Instant::now();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                println!("接收到退出事件");
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                virtual_keycode: Some(VirtualKeyCode::Space),
                                state: ElementState::Pressed,
                                ..
                            },
                        ..
                    },
                ..
            } => {
                if let Ok(mut player) = player.lock() {
                    player.toggle_pause_playing();
                }
            }
            Event::MainEventsCleared => {
                match frame_receiver.try_recv() {
                    Ok(frame) => {
                        frame_count += 1;

                        let new_frame = rescaler_for_frame(&frame);

                        println!("Frame info:");
                        println!(
                            "  Original width: {}, height: {}",
                            frame.width(),
                            frame.height()
                        );
                        println!(
                            "  Rescaled width: {}, height: {}",
                            new_frame.width(),
                            new_frame.height()
                        );
                        println!("  Format: {:?}", new_frame.format());

                        // Convert frame data to raw image data 
                        let y_data: &[u8] = new_frame.data(0);
                        let u_data: &[u8] = new_frame.data(1);
                        let v_data: &[u8] = new_frame.data(2);

                        let (y,u,v) = create_yuv_textures(&display, SHOW_WIDTH, SHOW_HEIGHT, y_data, u_data, v_data);
                        let uniforms = uniform! {
                            y_texture: &y,
                            u_texture: &u,
                            v_texture: &v,
                        };

                        // Render
                        let mut target = display.draw();
                        target.clear_color(0.0, 0.0, 0.0, 1.0);

                        target
                            .draw(
                                &vertex_buffer,
                                &index_buffer,
                                &program,
                                &uniforms,
                                &Default::default(),
                            )
                            .unwrap();

                        target.finish().unwrap();

                        if last_fps_update.elapsed() >= Duration::from_secs(1) {
                            println!("FPS: {}", frame_count);
                            frame_count = 0;
                            last_fps_update = Instant::now();
                        }

                        // Add frame rate control
                        let frame_duration = Duration::from_secs_f64(1.0 / 60.0); // 60 FPS
                        let elapsed = last_frame_time.elapsed();
                        if elapsed < frame_duration {
                            std::thread::sleep(frame_duration - elapsed);
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        // Reduce CPU usage when idle
                        std::thread::sleep(Duration::from_millis(16)); // Approximately 60 FPS
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        println!("Player disconnected, exiting loop");
                        *control_flow = ControlFlow::Exit;
                    }
                }
            }
            _ => (),
        }
    });
}

fn rescaler_for_frame(frame: &Video) -> Video {
    let mut context = ffmpeg_next::software::scaling::Context::get(
        frame.format(),
        frame.width(),
        frame.height(),
        Pixel::YUV420P, // Keep YUV420P format
        SHOW_WIDTH,
        SHOW_HEIGHT,
        ffmpeg::software::scaling::Flags::BILINEAR,
    )
    .unwrap();

    let mut new_frame = Video::empty();
    context.run(&frame, &mut new_frame).unwrap();

    new_frame
}


use glium::texture::{RawImage2d, SrgbTexture2d};

fn create_yuv_textures(display: &Display, width: u32, height: u32, y_data: &[u8], u_data: &[u8], v_data: &[u8]) -> (SrgbTexture2d, SrgbTexture2d, SrgbTexture2d) {
    let y_image = RawImage2d {
        data: Cow::Borrowed(y_data),
        width,
        height,
        format: glium::texture::ClientFormat::U8,
    };
    
    let uv_width = width / 2;
    let uv_height = height / 2;
    let u_image = RawImage2d {
        data: Cow::Borrowed(u_data),
        width: uv_width,
        height: uv_height,
        format: glium::texture::ClientFormat::U8,
    };
    
    let v_image = RawImage2d {
        data: Cow::Borrowed(v_data),
        width: uv_width,
        height: uv_height,
        format: glium::texture::ClientFormat::U8,
    };

    let y_texture = SrgbTexture2d::new(display, y_image).unwrap();
    let u_texture = SrgbTexture2d::new(display, u_image).unwrap();
    let v_texture = SrgbTexture2d::new(display, v_image).unwrap();

    (y_texture, u_texture, v_texture)
}