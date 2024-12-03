extern crate ffmpeg_next as ffmpeg;
use ffmpeg::format::Pixel;
use ffmpeg::util::frame::Video;
use glium::glutin::dpi::LogicalSize;
use glium::glutin::event::{ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent};
use glium::glutin::event_loop::{ControlFlow, EventLoop};
use glium::glutin::window::WindowBuilder;
use glium::glutin::ContextBuilder;
use glium::Surface;
use glium::implement_vertex;
use glium::uniform;
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

    let program =
        glium::Program::from_source(&display, VERTEX_SHADER_SRC, FRAGMENT_SHADER_SRC, None)
            .expect("Failed to create shader program");

    let mut frame_count = 0;
    let mut last_fps_update = Instant::now();
    let last_frame_time: Instant = Instant::now();

    // 创建纹理
    let y_tex = glium::texture::Texture2d::empty_with_format(
        &display,
        glium::texture::UncompressedFloatFormat::U8,
        glium::texture::MipmapsOption::NoMipmap,
        SHOW_WIDTH,
        SHOW_HEIGHT,
    )
    .expect("Failed to create Y texture");

    let u_tex = glium::texture::Texture2d::empty_with_format(
        &display,
        glium::texture::UncompressedFloatFormat::U8,
        glium::texture::MipmapsOption::NoMipmap,
        SHOW_WIDTH / 2,
        SHOW_HEIGHT / 2,
    )
    .expect("Failed to create U texture");

    let v_tex = glium::texture::Texture2d::empty_with_format(
        &display,
        glium::texture::UncompressedFloatFormat::U8,
        glium::texture::MipmapsOption::NoMipmap,
        SHOW_WIDTH / 2,
        SHOW_HEIGHT / 2,
    )
    .expect("Failed to create V texture");

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
                        println!("  Original width: {}, height: {}", frame.width(), frame.height());
                        println!("  Rescaled width: {}, height: {}", new_frame.width(), new_frame.height());
                        println!("  Format: {:?}", new_frame.format());
                        
                        // Y plane info
                        let y_raw_data = new_frame.data(0);
                        println!("\nY plane info:");
                        println!("  Raw data length: {}", y_raw_data.len());
                        println!("  Stride: {}", new_frame.stride(0));
                        println!("  Expected size: {} x {} = {}", 
                            new_frame.width(), 
                            new_frame.height(),
                            new_frame.width() * new_frame.height()
                        );

                        // U plane info
                        let u_raw_data = new_frame.data(1);
                        println!("\nU plane info:");
                        println!("  Raw data length: {}", u_raw_data.len());
                        println!("  Stride: {}", new_frame.stride(1));
                        println!("  Expected size: {} x {} = {}", 
                            new_frame.width() / 2, 
                            new_frame.height() / 2,
                            (new_frame.width() / 2) * (new_frame.height() / 2)
                        );

                        // V plane info
                        let v_raw_data = new_frame.data(2);
                        println!("\nV plane info:");
                        println!("  Raw data length: {}", v_raw_data.len());
                        println!("  Stride: {}", new_frame.stride(2));
                        println!("  Expected size: {} x {} = {}", 
                            new_frame.width() / 2, 
                            new_frame.height() / 2,
                            (new_frame.width() / 2) * (new_frame.height() / 2)
                        );

                        // Y plane texture data
                        let y_data = {
                            let mut rgba_data = Vec::with_capacity(new_frame.width() as usize * new_frame.height() as usize * 4);
                            let y_raw_data = new_frame.data(0);
                            let stride = new_frame.stride(0);
                            
                            for y in 0..new_frame.height() {
                                let line_start = y as usize * stride;
                                let line_end = line_start + new_frame.width() as usize;
                                for &pixel in &y_raw_data[line_start..line_end] {
                                    rgba_data.extend_from_slice(&[pixel, pixel, pixel, 255]);
                                }
                            }
                            
                            glium::texture::RawImage2d::from_raw_rgba(
                                rgba_data,
                                (new_frame.width(), new_frame.height()),
                            )
                        };

                        // U plane texture data
                        let u_data = {
                            let mut rgba_data = Vec::with_capacity((new_frame.width() as usize / 2) * (new_frame.height() as usize / 2) * 4);
                            let u_raw_data = new_frame.data(1);
                            let stride = new_frame.stride(1);
                            
                            for y in 0..(new_frame.height() / 2) {
                                let line_start = y as usize * stride;
                                let line_end = line_start + (new_frame.width() as usize / 2);
                                for &pixel in &u_raw_data[line_start..line_end] {
                                    rgba_data.extend_from_slice(&[pixel, pixel, pixel, 255]);
                                }
                            }
                            
                            glium::texture::RawImage2d::from_raw_rgba(
                                rgba_data,
                                (new_frame.width() / 2, new_frame.height() / 2),
                            )
                        };

                        // V plane texture data
                        let v_data = {
                            let mut rgba_data = Vec::with_capacity((new_frame.width() as usize / 2) * (new_frame.height() as usize / 2) * 4);
                            let v_raw_data = new_frame.data(2);
                            let stride = new_frame.stride(2);
                            
                            for y in 0..(new_frame.height() / 2) {
                                let line_start = y as usize * stride;
                                let line_end = line_start + (new_frame.width() as usize / 2);
                                for &pixel in &v_raw_data[line_start..line_end] {
                                    rgba_data.extend_from_slice(&[pixel, pixel, pixel, 255]);
                                }
                            }
                            
                            glium::texture::RawImage2d::from_raw_rgba(
                                rgba_data,
                                (new_frame.width() / 2, new_frame.height() / 2),
                            )
                        };

                        println!("\nTexture info:");
                        println!("  Y texture: width={}, height={}, data_len={}", y_data.width, y_data.height, y_data.data.len());
                        println!("  U texture: width={}, height={}, data_len={}", u_data.width, u_data.height, u_data.data.len());
                        println!("  V texture: width={}, height={}, data_len={}", v_data.width, v_data.height, v_data.data.len());

                        // Update textures with correct rectangle areas
                        let y_rect = glium::Rect {
                            left: 0,
                            bottom: 0,
                            width: new_frame.width(),
                            height: new_frame.height(),
                        };
                        let uv_rect = glium::Rect {
                            left: 0,
                            bottom: 0,
                            width: new_frame.width() / 2,
                            height: new_frame.height() / 2,
                        };

                        y_tex.write(y_rect, y_data);
                        u_tex.write(uv_rect, u_data);
                        v_tex.write(uv_rect, v_data);

                        // Update uniforms
                        let uniforms = uniform! {
                            y_tex: &y_tex,
                            u_tex: &u_tex,
                            v_tex: &v_tex,
                            tex_size: [new_frame.width() as f32, new_frame.height() as f32],
                            y_linesize: [new_frame.stride(0) as f32, 0.0],
                            uv_linesize: [new_frame.stride(1) as f32, 0.0],
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
