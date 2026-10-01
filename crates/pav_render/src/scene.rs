//! Plain-data description of one frame. The renderer knows nothing about the simulation;
//! a view layer turns game state into a `Scene` every frame.

use glam::{Mat4, Vec3};

pub use crate::mesh::MeshKey;

/// Surface style, per object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum Style {
    /// Albedo only (plus cast shadows as a darker tone): the clean 2D look.
    Flat = 0,
    /// Toon bands, rim light.
    #[default]
    Cel = 1,
    /// Smooth lighting with specular.
    Lit = 2,
    /// Pure albedo, ignores all lighting and shadows (UI-like markers, emissive signs).
    Unlit = 3,
}

impl Style {
    pub const ALL: [Style; 4] = [Style::Flat, Style::Cel, Style::Lit, Style::Unlit];
    pub fn name(self) -> &'static str {
        match self {
            Style::Flat => "flat",
            Style::Cel => "cel",
            Style::Lit => "lit",
            Style::Unlit => "unlit",
        }
    }
}

pub mod flags {
    /// Never removed by cutaway or occlusion fade (the player, markers).
    pub const NO_CUT: u32 = 1;
    /// Does not cast shadows.
    pub const NO_SHADOW: u32 = 2;
    /// Does not receive shadows.
    pub const NO_RECEIVE_SHADOW: u32 = 4;
    /// Cutaway lowers individual vertices near the player (for big meshes like terrain).
    pub const CUT_VERTEX: u32 = 8;
}

#[derive(Clone, Copy, Debug)]
pub struct MeshInstance {
    pub mesh: MeshKey,
    pub transform: Mat4,
    /// Linear RGB albedo.
    pub color: Vec3,
    /// Emissive strength (0 = none). Adds `color * emissive` on top of lighting.
    pub emissive: f32,
    pub style: Style,
    pub flags: u32,
    /// Outline group: edges are drawn where the group changes. 0 = background, use 1 for
    /// static level geometry and unique ids for objects.
    pub group: u32,
}

/// Analytic SDF primitive, ray-traced per pixel: a rounded cone between `a` and `b` with
/// radii `ra`/`rb`. Equal radii give a capsule; `a == b` gives a sphere.
#[derive(Clone, Copy, Debug)]
pub struct SdfInstance {
    pub a: Vec3,
    pub b: Vec3,
    pub ra: f32,
    pub rb: f32,
    pub color: Vec3,
    pub emissive: f32,
    pub style: Style,
    pub flags: u32,
    pub group: u32,
}

