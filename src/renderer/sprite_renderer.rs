use wgpu::util::DeviceExt;
// No imports needed here anymore
use super::spritesheet_parser::{parse_spritesheet, SpriteFrame, SpritesheetData};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteVertex {
    pub position: [f32; 2],
    pub tex_coord: [f32; 2],
    pub color: [f32; 4],
}

pub struct RenderSpriteOptions {
    pub anchor_x: f32,
    pub anchor_y: f32,
    pub opacity: f32,
    pub color: [f32; 3],
    pub insets: Option<[f32; 4]>,
    pub scissor: Option<[f32; 4]>,
}

impl Default for RenderSpriteOptions {
    fn default() -> Self {
        Self {
            anchor_x: 0.0,
            anchor_y: 0.0,
            opacity: 1.0,
            color: [1.0, 1.0, 1.0],
            insets: None,
            scissor: None,
        }
    }
}

pub struct SpriteRenderer {
    pub sheet_data: Option<SpritesheetData>,
    pipeline: Option<wgpu::RenderPipeline>,
    bind_group: Option<wgpu::BindGroup>,
    uniform_buffer: Option<wgpu::Buffer>,
    vertices: Vec<SpriteVertex>,
    initialized: bool,
}

impl SpriteRenderer {
    pub fn new() -> Self {
        Self {
            sheet_data: None,
            pipeline: None,
            bind_group: None,
            uniform_buffer: None,
            vertices: Vec::new(),
            initialized: false,
        }
    }

