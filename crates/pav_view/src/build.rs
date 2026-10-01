//! Scene building: interpolates between two simulation frames and emits render instances.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};
use pav_core::frame::{PuppetFrame, SimEvent};
use pav_core::params::{ChoiceParam, ParamVisitor, Tunable, nested};
use pav_core::puppet::PuppetDef;
use pav_core::statics::Ladder;
use pav_core::statics::{ChunkKey, block_flags};
use pav_core::zones::{Label, LabelMode, Zone, ZoneKind};
use pav_core::{Color, Look, RenderFrame, RenderObject, Shape, choice_enum};
use pav_render::mesh::{MeshData, Vertex};
use pav_render::scene::{self as rs, MeshInstance, MeshKey, Scene, SdfInstance, Style, Tonemap};
use pav_render::text::{Anchor, Text3d};
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
    pub fog: bool,
    pub fog_start: f32,
    pub fog_end: f32,
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
            fog: true,
            fog_start: 48.0,
            fog_end: 80.0,
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
        v.bool("fog", &mut self.fog, "Distance fog (hides streaming edges)");
        v.float("fog_start", &mut self.fog_start, 5.0, 300.0, "Fog starts at this distance from the camera target (m)");
        v.float("fog_end", &mut self.fog_end, 10.0, 400.0, "Fog is complete at this distance (m)");
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

#[derive(Clone, Copy, Debug)]
enum EffectKind {
    Explosion { radius: f32 },
    Dust,
}

#[derive(Clone, Copy, Debug)]
struct Effect {
    kind: EffectKind,
    pos: Vec3,
    start: f64,
}

/// Cached render data for one static region.
struct RegionCache {
    version: u64,
    style: StyleOverride,
    meshes: Vec<MeshInstance>,
    sdfs: Vec<SdfInstance>,
    lights: Vec<rs::PointLight>,
    terrain: Option<(MeshKey, Arc<MeshData>)>,
}

/// Keeps per-region instance caches and short-lived visual effects between frames.
#[derive(Default)]
pub struct ViewBuilder {
    static_cache: HashMap<ChunkKey, RegionCache>,
    effects: Vec<Effect>,
    /// Wall-clock seconds, drives effects.
    pub now: f64,
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
                if let (Some(a), Some(b)) = (p.puppet, o.puppet) {
                    obj.puppet = Some(PuppetFrame {
                        state: a.state.lerp(&b.state, alpha),
                        feet_offset: a.feet_offset + (b.feet_offset - a.feet_offset) * alpha,
                    });
                }
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

    /// Turns simulation events into visual effects (explosions, dust).
    pub fn add_events(&mut self, events: &[SimEvent]) {
        for e in events {
            match e {
                SimEvent::Explosion { pos, radius } => {
                    self.effects.push(Effect { kind: EffectKind::Explosion { radius: *radius }, pos: *pos, start: self.now })
                }
                SimEvent::Land { pos, speed } if *speed > 6.0 => {
                    self.effects.push(Effect { kind: EffectKind::Dust, pos: *pos, start: self.now })
                }
                _ => {}
            }
        }
    }

