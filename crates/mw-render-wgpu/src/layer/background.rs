use mw_core::TileLayerData;

use crate::RenderLayer;

pub struct BackgroundLayer;

impl RenderLayer for BackgroundLayer {
    fn upload(&mut self, _layer: &TileLayerData) -> anyhow::Result<()> {
        Ok(())
    }

    fn render(&self) {}
}
