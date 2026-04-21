use async_trait::async_trait;
use mw_core::{TileId, TileSceneData};

use crate::MvtProviderConfig;
use crate::decode::decode_mvt_tile;
use crate::fetch::fetch_tile_bytes;
use crate::map::map_decoded_tile_to_scene;

#[async_trait]
pub trait TileProvider: Send + Sync {
    async fn fetch_tile(&self, tile_id: TileId) -> anyhow::Result<TileSceneData>;
}

pub struct MvtProvider {
    pub config: MvtProviderConfig,
}

impl MvtProvider {
    pub fn new(config: MvtProviderConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl TileProvider for MvtProvider {
    async fn fetch_tile(&self, tile_id: TileId) -> anyhow::Result<TileSceneData> {
        let bytes = fetch_tile_bytes(&self.config, tile_id).await?;
        let decoded = decode_mvt_tile(&bytes)?;
        let scene = map_decoded_tile_to_scene(tile_id, decoded);
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
    async fn fetch_tile_returns_scene_data() {
        let _ = init_logging(LogConfig {
            level: LogLevel::Info,
        });
        let provider = MvtProvider::new(MvtProviderConfig {
            endpoint_template: "https://demotiles.maplibre.org/tiles/{z}/{x}/{y}.pbf".to_string(),
            access_token: String::new(),
            cache_dir: Some("./data-cache-root".to_string()),
        });

        let tile_id = TileId::new(1, 1, 0);
        let scene = provider
            .fetch_tile(tile_id)
            .await
            .expect("fetch_tile should return scene");

        assert_eq!(scene.tile_id, tile_id);
        assert!(!scene.layers.is_empty(), "scene should contain mapped layers");
    }
}
