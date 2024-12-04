use glium::{
    implement_vertex, uniform,
    glutin::{
        dpi::PhysicalSize,
        event_loop::EventLoop,
        window::WindowBuilder,
        ContextBuilder,
    },
    Display, Program, Surface, Texture2d, VertexBuffer, IndexBuffer,
    texture::{UncompressedFloatFormat, MipmapsOption, ClientFormat, RawImage2d},
    index::PrimitiveType,
    Rect,
};

use ffmpeg_next::util::frame::Video as VideoFrame;
use crate::config::Config;
use std::borrow::Cow;
use rayon::prelude::*;

#[derive(Copy, Clone, Debug)]
pub struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

implement_vertex!(Vertex, position, tex_coords);

#[derive(Copy, Clone, Debug)]
pub enum ScaleMode {
    Fit,// 保持原始比例,两侧或者上下留黑
    Fill,// 完全按原比例显示，，进行裁剪，画面全屏显示
}

struct YuvBuffer {
    y_buffer: Vec<u8>,
    u_buffer: Vec<u8>,
    v_buffer: Vec<u8>,
    width: u32,
    height: u32,
}

impl YuvBuffer {
    fn new(width: u32, height: u32) -> Self {
        Self {
            y_buffer: vec![0; (width * height) as usize],
            u_buffer: vec![0; ((width/2) * (height/2)) as usize],
            v_buffer: vec![0; ((width/2) * (height/2)) as usize],
            width,
            height,
        }
    }

    fn ensure_capacity(&mut self, width: u32, height: u32) {
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.y_buffer.resize((width * height) as usize, 0);
            self.u_buffer.resize(((width/2) * (height/2)) as usize, 0);
            self.v_buffer.resize(((width/2) * (height/2)) as usize, 0);
        }
    }

    fn copy_from_frame(&mut self, frame: &VideoFrame) {
        let width = frame.width() as u32;
        let height = frame.height() as u32;
        self.ensure_capacity(width, height);

        let y_data = frame.data(0);
        let u_data = frame.data(1);
        let v_data = frame.data(2);

        if y_data.is_empty() || u_data.is_empty() || v_data.is_empty() {
            println!("[YuvBuffer] Warning: Missing YUV data");
            return;
        }

        // Copy Y plane data
        self.y_buffer.par_chunks_mut(width as usize)
            .enumerate()
            .for_each(|(i, row)| {
                let src_offset = i * frame.stride(0);
                if src_offset + width as usize <= y_data.len() {
                    row.copy_from_slice(&y_data[src_offset..src_offset + width as usize]);
                }
            });

        // Copy U plane data
        let uv_width = width / 2;
        self.u_buffer.par_chunks_mut(uv_width as usize)
            .enumerate()
            .for_each(|(i, row)| {
                let src_offset = i * frame.stride(1);
                if src_offset + uv_width as usize <= u_data.len() {
                    row.copy_from_slice(&u_data[src_offset..src_offset + uv_width as usize]);
                }
            });

        // Copy V plane data
        self.v_buffer.par_chunks_mut(uv_width as usize)
            .enumerate()
            .for_each(|(i, row)| {
                let src_offset = i * frame.stride(2);
                if src_offset + uv_width as usize <= v_data.len() {
                    row.copy_from_slice(&v_data[src_offset..src_offset + uv_width as usize]);
                }
            });
    }
}

pub struct Renderer {
    display: Display,
    program: Program,
    vertex_buffer: VertexBuffer<Vertex>,
    index_buffer: IndexBuffer<u16>,
    y_texture: Option<Texture2d>,
    u_texture: Option<Texture2d>,
    v_texture: Option<Texture2d>,
    scale_mode: ScaleMode,
    frame_width: u32,
    frame_height: u32,
    front_buffer: YuvBuffer,
    back_buffer: YuvBuffer,
}

