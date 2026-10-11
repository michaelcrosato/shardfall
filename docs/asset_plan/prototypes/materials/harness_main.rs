//! Material prototype harness: packs JSON materials into the GPU table, measures cost,
//! renders contact sheets and checks CPU/GPU parity on a plane.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use glam::{Mat4, Quat, Vec3};
use pav_render::capture;
use pav_render::gpu::Headless;
use pav_render::mesh::MeshKey;
use pav_render::scene::*;
use pav_render::Renderer;
use serde_json::{Map, Value};

// ------------------------------------------------------------------ CPU reference (subset)

fn pcg(v: u32) -> u32 {
    let s = v.wrapping_mul(747796405).wrapping_add(2891336453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277803737);
    (w >> 22) ^ w
}
fn hash3i(x: i32, y: i32, z: i32, salt: u32) -> u32 {
    pcg((x as u32).wrapping_mul(0x8da6b343) ^ (y as u32).wrapping_mul(0xd8163841) ^ (z as u32).wrapping_mul(0xcb1ab31f) ^ salt)
}
fn rnd(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16777216.0)
}
fn mixf(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn vnoise(p: Vec3, salt: u32) -> f32 {
    let fl = p.floor();
    let (ix, iy, iz) = (fl.x as i32, fl.y as i32, fl.z as i32);
    let f = p - fl;
    let u = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let h = |dx: i32, dy: i32, dz: i32| rnd(hash3i(ix + dx, iy + dy, iz + dz, salt));
    let a = mixf(h(0, 0, 0), h(1, 0, 0), u.x);
    let b = mixf(h(0, 1, 0), h(1, 1, 0), u.x);
    let c = mixf(h(0, 0, 1), h(1, 0, 1), u.x);
    let d = mixf(h(0, 1, 1), h(1, 1, 1), u.x);
    mixf(mixf(a, b, u.y), mixf(c, d, u.y), u.z)
}
fn fbm(p: Vec3, octaves: u32, size: f32, fw: f32, salt: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut q, mut s) = (0.0, 0.5, 0.0, p, size);
    for o in 0..octaves {
        let k = smoothstep(1.0, 3.0, s / fw.max(1e-5));
        if k <= 0.0 {
            for _ in o..octaves {
                sum += amp * 0.5;
                norm += amp;
                amp *= 0.5;
            }
            break;
        }
        sum += amp * mixf(0.5, vnoise(q, salt + o * 7), k);
        norm += amp;
        amp *= 0.5;
        q *= 2.03;
        s /= 2.03;
    }
    sum / norm
}
fn magic(p: Vec3, depth: u32, distort: f32) -> f32 {
    let mut x = ((p.x + p.y + p.z) * 5.0).sin();
    let mut y = ((-p.x + p.y - p.z) * 5.0).cos();
    let mut z = -((-p.x - p.y + p.z) * 5.0).cos();
    if depth > 0 {
        x *= distort;
        y = -(x - y - z).cos() * distort;
    }
    if depth > 1 {
        z = (-x - y - z).sin() * distort;
        x = -(-x + y - z).cos() * distort;
    }
    if depth > 2 {
        y = -(-x + y + z).sin() * distort;
        z = (x + y + z).cos() * distort;
    }
    if depth > 3 {
        x = (-x - y + z).cos() * distort;
        y = (x - y + z).sin() * distort;
    }
    let _ = (y, z);
    if distort != 0.0 {
        x /= distort;
    }
    0.5 + 0.5 * x
}
fn pulse(d: f32, w: f32, fw: f32) -> f32 {
    let f = fw.max(1e-5);
    let lo = (d - 0.5 * f).max(-0.5 * w);
    let hi = (d + 0.5 * f).min(0.5 * w);
    ((hi - lo) / f).clamp(0.0, 1.0)
}
fn fade(feature: f32, fw: f32) -> f32 {
    smoothstep(1.0, 3.0, feature / fw.max(1e-5))
}

/// Quantile tables: threshold t such that P(v > t) = cover, for cover = 0, 0.05, .. 1.
fn quantiles(mut sample: impl FnMut(u32) -> f32) -> [f32; 21] {
    let mut v: Vec<f32> = (0..200_000u32).map(&mut sample).collect();
    v.sort_by(|a, b| a.total_cmp(b));
    let mut t = [0.0; 21];
    for (i, x) in t.iter_mut().enumerate() {
        let cover = i as f32 / 20.0;
        let q = ((1.0 - cover) * (v.len() - 1) as f32) as usize;
        *x = v[q];
    }
    t
}
fn sample_point(i: u32) -> Vec3 {
    let h = pcg(i.wrapping_mul(3) + 11);
    Vec3::new(rnd(h) * 97.0, rnd(pcg(h)) * 97.0, rnd(pcg(h ^ 0xabc)) * 97.0)
}
struct Tables {
    noise: Vec<[f32; 21]>, // per detail 1..=4
    magic: Vec<[f32; 21]>, // per depth 1..=4 (distort 1)
}
fn tables() -> Tables {
    let noise = (1..=4).map(|d| quantiles(|i| fbm(sample_point(i), d, 1.0, 0.0, 0))).collect();
    let magic = (1..=4).map(|d| quantiles(|i| magic(sample_point(i) * 0.3, d, 1.0))).collect();
    Tables { noise, magic }
}
fn lookup(t: &[f32; 21], cover: f32) -> f32 {
    let x = cover.clamp(0.0, 1.0) * 20.0;
    let i = (x.floor() as usize).min(19);
    mixf(t[i], t[i + 1], x - i as f32)
}

// ------------------------------------------------------------------ JSON -> GPU table

