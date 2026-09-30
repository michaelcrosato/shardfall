//! The frame renderer: shadow pass -> MSAA scene pass (HDR color + normal/group + depth)
//! -> composite (outlines, tonemap) into any target view.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use crate::mesh::{MeshData, MeshKey, Vertex};
use crate::scene::{Scene, Tonemap};

pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const NORMAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const SHADOW_SIZE: u32 = 2048;
pub const SAMPLES: u32 = 4;
const MAX_POINT_LIGHTS: usize = 64;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    light_view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    forward: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    sky_color: [f32; 4],
    ground_color: [f32; 4],
    cut: [f32; 4],
    cut2: [f32; 4],
    params: [f32; 4],
    style: [f32; 4],
    viewport: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuPointLight {
    pos_radius: [f32; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuMeshInstance {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    params: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuSdfInstance {
    a: [f32; 4],
    b: [f32; 4],
    color: [f32; 4],
    params: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PostUniform {
    inv_proj: [[f32; 4]; 4],
    outline_color: [f32; 4],
    outline: [f32; 4],
    tone: [f32; 4],
    misc: [f32; 4],
    fwd: [f32; 4],
}

struct GpuMesh {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
}

struct FrameTargets {
    size: (u32, u32),
    hdr_ms: wgpu::TextureView,
    hdr: wgpu::TextureView,
    normal_ms: wgpu::TextureView,
    depth_ms: wgpu::TextureView,
    post_bg: wgpu::BindGroup,
}

/// A growable GPU buffer.
struct DynBuffer {
    buf: wgpu::Buffer,
    cap: u64,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl DynBuffer {
    fn new(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages, cap: u64) -> Self {
        let usage = usage | wgpu::BufferUsages::COPY_DST;
        let buf =
            device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: cap, usage, mapped_at_creation: false });
        Self { buf, cap, usage, label }
    }
    /// Uploads `data`; returns true if the buffer was reallocated.
    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[u8]) -> bool {
        let mut grew = false;
        if data.len() as u64 > self.cap {
            self.cap = (data.len() as u64).next_power_of_two();
            self.buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: self.cap,
                usage: self.usage,
                mapped_at_creation: false,
            });
            grew = true;
        }
        if !data.is_empty() {
            queue.write_buffer(&self.buf, 0, data);
        }
        grew
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub mesh_instances: usize,
    pub sdf_instances: usize,
    pub draw_calls: usize,
    pub meshes_loaded: usize,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    meshes: HashMap<MeshKey, GpuMesh>,
    globals_buf: wgpu::Buffer,
    shadow_globals_buf: wgpu::Buffer,
    lights: DynBuffer,
    globals_layout: wgpu::BindGroupLayout,
    globals_bg: wgpu::BindGroup,
    shadow_bg: wgpu::BindGroup,
    shadow_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    mesh_pipeline: wgpu::RenderPipeline,
    sdf_pipeline: wgpu::RenderPipeline,
    mesh_shadow_pipeline: wgpu::RenderPipeline,
    sdf_shadow_pipeline: wgpu::RenderPipeline,
    post_layout: wgpu::BindGroupLayout,
    post_pipeline_layout: wgpu::PipelineLayout,
    post_shader: wgpu::ShaderModule,
    post_pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    post_buf: wgpu::Buffer,
    targets: Option<FrameTargets>,
    mesh_instances: DynBuffer,
    sdf_instances: DynBuffer,
    pub stats: RenderStats,
}

fn vertex_layouts() -> [wgpu::VertexBufferLayout<'static>; 2] {
    const VERT: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
    const INST: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Uint32x4
    ];
    [
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERT,
        },
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GpuMeshInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INST,
        },
    ]
}

fn sdf_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTR: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Uint32x4];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<GpuSdfInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTR,
    }
}

