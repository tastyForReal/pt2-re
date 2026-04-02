use bytemuck::{Pod, Zeroable};

/// Vertex for colored rectangle rendering (tiles, grid lines, etc.)
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct TileVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

/// The tile renderer draws colored rectangles using a simple vertex/fragment shader.
pub struct TileRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl TileRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader_src = include_str!("../../shaders/tile.wgsl");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Tile Shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Tile BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Tile Uniforms"),
            // Padded to 16 bytes for GL ES 3.0 compatibility (min_uniform_buffer_offset_alignment)
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Tile BG"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Tile PL"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Tile Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TileVertex>() as u64,
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

        Self {
            pipeline,
            uniform_buffer,
            bind_group,
        }
    }

    pub fn update_uniforms(&self, queue: &wgpu::Queue, width: f32, height: f32) {
        // Pad to 16 bytes for GL ES 3.0 compatibility
        let padded: [f32; 4] = [width, height, 0.0, 0.0];
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&padded));
    }

    pub fn draw<'a>(
        &'a self,
        device: &wgpu::Device,
        render_pass: &mut wgpu::RenderPass<'a>,
        vertices: &[TileVertex],
    ) {
        if vertices.is_empty() {
            return;
        }

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Tile VB"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        render_pass.draw(0..vertices.len() as u32, 0..1);
    }
}

pub fn push_rect(vertices: &mut Vec<TileVertex>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    let x0 = x;
    let y0 = y;
    let x1 = x + w;
    let y1 = y + h;
    vertices.push(TileVertex {
        position: [x0, y0],
        color,
    });
    vertices.push(TileVertex {
        position: [x1, y0],
        color,
    });
    vertices.push(TileVertex {
        position: [x1, y1],
        color,
    });
    vertices.push(TileVertex {
        position: [x0, y0],
        color,
    });
    vertices.push(TileVertex {
        position: [x1, y1],
        color,
    });
    vertices.push(TileVertex {
        position: [x0, y1],
        color,
    });
}

use wgpu::util::DeviceExt;