    fn emit_effects(&mut self, scene: &mut Scene) {
        let now = self.now;
        self.effects.retain(|e| now - e.start < 1.0);
        for e in &self.effects {
            let t = (now - e.start) as f32;
            match e.kind {
                EffectKind::Explosion { radius } => {
                    if t < 0.35 {
                        let grow = (t / 0.08).min(1.0);
                        let fade = 1.0 - ((t - 0.08) / 0.27).clamp(0.0, 1.0);
                        let r = radius * 0.85 * grow * fade.sqrt();
                        if r > 0.01 {
                            let mut s = SdfInstance::sphere(e.pos, r, Vec3::new(1.0, 0.62, 0.2));
                            s.style = Style::Unlit;
                            s.emissive = 2.0 * fade;
                            s.flags = rs::flags::NO_SHADOW | rs::flags::NO_CUT;
                            s.group = 0xfff0;
                            scene.sdfs.push(s);
                        }
                        scene.point_lights.push(rs::PointLight {
                            position: e.pos + Vec3::Y * 0.5,
                            color: Vec3::new(1.0, 0.6, 0.25) * 6.0 * fade,
                            radius: radius * 5.0,
                        });
                    }
                    // Smoke puffs drifting up.
                    if t < 0.9 {
                        for k in 0..6 {
                            let a = k as f32 * 1.047 + e.pos.x;
                            let dir = Vec3::new(a.cos(), 0.6, a.sin());
                            let p = e.pos + dir * (radius * 0.9 * (t * 3.0).min(1.0)) + Vec3::Y * t * 1.2;
                            let r = radius * 0.35 * (1.0 - t / 0.9);
                            let mut s = SdfInstance::sphere(p, r.max(0.0), Vec3::splat(0.35));
                            s.flags = rs::flags::NO_SHADOW;
                            s.group = 0xfff1;
                            scene.sdfs.push(s);
                        }
                    }
                }
                EffectKind::Dust => {
                    if t < 0.3 {
                        for k in 0..5 {
                            let a = k as f32 * 1.2566;
                            let p = e.pos + Vec3::new(a.cos(), 0.1, a.sin()) * (0.25 + t * 2.0);
                            let mut s = SdfInstance::sphere(p, 0.12 * (1.0 - t / 0.3), Vec3::splat(0.8));
                            s.flags = rs::flags::NO_SHADOW;
                            s.group = 0xfff2;
                            scene.sdfs.push(s);
                        }
                    }
                }
            }
        }
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
        let feet = curr
            .player
            .and_then(|id| curr.objects.iter().find(|o| o.id == id))
            .and_then(|o| o.puppet.map(|p| focus - Vec3::Y * p.feet_offset))
            .unwrap_or(focus);
        scene.cutaway = rs::Cutaway {
            focus: feet + Vec3::Y * 0.9,
            height_cut: c.height_cut && curr.focus_is_player,
            cut_height: feet.y + c.cut_above,
            cut_radius: c.cut_radius,
            fade: c.fade && curr.focus_is_player,
            fade_radius: c.fade_radius,
        };

        // Static geometry (cached per region version).
        self.static_cache.retain(|k, _| curr.statics.chunks.contains_key(k));
        for (key, chunk) in &curr.statics.chunks {
            let fresh = self.static_cache.get(key).is_some_and(|c| c.version == chunk.version && c.style == settings.style);
            if !fresh {
                let mut sdfs = Vec::new();
                let mut lights = Vec::new();
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
                for l in &chunk.ladders {
                    emit_ladder(&mut list, l, style_of(Look::Cel, settings.style));
                }
                for d in &chunk.decor {
                    let style = style_of(d.look, settings.style);
                    emit_shape(&mut list, &mut sdfs, &mut lights, &d.shape, d.pos, d.rot, v3(d.color), d.emissive, style, 1, 0);
                }
                for z in &chunk.zones {
                    emit_zone(&mut list, z, settings.style);
                }
                let terrain = chunk.terrain.as_ref().map(|t| {
                    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
                    for x in [t.cx as i64 as u64, t.cz as i64 as u64, chunk.version] {
                        h = (h ^ x).wrapping_mul(0x0100_0000_01b3);
                    }
                    (MeshKey::Custom(h), Arc::new(terrain_mesh(t)))
                });
                self.static_cache.insert(
                    *key,
                    RegionCache { version: chunk.version, style: settings.style, meshes: list, sdfs, lights, terrain },
                );
            }
            for l in &chunk.labels {
                emit_label(&mut scene.texts, l, rig);
            }
            for z in chunk.zones.iter().filter(|z| !z.label.is_empty() || (z.kind == ZoneKind::Start && z.color.is_some())) {
                let text = if !z.label.is_empty() { z.label.clone() } else { "START".into() };
                let (w, d) = (z.max.x - z.min.x, z.max.z - z.min.z);
                let label = Label {
                    text,
                    pos: z.floor_center() + Vec3::Y * 0.035,
                    size: (w.min(d) * 0.32).clamp(0.25, 0.7),
                    color: if z.kind == ZoneKind::Finish { Color::hex("#1d1f24") } else { Color::hex("#f6f3ea") },
                    mode: LabelMode::Floor,
                    facing: Default::default(),
                };
                emit_label(&mut scene.texts, &label, rig);
            }
            let c = &self.static_cache[key];
            scene.meshes.extend_from_slice(&c.meshes);
            scene.sdfs.extend_from_slice(&c.sdfs);
            scene.point_lights.extend_from_slice(&c.lights);
            if let Some((mk, data)) = &c.terrain {
                scene.custom_meshes.push((*mk, data.clone()));
                scene.meshes.push(MeshInstance {
                    mesh: *mk,
                    transform: Mat4::IDENTITY,
                    color: Vec3::ONE,
                    emissive: 0.0,
                    style: style_of(Look::Cel, settings.style),
                    flags: rs::flags::CUT_VERTEX,
                    group: 1,
                });
            }
        }
        scene.fog = rs::Fog {
            enabled: settings.fog,
            center: rig.target,
            start: settings.fog_start,
            end: settings.fog_end,
            color: scene.clear_color,
        };

        // Dynamic objects.
        let cam_fwd = scene.camera.forward;
        let now = self.now as f32;
        for o in interpolate(prev, curr, alpha) {
            match o.puppet {
                Some(p) => {
                    let player = curr.player == Some(o.id);
                    emit_puppet(&mut scene, &curr.puppet_def, &o, &p, cam_fwd, settings.style, player)
                }
                None => emit_object(&mut scene, &o, settings.style, now),
            }
        }
        // Projectiles: drawn where they are between ticks (they move in straight lines).
        let back = (1.0 - alpha) * curr.dt;
        for p in &curr.projectiles {
            let mut sd = SdfInstance::sphere(p.pos - p.vel * back, p.radius, v3(p.color));
            sd.style = Style::Unlit;
            sd.emissive = 0.6;
            sd.flags = rs::flags::NO_SHADOW | rs::flags::NO_CUT;
            sd.group = 0x7f1;
            scene.sdfs.push(sd);
        }
        self.emit_effects(&mut scene);
        scene
    }
}