fn role(s: &str) -> &str {
    match s {
        "$floor" => "#4a3f3a",
        "$floor2" => "#433834",
        "$wall" => "#5a4a44",
        "$pillar" => "#6a5850",
        "$accent" => "#ff7a2a",
        "$light" => "#ff8a3a",
        other => other,
    }
}
fn hex_lin(s: &str) -> Vec3 {
    let s = role(s);
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0xff00ff);
    srgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
fn hex_u32(s: &str) -> u32 {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0xff00ff);
    // RGBA8 little-endian for unpack4x8unorm: r in the low byte.
    ((v >> 16) & 0xff) | (((v >> 8) & 0xff) << 8) | ((v & 0xff) << 16) | (0xff << 24)
}

struct Packed {
    base: Vec3,
    look: Style,
    gpu: GpuMaterial,
}

fn load(dir: &Path, name: &str) -> Map<String, Value> {
    let rel = name.trim_start_matches("MAT/");
    let path = dir.join(format!("{rel}.json"));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut m = v.as_object().unwrap().clone();
    if let Some(base) = m.get("base").and_then(Value::as_str).map(str::to_string) {
        let mut b = load(dir, &base);
        m.remove("base");
        for (k, v) in m {
            b.insert(k, v);
        }
        return b;
    }
    m
}

fn num(l: &Map<String, Value>, k: &str, d: f32) -> f32 {
    l.get(k).and_then(Value::as_f64).map(|x| x as f32).unwrap_or(d)
}
fn size2(l: &Map<String, Value>, d: [f32; 2]) -> [f32; 2] {
    match l.get("size") {
        Some(Value::Array(a)) => [a[0].as_f64().unwrap() as f32, a[1].as_f64().unwrap() as f32],
        Some(v) => {
            let s = v.as_f64().unwrap() as f32;
            [s, s]
        }
        None => d,
    }
}
fn axis_of(l: &Map<String, Value>, d: &str) -> (u32, Vec3) {
    match l.get("axis") {
        Some(Value::Array(a)) => {
            let v = Vec3::new(a[0].as_f64().unwrap() as f32, a[1].as_f64().unwrap() as f32, a[2].as_f64().unwrap() as f32);
            (3, v.normalize())
        }
        other => match other.and_then(Value::as_str).unwrap_or(d) {
            "x" => (0, Vec3::X),
            "y" => (1, Vec3::Y),
            _ => (2, Vec3::Z),
        },
    }
}

