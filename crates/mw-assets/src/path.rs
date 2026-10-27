use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};

use crate::color::ColorTable;
use crate::cubemap::{CubemapFaces, CubemapLayout};
use crate::image_data::RgbaImageData;

/// Resolves paths under an asset root and loads common resource types.
#[derive(Debug, Clone)]
pub struct AssetServer {
    root: PathBuf,
}

impl AssetServer {
    /// Use an explicit asset root directory.
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Discover an asset root, in order:
    /// 1. `MW_ASSETS_DIR` environment variable
    /// 2. `./assets` relative to the current working directory
    /// 3. `../../assets` from this crate manifest (workspace `assets/` in `cargo run`)
    pub fn discover() -> anyhow::Result<Self> {
        if let Ok(dir) = env::var("MW_ASSETS_DIR") {
            let root = PathBuf::from(dir);
            if root.is_dir() {
                return Ok(Self { root });
            }
            bail!("MW_ASSETS_DIR is set but not a directory: {}", root.display());
        }

        let cwd_assets = env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("assets");
        if cwd_assets.is_dir() {
            return Ok(Self { root: cwd_assets });
        }

        // crates/mw-assets -> workspace root / assets
        let manifest_assets = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("assets");
        if manifest_assets.is_dir() {
            return Ok(Self {
                root: manifest_assets
                    .canonicalize()
                    .unwrap_or(manifest_assets),
            });
        }

        bail!(
            "could not find assets/ (set MW_ASSETS_DIR, or create ./assets, or workspace assets/)"
        );
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a path relative to the asset root.
    pub fn resolve(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root.join(relative)
    }

    pub fn read_bytes(&self, relative: impl AsRef<Path>) -> anyhow::Result<Vec<u8>> {
        let path = self.resolve(relative);
        fs::read(&path).with_context(|| format!("read asset {}", path.display()))
    }

    pub fn load_rgba8(&self, relative: impl AsRef<Path>) -> anyhow::Result<RgbaImageData> {
        let path = self.resolve(&relative);
        let bytes = self.read_bytes(relative)?;
        RgbaImageData::decode(&bytes)
            .with_context(|| format!("decode image {}", path.display()))
    }

    /// Load a horizontal-cross cubemap PNG/JPEG/WebP and split into 6 faces.
    pub fn load_cubemap_cross(
        &self,
        relative: impl AsRef<Path>,
        layout: CubemapLayout,
    ) -> anyhow::Result<CubemapFaces> {
        let image = self.load_rgba8(relative)?;
        CubemapFaces::from_cross(&image, layout)
    }

    /// Load a JSON color table (`{ "background": [r,g,b,a], ... }` with 0..1 floats).
    pub fn load_color_table(&self, relative: impl AsRef<Path>) -> anyhow::Result<ColorTable> {
        let path = self.resolve(&relative);
        let bytes = self.read_bytes(relative)?;
        ColorTable::from_json_slice(&bytes)
            .with_context(|| format!("parse color table {}", path.display()))
    }
}
