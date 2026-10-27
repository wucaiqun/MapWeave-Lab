use bytemuck::{Pod, Zeroable};

/// Per-frame camera values shared by map layers / post passes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct FrameUniforms {
    pub view_proj: [[f32; 4]; 4],
    /// Inverse of `view_proj` — used by [`crate::FogLayer`] to rebuild world pos from depth.
    pub inv_view_proj: [[f32; 4]; 4],
    /// Eye position in the same camera-relative space as uploaded meshes.
    pub camera_pos: [f32; 3],
    pub _pad0: f32,
    /// Horizon tint for fog.
    pub fog_color: [f32; 4],
    pub fog_start: f32,
    pub fog_end: f32,
    pub _pad1: [f32; 2],
}

impl Default for FrameUniforms {
    fn default() -> Self {
        Self {
            view_proj: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            inv_view_proj: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            camera_pos: [0.0, 0.0, 0.0],
            _pad0: 0.0,
            fog_color: [0.70, 0.78, 0.88, 1.0],
            fog_start: 4_000.0,
            fog_end: 14_000.0,
            _pad1: [0.0, 0.0],
        }
    }
}
