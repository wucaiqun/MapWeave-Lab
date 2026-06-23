use async_trait::async_trait;
use mw_core::{TileId, TileSceneData};

use crate::MvtProviderConfig;
use crate::decode::{decode_mvt_tile, DecodedMvtTile};
use crate::fetch::{fetch_tile_bytes, resolve_endpoint_template};
use crate::map::map_decoded_tile_to_scene;

#[async_trait]
pub trait TileProvider: Send + Sync {
    async fn fetch_tile(&self, tile_id: TileId) -> anyhow::Result<TileSceneData>;
}

pub struct MvtProvider {
    pub config: MvtProviderConfig,
}

impl Clone for MvtProvider {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
        }
    }
}

impl MvtProvider {
    pub fn new(config: MvtProviderConfig) -> Self {
        Self { config }
    }

    pub async fn with_resolved_config(mut config: MvtProviderConfig) -> anyhow::Result<Self> {
        resolve_endpoint_template(&mut config).await?;
        Ok(Self::new(config))
    }
}

#[async_trait]
impl TileProvider for MvtProvider {
    async fn fetch_tile(&self, tile_id: TileId) -> anyhow::Result<TileSceneData> {
        let bytes = fetch_tile_bytes(&self.config, tile_id).await?;
        let decoded = if bytes.is_empty() {
            DecodedMvtTile::default()
        } else {
            decode_mvt_tile(&bytes)?
        };
        let scene = map_decoded_tile_to_scene(tile_id, decoded, self.config.source_profile);
        Ok(scene)
    }
}

#[cfg(test)]
mod tests {
    use super::{MvtProvider, TileProvider};
    use crate::MvtProviderConfig;
    use mw_core::TileId;
    use mw_telemetry::{init_logging, LogConfig, LogLevel};

    #[tokio::test]
    async fn fetch_demotile_returns_scene_data() {
        let _ = init_logging(LogConfig {
            level: LogLevel::Info,
        });
        let provider = MvtProvider::new(MvtProviderConfig::demotiles());

        let tile_id = TileId::new(1, 1, 0);
        let scene = provider
            .fetch_tile(tile_id)
            .await
            .expect("fetch_tile should return scene");

        assert_eq!(scene.tile_id, tile_id);
        assert!(!scene.layers.is_empty(), "scene should contain mapped layers");
    }

    #[tokio::test]
    async fn fetch_openfreemap_valencia_tile_has_buildings() {
        let _ = init_logging(LogConfig {
            level: LogLevel::Info,
        });
        let provider = MvtProvider::with_resolved_config(MvtProviderConfig::openfreemap())
            .await
            .expect("resolve OpenFreeMap TileJSON");

        let tile_id = TileId::new(14, 8174, 6234);
        let scene = provider
            .fetch_tile(tile_id)
            .await
            .expect("fetch Valencia tile at z14");

        let building_count: usize = scene
            .layers
            .iter()
            .filter_map(|layer| {
                if let mw_core::LayerPayload::Buildings(buildings) = &layer.payload {
                    Some(buildings.len())
                } else {
                    None
                }
            })
            .sum();

        assert!(building_count > 0, "Valencia z14 tile should contain buildings");
    }

    #[tokio::test]
    async fn fetch_openfreemap_valencia_tile_has_roads() {
        let _ = init_logging(LogConfig {
            level: LogLevel::Info,
        });
        let provider = MvtProvider::with_resolved_config(MvtProviderConfig::openfreemap())
            .await
            .expect("resolve OpenFreeMap TileJSON");

        let tile_id = TileId::new(10, 510, 389);
        let scene = provider
            .fetch_tile(tile_id)
            .await
            .expect("fetch Valencia tile");

        let road_count: usize = scene
            .layers
            .iter()
            .filter_map(|layer| {
                if let mw_core::LayerPayload::Roads(roads) = &layer.payload {
                    Some(roads.len())
                } else {
                    None
                }
            })
            .sum();

        assert!(road_count > 0, "Valencia tile should contain transportation features");
    }
}
