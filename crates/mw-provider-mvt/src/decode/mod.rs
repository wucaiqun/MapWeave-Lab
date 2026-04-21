use anyhow::Result;
use mw_core::{RoadFeature, PolygonFeature};
use mw_telemetry::print_info;
use prost::Message;

#[derive(Debug, Clone, Default)]
pub struct DecodedMvtTile {
    pub roads: Vec<RoadFeature>,
    pub polygons: Vec<PolygonFeature>,
}

// Generated module name, from package in .proto.
// `prost-build` writes `vector_tile.rs` into Cargo's OUT_DIR.
pub mod vector_tile {
    include!(concat!(env!("OUT_DIR"), "/vector_tile.rs"));
}

pub fn decode_mvt_tile(bytes: &[u8]) -> Result<DecodedMvtTile> {
    let tile = vector_tile::Tile::decode(bytes)?;
    let mut decoded = DecodedMvtTile::default();

    for layer in &tile.layers {
        let name = &layer.name;
        let feature_count = layer.features.len();
        print_info(format!("Decoding layer: {} with {} features", name, feature_count));

        for feature in &layer.features {
            let geom_type = feature.r#type.and_then(vector_tile::tile::GeomType::from_i32);
            match geom_type {
                Some(vector_tile::tile::GeomType::Linestring) => {
                    if let Some(road) = decode_road_feature(layer, feature) {
                        decoded.roads.push(road);
                    }
                }
                Some(vector_tile::tile::GeomType::Polygon) => {
                    if let Some(polygon) = decode_polygon_feature(layer, feature) {
                        decoded.polygons.push(polygon);
                    }
                }
                _ => {}
            }
        }
    }

    Ok(decoded)
}

fn decode_road_feature(
    layer: &vector_tile::tile::Layer,
    feature: &vector_tile::tile::Feature,
) -> Option<RoadFeature> {
    let points = decode_geometry_points(&feature.geometry)?;
    Some(RoadFeature {
        id: feature.id.unwrap_or_default(),
        class: decode_feature_class(layer, feature),
        points_lon_lat: points,
    })
}

fn decode_polygon_feature(
    layer: &vector_tile::tile::Layer,
    feature: &vector_tile::tile::Feature,
) -> Option<PolygonFeature> {
    let points = decode_geometry_points(&feature.geometry)?;
    Some(PolygonFeature {
        id: feature.id.unwrap_or_default(),
        class: decode_feature_class(layer, feature),
        points_lon_lat: points,
    })
}

fn decode_feature_class(layer: &vector_tile::tile::Layer, feature: &vector_tile::tile::Feature) -> String {
    let tags = &feature.tags;
    let mut i = 0usize;
    while i + 1 < tags.len() {
        let key_idx = tags[i] as usize;
        let val_idx = tags[i + 1] as usize;
        if let Some(key) = layer.keys.get(key_idx) {
            if key == "class" {
                if let Some(value) = layer.values.get(val_idx) {
                    if let Some(class_name) = &value.string_value {
                        return class_name.clone();
                    }
                }
            }
        }
        i += 2;
    }

    layer.name.clone()
}

fn decode_geometry_points(geometry: &[u32]) -> Option<Vec<[f64; 2]>> {
    let mut points = Vec::new();
    let mut cursor = 0usize;
    let mut x = 0i32;
    let mut y = 0i32;

    while cursor < geometry.len() {
        let cmd = geometry[cursor];
        cursor += 1;

        let id = cmd & 0x7;
        let count = (cmd >> 3) as usize;

        match id {
            1 | 2 => {
                for _ in 0..count {
                    if cursor + 1 >= geometry.len() {
                        return None;
                    }

                    let dx = zigzag_decode(geometry[cursor]);
                    let dy = zigzag_decode(geometry[cursor + 1]);
                    cursor += 2;

                    x += dx;
                    y += dy;
                    points.push([x as f64, y as f64]);
                }
            }
            7 => {}
            _ => return None,
        }
    }

    if points.is_empty() {
        None
    } else {
        Some(points)
    }
}

fn zigzag_decode(value: u32) -> i32 {
    ((value >> 1) as i32) ^ (-((value & 1) as i32))
}