/// Floor markings, water surfaces and checkpoint flags for a zone.
fn emit_zone(list: &mut Vec<MeshInstance>, z: &Zone, ov: StyleOverride) {
    let Some(color) = z.color else { return };
    let size = z.max - z.min;
    let mut push = |center: Vec3, scale: Vec3, color: Vec3, style: Style, flags: u32| {
        list.push(MeshInstance {
            mesh: MeshKey::Cube,
            transform: Mat4::from_scale_rotation_translation(scale, Quat::IDENTITY, center),
            color,
            emissive: 0.0,
            style,
            flags,
            group: 1,
        })
    };
    let floor = Vec3::new((z.min.x + z.max.x) * 0.5, z.min.y + 0.012, (z.min.z + z.max.z) * 0.5);
    let decal = Vec3::new(size.x - 0.08, 0.02, size.z - 0.08);
    match z.kind {
        ZoneKind::Water => {
            let top = Vec3::new(floor.x, z.max.y - 0.04, floor.z);
            push(top, Vec3::new(size.x, 0.06, size.z), v3(color), style_of(Look::Flat, ov), rs::flags::NO_SHADOW);
        }
        ZoneKind::Finish => {
            // Checkerboard.
            let n = ((size.x / 0.5).round() as i32).max(1);
            let m = ((size.z / 0.5).round() as i32).max(1);
            let (cw, cd) = (size.x / n as f32, size.z / m as f32);
            for i in 0..n {
                for j in 0..m {
                    let c = if (i + j) % 2 == 0 { v3(color) } else { Vec3::splat(0.02) };
                    let p = Vec3::new(z.min.x + (i as f32 + 0.5) * cw, floor.y, z.min.z + (j as f32 + 0.5) * cd);
                    push(p, Vec3::new(cw, 0.02, cd), c, Style::Flat, rs::flags::NO_SHADOW);
                }
            }
        }
        ZoneKind::Checkpoint => {
            push(floor, decal, v3(color), style_of(Look::Flat, ov), rs::flags::NO_SHADOW);
            // A little flag on a pole at the zone's corner.
            let base = Vec3::new(z.min.x + 0.15, z.min.y, z.min.z + 0.15);
            push(base + Vec3::Y * 0.8, Vec3::new(0.05, 1.6, 0.05), Vec3::splat(0.2), Style::Cel, 0);
            push(base + Vec3::new(0.2, 1.42, 0.0), Vec3::new(0.38, 0.26, 0.03), v3(color), Style::Cel, 0);
        }
        _ => push(floor, decal, v3(color), style_of(Look::Flat, ov), rs::flags::NO_SHADOW),
    }
}