impl SdfInstance {
    pub fn sphere(center: Vec3, r: f32, color: Vec3) -> Self {
        Self { a: center, b: center, ra: r, rb: r, color, emissive: 0.0, style: Style::Cel, flags: 0, group: 1 }
    }
    pub fn capsule(a: Vec3, b: Vec3, r: f32, color: Vec3) -> Self {
        Self { a, b, ra: r, rb: r, color, emissive: 0.0, style: Style::Cel, flags: 0, group: 1 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CameraData {
    pub view: Mat4,
    pub proj: Mat4,
    pub eye: Vec3,
    pub forward: Vec3,
    pub ortho: bool,
}

impl Default for CameraData {
    fn default() -> Self {
        let eye = Vec3::new(0.0, 10.0, 10.0);
        let forward = (-eye).normalize();
        Self {
            view: glam::camera::rh::view::look_to_mat4(eye, forward, Vec3::Y),
            proj: glam::camera::rh::proj::directx::perspective(45f32.to_radians(), 16.0 / 9.0, 0.1, 500.0),
            eye,
            forward,
            ortho: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Sun {
    /// Direction the light travels (from the sun toward the ground).
    pub direction: Vec3,
    /// Linear RGB, already multiplied by intensity.
    pub color: Vec3,
    pub shadows: bool,
    /// Shadows are computed in a square region around this point.
    pub shadow_center: Vec3,
    pub shadow_radius: f32,
}

impl Default for Sun {
    fn default() -> Self {
        Self {
            direction: Vec3::new(-0.35, -1.0, -0.55).normalize(),
            color: Vec3::new(1.0, 0.95, 0.85) * 0.55,
            shadows: true,
            shadow_center: Vec3::ZERO,
            shadow_radius: 30.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PointLight {
    pub position: Vec3,
    /// Linear RGB times intensity.
    pub color: Vec3,
    pub radius: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Ambient {
    pub sky: Vec3,
    pub ground: Vec3,
}

impl Default for Ambient {
    fn default() -> Self {
        Self { sky: Vec3::new(0.52, 0.56, 0.66), ground: Vec3::new(0.36, 0.33, 0.30) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tonemap {
    Clamp = 0,
    /// Identity below 0.8, smooth roll-off above: keeps authored colors exact.
    SoftKnee = 1,
    Aces = 2,
}

#[derive(Clone, Copy, Debug)]
pub struct StyleSettings {
    /// Brightness of shadowed areas in the flat style (0..1).
    pub flat_shadow: f32,
    /// Number of light bands in the cel style.
    pub cel_bands: f32,
    pub rim: f32,
    pub specular: f32,
}

impl Default for StyleSettings {
    fn default() -> Self {
        Self { flat_shadow: 0.62, cel_bands: 2.0, rim: 0.25, specular: 0.35 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PostSettings {
    pub outlines: bool,
    /// Outline thickness in pixels.
    pub outline_px: f32,
    /// Relative depth jump that counts as an edge.
    pub outline_depth: f32,
    /// Normal dot product below which a crease counts as an edge.
    pub outline_normal: f32,
    /// Outline = pixel color * darken (when `outline_color` is None).
    pub outline_darken: f32,
    pub outline_color: Option<Vec3>,
    pub exposure: f32,
    pub tonemap: Tonemap,
    pub saturation: f32,
}

impl Default for PostSettings {
    fn default() -> Self {
        Self {
            outlines: true,
            outline_px: 1.0,
            outline_depth: 0.02,
            outline_normal: 0.6,
            outline_darken: 0.35,
            outline_color: None,
            exposure: 1.0,
            tonemap: Tonemap::SoftKnee,
            saturation: 1.0,
        }
    }
}

/// Distance fog around a centre point (usually the camera target), blending into `color`.
#[derive(Clone, Copy, Debug)]
pub struct Fog {
    pub enabled: bool,
    pub center: Vec3,
    /// Horizontal distance where fog starts / is complete (m).
    pub start: f32,
    pub end: f32,
    /// Linear RGB (usually the sky colour).
    pub color: Vec3,
}

impl Default for Fog {
    fn default() -> Self {
        Self { enabled: false, center: Vec3::ZERO, start: 60.0, end: 90.0, color: Vec3::ONE }
    }
}

/// Camera helpers that remove geometry hiding the player.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cutaway {
    /// Point of interest (usually the player's head).
    pub focus: Vec3,
    /// Remove geometry above `cut_height` within `cut_radius` (horizontal) of the focus.
    pub height_cut: bool,
    pub cut_height: f32,
    pub cut_radius: f32,
    /// Dither out geometry between the camera and the focus.
    pub fade: bool,
    pub fade_radius: f32,
    /// Remove everything closer to the camera than this distance in front of the focus
    /// (side views, where walls between camera and player hide everything). 0 = off.
    pub front_cut: f32,
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub camera: CameraData,
    pub sun: Sun,
    pub ambient: Ambient,
    pub point_lights: Vec<PointLight>,
    pub meshes: Vec<MeshInstance>,
    pub sdfs: Vec<SdfInstance>,
    /// Linear RGB.
    pub clear_color: Vec3,
    pub style: StyleSettings,
    pub post: PostSettings,
    pub cutaway: Cutaway,
    pub fog: Fog,
    /// Data for every `MeshKey::Custom` referenced this frame (uploaded when missing).
    pub custom_meshes: Vec<(MeshKey, std::sync::Arc<crate::mesh::MeshData>)>,
    /// Text in the world.
    pub texts: Vec<crate::text::Text3d>,
    pub time: f32,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            camera: CameraData::default(),
            sun: Sun::default(),
            ambient: Ambient::default(),
            point_lights: Vec::new(),
            meshes: Vec::new(),
            sdfs: Vec::new(),
            clear_color: srgb(0x8f, 0xb8, 0xd8),
            style: StyleSettings::default(),
            post: PostSettings::default(),
            cutaway: Cutaway::default(),
            fog: Fog::default(),
            custom_meshes: Vec::new(),
            texts: Vec::new(),
            time: 0.0,
        }
    }
}

/// Converts an 8-bit sRGB color to linear RGB.
pub fn srgb(r: u8, g: u8, b: u8) -> Vec3 {
    Vec3::new(srgb_to_linear(r as f32 / 255.0), srgb_to_linear(g as f32 / 255.0), srgb_to_linear(b as f32 / 255.0))
}

/// Parses "#rrggbb" (or "rrggbb") into linear RGB.
pub fn hex(s: &str) -> Option<Vec3> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(srgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}