impl Renderer {
    pub fn new(event_loop: &EventLoop<()>, config: &Config, frame_width: u32, frame_height: u32) -> Self {
        println!("[Renderer] 创建窗口，配置尺寸: {}x{}", config.window_width, config.window_height);
        
        let window_builder = WindowBuilder::new()
            .with_title(&config.window_title)
            .with_inner_size(PhysicalSize::new(config.window_width, config.window_height));

        let context_builder = ContextBuilder::new()
            .with_vsync(true);

        let display = Display::new(window_builder, context_builder, event_loop)
            .expect("Failed to create display");

        let vertex_shader_src = include_str!("shaders/vertex_shader.glsl");
        let fragment_shader_src = include_str!("shaders/fragment_shader.glsl");

        let program = Program::from_source(&display, vertex_shader_src, fragment_shader_src, None)
            .expect("Failed to create shader program");

        let vertex_buffer = VertexBuffer::new(
            &display,
            &[
                Vertex { position: [-1.0, -1.0], tex_coords: [0.0, 1.0] },
                Vertex { position: [ 1.0, -1.0], tex_coords: [1.0, 1.0] },
                Vertex { position: [ 1.0,  1.0], tex_coords: [1.0, 0.0] },
                Vertex { position: [-1.0,  1.0], tex_coords: [0.0, 0.0] },
            ],
        ).expect("Failed to create vertex buffer");

        let index_buffer = IndexBuffer::new(
            &display,
            PrimitiveType::TrianglesList,
            &[0u16, 1, 2, 0, 2, 3],
        ).expect("Failed to create index buffer");

        let front_buffer = YuvBuffer::new(frame_width, frame_height);
        let back_buffer = YuvBuffer::new(frame_width, frame_height);

        Self {
            display,
            program,
            vertex_buffer,
            index_buffer,
            y_texture: None,
            u_texture: None,
            v_texture: None,
            scale_mode: config.scale_mode,
            frame_width,
            frame_height,
            front_buffer,
            back_buffer,
        }
    }

    pub fn toggle_scale_mode(&mut self) {
        self.scale_mode = match self.scale_mode {
            ScaleMode::Fit => ScaleMode::Fill,
            ScaleMode::Fill => ScaleMode::Fit,
        };
        println!("切换到缩放模式: {:?}", self.scale_mode);
        self.update_vertex_buffer();
    }

    pub fn handle_resize(&mut self, new_size: PhysicalSize<u32>) {
        // 检查窗口大小是否有效
        if new_size.width == 0 || new_size.height == 0 || 
           new_size.width == u32::MAX || new_size.height == u32::MAX {
            println!("无效的窗口大小: {}x{}", new_size.width, new_size.height);
            return;
        }
        println!("窗口大小变化: {}x{}", new_size.width, new_size.height);
        self.update_vertex_buffer();
    }

    pub fn update_vertex_buffer(&mut self) {
        let window_size = self.display.gl_window().window().inner_size();
        println!("[Renderer] 更新顶点缓冲区，当前窗口尺寸: {}x{}", window_size.width, window_size.height);
        
        let vertices = Self::calculate_display_vertices(
            window_size.width,
            window_size.height,
            self.frame_width,
            self.frame_height,
            self.scale_mode,
        );

        self.vertex_buffer = VertexBuffer::new(&self.display, &vertices)
            .expect("Failed to create vertex buffer");
    }

