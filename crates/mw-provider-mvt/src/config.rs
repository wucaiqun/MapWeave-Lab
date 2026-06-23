/// Which MVT schema / source-layer names to expect when mapping tiles to scene data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MvtSourceProfile {
    /// OpenMapTiles schema (OpenFreeMap, self-hosted Planetiler output, etc.).
    OpenMapTiles,
    /// MapLibre demo tiles — coarse country polygons, max zoom 6 only.
    MapLibreDemo,
}

#[derive(Debug, Clone)]
pub struct MvtProviderConfig {
    /// Slippy-map URL template with `{z}`, `{x}`, `{y}` placeholders.
    pub endpoint_template: String,
    /// When set, `resolve()` fetches TileJSON and overwrites `endpoint_template`.
    pub tilejson_url: Option<String>,
    pub source_profile: MvtSourceProfile,
    pub access_token: String,
    pub cache_dir: Option<String>,
}

impl MvtProviderConfig {
    /// OpenStreetMap vector tiles via OpenFreeMap (no API key, zoom 0–14).
    pub fn openfreemap() -> Self {
        Self {
            endpoint_template: String::new(),
            tilejson_url: Some("https://tiles.openfreemap.org/planet".to_string()),
            source_profile: MvtSourceProfile::OpenMapTiles,
            access_token: String::new(),
            cache_dir: Some("./data-cache-openfreemap".to_string()),
        }
    }

    /// MapLibre global demo — country outlines only, max zoom 6.
    pub fn demotiles() -> Self {
        Self {
            endpoint_template: "https://demotiles.maplibre.org/tiles/{z}/{x}/{y}.pbf".to_string(),
            tilejson_url: None,
            source_profile: MvtSourceProfile::MapLibreDemo,
            access_token: String::new(),
            cache_dir: Some("./data-cache-root".to_string()),
        }
    }

    pub fn default() -> Self {
        Self::openfreemap()
    }
}
