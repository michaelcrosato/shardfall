//! Shadows for a few point lights: six perspective depth maps per light (one per axis
//! direction) in a 2D array. The scene shader picks the face by the dominant axis and samples it
//! with the same matrix it was rendered with, so no cube-map conventions are involved.

use glam::{Mat4, Vec3};

use crate::renderer::DEPTH_FORMAT;

pub const POINT_SHADOW_SIZE: u32 = 512;
pub const MAX_SHADOW_LIGHTS: usize = 4;
const LAYERS: u32 = (MAX_SHADOW_LIGHTS * 6) as u32;
const NEAR: f32 = 0.05;

pub(crate) struct PointShadows {
    /// The whole array, for sampling.
    pub view: wgpu::TextureView,
    /// One view per face, for rendering.
    pub layers: Vec<wgpu::TextureView>,
    /// Per-face uniform buffer + bind group (shadow pipeline layout).
    pub faces: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    /// View-projection of every face (light slot * 6 + face), read by the scene shader.
    pub mats: wgpu::Buffer,
}

impl PointShadows {
    pub fn new(device: &wgpu::Device, shadow_layout: &wgpu::BindGroupLayout, globals_size: u64) -> Self {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("point shadows"),
            size: wgpu::Extent3d { width: POINT_SHADOW_SIZE, height: POINT_SHADOW_SIZE, depth_or_array_layers: LAYERS },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("point shadows"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let layers = (0..LAYERS)
            .map(|i| {
                tex.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("point shadow face"),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: i,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let faces = (0..LAYERS)
            .map(|_| {
                let buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("point shadow globals"),
                    size: globals_size,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("point shadow globals"),
                    layout: shadow_layout,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
                });
                (buf, bg)
            })
            .collect();
        let mats = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point shadow matrices"),
            size: 64 * LAYERS as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { view, layers, faces, mats }
    }
}

/// The six face directions and view-projections of a light at `pos` reaching `radius`.
pub fn face_matrices(pos: Vec3, radius: f32) -> [(Vec3, Mat4); 6] {
    let proj = glam::camera::rh::proj::directx::perspective(std::f32::consts::FRAC_PI_2, 1.0, NEAR, radius.max(NEAR * 4.0));
    let dirs = [
        (Vec3::X, Vec3::Y),
        (Vec3::NEG_X, Vec3::Y),
        (Vec3::Y, Vec3::Z),
        (Vec3::NEG_Y, Vec3::Z),
        (Vec3::Z, Vec3::Y),
        (Vec3::NEG_Z, Vec3::Y),
    ];
    dirs.map(|(d, up)| (d, proj * glam::camera::rh::view::look_to_mat4(pos, d, up)))
}