    pub fn initialize(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        assets_dir: &str,
    ) {
        let plist_path = format!("{}/images/gameplay/skins/default/1.plist", assets_dir);
        let png_path = format!("{}/images/gameplay/skins/default/1.png", assets_dir);

        let plist_content = match std::fs::read_to_string(&plist_path) {
            Ok(c) => c,
            Err(e) => {
                log::error!("Failed to read plist {}: {}", plist_path, e);
                return;
            }
        };

        self.sheet_data = match parse_spritesheet(&plist_content) {
            Ok(d) => Some(d),
            Err(e) => {
                log::error!("Failed to parse plist: {}", e);
                return;
            }
        };

        let image_data = match std::fs::read(&png_path) {
            Ok(d) => d,
            Err(e) => {
                log::error!("Failed to read png {}: {}", png_path, e);
                return;
            }
        };

        let image = match image::load_from_memory(&image_data) {
            Ok(img) => img.into_rgba8(),
            Err(e) => {
                log::error!("Failed to decode png: {}", e);
                return;
            }
        };

        let texture_size = wgpu::Extent3d {
            width: image.width(),
            height: image.height(),
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Sprite Texture"),
            size: texture_size,
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
            &image,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * image.width()),
                rows_per_image: Some(image.height()),
            },
            texture_size,
        );

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let shader_src = include_str!("../../shaders/sprite.wgsl");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sprite Shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Sprite BGL"),
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
            label: Some("Sprite Uniforms"),
            size: 8,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Sprite BG"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Sprite PL"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sprite Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<SpriteVertex>() as u64,
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

        self.pipeline = Some(pipeline);
        self.bind_group = Some(bind_group);
        self.uniform_buffer = Some(uniform_buffer);
        self.initialized = true;

        log::info!("SpriteRenderer initialized successfully");
    }

    pub fn is_loaded(&self) -> bool {
        self.initialized
    }

    pub fn update_uniforms(&self, queue: &wgpu::Queue, width: f32, height: f32) {
        if let Some(ref ub) = self.uniform_buffer {
            queue.write_buffer(ub, 0, bytemuck::bytes_of(&[width, height]));
        }
    }

    pub fn begin_frame(&mut self) {
        self.vertices.clear();
    }

    pub fn batch_sprite(
        &mut self,
        frame_name: &str,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        options: &RenderSpriteOptions,
    ) {
        if !self.initialized {
            return;
        }
        let sheet = self.sheet_data.as_ref().unwrap();
        let frame_opt = sheet.frames.get(frame_name).cloned();

        if let Some(frame) = frame_opt {
            let start_x = x - width * options.anchor_x;
            let start_y = y - height * options.anchor_y;

            let color = [
                options.color[0],
                options.color[1],
                options.color[2],
                options.opacity,
            ];

            if let Some(slice) = options.insets {
                self.build_insets(
                    &frame,
                    start_x,
                    start_y,
                    width,
                    height,
                    color,
                    slice,
                    options.scissor,
                );
            } else {
                if frame_name == "long_tilelight.png" {
                    self.build_quad_repeat_y(
                        &frame,
                        start_x,
                        start_y,
                        width,
                        height,
                        color,
                        options.scissor,
                    );
                } else {
                    self.build_quad(
                        &frame,
                        start_x,
                        start_y,
                        width,
                        height,
                        color,
                        options.scissor,
                    );
                }
            }
        }
    }

    pub fn draw<'a>(&'a self, device: &wgpu::Device, render_pass: &mut wgpu::RenderPass<'a>) {
        if self.vertices.is_empty() || !self.initialized {
            return;
        }

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Sprite VB"),
            contents: bytemuck::cast_slice(&self.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        render_pass.set_pipeline(self.pipeline.as_ref().unwrap());
        render_pass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        render_pass.draw(0..self.vertices.len() as u32, 0..1);
    }

    fn calculate_uv(&self, frame: &SpriteFrame, norm_x: f32, norm_y: f32) -> [f32; 2] {
        let sheet = self.sheet_data.as_ref().unwrap();
        let tex_w = sheet.meta_size.x as f32;
        let tex_h = sheet.meta_size.y as f32;

        let padding = 0.5;
        let fw = (frame.frame.w as f32 - padding * 2.0).max(0.0);
        let fh = (frame.frame.h as f32 - padding * 2.0).max(0.0);
        let fx = frame.frame.x as f32 + padding;
        let fy = frame.frame.y as f32 + padding;

        if frame.rotated {
            let u = (fx + (1.0 - norm_y) * fw) / tex_w;
            let v = (fy + norm_x * fh) / tex_h;
            [u, v]
        } else {
            let u = (fx + norm_x * fw) / tex_w;
            let v = (fy + norm_y * fh) / tex_h;
            [u, v]
        }
    }

    fn push_clipped_quad(
        &mut self,
        dst_rect: [f32; 4],
        norm_rect: [f32; 4],
        frame: &SpriteFrame,
        color: [f32; 4],
        scissor: Option<[f32; 4]>,
    ) {
        let [mut dx, mut dy, mut dw, mut dh] = dst_rect;
        let [mut nx, mut ny, mut nw, mut nh] = norm_rect;

        if let Some(sc) = scissor {
            let sx = dx.max(sc[0]);
            let sy = dy.max(sc[1]);
            let ex = (dx + dw).min(sc[0] + sc[2]);
            let ey = (dy + dh).min(sc[1] + sc[3]);

            if sx >= ex || sy >= ey {
                return;
            }

            let clip_nx = nx + ((sx - dx) / dw) * nw;
            let clip_ny = ny + ((sy - dy) / dh) * nh;
            let clip_nw = ((ex - sx) / dw) * nw;
            let clip_nh = ((ey - sy) / dh) * nh;

            dx = sx;
            dy = sy;
            dw = ex - sx;
            dh = ey - sy;
            nx = clip_nx;
            ny = clip_ny;
            nw = clip_nw;
            nh = clip_nh;
        }

        let uv_00 = self.calculate_uv(frame, nx, ny);
        let uv_10 = self.calculate_uv(frame, nx + nw, ny);
        let uv_01 = self.calculate_uv(frame, nx, ny + nh);
        let uv_11 = self.calculate_uv(frame, nx + nw, ny + nh);

        self.vertices.push(SpriteVertex {
            position: [dx, dy],
            tex_coord: uv_00,
            color,
        });
        self.vertices.push(SpriteVertex {
            position: [dx + dw, dy],
            tex_coord: uv_10,
            color,
        });
        self.vertices.push(SpriteVertex {
            position: [dx + dw, dy + dh],
            tex_coord: uv_11,
            color,
        });

        self.vertices.push(SpriteVertex {
            position: [dx, dy],
            tex_coord: uv_00,
            color,
        });
        self.vertices.push(SpriteVertex {
            position: [dx + dw, dy + dh],
            tex_coord: uv_11,
            color,
        });
        self.vertices.push(SpriteVertex {
            position: [dx, dy + dh],
            tex_coord: uv_01,
            color,
        });
    }

    fn build_quad(
        &mut self,
        frame: &SpriteFrame,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
        scissor: Option<[f32; 4]>,
    ) {
        self.push_clipped_quad([x, y, w, h], [0.0, 0.0, 1.0, 1.0], frame, color, scissor);
    }

    fn build_quad_repeat_y(
        &mut self,
        frame: &SpriteFrame,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
        scissor: Option<[f32; 4]>,
    ) {
        let frame_h = frame.source_size.y as f32;
        let num_repeats = (h / frame_h).ceil();
        let repeat_h = h / num_repeats;

        for i in 0..num_repeats as u32 {
            let y_pos = y + i as f32 * repeat_h;
            self.push_clipped_quad(
                [x, y_pos, w, repeat_h],
                [0.0, 0.0, 1.0, 1.0],
                frame,
                color,
                scissor,
            );
        }
    }

    fn build_insets(
        &mut self,
        frame: &SpriteFrame,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
        insets: [f32; 4],
        scissor: Option<[f32; 4]>,
    ) {
        let [t, r, b, l] = insets;
        let src_w = frame.source_size.x as f32;
        let src_h = frame.source_size.y as f32;

        let left_nw = l / src_w;
        let right_nw = r / src_w;
        let mid_nw = 1.0 - left_nw - right_nw;
        let top_nh = t / src_h;
        let bot_nh = b / src_h;
        let mid_nh = 1.0 - top_nh - bot_nh;

        let cols = [(l, left_nw), ((w - l - r).max(0.0), mid_nw), (r, right_nw)];

        let rows = [(t, top_nh), ((h - t - b).max(0.0), mid_nh), (b, bot_nh)];

        let mut curr_y = y;
        let mut curr_ny = 0.0;
        for (dh, nh) in rows {
            let mut curr_x = x;
            let mut curr_nx = 0.0;
            for (dw, nw) in cols {
                if dw > 0.0 && dh > 0.0 {
                    self.push_clipped_quad(
                        [curr_x, curr_y, dw, dh],
                        [curr_nx, curr_ny, nw, nh],
                        frame,
                        color,
                        scissor,
                    );
                }
                curr_x += dw;
                curr_nx += nw;
            }
            curr_y += dh;
            curr_ny += nh;
        }
    }
}