fn mat(m: Mat4) -> [[f32; 4]; 4] {
    m.to_cols_array_2d()
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let scene_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/scene.wgsl").into()),
        });
        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/post.wgsl").into()),
        });

        let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                uniform_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow globals"),
            entries: &[uniform_entry(0)],
        });

        let globals_size = std::mem::size_of::<Globals>() as u64;
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: globals_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow globals"),
            size: globals_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let lights = DynBuffer::new(
            device,
            "point lights",
            wgpu::BufferUsages::STORAGE,
            (std::mem::size_of::<GpuPointLight>() * MAX_POINT_LIGHTS) as u64,
        );

        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow map"),
            size: wgpu::Extent3d { width: SHADOW_SIZE, height: SHADOW_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&Default::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let globals_bg = Self::make_globals_bg(device, &globals_layout, &globals_buf, &lights.buf, &shadow_view, &shadow_sampler);
        let shadow_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow globals"),
            layout: &shadow_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: shadow_globals_buf.as_entire_binding() }],
        });

        let main_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("main"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow"),
            bind_group_layouts: &[Some(&shadow_layout)],
            immediate_size: 0,
        });

        let color_targets = [
            Some(wgpu::ColorTargetState { format: HDR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
            Some(wgpu::ColorTargetState { format: NORMAL_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
        ];
        let depth_main = Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        });
        let depth_shadow = Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
        });
        let ms = wgpu::MultisampleState { count: SAMPLES, mask: !0, alpha_to_coverage_enabled: false };

        let vl = vertex_layouts();
        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh"),
            layout: Some(&main_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_mesh"),
                compilation_options: Default::default(),
                buffers: &[Some(vl[0].clone()), Some(vl[1].clone())],
            },
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: depth_main.clone(),
            multisample: ms,
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_mesh"),
                compilation_options: Default::default(),
                targets: &color_targets,
            }),
            multiview_mask: None,
            cache: None,
        });
        let sdf_buffers = [sdf_layout()];
        let sdf_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sdf"),
            layout: Some(&main_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_sdf"),
                compilation_options: Default::default(),
                buffers: &[Some(sdf_buffers[0].clone())],
            },
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Front), ..Default::default() },
            depth_stencil: depth_main,
            multisample: ms,
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_sdf"),
                compilation_options: Default::default(),
                targets: &color_targets,
            }),
            multiview_mask: None,
            cache: None,
        });
        let mesh_shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh shadow"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_mesh_shadow"),
                compilation_options: Default::default(),
                buffers: &[Some(vl[0].clone()), Some(vl[1].clone())],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: depth_shadow.clone(),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let sdf_shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sdf shadow"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_sdf"),
                compilation_options: Default::default(),
                buffers: &[Some(sdf_buffers[0].clone())],
            },
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Front), ..Default::default() },
            depth_stencil: depth_shadow,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_sdf_shadow"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });

        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                uniform_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
                    },
                    count: None,
                },
            ],
        });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post"),
            bind_group_layouts: &[Some(&post_layout)],
            immediate_size: 0,
        });
        let post_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post"),
            size: std::mem::size_of::<PostUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mesh_instances = DynBuffer::new(device, "mesh instances", wgpu::BufferUsages::VERTEX, 64 * 1024);
        let sdf_instances = DynBuffer::new(device, "sdf instances", wgpu::BufferUsages::VERTEX, 16 * 1024);

        Self {
            device: device.clone(),
            queue: queue.clone(),
            meshes: HashMap::new(),
            globals_buf,
            shadow_globals_buf,
            lights,
            globals_layout,
            globals_bg,
            shadow_bg,
            shadow_view,
            shadow_sampler,
            mesh_pipeline,
            sdf_pipeline,
            mesh_shadow_pipeline,
            sdf_shadow_pipeline,
            post_layout,
            post_pipeline_layout,
            post_shader,
            post_pipelines: HashMap::new(),
            post_buf,
            targets: None,
            mesh_instances,
            sdf_instances,
            stats: RenderStats::default(),
        }
    }

    fn make_globals_bg(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        globals: &wgpu::Buffer,
        lights: &wgpu::Buffer,
        shadow_view: &wgpu::TextureView,
        shadow_sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: lights.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(shadow_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(shadow_sampler) },
            ],
        })
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Uploads (or replaces) a mesh under `key`.
    pub fn upsert_mesh(&mut self, key: MeshKey, data: &MeshData) {
        let vbuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(&data.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ibuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&data.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.meshes.insert(key, GpuMesh { vbuf, ibuf, index_count: data.indices.len() as u32 });
    }

    pub fn has_mesh(&self, key: MeshKey) -> bool {
        self.meshes.contains_key(&key)
    }

    pub fn remove_mesh(&mut self, key: MeshKey) {
        self.meshes.remove(&key);
    }

    fn ensure_targets(&mut self, size: (u32, u32)) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let extent = wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 };
        let make = |label: &str, format: wgpu::TextureFormat, samples: u32, usage: wgpu::TextureUsages| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let rt = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let tb = wgpu::TextureUsages::TEXTURE_BINDING;
        let hdr_ms = make("hdr ms", HDR_FORMAT, SAMPLES, rt);
        let hdr = make("hdr", HDR_FORMAT, 1, rt | tb);
        let normal_ms = make("normal ms", NORMAL_FORMAT, SAMPLES, rt | tb);
        let depth_ms = make("depth ms", DEPTH_FORMAT, SAMPLES, rt | tb);
        let post_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post"),
            layout: &self.post_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.post_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&hdr) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&depth_ms) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&normal_ms) },
            ],
        });
        self.targets = Some(FrameTargets { size, hdr_ms, hdr, normal_ms, depth_ms, post_bg });
    }

    fn post_pipeline(&mut self, format: wgpu::TextureFormat) -> &wgpu::RenderPipeline {
        if !self.post_pipelines.contains_key(&format) {
            let p = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("post"),
                layout: Some(&self.post_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.post_shader,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.post_shader,
                    entry_point: Some("fs_post"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            });
            self.post_pipelines.insert(format, p);
        }
        &self.post_pipelines[&format]
    }

    /// Light view-projection for the sun, snapped to shadow texels to avoid shimmering.
    fn light_matrix(scene: &Scene) -> (Mat4, f32) {
        let dir = scene.sun.direction.normalize_or(Vec3::NEG_Y);
        let r = scene.sun.shadow_radius.max(1.0);
        let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        let right = dir.cross(up).normalize();
        let up2 = right.cross(dir);
        let texel = 2.0 * r / SHADOW_SIZE as f32;
        let c = scene.sun.shadow_center;
        let (cx, cy) = (c.dot(right), c.dot(up2));
        let snapped = c + right * ((cx / texel).round() * texel - cx) + up2 * ((cy / texel).round() * texel - cy);
        let depth = r * 6.0;
        let view = glam::camera::rh::view::look_to_mat4(snapped - dir * depth * 0.5, dir, up2);
        let proj = glam::camera::rh::proj::directx::orthographic(-r, r, -r, r, 0.0, depth);
        (proj * view, texel)
    }

    /// Records the whole frame into `encoder`, writing the final image into `target`.
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene,
        target: &wgpu::TextureView,
        target_format: wgpu::TextureFormat,
        size: (u32, u32),
    ) {
        self.ensure_targets(size);

        // Make sure every referenced mesh exists.
        for inst in &scene.meshes {
            if !self.meshes.contains_key(&inst.mesh) {
                if let Some(data) = inst.mesh.generate() {
                    self.upsert_mesh(inst.mesh, &data);
                }
            }
        }

        // Globals.
        let cam = &scene.camera;
        let view_proj = cam.proj * cam.view;
        let (light_vp, texel) = Self::light_matrix(scene);
        let sun_dir = scene.sun.direction.normalize_or(Vec3::NEG_Y);
        let n_lights = scene.point_lights.len().min(MAX_POINT_LIGHTS);
        let cut = &scene.cutaway;
        let mut globals = Globals {
            view_proj: mat(view_proj),
            inv_view_proj: mat(view_proj.inverse()),
            light_view_proj: mat(light_vp),
            eye: [cam.eye.x, cam.eye.y, cam.eye.z, if cam.ortho { 0.0 } else { 1.0 }],
            forward: cam.forward.normalize_or(Vec3::NEG_Z).extend(0.0).to_array(),
            sun_dir: [sun_dir.x, sun_dir.y, sun_dir.z, if scene.sun.shadows { 1.0 } else { 0.0 }],
            sun_color: scene.sun.color.extend(1.0).to_array(),
            sky_color: scene.ambient.sky.extend(1.0).to_array(),
            ground_color: scene.ambient.ground.extend(1.0).to_array(),
            cut: [cut.focus.x, cut.focus.y, cut.focus.z, cut.cut_height],
            cut2: [cut.cut_radius, if cut.height_cut { 1.0 } else { 0.0 }, cut.fade_radius, if cut.fade { 1.0 } else { 0.0 }],
            params: [scene.time, n_lights as f32, texel, 0.0],
            style: [scene.style.flat_shadow, scene.style.cel_bands, scene.style.rim, scene.style.specular],
            viewport: [size.0 as f32, size.1 as f32, 1.0 / size.0.max(1) as f32, 1.0 / size.1.max(1) as f32],
        };
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));
        globals.view_proj = mat(light_vp);
        globals.eye = [0.0, 0.0, 0.0, 0.0];
        globals.forward = sun_dir.extend(0.0).to_array();
        globals.cut2 = [0.0; 4];
        self.queue.write_buffer(&self.shadow_globals_buf, 0, bytemuck::bytes_of(&globals));

        let mut lights: Vec<GpuPointLight> = scene.point_lights[..n_lights]
            .iter()
            .map(|l| GpuPointLight {
                pos_radius: [l.position.x, l.position.y, l.position.z, l.radius],
                color: l.color.extend(1.0).to_array(),
            })
            .collect();
        if lights.is_empty() {
            lights.push(GpuPointLight::zeroed());
        }
        if self.lights.write(&self.device, &self.queue, bytemuck::cast_slice(&lights)) {
            self.globals_bg = Self::make_globals_bg(
                &self.device,
                &self.globals_layout,
                &self.globals_buf,
                &self.lights.buf,
                &self.shadow_view,
                &self.shadow_sampler,
            );
        }

        // Instances, grouped by mesh.
        let mut order: Vec<usize> =
            (0..scene.meshes.len()).filter(|&i| self.meshes.contains_key(&scene.meshes[i].mesh)).collect();
        order.sort_by_key(|&i| scene.meshes[i].mesh);
        let mut gpu_meshes = Vec::with_capacity(order.len());
        let mut batches: Vec<(MeshKey, u32, u32, u32, u32)> = Vec::new(); // key, start, end, shadow_start, shadow_end
        // Shadow casters are the same instances; NO_SHADOW ones are skipped by a second ordering.
        for &i in &order {
            let m = &scene.meshes[i];
            let idx = gpu_meshes.len() as u32;
            gpu_meshes.push(GpuMeshInstance {
                model: mat(m.transform),
                color: m.color.extend(m.emissive).to_array(),
                params: [m.style as u32, m.flags, m.group, 0],
            });
            match batches.last_mut() {
                Some(b) if b.0 == m.mesh => b.2 = idx + 1,
                _ => batches.push((m.mesh, idx, idx + 1, 0, 0)),
            }
        }
        // Shadow-casting copies appended after the main list.
        for b in batches.iter_mut() {
            let start = gpu_meshes.len() as u32;
            for k in b.1..b.2 {
                let src = gpu_meshes[k as usize];
                if src.params[1] & crate::scene::flags::NO_SHADOW == 0 {
                    gpu_meshes.push(src);
                }
            }
            b.3 = start;
            b.4 = gpu_meshes.len() as u32;
        }
        self.mesh_instances.write(&self.device, &self.queue, bytemuck::cast_slice(&gpu_meshes));

        let mut gpu_sdfs: Vec<GpuSdfInstance> = scene
            .sdfs
            .iter()
            .map(|s| GpuSdfInstance {
                a: [s.a.x, s.a.y, s.a.z, s.ra],
                b: [s.b.x, s.b.y, s.b.z, s.rb],
                color: s.color.extend(s.emissive).to_array(),
                params: [s.style as u32, s.flags, s.group, 0],
            })
            .collect();
        let sdf_count = gpu_sdfs.len() as u32;
        gpu_sdfs.extend(scene.sdfs.iter().filter(|s| s.flags & crate::scene::flags::NO_SHADOW == 0).map(|s| GpuSdfInstance {
            a: [s.a.x, s.a.y, s.a.z, s.ra],
            b: [s.b.x, s.b.y, s.b.z, s.rb],
            color: [0.0; 4],
            params: [0; 4],
        }));
        let sdf_shadow_end = gpu_sdfs.len() as u32;
        self.sdf_instances.write(&self.device, &self.queue, bytemuck::cast_slice(&gpu_sdfs));

        let mut draw_calls = 0;

        // 1. Shadow pass.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if scene.sun.shadows {
                pass.set_bind_group(0, &self.shadow_bg, &[]);
                pass.set_pipeline(&self.mesh_shadow_pipeline);
                pass.set_vertex_buffer(1, self.mesh_instances.buf.slice(..));
                for b in &batches {
                    if b.4 > b.3 {
                        let mesh = &self.meshes[&b.0];
                        pass.set_vertex_buffer(0, mesh.vbuf.slice(..));
                        pass.set_index_buffer(mesh.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..mesh.index_count, 0, b.3..b.4);
                        draw_calls += 1;
                    }
                }
                if sdf_shadow_end > sdf_count {
                    pass.set_pipeline(&self.sdf_shadow_pipeline);
                    pass.set_vertex_buffer(0, self.sdf_instances.buf.slice(..));
                    pass.draw(0..36, sdf_count..sdf_shadow_end);
                    draw_calls += 1;
                }
            }
        }

        // 2. Scene pass.
        let t = self.targets.as_ref().expect("targets");
        {
            let cc = scene.clear_color;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &t.hdr_ms,
                        depth_slice: None,
                        resolve_target: Some(&t.hdr),
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color { r: cc.x as f64, g: cc.y as f64, b: cc.z as f64, a: 1.0 }),
                            store: wgpu::StoreOp::Discard,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &t.normal_ms,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth_ms,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bg, &[]);
            pass.set_pipeline(&self.mesh_pipeline);
            pass.set_vertex_buffer(1, self.mesh_instances.buf.slice(..));
            for b in &batches {
                let mesh = &self.meshes[&b.0];
                pass.set_vertex_buffer(0, mesh.vbuf.slice(..));
                pass.set_index_buffer(mesh.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, b.1..b.2);
                draw_calls += 1;
            }
            if sdf_count > 0 {
                pass.set_pipeline(&self.sdf_pipeline);
                pass.set_vertex_buffer(0, self.sdf_instances.buf.slice(..));
                pass.draw(0..36, 0..sdf_count);
                draw_calls += 1;
            }
        }

        // 3. Composite.
        let p = &scene.post;
        let post = PostUniform {
            inv_proj: mat(cam.proj.inverse()),
            outline_color: match p.outline_color {
                Some(c) => c.extend(1.0).to_array(),
                None => [0.0; 4],
            },
            outline: [p.outline_px, p.outline_depth, p.outline_normal, if p.outlines { 1.0 } else { 0.0 }],
            tone: [
                p.exposure,
                match p.tonemap {
                    Tonemap::Clamp => 0.0,
                    Tonemap::SoftKnee => 1.0,
                    Tonemap::Aces => 2.0,
                },
                if target_format.is_srgb() { 0.0 } else { 1.0 },
                p.saturation,
            ],
            misc: [p.outline_darken, 0.0, 0.0, 0.0],
            fwd: cam.forward.normalize_or(Vec3::NEG_Z).extend(0.0).to_array(),
        };
        self.queue.write_buffer(&self.post_buf, 0, bytemuck::bytes_of(&post));
        self.post_pipeline(target_format);
        let t = self.targets.as_ref().expect("targets");
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.post_pipelines[&target_format]);
            pass.set_bind_group(0, &t.post_bg, &[]);
            pass.draw(0..3, 0..1);
            draw_calls += 1;
        }

        self.stats = RenderStats {
            mesh_instances: order.len(),
            sdf_instances: sdf_count as usize,
            draw_calls,
            meshes_loaded: self.meshes.len(),
        };
    }
}
