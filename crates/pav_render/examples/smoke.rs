//! Renders a small test scene to smoke.png (headless). `cargo run -p pav_render --example smoke`
use glam::{Mat4, Quat, Vec3};
use pav_render::*;

fn main() -> anyhow::Result<()> {
    let gpu = gpu::Headless::new()?;
    println!("adapter: {}", gpu::describe(&gpu.adapter.get_info()));
    let mut r = Renderer::new(&gpu.device, &gpu.queue);
    let mut scene = Scene::default();
    let eye = Vec3::new(0.0, 12.0, 9.0);
    let fwd = (Vec3::new(0.0, 0.0, 0.0) - eye).normalize();
    scene.camera = CameraData {
        view: glam::camera::rh::view::look_to_mat4(eye, fwd, Vec3::Y),
        proj: glam::camera::rh::proj::directx::perspective(40f32.to_radians(), 16.0 / 9.0, 0.1, 200.0),
        eye,
        forward: fwd,
        ortho: false,
    };
    let inst = |mesh, t: Mat4, c: Vec3, style, group| MeshInstance {
        mesh,
        transform: t,
        color: c,
        emissive: 0.0,
        style,
        flags: 0,
        group,
    };
    scene.meshes.push(inst(
        MeshKey::Cube,
        Mat4::from_scale_rotation_translation(Vec3::new(20.0, 1.0, 20.0), Quat::IDENTITY, Vec3::new(0.0, -0.5, 0.0)),
        srgb(120, 170, 110),
        Style::Cel,
        1,
    ));
    for (i, style) in Style::ALL.iter().enumerate() {
        let x = i as f32 * 2.5 - 3.75;
        scene.meshes.push(inst(
            MeshKey::Cube,
            Mat4::from_rotation_translation(Quat::from_rotation_y(0.5), Vec3::new(x, 0.5, 2.0)),
            srgb(220, 120, 80),
            *style,
            10 + i as u32,
        ));
        scene.meshes.push(inst(
            MeshKey::rounded_box(Vec3::new(0.5, 0.4, 0.5), 0.15),
            Mat4::from_translation(Vec3::new(x, 0.4, 4.2)),
            srgb(90, 140, 230),
            *style,
            20 + i as u32,
        ));
        let mut s = SdfInstance::sphere(Vec3::new(x, 0.7, -0.5), 0.7, srgb(240, 200, 60));
        s.style = *style;
        s.group = 30 + i as u32;
        scene.sdfs.push(s);
        let mut c = SdfInstance::capsule(Vec3::new(x - 0.4, 0.4, -3.0), Vec3::new(x + 0.4, 1.4, -3.0), 0.35, srgb(200, 90, 200));
        c.style = *style;
        c.group = 40 + i as u32;
        scene.sdfs.push(c);
        scene.meshes.push(inst(
            MeshKey::Cylinder,
            Mat4::from_scale_rotation_translation(Vec3::new(0.8, 1.6, 0.8), Quat::IDENTITY, Vec3::new(x, 0.8, -5.5)),
            srgb(150, 150, 160),
            *style,
            50 + i as u32,
        ));
    }
    scene.point_lights.push(PointLight {
        position: Vec3::new(0.0, 1.5, 1.0),
        color: Vec3::new(1.0, 0.5, 0.2) * 2.0,
        radius: 5.0,
    });
    let t = std::time::Instant::now();
    let px = capture::render_to_rgba(&mut r, &scene, 960, 540)?;
    println!("rendered in {:?}, stats {:?}", t.elapsed(), r.stats);
    capture::save_png(std::path::Path::new("smoke.png"), 960, 540, &px)?;
    Ok(())
}
