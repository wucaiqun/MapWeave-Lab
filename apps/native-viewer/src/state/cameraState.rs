use mw_core::{
    lng_lat_to_world_center, tiles_in_world_rect, TileId, WorldRect, DEFAULT_ZOOM, TILE_EXTENT,
    VALENCIA,
};

/// Extra margin beyond the screen footprint (fraction of visible ground width).
const SCREEN_MARGIN_FRACTION: f64 = 0.12;

/// 3D orbit camera over the map plane (world XZ, Y up).
#[derive(Debug, Clone)]
pub struct CameraState {
    /// Look-at point on the map plane (world X, 0, world Z).
    target: glam::Vec3,
    /// Azimuth around world Y (radians).
    yaw: f32,
    /// Elevation above the horizon (radians, clamped).
    pitch: f32,
    /// Distance from target to eye.
    distance: f32,
    /// Slippy-map zoom level used for tile selection.
    zoom: u8,
    fov_y: f32,
    near: f32,
    far: f32,
    viewport_width: f32,
    viewport_height: f32,
}

impl CameraState {
    pub fn new(viewport_width: u32, viewport_height: u32) -> Self {
        let [cx, cz] = lng_lat_to_world_center(VALENCIA.lng, VALENCIA.lat, DEFAULT_ZOOM);
        Self {
            target: glam::Vec3::new(cx as f32, 0.0, cz as f32),
            yaw: 0.7,
            pitch: 0.65,
            distance: 9_000.0,
            zoom: DEFAULT_ZOOM,
            fov_y: 45.0_f32.to_radians(),
            near: 1.0,
            far: 50_000.0,
            viewport_width: viewport_width.max(1) as f32,
            viewport_height: viewport_height.max(1) as f32,
        }
    }

    pub fn resize(&mut self, viewport_width: u32, viewport_height: u32) {
        self.viewport_width = viewport_width.max(1) as f32;
        self.viewport_height = viewport_height.max(1) as f32;
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.viewport_width as u32, self.viewport_height as u32);
    }

    pub fn eye_position(&self) -> glam::Vec3 {
        let cos_pitch = self.pitch.cos();
        let sin_pitch = self.pitch.sin();
        let sin_yaw = self.yaw.sin();
        let cos_yaw = self.yaw.cos();

        let offset = glam::Vec3::new(
            cos_pitch * sin_yaw,
            sin_pitch,
            cos_pitch * cos_yaw,
        );

        self.target + offset * self.distance
    }

    pub fn view(&self) -> glam::Mat4 {
        glam::Mat4::look_at_rh(self.eye_position(), self.target, glam::Vec3::Y)
    }

    pub fn projection(&self) -> glam::Mat4 {
        let aspect = self.viewport_width / self.viewport_height;
        glam::Mat4::perspective_rh(self.fov_y, aspect, self.near, self.far)
    }

    pub fn view_proj(&self) -> glam::Mat4 {
        self.projection() * self.view()
    }

    pub fn view_proj_cols(&self) -> [[f32; 4]; 4] {
        self.view_proj().to_cols_array_2d()
    }

    pub fn target(&self) -> glam::Vec3 {
        self.target
    }

    /// Unproject a screen pixel to the Y=0 ground plane.
    pub fn unproject_to_ground(&self, screen_x: f32, screen_y: f32) -> Option<glam::Vec3> {
        let inv = self.view_proj().inverse();
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

        Some(near + dir * t)
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
            if let Some(p) = self.unproject_to_ground(sx, sy) {
                points.push(self.clamp_to_screen_ground_extent(p));
            }
        }

        if let Some(rect) = WorldRect::from_points(&points) {
            return self.expand_screen_margin(rect);
        }

        self.expand_screen_margin(self.fallback_ground_rect())
    }

    /// Tiles intersecting the screen footprint (+ small margin), derived from viewport size.
    pub fn visible_tiles(&self) -> Vec<TileId> {
        tiles_in_world_rect(self.ground_bbox(), self.zoom)
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
    fn clamp_to_screen_ground_extent(&self, p: glam::Vec3) -> [f64; 2] {
        let max_half = self.max_ground_half_extent();
        let dx = f64::from(p.x - self.target.x);
        let dz = f64::from(p.z - self.target.z);
        let dist = (dx * dx + dz * dz).sqrt();
        if dist > max_half && dist > f64::EPSILON {
            let scale = max_half / dist;
            [
                f64::from(self.target.x) + dx * scale,
                f64::from(self.target.z) + dz * scale,
            ]
        } else {
            [f64::from(p.x), f64::from(p.z)]
        }
    }

    fn expand_screen_margin(&self, rect: WorldRect) -> WorldRect {
        let width = rect.x_max - rect.x_min;
        let height = rect.z_max - rect.z_min;
        let margin = width.max(height) * SCREEN_MARGIN_FRACTION;
        rect.expand(margin.max(TILE_EXTENT * 0.25))
    }

    fn fallback_ground_rect(&self) -> WorldRect {
        let half = self.max_ground_half_extent();
        let cx = f64::from(self.target.x);
        let cz = f64::from(self.target.z);
        WorldRect {
            x_min: cx - half,
            x_max: cx + half,
            z_min: cz - half,
            z_max: cz + half,
        }
    }

    /// Left-drag: orbit around the target.
    pub fn orbit_screen_pixels(&mut self, dx: f32, dy: f32) {
        const ORBIT_SENSITIVITY: f32 = 0.005;
        self.yaw -= dx * ORBIT_SENSITIVITY;
        self.pitch = (self.pitch - dy * ORBIT_SENSITIVITY).clamp(0.12, 1.45);
    }

    /// Right-drag / middle-drag: pan target on the map plane.
    pub fn pan_screen_pixels(&mut self, dx: f32, dy: f32) {
        let eye = self.eye_position();
        let forward = (self.target - eye).normalize();
        let mut right = forward.cross(glam::Vec3::Y);
        if right.length_squared() < f32::EPSILON {
            right = glam::Vec3::X;
        } else {
            right = right.normalize();
        }

        let mut forward_ground = glam::Vec3::new(-forward.x, 0.0, -forward.z);
        if forward_ground.length_squared() < f32::EPSILON {
            forward_ground = glam::Vec3::NEG_Z;
        } else {
            forward_ground = forward_ground.normalize();
        }

        let scale = self.distance * 0.0012;
        self.target += right * (-dx * scale) + forward_ground * (dy * scale);
    }

    pub fn zoom_by_factor(&mut self, factor: f32) {
        const MIN_DISTANCE: f32 = 400.0;
        const MAX_DISTANCE: f32 = 30_000.0;
        self.distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }
}
