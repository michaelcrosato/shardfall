//! Effects around the scene pass: HDR bloom (a mip chain), screen distortion (shockwaves, heat
//! haze, lenses, ripples) and GPU particles (born on the CPU, integrated by a compute shader,
//! drawn as camera-facing sprites inside the MSAA scene pass). All cosmetic: nothing here feeds
//! back into the simulation.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::renderer::{DEPTH_FORMAT, DynBuffer, HDR_FORMAT, NORMAL_FORMAT, SAMPLES};
use crate::scene::{CameraData, Distortion, ParticleBurst};

pub const BLOOM_LEVELS: usize = 5;
pub const MAX_PARTICLES: u32 = 1 << 16;
pub const DISTORT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

/// Camera data shared by the distortion and particle shaders.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FxCam {
    view_proj: [[f32; 4]; 4],
    right: [f32; 4],
    up: [f32; 4],
    eye: [f32; 4],
}

impl FxCam {
    fn new(cam: &CameraData, time: f32) -> Self {
        let inv = cam.view.inverse();
        let right = inv.x_axis.truncate().normalize_or(Vec3::X);
        let up = inv.y_axis.truncate().normalize_or(Vec3::Y);
        Self {
            view_proj: (cam.proj * cam.view).to_cols_array_2d(),
            right: right.extend(0.0).to_array(),
            up: up.extend(0.0).to_array(),
            eye: cam.eye.extend(time).to_array(),
        }
    }
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn storage_entry(binding: u32, read_only: bool, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn buffer(device: &wgpu::Device, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn shader(device: &wgpu::Device, label: &str, src: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(src.into()) })
}

fn fullscreen_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    fs: &str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

const ADD: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

fn pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    label: &str,
    view: &'a wgpu::TextureView,
    clear: bool,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: if clear { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load },
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

// ------------------------------------------------------------------------------------- bloom

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BloomUniform {
    src_texel: [f32; 2],
    dst_texel: [f32; 2],
    threshold: f32,
    knee: f32,
    pad: [f32; 2],
}

struct BloomTargets {
    views: Vec<wgpu::TextureView>,
    /// Pass i writes level i: from the HDR image (i = 0) or level i - 1.
    down: Vec<(wgpu::BindGroup, wgpu::Buffer)>,
    /// Pass j adds level j + 1 onto level j.
    up: Vec<(wgpu::BindGroup, wgpu::Buffer)>,
}

pub(crate) struct Bloom {
    layout: wgpu::BindGroupLayout,
    prefilter: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    targets: Option<BloomTargets>,
}

impl Bloom {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = shader(device, "bloom", include_str!("shaders/bloom.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::FRAGMENT), texture_entry(1), sampler_entry(2)],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bloom"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        Self {
            prefilter: fullscreen_pipeline(device, &pl, &module, "fs_prefilter", HDR_FORMAT, None),
            down: fullscreen_pipeline(device, &pl, &module, "fs_down", HDR_FORMAT, None),
            up: fullscreen_pipeline(device, &pl, &module, "fs_up", HDR_FORMAT, Some(ADD)),
            layout,
            sampler: linear_sampler(device),
            targets: None,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32), hdr: &wgpu::TextureView) {
        let mut views = Vec::new();
        let mut sizes = Vec::new();
        let (mut w, mut h) = size;
        for i in 0..BLOOM_LEVELS {
            w = (w / 2).max(1);
            h = (h / 2).max(1);
            sizes.push((w, h));
            views.push(
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(&format!("bloom {i}")),
                        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: HDR_FORMAT,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&Default::default()),
            );
        }
        let texel = |s: (u32, u32)| [1.0 / s.0 as f32, 1.0 / s.1 as f32];
        let make = |src: &wgpu::TextureView, src_size: (u32, u32), dst_size: (u32, u32)| {
            let buf = buffer(device, "bloom pass", std::mem::size_of::<BloomUniform>() as u64, wgpu::BufferUsages::UNIFORM);
            let u =
                BloomUniform { src_texel: texel(src_size), dst_texel: texel(dst_size), threshold: 1.0, knee: 0.5, pad: [0.0; 2] };
            queue.write_buffer(&buf, 0, bytemuck::bytes_of(&u));
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(src) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            (bg, buf)
        };
        let mut down = Vec::new();
        for i in 0..BLOOM_LEVELS {
            let (src, src_size) = if i == 0 { (hdr, size) } else { (&views[i - 1], sizes[i - 1]) };
            down.push(make(src, src_size, sizes[i]));
        }
        let mut up = Vec::new();
        for j in 0..BLOOM_LEVELS - 1 {
            up.push(make(&views[j + 1], sizes[j + 1], sizes[j]));
        }
        self.targets = Some(BloomTargets { views, down, up });
    }

