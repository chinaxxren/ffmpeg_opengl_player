extern crate ffmpeg_next as ffmpeg;

use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use ffmpeg::frame::Video as AVFrame;
use glium::{implement_vertex, uniform};
use glium::glutin::window::WindowBuilder;
use glium::glutin::event_loop::{ControlFlow, EventLoop};
use glium::glutin::ContextBuilder;
use glium::glutin::dpi::LogicalSize;
use glium::Surface;
use glium::texture::{RawImage2d, MipmapsOption};
use glium::glutin::event::{Event, WindowEvent, VirtualKeyCode, ElementState, KeyboardInput};
use glium::uniforms::{MinifySamplerFilter, MagnifySamplerFilter};

mod player;
mod audio;
mod video;

use crate::player::Player;

#[derive(Copy, Clone)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

implement_vertex!(Vertex, position, tex_coords);

const VERTEX_SHADER_SRC: &str = r#"
    #version 140
    in vec2 position;
    in vec2 tex_coords;
    out vec2 v_tex_coords;
    void main() {
        gl_Position = vec4(position, 0.0, 1.0);
        v_tex_coords = tex_coords;
    }
"#;

const FRAGMENT_SHADER_SRC: &str = r#"
    #version 140

    in vec2 v_tex_coords;
    out vec4 color;

    uniform sampler2D y_tex;
    uniform sampler2D u_tex;
    uniform sampler2D v_tex;
    uniform vec2 tex_size;
    uniform vec2 y_linesize;
    uniform vec2 uv_linesize;

    void main() {
        // 计算实际的纹理坐标
        vec2 y_coords = vec2(v_tex_coords.x * (y_linesize.x / tex_size.x), v_tex_coords.y);
        vec2 uv_coords = vec2(v_tex_coords.x * (uv_linesize.x / tex_size.x), v_tex_coords.y);

        // 从纹理中采样YUV值
        float y = texture(y_tex, y_coords).r;
        float u = texture(u_tex, uv_coords).r - 0.5;
        float v = texture(v_tex, uv_coords).r - 0.5;

        // YUV转RGB
        float r = y + 1.402 * v;
        float g = y - 0.344136 * u - 0.714136 * v;
        float b = y + 1.772 * u;

        color = vec4(r, g, b, 1.0);
    }
"#;

