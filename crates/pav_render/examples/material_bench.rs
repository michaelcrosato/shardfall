//! Frame time of procedural materials (asset plan decision D8: typical ≤ +25%, every surface
//! with three layers ≤ +60%, over the same scene with every material off).
//!
//! A Shardfall-like room at the game camera (tilt 56°, 15.5 m, fov 40°): world-mapped
//! flagstone floor and brick walls, crates, barrels, a stall, iron posts with gold knobs,
//! mossy pillars and a cobble patch. Rendered headless (lavapipe here) at 960×540; each case's
//! best median over rounds.
//! `cargo run -p pav_render --example material_bench [-- WIDTH HEIGHT ROUNDS FRAMES OUT_DIR]`
use std::sync::Arc;
use std::time::Instant;

use glam::{Mat4, Quat, Vec3};
use pav_core::Look;
use pav_core::material::Library;
use pav_core::props::doc::MapMode;
use pav_render::*;

/// Materials packed into a table: the rows and what travels in the instance, by (name, map).
struct Table {
    rows: Vec<GpuMaterial>,
    slots: Vec<(String, MapMode, u32, Vec3, Style)>,
}

impl Table {
    fn slot(&mut self, name: &str, map: MapMode) -> (u32, Vec3, Style) {
        let full = format!("MAT/{name}");
        if let Some((_, _, slot, base, style)) = self.slots.iter().find(|(n, m, ..)| *n == full && *m == map) {
            return (*slot, *base, *style);
        }
        let material = Library::core().get(&full).unwrap_or_else(|| panic!("no {full}"));
        let packed = material.pack(map, &[]);
        let style = match packed.look {
            Look::Flat | Look::Cutout => Style::Flat,
            Look::Cel => Style::Cel,
            Look::Lit => Style::Lit,
            Look::Unlit => Style::Unlit,
        };
        let slot = self.rows.len() as u32;
        self.rows.push(GpuMaterial::from_words(&packed.gpu.words()));
        self.slots.push((full, map, slot, packed.base, style));
        (slot, packed.base, style)
    }
}

/// Builds the room; `on` = materials drawn, `every` = this material on every surface.
struct Room<'a> {
    scene: Scene,
    table: &'a mut Table,
    on: bool,
    every: Option<&'a str>,
    group: u32,
}

impl Room<'_> {
    fn add(&mut self, name: &str, map: MapMode, mesh: MeshKey, transform: Mat4, seed: u32) {
        let (slot, base, style) = self.table.slot(self.every.unwrap_or(name), map);
        self.group += 1;
        self.scene.meshes.push(MeshInstance {
            mesh,
            transform,
            color: base,
            emissive: 0.0,
            style,
            flags: 0,
            group: if map == MapMode::World { 1 } else { self.group },
            material: if self.on { material_param(slot, if map == MapMode::World { 0 } else { seed }) } else { 0 },
            ext: Vec3::ZERO,
        });
    }

    fn cube(&mut self, name: &str, map: MapMode, center: Vec3, size: Vec3, rot: Quat, seed: u32) {
        self.add(name, map, MeshKey::Cube, Mat4::from_scale_rotation_translation(size, rot, center), seed);
    }

    fn cylinder(&mut self, name: &str, center: Vec3, r: f32, h: f32, seed: u32) {
        let t = Mat4::from_scale_rotation_translation(Vec3::new(r * 2.0, h, r * 2.0), Quat::IDENTITY, center);
        self.add(name, MapMode::Axis, MeshKey::Cylinder, t, seed);
    }

    fn sphere(&mut self, name: &str, center: Vec3, r: f32) {
        let (slot, base, style) = self.table.slot(self.every.unwrap_or(name), MapMode::Object);
        self.group += 1;
        let mut s = SdfInstance::sphere(center, r, base);
        s.style = style;
        s.group = self.group;
        s.material = if self.on { slot } else { 0 };
        self.scene.sdfs.push(s);
    }
}

fn game_camera(aspect: f32) -> CameraData {
    let tilt = 56f32.to_radians();
    let focus = Vec3::Y * 0.8;
    let eye = focus + Vec3::new(0.0, tilt.sin(), tilt.cos()) * 15.5;
    let forward = (focus - eye).normalize();
    CameraData {
        view: glam::camera::rh::view::look_to_mat4(eye, forward, Vec3::Y),
        proj: glam::camera::rh::proj::directx::perspective(40f32.to_radians(), aspect, 0.1, 300.0),
        eye,
        forward,
        ortho: false,
    }
}

