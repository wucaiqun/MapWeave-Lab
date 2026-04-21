use serde::{Deserialize, Serialize};

use crate::{RoadFeature, TileId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LayerKind {
    Background,
    Roads,
    Buildings,
    Labels,
    Raster,
    Custom(u16),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundLayerData {
    pub color_rgba: [f32; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayerPayload {
    Background(BackgroundLayerData),
    Roads(Vec<RoadFeature>),
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileLayerData {
    pub kind: LayerKind,
    pub payload: LayerPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileSceneData {
    pub tile_id: TileId,
    pub layers: Vec<TileLayerData>,
}
