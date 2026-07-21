#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderStats {
    pub draw_calls: u32,
    pub triangles: u32,
}

impl RenderStats {
    pub fn merge(&mut self, other: Self) {
        self.draw_calls += other.draw_calls;
        self.triangles += other.triangles;
    }

    pub fn from_triangle_list_vertices(vertex_count: u32) -> Self {
        Self {
            draw_calls: 1,
            triangles: vertex_count / 3,
        }
    }
}
