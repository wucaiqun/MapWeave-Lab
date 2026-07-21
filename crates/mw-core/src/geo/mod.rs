use std::f64::consts::PI;

use crate::{LayerKind, LayerPayload, PolygonFeature, PolygonRing, RoadFeature, TileId, TileLayerData, TileSceneData};

/// MVT tile coordinate extent (Mapbox Vector Tile spec default).
pub const TILE_EXTENT: f64 = 4096.0;

/// Camera / merge world space is always expressed at this slippy-map zoom.
///
/// Tiles fetched at coarser zooms are scaled into this space so the camera never
/// has to rebase when LOD changes.
pub const WORLD_ZOOM: u8 = 14;

/// Coarsest tile zoom used by the viewer LOD ladder.
pub const MIN_TILE_ZOOM: u8 = 10;

/// Default map zoom — alias of [`WORLD_ZOOM`] (buildings appear from z13+).
pub const DEFAULT_ZOOM: u8 = WORLD_ZOOM;

/// WGS84 equator circumference used for meter ↔ world-unit scaling.
const EARTH_CIRCUMFERENCE_M: f64 = 40_075_016.686;

/// Convert meters to world Y units in [`WORLD_ZOOM`] space (equator approximation).
pub fn meters_to_world(meters: f64, z: u8) -> f64 {
    let tiles_at_zoom = f64::from(1u32 << z);
    meters * (TILE_EXTENT * tiles_at_zoom) / EARTH_CIRCUMFERENCE_M
}

/// How many [`WORLD_ZOOM`] world units one tile at zoom `z` covers on an axis.
pub fn world_units_per_tile(z: u8) -> f64 {
    let delta = WORLD_ZOOM.saturating_sub(z);
    TILE_EXTENT * f64::from(1u32 << delta)
}

/// Scale factor from a tile's local MVT units into [`WORLD_ZOOM`] world units.
pub fn tile_to_world_scale(z: u8) -> f64 {
    world_units_per_tile(z) / TILE_EXTENT
}

/// Pick a tile zoom so roughly `target_tiles` tiles span `ground_width` world units.
///
/// `current` adds hysteresis (~0.55 zoom levels) so LOD does not flicker at edges.
pub fn tile_zoom_for_ground_width(ground_width: f64, current: u8) -> u8 {
    const TARGET_TILES: f64 = 2.5;
    /// Stay on `current` until continuous ideal drifts past this band.
    const HYSTERESIS: f64 = 0.55;

    let width = ground_width.max(TILE_EXTENT);
    let ideal_tile_span = width / TARGET_TILES;
    // span = TILE_EXTENT * 2^(WORLD_ZOOM - z)  →  z = WORLD_ZOOM - log2(span / TILE_EXTENT)
    let zoom_f = f64::from(WORLD_ZOOM) - (ideal_tile_span / TILE_EXTENT).log2();
    let zoom_f = zoom_f.clamp(f64::from(MIN_TILE_ZOOM), f64::from(WORLD_ZOOM));

    let current = current.clamp(MIN_TILE_ZOOM, WORLD_ZOOM);
    if (zoom_f - f64::from(current)).abs() < HYSTERESIS {
        return current;
    }
    zoom_f.round().clamp(f64::from(MIN_TILE_ZOOM), f64::from(WORLD_ZOOM)) as u8
}

/// Valencia, Spain — default map center.
pub const VALENCIA: LngLat = LngLat {
    lng: -0.3763,
    lat: 39.4699,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LngLat {
    pub lng: f64,
    pub lat: f64,
}

/// Axis-aligned rectangle on the map ground plane (world X / world Z).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldRect {
    pub x_min: f64,
    pub x_max: f64,
    pub z_min: f64,
    pub z_max: f64,
}

impl WorldRect {
    pub fn from_points(points: &[[f64; 2]]) -> Option<Self> {
        if points.is_empty() {
            return None;
        }

        let mut x_min = f64::MAX;
        let mut x_max = f64::MIN;
        let mut z_min = f64::MAX;
        let mut z_max = f64::MIN;

        for [x, z] in points {
            x_min = x_min.min(*x);
            x_max = x_max.max(*x);
            z_min = z_min.min(*z);
            z_max = z_max.max(*z);
        }

        Some(Self {
            x_min,
            x_max,
            z_min,
            z_max,
        })
    }

