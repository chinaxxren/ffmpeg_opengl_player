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

#[derive(Copy, Clone, Debug)]
pub struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

implement_vertex!(Vertex, position, tex_coords);

#[derive(Copy, Clone, Debug)]
pub enum ScaleMode {
    Fit,     // 适应窗口，保持比例，可能有黑边
    Fill,    // 填充窗口，保持比例，可能裁剪
    Stretch, // 拉伸填充，可能变形
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
}

impl Renderer {
    pub fn new(config: &Config, event_loop: &EventLoop<()>, frame_width: u32, frame_height: u32) -> Self {
        let window_builder = WindowBuilder::new()
            .with_title(&config.window_title)
            .with_inner_size(PhysicalSize::new(config.window_width, config.window_height));

        let context_builder = ContextBuilder::new();
        let display = Display::new(window_builder, context_builder, event_loop)
            .expect("Failed to create display");

        let vertex_shader_src = include_str!("vertex_shader.glsl");
        let fragment_shader_src = include_str!("fragment_shader.glsl");

        let program = Program::from_source(&display, vertex_shader_src, fragment_shader_src, None)
            .expect("Failed to create shader program");

        let vertices = Self::calculate_display_vertices(
            config.window_width,
            config.window_height,
            frame_width,
            frame_height,
            ScaleMode::Fill,
        );

        let vertex_buffer = VertexBuffer::new(&display, &vertices)
            .expect("Failed to create vertex buffer");

        let index_buffer = IndexBuffer::new(
            &display,
            PrimitiveType::TrianglesList,
            &[0u16, 1, 2, 0, 2, 3],
        ).expect("Failed to create index buffer");

        let y_texture = None;
        let u_texture = None;
        let v_texture = None;

        Self {
            display,
            program,
            vertex_buffer,
            index_buffer,
            y_texture,
            u_texture,
            v_texture,
            scale_mode: ScaleMode::Fill,
            frame_width,
            frame_height,
        }
    }

