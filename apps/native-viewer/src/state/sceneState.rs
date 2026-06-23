use std::collections::{HashMap, HashSet};

use anyhow::Context;
use mw_core::{merge_tiles_into_scene, tile_world_origin, LayerPayload, TileId, TileSceneData, TILE_EXTENT};
use mw_provider_mvt::{MvtProvider, MvtProviderConfig, TileProvider};
use mw_render_wgpu::{BackgroundLayer, BuildingsLayer, Renderer, RendererConfig, RoadsLayer};

use super::CameraState;

/// Max new tile HTTP requests per frame (bandwidth throttle only — not a tile-count cap).
const MAX_TILES_PER_SYNC: usize = 9;

enum TileSource {
    Unresolved(MvtProviderConfig),
    Ready(MvtProvider),
}

pub struct SceneState {
    pub renderer: Renderer,
    tile_source: TileSource,
    runtime: tokio::runtime::Runtime,
    tiles: HashMap<TileId, TileSceneData>,
    visible: HashSet<TileId>,
    merged: TileSceneData,
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
        renderer.add_layer(Box::new(BuildingsLayer::default()));
        renderer.add_layer(Box::new(RoadsLayer::default()));

        Ok(Self {
            renderer,
            tile_source: TileSource::Unresolved(MvtProviderConfig::default()),
            runtime: tokio::runtime::Runtime::new().context("failed to create tokio runtime")?,
            tiles: HashMap::new(),
            visible: HashSet::new(),
            merged: TileSceneData {
                tile_id: TileId::new(0, 0, 0),
                layers: vec![],
            },
        })
    }

    pub fn prepare(&mut self, device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> anyhow::Result<()> {
        self.renderer.prepare(device, surface_format)
    }

    fn ensure_provider(&mut self) -> bool {
        loop {
            match &self.tile_source {
                TileSource::Ready(_) => return true,
                TileSource::Unresolved(config) => {
                    let config = config.clone();
                    match self
                        .runtime
                        .block_on(MvtProvider::with_resolved_config(config))
                    {
                        Ok(provider) => {
                            log::info!("tile endpoint: {}", provider.config.endpoint_template);
                            self.tile_source = TileSource::Ready(provider);
                        }
                        Err(err) => {
                            log::warn!("tile endpoint not ready yet: {err:#}");
                            return false;
                        }
                    }
                }
            }
        }
    }

    /// Resolve the tile endpoint (once) and fetch missing tiles incrementally.
    /// Never fails the caller — errors are logged and retried on later frames.
    pub fn sync_visible_tiles(
        &mut self,
        camera: &CameraState,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) {
        if !self.ensure_provider() {
            return;
        }

        let mut wanted: Vec<TileId> = camera.visible_tiles();
        let center = [f64::from(camera.target().x), f64::from(camera.target().z)];
        wanted.sort_by_key(|tile| {
            let [ox, oz] = tile_world_origin(*tile);
            let cx = ox + TILE_EXTENT * 0.5;
            let cz = oz + TILE_EXTENT * 0.5;
            let dx = cx - center[0];
            let dz = cz - center[1];
            ((dx * dx + dz * dz) * 1_000.0) as u64
        });
        let wanted_set: HashSet<TileId> = wanted.iter().copied().collect();

        let mut fetched = 0usize;
        let mut loaded_any = false;

        for tile_id in wanted {
            if self.tiles.contains_key(&tile_id) {
                continue;
            }
            if fetched >= MAX_TILES_PER_SYNC {
                break;
            }
            fetched += 1;

            match self.fetch_one_tile(tile_id) {
                Ok(scene) => {
                    log::info!(
                        "loaded tile z={} x={} y={} ({} background, {} buildings, {} roads)",
                        scene.tile_id.z,
                        scene.tile_id.x,
                        scene.tile_id.y,
                        count_background(&scene),
                        count_buildings(&scene),
                        count_roads(&scene),
                    );
                    self.tiles.insert(tile_id, scene);
                    loaded_any = true;
                }
                Err(err) => {
                    log::warn!(
                        "failed to fetch tile z={} x={} y={}: {err:#}",
                        tile_id.z,
                        tile_id.x,
                        tile_id.y,
                    );
                }
            }
        }

        let loaded: HashSet<TileId> = wanted_set
            .iter()
            .copied()
            .filter(|id| self.tiles.contains_key(id))
            .collect();

        if !loaded_any && loaded == self.visible {
            return;
        }
        if loaded.is_empty() {
            return;
        }

        let tile_refs: Vec<&TileSceneData> = loaded
            .iter()
            .filter_map(|id| self.tiles.get(id))
            .collect();

        self.merged = merge_tiles_into_scene(&tile_refs);
        log::info!(
            "merged {} tiles ({} background, {} buildings, {} roads)",
            loaded.len(),
            count_background(&self.merged),
            count_buildings(&self.merged),
            count_roads(&self.merged),
        );

        if let Err(err) = self.renderer.upload_tile(&self.merged, device, queue) {
            log::warn!("failed to upload tile geometry: {err:#}");
            return;
        }

        self.visible = loaded;
    }

    fn fetch_one_tile(&mut self, tile_id: TileId) -> anyhow::Result<TileSceneData> {
        let TileSource::Ready(provider) = &self.tile_source else {
            anyhow::bail!("tile provider not ready");
        };
        let provider = provider.clone();
        self.runtime.block_on(provider.fetch_tile(tile_id))
    }

    pub fn clear_color(&self) -> wgpu::Color {
        self.renderer.clear_color()
    }
}

fn count_roads(scene: &TileSceneData) -> usize {
    scene
        .layers
        .iter()
        .filter_map(|layer| {
            if let LayerPayload::Roads(roads) = &layer.payload {
                Some(roads.len())
            } else {
                None
            }
        })
        .sum()
}

fn count_buildings(scene: &TileSceneData) -> usize {
    scene
        .layers
        .iter()
        .filter_map(|layer| {
            if let LayerPayload::Buildings(buildings) = &layer.payload {
                Some(buildings.len())
            } else {
                None
            }
        })
        .sum()
}

fn count_background(scene: &TileSceneData) -> usize {
    scene
        .layers
        .iter()
        .filter_map(|layer| {
            if let LayerPayload::Background(background) = &layer.payload {
                Some(background.len())
            } else {
                None
            }
        })
        .sum()
}
