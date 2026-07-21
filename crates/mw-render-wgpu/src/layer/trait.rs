use mw_core::TileLayerData;

use crate::{FrameUniforms, RenderStats};

pub trait RenderLayer {
    fn prepare(&mut self, device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> anyhow::Result<()>;

    fn upload(
        &mut self,
        layer: &TileLayerData,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<()>;

    fn render(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        queue: &wgpu::Queue,
        frame: &FrameUniforms,
    ) -> RenderStats;
}