    pub fn toggle_scale_mode(&mut self) {
        self.scale_mode = match self.scale_mode {
            ScaleMode::Fit => ScaleMode::Fill,
            ScaleMode::Fill => ScaleMode::Stretch,
            ScaleMode::Stretch => ScaleMode::Fit,
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
        let (width, height) = {
            let window_size = self.display.gl_window().window().inner_size();
            (window_size.width, window_size.height)
        };

        let vertices = Self::calculate_display_vertices(
            width,
            height,
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

        // 检查帧大小是否改变
        if width != self.frame_width || height != self.frame_height {
            println!("帧大小改变: {}x{} -> {}x{}", self.frame_width, self.frame_height, width, height);
            
            // 更新帧大小
            self.frame_width = width;
            self.frame_height = height;

            // 重新创建纹理
            self.y_texture = None;
            self.u_texture = None;
            self.v_texture = None;

            // 重新计算顶点
            self.update_vertex_buffer();
        }

        // 如果纹理不存在，创建新的纹理
        if self.y_texture.is_none() {
            println!("创建Y纹理 - 大小: {}x{}", width, height);
            self.y_texture = Some(Texture2d::empty_with_format(
                &self.display,
                UncompressedFloatFormat::U8,
                MipmapsOption::NoMipmap,
                width,
                height,
            ).unwrap());
        }

        if self.u_texture.is_none() {
            println!("创建U纹理 - 大小: {}x{}", width/2, height/2);
            self.u_texture = Some(Texture2d::empty_with_format(
                &self.display,
                UncompressedFloatFormat::U8,
                MipmapsOption::NoMipmap,
                width / 2,
                height / 2,
            ).unwrap());
        }

        if self.v_texture.is_none() {
            println!("创建V纹理 - 大小: {}x{}", width/2, height/2);
            self.v_texture = Some(Texture2d::empty_with_format(
                &self.display,
                UncompressedFloatFormat::U8,
                MipmapsOption::NoMipmap,
                width / 2,
                height / 2,
            ).unwrap());
        }

        let y = self.y_texture.as_ref().unwrap();
        let u = self.u_texture.as_ref().unwrap();
        let v = self.v_texture.as_ref().unwrap();

        let uv_width = width / 2;
        let uv_height = height / 2;

        let y_data = frame.data(0);
        let u_data = frame.data(1);
        let v_data = frame.data(2);

        // Get line sizes and ensure we handle stride correctly
        let y_stride = frame.stride(0);
        let u_stride = frame.stride(1);
        let v_stride = frame.stride(2);

        println!("Frame info:");
        println!("  Dimensions: {}x{}", width, height);
        println!("  Strides - Y: {}, U: {}, V: {}", y_stride, u_stride, v_stride);

        // Create new buffers with correct sizes
        let mut y_buffer = vec![0u8; (width * height) as usize];
        let mut u_buffer = vec![0u8; (uv_width * uv_height) as usize];
        let mut v_buffer = vec![0u8; (uv_width * uv_height) as usize];

        // Copy Y plane data line by line
        for y in 0..height as usize {
            let src_start = y * y_stride;
            let dst_start = y * width as usize;
            let src_end = src_start + width as usize;
            let dst_end = dst_start + width as usize;
            y_buffer[dst_start..dst_end].copy_from_slice(&y_data[src_start..src_end]);
        }

        // Copy U plane data line by line
        for y in 0..uv_height as usize {
            let src_start = y * u_stride;
            let dst_start = y * uv_width as usize;
            let src_end = src_start + uv_width as usize;
            let dst_end = dst_start + uv_width as usize;
            u_buffer[dst_start..dst_end].copy_from_slice(&u_data[src_start..src_end]);
        }

        // Copy V plane data line by line
        for y in 0..uv_height as usize {
            let src_start = y * v_stride;
            let dst_start = y * uv_width as usize;
            let src_end = src_start + uv_width as usize;
            let dst_end = dst_start + uv_width as usize;
            v_buffer[dst_start..dst_end].copy_from_slice(&v_data[src_start..src_end]);
        }

        // Write Y texture
        y.write(
            Rect {
                left: 0,
                bottom: 0,
                width,
                height,
            },
            RawImage2d {
                data: Cow::Owned(y_buffer),
                width,
                height,
                format: ClientFormat::U8,
            },
        );

        // Write U texture
        u.write(
            Rect {
                left: 0,
                bottom: 0,
                width: uv_width,
                height: uv_height,
            },
            RawImage2d {
                data: Cow::Owned(u_buffer),
                width: uv_width,
                height: uv_height,
                format: ClientFormat::U8,
            },
        );

        // Write V texture
        v.write(
            Rect {
                left: 0,
                bottom: 0,
                width: uv_width,
                height: uv_height,
            },
            RawImage2d {
                data: Cow::Owned(v_buffer),
                width: uv_width,
                height: uv_height,
                format: ClientFormat::U8,
            },
        );

        let mut target = self.display.draw();
        target.clear_color(0.0, 0.0, 0.0, 1.0);

        let uniforms = uniform! {
            y_tex: y.sampled(),
            u_tex: u.sampled(),
            v_tex: v.sampled(),
        };

        target.draw(
            &self.vertex_buffer,
            &self.index_buffer,
            &self.program,
            &uniforms,
            &Default::default(),
        ).unwrap();

        target.finish().unwrap();
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

        let (display_width, display_height, tex_coords) = match mode {
            ScaleMode::Fit => {
                if window_aspect > video_aspect {
                    let height = 2.0;
                    let width = height * video_aspect;
                    (width, height, [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
                } else {
                    let width = 2.0;
                    let height = width / video_aspect;
                    (width, height, [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
                }
            }
            ScaleMode::Fill => {
                if window_aspect > video_aspect {
                    let width = 2.0;
                    let scale = window_aspect / video_aspect;
                    let tex_height = 1.0 / scale;
                    let tex_offset = (1.0 - tex_height) / 2.0;
                    (width, width / window_aspect, [
                        [0.0, 1.0 - tex_offset],
                        [1.0, 1.0 - tex_offset],
                        [1.0, tex_offset],
                        [0.0, tex_offset],
                    ])
                } else {
                    let height = 2.0;
                    let scale = video_aspect / window_aspect;
                    let tex_width = 1.0 / scale;
                    let tex_offset = (1.0 - tex_width) / 2.0;
                    (height * window_aspect, height, [
                        [tex_offset, 1.0],
                        [1.0 - tex_offset, 1.0],
                        [1.0 - tex_offset, 0.0],
                        [tex_offset, 0.0],
                    ])
                }
            }
            ScaleMode::Stretch => {
                (2.0, 2.0, [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
            }
        };

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