fn pack(dir: &Path, name: &str, t: &Tables) -> Packed {
    let m = load(dir, name);
    let base = hex_lin(m.get("color").and_then(Value::as_str).unwrap_or("#808080"));
    let look = match m.get("look").and_then(Value::as_str).unwrap_or("cel") {
        "flat" => Style::Flat,
        "lit" => Style::Lit,
        "unlit" => Style::Unlit,
        _ => Style::Cel,
    };
    let mut g = GpuMaterial::default();
    let rough = m.get("roughness").and_then(Value::as_f64).map(|x| x as f32);
    let metal = num(&m, "metallic", 0.0);
    g.pbr = [rough.unwrap_or(if metal > 0.0 { 0.5 } else { -1.0 }), metal, 1.0 / num(&m, "scale", 1.0), 0.0];
    g.head[1] = match m.get("map").and_then(Value::as_str).unwrap_or("object") {
        "world" => 1,
        "axis" => 2,
        _ => 0,
    };
    g.head[2] = m.get("turn").and_then(Value::as_bool).unwrap_or(false) as u32;
    let layers = m.get("layers").and_then(Value::as_array).cloned().unwrap_or_default();
    g.head[0] = layers.len().min(3) as u32;
    for (i, lv) in layers.iter().take(3).enumerate() {
        let l = lv.as_object().unwrap();
        let ty = l["type"].as_str().unwrap();
        let mut o = GpuLayer::default();
        let (mut kind, mut variant) = (0u32, 0u32);
        let mut axis_default = "x";
        let mut cellular_defaults = (0.0f32, 0.0f32, 0.0f32); // vary, cavity, bevel
        match ty {
            "noise" => {
                kind = 1;
                let detail = num(l, "detail", 3.0).clamp(1.0, 4.0);
                let cover = num(l, "cover", 0.5);
                o.a = [num(l, "size", 0.5), detail, num(l, "distort", 0.0), cover];
                o.b = [lookup(&t.noise[detail as usize - 1], cover), num(l, "soft", 0.3) * 0.08 + 0.002, 0.0, 0.0];
            }
            "voronoi" | "cobbles" => {
                kind = 2;
                let cob = ty == "cobbles";
                variant = match l.get("feature").and_then(Value::as_str).unwrap_or(if cob { "edges" } else { "spots" }) {
                    "spots" => 0,
                    "edges" => 1,
                    _ => 2,
                };
                let size = num(l, "size", if cob { 0.25 } else { 0.3 });
                let cover = num(l, "cover", 0.3);
                o.a = [size, num(l, "jitter", if cob { 0.8 } else { 1.0 }), num(l, "gap", if cob { 0.03 } else { 0.02 }), cover];
                o.b = [(cover / std::f32::consts::PI).sqrt(), 0.0, 0.0, 0.0];
                cellular_defaults = if cob { (0.2, 0.5, 0.3 * size) } else { (0.0, 0.0, 0.03) };
            }
            "brick" | "planks" | "tiles" => {
                kind = 3;
                let (sz, gap, off, cd) = match ty {
                    "brick" => ([0.4, 0.2], 0.03, 0.5, (0.15, 0.3)),
                    "planks" => ([1.2, 0.2], 0.015, 0.0, (0.2, 0.2)),
                    _ => ([0.5, 0.5], 0.02, 0.0, (0.1, 0.2)),
                };
                variant = (ty == "planks") as u32;
                let s = size2(l, sz);
                let gap = num(l, "gap", gap);
                o.a = [s[0], s[1], gap, num(l, "offset", off)];
                cellular_defaults = (cd.0, cd.1, gap.max(0.02));
            }
            "debug_y" => {
                kind = 12;
            }
            "debug_noise" => {
                kind = 11;
                o.a = [num(l, "size", 0.4), 0.0, 0.0, 0.0];
            }
            "checker" => {
                kind = 4;
                o.a = [num(l, "size", 0.5), 0.0, 0.0, 0.0];
            }
            "gradient" => {
                kind = 5;
                axis_default = "y";
                o.a = [num(l, "from", 0.0), num(l, "to", 1.0), 0.0, 0.0];
            }
            "wave" | "stripes" => {
                kind = 6;
                let st = ty == "stripes";
                variant = (l.get("rings").and_then(Value::as_bool) == Some(true)) as u32;
                o.a = [num(l, "size", if st { 0.3 } else { 0.2 }), num(l, "cover", 0.5), num(l, "distort", 0.0), 0.0];
                o.b = [num(l, "soft", if st { 0.0 } else { 0.5 }), 0.0, 0.0, 0.0];
            }
            "wood" => {
                kind = 7;
                axis_default = "y";
                o.a = [num(l, "size", 0.08), num(l, "cover", 0.35), num(l, "distort", 1.5), 0.0];
                o.b = [num(l, "soft", 0.4), 0.0, 0.0, 0.0];
            }
            "magic" => {
                kind = 8;
                let depth = num(l, "depth", 2.0).clamp(1.0, 4.0);
                let cover = num(l, "cover", 0.5);
                o.a = [num(l, "size", 0.5), depth, num(l, "distort", 1.0), cover];
                o.b = [lookup(&t.magic[depth as usize - 1], cover), num(l, "soft", 0.3) * 0.1 + 0.002, 0.0, 0.0];
            }
            "dots" => {
                kind = 9;
                o.a = [num(l, "size", 0.15), num(l, "radius", 0.04), num(l, "offset", 0.5), num(l, "jitter", 0.0)];
            }
            "shingles" => {
                kind = 10;
                variant = match l.get("shape").and_then(Value::as_str).unwrap_or("round") {
                    "round" => 0,
                    "square" => 1,
                    _ => 2,
                };
                let s = size2(l, [0.3, 0.2]);
                o.a = [s[0], s[1], num(l, "gap", 0.015), num(l, "offset", 0.5)];
                cellular_defaults = (0.15, 0.4, 0.05);
            }
            other => panic!("unknown pattern {other}"),
        }
        let (axis_code, dir) = axis_of(l, axis_default);
        let flip = l.get("down").and_then(Value::as_str).is_some_and(|d| d.starts_with('+'));
        let axis_code = if kind == 10 && flip { 4 } else { axis_code };
        o.dir = dir.extend(0.0).to_array();
        let facing = match l.get("facing").and_then(Value::as_str) {
            Some("up") => 1,
            Some("down") => 2,
            Some("side") => 3,
            _ => 0,
        };
        let maskby = match l.get("masked_by").and_then(Value::as_str) {
            Some("prev") => 1,
            Some("not_prev") => 2,
            _ => 0,
        };
        let cells: Vec<String> =
            l.get("cells").and_then(Value::as_array).map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_string)).collect()).unwrap_or_default();
        for (k, c) in cells.iter().take(4).enumerate() {
            o.cells[k] = hex_u32(c);
        }
        let strength = num(l, "strength", 1.0);
        let paint_mode = if let Some(c) = l.get("color").and_then(Value::as_str) {
            o.paint = hex_lin(c).extend(strength).to_array();
            0
        } else {
            let sh = num(l, "shade", 0.6);
            o.paint = [sh.powf(2.2), 0.0, 0.0, strength];
            1
        };
        o.c = [num(l, "vary", cellular_defaults.0), num(l, "cavity", cellular_defaults.1), num(l, "glow", 0.0), num(l, "bevel", cellular_defaults.2)];
        o.head = [
            kind | variant << 8 | axis_code << 12 | facing << 16 | maskby << 20 | paint_mode << 24 | (cells.len().min(4) as u32) << 28,
            pcg(i as u32 * 7919 + name.len() as u32),
            0,
            0,
        ];
        g.layers[i] = o;
    }
    Packed { base, look, gpu: g }
}

// ------------------------------------------------------------------ scenes

fn game_camera(aspect: f32, target: Vec3) -> CameraData {
    let tilt = 56f32.to_radians();
    let dist = 15.5;
    let focus = target + Vec3::Y * 0.8;
    let eye = focus + Vec3::new(0.0, tilt.sin(), tilt.cos()) * dist;
    let fwd = (focus - eye).normalize();
    CameraData {
        view: glam::camera::rh::view::look_to_mat4(eye, fwd, Vec3::Y),
        proj: glam::camera::rh::proj::directx::perspective(40f32.to_radians(), aspect, 0.1, 300.0),
        eye,
        forward: fwd,
        ortho: false,
    }
}
fn close_camera(aspect: f32, target: Vec3, dist: f32) -> CameraData {
    let tilt = 30f32.to_radians();
    let yaw = 0.0f32;
    let eye = target + Vec3::new(yaw.sin() * tilt.cos(), tilt.sin(), yaw.cos() * tilt.cos()) * dist;
    let fwd = (target - eye).normalize();
    CameraData {
        view: glam::camera::rh::view::look_to_mat4(eye, fwd, Vec3::Y),
        proj: glam::camera::rh::proj::directx::perspective(35f32.to_radians(), aspect, 0.1, 300.0),
        eye,
        forward: fwd,
        ortho: false,
    }
}

