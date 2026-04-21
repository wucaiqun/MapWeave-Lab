#[derive(Debug, Clone)]
pub struct MvtProviderConfig {
    pub endpoint_template: String,
    pub access_token: String,
    pub cache_dir: Option<String>,
}

impl MvtProviderConfig {
    pub fn default() -> Self {
        Self {
            endpoint_template: "https://demotiles.maplibre.org/tiles/{z}/{x}/{y}.pbf".to_string(),
            access_token: "".to_string(),
            cache_dir: Some("./data-cache-root".to_string()),
        }
    }
}
