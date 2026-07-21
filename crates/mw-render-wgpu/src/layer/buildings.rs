use std::mem;

use bytemuck::{Pod, Zeroable};
use mw_core::{triangulate_polygon_features, LayerKind, LayerPayload, PolygonFeature, TileLayerData};

use crate::{FrameUniforms, RenderLayer, RenderStats, DEPTH_FORMAT};

const SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
    color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) shade: f32,
}

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) shade: f32,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = uniforms.view_proj * vec4<f32>(pos, 1.0);
    out.shade = shade;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(uniforms.color.rgb * in.shade, uniforms.color.a);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BuildingsUniforms {
    view_proj: [[f32; 4]; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BuildingsVertex {
    position: [f32; 3],
    shade: f32,
}

pub struct BuildingsLayer {
    pub buildings: Vec<mw_core::PolygonFeature>,
    pipeline: Option<wgpu::RenderPipeline>,
    bind_group_layout: Option<wgpu::BindGroupLayout>,
    bind_group: Option<wgpu::BindGroup>,
    uniform_buffer: Option<wgpu::Buffer>,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    index_count: u32,
    fill_color: [f32; 4],
}

impl Default for BuildingsLayer {
    fn default() -> Self {
        Self {
            buildings: Vec::new(),
            pipeline: None,
            bind_group_layout: None,
            bind_group: None,
            uniform_buffer: None,
            vertex_buffer: None,
            index_buffer: None,
            index_count: 0,
            fill_color: [0.78, 0.74, 0.68, 1.0],
        }
    }
}

impl BuildingsLayer {
    fn build_mesh(polygons: &[PolygonFeature]) -> (Vec<BuildingsVertex>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for polygon in polygons {
            // Heights are already world-Y after `map_decoded_tile_to_scene`.
            let top = polygon.height.max(polygon.min_height) as f32;
            let bottom = polygon.min_height as f32;

            push_roof(&mut vertices, &mut indices, polygon, top);
            for ring in &polygon.rings {
                push_wall_ring(&mut vertices, &mut indices, &ring.points, bottom, top);
            }
        }

        (vertices, indices)
    }

    fn upload_mesh(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[BuildingsVertex],
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
            label: Some("buildings-vertex-buffer"),
            size: (vertices.len() * mem::size_of::<BuildingsVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(vertices));
        self.vertex_buffer = Some(vertex_buffer);

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buildings-index-buffer"),
            size: (indices.len() * mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&index_buffer, 0, bytemuck::cast_slice(indices));
        self.index_buffer = Some(index_buffer);
    }
}

fn push_roof(
    vertices: &mut Vec<BuildingsVertex>,
    indices: &mut Vec<u32>,
    polygon: &PolygonFeature,
    top: f32,
) {
    let mesh = triangulate_polygon_features(std::slice::from_ref(polygon));
    if mesh.indices.is_empty() {
        return;
    }

    let base = vertices.len() as u32;
    for position in mesh.positions {
        vertices.push(BuildingsVertex {
            position: [position[0], top, position[1]],
            shade: 1.0,
        });
    }
    indices.extend(mesh.indices.into_iter().map(|i| i + base));
}

fn push_wall_ring(
    vertices: &mut Vec<BuildingsVertex>,
    indices: &mut Vec<u32>,
    points: &[[f64; 2]],
    bottom: f32,
    top: f32,
) {
    if points.len() < 2 || top <= bottom {
        return;
    }

    let count = points.len();
    for i in 0..count {
        let a = points[i];
        let b = points[(i + 1) % count];
        let ax = a[0] as f32;
        let az = a[1] as f32;
        let bx = b[0] as f32;
        let bz = b[1] as f32;

        let base = vertices.len() as u32;
        // Slightly darker walls so volume reads without a lighting system.
        let shade = 0.72;
        vertices.extend_from_slice(&[
            BuildingsVertex {
                position: [ax, bottom, az],
                shade,
            },
            BuildingsVertex {
                position: [bx, bottom, bz],
                shade,
            },
            BuildingsVertex {
                position: [bx, top, bz],
                shade,
            },
            BuildingsVertex {
                position: [ax, top, az],
                shade,
            },
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

impl RenderLayer for BuildingsLayer {
    fn prepare(&mut self, device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> anyhow::Result<()> {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("buildings-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("buildings-bind-group-layout"),
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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("buildings-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("buildings-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: mem::size_of::<BuildingsVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 12,
                            shader_location: 1,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
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
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buildings-uniform-buffer"),
            size: mem::size_of::<BuildingsUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("buildings-bind-group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        self.pipeline = Some(pipeline);
        self.bind_group_layout = Some(bind_group_layout);
        self.bind_group = Some(bind_group);
        self.uniform_buffer = Some(uniform_buffer);

        Ok(())
    }

    fn upload(
        &mut self,
        layer: &TileLayerData,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<()> {
        if layer.kind != LayerKind::Buildings {
            return Ok(());
        }

        if let LayerPayload::Buildings(buildings) = &layer.payload {
            self.buildings = buildings.clone();
            let (vertices, indices) = Self::build_mesh(&self.buildings);
            self.upload_mesh(device, queue, &vertices, &indices);
        }

        Ok(())
    }

    fn render(&self, pass: &mut wgpu::RenderPass<'_>, queue: &wgpu::Queue, frame: &FrameUniforms) -> RenderStats {
        let Some(pipeline) = &self.pipeline else {
            return RenderStats::default();
        };
        let Some(bind_group) = &self.bind_group else {
            return RenderStats::default();
        };
        let Some(vertex_buffer) = &self.vertex_buffer else {
            return RenderStats::default();
        };
        let Some(index_buffer) = &self.index_buffer else {
            return RenderStats::default();
        };
        let Some(uniform_buffer) = &self.uniform_buffer else {
            return RenderStats::default();
        };

        if self.index_count == 0 {
            return RenderStats::default();
        }

        let uniforms = BuildingsUniforms {
            view_proj: frame.view_proj,
            color: self.fill_color,
        };
        queue.write_buffer(uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);

        RenderStats {
            draw_calls: 1,
            triangles: self.index_count / 3,
        }
    }
}
