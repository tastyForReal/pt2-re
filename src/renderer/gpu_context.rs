use std::sync::Arc;

/// GPU context wrapping wgpu device, queue, and optional surface/config.
/// When `surface` and `config` are `None`, the context operates in headless mode
/// (suitable for offscreen rendering only, e.g. video recording).
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: Option<wgpu::Surface<'static>>,
    pub config: Option<wgpu::SurfaceConfiguration>,
    pub format: wgpu::TextureFormat,
}

/// Offscreen render target for capturing frames to pixel buffers.
pub struct OffscreenTarget {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub buffer: wgpu::Buffer,
}

impl OffscreenTarget {
    /// Create an offscreen target for rendering and pixel readback.
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Self {
        // Texture with RENDER_ATTACHMENT | COPY_SRC so we can render to it and copy from it
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Offscreen Render Target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Buffer with MAP_READ | COPY_DST so we can copy texture data and read it back
        // Use 4 bytes per pixel (BGRA) with row alignment padding
        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

        let buffer_size = (padded_bytes_per_row * height) as u64;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Offscreen Readback Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Self {
            texture,
            view,
            buffer,
        }
    }

    /// Copy the texture contents to the readback buffer and return the mapped pixel data.
    /// Returns BGRA pixel data (unpadded, i.e., width * height * 4 bytes).
    pub fn read_pixels(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = self.texture.width() * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Readback Encoder"),
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(self.texture.height()),
                },
            },
            wgpu::Extent3d {
                width: self.texture.width(),
                height: self.texture.height(),
                depth_or_array_layers: 1,
            },
        );

        queue.submit(std::iter::once(encoder.finish()));

        // Map the buffer and read pixels
        let buffer_slice = self.buffer.slice(..);

        // NOTE: We use pollster::block_on here. The wgpu Buffer mapping API
        // requires polling the device to complete the mapping.
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().expect("Failed to map buffer");

        let data = buffer_slice.get_mapped_range();
        let width = self.texture.width() as usize;
        let height = self.texture.height() as usize;

        // Remove row padding to get contiguous BGRA data
        let mut pixels = Vec::with_capacity(width * height * 4);
        let padded_row_size = padded_bytes_per_row as usize;
        for row in 0..height {
            let start = row * padded_row_size;
            let end = start + width * 4;
            pixels.extend_from_slice(&data[start..end]);
        }

        drop(data);
        self.buffer.unmap();

        pixels
    }
}

impl GpuContext {
    /// Create a GPU context with a window surface (normal interactive mode).
    pub async fn new(window: Arc<winit::window::Window>) -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("Failed to create surface: {}", e))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| "No suitable GPU adapter found".to_string())?;

        log::info!("GPU adapter: {:?}", adapter.get_info());

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Game Device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    ..Default::default()
                },
                None,
            )
            .await
            .map_err(|e| format!("Failed to create device: {}", e))?;

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        Ok(Self {
            device,
            queue,
            surface: Some(surface),
            config: Some(config),
            format,
        })
    }

    /// Create a headless GPU context without a window surface.
    /// Used for video recording in environments without display drivers.
    /// Tries multiple backends (GL, Vulkan) with force_fallback_adapter enabled.
    pub async fn new_headless() -> Result<Self, String> {
        // Try multiple backend strategies
        let backend_strategies: Vec<(wgpu::Backends, &str)> = vec![
            (wgpu::Backends::GL, "GL"),
            (wgpu::Backends::VULKAN, "Vulkan"),
            (wgpu::Backends::all(), "all (fallback)"),
        ];

        let mut last_error = String::from("No backend strategies available");

        for (backends, name) in &backend_strategies {
            log::info!("[HEADLESS] Trying wgpu backend: {}", name);

            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: *backends,
                ..Default::default()
            });

            let adapter = match instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: true,
                })
                .await
            {
                Some(a) => {
                    log::info!("[HEADLESS] Found adapter ({}): {:?}", name, a.get_info());
                    a
                }
                None => {
                    last_error = format!("No adapter found for backend: {}", name);
                    log::warn!("[HEADLESS] {}", last_error);
                    continue;
                }
            };

            let (device, queue) = match adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("Headless Game Device"),
                        required_features: wgpu::Features::empty(),
                        required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                            .using_resolution(adapter.limits()),
                        ..Default::default()
                    },
                    None,
                )
                .await
            {
                Ok((d, q)) => (d, q),
                Err(e) => {
                    last_error = format!("Failed to create device ({}): {}", name, e);
                    log::warn!("[HEADLESS] {}", last_error);
                    continue;
                }
            };

            // Use BGRA8Unorm as a safe default for offscreen rendering
            let format = wgpu::TextureFormat::Bgra8Unorm;

            log::info!(
                "[HEADLESS] Successfully created headless GPU context (backend: {})",
                name
            );

            return Ok(Self {
                device,
                queue,
                surface: None,
                config: None,
                format,
            });
        }

        Err(format!(
            "Failed to create headless GPU context. Last error: {}",
            last_error
        ))
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0
            && height > 0
            && let (Some(surface), Some(config)) = (&self.surface, &mut self.config)
        {
            config.width = width;
            config.height = height;
            surface.configure(&self.device, config);
        }
    }
}