struct Lib {
    slots: HashMap<String, u32>,
    table: Vec<GpuMaterial>,
    info: Vec<(Vec3, Style)>,
}
impl Lib {
    fn new(dir: &Path, names: &[&str], t: &Tables) -> Lib {
        let mut lib = Lib { slots: HashMap::new(), table: vec![GpuMaterial::default()], info: vec![(Vec3::ONE, Style::Cel)] };
        for n in names {
            let p = pack(dir, n, t);
            lib.slots.insert(n.to_string(), lib.table.len() as u32);
            lib.table.push(p.gpu);
            lib.info.push((p.base, p.look));
        }
        lib
    }
    fn with_map(&mut self, name: &str, map: u32) -> u32 {
        let key = format!("{name}@{map}");
        if let Some(&s) = self.slots.get(&key) {
            return s;
        }
        let src = self.slots[name] as usize;
        let mut g = self.table[src];
        g.head[1] = map;
        let s = self.table.len() as u32;
        self.table.push(g);
        self.info.push(self.info[src]);
        self.slots.insert(key, s);
        s
    }
}

fn inst(mesh: MeshKey, t: Mat4, color: Vec3, style: Style, group: u32, material: u32) -> MeshInstance {
    MeshInstance { mesh, transform: t, color, emissive: 0.0, style, flags: 0, group, material }
}

fn add_mat_box(s: &mut Scene, lib: &mut Lib, name: &str, map: u32, center: Vec3, size: Vec3, rot: Quat, group: u32, seed: u32, on: bool) {
    let slot = lib.with_map(name, map);
    let (base, look) = lib.info[slot as usize];
    let m = if on { slot | (seed & 0xfffff) << 12 } else { 0 };
    s.meshes.push(inst(MeshKey::Cube, Mat4::from_scale_rotation_translation(size, rot, center), base, look, group, m));
}
fn add_mat_cyl(s: &mut Scene, lib: &mut Lib, name: &str, map: u32, center: Vec3, r: f32, h: f32, group: u32, seed: u32, on: bool) {
    let slot = lib.with_map(name, map);
    let (base, look) = lib.info[slot as usize];
    let m = if on { slot | (seed & 0xfffff) << 12 } else { 0 };
    s.meshes.push(inst(MeshKey::Cylinder, Mat4::from_scale_rotation_translation(Vec3::new(r * 2.0, h, r * 2.0), Quat::IDENTITY, center), base, look, group, m));
}
fn add_mat_sphere(s: &mut Scene, lib: &mut Lib, name: &str, map: u32, center: Vec3, r: f32, group: u32, on: bool) {
    let slot = lib.with_map(name, map);
    let (base, look) = lib.info[slot as usize];
    let mut sd = SdfInstance::sphere(center, r, base);
    sd.style = look;
    sd.group = group;
    sd.material = if on { slot } else { 0 };
    s.sdfs.push(sd);
}

fn lights(s: &mut Scene) {
    s.sun.shadow_radius = 22.0;
    s.point_lights.push(PointLight { position: Vec3::new(-4.0, 2.2, -3.0), color: Vec3::new(1.0, 0.65, 0.35) * 1.6, radius: 8.0, shadows: false });
}

/// A Shardfall-like room at the game camera: world-mapped floor and walls, props on top.
fn game_scene(aspect: f32, lib: &mut Lib, on: bool, floor: &str, wall: &str, worst: Option<&str>) -> Scene {
    let mut s = Scene { camera: game_camera(aspect, Vec3::ZERO), ..Default::default() };
    lights(&mut s);
    let w = |n: &str| worst.map(str::to_string).unwrap_or_else(|| n.to_string());
    // floor: 4 m tiles like the game
    for i in -3..3 {
        for j in -3..2 {
            let c = Vec3::new(i as f32 * 4.0 + 2.0, -0.25, j as f32 * 4.0 + 2.0);
            add_mat_box(&mut s, lib, &w(floor), 1, c, Vec3::new(4.0, 0.5, 4.0), Quat::IDENTITY, 1, 0, on);
        }
    }
    // walls (north and west), 1.9 m high, 1 m thick
    for i in -3..3 {
        add_mat_box(&mut s, lib, &w(wall), 1, Vec3::new(i as f32 * 4.0 + 2.0, 0.95, -12.5), Vec3::new(4.0, 1.9, 1.0), Quat::IDENTITY, 1, 0, on);
    }
    for j in -3..2 {
        add_mat_box(&mut s, lib, &w(wall), 1, Vec3::new(-12.5, 0.95, j as f32 * 4.0 + 2.0), Vec3::new(1.0, 1.9, 4.0), Quat::IDENTITY, 1, 0, on);
    }
    let mut g = 10;
    // crates
    for k in 0..4 {
        let p = Vec3::new(-6.0 + k as f32 * 1.3, 0.45, -8.0);
        add_mat_box(&mut s, lib, &w("oak_planks"), 0, p, Vec3::splat(0.9), Quat::from_rotation_y(0.2 * k as f32), g, k, on);
        g += 1;
    }
    // barrels (axis map)
    for k in 0..3 {
        add_mat_cyl(&mut s, lib, &w("barrel_staves"), 2, Vec3::new(2.0 + k as f32 * 1.1, 0.55, -8.5), 0.4, 1.1, g, k, on);
        g += 1;
    }
    // market stall: awning (stripes) + counter (planks) + roof (shingles)
    add_mat_box(&mut s, lib, &w("awning_stripes"), 0, Vec3::new(5.0, 2.3, -3.0), Vec3::new(3.0, 0.08, 1.8), Quat::from_rotation_x(0.25), g, 0, on);
    g += 1;
    add_mat_box(&mut s, lib, &w("oak_planks"), 0, Vec3::new(5.0, 0.5, -3.0), Vec3::new(2.8, 1.0, 1.0), Quat::IDENTITY, g, 1, on);
    g += 1;
    add_mat_box(&mut s, lib, &w("roof_shingles"), 0, Vec3::new(-5.0, 2.6, -3.0), Vec3::new(3.4, 0.12, 2.2), Quat::from_rotation_x(0.45), g, 0, on);
    g += 1;
    add_mat_box(&mut s, lib, &w("brick_wall"), 0, Vec3::new(-5.0, 1.1, -3.6), Vec3::new(3.0, 2.2, 0.4), Quat::IDENTITY, g, 0, on);
    g += 1;
    // iron and gold bits
    for k in 0..3 {
        add_mat_cyl(&mut s, lib, &w("wrought_iron"), 2, Vec3::new(-1.0 + k as f32 * 0.9, 0.6, -1.0), 0.18, 1.2, g, k, on);
        g += 1;
        add_mat_sphere(&mut s, lib, &w("gold_trim"), 0, Vec3::new(-1.0 + k as f32 * 0.9, 1.4, -1.0), 0.22, g, on);
        g += 1;
    }
    // mossy stone pillars and a cobble patch
    for k in 0..2 {
        add_mat_box(&mut s, lib, &w("mossy_stone"), 0, Vec3::new(-8.0 + k as f32 * 16.0, 1.2, 2.0), Vec3::new(0.9, 2.4, 0.9), Quat::IDENTITY, g, k, on);
        g += 1;
    }
    add_mat_box(&mut s, lib, &w("cobblestone"), 0, Vec3::new(2.0, 0.02, 3.0), Vec3::new(4.0, 0.04, 3.0), Quat::IDENTITY, g, 0, on);
    s
}

