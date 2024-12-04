extern crate ffmpeg_next as ffmpeg;
use ffmpeg::format::Pixel;
use ffmpeg::util::frame::Video;

use glium::glutin::event::{ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent};
use glium::glutin::event_loop::{ControlFlow, EventLoop};
use glium::glutin::window::WindowBuilder;
use glium::glutin::dpi::LogicalSize;
use glium::glutin::ContextBuilder;
use glium::{implement_vertex, Display, Program, Surface, uniform};
use glium::backend::glutin::DisplayCreationError;
use glium::texture::{RawImage2d, Texture2d, UncompressedFloatFormat, MipmapsOption, ClientFormat};
use glium::Rect;
use glium::uniforms::MagnifySamplerFilter;

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
        .with_inner_size(LogicalSize::new(800, 600));
    
    let context_builder = ContextBuilder::new();
    let display = glium::Display::new(window_builder, context_builder, &event_loop)
        .expect("Failed to create display");

    // 计算保持宽高比的顶点坐标
    fn calculate_display_vertices(window_width: u32, window_height: u32, video_width: u32, video_height: u32) -> Vec<Vertex> {
        // 计算视频宽高比
        let video_aspect = video_width as f32 / video_height as f32;
        let window_aspect = window_width as f32 / window_height as f32;

        // 计算实际显示尺寸，保持宽高比
        let (display_width, display_height) = if window_aspect > video_aspect {
            // 窗口较宽，以高度为基准
            let height = 2.0;
            let width = height * video_aspect;
            (width, height)
        } else {
            // 窗口较高，以宽度为基准
            let width = 2.0;
            let height = width / video_aspect;
            (width, height)
        };

        // 计算显示位置，使视频居中
        let x_offset = -display_width / 2.0;
        let y_offset = -display_height / 2.0;

        vec![
            Vertex {
                position: [x_offset, y_offset],
                tex_coords: [0.0, 1.0],
            },
            Vertex {
                position: [x_offset + display_width, y_offset],
                tex_coords: [1.0, 1.0],
            },
            Vertex {
                position: [x_offset + display_width, y_offset + display_height],
                tex_coords: [1.0, 0.0],
            },
            Vertex {
                position: [x_offset, y_offset + display_height],
                tex_coords: [0.0, 0.0],
            },
        ]
    }

    // 接收第一帧以获取视频尺寸
    let first_frame = frame_receiver.recv().expect("Failed to receive first frame");
    let frame_width = first_frame.width();
    let frame_height = first_frame.height();
    println!("Video dimensions: {}x{}", frame_width, frame_height);

    // 创建顶点缓冲
    let mut vertex_buffer = {
        let vertices = calculate_display_vertices(
            800,
            600,
            frame_width as u32,
            frame_height as u32,
        );
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

    // 创建纹理
    let mut y_texture: Option<Texture2d> = Some(Texture2d::empty_with_format(
        &display,
        UncompressedFloatFormat::U8,
        MipmapsOption::NoMipmap,
        frame_width as u32,
        frame_height as u32,
    ).unwrap());

    let mut u_texture: Option<Texture2d> = Some(Texture2d::empty_with_format(
        &display,
        UncompressedFloatFormat::U8,
        MipmapsOption::NoMipmap,
        frame_width as u32 / 2,
        frame_height as u32 / 2,
    ).unwrap());

    let mut v_texture: Option<Texture2d> = Some(Texture2d::empty_with_format(
        &display,
        UncompressedFloatFormat::U8,
        MipmapsOption::NoMipmap,
        frame_width as u32 / 2,
        frame_height as u32 / 2,
    ).unwrap());

    // 处理第一帧
    if let (Some(ref mut y), Some(ref mut u), Some(ref mut v)) = 
       (y_texture.as_mut(), u_texture.as_mut(), v_texture.as_mut()) {
        update_yuv_textures(&first_frame, y, u, v, frame_width as u32, frame_height as u32);
    }

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
                event: WindowEvent::Resized(physical_size),
                ..
            } => {
                // 处理窗口大小变化
                println!("窗口大小变化: {}x{}", physical_size.width, physical_size.height);
                
                // 更新顶点缓冲以保持正确的宽高比
                let vertices = calculate_display_vertices(
                    physical_size.width,
                    physical_size.height,
                    frame_width as u32,
                    frame_height as u32
                );
                vertex_buffer = glium::VertexBuffer::new(&display, &vertices)
                    .expect("Failed to create vertex buffer");

                // 通知显示系统窗口大小已更改
                display.gl_window().window().request_redraw();
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
                        
                        // 更新纹理
                        if let (Some(ref mut y), Some(ref mut u), Some(ref mut v)) = 
                           (y_texture.as_mut(), u_texture.as_mut(), v_texture.as_mut()) {
                            update_yuv_textures(&new_frame, y, u, v, frame_width as u32, frame_height as u32);
                        }

                        // 渲染
                        let mut target = display.draw();
                        target.clear_color(0.0, 0.0, 0.0, 1.0);

                        if let (Some(ref y), Some(ref u), Some(ref v)) = 
                           (y_texture.as_ref(), u_texture.as_ref(), v_texture.as_ref()) {
                            let uniforms = uniform! {
                                y_tex: y.sampled().magnify_filter(MagnifySamplerFilter::Linear),
                                u_tex: u.sampled().magnify_filter(MagnifySamplerFilter::Linear),
                                v_tex: v.sampled().magnify_filter(MagnifySamplerFilter::Linear),
                            };

                            target
                                .draw(
                                    &vertex_buffer,
                                    &index_buffer,
                                    &program,
                                    &uniforms,
                                    &Default::default(),
                                )
                                .unwrap();
                        }

                        target.finish().unwrap();
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        // 没有新帧时，继续显示上一帧
                        if let (Some(ref y), Some(ref u), Some(ref v)) = 
                           (y_texture.as_ref(), u_texture.as_ref(), v_texture.as_ref()) {
                            let mut target = display.draw();
                            target.clear_color(0.0, 0.0, 0.0, 1.0);

                            let uniforms = uniform! {
                                y_tex: y.sampled().magnify_filter(MagnifySamplerFilter::Linear),
                                u_tex: u.sampled().magnify_filter(MagnifySamplerFilter::Linear),
                                v_tex: v.sampled().magnify_filter(MagnifySamplerFilter::Linear),
                            };

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
                        }
                    }
                    Err(_) => {
                        *control_flow = ControlFlow::Exit;
                    }
                }

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
            _ => (),
        }
    });
}

