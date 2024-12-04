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
    Fit,     //按原视频比例显示，是竖屏的就显示出竖屏的，两边留黑
    Fill,    //按照原比例拉伸占满整个播放器，但视频内容超出部分会被剪切
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
        println!("[Renderer] 创建窗口，配置尺寸: {}x{}", config.window_width, config.window_height);
        
        let window_builder = WindowBuilder::new()
            .with_title(&config.window_title)
            .with_inner_size(PhysicalSize::new(config.window_width, config.window_height));

        let context_builder = ContextBuilder::new();
        let display = Display::new(window_builder, context_builder, event_loop)
            .expect("Failed to create display");

        let window_size = display.gl_window().window().inner_size();
        println!("[Renderer] 窗口实际尺寸: {}x{}", window_size.width, window_size.height);

        let vertex_shader_src = include_str!("vertex_shader.glsl");
        let fragment_shader_src = include_str!("fragment_shader.glsl");

        let program = Program::from_source(&display, vertex_shader_src, fragment_shader_src, None)
            .expect("Failed to create shader program");

        let vertices = Self::calculate_display_vertices(
            config.window_width,
            config.window_height,
            frame_width,
            frame_height,
            config.scale_mode,
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
            scale_mode: config.scale_mode,
            frame_width,
            frame_height,
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

        // 检查帧大小是否改变
        if width != self.frame_width || height != self.frame_height {
            println!("[Renderer] 帧大小改变: {}x{} -> {}x{}", self.frame_width, self.frame_height, width, height);
            
            self.frame_width = width;
            self.frame_height = height;

            // 重新创建纹理和顶点缓冲
            self.y_texture = None;
            self.u_texture = None;
            self.v_texture = None;
            self.update_vertex_buffer();
        }

        // 创建或更新纹理
        if self.y_texture.is_none() {
            println!("[Renderer] 创建Y纹理 - {}x{}", width, height);
            self.y_texture = Some(Texture2d::empty_with_format(
                &self.display,
                UncompressedFloatFormat::U8,
                MipmapsOption::NoMipmap,
                width,
                height,
            ).unwrap());
        }

        if self.u_texture.is_none() {
            println!("[Renderer] 创建U纹理 - {}x{}", width/2, height/2);
            self.u_texture = Some(Texture2d::empty_with_format(
                &self.display,
                UncompressedFloatFormat::U8,
                MipmapsOption::NoMipmap,
                width / 2,
                height / 2,
            ).unwrap());
        }

        if self.v_texture.is_none() {
            println!("[Renderer] 创建V纹理 - {}x{}", width/2, height/2);
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

        // 获取YUV数据和步长
        let y_data = frame.data(0);
        let u_data = frame.data(1);
        let v_data = frame.data(2);

        let y_stride = frame.stride(0);
        let u_stride = frame.stride(1);
        let v_stride = frame.stride(2);

        println!("[Renderer] 帧信息:");
        println!("  尺寸: {}x{}", width, height);
        println!("  步长 - Y: {}, U: {}, V: {}", y_stride, u_stride, v_stride);

        // 创建对齐的缓冲区
        let mut y_buffer = vec![0u8; (width * height) as usize];
        let mut u_buffer = vec![0u8; ((width/2) * (height/2)) as usize];
        let mut v_buffer = vec![0u8; ((width/2) * (height/2)) as usize];

        // 按行复制Y平面数据，处理步长对齐
        for y in 0..height as usize {
            let src_start = y * y_stride;
            let dst_start = y * width as usize;
            y_buffer[dst_start..dst_start + width as usize]
                .copy_from_slice(&y_data[src_start..src_start + width as usize]);
        }

        // 按行复制U平面数据，处理步长对齐
        for y in 0..(height/2) as usize {
            let src_start = y * u_stride;
            let dst_start = y * (width/2) as usize;
            u_buffer[dst_start..dst_start + (width/2) as usize]
                .copy_from_slice(&u_data[src_start..src_start + (width/2) as usize]);
        }

        // 按行复制V平面数据，处理步长对齐
        for y in 0..(height/2) as usize {
            let src_start = y * v_stride;
            let dst_start = y * (width/2) as usize;
            v_buffer[dst_start..dst_start + (width/2) as usize]
                .copy_from_slice(&v_data[src_start..src_start + (width/2) as usize]);
        }

        // 更新纹理
        y.write(
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

        u.write(
            Rect {
                left: 0,
                bottom: 0,
                width: width / 2,
                height: height / 2,
            },
            RawImage2d {
                data: Cow::Borrowed(&u_buffer),
                width: width / 2,
                height: height / 2,
                format: ClientFormat::U8,
            },
        );

        v.write(
            Rect {
                left: 0,
                bottom: 0,
                width: width / 2,
                height: height / 2,
            },
            RawImage2d {
                data: Cow::Borrowed(&v_buffer),
                width: width / 2,
                height: height / 2,
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
