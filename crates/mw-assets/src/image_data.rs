use anyhow::Context;

/// CPU RGBA8 image (row-major, 4 bytes per pixel).
#[derive(Debug, Clone)]
pub struct RgbaImageData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl RgbaImageData {
    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        let image = image::load_from_memory(bytes)
            .context("image decode failed")?
            .into_rgba8();
        let (width, height) = image.dimensions();
        Ok(Self {
            width,
            height,
            rgba: image.into_raw(),
        })
    }

    pub fn pixel_count(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }
}
