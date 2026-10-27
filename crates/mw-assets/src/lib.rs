//! CPU-side asset loading for MapWeave.
//!
//! Responsibilities:
//! - resolve an `assets/` root
//! - read bytes / decode images
//! - split horizontal-cross cubemaps into 6 faces
//! - load simple color tables (JSON)
//!
//! GPU upload stays in `mw-render-wgpu`. Async network tiles stay in
//! `mw-provider-mvt` / `mw-async-data`.

mod color;
mod cubemap;
mod image_data;
mod path;

pub use color::{ColorRgba, ColorTable};
pub use cubemap::{CubemapFaces, CubemapLayout};
pub use image_data::RgbaImageData;
pub use path::AssetServer;