    pub fn expand(&self, margin: f64) -> Self {
        Self {
            x_min: self.x_min - margin,
            x_max: self.x_max + margin,
            z_min: self.z_min - margin,
            z_max: self.z_max + margin,
        }
    }

    pub fn width(&self) -> f64 {
        self.x_max - self.x_min
    }

    pub fn height(&self) -> f64 {
        self.z_max - self.z_min
    }
}

/// WGS84 lng/lat → Web-Mercator tile index (XYZ / slippy-map scheme).
pub fn lng_lat_to_tile(lng: f64, lat: f64, z: u8) -> TileId {
    let n = 2f64.powi(i32::from(z));
    let x = ((lng + 180.0) / 360.0 * n).floor().clamp(0.0, n - 1.0) as u32;

    let lat_rad = lat.to_radians();
    let y = ((1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / PI) / 2.0 * n)
        .floor()
        .clamp(0.0, n - 1.0) as u32;

    TileId::new(z, x, y)
}

/// World-space origin (min corner) of a tile in [`WORLD_ZOOM`] units.
pub fn tile_world_origin(tile: TileId) -> [f64; 2] {
    let span = world_units_per_tile(tile.z);
    [f64::from(tile.x) * span, f64::from(tile.y) * span]
}

/// World-space center of a lng/lat at [`WORLD_ZOOM`] (camera space).
pub fn lng_lat_to_world_center(lng: f64, lat: f64, z: u8) -> [f64; 2] {
    let tile = lng_lat_to_tile(lng, lat, z);
    let [ox, oz] = tile_world_origin(tile);
    let half = world_units_per_tile(z) * 0.5;
    [ox + half, oz + half]
}

/// All tiles at zoom `z` whose bounds intersect a [`WORLD_ZOOM`] ground rectangle.
pub fn tiles_in_world_rect(rect: WorldRect, z: u8) -> Vec<TileId> {
    if !rect.x_min.is_finite()
        || !rect.x_max.is_finite()
        || !rect.z_min.is_finite()
        || !rect.z_max.is_finite()
        || rect.x_min > rect.x_max
        || rect.z_min > rect.z_max
    {
        return Vec::new();
    }

    let span = world_units_per_tile(z);
    let max_index = (1u32 << z).saturating_sub(1);
    let x_min = (rect.x_min / span).floor().clamp(0.0, f64::from(max_index)) as u32;
    let x_max = (rect.x_max / span).floor().clamp(0.0, f64::from(max_index)) as u32;
    let y_min = (rect.z_min / span).floor().clamp(0.0, f64::from(max_index)) as u32;
    let y_max = (rect.z_max / span).floor().clamp(0.0, f64::from(max_index)) as u32;

    if x_min > x_max || y_min > y_max {
        return Vec::new();
    }

    let mut tiles = Vec::new();
    for x in x_min..=x_max {
        for y in y_min..=y_max {
            tiles.push(TileId::new(z, x, y));
        }
    }
    tiles
}

/// Visible tiles for a ground rectangle, with one tile of padding on each side.
pub fn visible_tiles_for_rect(rect: WorldRect, z: u8) -> Vec<TileId> {
    tiles_in_world_rect(rect.expand(world_units_per_tile(z)), z)
}

fn map_point_to_world(point: [f64; 2], scale: f64, shift: [f64; 2]) -> [f64; 2] {
    [point[0] * scale + shift[0], point[1] * scale + shift[1]]
}

fn offset_road(road: &RoadFeature, scale: f64, shift: [f64; 2]) -> RoadFeature {
    RoadFeature {
        id: road.id,
        class: road.class.clone(),
        source_layer: road.source_layer.clone(),
        points_tile: road
            .points_tile
            .iter()
            .map(|p| map_point_to_world(*p, scale, shift))
            .collect(),
    }
}

fn offset_polygon(polygon: &PolygonFeature, scale: f64, shift: [f64; 2]) -> PolygonFeature {
    PolygonFeature {
        id: polygon.id,
        class: polygon.class.clone(),
        source_layer: polygon.source_layer.clone(),
        rings: polygon
            .rings
            .iter()
            .map(|ring| PolygonRing {
                points: ring
                    .points
                    .iter()
                    .map(|p| map_point_to_world(*p, scale, shift))
                    .collect(),
                role: ring.role,
            })
            .collect(),
        // Heights are already authored in WORLD_ZOOM units at decode/map time.
        height: polygon.height,
        min_height: polygon.min_height,
    }
}