fn room(table: &mut Table, aspect: f32, on: bool, every: Option<&str>) -> Scene {
    use MapMode::{Object, World};
    let mut r = Room { scene: Scene { camera: game_camera(aspect), ..Default::default() }, table, on, every, group: 10 };
    r.scene.sun.shadow_radius = 22.0;
    r.scene.point_lights.push(PointLight {
        position: Vec3::new(-4.0, 2.2, -3.0),
        color: Vec3::new(1.0, 0.65, 0.35) * 1.6,
        radius: 8.0,
        shadows: false,
    });
    let id = Quat::IDENTITY;
    // Floor: 4 m tiles, as the game lays them.
    for i in -3..3 {
        for j in -3..2 {
            let c = Vec3::new(i as f32 * 4.0 + 2.0, -0.25, j as f32 * 4.0 + 2.0);
            r.cube("flagstones", World, c, Vec3::new(4.0, 0.5, 4.0), id, 0);
        }
    }
    // Walls (north and west), 1.9 m high, 1 m thick.
    for i in -3..3 {
        r.cube("brick_wall", World, Vec3::new(i as f32 * 4.0 + 2.0, 0.95, -12.5), Vec3::new(4.0, 1.9, 1.0), id, 0);
    }
    for j in -3..2 {
        r.cube("brick_wall", World, Vec3::new(-12.5, 0.95, j as f32 * 4.0 + 2.0), Vec3::new(1.0, 1.9, 4.0), id, 0);
    }
    for k in 0..4u32 {
        let p = Vec3::new(-6.0 + k as f32 * 1.3, 0.45, -8.0);
        r.cube("oak_planks", Object, p, Vec3::splat(0.9), Quat::from_rotation_y(0.2 * k as f32), k);
    }
    for k in 0..3u32 {
        r.cylinder("barrel_staves", Vec3::new(2.0 + k as f32 * 1.1, 0.55, -8.5), 0.4, 1.1, k);
    }
    let awning = Quat::from_rotation_x(0.25);
    r.cube("awning_stripes", Object, Vec3::new(5.0, 2.3, -3.0), Vec3::new(3.0, 0.08, 1.8), awning, 0);
    r.cube("oak_planks", Object, Vec3::new(5.0, 0.5, -3.0), Vec3::new(2.8, 1.0, 1.0), id, 1);
    let roof = Quat::from_rotation_x(0.45);
    r.cube("roof_shingles", Object, Vec3::new(-5.0, 2.6, -3.0), Vec3::new(3.4, 0.12, 2.2), roof, 0);
    r.cube("brick_wall", Object, Vec3::new(-5.0, 1.1, -3.6), Vec3::new(3.0, 2.2, 0.4), id, 0);
    for k in 0..3u32 {
        r.cylinder("wrought_iron", Vec3::new(-1.0 + k as f32 * 0.9, 0.6, -1.0), 0.18, 1.2, k);
        r.sphere("gold_trim", Vec3::new(-1.0 + k as f32 * 0.9, 1.4, -1.0), 0.22);
    }
    for k in 0..2u32 {
        r.cube("mossy_stone", Object, Vec3::new(-8.0 + k as f32 * 16.0, 1.2, 2.0), Vec3::new(0.9, 2.4, 0.9), id, k);
    }
    r.cube("cobblestone", Object, Vec3::new(2.0, 0.02, 3.0), Vec3::new(4.0, 0.04, 3.0), id, 0);
    let mut scene = r.scene;
    scene.materials = Arc::new(table.rows.clone());
    scene.materials_version = table.rows.len() as u64;
    scene
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize, default: u32| args.get(i).and_then(|a| a.parse().ok()).unwrap_or(default);
    let (w, h, rounds, frames) = (arg(1, 960), arg(2, 540), arg(3, 3) as usize, arg(4, 5) as usize);
    let out = args.get(5).map(std::path::PathBuf::from);
    let gpu = gpu::Headless::new()?;
    println!("adapter: {}", gpu::describe(&gpu.adapter.get_info()));
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let mut table = Table { rows: vec![GpuMaterial::default()], slots: Vec::new() };
    let aspect = w as f32 / h as f32;
    let cases: [(&str, bool, Option<&str>); 3] = [
        ("slot 0 (materials off)", false, None),
        ("typical", true, None),
        ("all mossy_stone (3 layers)", true, Some("mossy_stone")),
    ];
    // Build every scene first, so the table (and its version) is final.
    let scenes: Vec<Scene> = cases.iter().map(|(_, on, every)| room(&mut table, aspect, *on, *every)).collect();
    let scenes: Vec<Scene> = scenes
        .into_iter()
        .map(|mut s| {
            s.materials = Arc::new(table.rows.clone());
            s.materials_version = table.rows.len() as u64;
            s
        })
        .collect();
    let mut best = vec![f64::MAX; cases.len()];
    for round in 0..rounds {
        for (i, scene) in scenes.iter().enumerate() {
            let mut px = capture::render_to_rgba(&mut renderer, scene, w, h)?;
            let mut times = Vec::with_capacity(frames);
            for _ in 0..frames {
                let t = Instant::now();
                px = capture::render_to_rgba(&mut renderer, scene, w, h)?;
                times.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(f64::total_cmp);
            best[i] = best[i].min(times[times.len() / 2]);
            if let (0, Some(dir)) = (round, &out) {
                capture::save_png(&dir.join(format!("material_bench_{i}.png")), w, h, &px)?;
            }
        }
    }
    for (i, (label, ..)) in cases.iter().enumerate() {
        println!("{w}x{h} {label:<28} {:7.1} ms  {:+6.1}%", best[i], 100.0 * (best[i] / best[0] - 1.0));
    }
    Ok(())
}
