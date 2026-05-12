#[derive(Debug, Clone)]
pub struct CameraState {
    position: glam::Vec3,
    target: glam::Vec3,
    up: glam::Vec3,
    fov: f32,
    aspect_ratio: f32,
    near: f32,
    far: f32,
}

impl CameraState {
    pub fn new() -> Self {
        Self {
            position: glam::Vec3::new(0.0, 0.0, 0.0),
            target: glam::Vec3::new(0.0, 0.0, 0.0),
            up: glam::Vec3::new(0.0, 1.0, 0.0),
            fov: 45.0,
            aspect_ratio: 1.0,
            near: 0.1,
            far: 100.0,
        }
    }

    pub fn update(&mut self) {
        //self.position = self.target + self.up;
    }
}