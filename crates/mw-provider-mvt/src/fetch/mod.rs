use anyhow::{Result, anyhow};
use mw_core::TileId;
use mw_telemetry::print_error;
use std::path::PathBuf;

use crate::MvtProviderConfig;

pub fn build_tile_url(config: &MvtProviderConfig, tile_id: TileId) -> String {
    config
        .endpoint_template
        .replace("{z}", &tile_id.z.to_string())
        .replace("{x}", &tile_id.x.to_string())
        .replace("{y}", &tile_id.y.to_string())
        .replace("{access_token}", &config.access_token)
}

fn build_tile_cache_path(cache_dir: &str, tile_id: TileId) -> PathBuf {
    let mut path = PathBuf::from(cache_dir);
    path.push(tile_id.z.to_string());
    path.push(tile_id.x.to_string());
    path.push(format!("{}.pbf", tile_id.y));
    path
}

fn try_read_tile_from_cache(config: &MvtProviderConfig, tile_id: TileId) -> Result<Option<Vec<u8>>> {
    let Some(cache_dir) = &config.cache_dir else {
        return Ok(None);
    };

    let cache_path = build_tile_cache_path(cache_dir, tile_id);
    if !cache_path.exists() {
        return Ok(None);
    }

    let bytes = std::fs::read(&cache_path).map_err(|e| {
        anyhow!(
            "failed to read cached tile at {}: {e}",
            cache_path.display()
        )
    })?;
    Ok(Some(bytes))
}

fn try_write_tile_to_cache(config: &MvtProviderConfig, tile_id: TileId, bytes: &[u8]) -> Result<()> {
    let Some(cache_dir) = &config.cache_dir else {
        return Ok(());
    };

    let cache_path = build_tile_cache_path(cache_dir, tile_id);
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            anyhow!(
                "failed to create tile cache directory {}: {e}",
                parent.display()
            )
        })?;
    }

    std::fs::write(&cache_path, bytes).map_err(|e| {
        anyhow!(
            "failed to write cached tile at {}: {e}",
            cache_path.display()
        )
    })?;

    Ok(())
}

pub async fn fetch_tile_bytes(config: &MvtProviderConfig, tile_id: TileId) -> Result<Vec<u8>> {
    if let Some(bytes) = try_read_tile_from_cache(config, tile_id)? {
        return Ok(bytes);
    }

    let url = build_tile_url(config, tile_id);
    if url.is_empty() {
        return Err(anyhow!("tile url is empty"));
    }
    let response = reqwest::get(&url).await?;
    if !response.status().is_success() {
        let message = format!("failed to fetch tile: {}", response.status());
        print_error(&message);
        return Err(anyhow!(message));
    }

    let bytes = response.bytes().await?;
    let bytes = bytes.to_vec();
    try_write_tile_to_cache(config, tile_id, &bytes)?;
    Ok(bytes)
}
