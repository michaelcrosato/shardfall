//! Scene building: interpolates between two simulation frames and emits render instances.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};
use pav_core::params::{ChoiceParam, ParamVisitor, Tunable, nested};
use pav_core::statics::{ChunkKey, block_flags};
use pav_core::{Color, Look, RenderFrame, RenderObject, Shape, choice_enum};
use pav_render::scene::{self as rs, MeshInstance, MeshKey, Scene, SdfInstance, Style, Tonemap};
use serde::{Deserialize, Serialize};

use crate::camera::CameraRig;

choice_enum! {
    pub enum StyleOverride { PerObject => "per object", Flat => "flat", Cel => "cel", Lit => "lit" }
}

choice_enum! {
    pub enum TonemapChoice { Clamp => "clamp", SoftKnee => "soft knee", Aces => "aces" }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LightSettings {
    /// Sun height above the horizon (degrees).
    pub sun_elevation: f32,
    /// Sun compass direction (degrees).
    pub sun_azimuth: f32,
    pub sun_intensity: f32,
    pub ambient: f32,
    pub shadows: bool,
    pub shadow_radius: f32,
}

impl Default for LightSettings {
    fn default() -> Self {
        Self { sun_elevation: 58.0, sun_azimuth: 215.0, sun_intensity: 0.55, ambient: 1.0, shadows: true, shadow_radius: 30.0 }
    }
}

impl Tunable for LightSettings {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.float("sun_elevation", &mut self.sun_elevation, 5.0, 90.0, "Sun height (degrees)");
        v.float("sun_azimuth", &mut self.sun_azimuth, 0.0, 360.0, "Sun compass direction (degrees)");
        v.float("sun_intensity", &mut self.sun_intensity, 0.0, 2.0, "Direct light strength");
        v.float("ambient", &mut self.ambient, 0.0, 2.0, "Sky/ground fill light multiplier");
        v.bool("shadows", &mut self.shadows, "Sun shadows");
        v.float("shadow_radius", &mut self.shadow_radius, 5.0, 120.0, "Shadowed area around the camera (m)");
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CutawaySettings {
    pub height_cut: bool,
    /// Cut geometry this far above the player's feet (m).
    pub cut_above: f32,
    pub cut_radius: f32,
    pub fade: bool,
    pub fade_radius: f32,
}

impl Default for CutawaySettings {
    fn default() -> Self {
        Self { height_cut: true, cut_above: 2.4, cut_radius: 9.0, fade: true, fade_radius: 1.6 }
    }
}

impl Tunable for CutawaySettings {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.bool("height_cut", &mut self.height_cut, "Hide geometry above the player (upper floors, roofs)");
        v.float("cut_above", &mut self.cut_above, 0.5, 10.0, "Cut height above the player's feet (m)");
        v.float("cut_radius", &mut self.cut_radius, 1.0, 40.0, "Horizontal radius of the cut (m)");
        v.bool("fade", &mut self.fade, "Dither out geometry between camera and player");
        v.float("fade_radius", &mut self.fade_radius, 0.2, 6.0, "Radius of the see-through tunnel (m)");
    }
}

/// Everything visual that is tunable live ("shaders" group in the panel).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewSettings {
    pub style: StyleOverride,
    pub outlines: bool,
    pub outline_px: f32,
    pub outline_darken: f32,
    pub cel_bands: f32,
    pub flat_shadow: f32,
    pub rim: f32,
    pub specular: f32,
    pub exposure: f32,
    pub tonemap: TonemapChoice,
    pub saturation: f32,
    pub sky: String,
    pub light: LightSettings,
    pub cutaway: CutawaySettings,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            style: StyleOverride::PerObject,
            outlines: true,
            outline_px: 1.0,
            outline_darken: 0.35,
            cel_bands: 2.0,
            flat_shadow: 0.62,
            rim: 0.25,
            specular: 0.35,
            exposure: 1.0,
            tonemap: TonemapChoice::SoftKnee,
            saturation: 1.0,
            sky: "#8fb8d8".into(),
            light: LightSettings::default(),
            cutaway: CutawaySettings::default(),
        }
    }
}