fn main() {
    // 创建带缓冲的通道，避免阻塞
    let (frame_sender, frame_receiver) = mpsc::channel::<AVFrame>();
    
    let path = "/Users/chinaxxren/Desktop/a.mp4";
    println!("开始播放视频: {}", path);

    // 保持对 Player 的引用
    let player = Arc::new(Mutex::new(Player::start(
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
    ).expect("Failed to start player")));

    // 创建事件循环和窗口
    let event_loop = EventLoop::new();
    let window_builder = WindowBuilder::new()
        .with_title("视频播放器")
        .with_inner_size(LogicalSize::new(800.0, 600.0));
    
    let context_builder = ContextBuilder::new();
    let display = glium::Display::new(window_builder, context_builder, &event_loop).expect("Failed to create display");

    // 创建顶点缓冲
    let vertex_buffer = {
        let vertices = vec![
            Vertex { position: [-1.0, -1.0], tex_coords: [0.0, 1.0] },
            Vertex { position: [ 1.0, -1.0], tex_coords: [1.0, 1.0] },
            Vertex { position: [ 1.0,  1.0], tex_coords: [1.0, 0.0] },
            Vertex { position: [-1.0,  1.0], tex_coords: [0.0, 0.0] },
        ];
        glium::VertexBuffer::new(&display, &vertices).expect("Failed to create vertex buffer")
    };

    let index_buffer = glium::IndexBuffer::new(
        &display,
        glium::index::PrimitiveType::TrianglesList,
        &[0u16, 1, 2, 0, 2, 3],
    ).expect("Failed to create index buffer");

    let program = glium::Program::from_source(
        &display,
        VERTEX_SHADER_SRC,
        FRAGMENT_SHADER_SRC,
        None,
    ).expect("Failed to create shader program");

    let mut frame_count = 0;
    let mut last_fps_update = Instant::now();
    let mut last_frame_time: Instant = Instant::now();

    let mut y_tex: Option<glium::texture::Texture2d> = None;
    let mut u_tex: Option<glium::texture::Texture2d> = None;
    let mut v_tex: Option<glium::texture::Texture2d> = None;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                println!("接收到退出事件");
                *control_flow = ControlFlow::Exit;
            },
            Event::WindowEvent { 
                event: WindowEvent::KeyboardInput { 
                    input: KeyboardInput { 
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
            },
            Event::MainEventsCleared => {
                match frame_receiver.try_recv() {
                    Ok(frame) => {
                        frame_count += 1;

                        let (width, height) = (frame.width() as u32, frame.height() as u32);
                        
                        // 获取帧的行大小（stride）
                        let y_stride = frame.stride(0);
                        let u_stride = frame.stride(1);
                        let v_stride = frame.stride(2);
                        
                        // Y plane
                        let y_data = frame.data(0);
                        let mut y_vec = Vec::with_capacity((width * height) as usize);
                        for y in 0..height {
                            let start = y as usize * y_stride;
                            let end = start + width as usize;
                            if start >= y_data.len() || end > y_data.len() {
                                println!("Y plane - 数据越界: start={}, end={}, len={}", start, end, y_data.len());
                                continue;
                            }
                            y_vec.extend_from_slice(&y_data[start..end]);
                        }
                        println!("Y plane - 数据大小: {}, 预期大小: {}, 宽度: {}, 高度: {}, stride: {}", 
                            y_vec.len(), width * height, width, height, y_stride);

                        if y_tex.is_none() {
                            println!("创建Y纹理: {}x{}", width, height);
                            y_tex = Some(glium::texture::Texture2d::new(
                                &display,
                                glium::texture::RawImage2d {
                                    data: std::borrow::Cow::Borrowed(&y_vec),
                                    width: width,
                                    height: height,
                                    format: glium::texture::ClientFormat::U8
                                }
                            ).expect("Failed to create Y texture"));
                        } else {
                            y_tex.as_mut().unwrap().write(
                                glium::Rect { left: 0, bottom: 0, width, height },
                                glium::texture::RawImage2d {
                                    data: std::borrow::Cow::Borrowed(&y_vec),
                                    width: width,
                                    height: height,
                                    format: glium::texture::ClientFormat::U8
                                }
                            );
                        }

                        // U plane
                        let u_width = width / 2;
                        let u_height = height / 2;
                        let u_data = frame.data(1);
                        let mut u_vec = Vec::with_capacity((u_width * u_height) as usize);
                        for y in 0..u_height {
                            let start = y as usize * u_stride;
                            let end = start + u_width as usize;
                            if start >= u_data.len() || end > u_data.len() {
                                println!("U plane - 数据越界: start={}, end={}, len={}", start, end, u_data.len());
                                continue;
                            }
                            u_vec.extend_from_slice(&u_data[start..end]);
                        }
                        println!("U plane - 数据大小: {}, 预期大小: {}, 宽度: {}, 高度: {}, stride: {}", 
                            u_vec.len(), u_width * u_height, u_width, u_height, u_stride);

                        if u_tex.is_none() {
                            println!("创建U纹理: {}x{}", u_width, u_height);
                            u_tex = Some(glium::texture::Texture2d::new(
                                &display,
                                glium::texture::RawImage2d {
                                    data: std::borrow::Cow::Borrowed(&u_vec),
                                    width: u_width,
                                    height: u_height,
                                    format: glium::texture::ClientFormat::U8
                                }
                            ).expect("Failed to create U texture"));
                        } else {
                            u_tex.as_mut().unwrap().write(
                                glium::Rect { left: 0, bottom: 0, width: u_width, height: u_height },
                                glium::texture::RawImage2d {
                                    data: std::borrow::Cow::Borrowed(&u_vec),
                                    width: u_width,
                                    height: u_height,
                                    format: glium::texture::ClientFormat::U8
                                }
                            );
                        }

                        // V plane
                        let v_width = width / 2;
                        let v_height = height / 2;
                        let v_data = frame.data(2);
                        let mut v_vec = Vec::with_capacity((v_width * v_height) as usize);
                        for y in 0..v_height {
                            let start = y as usize * v_stride;
                            let end = start + v_width as usize;
                            if start >= v_data.len() || end > v_data.len() {
                                println!("V plane - 数据越界: start={}, end={}, len={}", start, end, v_data.len());
                                continue;
                            }
                            v_vec.extend_from_slice(&v_data[start..end]);
                        }
                        println!("V plane - 数据大小: {}, 预期大小: {}, 宽度: {}, 高度: {}, stride: {}", 
                            v_vec.len(), v_width * v_height, v_width, v_height, v_stride);

                        if v_tex.is_none() {
                            println!("创建V纹理: {}x{}", v_width, v_height);
                            v_tex = Some(glium::texture::Texture2d::new(
                                &display,
                                glium::texture::RawImage2d {
                                    data: std::borrow::Cow::Borrowed(&v_vec),
                                    width: v_width,
                                    height: v_height,
                                    format: glium::texture::ClientFormat::U8
                                }
                            ).expect("Failed to create V texture"));
                        } else {
                            v_tex.as_mut().unwrap().write(
                                glium::Rect { left: 0, bottom: 0, width: v_width, height: v_height },
                                glium::texture::RawImage2d {
                                    data: std::borrow::Cow::Borrowed(&v_vec),
                                    width: v_width,
                                    height: v_height,
                                    format: glium::texture::ClientFormat::U8
                                }
                            );
                        }

                        // 确保所有纹理都已创建后再进行渲染
                        if let (Some(y), Some(u), Some(v)) = (&y_tex, &u_tex, &v_tex) {
                            let uniforms = uniform! {
                                y_tex: y.sampled()
                                    .minify_filter(MinifySamplerFilter::Linear)
                                    .magnify_filter(MagnifySamplerFilter::Linear),
                                u_tex: u.sampled()
                                    .minify_filter(MinifySamplerFilter::Linear)
                                    .magnify_filter(MagnifySamplerFilter::Linear),
                                v_tex: v.sampled()
                                    .minify_filter(MinifySamplerFilter::Linear)
                                    .magnify_filter(MagnifySamplerFilter::Linear),
                                tex_size: [width as f32, height as f32],
                                y_linesize: [y_stride as f32, height as f32],
                                uv_linesize: [u_stride as f32, (height/2) as f32],
                            };

                            // 渲染
                            let mut target = display.draw();
                            target.clear_color(0.0, 0.0, 0.0, 1.0);

                            target.draw(
                                &vertex_buffer,
                                &index_buffer,
                                &program,
                                &uniforms,
                                &Default::default()
                            ).unwrap();

                            target.finish().unwrap();
                        } else {
                            println!("等待纹理初始化...");
                        }

                        if last_fps_update.elapsed() >= Duration::from_secs(1) {
                            println!("FPS: {}", frame_count);
                            frame_count = 0;
                            last_fps_update = Instant::now();
                        }

                        // 添加帧率控制
                        let frame_duration = Duration::from_secs_f64(1.0 / 60.0); // 60 FPS
                        let elapsed = last_frame_time.elapsed();
                        if elapsed < frame_duration {
                            std::thread::sleep(frame_duration - elapsed);
                        }
                    },
                    Err(mpsc::TryRecvError::Empty) => {
                        // 减少空闲时的 CPU 使用
                        std::thread::sleep(Duration::from_millis(16)); // 约60fps
                    },
                    Err(mpsc::TryRecvError::Disconnected) => {
                        println!("播放器断开连接，退出循环");
                        *control_flow = ControlFlow::Exit;
                    }
                }
            },
            _ => (),
        }
    });
}