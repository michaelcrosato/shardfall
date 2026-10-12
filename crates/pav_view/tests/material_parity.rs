//! The GPU draws procedural materials exactly as the CPU reference evaluates them
//! (`pav_core::material::eval_packed`): every kernel, world-mapped on a floor and
//! object-mapped on a turned, seeded slab, through the view's material table and `emit_shape`.
//! At least 99.9% of pixels within 1/255 per kernel (asset plan §6.5).
//!
//! Top-down orthographic camera on a surface at y = 0, so pixel (i, j) is world
//! (x, z) = (-2 + (i + ½)·px, -2 + (j + ½)·px); unlit, with no outlines, bloom or tonemap
//! curve, so the picture is the evaluated albedo (times 1 + glow) in sRGB.
//! `MATERIAL_PARITY_OUT=dir` writes each GPU picture and its CPU twin there.
//! Needs a GPU adapter (lavapipe will do); without one it says so and passes.

use glam::{Quat, Vec3};
use pav_core::Shape;
use pav_core::material::{Library, Sample, eval_packed, footprint};
use pav_core::props::doc::MapMode;
use pav_render::scene::{CameraData, Scene, Style, Tonemap};
use pav_render::{Renderer, capture, gpu};
use pav_view::build::emit_shape;
use pav_view::materials::{MaterialTable, seed};

const SIZE: u32 = 256;
const HALF: f32 = 2.0;

/// One material per kernel (several layers cover its variants), in cm.
const KERNELS: &[(&str, &str)] = &[
    (
        "noise",
        r##"[{"type":"noise","size":40,"cover":0.5,"color":"#203040"},
        {"type":"noise","size":25,"detail":4,"distort":0.6,"cover":0.3,"shade":0.5}]"##,
    ),
    (
        "cells",
        r##"[{"type":"cobbles","size":30,"gap":3,"vary":0.25,"cavity":0.4,"cells":["#a08060","#806040","#5a6070"]},
        {"type":"voronoi","size":45,"cover":0.2,"color":"#c0b030"},
        {"type":"voronoi","size":60,"feature":"cells","cover":0.3,"shade":0.7}]"##,
    ),
    (
        "grid",
        r##"[{"type":"planks","size":[90,18],"gap":2,"vary":0.2,"cavity":0.3},
        {"type":"brick","size":[40,20],"gap":3,"color":"#d0c8b0","strength":0.5},
        {"type":"tiles","size":55,"gap":2.5,"vary":0.1,"cells":["#406080","#604080"]}]"##,
    ),
    ("checker", r##"[{"type":"checker","size":25,"color":"#304050"}]"##),
    (
        "gradient",
        r##"[{"type":"gradient","axis":"x","from":-150,"to":150,"color":"#f0e0a0"},
        {"type":"gradient","axis":[1,0,1],"from":0,"to":100,"shade":0.5,"strength":0.6}]"##,
    ),
    (
        "bands",
        r##"[{"type":"stripes","size":30,"cover":0.4,"axis":"x","color":"#e0e0f0"},
        {"type":"wave","size":25,"rings":true,"axis":"y","distort":0.5,"shade":0.6},
        {"type":"stripes","size":40,"axis":[1,0,1],"soft":0.3,"color":"#802020","strength":0.5}]"##,
    ),
    (
        "wood",
        r##"[{"type":"wood","size":10,"axis":"x","cover":0.35,"shade":0.7},
        {"type":"wood","size":20,"axis":"z","distort":2,"color":"#503010","strength":0.4}]"##,
    ),
    (
        "magic",
        r##"[{"type":"magic","size":60,"cover":0.5,"color":"#40a0a0"},
        {"type":"magic","size":90,"depth":4,"distort":1.5,"cover":0.3,"shade":0.6}]"##,
    ),
    (
        "dots",
        r##"[{"type":"dots","size":20,"radius":5,"color":"#f0f0f0","glow":0.5},
        {"type":"dots","size":35,"radius":8,"jitter":0.5,"offset":0,"shade":0.4}]"##,
    ),
    (
        "shingles",
        r##"[{"type":"shingles","size":[30,20],"gap":2,"shape":"round"},
        {"type":"shingles","size":[45,30],"gap":2,"shape":"point","color":"#405060","strength":0.5},
        {"type":"shingles","size":[25,15],"gap":2,"shape":"square","shade":0.8,"masked_by":"prev"}]"##,
    ),
];

