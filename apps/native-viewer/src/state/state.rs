use super::{CameraState, RenderState, SceneState};
use mw_render_wgpu::FrameUniforms;
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{Key, NamedKey},
};

pub struct State {
    pub render: RenderState,
    pub scene: SceneState,
    pub camera: CameraState,
    orbit_drag: bool,
    pan_drag: bool,
    last_cursor: Option<PhysicalPosition<f64>>,
}

impl State {
    pub fn new(window: std::sync::Arc<winit::window::Window>) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let render = RenderState::new(window)?;
        let mut scene = SceneState::new()?;

        scene.prepare(&render.device(), render.surface_format())?;
        scene.upload(render.device(), render.queue())?;

        Ok(Self {
            render,
            scene,
            camera: CameraState::new(size.width, size.height),
            orbit_drag: false,
            pan_drag: false,
            last_cursor: None,
        })
    }

    pub fn resize(&mut self, window_size: winit::dpi::PhysicalSize<u32>) {
        self.render.resize(window_size);
        self.camera.resize(window_size.width, window_size.height);
    }

    pub fn handle_window_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = *state == ElementState::Pressed;
                match button {
                    MouseButton::Left => {
                        self.orbit_drag = pressed;
                        if !pressed {
                            self.last_cursor = None;
                        }
                    }
                    MouseButton::Right | MouseButton::Middle => {
                        self.pan_drag = pressed;
                        if !pressed {
                            self.last_cursor = None;
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if !(self.orbit_drag || self.pan_drag) {
                    return;
                }

                if let Some(last) = self.last_cursor {
                    let dx = (position.x - last.x) as f32;
                    let dy = (position.y - last.y) as f32;

                    if self.orbit_drag {
                        self.camera.orbit_screen_pixels(dx, dy);
                    } else if self.pan_drag {
                        self.camera.pan_screen_pixels(dx, dy);
                    }
                }
                self.last_cursor = Some(*position);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll_y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y / 120.0) as f32,
                };

                if scroll_y > 0.0 {
                    self.camera.zoom_by_factor(0.9);
                } else if scroll_y < 0.0 {
                    self.camera.zoom_by_factor(1.0 / 0.9);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }

                match event.logical_key {
                    Key::Named(NamedKey::ArrowLeft) => self.camera.pan_screen_pixels(-48.0, 0.0),
                    Key::Named(NamedKey::ArrowRight) => self.camera.pan_screen_pixels(48.0, 0.0),
                    Key::Named(NamedKey::ArrowUp) => self.camera.pan_screen_pixels(0.0, -48.0),
                    Key::Named(NamedKey::ArrowDown) => self.camera.pan_screen_pixels(0.0, 48.0),
                    Key::Named(NamedKey::PageUp) => self.camera.orbit_screen_pixels(0.0, -32.0),
                    Key::Named(NamedKey::PageDown) => self.camera.orbit_screen_pixels(0.0, 32.0),
                    Key::Character(ref c) if c.as_str() == "=" || c.as_str() == "+" => {
                        self.camera.zoom_by_factor(0.9);
                    }
                    Key::Character(ref c) if c.as_str() == "-" => {
                        self.camera.zoom_by_factor(1.0 / 0.9);
                    }
                    Key::Character(ref c) if c.as_str() == "0" => {
                        self.camera.reset();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub fn render(&mut self) {
        let frame = FrameUniforms {
            view_proj: self.camera.view_proj_cols(),
        };
        self.render
            .render(&self.scene.renderer, self.scene.clear_color(), &frame);
    }
}