    /// The finished glow (half resolution), for the composite.
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.views[0])
    }

    pub fn record(&self, encoder: &mut wgpu::CommandEncoder, queue: &wgpu::Queue, threshold: f32) {
        let Some(t) = &self.targets else { return };
        // Threshold of the bright pass (pass 0) can change every frame.
        queue.write_buffer(&t.down[0].1, 16, bytemuck::cast_slice(&[threshold, (threshold * 0.5).max(0.05)]));
        for (i, (bg, _)) in t.down.iter().enumerate() {
            let mut p = pass(encoder, "bloom down", &t.views[i], true);
            p.set_pipeline(if i == 0 { &self.prefilter } else { &self.down });
            p.set_bind_group(0, bg, &[]);
            p.draw(0..3, 0..1);
        }
        for j in (0..BLOOM_LEVELS - 1).rev() {
            let mut p = pass(encoder, "bloom up", &t.views[j], false);
            p.set_pipeline(&self.up);
            p.set_bind_group(0, &t.up[j].0, &[]);
            p.draw(0..3, 0..1);
        }
    }
}

pub(crate) fn linear_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("linear clamp"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

// --------------------------------------------------------------------------------- distortion

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuDistortion {
    pos_radius: [f32; 4],
    params: [f32; 4],
}

pub(crate) struct Distort {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    cam: wgpu::Buffer,
    items: DynBuffer,
    bg: Option<wgpu::BindGroup>,
    target: Option<wgpu::TextureView>,
}

impl Distort {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = shader(device, "distort", include_str!("shaders/distort.wgsl"));
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("distort"),
            entries: &[uniform_entry(0, vf), storage_entry(1, true, vf)],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("distort"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("distort"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: DISTORT_FORMAT,
                    blend: Some(ADD),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            layout,
            pipeline,
            cam: buffer(device, "distort cam", std::mem::size_of::<FxCam>() as u64, wgpu::BufferUsages::UNIFORM),
            items: DynBuffer::new(device, "distortions", wgpu::BufferUsages::STORAGE, 64 * 32),
            bg: None,
            target: None,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        self.target = Some(
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("distortion"),
                    size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: DISTORT_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default()),
        );
    }

    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref()
    }

    /// Draws the sources; returns whether there were any (the composite skips the lookup if not).
    pub fn record(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cam: &CameraData,
        time: f32,
        list: &[Distortion],
    ) -> bool {
        let Some(target) = &self.target else { return false };
        if list.is_empty() {
            return false;
        }
        let items: Vec<GpuDistortion> = list
            .iter()
            .map(|d| GpuDistortion {
                pos_radius: d.pos.extend(d.radius.max(0.01)).to_array(),
                params: [d.strength, d.progress, d.kind as u32 as f32, 0.0],
            })
            .collect();
        queue.write_buffer(&self.cam, 0, bytemuck::bytes_of(&FxCam::new(cam, time)));
        if self.items.write(device, queue, bytemuck::cast_slice(&items)) || self.bg.is_none() {
            self.bg = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("distort"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.cam.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.items.buf.as_entire_binding() },
                ],
            }));
        }
        let mut p = pass(encoder, "distortion", target, true);
        p.set_pipeline(&self.pipeline);
        p.set_bind_group(0, self.bg.as_ref().unwrap(), &[]);
        p.draw(0..6, 0..items.len() as u32);
        true
    }
}

