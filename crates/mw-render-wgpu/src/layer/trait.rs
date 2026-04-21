use mw_core::TileLayerData;

pub trait RenderLayer {
    fn upload(&mut self, layer: &TileLayerData) -> anyhow::Result<()>;
    fn render(&self);
}
