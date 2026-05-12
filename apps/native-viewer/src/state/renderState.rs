use anyhow::Context;
use std::sync::Arc;
use winit::dpi::PhysicalSize;

pub struct RenderState {
    #[allow(dead_code)]
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    window_size: PhysicalSize<u32>,
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

        Ok(Self {
            instance,
            surface,
            device,
            queue,
            config,
            window_size,
        })
    }

    fn reconfigure_surface(&mut self) {
        self.config.width = self.window_size.width.max(1);
        self.config.height = self.window_size.height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    pub fn resize(&mut self, window_size: PhysicalSize<u32>) {
        if window_size.width == 0 || window_size.height == 0 {
            return;
        }
        self.window_size = window_size;
        self.reconfigure_surface();
    }

    pub fn render(&mut self) {
        let output = match self.surface.get_current_texture() {
            Ok(o) => o,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.reconfigure_surface();
                return;
            }
            Err(wgpu::SurfaceError::Timeout) => return,
            Err(e) => {
                log::warn!("surface error: {e}");
                return;
            }
        };

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("native-viewer-frame"),
        });

        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.02,
                            b: 0.08,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }
}