impl Tunable for ViewSettings {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        self.style.visit_choice(v, "style", "Force one surface style everywhere");
        v.bool("outlines", &mut self.outlines, "Screen-space outlines");
        v.float("outline_px", &mut self.outline_px, 1.0, 4.0, "Outline thickness (pixels)");
        v.float("outline_darken", &mut self.outline_darken, 0.0, 1.0, "Outline brightness relative to the surface");
        v.float("cel_bands", &mut self.cel_bands, 2.0, 6.0, "Light bands in cel style");
        v.float("flat_shadow", &mut self.flat_shadow, 0.0, 1.0, "Shadow brightness in flat style");
        v.float("rim", &mut self.rim, 0.0, 1.0, "Rim light (cel)");
        v.float("specular", &mut self.specular, 0.0, 2.0, "Specular highlights (lit)");
        v.float("exposure", &mut self.exposure, 0.2, 3.0, "Exposure");
        self.tonemap.visit_choice(v, "tonemap", "HDR to screen mapping");
        v.float("saturation", &mut self.saturation, 0.0, 2.0, "Color saturation");
        nested(v, "light", &mut self.light);
        nested(v, "cutaway", &mut self.cutaway);
    }
}

fn v3(c: Color) -> Vec3 {
    Vec3::from(c.0)
}

fn style_of(look: Look, ov: StyleOverride) -> Style {
    match ov {
        StyleOverride::Flat => return Style::Flat,
        StyleOverride::Cel => return Style::Cel,
        StyleOverride::Lit => return Style::Lit,
        StyleOverride::PerObject => {}
    }
    match look {
        Look::Flat => Style::Flat,
        Look::Cel => Style::Cel,
        Look::Lit => Style::Lit,
        Look::Unlit => Style::Unlit,
    }
}

/// Keeps per-chunk instance caches between frames.
#[derive(Default)]
pub struct ViewBuilder {
    static_cache: HashMap<ChunkKey, (u64, StyleOverride, Vec<MeshInstance>)>,
}

/// Shortest-arc interpolation of object poses between two frames (both sorted by id).
pub fn interpolate(prev: &RenderFrame, curr: &RenderFrame, alpha: f32) -> Vec<RenderObject> {
    let mut out = Vec::with_capacity(curr.objects.len());
    let mut j = 0;
    for o in &curr.objects {
        while j < prev.objects.len() && prev.objects[j].id < o.id {
            j += 1;
        }
        let mut obj = o.clone();
        if let Some(p) = prev.objects.get(j).filter(|p| p.id == o.id) {
            // Large jumps (teleports, respawns) are not smoothed.
            if p.pos.distance_squared(o.pos) < 4.0 {
                obj.pos = p.pos.lerp(o.pos, alpha);
                obj.rot = p.rot.slerp(o.rot, alpha);
            }
        }
        out.push(obj);
    }
    out
}