/// One material on a cube, a sphere and a floor patch, close up.
fn swatch_scene(aspect: f32, lib: &mut Lib, name: &str, dist: f32, game: bool) -> Scene {
    let target = Vec3::new(0.0, 0.5, 0.0);
    let cam = if game { game_camera(aspect, Vec3::new(0.0, -0.3, 0.0)) } else { close_camera(aspect, target, dist) };
    let mut s = Scene { camera: cam, ..Default::default() };
    s.sun.shadow_radius = 8.0;
    s.clear_color = srgb(0x2a, 0x2a, 0x30);
    add_mat_box(&mut s, lib, "_test/floor_grey", 1, Vec3::new(0.0, -0.05, 0.0), Vec3::new(30.0, 0.1, 30.0), Quat::IDENTITY, 1, 0, false);
    add_mat_box(&mut s, lib, name, 1, Vec3::new(2.4, 0.01, 0.0), Vec3::new(2.0, 0.02, 2.0), Quat::IDENTITY, 2, 0, true);
    add_mat_box(&mut s, lib, name, 0, Vec3::new(0.0, 0.5, 0.0), Vec3::splat(1.0), Quat::from_rotation_y(0.6), 3, 0, true);
    add_mat_sphere(&mut s, lib, name, 0, Vec3::new(-2.0, 0.55, 0.0), 0.55, 4, true);
    add_mat_cyl(&mut s, lib, name, 2, Vec3::new(1.2, 0.55, -1.6), 0.45, 1.1, 5, 0, true);
    s
}

