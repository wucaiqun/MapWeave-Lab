use std::mem;

use bytemuck::{Pod, Zeroable};
use mw_core::{LayerKind, LayerPayload, RoadFeature, TileLayerData};

use crate::{FrameUniforms, RenderLayer};

const EXPAND_SHADER: &str = r#"
struct Params {
    segment_count: u32,
    half_width: f32,
}

struct SegmentRef {
    point_index: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> points: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> segments: array<SegmentRef>;
@group(0) @binding(3) var<storage, read_write> out_verts: array<vec2<f32>>;

@compute @workgroup_size(64)
fn expand_segment(@builtin(global_invocation_id) gid: vec3<u32>) {
    let seg = gid.x;
    if seg >= params.segment_count {
        return;
    }

    let idx = segments[seg].point_index;
    let a = points[idx];
    let b = points[idx + 1u];
    let delta = b - a;
    let len = length(delta);
    if len < 1e-6 {
        return;
    }

    let dir = delta / len;
    let n = vec2<f32>(-dir.y, dir.x) * params.half_width;

    let base = seg * 6u;
    out_verts[base + 0u] = a - n;
    out_verts[base + 1u] = a + n;
    out_verts[base + 2u] = b + n;
    out_verts[base + 3u] = a - n;
    out_verts[base + 4u] = b + n;
    out_verts[base + 5u] = b - n;
}
"#;

const RENDER_SHADER: &str = r#"
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
struct ExpandParams {
    segment_count: u32,
    half_width: f32,
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuPoint {
    position: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuSegment {
    point_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RoadUniforms {
    view_proj: [[f32; 4]; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RoadVertex {
    position: [f32; 2],
}

const VERTS_PER_SEGMENT: u32 = 6;
const WORKGROUP_SIZE: u32 = 64;

pub struct RoadsLayer {
    pub roads: Vec<RoadFeature>,
    compute_pipeline: Option<wgpu::ComputePipeline>,
    compute_bind_group_layout: Option<wgpu::BindGroupLayout>,
    compute_bind_group: Option<wgpu::BindGroup>,
    expand_params_buffer: Option<wgpu::Buffer>,
    points_buffer: Option<wgpu::Buffer>,
    segments_buffer: Option<wgpu::Buffer>,
    output_buffer: Option<wgpu::Buffer>,
    segment_count: u32,
    vertex_count: u32,
    render_pipeline: Option<wgpu::RenderPipeline>,
    render_bind_group_layout: Option<wgpu::BindGroupLayout>,
    render_bind_group: Option<wgpu::BindGroup>,
    render_uniform_buffer: Option<wgpu::Buffer>,
    fill_color: [f32; 4],
    half_width: f32,
}

impl Default for RoadsLayer {
    fn default() -> Self {
        Self {
            roads: Vec::new(),
            compute_pipeline: None,
            compute_bind_group_layout: None,
            compute_bind_group: None,
            expand_params_buffer: None,
            points_buffer: None,
            segments_buffer: None,
            output_buffer: None,
            segment_count: 0,
            vertex_count: 0,
            render_pipeline: None,
            render_bind_group_layout: None,
            render_bind_group: None,
            render_uniform_buffer: None,
            fill_color: [0.45, 0.48, 0.52, 1.0],
            half_width: 8.0,
        }
    }
}

impl RoadsLayer {
    fn build_gpu_inputs(roads: &[RoadFeature]) -> (Vec<GpuPoint>, Vec<GpuSegment>) {
        let mut points = Vec::new();
        let mut segments = Vec::new();

        for road in roads {
            let road_points = &road.points_lon_lat;
            if road_points.len() < 2 {
                continue;
            }

            let point_offset = points.len() as u32;
            for p in road_points {
                points.push(GpuPoint {
                    position: [p[0] as f32, p[1] as f32],
                });
            }

            for i in 0..road_points.len() - 1 {
                segments.push(GpuSegment {
                    point_index: point_offset + i as u32,
                });
            }
        }

        (points, segments)
    }

    fn run_expand_pass(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<()> {
        let Some(compute_pipeline) = &self.compute_pipeline else {
            return Ok(());
        };
        let Some(compute_bind_group) = &self.compute_bind_group else {
            return Ok(());
        };

        if self.segment_count == 0 {
            return Ok(());
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("roads-expand-encoder"),
        });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("roads-expand-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(compute_pipeline);
            pass.set_bind_group(0, compute_bind_group, &[]);
            pass.dispatch_workgroups(
                self.segment_count.div_ceil(WORKGROUP_SIZE),
                1,
                1,
            );
        }

        queue.submit(Some(encoder.finish()));
        Ok(())
    }

    fn upload_gpu_data(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<()> {
        let (points, segments) = Self::build_gpu_inputs(&self.roads);
        self.segment_count = segments.len() as u32;
        self.vertex_count = self.segment_count * VERTS_PER_SEGMENT;

        if self.segment_count == 0 {
            self.points_buffer = None;
            self.segments_buffer = None;
            self.output_buffer = None;
            self.compute_bind_group = None;
            return Ok(());
        }

        let Some(compute_bind_group_layout) = &self.compute_bind_group_layout else {
            return Ok(());
        };

        let points_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("roads-points-storage"),
            size: (points.len() * mem::size_of::<GpuPoint>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&points_buffer, 0, bytemuck::cast_slice(&points));

        let segments_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("roads-segments-storage"),
            size: (segments.len() * mem::size_of::<GpuSegment>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&segments_buffer, 0, bytemuck::cast_slice(&segments));

        let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("roads-expanded-verts"),
            size: (self.vertex_count as u64) * mem::size_of::<RoadVertex>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });

        let expand_params = ExpandParams {
            segment_count: self.segment_count,
            half_width: self.half_width,
            _pad: [0.0; 2],
        };
        let Some(expand_params_buffer) = &self.expand_params_buffer else {
            return Ok(());
        };
        queue.write_buffer(expand_params_buffer, 0, bytemuck::bytes_of(&expand_params));

        let compute_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("roads-compute-bind-group"),
            layout: compute_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: expand_params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: points_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: segments_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: output_buffer.as_entire_binding(),
                },
            ],
        });

        self.points_buffer = Some(points_buffer);
        self.segments_buffer = Some(segments_buffer);
        self.output_buffer = Some(output_buffer);
        self.compute_bind_group = Some(compute_bind_group);

        self.run_expand_pass(device, queue)
    }
}

impl RenderLayer for RoadsLayer {
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
    ) -> anyhow::Result<()> {
        let expand_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("roads-expand-shader"),
            source: wgpu::ShaderSource::Wgsl(EXPAND_SHADER.into()),
        });

        let compute_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("roads-compute-bind-group-layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("roads-compute-pipeline-layout"),
                bind_group_layouts: &[&compute_bind_group_layout],
                push_constant_ranges: &[],
            });

        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("roads-expand-pipeline"),
            layout: Some(&compute_pipeline_layout),
            module: &expand_shader,
            entry_point: Some("expand_segment"),
            compilation_options: Default::default(),
            cache: None,
        });

        let expand_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("roads-expand-params"),
            size: mem::size_of::<ExpandParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

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
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    }],
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
            depth_stencil: None,
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

        self.compute_pipeline = Some(compute_pipeline);
        self.compute_bind_group_layout = Some(compute_bind_group_layout);
        self.expand_params_buffer = Some(expand_params_buffer);
        self.render_pipeline = Some(render_pipeline);
        self.render_bind_group_layout = Some(render_bind_group_layout);
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
            self.upload_gpu_data(device, queue)?;
        }

        Ok(())
    }

    fn render(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        queue: &wgpu::Queue,
        frame: &FrameUniforms,
    ) {
        let Some(render_pipeline) = &self.render_pipeline else {
            return;
        };
        let Some(render_bind_group) = &self.render_bind_group else {
            return;
        };
        let Some(output_buffer) = &self.output_buffer else {
            return;
        };
        let Some(render_uniform_buffer) = &self.render_uniform_buffer else {
            return;
        };

        if self.vertex_count == 0 {
            return;
        }

        let uniforms = RoadUniforms {
            view_proj: frame.view_proj,
            color: self.fill_color,
        };
        queue.write_buffer(render_uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        pass.set_pipeline(render_pipeline);
        pass.set_bind_group(0, render_bind_group, &[]);
        pass.set_vertex_buffer(0, output_buffer.slice(..));
        pass.draw(0..self.vertex_count, 0..1);
    }
}