/// Merge multiple tiles into one scene, shifting geometry into shared world space.
pub fn merge_tiles_into_scene(tiles: &[&TileSceneData]) -> TileSceneData {
    merge_tiles_into_scene_relative(tiles, [0.0, 0.0])
}

/// Merge tiles into camera-relative [`WORLD_ZOOM`] space: `world - origin`.
///
/// Coarser-zoom tile local units are scaled by [`tile_to_world_scale`] first.
pub fn merge_tiles_into_scene_relative(
    tiles: &[&TileSceneData],
    origin: [f64; 2],
) -> TileSceneData {
    let mut backgrounds = Vec::new();
    let mut buildings = Vec::new();
    let mut roads = Vec::new();

    for tile in tiles {
        let scale = tile_to_world_scale(tile.tile_id.z);
        let tile_origin = tile_world_origin(tile.tile_id);
        let shift = [tile_origin[0] - origin[0], tile_origin[1] - origin[1]];
        for layer in &tile.layers {
            match &layer.payload {
                LayerPayload::Background(bg) => {
                    for polygon in bg {
                        backgrounds.push(offset_polygon(polygon, scale, shift));
                    }
                }
                LayerPayload::Buildings(bldg) => {
                    for polygon in bldg {
                        buildings.push(offset_polygon(polygon, scale, shift));
                    }
                }
                LayerPayload::Roads(tile_roads) => {
                    for road in tile_roads {
                        roads.push(offset_road(road, scale, shift));
                    }
                }
                LayerPayload::Empty => {}
            }
        }
    }

    TileSceneData {
        tile_id: TileId::new(0, 0, 0),
        layers: vec![
            TileLayerData {
                kind: LayerKind::Background,
                payload: LayerPayload::Background(backgrounds),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valencia_tile_at_zoom_14() {
        let tile = lng_lat_to_tile(VALENCIA.lng, VALENCIA.lat, 14);
        assert_eq!(tile.z, 14);
        assert_eq!(tile.x, 8174);
        assert_eq!(tile.y, 6234);
    }

    #[test]
    fn valencia_tile_at_zoom_10() {
        let tile = lng_lat_to_tile(VALENCIA.lng, VALENCIA.lat, 10);
        assert_eq!(tile.z, 10);
        assert_eq!(tile.x, 510);
        assert_eq!(tile.y, 389);
    }

    #[test]
    fn tile_world_origin_uses_world_zoom_scale() {
        let tile = TileId::new(10, 511, 389);
        let [ox, oz] = tile_world_origin(tile);
        let span = TILE_EXTENT * 16.0; // 2^(14-10)
        assert_eq!(ox, 511.0 * span);
        assert_eq!(oz, 389.0 * span);
    }

    #[test]
    fn tiles_in_world_rect_covers_single_tile() {
        let tile = TileId::new(10, 2, 3);
        let [ox, oz] = tile_world_origin(tile);
        let rect = WorldRect {
            x_min: ox + 100.0,
            x_max: ox + 200.0,
            z_min: oz + 100.0,
            z_max: oz + 200.0,
        };

        let tiles = tiles_in_world_rect(rect, 10);
        assert_eq!(tiles, vec![tile]);
    }

    #[test]
    fn wider_view_picks_coarser_zoom() {
        let near = tile_zoom_for_ground_width(TILE_EXTENT * 2.5, 14);
        assert_eq!(near, 14);

        let far = tile_zoom_for_ground_width(TILE_EXTENT * 40.0, 14);
        assert!(far < 14);
        assert!(far >= MIN_TILE_ZOOM);
    }

    #[test]
    fn parent_and_child_share_aligned_origins() {
        // z14 child (8174,6234) sits inside z10 parent (510,389): 8174>>4 == 510.
        let child = TileId::new(14, 8174, 6234);
        let parent = TileId::new(10, 510, 389);
        let [cx, cz] = tile_world_origin(child);
        let [px, pz] = tile_world_origin(parent);
        assert!(cx >= px && cx < px + world_units_per_tile(10));
        assert!(cz >= pz && cz < pz + world_units_per_tile(10));
    }
}
