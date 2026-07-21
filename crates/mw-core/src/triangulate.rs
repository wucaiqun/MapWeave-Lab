use crate::{PolygonFeature, RingRole};

/// 2D positions plus triangle indices (earcut output).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriangulatedMesh2d {
    pub positions: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// Triangulate polygon features with ear clipping (concave + holes).
pub fn triangulate_polygon_features(polygons: &[PolygonFeature]) -> TriangulatedMesh2d {
    let mut mesh = TriangulatedMesh2d::default();

    for polygon in polygons {
        for (exterior, holes) in ring_groups(polygon) {
            if let Some(part) = triangulate_ring_group(&exterior, &holes) {
                let base = mesh.positions.len() as u32;
                mesh.positions.extend(part.positions);
                mesh.indices.extend(part.indices.into_iter().map(|i| i + base));
            }
        }
    }

    mesh
}

fn ring_groups(polygon: &PolygonFeature) -> Vec<(Vec<[f64; 2]>, Vec<Vec<[f64; 2]>>)> {
    let mut groups = Vec::new();

    for ring in &polygon.rings {
        if ring.points.len() < 3 {
            continue;
        }

        match ring.role {
            RingRole::Exterior => groups.push((ring.points.clone(), Vec::new())),
            RingRole::Hole => {
                if let Some((_, holes)) = groups.last_mut() {
                    holes.push(ring.points.clone());
                }
            }
        }
    }

    groups
}

fn triangulate_ring_group(
    exterior: &[[f64; 2]],
    holes: &[Vec<[f64; 2]>],
) -> Option<TriangulatedMesh2d> {
    if exterior.len() < 3 {
        return None;
    }

    let mut flat = Vec::with_capacity((exterior.len() + holes.iter().map(|h| h.len()).sum::<usize>()) * 2);
    for point in exterior {
        flat.push(point[0]);
        flat.push(point[1]);
    }

    let mut hole_indices = Vec::with_capacity(holes.len());
    for hole in holes {
        if hole.len() < 3 {
            continue;
        }
        hole_indices.push(flat.len() / 2);
        for point in hole {
            flat.push(point[0]);
            flat.push(point[1]);
        }
    }

    let indices: Vec<usize> = match earcutr::earcut(&flat, &hole_indices, 2) {
        Ok(indices) if !indices.is_empty() => indices,
        _ => return None,
    };

    let positions = flat
        .chunks_exact(2)
        .map(|chunk| [chunk[0] as f32, chunk[1] as f32])
        .collect();

    Some(TriangulatedMesh2d {
        positions,
        indices: indices.iter().map(|&i| i as u32).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PolygonRing;

    #[test]
    fn concave_l_shape_gets_more_than_one_triangle() {
        let polygon = PolygonFeature {
            id: 1,
            class: String::new(),
            source_layer: String::new(),
            rings: vec![PolygonRing {
                points: vec![
                    [0.0, 0.0],
                    [2.0, 0.0],
                    [2.0, 1.0],
                    [1.0, 1.0],
                    [1.0, 2.0],
                    [0.0, 2.0],
                ],
                role: RingRole::Exterior,
            }],
            height: 0.0,
            min_height: 0.0,
        };

        let mesh = triangulate_polygon_features(&[polygon]);
        assert!(mesh.indices.len() >= 6, "L-shape needs at least 2 triangles");
    }

    #[test]
    fn hole_cuts_interior() {
        let polygon = PolygonFeature {
            id: 1,
            class: String::new(),
            source_layer: String::new(),
            rings: vec![
                PolygonRing {
                    points: vec![
                        [0.0, 0.0],
                        [4.0, 0.0],
                        [4.0, 4.0],
                        [0.0, 4.0],
                    ],
                    role: RingRole::Exterior,
                },
                PolygonRing {
                    points: vec![
                        [1.0, 1.0],
                        [3.0, 1.0],
                        [3.0, 3.0],
                        [1.0, 3.0],
                    ],
                    role: RingRole::Hole,
                },
            ],
            height: 0.0,
            min_height: 0.0,
        };

        let mesh = triangulate_polygon_features(&[polygon]);
        assert!(mesh.indices.len() >= 12, "square with hole needs multiple triangles");
    }
}
