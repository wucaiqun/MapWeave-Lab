mod config;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use std::fmt::Display;

pub use config::{LogConfig, LogLevel};
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
