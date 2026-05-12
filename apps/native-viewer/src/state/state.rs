use super::{CameraState, RenderState, SceneState};

pub struct State {
    pub render: RenderState,
    pub scene: SceneState,
    pub camera: CameraState,
}

impl State {
    pub fn new(window: std::sync::Arc<winit::window::Window>) -> anyhow::Result<Self> {
        Ok(Self {
            render: RenderState::new(window)?,
            scene: SceneState::new(),
            camera: CameraState::new(),
        })
    }

    pub fn resize(&mut self, window_size: winit::dpi::PhysicalSize<u32>) {
        self.render.resize(window_size);
    }

    pub fn render(&mut self) {
        self.render.render();
    }

    pub fn update(&mut self) {
        self.camera.update();
    }

    pub fn update_camera(&mut self, camera: CameraState) {
        self.camera = camera;
    }
}