    pub fn render_frame(&mut self, frame: &VideoFrame) {
        let width = frame.width() as u32;
        let height = frame.height() as u32;

        // 在后台缓冲区中准备下一帧
        self.back_buffer.copy_from_frame(frame);

        println!("[Renderer] Frame info - width: {}, height: {}, format: {:?}", width, height, frame.format());
        println!("[Renderer] Buffer sizes - Y: {}, U: {}, V: {}", 
            self.back_buffer.y_buffer.len(),
            self.back_buffer.u_buffer.len(),
            self.back_buffer.v_buffer.len()
        );

        // 检查帧大小是否改变
        if self.frame_width != width || self.frame_height != height {
            println!("[Renderer] 帧大小改变: {}x{} -> {}x{}", self.frame_width, self.frame_height, width, height);
            
            self.frame_width = width;
            self.frame_height = height;

            // 更新顶点缓冲区
            let vertices = [
                Vertex { position: [-1.0, -1.0], tex_coords: [0.0, 1.0] },
                Vertex { position: [ 1.0, -1.0], tex_coords: [1.0, 1.0] },
                Vertex { position: [ 1.0,  1.0], tex_coords: [1.0, 0.0] },
                Vertex { position: [-1.0,  1.0], tex_coords: [0.0, 0.0] },
            ];

            self.vertex_buffer = VertexBuffer::new(&self.display, &vertices)
                .expect("Failed to create vertex buffer");

            // 重新创建纹理
            self.y_texture = None;
            self.u_texture = None;
            self.v_texture = None;
        }

        // 创建或更新纹理
        if self.y_texture.is_none() {
            println!("[Renderer] Creating Y texture: {}x{}", width, height);
            self.y_texture = Some(Texture2d::empty_with_format(
                &self.display,
                UncompressedFloatFormat::U8,
                MipmapsOption::NoMipmap,
                width,
                height,
            ).unwrap());
        }

        if self.u_texture.is_none() {
            println!("[Renderer] Creating U texture: {}x{}", width/2, height/2);
            self.u_texture = Some(Texture2d::empty_with_format(
                &self.display,
                UncompressedFloatFormat::U8,
                MipmapsOption::NoMipmap,
                width / 2,
                height / 2,
            ).unwrap());
        }

        if self.v_texture.is_none() {
            println!("[Renderer] Creating V texture: {}x{}", width/2, height/2);
            self.v_texture = Some(Texture2d::empty_with_format(
                &self.display,
                UncompressedFloatFormat::U8,
                MipmapsOption::NoMipmap,
                width / 2,
                height / 2,
            ).unwrap());
        }

        // 确保缓冲区大小正确
        if self.back_buffer.y_buffer.len() != (width * height) as usize ||
           self.back_buffer.u_buffer.len() != ((width/2) * (height/2)) as usize ||
           self.back_buffer.v_buffer.len() != ((width/2) * (height/2)) as usize {
            println!("[Renderer] Warning: Buffer size mismatch");
            println!("[Renderer] Expected - Y: {}, U/V: {}", 
                width * height,
                (width/2) * (height/2)
            );
            return;
        }

        // 更新纹理数据
        if let Some(ref texture) = self.y_texture {
            texture.write(
                Rect { left: 0, bottom: 0, width, height },
                RawImage2d {
                    data: Cow::Borrowed(&self.back_buffer.y_buffer),
                    width,
                    height,
                    format: ClientFormat::U8,
                },
            );
        }

        if let Some(ref texture) = self.u_texture {
            texture.write(
                Rect { left: 0, bottom: 0, width: width / 2, height: height / 2 },
                RawImage2d {
                    data: Cow::Borrowed(&self.back_buffer.u_buffer),
                    width: width / 2,
                    height: height / 2,
                    format: ClientFormat::U8,
                },
            );
        }

        if let Some(ref texture) = self.v_texture {
            texture.write(
                Rect { left: 0, bottom: 0, width: width / 2, height: height / 2 },
                RawImage2d {
                    data: Cow::Borrowed(&self.back_buffer.v_buffer),
                    width: width / 2,
                    height: height / 2,
                    format: ClientFormat::U8,
                },
            );
        }

        // 渲染到屏幕
        let mut target = self.display.draw();
        target.clear_color(0.0, 0.0, 0.0, 1.0);

        let uniforms = uniform! {
            y_tex: self.y_texture.as_ref().unwrap(),
            u_tex: self.u_texture.as_ref().unwrap(),
            v_tex: self.v_texture.as_ref().unwrap(),
        };

        target.draw(
            &self.vertex_buffer,
            &self.index_buffer,
            &self.program,
            &uniforms,
            &Default::default(),
        ).unwrap();

        target.finish().unwrap();

        // 交换前后缓冲区
        std::mem::swap(&mut self.front_buffer, &mut self.back_buffer);
    }