fn library() -> Library {
    let docs: Vec<(String, String)> = KERNELS
        .iter()
        .flat_map(|(kernel, layers)| {
            let doc = |name: String, extra: &str| {
                let text = format!(r##"{{"format":1,"name":"{name}","unit":"cm","color":"#8a7a6a"{extra},"layers":{layers}}}"##);
                (name, text)
            };
            // The object-mapped twin is turned and scaled, which the world floor never is.
            [doc(kernel.to_string(), ""), doc(format!("{kernel}_turned"), r#","turn":true,"scale":1.3"#)]
        })
        .collect();
    let sources: Vec<(&str, &str)> = docs.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
    let lib = Library::from_sources(&sources);
    let errors: Vec<String> = lib.broken().map(str::to_string).collect();
    assert!(errors.is_empty(), "test materials don't load: {errors:?} {:?}", lib.issues());
    lib
}

fn camera() -> CameraData {
    let eye = Vec3::new(0.0, 10.0, 0.0);
    CameraData {
        view: glam::camera::rh::view::look_to_mat4(eye, Vec3::NEG_Y, Vec3::NEG_Z),
        proj: glam::camera::rh::proj::directx::orthographic(-HALF, HALF, -HALF, HALF, 0.05, 50.0),
        eye,
        forward: Vec3::NEG_Y,
        ortho: true,
    }
}

fn linear_to_srgb8(c: f32) -> i32 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as i32
}

/// A slab whose top face is the surface at y = 0: world-mapped and level, or object-mapped,
/// turned about y and seeded.
struct Case {
    map: MapMode,
    rot: Quat,
    seed: u32,
}

const SLAB: Vec3 = Vec3::new(8.0, 0.2, 8.0);

fn render(renderer: &mut Renderer, table: &mut MaterialTable, lib: &Library, name: &str, case: &Case) -> (Vec<u8>, Vec<u8>) {
    let slot = table.resolve(lib, name, Some(case.map), &[]).expect("material");
    let mut scene = Scene { camera: camera(), ..Default::default() };
    scene.post.outlines = false;
    scene.post.tonemap = Tonemap::Clamp;
    scene.post.bloom = 0.0;
    scene.sun.shadows = false;
    let center = Vec3::new(0.0, -SLAB.y * 0.5, 0.0);
    let shape = Shape::Box { half: SLAB * 0.5 };
    let mut lights = Vec::new();
    let param = slot.param(case.seed);
    emit_shape(
        &mut scene.meshes,
        &mut scene.sdfs,
        &mut lights,
        &shape,
        center,
        case.rot,
        slot.base,
        0.0,
        Style::Unlit,
        1,
        0,
        param,
    );
    table.attach(&mut scene);
    let gpu = capture::render_to_rgba(renderer, &scene, SIZE, SIZE).expect("capture");

    // The CPU twin, from the same packed row.
    let gpu_row = lib.get(name).unwrap().pack(case.map, &[]);
    let px = 2.0 * HALF / SIZE as f32;
    let pixel = 2.0 / (scene.camera.proj.y_axis.y.abs() * SIZE as f32);
    let inv = case.rot.inverse();
    let mut cpu = vec![0u8; gpu.len()];
    for j in 0..SIZE {
        for i in 0..SIZE {
            let world = Vec3::new(-HALF + (i as f32 + 0.5) * px, 0.0, -HALF + (j as f32 + 0.5) * px);
            let sample = Sample {
                local: inv * (world - center),
                local_normal: inv * Vec3::Y,
                world,
                world_normal: Vec3::Y,
                half_extents: SLAB * 0.5,
                seed: param >> 12,
                footprint: footprint(pixel, None, scene.camera.forward, world, Vec3::Y),
            };
            let shaded = eval_packed(&gpu_row.gpu, gpu_row.base, &sample);
            // Unlit: albedo × max(1, 1 + emissive).
            let rgb = shaded.rgb * (1.0 + shaded.glow).max(1.0);
            let k = ((j * SIZE + i) * 4) as usize;
            for (ch, v) in rgb.to_array().into_iter().enumerate() {
                cpu[k + ch] = linear_to_srgb8(v) as u8;
            }
            cpu[k + 3] = 255;
        }
    }
    (gpu, cpu)
}

#[test]
fn gpu_materials_match_the_cpu_reference() {
    let hd = match gpu::Headless::new() {
        Ok(hd) => hd,
        Err(e) => {
            eprintln!("material parity skipped: no GPU adapter ({e:#})");
            return;
        }
    };
    let mut renderer = Renderer::new(&hd.device, &hd.queue);
    let lib = library();
    let mut table = MaterialTable::default();
    let out = std::env::var_os("MATERIAL_PARITY_OUT").map(std::path::PathBuf::from);
    let cases = [
        ("world", Case { map: MapMode::World, rot: Quat::IDENTITY, seed: 0 }),
        ("object", Case { map: MapMode::Object, rot: Quat::from_rotation_y(0.52), seed: seed(41, 3) }),
    ];
    let mut failures = Vec::new();
    for (kernel, _) in KERNELS {
        for (label, case) in &cases {
            let name = if case.map == MapMode::World { format!("MAT/{kernel}") } else { format!("MAT/{kernel}_turned") };
            let (gpu, cpu) = render(&mut renderer, &mut table, &lib, &name, case);
            let total = (SIZE * SIZE) as usize;
            let (mut ok, mut worst) = (0usize, 0i32);
            for p in 0..total {
                let d = (0..3).map(|ch| (gpu[p * 4 + ch] as i32 - cpu[p * 4 + ch] as i32).abs()).max().unwrap();
                worst = worst.max(d);
                ok += (d <= 1) as usize;
            }
            let share = 100.0 * ok as f64 / total as f64;
            println!("parity {kernel:<8} {label:<6}: {share:7.3}% within 1/255, worst {worst}/255");
            if let Some(dir) = &out {
                capture::save_png(&dir.join(format!("{kernel}_{label}_gpu.png")), SIZE, SIZE, &gpu).unwrap();
                capture::save_png(&dir.join(format!("{kernel}_{label}_cpu.png")), SIZE, SIZE, &cpu).unwrap();
            }
            // A flat picture would match trivially: every kernel must paint something.
            let distinct: std::collections::HashSet<&[u8]> = gpu.chunks(4).collect();
            assert!(distinct.len() > 1, "{kernel} {label}: only {} colours", distinct.len());
            if share < 99.9 {
                failures.push(format!("{kernel} {label}: {share:.3}% (worst {worst})"));
            }
        }
    }
    assert!(failures.is_empty(), "below 99.9% within 1/255: {failures:?}");
}
