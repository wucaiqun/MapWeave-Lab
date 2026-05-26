use mw_core::{LayerKind, LayerPayload, PolygonFeature, RoadFeature, TileId, TileLayerData, TileSceneData};

use crate::decode::DecodedMvtTile;

/// MapLibre demotiles style uses `countries` for fill polygons and
/// `countries` + `geolines` for line layers.
const FILL_SOURCE_LAYERS: &[&str] = &["countries"];
const LINE_SOURCE_LAYERS: &[&str] = &["countries", "geolines"];

pub fn map_decoded_tile_to_scene(tile_id: TileId, decoded: DecodedMvtTile) -> TileSceneData {
    let background: Vec<PolygonFeature> = decoded
        .polygons
        .into_iter()
        .filter(|polygon| FILL_SOURCE_LAYERS.contains(&polygon.source_layer.as_str()))
        .collect();

    let roads: Vec<RoadFeature> = decoded
        .roads
        .into_iter()
        .filter(|road| LINE_SOURCE_LAYERS.contains(&road.source_layer.as_str()))
        .collect();

    TileSceneData {
        tile_id,
        layers: vec![
            TileLayerData {
                kind: LayerKind::Background,
                payload: LayerPayload::Background(background),
            },
            TileLayerData {
                kind: LayerKind::Roads,
                payload: LayerPayload::Roads(roads),
            },
        ],
    }
}
