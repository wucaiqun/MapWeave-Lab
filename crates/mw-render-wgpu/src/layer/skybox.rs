use bytemuck::{Pod, Zeroable};
use mw_assets::{AssetServer, CubemapFaces, CubemapLayout};
use mw_core::TileLayerData;

use crate::{FrameUniforms, RenderLayer, RenderStats, DEPTH_FORMAT};

/// Object-space unit cube corner. Cubemap sample direction = this position.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SkyVertex {
    position: [f32; 3],
}

impl SkyVertex {
    const fn new(x: f32, y: f32, z: f32) -> Self {
        Self {
            position: [x, y, z],
        }
    }
}

const SKY_VERTICES: [SkyVertex; 8] = [
    SkyVertex::new(-1.0, -1.0, -1.0), // 0
    SkyVertex::new(1.0, -1.0, -1.0),  // 1
    SkyVertex::new(1.0, 1.0, -1.0),   // 2
    SkyVertex::new(-1.0, 1.0, -1.0),  // 3
    SkyVertex::new(-1.0, -1.0, 1.0),  // 4
    SkyVertex::new(1.0, -1.0, 1.0),   // 5
    SkyVertex::new(1.0, 1.0, 1.0),    // 6
    SkyVertex::new(-1.0, 1.0, 1.0),   // 7
];

/// 12 triangles (CCW from outside). Prefer front-face culling when drawing.
const SKY_INDICES: [u32; 36] = [
    // -Z
    0, 3, 2, 0, 2, 1,
    // +Z
    4, 5, 6, 4, 6, 7,
    // -X
    0, 4, 7, 0, 7, 3,
    // +X
    1, 2, 6, 1, 6, 5,
    // -Y
    0, 1, 5, 0, 5, 4,
    // +Y
    3, 7, 6, 3, 6, 2,
];

const SKYBOX_SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var sky_tex: texture_cube<f32>;
@group(0) @binding(2) var sky_tex_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) dir: vec3<f32>,
}

@vertex
fn vs_main(@location(0) pos: vec3<f32>) -> VertexOutput {
    var out: VertexOutput;
    var clip = uniforms.view_proj * vec4<f32>(pos, 0.0);
    clip.z = clip.w;
    out.clip_position = clip;
    out.dir = pos;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(sky_tex, sky_tex_sampler, in.dir);
}
"#;

pub struct SkyboxLayer {
    cubemap: CubemapFaces,

    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    index_count: u32,

    cube_texture: Option<wgpu::Texture>,
    cube_view: Option<wgpu::TextureView>,
    sampler: Option<wgpu::Sampler>,

    uniform_buffer: Option<wgpu::Buffer>,
    bind_group: Option<wgpu::BindGroup>,
    pipeline: Option<wgpu::RenderPipeline>,
}

impl Default for SkyboxLayer {
    fn default() -> Self {
        let cubemap = AssetServer::discover()
            .and_then(|assets| {
                assets.load_cubemap_cross("sky/sky_cross.png", CubemapLayout::HorizontalCross)
            })
            .expect("SkyboxLayer: failed to load sky/sky_cross.png");
        Self {
            cubemap,
            vertex_buffer: None,
            index_buffer: None,
            index_count: 0,
            cube_texture: None,
            cube_view: None,
            sampler: None,
            uniform_buffer: None,
            bind_group: None,
            pipeline: None,
        }
    }
}

impl RenderLayer for SkyboxLayer {
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
    ) -> anyhow::Result<()> {
        // self.cubemap is already six RGBA faces from Default — do not from_cross again.
        let size = self.cubemap.size;
        let cube_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("skybox-cube"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texture_view = cube_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("skybox-cube-view"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });
        self.cube_texture = Some(cube_texture);
        self.cube_view = Some(texture_view);
        // bind group layout — must match shader @binding 0/1/2
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skybox-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        //create sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("skybox-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        self.sampler = Some(sampler);

        // create uniform buffer for view_proj
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skybox-uniforms"),
            size: std::mem::size_of::<[[f32; 4]; 4]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.uniform_buffer = Some(uniform_buffer);
        
        //preparing vertex and index buffers.
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skybox-vertices"),
            size: (SKY_VERTICES.len() * std::mem::size_of::<SkyVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.vertex_buffer = Some(vertex_buffer);

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skybox-indices"),
            size: (SKY_INDICES.len() * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.index_buffer = Some(index_buffer);
        self.index_count = SKY_INDICES.len() as u32;

        // create pipeline
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("skybox-shader"),
            source: wgpu::ShaderSource::Wgsl(SKYBOX_SHADER.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skybox-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skybox-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<SkyVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
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
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // Camera is inside the cube — cull front faces.
                cull_mode: Some(wgpu::Face::Front),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        self.pipeline = Some(pipeline);

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("skybox-bind-group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self
                        .uniform_buffer
                        .as_ref()
                        .unwrap()
                        .as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        self.cube_view.as_ref().unwrap(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(self.sampler.as_ref().unwrap()),
                },
            ],
        });
        self.bind_group = Some(bind_group);

        Ok(())
    }

    fn upload(
        &mut self,
        _layer: &TileLayerData,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<()> {
        // Draft: one face. Loop layer 0..6 with origin.z = layer for full cubemap.
        let Some(texture) = self.cube_texture.as_ref() else {
            return Ok(());
        };
        let size = self.cubemap.size;
        for layer in 0..6 {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: layer as u32 },
                    aspect: wgpu::TextureAspect::All,
                },
                &self.cubemap.faces[layer],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * size),
                    rows_per_image: Some(size),
                },
                wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
            );
        }
        //write vertext buffer
        queue.write_buffer(
            self.vertex_buffer.as_ref().unwrap(),
            0,
            bytemuck::cast_slice(&SKY_VERTICES),
        );
        //write index buffer
        queue.write_buffer(
            self.index_buffer.as_ref().unwrap(),
            0,
            bytemuck::cast_slice(&SKY_INDICES),
        );
        Ok(())
    }

    fn render(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        queue: &wgpu::Queue,
        frame: &FrameUniforms,
    ) -> RenderStats {
        let Some(pipeline) = self.pipeline.as_ref() else {
            return RenderStats::default();
        };
        let Some(bind_group) = self.bind_group.as_ref() else {
            return RenderStats::default();
        };
        let Some(vertex_buffer) = self.vertex_buffer.as_ref() else {
            return RenderStats::default();
        };
        let Some(index_buffer) = self.index_buffer.as_ref() else {
            return RenderStats::default();
        };
        let Some(uniform_buffer) = self.uniform_buffer.as_ref() else {
            return RenderStats::default();
        };
        if self.index_count == 0 {
            return RenderStats::default();
        }

        // Shader uses w=0, so translation in view is dropped — pass frame.view_proj as-is.
        queue.write_buffer(uniform_buffer, 0, bytemuck::bytes_of(&frame.view_proj));

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
