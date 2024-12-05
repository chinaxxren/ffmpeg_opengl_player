use glium::{
    glutin::{dpi::PhysicalSize, event_loop::EventLoop, window::WindowBuilder, ContextBuilder},
    implement_vertex,
    index::PrimitiveType,
    texture::{ClientFormat, MipmapsOption, RawImage2d, UncompressedFloatFormat},
    uniform, Display, IndexBuffer, Program, Rect, Surface, Texture2d, VertexBuffer,
};
use tracing::info;

use crate::config::Config;
use ffmpeg_next::util::frame::Video as VideoFrame;
use rayon::prelude::*;
use std::borrow::Cow;

#[derive(Copy, Clone, Debug)]
pub struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

implement_vertex!(Vertex, position, tex_coords);

#[derive(Copy, Clone, Debug)]
pub enum ScaleMode {
    Fit,  // 保持原始比例,两侧或者上下留黑
    Fill, // 完全按原比例显示，，进行裁剪，画面全屏显示
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
        let y_size = (width * height) as usize;
        let uv_size = ((width / 2) * (height / 2)) as usize;

        Self {
            y_buffer: vec![0; y_size],
            u_buffer: vec![0; uv_size],
            v_buffer: vec![0; uv_size],
            width,
            height,
        }
    }

    fn ensure_capacity(&mut self, width: u32, height: u32) {
        let y_size = (width * height) as usize;
        let uv_size = ((width / 2) * (height / 2)) as usize;

        if self.y_buffer.len() != y_size {
            self.y_buffer.resize(y_size, 0);
        }
        if self.u_buffer.len() != uv_size {
            self.u_buffer.resize(uv_size, 0);
        }
        if self.v_buffer.len() != uv_size {
            self.v_buffer.resize(uv_size, 0);
        }

        self.width = width;
        self.height = height;
    }

    fn copy_from_frame(&mut self, frame: &VideoFrame) {
        let width = frame.width() as u32;
        let height = frame.height() as u32;
        self.ensure_capacity(width, height);

        let y_data = frame.data(0);
        let u_data = frame.data(1);
        let v_data = frame.data(2);

        if y_data.is_empty() || u_data.is_empty() || v_data.is_empty() {
            info!("[YuvBuffer] Warning: Missing YUV data");
            return;
        }

        self.y_buffer
            .par_chunks_mut(width as usize)
            .enumerate()
            .for_each(|(i, row)| {
                let src_offset = i * frame.stride(0);
                if src_offset + width as usize <= y_data.len() {
                    row.copy_from_slice(&y_data[src_offset..src_offset + width as usize]);
                }
            });

        let uv_width = width / 2;
        rayon::join(
            || {
                self.u_buffer
                    .par_chunks_mut(uv_width as usize)
                    .enumerate()
                    .for_each(|(i, row)| {
                        let src_offset = i * frame.stride(1);
                        if src_offset + uv_width as usize <= u_data.len() {
                            row.copy_from_slice(
                                &u_data[src_offset..src_offset + uv_width as usize],
                            );
                        }
                    });
            },
            || {
                self.v_buffer
                    .par_chunks_mut(uv_width as usize)
                    .enumerate()
                    .for_each(|(i, row)| {
                        let src_offset = i * frame.stride(2);
                        if src_offset + uv_width as usize <= v_data.len() {
                            row.copy_from_slice(
                                &v_data[src_offset..src_offset + uv_width as usize],
                            );
                        }
                    });
            },
        );
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
    pub fn new(
        event_loop: &EventLoop<()>,
        config: &Config,
        frame_width: u32,
        frame_height: u32,
    ) -> Self {
        info!(
            "[Renderer] 创建窗口，配置尺寸: {}x{}",
            config.window_width, config.window_height
        );

        let scale_factor = event_loop.primary_monitor().unwrap().scale_factor();
        info!("[Renderer] 系统缩放因子: {}", scale_factor);

        let physical_width = (config.window_width as f64 * scale_factor) as u32;
        let physical_height = (config.window_height as f64 * scale_factor) as u32;

        let window_builder = WindowBuilder::new()
            .with_title(&config.window_title)
            .with_inner_size(PhysicalSize::new(physical_width, physical_height))
            .with_resizable(true);

        let context_builder = ContextBuilder::new()
            .with_vsync(true)
            .with_multisampling(0)
            .with_double_buffer(Some(true));

        let display = Display::new(window_builder, context_builder, event_loop)
            .expect("Failed to create display");

        let actual_scale_factor = display.gl_window().window().scale_factor();
        info!("[Renderer] 实际显示器缩放因子: {}", actual_scale_factor);

        let vertex_shader_src = include_str!("shaders/vertex_shader.glsl");
        let fragment_shader_src = include_str!("shaders/fragment_shader.glsl");

        let program = Program::from_source(&display, vertex_shader_src, fragment_shader_src, None)
            .expect("Failed to create shader program");

        let vertex_buffer = VertexBuffer::new(
            &display,
            &[
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
            ],
        )
        .expect("Failed to create vertex buffer");

        let index_buffer = IndexBuffer::new(
            &display,
            PrimitiveType::TrianglesList,
            &[0u16, 1, 2, 0, 2, 3],
        )
        .expect("Failed to create index buffer");

        let front_buffer = YuvBuffer::new(frame_width, frame_height);
        let back_buffer = YuvBuffer::new(frame_width, frame_height);

        let mut renderer = Self {
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
        };

        renderer.update_vertex_buffer();

        renderer
    }

    pub fn toggle_scale_mode(&mut self) {
        self.scale_mode = match self.scale_mode {
            ScaleMode::Fit => ScaleMode::Fill,
            ScaleMode::Fill => ScaleMode::Fit,
        };
        info!("切换到缩放模式: {:?}", self.scale_mode);
        self.update_vertex_buffer();
    }

    pub fn handle_resize(&mut self, new_size: PhysicalSize<u32>) {
        info!(
            "[Renderer] 处理窗口调整大小: {}x{}",
            new_size.width, new_size.height
        );

        let scale_factor = {
            let gl_window = self.display.gl_window();
            gl_window.window().scale_factor()
        };

        info!("[Renderer] 当前缩放因子: {}", scale_factor);

        let logical_size = new_size.to_logical::<f64>(scale_factor);
        info!(
            "[Renderer] 逻辑尺寸: {}x{}",
            logical_size.width, logical_size.height
        );

        if new_size.width == 0
            || new_size.height == 0
            || new_size.width == u32::MAX
            || new_size.height == u32::MAX
        {
            info!("[Renderer] 忽略无效的窗口尺寸");
            return;
        }

        self.update_vertex_buffer();
    }

    pub fn update_vertex_buffer(&mut self) {
        let (physical_size, scale_factor) = {
            let gl_window = self.display.gl_window();
            let window = gl_window.window();
            (window.inner_size(), window.scale_factor())
        };

        let logical_size = physical_size.to_logical::<f64>(scale_factor);

        info!("[Renderer] 更新顶点缓冲区");
        info!(
            "[Renderer] 物理尺寸: {}x{}",
            physical_size.width, physical_size.height
        );
        info!(
            "[Renderer] 逻辑尺寸: {}x{}",
            logical_size.width, logical_size.height
        );
        info!("[Renderer] 缩放因子: {}", scale_factor);

        let vertices = Self::calculate_display_vertices(
            physical_size.width,
            physical_size.height,
            self.frame_width,
            self.frame_height,
            self.scale_mode,
        );

        self.vertex_buffer =
            VertexBuffer::new(&self.display, &vertices).expect("Failed to create vertex buffer");
    }

    pub fn render_frame(&mut self, frame: &VideoFrame) {
        let width = frame.width() as u32;
        let height = frame.height() as u32;

        if self.frame_width != width || self.frame_height != height {
            info!(
                "[Renderer] 帧大小改变: {}x{} -> {}x{}",
                self.frame_width, self.frame_height, width, height
            );
            self.frame_width = width;
            self.frame_height = height;
            self.update_vertex_buffer();
        }

        self.back_buffer.copy_from_frame(frame);

        info!(
            "[Renderer] Frame info - width: {}, height: {}, format: {:?}",
            width,
            height,
            frame.format()
        );
        info!(
            "[Renderer] Buffer sizes - Y: {}, U: {}, V: {}",
            self.back_buffer.y_buffer.len(),
            self.back_buffer.u_buffer.len(),
            self.back_buffer.v_buffer.len()
        );

        if self.y_texture.is_none() {
            info!("[Renderer] Creating Y texture: {}x{}", width, height);
            self.y_texture = Some(
                Texture2d::empty_with_format(
                    &self.display,
                    UncompressedFloatFormat::U8,
                    MipmapsOption::NoMipmap,
                    width,
                    height,
                )
                .unwrap(),
            );
        }

        if self.u_texture.is_none() {
            info!(
                "[Renderer] Creating U texture: {}x{}",
                width / 2,
                height / 2
            );
            self.u_texture = Some(
                Texture2d::empty_with_format(
                    &self.display,
                    UncompressedFloatFormat::U8,
                    MipmapsOption::NoMipmap,
                    width / 2,
                    height / 2,
                )
                .unwrap(),
            );
        }

        if self.v_texture.is_none() {
            info!(
                "[Renderer] Creating V texture: {}x{}",
                width / 2,
                height / 2
            );
            self.v_texture = Some(
                Texture2d::empty_with_format(
                    &self.display,
                    UncompressedFloatFormat::U8,
                    MipmapsOption::NoMipmap,
                    width / 2,
                    height / 2,
                )
                .unwrap(),
            );
        }

        if self.back_buffer.y_buffer.len() != (width * height) as usize
            || self.back_buffer.u_buffer.len() != ((width / 2) * (height / 2)) as usize
            || self.back_buffer.v_buffer.len() != ((width / 2) * (height / 2)) as usize
        {
            info!("[Renderer] Warning: Buffer size mismatch");
            info!(
                "[Renderer] Expected - Y: {}, U/V: {}",
                width * height,
                (width / 2) * (height / 2)
            );
            return;
        }

        if let Some(ref texture) = self.y_texture {
            texture.write(
                Rect {
                    left: 0,
                    bottom: 0,
                    width,
                    height,
                },
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
                Rect {
                    left: 0,
                    bottom: 0,
                    width: width / 2,
                    height: height / 2,
                },
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
                Rect {
                    left: 0,
                    bottom: 0,
                    width: width / 2,
                    height: height / 2,
                },
                RawImage2d {
                    data: Cow::Borrowed(&self.back_buffer.v_buffer),
                    width: width / 2,
                    height: height / 2,
                    format: ClientFormat::U8,
                },
            );
        }

        let mut target = self.display.draw();
        target.clear_color(0.0, 0.0, 0.0, 1.0);

        let uniforms = uniform! {
            y_tex: self.y_texture.as_ref().unwrap(),
            u_tex: self.u_texture.as_ref().unwrap(),
            v_tex: self.v_texture.as_ref().unwrap(),
        };

        target
            .draw(
                &self.vertex_buffer,
                &self.index_buffer,
                &self.program,
                &uniforms,
                &Default::default(),
            )
            .unwrap();

        target.finish().unwrap();

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

        info!("[Renderer] 计算显示顶点");
        info!(
            "[Renderer] 窗口尺寸: {}x{} (比例: {:.3})",
            window_width, window_height, window_aspect
        );
        info!(
            "[Renderer] 视频尺寸: {}x{} (比例: {:.3})",
            video_width, video_height, video_aspect
        );
        info!("[Renderer] 缩放模式: {:?}", mode);

        let (scale_x, scale_y) = match mode {
            ScaleMode::Fit => {
                if window_aspect > video_aspect {
                    (video_aspect / window_aspect, 1.0)
                } else {
                    (1.0, window_aspect / video_aspect)
                }
            }
            ScaleMode::Fill => {
                if window_aspect > video_aspect {
                    (1.0, window_aspect / video_aspect)
                } else {
                    (video_aspect / window_aspect, 1.0)
                }
            }
        };

        info!("[Renderer] 缩放比例: ({:.3}, {:.3})", scale_x, scale_y);
        info!(
            "[Renderer] 最终显示尺寸: {:.3} x {:.3}",
            2.0 * scale_x,
            2.0 * scale_y
        );

        vec![
            Vertex {
                position: [-scale_x, -scale_y],
                tex_coords: [0.0, 1.0],
            },
            Vertex {
                position: [scale_x, -scale_y],
                tex_coords: [1.0, 1.0],
            },
            Vertex {
                position: [scale_x, scale_y],
                tex_coords: [1.0, 0.0],
            },
            Vertex {
                position: [-scale_x, scale_y],
                tex_coords: [0.0, 0.0],
            },
        ]
    }
}
