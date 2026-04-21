use mw_core::TileSceneData;

use crate::RenderLayer;

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

    pub fn upload_tile(&mut self, tile: &TileSceneData) -> anyhow::Result<()> {
        for layer_data in &tile.layers {
            for layer in &mut self.layers {
                layer.upload(layer_data)?;
            }
        }

        Ok(())
    }

    pub fn render(&self) {
        for layer in &self.layers {
            layer.render();
        }
    }
}
