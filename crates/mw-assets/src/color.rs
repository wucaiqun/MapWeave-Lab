use std::collections::HashMap;

use anyhow::Context;
use serde::Deserialize;

/// Linear RGBA in 0..1 (same convention as layer `fill_color` fields).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorRgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl ColorRgba {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

/// Named colors loaded from JSON.
///
/// Example:
/// ```json
/// {
///   "clear": [0.02, 0.02, 0.08, 1.0],
///   "buildings": [0.72, 0.70, 0.66, 1.0]
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct ColorTable {
    colors: HashMap<String, ColorRgba>,
}

#[derive(Debug, Deserialize)]
struct ColorTableFile {
    #[serde(flatten)]
    colors: HashMap<String, [f32; 4]>,
}

impl ColorTable {
    pub fn from_json_slice(bytes: &[u8]) -> anyhow::Result<Self> {
        let file: ColorTableFile =
            serde_json::from_slice(bytes).context("color table JSON")?;
        let mut colors = HashMap::with_capacity(file.colors.len());
        for (name, rgba) in file.colors {
            colors.insert(
                name,
                ColorRgba::new(rgba[0], rgba[1], rgba[2], rgba[3]),
            );
        }
        Ok(Self { colors })
    }

    pub fn get(&self, name: &str) -> Option<ColorRgba> {
        self.colors.get(name).copied()
    }

    pub fn get_or(&self, name: &str, fallback: ColorRgba) -> ColorRgba {
        self.get(name).unwrap_or(fallback)
    }

    pub fn insert(&mut self, name: impl Into<String>, color: ColorRgba) {
        self.colors.insert(name.into(), color);
    }

    pub fn len(&self) -> usize {
        self.colors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_color_table() {
        let json = br#"{
            "clear": [0.1, 0.2, 0.3, 1.0],
            "buildings": [0.7, 0.7, 0.6, 1.0]
        }"#;
        let table = ColorTable::from_json_slice(json).unwrap();
        let c = table.get("clear").unwrap();
        assert!((c.r - 0.1).abs() < 1e-6);
        assert_eq!(table.len(), 2);
    }
}
