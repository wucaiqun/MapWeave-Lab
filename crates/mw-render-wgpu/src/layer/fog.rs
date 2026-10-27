use std::mem;

use bytemuck::{Pod, Zeroable};

use crate::{FrameUniforms, RenderStats};

const SHADER: &str = r#"
struct Uniforms {
    inv_view_proj: mat4x4<f32>,
    camera_pos: vec3<f32>,
    _pad0: f32,
    fog_color: vec4<f32>,
    fog_start: f32,
    fog_end: f32,
    _pad1: vec2<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var depth_tex: texture_depth_2d;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var out: VertexOutput;
    out.position = vec4<f32>(pos[idx], 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(depth_tex));
    let coord = vec2<i32>(i32(frag.x), i32(frag.y));
    let depth = textureLoad(depth_tex, coord, 0);

    // Sky / cleared far plane — leave cubemap alone.
    if (depth >= 0.9999) {
        return vec4<f32>(0.0);
    }

    // frag.xy is top-left pixel space → NDC (Y up).
    let uv = frag.xy / dims;
    let ndc = vec4<f32>(
        uv.x * 2.0 - 1.0,
        (1.0 - uv.y) * 2.0 - 1.0,
        depth,
        1.0,
    );
    let world_h = uniforms.inv_view_proj * ndc;
    let world = world_h.xyz / world_h.w;

    // Distance from the eye (same space as mesh / camera_pos).
    let dist = length(world - uniforms.camera_pos);
    let t = smoothstep(uniforms.fog_start, uniforms.fog_end, dist) * 0.75;
    return vec4<f32>(uniforms.fog_color.rgb, t);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FogUniforms {
    inv_view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 3],
    _pad0: f32,
    fog_color: [f32; 4],
    fog_start: f32,
    fog_end: f32,
    _pad1: [f32; 2],
}

/// Fullscreen depth fog. Second pass after map layers; samples depth, rebuilds
/// world position, fogs by distance to the camera (not by screen Y).
pub struct FogLayer {
    pipeline: Option<wgpu::RenderPipeline>,
    bind_group_layout: Option<wgpu::BindGroupLayout>,
    uniform_buffer: Option<wgpu::Buffer>,
}

impl Default for FogLayer {
    fn default() -> Self {
        Self {
            pipeline: None,
            bind_group_layout: None,
            uniform_buffer: None,
        }
    }
}

impl FogLayer {
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
    ) -> anyhow::Result<()> {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fog-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fog-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fog-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fog-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
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
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fog-uniform-buffer"),
            size: mem::size_of::<FogUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.pipeline = Some(pipeline);
        self.bind_group_layout = Some(bind_group_layout);
        self.uniform_buffer = Some(uniform_buffer);
        Ok(())
    }

    pub fn render(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &FrameUniforms,
        depth_view: &wgpu::TextureView,
    ) -> RenderStats {
        let Some(pipeline) = self.pipeline.as_ref() else {
            return RenderStats::default();
        };
        let Some(layout) = self.bind_group_layout.as_ref() else {
            return RenderStats::default();
        };
        let Some(uniform_buffer) = self.uniform_buffer.as_ref() else {
            return RenderStats::default();
        };

        let uniforms = FogUniforms {
            inv_view_proj: frame.inv_view_proj,
            camera_pos: frame.camera_pos,
            _pad0: 0.0,
            fog_color: frame.fog_color,
            fog_start: frame.fog_start,
            fog_end: frame.fog_end,
            _pad1: [0.0; 2],
        };
        queue.write_buffer(uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fog-bind-group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depth_view),
                },
            ],
        });

        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);

        RenderStats {
            draw_calls: 1,
            triangles: 1,
        }
    }
}