fn timed(r: &mut Renderer, s: &Scene, w: u32, h: u32, n: usize) -> (f64, Vec<u8>) {
    let mut px = capture::render_to_rgba(r, s, w, h).unwrap();
    capture::render_to_rgba(r, s, w, h).unwrap();
    let mut v = Vec::new();
    for _ in 0..n {
        let t = Instant::now();
        px = capture::render_to_rgba(r, s, w, h).unwrap();
        v.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    v.sort_by(|a, b| a.total_cmp(b));
    (v[v.len() / 2], px)
}

fn stack(images: &[(u32, u32, Vec<u8>)]) -> (u32, u32, Vec<u8>) {
    let w = images[0].0;
    let h: u32 = images.iter().map(|i| i.1).sum();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for (_, _, px) in images {
        out.extend_from_slice(px);
    }
    (w, h, out)
}

fn hstack(images: &[(u32, u32, Vec<u8>)]) -> (u32, u32, Vec<u8>) {
    let h = images[0].1;
    let w: u32 = images.iter().map(|i| i.0).sum();
    let mut out = vec![0u8; (w * h * 4) as usize];
    let mut x0 = 0;
    for (iw, ih, px) in images {
        for y in 0..*ih {
            let src = &px[(y * iw * 4) as usize..((y + 1) * iw * 4) as usize];
            let dst = ((y * w + x0) * 4) as usize;
            out[dst..dst + src.len()].copy_from_slice(src);
        }
        x0 += iw;
    }
    (w, h, out)
}

// ------------------------------------------------------------------ parity

fn l2s(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// CPU evaluation of the parity material (noise layer then brick layer) at world point p on
/// a floor (normal +y), in world mapping with seed 0. Mirrors materials.wgsl.
fn cpu_parity(g: &GpuMaterial, base: Vec3, p: Vec3, fw: f32) -> Vec3 {
    let n = Vec3::Y;
    let p3 = p - n * 1e-3;
    let mut c = base;
    for li in 0..g.head[0] as usize {
        let l = &g.layers[li];
        let kind = l.head[0] & 0xff;
        let paint = (l.head[0] >> 24) & 0xf;
        let salt = l.head[1];
        let (m, mean, feat, cell, d, gap);
        match kind {
            1 => {
                let q = p3 / l.a[0];
                let v = fbm(q, l.a[1] as u32, l.a[0], fw, salt);
                m = smoothstep(l.b[0] - l.b[1], l.b[0] + l.b[1], v);
                mean = l.a[3];
                feat = l.a[0] * 0.5;
                cell = 0u32;
                d = 1e9;
                gap = 0.0;
            }
            3 => {
                let uv = glam::Vec2::new(p.x, p.z); // floor: plane y, axis x keeps (x, z)
                let (w, h, gp) = (l.a[0], l.a[1], l.a[2]);
                let row = (uv.y / h).floor();
                let off = l.a[3] * w * (row - 2.0 * (row * 0.5).floor());
                let x = (uv.x - off) / w;
                let col = x.floor();
                let fu = x - col;
                let fv = uv.y / h - row;
                let du = fu.min(1.0 - fu) * w;
                let dv = fv.min(1.0 - fv) * h;
                m = pulse(du, gp, fw).max(pulse(dv, gp, fw));
                mean = 1.0 - (1.0 - (gp / w).min(1.0)) * (1.0 - (gp / h).min(1.0));
                feat = w.min(h);
                cell = hash3i(col as i32, row as i32, 1, salt) | 1;
                d = du.min(dv);
                gap = gp;
            }
            12 => {
                m = (0.5 - p3.y * 100.0).clamp(0.0, 1.0);
                mean = 0.5;
                feat = 1e9;
                cell = 0;
                d = 1e9;
                gap = 0.0;
            }
            11 => {
                m = vnoise(p3 / l.a[0], salt);
                mean = 0.5;
                feat = 1e9;
                cell = 0;
                d = 1e9;
                gap = 0.0;
            }
            _ => unreachable!(),
        }
        let k = fade(feat, fw);
        let m = mixf(mean, m, k);
        if cell != 0 {
            let vary = l.c[0] * (2.0 * rnd(cell) - 1.0) * k;
            c *= (1.0 + vary) * (1.0 + vary);
            if l.c[1] > 0.0 {
                let bev = l.c[3].max(1e-4);
                let prof = 1.0 - smoothstep(gap * 0.5, gap * 0.5 + bev, d);
                let s = 1.0 - l.c[1] * mixf((bev / feat.max(1e-4)).min(1.0) * 0.5, prof, fade(bev, fw));
                c *= s * s;
            }
        }
        let col = if paint == 1 { c * l.paint[0] } else { Vec3::new(l.paint[0], l.paint[1], l.paint[2]) };
        c = c.lerp(col, m * l.paint[3]);
    }
    c
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = PathBuf::from(args.get(2).cloned().unwrap_or_else(|| ".".into()));
    let dir = PathBuf::from(args.get(3).cloned().unwrap_or_else(|| "../examples".into()));
    std::fs::create_dir_all(&out).unwrap();
    let t0 = Instant::now();
    let tab = tables();
    eprintln!("quantile tables {:.0} ms", t0.elapsed().as_secs_f64() * 1000.0);
    eprintln!("noise detail3 thresholds (cover 0..1 step .05): {:?}", tab.noise[2].map(|x| (x * 1000.0).round() / 1000.0));
    let hd = Headless::new().unwrap();
    let ti = Instant::now();
    let mut r = Renderer::new(&hd.device, &hd.queue);
    eprintln!("TIMING renderer init {:.0} ms", ti.elapsed().as_secs_f64() * 1000.0);
    let names = ["barrel_staves", "oak_planks", "cobblestone", "brick_wall", "wrought_iron", "awning_stripes", "roof_shingles", "mossy_stone", "gold_trim", "_test/floor_grey", "flagstones"];
    let mut lib = Lib::new(&dir, &names, &tab);
    match args.get(1).map(String::as_str) {
        Some("first") => {
            let mut s = game_scene(16.0 / 9.0, &mut lib, true, "flagstones", "brick_wall", None);
            s.materials = Arc::new(lib.table.clone());
            s.materials_version = 1;
            let t = Instant::now();
            capture::render_to_rgba(&mut r, &s, 480, 270).unwrap();
            eprintln!("TIMING first frame {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
            let t = Instant::now();
            capture::render_to_rgba(&mut r, &s, 480, 270).unwrap();
            eprintln!("TIMING second frame {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
        }
        Some("bench") => {
            for (w, h) in [(480u32, 270u32), (960, 540)] {
                let aspect = w as f32 / h as f32;
                let cases: [(&str, bool, Option<&str>); 6] = [
                    ("base (slot 0)", false, None),
                    ("materials", true, None),
                    ("all mossy_stone (3 layers)", true, Some("mossy_stone")),
                    ("all cobblestone (2 layers)", true, Some("cobblestone")),
                    ("all brick_wall (2 layers)", true, Some("brick_wall")),
                    ("all awning_stripes (1 layer)", true, Some("awning_stripes")),
                ];
                let mut best = vec![f64::MAX; cases.len()];
                for round in 0..3 {
                    for (ci, (label, on, worst)) in cases.iter().enumerate() {
                        let mut s = game_scene(aspect, &mut lib, *on, "flagstones", "brick_wall", *worst);
                        s.materials = Arc::new(lib.table.clone());
                        s.materials_version = lib.table.len() as u64;
                        let (med, px) = timed(&mut r, &s, w, h, 7);
                        best[ci] = best[ci].min(med);
                        if round == 0 {
                            let tag = label.split_whitespace().nth(if *on && worst.is_some() { 1 } else { 0 }).unwrap();
                            capture::save_png(&out.join(format!("game_{tag}_{w}.png")), w, h, &px).unwrap();
                        }
                    }
                }
                for (ci, (label, _, _)) in cases.iter().enumerate() {
                    println!("{w}x{h} {label:<30} best median {:7.1} ms  (+{:.0}%)", best[ci], 100.0 * (best[ci] / best[0] - 1.0));
                }
            }
        }
        Some("sheet") => {
            let (w, h) = (330u32, 200u32);
            let show = ["oak_planks", "cobblestone", "brick_wall", "wrought_iron", "awning_stripes", "roof_shingles", "mossy_stone", "gold_trim"];
            let mut rows = Vec::new();
            for n in show {
                let mut close = swatch_scene(w as f32 / h as f32, &mut lib, n, 6.5, false);
                close.materials = Arc::new(lib.table.clone());
                close.materials_version = lib.table.len() as u64;
                let a = capture::render_to_rgba(&mut r, &close, w, h).unwrap();
                let mut game = swatch_scene(w as f32 / h as f32, &mut lib, n, 6.5, true);
                game.materials = Arc::new(lib.table.clone());
                game.materials_version = lib.table.len() as u64;
                let b = capture::render_to_rgba(&mut r, &game, w, h).unwrap();
                rows.push(hstack(&[(w, h, a), (w, h, b)]));
            }
            let (sw, sh, px) = stack(&rows[..4]);
            capture::save_png(&out.join("sheet_a.png"), sw, sh, &px).unwrap();
            let (sw, sh, px) = stack(&rows[4..]);
            capture::save_png(&out.join("sheet_b.png"), sw, sh, &px).unwrap();
        }
        Some("parity") => {
            // Ortho camera straight down onto a plane at y = 0: pixel (i, j) <-> world (x, z).
            let (w, h) = (256u32, 256u32);
            let half = 2.0f32;
            let eye = Vec3::new(0.0, 10.0, 0.0);
            let fwd = Vec3::NEG_Y;
            let cam = CameraData {
                view: glam::camera::rh::view::look_to_mat4(eye, fwd, Vec3::NEG_Z),
                proj: glam::camera::rh::proj::directx::orthographic(-half, half, -half, half, 0.05, 50.0),
                eye,
                forward: fwd,
                ortho: true,
            };
            let name = args.get(4).map(String::as_str).unwrap_or("parity");
            let p = pack(&dir, name, &tab);
            let mut s = Scene { camera: cam, ..Default::default() };
            s.post.outlines = false;
            s.post.tonemap = Tonemap::Clamp;
            s.post.bloom = 0.0;
            s.post.gi = 0.0;
            s.post.haze = 0.0;
            s.post.halos = 0.0;
            s.post.shafts = 0.0;
            s.post.saturation = 1.0;
            s.sun.shadows = false;
            s.materials = Arc::new(vec![GpuMaterial::default(), p.gpu]);
            s.materials_version = 1;
            s.meshes.push(inst(MeshKey::Cube, Mat4::from_scale_rotation_translation(Vec3::new(8.0, 0.2, 8.0), Quat::IDENTITY, Vec3::new(0.0, -0.1, 0.0)), p.base, Style::Unlit, 1, 1));
            let px = capture::render_to_rgba(&mut r, &s, w, h).unwrap();
            capture::save_png(&out.join("parity_gpu.png"), w, h, &px).unwrap();
            let fw = 2.0 * half / h as f32;
            let mut cpu = vec![0u8; (w * h * 4) as usize];
            let (mut n_ok, mut n2, mut sum, mut worst) = (0usize, 0usize, 0f64, 0i32);
            for j in 0..h {
                for i in 0..w {
                    let x = -half + (i as f32 + 0.5) * fw;
                    let z = -half + (j as f32 + 0.5) * fw;
                    let c = cpu_parity(&p.gpu, p.base, Vec3::new(x, 0.0, z), fw);
                    let k = ((j * w + i) * 4) as usize;
                    for ch in 0..3 {
                        let v = (l2s(c[ch]) * 255.0).round() as i32;
                        cpu[k + ch] = v as u8;
                        let d = (v - px[k + ch] as i32).abs();
                        worst = worst.max(d);
                        sum += d as f64;
                    }
                    cpu[k + 3] = 255;
                    let d = (0..3).map(|ch| (cpu[k + ch] as i32 - px[k + ch] as i32).abs()).max().unwrap();
                    if d <= 1 {
                        n_ok += 1;
                    }
                    if d <= 3 {
                        n2 += 1;
                    }
                }
            }
            capture::save_png(&out.join("parity_cpu.png"), w, h, &cpu).unwrap();
            if name == "parity_raw" {
                let l = &p.gpu.layers[0];
                let mut worst = 0.0f32;
                let mut sum = 0.0f32;
                let mut cnt = 0;
                for j in (0..h).step_by(2) {
                    for i in (0..w).step_by(2) {
                        let k = ((j * w + i) * 4) as usize;
                        let s2l = |c: u8| { let c = c as f32 / 255.0; if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } };
                        let vg = s2l(px[k]);
                        let x = -half + (i as f32 + 0.5) * fw;
                        let z = -half + (j as f32 + 0.5) * fw;
                        let vc = vnoise(Vec3::new(x, -1e-3, z) / l.a[0], l.head[1]);
                        let d = (vg - vc).abs();
                        if d > worst { worst = d; println!("raw worst so far px ({i},{j}) world ({x:.3},{z:.3}) gpu {vg:.4} cpu {vc:.4}"); }
                        sum += d; cnt += 1;
                    }
                }
                println!("raw noise: mean |d| {:.4} worst {:.4}", sum / cnt as f32, worst);
                let mut q = [[0.0f32; 2]; 4];
                for j in 0..h {
                    for i in 0..w {
                        let k = ((j * w + i) * 4) as usize;
                        let s2l = |c: u8| { let c = c as f32 / 255.0; if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } };
                        let x = -half + (i as f32 + 0.5) * fw;
                        let z = -half + (j as f32 + 0.5) * fw;
                        let vc = vnoise(Vec3::new(x, -1e-3, z) / l.a[0], l.head[1]);
                        let qi = (x > 0.0) as usize + 2 * (z > 0.0) as usize;
                        q[qi][0] += (s2l(px[k]) - vc).abs();
                        q[qi][1] += 1.0;
                    }
                }
                for j in [0u32, 40, 80, 120] {
                    let mut line = String::new();
                    for i in (0..w).step_by(16) {
                        let k = ((j * w + i) * 4) as usize;
                        let s2l = |c: u8| { let c = c as f32 / 255.0; if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } };
                        let x = -half + (i as f32 + 0.5) * fw;
                        let z = -half + (j as f32 + 0.5) * fw;
                        let vc = vnoise(Vec3::new(x, -1e-3, z) / l.a[0], l.head[1]);
                        line += &format!("{:.2}/{:.2} ", s2l(px[k]), vc);
                    }
                    println!("row {j}: {line}");
                }
                for (qi, v) in q.iter().enumerate() {
                    println!("quadrant x>0:{} z>0:{} mean |d| {:.4}", qi & 1, qi >> 1, v[0] / v[1]);
                }
            }
            // Debug: which hypothesis about the GPU noise input matches best?
            {
                let l = &p.gpu.layers[0];
                let hyps: [(&str, Box<dyn Fn(f32, f32) -> Vec3>); 4] = [
                    ("p3 (y=-0.001)", Box::new(|x, z| Vec3::new(x, -1e-3, z))),
                    ("y=0", Box::new(|x, z| Vec3::new(x, 0.0, z))),
                    ("y=+0.001", Box::new(|x, z| Vec3::new(x, 1e-3, z))),
                    ("swap xz", Box::new(|x, z| Vec3::new(z, -1e-3, x))),
                ];
                for (label, f) in hyps.iter() {
                    let mut ok = 0;
                    for j in 0..h {
                        for i in 0..w {
                            let x = -half + (i as f32 + 0.5) * fw;
                            let z = -half + (j as f32 + 0.5) * fw;
                            let v = fbm(f(x, z) / l.a[0], l.a[1] as u32, l.a[0], fw, l.head[1]);
                            let m = smoothstep(l.b[0] - l.b[1], l.b[0] + l.b[1], v);
                            let k = ((j * w + i) * 4) as usize;
                            let gm = if px[k] < 100 { 1.0 } else if px[k] > 130 { 0.0 } else { m };
                            if (gm - m).abs() < 0.5 { ok += 1; }
                        }
                    }
                    println!("hyp {label}: {:.1}% agree", 100.0 * ok as f64 / (w * h) as f64);
                }
            }
            {
                let l = &p.gpu.layers[0];
                let s2l = |c: u8| { let c = c as f32 / 255.0; if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } };
                let (b0, g0) = (s2l(0x8a), s2l(0x3a));
                let mut shown = 0;
                for j in (0..h).step_by(3) {
                    for i in (0..w).step_by(3) {
                        let k = ((j * w + i) * 4) as usize;
                        let r = px[k];
                        if r > 62 && r < 134 && shown < 10 {
                            let mg = (b0 - s2l(r)) / (b0 - g0);
                            // invert smoothstep
                            let (mut lo, mut hi) = (0.0f32, 1.0f32);
                            for _ in 0..30 { let mid = 0.5 * (lo + hi); if mid * mid * (3.0 - 2.0 * mid) < mg { lo = mid } else { hi = mid } }
                            let vg = l.b[0] - l.b[1] + 2.0 * l.b[1] * lo;
                            let x = -half + (i as f32 + 0.5) * fw;
                            let z = -half + (j as f32 + 0.5) * fw;
                            let vc = fbm(Vec3::new(x, -1e-3, z) / l.a[0], l.a[1] as u32, l.a[0], fw, l.head[1]);
                            println!("trans px ({i},{j}): v_gpu {vg:.4} v_cpu {vc:.4} diff {:.4}", vg - vc);
                            shown += 1;
                        }
                    }
                }
            }
            // Debug: report CPU noise values at a few mismatching pixels.
            let mut shown = 0;
            for j in (0..h).step_by(7) {
                for i in (0..w).step_by(5) {
                    let k = ((j * w + i) * 4) as usize;
                    let d = (0..3).map(|ch| (cpu[k + ch] as i32 - px[k + ch] as i32).abs()).max().unwrap();
                    if d > 20 && shown < 12 {
                        shown += 1;
                        let x = -half + (i as f32 + 0.5) * fw;
                        let z = -half + (j as f32 + 0.5) * fw;
                        let l = &p.gpu.layers[0];
                        let q = (Vec3::new(x, 0.0, z) - Vec3::Y * 1e-3) / l.a[0];
                        let v = fbm(q, l.a[1] as u32, l.a[0], fw, l.head[1]);
                        println!("px ({i},{j}) world ({x:.3},{z:.3}) cpu v {v:.4} t {:.4} cpu rgb {:?} gpu rgb {:?}", l.b[0], &cpu[k..k + 3], &px[k..k + 3]);
                    }
                }
            }
            let total = (w * h) as f64;
            println!(
                "parity {}x{}: <=1/255 {:.2}%  <=3/255 {:.2}%  mean |d| {:.3}  worst {}",
                w,
                h,
                100.0 * n_ok as f64 / total,
                100.0 * n2 as f64 / total,
                sum / (total * 3.0),
                worst
            );
        }
        Some("one") => {
            let (w, h) = (480u32, 300u32);
            let mut tiles = Vec::new();
            for n in args[4].split(',') {
                if !lib.slots.contains_key(n) {
                    let p = pack(&dir, n, &tab);
                    lib.slots.insert(n.to_string(), lib.table.len() as u32);
                    lib.table.push(p.gpu);
                    lib.info.push((p.base, p.look));
                }
                let mut sc = swatch_scene(w as f32 / h as f32, &mut lib, n, 7.5, false);
                sc.camera = close_camera(w as f32 / h as f32, Vec3::new(0.0, 0.4, -0.6), 7.5);
                sc.materials = Arc::new(lib.table.clone());
                sc.materials_version = lib.table.len() as u64 + 1000;
                tiles.push((w, h, capture::render_to_rgba(&mut r, &sc, w, h).unwrap()));
            }
            let (sw, sh, px) = hstack(&tiles);
            capture::save_png(&out.join("one.png"), sw, sh, &px).unwrap();
        }
        _ => eprintln!("usage: harness bench|sheet|parity OUTDIR EXAMPLES_DIR"),
    }
}
