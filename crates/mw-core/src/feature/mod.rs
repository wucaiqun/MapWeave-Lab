
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RingRole {
    Exterior,
    Hole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolygonRing {
    pub points: Vec<[f64; 2]>,
    pub role: RingRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoadFeature {
    pub id: u64,
    pub class: String,
    pub source_layer: String,
    pub points_tile: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolygonFeature {
    pub id: u64,
    pub class: String,
    pub source_layer: String,
    pub rings: Vec<PolygonRing>,
}
