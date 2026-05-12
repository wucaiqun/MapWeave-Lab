use mw_telemetry::{init_logging, LogConfig};

#[cfg(target_arch = "wasm32")]
pub fn start() {
    let _ = init_logging(LogConfig::default());
    log::info!("web viewer starting");

    // Placeholder: initialize web canvas + wgpu in wasm.
    // Keep this entry minimal until the render loop lands.
}