fn rescaler_for_frame(frame: &Video) -> Video {
    println!("开始处理帧");
    println!("输入帧信息:");
    println!("  格式: {:?}", frame.format());
    println!("  尺寸: {}x{}", frame.width(), frame.height());
    println!("  Y平面数据大小: {} 字节", frame.data(0).len());
    println!("  U平面数据大小: {} 字节", frame.data(1).len());
    println!("  V平面数据大小: {} 字节", frame.data(2).len());

    // 只转换格式，保持原始尺寸
    let mut context = ffmpeg_next::software::scaling::Context::get(
        frame.format(),
        frame.width(),
        frame.height(),
        Pixel::YUV420P,
        frame.width(),
        frame.height(),
        ffmpeg::software::scaling::Flags::BILINEAR,
    )
    .unwrap();

    let mut new_frame = Video::empty();
    context.run(&frame, &mut new_frame).unwrap();

    println!("输出帧信息:");
    println!("  格式: {:?}", new_frame.format());
    println!("  尺寸: {}x{}", new_frame.width(), new_frame.height());
    println!("  Y平面数据大小: {} 字节", new_frame.data(0).len());
    println!("  U平面数据大小: {} 字节", new_frame.data(1).len());
    println!("  V平面数据大小: {} 字节", new_frame.data(2).len());
    println!("帧处理完成");

    new_frame
}

fn update_yuv_textures(frame: &Video, y_texture: &mut Texture2d, u_texture: &mut Texture2d, v_texture: &mut Texture2d, width: u32, height: u32) {
    let y_data = frame.data(0);
    let u_data = frame.data(1);
    let v_data = frame.data(2);

    let y_stride = frame.stride(0);
    let u_stride = frame.stride(1);
    let v_stride = frame.stride(2);

    println!("Y plane: size={}, stride={}", y_data.len(), y_stride);
    println!("U plane: size={}, stride={}", u_data.len(), u_stride);
    println!("V plane: size={}, stride={}", v_data.len(), v_stride);

    // 创建正确大小的数据缓冲区
    let mut y_buffer = vec![0u8; (width * height) as usize];
    let mut u_buffer = vec![0u8; (width * height / 4) as usize];
    let mut v_buffer = vec![0u8; (width * height / 4) as usize];

    // 复制Y平面数据，考虑stride
    for y in 0..height as usize {
        let src_start = y * y_stride;
        let dst_start = y * width as usize;
        let src_end = src_start + width as usize;
        let dst_end = dst_start + width as usize;
        y_buffer[dst_start..dst_end].copy_from_slice(&y_data[src_start..src_end]);
    }

    // 复制U平面数据，考虑stride
    let uv_height = height / 2;
    let uv_width = width / 2;
    for y in 0..uv_height as usize {
        let src_start = y * u_stride;
        let dst_start = y * uv_width as usize;
        let src_end = src_start + uv_width as usize;
        let dst_end = dst_start + uv_width as usize;
        u_buffer[dst_start..dst_end].copy_from_slice(&u_data[src_start..src_end]);
    }

    // 复制V平面数据，考虑stride
    for y in 0..uv_height as usize {
        let src_start = y * v_stride;
        let dst_start = y * uv_width as usize;
        let src_end = src_start + uv_width as usize;
        let dst_end = dst_start + uv_width as usize;
        v_buffer[dst_start..dst_end].copy_from_slice(&v_data[src_start..src_end]);
    }

    // 更新Y纹理
    y_texture.write(
        Rect {
            left: 0,
            bottom: 0,
            width,
            height,
        },
        RawImage2d {
            data: Cow::Borrowed(&y_buffer),
            width,
            height,
            format: ClientFormat::U8,
        },
    );

    // 更新U纹理
    u_texture.write(
        Rect {
            left: 0,
            bottom: 0,
            width: uv_width,
            height: uv_height,
        },
        RawImage2d {
            data: Cow::Borrowed(&u_buffer),
            width: uv_width,
            height: uv_height,
            format: ClientFormat::U8,
        },
    );

    // 更新V纹理
    v_texture.write(
        Rect {
            left: 0,
            bottom: 0,
            width: uv_width,
            height: uv_height,
        },
        RawImage2d {
            data: Cow::Borrowed(&v_buffer),
            width: uv_width,
            height: uv_height,
            format: ClientFormat::U8,
        },
    );
}