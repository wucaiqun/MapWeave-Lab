mod config;
mod perf;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use std::fmt::Display;

pub use config::{LogConfig, LogLevel};
pub use perf::{elapsed_ms, print_perf, print_perf_if, FramePerfMonitor, PERF_LOG_THRESHOLD_MS};
#[cfg(not(target_arch = "wasm32"))]
pub use native::init_logging;
#[cfg(target_arch = "wasm32")]
pub use web::init_logging;

pub fn print_error(message: impl Display) {
    log::error!("{message}");
}

pub fn print_info(message: impl Display) {
    log::info!("{message}");
}
