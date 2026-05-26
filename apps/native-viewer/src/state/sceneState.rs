use anyhow::Context;
use mw_core::{LayerPayload, TileId, TileSceneData};
use mw_provider_mvt::{MvtProvider, MvtProviderConfig, TileProvider};
use mw_render_wgpu::{BackgroundLayer, Renderer, RendererConfig, RoadsLayer};

pub struct SceneState {
    pub renderer: Renderer,
    scene: TileSceneData,
}

impl SceneState {
    pub fn new() -> anyhow::Result<Self> {
        let mut renderer = Renderer::new(RendererConfig {
            clear_color: wgpu::Color {
                r: 0.02,
                g: 0.02,
                b: 0.08,
                a: 1.0,
            },
        });
        renderer.add_layer(Box::new(BackgroundLayer::default()));
        renderer.add_layer(Box::new(RoadsLayer::default()));

        let provider = MvtProvider::new(MvtProviderConfig::default());
        let tile_id = TileId::new(1, 0, 0);
        let runtime = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
        let scene = runtime
            .block_on(provider.fetch_tile(tile_id))
            .context("failed to fetch MVT tile")?;

        let road_count = scene
            .layers
            .iter()
            .filter_map(|layer| {
                if let LayerPayload::Roads(roads) = &layer.payload {
                    Some(roads.len())
                } else {
                    None
                }
            })
            .sum::<usize>();

        let background_count = scene
            .layers
            .iter()
            .filter_map(|layer| {
                if let LayerPayload::Background(background) = &layer.payload {
                    Some(background.len())
                } else {
                    None
                }
            })
            .sum::<usize>();

        log::info!(
            "loaded tile z={} x={} y={} ({} background polygons, {} roads)",
            scene.tile_id.z,
            scene.tile_id.x,
            scene.tile_id.y,
            background_count,
            road_count
        );

        Ok(Self { renderer, scene })
    }

    pub fn prepare(&mut self, device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> anyhow::Result<()> {
        self.renderer.prepare(device, surface_format)
    }

    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> anyhow::Result<()> {
        self.renderer.upload_tile(&self.scene, device, queue)
    }

    pub fn clear_color(&self) -> wgpu::Color {
        self.renderer.clear_color()
    }
}