// ---------------------------------------------------------------------------------- particles

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuParticle {
    p0: [f32; 4],
    p1: [f32; 4],
    c0: [f32; 4],
    c1: [f32; 4],
    s: [f32; 4],
    t: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SimUniform {
    dt: f32,
    time: f32,
    count: u32,
    pad: u32,
}

pub(crate) struct Particles {
    buf: wgpu::Buffer,
    sim: wgpu::Buffer,
    cam: wgpu::Buffer,
    update: wgpu::ComputePipeline,
    update_bg: wgpu::BindGroup,
    draw: wgpu::RenderPipeline,
    draw_bg: wgpu::BindGroup,
    /// Next ring slot, and how many slots have ever been used (draw/update range).
    head: u32,
    used: u32,
    last_time: Option<f32>,
    rng: u64,
}

impl Particles {
    pub fn new(device: &wgpu::Device) -> Self {
        let stride = std::mem::size_of::<GpuParticle>() as u64;
        let buf = buffer(device, "particles", stride * MAX_PARTICLES as u64, wgpu::BufferUsages::STORAGE);
        let sim = buffer(device, "particle sim", std::mem::size_of::<SimUniform>() as u64, wgpu::BufferUsages::UNIFORM);
        let cam = buffer(device, "particle cam", std::mem::size_of::<FxCam>() as u64, wgpu::BufferUsages::UNIFORM);

        let cs = shader(device, "particle update", include_str!("shaders/particle_update.wgsl"));
        let ul = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle update"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::COMPUTE), storage_entry(1, false, wgpu::ShaderStages::COMPUTE)],
        });
        let update = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("particle update"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("particle update"),
                bind_group_layouts: &[Some(&ul)],
                immediate_size: 0,
            })),
            module: &cs,
            entry_point: Some("cs_update"),
            compilation_options: Default::default(),
            cache: None,
        });
        let update_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle update"),
            layout: &ul,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: sim.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: buf.as_entire_binding() },
            ],
        });

        let ds = shader(device, "particle draw", include_str!("shaders/particle_draw.wgsl"));
        let dl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle draw"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX), storage_entry(1, true, wgpu::ShaderStages::VERTEX)],
        });
        let premul = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let draw = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("particle draw"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("particle draw"),
                bind_group_layouts: &[Some(&dl)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &ds,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLES, mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module: &ds,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState { format: HDR_FORMAT, blend: Some(premul), write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: NORMAL_FORMAT, blend: None, write_mask: wgpu::ColorWrites::empty() }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        let draw_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle draw"),
            layout: &dl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: cam.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: buf.as_entire_binding() },
            ],
        });
        Self { buf, sim, cam, update, update_bg, draw, draw_bg, head: 0, used: 0, last_time: None, rng: 0x9e37_79b9_7f4a_7c15 }
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        ((self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32) / (1u64 << 24) as f32
    }

    fn signed(&mut self) -> f32 {
        self.rand() * 2.0 - 1.0
    }

    /// Births this frame's particles and advances everything (records a compute pass).
    pub fn prepare(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        cam: &CameraData,
        time: f32,
        bursts: &[ParticleBurst],
    ) {
        let dt = match self.last_time {
            Some(t) => (time - t).clamp(0.0, 0.1),
            None => 0.0,
        };
        self.last_time = Some(time);
        let mut born: Vec<GpuParticle> = Vec::new();
        for b in bursts {
            for _ in 0..b.count.min(MAX_PARTICLES / 4) {
                let off = Vec3::new(self.signed(), self.signed(), self.signed()) * b.area;
                let dir = loop {
                    let d = Vec3::new(self.signed(), self.signed(), self.signed());
                    if d.length_squared() <= 1.0 {
                        break d;
                    }
                };
                let mut vel = b.vel + dir * b.spread;
                let life = b.life.0 + (b.life.1 - b.life.0) * self.rand();
                let mut pos = b.pos + off;
                let mut left = life;
                if b.prewarm > 0.0 {
                    // Already flying for a while (drag and turbulence ignored).
                    let age = self.rand() * b.prewarm.min(life) * 0.95;
                    let g = Vec3::Y * -b.gravity;
                    pos += vel * age + g * 0.5 * age * age;
                    vel += g * age;
                    if let Some(f) = b.floor {
                        pos.y = pos.y.max(f);
                    }
                    left = life - age;
                }
                let flags = (b.additive as u32) | ((b.stretch as u32) << 1);
                born.push(GpuParticle {
                    p0: pos.extend(left.max(0.01)).to_array(),
                    p1: vel.extend(life.max(0.01)).to_array(),
                    c0: b.color0.to_array(),
                    c1: b.color1.to_array(),
                    s: [b.size.0, b.size.1, b.gravity, b.drag],
                    t: [b.floor.unwrap_or(-1.0e9), b.bounce, b.turbulence, flags as f32],
                });
            }
        }
        let stride = std::mem::size_of::<GpuParticle>() as u64;
        let mut rest = &born[..];
        while !rest.is_empty() {
            let room = (MAX_PARTICLES - self.head) as usize;
            let n = rest.len().min(room);
            queue.write_buffer(&self.buf, self.head as u64 * stride, bytemuck::cast_slice(&rest[..n]));
            self.head = (self.head + n as u32) % MAX_PARTICLES;
            self.used = (self.used + n as u32).min(MAX_PARTICLES);
            rest = &rest[n..];
        }
        if self.used == 0 {
            return;
        }
        queue.write_buffer(&self.sim, 0, bytemuck::bytes_of(&SimUniform { dt, time, count: self.used, pad: 0 }));
        queue.write_buffer(&self.cam, 0, bytemuck::bytes_of(&FxCam::new(cam, time)));
        if dt > 0.0 {
            let mut cp =
                encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("particles"), timestamp_writes: None });
            cp.set_pipeline(&self.update);
            cp.set_bind_group(0, &self.update_bg, &[]);
            cp.dispatch_workgroups(self.used.div_ceil(64), 1, 1);
        }
    }

    /// Draws the particles inside the scene pass.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) -> bool {
        if self.used == 0 {
            return false;
        }
        pass.set_pipeline(&self.draw);
        pass.set_bind_group(0, &self.draw_bg, &[]);
        pass.draw(0..6, 0..self.used);
        true
    }

    /// Particles alive somewhere in the ring (upper bound).
    pub fn slots(&self) -> u32 {
        self.used
    }
}
