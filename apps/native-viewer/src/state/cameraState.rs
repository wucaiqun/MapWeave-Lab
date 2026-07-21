use mw_core::{
    lng_lat_to_world_center, tile_zoom_for_ground_width, tiles_in_world_rect, world_units_per_tile,
    TileId, WorldRect, DEFAULT_ZOOM, VALENCIA, WORLD_ZOOM,
};

/// Extra margin beyond the screen footprint (fraction of visible ground width).
const SCREEN_MARGIN_FRACTION: f64 = 0.12;

/// 3D orbit camera over the map plane (world XZ, Y up).
///
/// Target is stored in `f64` world units at [`WORLD_ZOOM`]. Rendering uses a
/// camera-relative `view_proj` so GPU `f32` math stays near the origin.
///
/// Tile fetch zoom (`zoom`) follows view width via [`tile_zoom_for_ground_width`].
#[derive(Debug, Clone)]
pub struct CameraState {
    /// Look-at point on the map plane (world X, world Z) in f64.
    target_x: f64,
    target_z: f64,
    /// Azimuth around world Y (radians).
    yaw: f32,
    /// Elevation above the horizon (radians, clamped).
    pitch: f32,
    /// Distance from target to eye.
    distance: f32,
    /// Slippy-map zoom level used for tile selection (LOD).
    zoom: u8,
    fov_y: f32,
    near: f32,
    far: f32,
    viewport_width: f32,
    viewport_height: f32,
}

impl CameraState {
    pub fn new(viewport_width: u32, viewport_height: u32) -> Self {
        let [cx, cz] = lng_lat_to_world_center(VALENCIA.lng, VALENCIA.lat, WORLD_ZOOM);
        let mut cam = Self {
            target_x: cx,
            target_z: cz,
            yaw: 0.7,
            pitch: 0.65,
            distance: 9_000.0,
            zoom: DEFAULT_ZOOM,
            fov_y: 45.0_f32.to_radians(),
            near: 1.0,
            far: 80_000.0,
            viewport_width: viewport_width.max(1) as f32,
            viewport_height: viewport_height.max(1) as f32,
        };
        cam.refresh_tile_zoom();
        cam
    }

