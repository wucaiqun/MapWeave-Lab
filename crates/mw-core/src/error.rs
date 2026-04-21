#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("tile {z}/{x}/{y} not available")]
    TileUnavailable { z: u8, x: u32, y: u32 },
}
