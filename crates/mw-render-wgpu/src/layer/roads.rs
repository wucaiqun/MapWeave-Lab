use std::mem;

use bytemuck::{Pod, Zeroable};
use mw_core::{LayerKind, LayerPayload, RoadFeature, TileLayerData};

use crate::{FrameUniforms, RenderLayer, RenderStats, DEPTH_FORMAT};

const RENDER_SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    // Slight lift above the ground plane so roads win the depth test over fills.
    out.position = uniforms.view_proj * vec4<f32>(pos.x, 0.5, pos.y, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RoadUniforms {
    view_proj: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RoadVertex {
    position: [f32; 2],
    color: [f32; 4],
}

const MIN_SEGMENT_LEN: f32 = 1e-3;
/// Semicircle tessellation steps for round end caps.
const ROUND_CAP_STEPS: u32 = 10;
/// Arc steps for a 180° outer join; sharper/blunter corners scale from this.
const ROUND_JOIN_STEPS: u32 = 8;

pub struct RoadsLayer {
    pub roads: Vec<RoadFeature>,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    index_count: u32,
    render_pipeline: Option<wgpu::RenderPipeline>,
    render_bind_group: Option<wgpu::BindGroup>,
    render_uniform_buffer: Option<wgpu::Buffer>,
}

impl Default for RoadsLayer {
    fn default() -> Self {
        Self {
            roads: Vec::new(),
            vertex_buffer: None,
            index_buffer: None,
            index_count: 0,
            render_pipeline: None,
            render_bind_group: None,
            render_uniform_buffer: None,
        }
    }
}

impl RoadsLayer {
    /// Bake per-class color + width into one vertex/index buffer.
    ///
    /// Width is applied at expand time (vertex positions), color is a vertex
    /// attribute — so we still draw all road classes in a single `draw_indexed`.
    /// No per-class uniform / multi-pass needed.
    fn build_mesh(roads: &[RoadFeature]) -> (Vec<RoadVertex>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        // Minor first → major on top where ribbons overlap.
        let mut ordered: Vec<&RoadFeature> = roads.iter().collect();
        ordered.sort_by_key(|road| road_style(road).draw_order);

        for road in ordered {
            let style = road_style(road);
            expand_polyline(
                &road.points_tile,
                style.half_width,
                style.color,
                &mut vertices,
                &mut indices,
            );
        }

        (vertices, indices)
    }

    fn upload_mesh(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[RoadVertex],
        indices: &[u32],
    ) {
        self.index_count = indices.len() as u32;
        if vertices.is_empty() || indices.is_empty() {
            self.vertex_buffer = None;
            self.index_buffer = None;
            self.index_count = 0;
            return;
        }

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("roads-vertex-buffer"),
            size: (vertices.len() * mem::size_of::<RoadVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(vertices));
        self.vertex_buffer = Some(vertex_buffer);

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("roads-index-buffer"),
            size: (indices.len() * mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&index_buffer, 0, bytemuck::cast_slice(indices));
        self.index_buffer = Some(index_buffer);
    }
}

/// Per-class look. Edit this table to restyle roads — both fields are baked into
/// the mesh (not uniforms), so changing styles never adds draw calls.
#[derive(Clone, Copy)]
struct RoadStyle {
    color: [f32; 4],
    /// Half stroke width in world units (same scale as tile geometry).
    half_width: f32,
    /// Higher draws later (on top).
    draw_order: u8,
}

fn road_style(road: &RoadFeature) -> RoadStyle {
    if road.source_layer == "waterway" {
        return RoadStyle {
            color: [0.25, 0.55, 0.78, 1.0],
            half_width: 4.5,
            draw_order: 0,
        };
    }

    match road.class.as_str() {
        "motorway" | "motorway_construction" => RoadStyle {
            color: [0.95, 0.45, 0.20, 1.0],
            half_width: 8.5,
            draw_order: 8,
        },
        "trunk" | "trunk_construction" => RoadStyle {
            color: [0.95, 0.58, 0.22, 1.0],
            half_width: 8.0,
            draw_order: 7,
        },
        "primary" | "primary_construction" => RoadStyle {
            color: [0.92, 0.72, 0.28, 1.0],
            half_width: 7.0,
            draw_order: 6,
        },
        "secondary" | "secondary_construction" => RoadStyle {
            color: [0.88, 0.82, 0.38, 1.0],
            half_width: 6.0,
            draw_order: 5,
        },
        "tertiary" | "tertiary_construction" => RoadStyle {
            color: [0.78, 0.78, 0.55, 1.0],
            half_width: 5.0,
            draw_order: 4,
        },
        "minor" | "street" => RoadStyle {
            color: [0.62, 0.62, 0.64, 1.0],
            half_width: 4.0,
            draw_order: 3,
        },
        "service" | "busway" | "bus_guideway" => RoadStyle {
            color: [0.52, 0.52, 0.55, 1.0],
            half_width: 3.0,
            draw_order: 2,
        },
        "path" | "track" | "footway" | "cycleway" | "pedestrian" | "bridleway" => RoadStyle {
            color: [0.42, 0.58, 0.40, 1.0],
            half_width: 2.0,
            draw_order: 1,
        },
        "rail" | "transit" => RoadStyle {
            color: [0.58, 0.42, 0.65, 1.0],
            half_width: 3.0,
            draw_order: 9,
        },
        "ferry" => RoadStyle {
            color: [0.35, 0.55, 0.80, 1.0],
            half_width: 4.0,
            draw_order: 1,
        },
        "raceway" => RoadStyle {
            color: [0.75, 0.35, 0.55, 1.0],
            half_width: 5.0,
            draw_order: 5,
        },
        _ => RoadStyle {
            color: [0.55, 0.56, 0.58, 1.0],
            half_width: 3.5,
            draw_order: 3,
        },
    }
}

fn road_vertex(position: [f32; 2], color: [f32; 4]) -> RoadVertex {
    RoadVertex { position, color }
}

/// Expand a polyline into a stroke mesh (Canvas2D-style round caps + round joins).
///
/// Each segment is a butt quad; every corner gets an outer circular arc fan
/// (`lineJoin = 'round'`), including 90° turns. Inner sides overlap from the quads.
fn expand_polyline(
    points: &[[f64; 2]],
    half_width: f32,
    color: [f32; 4],
    vertices: &mut Vec<RoadVertex>,
    indices: &mut Vec<u32>,
) {
    if points.len() < 2 || half_width <= 0.0 {
        return;
    }

    let mut poly: Vec<[f32; 2]> = Vec::with_capacity(points.len());
    for p in points {
        let q = [p[0] as f32, p[1] as f32];
        if let Some(last) = poly.last() {
            let dx = q[0] - last[0];
            let dy = q[1] - last[1];
            if dx * dx + dy * dy < MIN_SEGMENT_LEN * MIN_SEGMENT_LEN {
                continue;
            }
        }
        poly.push(q);
    }
    if poly.len() < 2 {
        return;
    }

    let n = poly.len();
    let hw = half_width;

    let mut normals = Vec::with_capacity(n - 1);
    for i in 0..n - 1 {
        normals.push(perp(direction(poly[i + 1], poly[i])));
    }

    // Butt-capped segment quads.
    for i in 0..n - 1 {
        let a = poly[i];
        let b = poly[i + 1];
        let nr = normals[i];
        let base = vertices.len() as u32;
        vertices.extend_from_slice(&[
            road_vertex(add(a, scale(nr, hw)), color),
            road_vertex(add(a, scale(nr, -hw)), color),
            road_vertex(add(b, scale(nr, hw)), color),
            road_vertex(add(b, scale(nr, -hw)), color),
        ]);
        indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    }

    // Outer round joins at every corner (including 90°).
    for i in 1..n - 1 {
        let p = poly[i];
        let t_in = direction(p, poly[i - 1]);
        let t_out = direction(poly[i + 1], p);
        let cross = t_in[0] * t_out[1] - t_in[1] * t_out[0];
        if cross.abs() < 1e-5 {
            continue;
        }

        let n_in = normals[i - 1];
        let n_out = normals[i];
        if cross > 0.0 {
            // Left turn → right side is outer.
            push_round_arc_fan(
                vertices,
                indices,
                p,
                scale(n_in, -1.0),
                scale(n_out, -1.0),
                hw,
                color,
            );
        } else {
            // Right turn → left side is outer.
            push_round_arc_fan(vertices, indices, p, n_in, n_out, hw, color);
        }
    }

    // Round end caps (`lineCap = 'round'`).
    let dir_start = direction(poly[1], poly[0]);
    push_round_cap(
        vertices,
        indices,
        poly[0],
        perp(dir_start),
        scale(dir_start, -1.0),
        hw,
        color,
    );
    let last = n - 1;
    let dir_end = direction(poly[last], poly[last - 1]);
    push_round_cap(
        vertices,
        indices,
        poly[last],
        perp(dir_end),
        dir_end,
        hw,
        color,
    );
}

/// Circular-sector fan from unit direction `dir0` to `dir1` around `center`.
fn push_round_arc_fan(
    positions: &mut Vec<RoadVertex>,
    cells: &mut Vec<u32>,
    center: [f32; 2],
    dir0: [f32; 2],
    dir1: [f32; 2],
    half_thick: f32,
    color: [f32; 4],
) {
    let cos_a = dot(dir0, dir1).clamp(-1.0, 1.0);
    let mut sweep = cos_a.acos();
    if dir0[0] * dir1[1] - dir0[1] * dir1[0] < 0.0 {
        sweep = -sweep;
    }
    if sweep.abs() < 1e-4 {
        return;
    }

    let steps = ((sweep.abs() / std::f32::consts::PI) * ROUND_JOIN_STEPS as f32)
        .ceil()
        .max(1.0) as u32;

    let center_i = positions.len() as u32;
    positions.push(road_vertex(center, color));

    let rim0 = positions.len() as u32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let dir = rotate(dir0, sweep * t);
        positions.push(road_vertex(add(center, scale(dir, half_thick)), color));
    }

    for i in 0..steps {
        cells.extend_from_slice(&[center_i, rim0 + i, rim0 + i + 1]);
    }
}

/// Semicircle fan: from `-normal` through `outward` to `+normal`.
fn push_round_cap(
    positions: &mut Vec<RoadVertex>,
    cells: &mut Vec<u32>,
    center: [f32; 2],
    normal: [f32; 2],
    outward: [f32; 2],
    half_thick: f32,
    color: [f32; 4],
) {
    let center_i = positions.len() as u32;
    positions.push(road_vertex(center, color));

    let rim0 = positions.len() as u32;
    for i in 0..=ROUND_CAP_STEPS {
        let t = i as f32 / ROUND_CAP_STEPS as f32;
        let angle = std::f32::consts::PI * t;
        let dir = add(
            scale(scale(normal, -1.0), angle.cos()),
            scale(outward, angle.sin()),
        );
        positions.push(road_vertex(add(center, scale(dir, half_thick)), color));
    }

    for i in 0..ROUND_CAP_STEPS {
        cells.extend_from_slice(&[center_i, rim0 + i, rim0 + i + 1]);
    }
}

fn direction(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    normalize(sub(a, b))
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn scale(v: [f32; 2], s: f32) -> [f32; 2] {
    [v[0] * s, v[1] * s]
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0].mul_add(b[0], a[1] * b[1])
}

fn normalize(v: [f32; 2]) -> [f32; 2] {
    let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if len < 1e-8 {
        [1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len]
    }
}

fn perp(v: [f32; 2]) -> [f32; 2] {
    [-v[1], v[0]]
}

fn rotate(v: [f32; 2], angle: f32) -> [f32; 2] {
    let (s, c) = angle.sin_cos();
    [v[0] * c - v[1] * s, v[0] * s + v[1] * c]
}

impl RenderLayer for RoadsLayer {
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
    ) -> anyhow::Result<()> {
        let render_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("roads-render-shader"),
            source: wgpu::ShaderSource::Wgsl(RENDER_SHADER.into()),
        });

        let render_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("roads-render-bind-group-layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("roads-render-pipeline-layout"),
                bind_group_layouts: &[&render_bind_group_layout],
                push_constant_ranges: &[],
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("roads-render-pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &render_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: mem::size_of::<RoadVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &render_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let render_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("roads-render-uniform-buffer"),
            size: mem::size_of::<RoadUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("roads-render-bind-group"),
            layout: &render_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: render_uniform_buffer.as_entire_binding(),
            }],
        });

        self.render_pipeline = Some(render_pipeline);
        self.render_bind_group = Some(render_bind_group);
        self.render_uniform_buffer = Some(render_uniform_buffer);

        Ok(())
    }

    fn upload(
        &mut self,
        layer: &TileLayerData,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<()> {
        if layer.kind != LayerKind::Roads {
            return Ok(());
        }

        if let LayerPayload::Roads(roads) = &layer.payload {
            self.roads = roads.clone();
            let (vertices, indices) = Self::build_mesh(&self.roads);
            self.upload_mesh(device, queue, &vertices, &indices);
        }

        Ok(())
    }

    fn render(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        queue: &wgpu::Queue,
        frame: &FrameUniforms,
    ) -> RenderStats {
        let Some(render_pipeline) = &self.render_pipeline else {
            return RenderStats::default();
        };
        let Some(render_bind_group) = &self.render_bind_group else {
            return RenderStats::default();
        };
        let Some(vertex_buffer) = &self.vertex_buffer else {
            return RenderStats::default();
        };
        let Some(index_buffer) = &self.index_buffer else {
            return RenderStats::default();
        };
        let Some(render_uniform_buffer) = &self.render_uniform_buffer else {
            return RenderStats::default();
        };

        if self.index_count == 0 {
            return RenderStats::default();
        }

        let uniforms = RoadUniforms {
            view_proj: frame.view_proj,
        };
        queue.write_buffer(render_uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        pass.set_pipeline(render_pipeline);
        pass.set_bind_group(0, render_bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);

        RenderStats {
            draw_calls: 1,
            triangles: self.index_count / 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_angle_gets_round_join_and_caps() {
        let points = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let color = [0.9, 0.5, 0.2, 1.0];
        expand_polyline(&points, 2.0, color, &mut vertices, &mut indices);
        // 2 segment quads (8) + join fan + 2 caps — more than bare quads.
        assert!(vertices.len() > 8);
        assert_eq!(indices.len() % 3, 0);
        for v in &vertices {
            assert!(v.position[0].is_finite() && v.position[1].is_finite());
            assert_eq!(v.color, color);
        }
    }

    #[test]
    fn motorway_style_is_wider_and_warmer_than_path() {
        let motorway = RoadFeature {
            id: 1,
            class: "motorway".into(),
            source_layer: "transportation".into(),
            points_tile: vec![],
        };
        let path = RoadFeature {
            id: 2,
            class: "path".into(),
            source_layer: "transportation".into(),
            points_tile: vec![],
        };
        let m = road_style(&motorway);
        let p = road_style(&path);
        assert!(m.half_width > p.half_width);
        assert!(m.color[0] > p.color[0]);
        assert!(m.draw_order > p.draw_order);
    }
}