/// A label as renderer text, oriented for the current camera.
fn emit_label(out: &mut Vec<Text3d>, l: &Label, rig: &CameraRig) {
    let (fwd, right) = rig.ground_axes();
    let (r, u) = match l.mode {
        LabelMode::Floor => (right, fwd),
        LabelMode::Wall => {
            let n = l.facing.dir();
            (Vec3::Y.cross(n).normalize_or(Vec3::X), Vec3::Y)
        }
        LabelMode::Billboard => {
            let f = rig.forward();
            let r = f.cross(Vec3::Y).normalize_or(right);
            (r, r.cross(f).normalize_or(Vec3::Y))
        }
    };
    out.push(Text3d {
        text: l.text.clone(),
        origin: l.pos,
        right: r,
        up: u,
        size: l.size,
        color: v3(l.color),
        anchor: Anchor::Center,
        flags: 0,
        weight: 0.04,
    });
}

/// Ladder: two rails and rungs against the wall.
fn emit_ladder(list: &mut Vec<MeshInstance>, l: &Ladder, style: Style) {
    let f = l.facing.dir();
    let lat = Vec3::new(-f.z, 0.0, f.x);
    let c = l.center();
    let depth = (l.max - l.min).dot(f.abs());
    let width = (l.max - l.min).dot(lat.abs());
    let wall_side = c + f * (depth * 0.5 - 0.07);
    let color = v3(l.color);
    let h = l.max.y - l.min.y + 0.5;
    let mut push = |center: Vec3, size: Vec3| {
        list.push(MeshInstance {
            mesh: MeshKey::Cube,
            transform: Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, center),
            color,
            emissive: 0.0,
            style,
            flags: 0,
            group: 3,
        })
    };
    let rail = |s: f32| wall_side + lat * s * (width * 0.5 - 0.04);
    for s in [-1.0f32, 1.0] {
        let p = rail(s);
        let size = lat.abs() * 0.07 + f.abs() * 0.07 + Vec3::Y * h;
        push(Vec3::new(p.x, l.min.y + h * 0.5, p.z), size);
    }
    let mut y = l.min.y + 0.3;
    while y < l.max.y + 0.3 {
        let size = lat.abs() * (width - 0.08) + f.abs() * 0.05 + Vec3::Y * 0.05;
        push(Vec3::new(wall_side.x, y, wall_side.z), size);
        y += 0.3;
    }
}

