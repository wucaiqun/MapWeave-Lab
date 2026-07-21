use anyhow::Context;
use std::sync::Arc;
use std::time::Instant;
use winit::dpi::PhysicalSize;

use mw_render_wgpu::{FrameUniforms, Renderer, RenderStats, DEPTH_FORMAT};
use mw_telemetry::elapsed_ms;

pub struct RenderState {
    #[allow(dead_code)]
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    window_size: PhysicalSize<u32>,
    _depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
}

impl RenderState {
    pub fn new(window: Arc<winit::window::Window>) -> anyhow::Result<Self> {
        pollster::block_on(Self::init_async(window))
    }

    async fn init_async(window: Arc<winit::window::Window>) -> anyhow::Result<Self> {
        let window_size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window)?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("no suitable adapter found")?;

        let mut config = surface
            .get_default_config(&adapter, window_size.width.max(1), window_size.height.max(1))
            .context("surface not compatible with adapter")?;

        let caps = surface.get_capabilities(&adapter);
        if let Some(format) = caps.formats.iter().copied().find(|f| f.is_srgb()) {
            config.format = format;
        }

        let device_descriptor = wgpu::DeviceDescriptor {
            label: Some("native-viewer"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::default(),
        };

        let (device, queue) = adapter
            .request_device(&device_descriptor)
            .await
            .context("failed to request device")?;

        surface.configure(&device, &config);
        let (depth_texture, depth_view) = create_depth_targets(&device, config.width, config.height);

        Ok(Self {
            instance,
            surface,
            device,
            queue,
            config,
            window_size,
            _depth_texture: depth_texture,
            depth_view,
        })
    }

    fn reconfigure_surface(&mut self) {
        self.config.width = self.window_size.width.max(1);
        self.config.height = self.window_size.height.max(1);
        self.surface.configure(&self.device, &self.config);
        let (depth_texture, depth_view) =
            create_depth_targets(&self.device, self.config.width, self.config.height);
        self._depth_texture = depth_texture;
        self.depth_view = depth_view;
    }

    pub fn resize(&mut self, window_size: PhysicalSize<u32>) {
        if window_size.width == 0 || window_size.height == 0 {
            return;
        }
        self.window_size = window_size;
        self.reconfigure_surface();
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn render(
        &mut self,
        renderer: &Renderer,
        clear_color: wgpu::Color,
        frame: &FrameUniforms,
    ) -> (f64, RenderStats) {
        let render_start = Instant::now();
        let output = match self.surface.get_current_texture() {
            Ok(o) => o,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.reconfigure_surface();
                return (0.0, RenderStats::default());
            }
            Err(wgpu::SurfaceError::Timeout) => return (0.0, RenderStats::default()),
            Err(e) => {
                log::warn!("surface error: {e}");
                return (0.0, RenderStats::default());
            }
        };

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("native-viewer-frame"),
        });

        let mut stats = RenderStats::default();
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            stats = renderer.render(&mut pass, &self.queue, frame);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        (elapsed_ms(render_start), stats)
    }
}

fn create_depth_targets(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frame-depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}
