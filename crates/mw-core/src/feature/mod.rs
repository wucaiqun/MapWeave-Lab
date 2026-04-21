
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoadFeature {
    pub id: u64,
    pub class: String,
    pub points_lon_lat: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolygonFeature {
    pub id: u64,
    pub class: String,
    pub points_lon_lat: Vec<[f64; 2]>,
}