impl ViewBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the render scene. `alpha` in [0,1] blends `prev` -> `curr`.
    pub fn build(
        &mut self,
        prev: &RenderFrame,
        curr: &RenderFrame,
        alpha: f32,
        rig: &CameraRig,
        aspect: f32,
        settings: &ViewSettings,
        focus: Vec3,
    ) -> Scene {
        let mut scene = Scene::default();
        scene.camera = rig.data(aspect);
        scene.time = curr.time as f32;

        let l = &settings.light;
        let (el, az) = (l.sun_elevation.to_radians(), l.sun_azimuth.to_radians());
        let to_sun = Vec3::new(az.sin() * el.cos(), el.sin(), -az.cos() * el.cos());
        scene.sun.direction = -to_sun;
        scene.sun.color = Vec3::new(1.0, 0.95, 0.86) * l.sun_intensity;
        scene.sun.shadows = l.shadows;
        scene.sun.shadow_radius = l.shadow_radius;
        scene.sun.shadow_center = rig.target;
        scene.ambient.sky *= l.ambient;
        scene.ambient.ground *= l.ambient;
        scene.clear_color = v3(Color::try_hex(&settings.sky).unwrap_or(Color::hex("#8fb8d8")));

        scene.style = rs::StyleSettings {
            flat_shadow: settings.flat_shadow,
            cel_bands: settings.cel_bands,
            rim: settings.rim,
            specular: settings.specular,
        };
        scene.post.outlines = settings.outlines;
        scene.post.outline_px = settings.outline_px;
        scene.post.outline_darken = settings.outline_darken;
        scene.post.exposure = settings.exposure;
        scene.post.saturation = settings.saturation;
        scene.post.tonemap = match settings.tonemap {
            TonemapChoice::Clamp => Tonemap::Clamp,
            TonemapChoice::SoftKnee => Tonemap::SoftKnee,
            TonemapChoice::Aces => Tonemap::Aces,
        };
        let c = &settings.cutaway;
        scene.cutaway = rs::Cutaway {
            focus: focus + Vec3::Y * 0.9,
            height_cut: c.height_cut && curr.focus_is_player,
            cut_height: focus.y + c.cut_above,
            cut_radius: c.cut_radius,
            fade: c.fade && curr.focus_is_player,
            fade_radius: c.fade_radius,
        };

        // Static geometry (cached per chunk version).
        self.static_cache.retain(|k, _| curr.statics.chunks.contains_key(k));
        for (key, chunk) in &curr.statics.chunks {
            let fresh = self.static_cache.get(key).is_some_and(|(v, s, _)| *v == chunk.version && *s == settings.style);
            if !fresh {
                let mut list = Vec::with_capacity(chunk.blocks.len());
                for b in chunk.blocks.iter().filter(|b| b.alive) {
                    let size = b.max - b.min;
                    let (mesh, scale) = if b.has(block_flags::ROUNDED) {
                        (MeshKey::rounded_box(size * 0.5, (size.min_element() * 0.2).min(0.15)), Vec3::ONE)
                    } else {
                        (MeshKey::Cube, size)
                    };
                    list.push(MeshInstance {
                        mesh,
                        transform: Mat4::from_scale_rotation_translation(scale, Quat::IDENTITY, b.center()),
                        color: v3(b.color),
                        emissive: 0.0,
                        style: style_of(b.look, settings.style),
                        flags: 0,
                        group: 1,
                    });
                }
                self.static_cache.insert(*key, (chunk.version, settings.style, list));
            }
            scene.meshes.extend_from_slice(&self.static_cache[key].2);
        }

        // Dynamic objects.
        for o in interpolate(prev, curr, alpha) {
            emit_object(&mut scene, &o, settings.style);
        }
        scene
    }
}

/// Adds one object's instances to the scene.
pub fn emit_object(scene: &mut Scene, o: &RenderObject, ov: StyleOverride) {
    let v = &o.visual;
    let style = style_of(v.look, ov);
    let color = v3(v.color);
    let group = o.id.0 + 2;
    let mesh = |mesh, scale: Vec3| MeshInstance {
        mesh,
        transform: Mat4::from_scale_rotation_translation(scale, o.rot, o.pos),
        color,
        emissive: v.emissive,
        style,
        flags: 0,
        group,
    };
    match v.shape {
        Shape::Box { half } => scene.meshes.push(mesh(MeshKey::Cube, half * 2.0)),
        Shape::RoundedBox { half, radius } => scene.meshes.push(mesh(MeshKey::rounded_box(half, radius), Vec3::ONE)),
        Shape::Cylinder { half_height, radius } => {
            scene.meshes.push(mesh(MeshKey::Cylinder, Vec3::new(radius * 2.0, half_height * 2.0, radius * 2.0)))
        }
        Shape::Sphere { radius } => scene.sdfs.push(SdfInstance {
            a: o.pos,
            b: o.pos,
            ra: radius,
            rb: radius,
            color,
            emissive: v.emissive,
            style,
            flags: 0,
            group,
        }),
        Shape::Capsule { half_height, radius } => {
            let axis = o.rot * Vec3::Y * half_height;
            scene.sdfs.push(SdfInstance {
                a: o.pos - axis,
                b: o.pos + axis,
                ra: radius,
                rb: radius,
                color,
                emissive: v.emissive,
                style,
                flags: 0,
                group,
            })
        }
    }
    if v.emissive > 0.5 {
        scene.point_lights.push(rs::PointLight { position: o.pos, color: color * v.emissive, radius: 4.0 + v.emissive * 2.0 });
    }
}
