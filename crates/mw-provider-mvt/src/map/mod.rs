use mw_core::{LayerKind, LayerPayload, TileId, TileLayerData, TileSceneData};

use crate::decode::DecodedMvtTile;

pub fn map_decoded_tile_to_scene(tile_id: TileId, decoded: DecodedMvtTile) -> TileSceneData {
    TileSceneData {
        tile_id,
        layers: vec![
            TileLayerData {
                kind: LayerKind::Background,
                payload: LayerPayload::Empty,
            },
            TileLayerData {
                kind: LayerKind::Roads,
                payload: LayerPayload::Roads(decoded.roads),
            },
        ],
    }
}
