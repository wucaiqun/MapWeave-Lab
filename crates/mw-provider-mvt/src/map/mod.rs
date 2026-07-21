use mw_core::{
    meters_to_world, LayerKind, LayerPayload, RoadFeature, TileId, TileLayerData, TileSceneData,
    WORLD_ZOOM,
};

use crate::config::MvtSourceProfile;
use crate::decode::DecodedMvtTile;

/// Ensure buildings without/near-zero height still extrude enough to read as volumes.
const MIN_BUILDING_EXTRUDE_M: f64 = 3.0;

/// OpenMapTiles polygon layers used as map background fills.
const OPENMAPTILES_FILL_LAYERS: &[&str] = &["water", "landcover", "landuse", "park"];

/// OpenMapTiles building footprints (available from zoom 13+).
const OPENMAPTILES_BUILDING_LAYERS: &[&str] = &["building"];

/// OpenMapTiles line layers rendered as roads / paths.
const OPENMAPTILES_LINE_LAYERS: &[&str] = &["transportation", "waterway"];

/// MapLibre demotiles style uses `countries` for fill polygons and
/// `countries` + `geolines` for line layers (zoom 0–6 only).
const DEMOTILES_FILL_LAYERS: &[&str] = &["countries"];
const DEMOTILES_LINE_LAYERS: &[&str] = &["countries", "geolines"];

pub fn map_decoded_tile_to_scene(
    tile_id: TileId,
    decoded: DecodedMvtTile,
    profile: MvtSourceProfile,
) -> TileSceneData {
    let (fill_layers, building_layers, line_layers) = match profile {
        MvtSourceProfile::OpenMapTiles => (
            OPENMAPTILES_FILL_LAYERS,
            OPENMAPTILES_BUILDING_LAYERS,
            OPENMAPTILES_LINE_LAYERS,
        ),
        MvtSourceProfile::MapLibreDemo => (DEMOTILES_FILL_LAYERS, &[] as &[&str], DEMOTILES_LINE_LAYERS),
    };

    let mut background = Vec::new();
    let mut buildings = Vec::new();
    for mut polygon in decoded.polygons {
        if building_layers.contains(&polygon.source_layer.as_str()) {
            let top_m = polygon
                .height
                .max(polygon.min_height + MIN_BUILDING_EXTRUDE_M);
            let bottom_m = polygon.min_height.max(0.0);
            // Heights live in WORLD_ZOOM units so merge can scale footprints without
            // double-scaling extrusion.
            polygon.height = meters_to_world(top_m, WORLD_ZOOM);
            polygon.min_height = meters_to_world(bottom_m, WORLD_ZOOM);
            buildings.push(polygon);
        } else if fill_layers.contains(&polygon.source_layer.as_str()) {
            polygon.height = 0.0;
            polygon.min_height = 0.0;
            background.push(polygon);
        }
    }

    let roads: Vec<RoadFeature> = decoded
        .roads
        .into_iter()
        .filter(|road| line_layers.contains(&road.source_layer.as_str()))
        .collect();

    TileSceneData {
        tile_id,
        layers: vec![
            TileLayerData {
                kind: LayerKind::Background,
                payload: LayerPayload::Background(background),
            },
            TileLayerData {
                kind: LayerKind::Buildings,
                payload: LayerPayload::Buildings(buildings),
            },
            TileLayerData {
                kind: LayerKind::Roads,
                payload: LayerPayload::Roads(roads),
            },
        ],
    }
}
