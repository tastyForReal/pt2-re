use bytemuck::{Pod, Zeroable};
use std::path::Path;
use wgpu::util::DeviceExt;

use super::bitmap_font_parser::*;
use crate::game::types::*;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct FontVertex {
    position: [f32; 2],
    tex_coord: [f32; 2],
    color: [f32; 4],
}

/// Renders bitmap font text using a texture atlas.
pub struct BitmapFontRenderer {
    font_data: Option<BitmapFontData>,
    pipeline: Option<wgpu::RenderPipeline>,
    uniform_buffer: Option<wgpu::Buffer>,
    bind_group: Option<wgpu::BindGroup>,
    loaded: bool,
}

impl BitmapFontRenderer {
    pub fn new() -> Self {
        Self {
            font_data: None,
            pipeline: None,
            uniform_buffer: None,
            bind_group: None,
            loaded: false,
        }
    }

    /// Initialize the font renderer with a .fnt file and texture from the assets directory.
    pub fn initialize(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        font_path: &str,
    ) -> bool {
        // Load .fnt file
        let fnt_content = match std::fs::read_to_string(font_path) {
            Ok(c) => c,
            Err(e) => {
                log::error!("Failed to load font file {}: {}", font_path, e);
                return false;
            }
        };
        let font_data = parse_bitmap_font(&fnt_content);

        // Load texture
        let font_dir = Path::new(font_path).parent().unwrap_or(Path::new("."));
        let texture_path = font_dir.join(&font_data.page_file);
        let img = match image::open(&texture_path) {
            Ok(img) => img.to_rgba8(),
            Err(e) => {
                log::error!("Failed to load font texture {:?}: {}", texture_path, e);
                return false;
            }
        };

        let tex_size = wgpu::Extent3d {
            width: img.width(),
            height: img.height(),
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Font Texture"),
            size: tex_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * img.width()),
                rows_per_image: Some(img.height()),
            },
            tex_size,
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let shader_src = include_str!("../../shaders/font.wgsl");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Font Shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Font BGL"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Font Uniforms"),
            // Padded to 16 bytes for GL ES 3.0 compatibility
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &uniform_buffer,
            0,
            bytemuck::bytes_of(&[SCREEN_WIDTH, SCREEN_HEIGHT, 0.0f32, 0.0f32]),
        );

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Font BG"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Font PL"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Font Pipeline"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<FontVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 16,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        self.font_data = Some(font_data);
        self.pipeline = Some(pipeline);
        self.uniform_buffer = Some(uniform_buffer);
        self.bind_group = Some(bind_group);
        self.loaded = true;

        log::info!("Bitmap font loaded from {}", font_path);
        true
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn update_uniforms(&mut self, queue: &wgpu::Queue, width: f32, height: f32) {
        if let Some(ref ub) = self.uniform_buffer {
            let padded: [f32; 4] = [width, height, 0.0, 0.0];
            queue.write_buffer(ub, 0, bytemuck::bytes_of(&padded));
        }
    }

    pub fn begin_frame(&mut self) {}

    pub fn get_line_height(&self, scale: f32) -> f32 {
        if let Some(font_data) = &self.font_data {
            font_data.common.line_height as f32 * scale
        } else {
            0.0
        }
    }

    pub fn get_text_width(&self, text: &str, scale: f32) -> f32 {
        let font_data = match &self.font_data {
            Some(f) => f,
            None => return 0.0,
        };
        let mut width = 0.0;
        for ch in text.chars() {
            if let Some(info) = font_data.chars.get(&(ch as u32)) {
                width += info.x_advance as f32 * scale;
            }
        }
        width
    }

    pub fn calculate_scale_to_fit(
        &self,
        text: &str,
        max_width: f32,
        max_height: f32,
        max_scale: f32,
    ) -> f32 {
        let base_width = self.get_text_width(text, 1.0);
        let base_height = self.get_line_height(1.0);

        let scale_x = if base_width > 0.0 {
            max_width / base_width
        } else {
            max_scale
        };

        let scale_y = if base_height > 0.0 {
            max_height / base_height
        } else {
            max_scale
        };

        scale_x.min(scale_y).min(max_scale)
    }

    /// Render text at the given position.
    pub fn render_text(
        &self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: [f32; 4],
        scroll_offset: f32,
        device: &wgpu::Device,
        render_pass: &mut wgpu::RenderPass<'_>,
        anchor_x: f32,
        anchor_y: f32,
    ) {
        let font_data = match &self.font_data {
            Some(f) => f,
            None => return,
        };
        let pipeline = match &self.pipeline {
            Some(p) => p,
            None => return,
        };
        let bind_group = match &self.bind_group {
            Some(b) => b,
            None => return,
        };

        let text_width = calculate_text_width(text, font_data, scale);
        let text_height = font_data.common.line_height as f32 * scale;
        let offset_x = -text_width * anchor_x;
        let offset_y = -text_height * anchor_y;

        let mut vertices: Vec<FontVertex> = Vec::new();
        let mut cursor_x = x + offset_x;
        let cursor_y = y + scroll_offset + offset_y;
        let scale_w = font_data.common.scale_w as f32;
        let scale_h = font_data.common.scale_h as f32;

        for ch in text.chars() {
            let char_code = ch as u32;
            if let Some(info) = font_data.chars.get(&char_code) {
                let cx = cursor_x + info.x_offset as f32 * scale;
                let cy = cursor_y + info.y_offset as f32 * scale;
                let cw = info.width as f32 * scale;
                let ch_h = info.height as f32 * scale;

                let u0 = info.x as f32 / scale_w;
                let v0 = info.y as f32 / scale_h;
                let u1 = (info.x + info.width) as f32 / scale_w;
                let v1 = (info.y + info.height) as f32 / scale_h;

                vertices.push(FontVertex {
                    position: [cx, cy],
                    tex_coord: [u0, v0],
                    color,
                });
                vertices.push(FontVertex {
                    position: [cx + cw, cy],
                    tex_coord: [u1, v0],
                    color,
                });
                vertices.push(FontVertex {
                    position: [cx + cw, cy + ch_h],
                    tex_coord: [u1, v1],
                    color,
                });

                vertices.push(FontVertex {
                    position: [cx, cy],
                    tex_coord: [u0, v0],
                    color,
                });
                vertices.push(FontVertex {
                    position: [cx + cw, cy + ch_h],
                    tex_coord: [u1, v1],
                    color,
                });
                vertices.push(FontVertex {
                    position: [cx, cy + ch_h],
                    tex_coord: [u0, v1],
                    color,
                });

                cursor_x += info.x_advance as f32 * scale;
            }
        }

        if vertices.is_empty() {
            return;
        }

        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Font VB"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.set_vertex_buffer(0, vb.slice(..));
        render_pass.draw(0..vertices.len() as u32, 0..1);
    }
}
