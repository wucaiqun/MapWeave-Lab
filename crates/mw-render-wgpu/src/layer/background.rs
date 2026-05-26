use std::mem;

use bytemuck::{Pod, Zeroable};
use mw_core::{LayerKind, LayerPayload, RingRole, TileLayerData};

use crate::{FrameUniforms, RenderLayer};

const SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
    color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vs_main(@location(0) pos: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    // MVT tile (x, y) -> world (x, 0, z) on the ground plane, Y up.
    out.position = uniforms.view_proj * vec4<f32>(pos.x, 0.0, pos.y, 1.0);
    return out;
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return uniforms.color;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BackgroundUniforms {
    view_proj: [[f32; 4]; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BackgroundVertex {
    position: [f32; 2],
}

pub struct BackgroundLayer {
    pub background: Vec<mw_core::PolygonFeature>,
    pipeline: Option<wgpu::RenderPipeline>,
    bind_group_layout: Option<wgpu::BindGroupLayout>,
    bind_group: Option<wgpu::BindGroup>,
    uniform_buffer: Option<wgpu::Buffer>,
    vertex_buffer: Option<wgpu::Buffer>,
    vertex_count: u32,
    fill_color: [f32; 4],
}

impl Default for BackgroundLayer {
    fn default() -> Self {
        Self {
            background: Vec::new(),
            pipeline: None,
            bind_group_layout: None,
            bind_group: None,
            uniform_buffer: None,
            vertex_buffer: None,
            vertex_count: 0,
            fill_color: [0.08, 0.12, 0.18, 1.0],
        }
    }
}

impl BackgroundLayer {
    fn build_vertices(polygons: &[mw_core::PolygonFeature]) -> Vec<BackgroundVertex> {
        let mut vertices = Vec::new();

        for polygon in polygons {
            for ring in &polygon.rings {
                if ring.role != RingRole::Exterior {
                    continue;
                }

                let points = &ring.points;
                if points.len() < 3 {
                    continue;
                }

                let anchor = points[0];
                for i in 1..points.len() - 1 {
                    vertices.push(BackgroundVertex {
                        position: [anchor[0] as f32, anchor[1] as f32],
                    });
                    vertices.push(BackgroundVertex {
                        position: [points[i][0] as f32, points[i][1] as f32],
                    });
                    vertices.push(BackgroundVertex {
                        position: [points[i + 1][0] as f32, points[i + 1][1] as f32],
                    });
                }
            }
        }

        vertices
    }

    fn upload_vertices(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[BackgroundVertex],
    ) {
        self.vertex_count = vertices.len() as u32;
        if vertices.is_empty() {
            self.vertex_buffer = None;
            return;
        }

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("background-vertex-buffer"),
            size: (vertices.len() * mem::size_of::<BackgroundVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(vertices));
        self.vertex_buffer = Some(vertex_buffer);
    }
}

impl RenderLayer for BackgroundLayer {
    fn prepare(&mut self, device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> anyhow::Result<()> {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("background-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("background-bind-group-layout"),
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
            label: Some("background-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("background-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: mem::size_of::<BackgroundVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    }],
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
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("background-uniform-buffer"),
            size: mem::size_of::<BackgroundUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("background-bind-group"),
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
        if layer.kind != LayerKind::Background {
            return Ok(());
        }

        if let LayerPayload::Background(background) = &layer.payload {
            self.background = background.clone();
            let vertices = Self::build_vertices(&self.background);
            self.upload_vertices(device, queue, &vertices);
        }

        Ok(())
    }

    fn render(&self, pass: &mut wgpu::RenderPass<'_>, queue: &wgpu::Queue, frame: &FrameUniforms) {
        let Some(pipeline) = &self.pipeline else {
            return;
        };
        let Some(bind_group) = &self.bind_group else {
            return;
        };
        let Some(vertex_buffer) = &self.vertex_buffer else {
            return;
        };
        let Some(uniform_buffer) = &self.uniform_buffer else {
            return;
        };

        if self.vertex_count == 0 {
            return;
        }

        let uniforms = BackgroundUniforms {
            view_proj: frame.view_proj,
            color: self.fill_color,
        };
        queue.write_buffer(uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.draw(0..self.vertex_count, 0..1);
    }
}