    pub fn resize(&mut self, viewport_width: u32, viewport_height: u32) {
        self.viewport_width = viewport_width.max(1) as f32;
        self.viewport_height = viewport_height.max(1) as f32;
        self.refresh_tile_zoom();
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.viewport_width as u32, self.viewport_height as u32);
    }

    /// World-space look-at on the ground plane (Y = 0).
    pub fn target_world(&self) -> [f64; 2] {
        [self.target_x, self.target_z]
    }

    /// Eye position relative to the look-at target (camera-local space).
    fn eye_offset(&self) -> glam::Vec3 {
        let cos_pitch = self.pitch.cos();
        let sin_pitch = self.pitch.sin();
        let sin_yaw = self.yaw.sin();
        let cos_yaw = self.yaw.cos();

        glam::Vec3::new(
            cos_pitch * sin_yaw,
            sin_pitch,
            cos_pitch * cos_yaw,
        ) * self.distance
    }

    /// View-projection for geometry uploaded as `world - mesh_origin`.
    ///
    /// When `mesh_origin == target`, look-at is at the relative origin.
    /// A small pan residual is absorbed here so we need not re-upload every frame.
    pub fn view_proj_relative_to(&self, mesh_origin: [f64; 2]) -> glam::Mat4 {
        let residual = glam::Vec3::new(
            (self.target_x - mesh_origin[0]) as f32,
            0.0,
            (self.target_z - mesh_origin[1]) as f32,
        );
        let eye = residual + self.eye_offset();
        let view = glam::Mat4::look_at_rh(eye, residual, glam::Vec3::Y);
        let aspect = self.viewport_width / self.viewport_height;
        let proj = glam::Mat4::perspective_rh(self.fov_y, aspect, self.near, self.far);
        proj * view
    }

    pub fn view_proj_cols_for(&self, mesh_origin: [f64; 2]) -> [[f32; 4]; 4] {
        self.view_proj_relative_to(mesh_origin).to_cols_array_2d()
    }

    pub fn zoom(&self) -> u8 {
        self.zoom
    }

    pub fn distance(&self) -> f32 {
        self.distance
    }

    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    pub fn pitch(&self) -> f32 {
        self.pitch
    }

    /// Update tile LOD from the current ground footprint width.
    pub fn refresh_tile_zoom(&mut self) {
        let width = self.ground_width();
        self.zoom = tile_zoom_for_ground_width(width, self.zoom);
    }

    /// Approximate visible ground width (screen bbox, with FOV fallback).
    fn ground_width(&self) -> f64 {
        let bbox = self.ground_bbox();
        bbox.width().max(self.max_ground_half_extent() * 2.0)
    }

    /// Unproject a screen pixel to the Y=0 ground plane in camera-relative space,
    /// then convert back to world XZ.
    pub fn unproject_to_ground_world(&self, screen_x: f32, screen_y: f32) -> Option<[f64; 2]> {
        let origin = self.target_world();
        let inv = self.view_proj_relative_to(origin).inverse();
        let ndc_x = (screen_x / self.viewport_width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (screen_y / self.viewport_height) * 2.0;

        let near_h = inv * glam::Vec4::new(ndc_x, ndc_y, 0.0, 1.0);
        let far_h = inv * glam::Vec4::new(ndc_x, ndc_y, 1.0, 1.0);
        if near_h.w.abs() < f32::EPSILON || far_h.w.abs() < f32::EPSILON {
            return None;
        }

        let near = near_h.truncate() / near_h.w;
        let far = far_h.truncate() / far_h.w;
        let dir = far - near;

        if dir.y.abs() < f32::EPSILON {
            return None;
        }

        let t = -near.y / dir.y;
        if t < 0.0 {
            return None;
        }

        let local = near + dir * t;
        Some([
            origin[0] + f64::from(local.x),
            origin[1] + f64::from(local.z),
        ])
    }

    /// Ground-plane footprint of the viewport via screen-corner unprojection.
    pub fn ground_bbox(&self) -> WorldRect {
        let corners = [
            (0.0, 0.0),
            (self.viewport_width, 0.0),
            (self.viewport_width, self.viewport_height),
            (0.0, self.viewport_height),
        ];

        let mut points = Vec::with_capacity(corners.len());
        for (sx, sy) in corners {
            if let Some(p) = self.unproject_to_ground_world(sx, sy) {
                points.push(self.clamp_to_screen_ground_extent(p));
            }
        }

        if let Some(rect) = WorldRect::from_points(&points) {
            return self.expand_screen_margin(rect);
        }

        self.expand_screen_margin(self.fallback_ground_rect())
    }

    /// Tiles intersecting the screen footprint (+ small margin), at current LOD zoom.
    pub fn visible_tiles(&self) -> Vec<TileId> {
        let span = world_units_per_tile(self.zoom);
        let tiles = tiles_in_world_rect(self.ground_bbox(), self.zoom);
        if !tiles.is_empty() {
            return tiles;
        }

        // Degenerate unprojection (horizon / NaN): fall back to the tile under the target.
        let tx = (self.target_x / span).floor().max(0.0) as u32;
        let ty = (self.target_z / span).floor().max(0.0) as u32;
        let max_index = (1u32 << self.zoom).saturating_sub(1);
        vec![TileId::new(
            self.zoom,
            tx.min(max_index),
            ty.min(max_index),
        )]
    }

    /// Horizontal half-extent of visible ground from viewport FOV, aspect, distance, and pitch.
    fn max_ground_half_extent(&self) -> f64 {
        let aspect = f64::from(self.viewport_width / self.viewport_height);
        let half_v = f64::from(self.fov_y * 0.5).tan();
        let half_h = half_v * aspect;
        let pitch_cos = f64::from(self.pitch.cos().max(0.12));
        f64::from(self.distance) * half_h / pitch_cos
    }

    /// Keep corner rays inside the screen-derived ground range (steep pitch can hit very far away).
    fn clamp_to_screen_ground_extent(&self, p: [f64; 2]) -> [f64; 2] {
        let max_half = self.max_ground_half_extent();
        let dx = p[0] - self.target_x;
        let dz = p[1] - self.target_z;
        let dist = (dx * dx + dz * dz).sqrt();
        if dist > max_half && dist > f64::EPSILON {
            let scale = max_half / dist;
            [self.target_x + dx * scale, self.target_z + dz * scale]
        } else {
            p
        }
    }

    fn expand_screen_margin(&self, rect: WorldRect) -> WorldRect {
        let width = rect.x_max - rect.x_min;
        let height = rect.z_max - rect.z_min;
        let margin = width.max(height) * SCREEN_MARGIN_FRACTION;
        let min_margin = world_units_per_tile(self.zoom) * 0.25;
        rect.expand(margin.max(min_margin))
    }

    fn fallback_ground_rect(&self) -> WorldRect {
        let half = self.max_ground_half_extent();
        WorldRect {
            x_min: self.target_x - half,
            x_max: self.target_x + half,
            z_min: self.target_z - half,
            z_max: self.target_z + half,
        }
    }

    /// Right-drag: orbit around the target.
    pub fn orbit_screen_pixels(&mut self, dx: f32, dy: f32) {
        const ORBIT_SENSITIVITY: f32 = 0.005;
        self.yaw += dx * ORBIT_SENSITIVITY;
        self.pitch = (self.pitch + dy * ORBIT_SENSITIVITY).clamp(0.12, 1.45);
        self.refresh_tile_zoom();
    }

    /// Left-drag: pan on the ground plane so the map follows the cursor (grab-to-scroll).
    pub fn pan_screen_pixels(&mut self, dx: f32, dy: f32) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }

        // Compare ground under screen-center vs center+(dx,dy); moving the target by
        // (from - to) keeps the grabbed point glued to the cursor on both axes.
        let cx = self.viewport_width * 0.5;
        let cy = self.viewport_height * 0.5;
        let Some(from) = self.unproject_to_ground_world(cx, cy) else {
            return;
        };
        let Some(to) = self.unproject_to_ground_world(cx + dx, cy + dy) else {
            return;
        };

        self.target_x += from[0] - to[0];
        self.target_z += from[1] - to[1];
    }

    pub fn zoom_by_factor(&mut self, factor: f32) {
        const MIN_DISTANCE: f32 = 400.0;
        const MAX_DISTANCE: f32 = 80_000.0;
        self.distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
        self.refresh_tile_zoom();
    }
}
