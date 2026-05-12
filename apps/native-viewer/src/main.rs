use mw_telemetry::{init_logging, LogConfig};
use std::sync::Arc;
use winit::{
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowAttributes},
    application::ApplicationHandler,
};

mod state;
use crate::state::State;

struct App {
    window: Option<Arc<Window>>,
    state: Option<State>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attrs = WindowAttributes::default()
                .with_title("MapWeave Lab")
                .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));

            let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
            match State::new(window.clone()) {
                Ok(state) => {
                    self.window = Some(window);
                    self.state = Some(state);
                }
                Err(err) => {
                    log::error!("failed to initialize state: {err}");
                    event_loop.exit();
                }
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::Resized(size) => {
                if let Some(state) = self.state.as_mut() {
                    state.resize(size);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(state) = self.state.as_mut() {
                    state.update();
                    state.render();
                }
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }
}

fn main() {
    let _ = init_logging(LogConfig::default());
    log::info!("native viewer starting");

    let event_loop = EventLoop::new().expect("create event loop");

    let mut app = App {
        window: None,
        state: None,
    };

    event_loop.run_app(&mut app).expect("run app");
}
