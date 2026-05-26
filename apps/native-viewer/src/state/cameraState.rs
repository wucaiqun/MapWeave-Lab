/// 3D orbit camera over the map plane (MVT tile XZ, Y up).
pub const TILE_EXTENT: f32 = 4096.0;

#[derive(Debug, Clone)]
pub struct CameraState {
    /// Look-at point on the map plane (tile X, 0, tile Z).
    target: glam::Vec3,
    /// Azimuth around world Y (radians). 
    yaw: f32,
    /// Elevation above the horizon (radians, clamped).
    pitch: f32,
    /// Distance from target to eye.
    distance: f32,
    fov_y: f32,
    near: f32,
    far: f32,
    viewport_width: f32,
    viewport_height: f32,
}

impl CameraState {
    pub fn new(viewport_width: u32, viewport_height: u32) -> Self {
        Self {
            target: glam::Vec3::new(TILE_EXTENT * 0.5, 0.0, TILE_EXTENT * 0.5),
            yaw: 0.7,
            pitch: 0.65,
            distance: 7200.0,
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
