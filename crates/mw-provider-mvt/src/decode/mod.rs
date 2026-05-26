use anyhow::Result;
use mw_core::{PolygonFeature, PolygonRing, RingRole, RoadFeature};
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
        let source_layer = layer.name.clone();
        let feature_count = layer.features.len();
        print_info(format!(
            "Decoding layer: {source_layer} with {feature_count} features"
        ));

        for feature in &layer.features {
            let geom_type = feature.r#type.and_then(vector_tile::tile::GeomType::from_i32);
            match geom_type {
                Some(vector_tile::tile::GeomType::Linestring) => {
                    if let Some(road) = decode_road_feature(&source_layer, layer, feature) {
                        decoded.roads.push(road);
                    }
                }
                Some(vector_tile::tile::GeomType::Polygon) => {
                    if let Some(polygon) = decode_polygon_feature(&source_layer, layer, feature) {
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
    source_layer: &str,
    layer: &vector_tile::tile::Layer,
    feature: &vector_tile::tile::Feature,
) -> Option<RoadFeature> {
    let lines = decode_geometry_linestrings(&feature.geometry)?;
    let points = lines.into_iter().next()?;
    Some(RoadFeature {
        id: feature.id.unwrap_or_default(),
        class: decode_feature_class(layer, feature),
        source_layer: source_layer.to_string(),
        points_lon_lat: points,
    })
}

fn decode_polygon_feature(
    source_layer: &str,
    layer: &vector_tile::tile::Layer,
    feature: &vector_tile::tile::Feature,
) -> Option<PolygonFeature> {
    let raw_rings = decode_geometry_rings(&feature.geometry)?;
    let rings = classify_polygon_rings(raw_rings);
    if rings.is_empty() {
        return None;
    }

    Some(PolygonFeature {
        id: feature.id.unwrap_or_default(),
        class: decode_feature_class(layer, feature),
        source_layer: source_layer.to_string(),
        rings,
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

/// Decode MVT geometry into separate rings.
///
/// Per the Mapbox Vector Tile spec, each `MoveTo` starts a new ring and `ClosePath`
/// completes the current ring without adding a new point.
fn decode_geometry_rings(geometry: &[u32]) -> Option<Vec<Vec<[f64; 2]>>> {
    let mut rings = Vec::new();
    let mut current_ring: Vec<[f64; 2]> = Vec::new();
    let mut cursor = 0usize;
    let mut x = 0i32;
    let mut y = 0i32;

    while cursor < geometry.len() {
        let cmd = geometry[cursor];
        cursor += 1;

        let id = cmd & 0x7;
        let count = (cmd >> 3) as usize;

        match id {
            1 => {
                if !current_ring.is_empty() {
                    rings.push(current_ring);
                    current_ring = Vec::new();
                }

                for _ in 0..count {
                    let (px, py) = read_point(geometry, &mut cursor, &mut x, &mut y)?;
                    current_ring.push([px, py]);
                }
            }
            2 => {
                for _ in 0..count {
                    let (px, py) = read_point(geometry, &mut cursor, &mut x, &mut y)?;
                    current_ring.push([px, py]);
                }
            }
            7 => {
                if !current_ring.is_empty() {
                    rings.push(current_ring);
                    current_ring = Vec::new();
                }
            }
            _ => return None,
        }
    }

    if !current_ring.is_empty() {
        rings.push(current_ring);
    }

    if rings.is_empty() {
        None
    } else {
        Some(rings)
    }
}

fn decode_geometry_linestrings(geometry: &[u32]) -> Option<Vec<Vec<[f64; 2]>>> {
    let mut lines = Vec::new();
    let mut current_line: Vec<[f64; 2]> = Vec::new();
    let mut cursor = 0usize;
    let mut x = 0i32;
    let mut y = 0i32;

    while cursor < geometry.len() {
        let cmd = geometry[cursor];
        cursor += 1;

        let id = cmd & 0x7;
        let count = (cmd >> 3) as usize;

        match id {
            1 => {
                if !current_line.is_empty() {
                    lines.push(current_line);
                    current_line = Vec::new();
                }

                for _ in 0..count {
                    let (px, py) = read_point(geometry, &mut cursor, &mut x, &mut y)?;
                    current_line.push([px, py]);
                }
            }
            2 => {
                for _ in 0..count {
                    let (px, py) = read_point(geometry, &mut cursor, &mut x, &mut y)?;
                    current_line.push([px, py]);
                }
            }
            7 => {
                if !current_line.is_empty() {
                    lines.push(current_line);
                    current_line = Vec::new();
                }
            }
            _ => return None,
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines)
    }
}

fn read_point(
    geometry: &[u32],
    cursor: &mut usize,
    x: &mut i32,
    y: &mut i32,
) -> Option<(f64, f64)> {
    if *cursor + 1 >= geometry.len() {
        return None;
    }

    let dx = zigzag_decode(geometry[*cursor]);
    let dy = zigzag_decode(geometry[*cursor + 1]);
    *cursor += 2;

    *x += dx;
    *y += dy;
    Some((*x as f64, *y as f64))
}

/// Classify rings into exterior/hole roles using Mapbox winding rules.
///
/// Adjacent rings with opposite signed area are treated as holes. Rings with the
/// same winding as the current polygon part start a new polygon (MultiPolygon).
fn classify_polygon_rings(raw_rings: Vec<Vec<[f64; 2]>>) -> Vec<PolygonRing> {
    if raw_rings.is_empty() {
        return Vec::new();
    }

    if raw_rings.len() == 1 {
        return vec![PolygonRing {
            points: raw_rings[0].clone(),
            role: RingRole::Exterior,
        }];
    }

    let mut classified = Vec::new();
    let mut reference_winding = 0i8;

    for ring in raw_rings {
        if ring.len() < 3 {
            continue;
        }

        let area = ring_signed_area(&ring);
        if area.abs() < f64::EPSILON {
            continue;
        }

        let winding = if area < 0.0 { -1 } else { 1 };

        if classified.is_empty() || reference_winding == 0 {
            reference_winding = winding;
            classified.push(PolygonRing {
                points: ring,
                role: RingRole::Exterior,
            });
        } else if reference_winding * winding < 0 {
            classified.push(PolygonRing {
                points: ring,
                role: RingRole::Hole,
            });
        } else {
            reference_winding = winding;
            classified.push(PolygonRing {
                points: ring,
                role: RingRole::Exterior,
            });
        }
    }

    classified
}

fn ring_signed_area(ring: &[[f64; 2]]) -> f64 {
    let mut area = 0.0;
    for i in 0..ring.len() {
        let j = (i + 1) % ring.len();
        area += ring[i][0] * ring[j][1];
        area -= ring[j][0] * ring[i][1];
    }
    area / 2.0
}

fn zigzag_decode(value: u32) -> i32 {
    ((value >> 1) as i32) ^ (-((value & 1) as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_geometry_rings_splits_on_move_to_and_close_path() {
        // Square: MoveTo(0,0), LineTo(10,0)+(0,10), ClosePath
        let square = vec![9, 0, 0, 18, 20, 0, 20, 20, 15];
        let rings = decode_geometry_rings(&square).expect("square ring");
        assert_eq!(rings.len(), 1);
        assert_eq!(rings[0].len(), 3);

        // Two separate squares in one geometry buffer.
        let two_squares = vec![
            9, 0, 0, 18, 20, 0, 20, 20, 15, //
            9, 0, 0, 18, 20, 0, 20, 20, 15,
        ];
        let rings = decode_geometry_rings(&two_squares).expect("two rings");
        assert_eq!(rings.len(), 2);
        assert_eq!(rings[0].len(), 3);
        assert_eq!(rings[1].len(), 3);
    }

    #[test]
    fn classify_polygon_rings_marks_holes_with_opposite_winding() {
        let exterior = vec![[0.0, 0.0], [4096.0, 0.0], [4096.0, 4096.0], [0.0, 4096.0]];
        let hole = vec![[1024.0, 1024.0], [1024.0, 3072.0], [3072.0, 3072.0], [3072.0, 1024.0]];

        let classified = classify_polygon_rings(vec![exterior, hole]);
        assert_eq!(classified.len(), 2);
        assert_eq!(classified[0].role, RingRole::Exterior);
        assert_eq!(classified[1].role, RingRole::Hole);
    }
}
