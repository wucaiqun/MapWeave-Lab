use mw_core::TileSceneData;

use crate::{FrameUniforms, RenderLayer};

pub struct RendererConfig {
    pub clear_color: wgpu::Color,
}

pub struct Renderer {
    pub config: RendererConfig,
    pub layers: Vec<Box<dyn RenderLayer + Send + Sync>>,
}

impl Renderer {
    pub fn new(config: RendererConfig) -> Self {
        Self {
            config,
            layers: Vec::new(),
        }
    }

    pub fn add_layer(&mut self, layer: Box<dyn RenderLayer + Send + Sync>) {
        self.layers.push(layer);
    }

    pub fn prepare(&mut self, device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> anyhow::Result<()> {
        for layer in &mut self.layers {
            layer.prepare(device, surface_format)?;
        }
        Ok(())
    }

    pub fn upload_tile(
        &mut self,
        tile: &TileSceneData,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<()> {
        for layer_data in &tile.layers {
            for layer in &mut self.layers {
                layer.upload(layer_data, device, queue)?;
            }
        }

        Ok(())
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>, queue: &wgpu::Queue, frame: &FrameUniforms) {
        for layer in &self.layers {
            layer.render(pass, queue, frame);
        }
    }

    pub fn clear_color(&self) -> wgpu::Color {
        self.config.clear_color
    }
}
