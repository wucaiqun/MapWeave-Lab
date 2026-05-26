use mw_core::{LayerKind, LayerPayload, RoadFeature, TileLayerData};

use crate::{FrameUniforms, RenderLayer};

pub struct RoadsLayer {
    pub roads: Vec<RoadFeature>,
}

impl Default for RoadsLayer {
    fn default() -> Self {
        Self { roads: Vec::new() }
    }
}

impl RenderLayer for RoadsLayer {
    fn prepare(&mut self, _device: &wgpu::Device, _surface_format: wgpu::TextureFormat) -> anyhow::Result<()> {
        Ok(())
    }

    fn upload(
        &mut self,
        layer: &TileLayerData,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
    ) -> anyhow::Result<()> {
        if layer.kind != LayerKind::Roads {
            return Ok(());
        }

        if let LayerPayload::Roads(roads) = &layer.payload {
            self.roads = roads.clone();
        }

        Ok(())
    }

    fn render(&self, _pass: &mut wgpu::RenderPass<'_>, _queue: &wgpu::Queue, _frame: &FrameUniforms) {}
}