    fn calculate_display_vertices(
        window_width: u32,
        window_height: u32,
        video_width: u32,
        video_height: u32,
        mode: ScaleMode,
    ) -> Vec<Vertex> {
        let video_aspect = video_width as f32 / video_height as f32;
        let window_aspect = window_width as f32 / window_height as f32;

        println!("[Renderer] 计算显示顶点 - 视频: {}x{} (比例: {:.3}), 窗口: {}x{} (比例: {:.3}), 模式: {:?}",
            video_width, video_height, video_aspect,
            window_width, window_height, window_aspect,
            mode);

        let (display_width, display_height, tex_coords) = match mode {
            ScaleMode::Fit => {
                // Fit模式：完全按原比例显示，可能两边留黑
                if window_aspect > video_aspect {
                    // 窗口较宽，视频高度撑满，宽度按比例缩放（两侧留黑）
                    let height = 2.0;
                    let width = height * video_aspect;
                    println!("[Renderer] Fit模式 - 保持原始比例 {:.3}, 两侧留黑", video_aspect);
                    (width, height, [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
                } else {
                    // 窗口较高，视频宽度撑满，高度按比例缩放（上下留黑）
                    let width = 2.0;
                    let height = width / video_aspect;
                    println!("[Renderer] Fit模式 - 保持原始比例 {:.3}, 上下留黑", video_aspect);
                    (width, height, [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
                }
            }
            ScaleMode::Fill => {
                // Fill模式：保持原比例占满窗口，超出部分裁剪
                if window_aspect > video_aspect {
                    // 窗口较宽，视频宽度撑满，超出高度裁剪
                    let width = 2.0;
                    let height = width / window_aspect;
                    
                    // 计算需要裁剪的比例
                    let scale = width / (video_aspect * height);
                    let crop = (scale - 1.0) / scale;
                    let offset = crop / 2.0;
                    
                    println!("[Renderer] Fill模式 - 保持原始比例 {:.3}, 上下裁剪 {:.1}%", video_aspect, crop * 100.0);
                    (width, height, [
                        [0.0, 1.0 - offset],
                        [1.0, 1.0 - offset],
                        [1.0, offset],
                        [0.0, offset]
                    ])
                } else {
                    // 窗口较高，视频高度撑满，超出宽度裁剪
                    let height = 2.0;
                    let width = height * window_aspect;
                    
                    // 计算需要裁剪的比例
                    let scale = height * video_aspect / width;
                    let crop = (scale - 1.0) / scale;
                    let offset = crop / 2.0;
                    
                    println!("[Renderer] Fill模式 - 保持原始比例 {:.3}, 两侧裁剪 {:.1}%", video_aspect, crop * 100.0);
                    (width, height, [
                        [offset, 1.0],
                        [1.0 - offset, 1.0],
                        [1.0 - offset, 0.0],
                        [offset, 0.0]
                    ])
                }
            }
        };

        println!("[Renderer] 最终显示尺寸: {:.3} x {:.3}", display_width, display_height);

        // 确保显示在窗口中心
        let x_offset = -display_width / 2.0;
        let y_offset = -display_height / 2.0;

        vec![
            Vertex {
                position: [x_offset, y_offset],
                tex_coords: tex_coords[0],
            },
            Vertex {
                position: [x_offset + display_width, y_offset],
                tex_coords: tex_coords[1],
            },
            Vertex {
                position: [x_offset + display_width, y_offset + display_height],
                tex_coords: tex_coords[2],
            },
            Vertex {
                position: [x_offset, y_offset + display_height],
                tex_coords: tex_coords[3],
            },
        ]
    }
}