fn emit_puppet(
    scene: &mut Scene,
    def: &PuppetDef,
    o: &RenderObject,
    p: &PuppetFrame,
    cam_fwd: Vec3,
    ov: StyleOverride,
    player: bool,
) {
    let feet = o.pos - Vec3::Y * p.feet_offset;
    let style = style_of(def.look, ov);
    let flags = if player { rs::flags::NO_CUT } else { 0 };
    for part in pav_core::puppet::pose(def, &p.state, feet, cam_fwd) {
        scene.sdfs.push(SdfInstance {
            a: part.a,
            b: part.b,
            ra: part.ra,
            rb: part.rb,
            color: v3(part.color),
            emissive: 0.0,
            style,
            flags,
            group: o.id.0 + 2,
        });
    }
}

/// Adds one object's instances to the scene.
pub fn emit_object(scene: &mut Scene, o: &RenderObject, ov: StyleOverride, now: f32) {
    let v = &o.visual;
    let style = style_of(v.look, ov);
    let mut color = v3(v.color);
    let group = o.id.0 + 2;
    let mut v = v.clone();
    if o.pulse >= 0.0 {
        // Bomb fuse: blink faster as it runs out.
        let rate = 4.0 + 14.0 / (o.pulse + 0.25);
        if (now * rate).sin() > 0.0 {
            color = Vec3::new(1.0, 0.25, 0.15);
            v.emissive = 1.2;
            scene.point_lights.push(rs::PointLight { position: o.pos, color: Vec3::new(1.0, 0.2, 0.1) * 1.5, radius: 2.5 });
        }
    }
    let v = &v;
    let mut lights = Vec::new();
    emit_shape(&mut scene.meshes, &mut scene.sdfs, &mut lights, &v.shape, o.pos, o.rot, color, v.emissive, style, group, 0);
    scene.point_lights.extend(lights);
}

/// Instances for one shape (meshes for boxes/cylinders, SDF impostors for spheres/capsules).
#[allow(clippy::too_many_arguments)]
pub fn emit_shape(
    meshes: &mut Vec<MeshInstance>,
    sdfs: &mut Vec<SdfInstance>,
    lights: &mut Vec<rs::PointLight>,
    shape: &Shape,
    pos: Vec3,
    rot: Quat,
    color: Vec3,
    emissive: f32,
    style: Style,
    group: u32,
    flags: u32,
) {
    let mesh = |mesh, scale: Vec3| MeshInstance {
        mesh,
        transform: Mat4::from_scale_rotation_translation(scale, rot, pos),
        color,
        emissive,
        style,
        flags,
        group,
    };
    match *shape {
        Shape::Box { half } => meshes.push(mesh(MeshKey::Cube, half * 2.0)),
        Shape::RoundedBox { half, radius } => meshes.push(mesh(MeshKey::rounded_box(half, radius), Vec3::ONE)),
        Shape::Cylinder { half_height, radius } => {
            meshes.push(mesh(MeshKey::Cylinder, Vec3::new(radius * 2.0, half_height * 2.0, radius * 2.0)))
        }
        Shape::Sphere { radius } => {
            sdfs.push(SdfInstance { a: pos, b: pos, ra: radius, rb: radius, color, emissive, style, flags, group })
        }
        Shape::Capsule { half_height, radius } => {
            let axis = rot * Vec3::Y * half_height;
            sdfs.push(SdfInstance { a: pos - axis, b: pos + axis, ra: radius, rb: radius, color, emissive, style, flags, group })
        }
    }
    if emissive > 0.5 {
        lights.push(rs::PointLight { position: pos, color: color * emissive, radius: 4.0 + emissive * 2.0 });
    }
}

/// Render mesh for a terrain patch (positions are in world space).
fn terrain_mesh(t: &pav_core::terrain::TerrainPatch) -> MeshData {
    MeshData {
        vertices: t
            .positions
            .iter()
            .zip(&t.normals)
            .zip(&t.colors)
            .map(|((p, n), c)| Vertex { pos: *p, normal: *n, uv: [0.0, 0.0], color: [c[0], c[1], c[2], 1.0] })
            .collect(),
        indices: t.indices.clone(),
    }
}
