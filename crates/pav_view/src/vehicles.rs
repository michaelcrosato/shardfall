//! Drawing vehicles: the body comes from the entity's visual; this adds the cabin, wheels and
//! lights of a car (plus tyre smoke while it slides), or the cockpit, tail, rotors and skids of a
//! helicopter.

use glam::{Quat, Vec3};
use pav_core::color::Color;
use pav_core::frame::RenderObject;
use pav_core::shape::Shape;
use pav_core::vehicle::{VehicleKind, VehicleView};
use pav_render::{self as rs, Scene, Style};

use crate::build::emit_shape;

fn hex(s: &str, fallback: &str) -> Vec3 {
    Vec3::from(Color::try_hex(s).unwrap_or(Color::hex(fallback)).0)
}

/// A guard's vision cone as a flat fan on the floor: yellow, turning red as the alert rises.
pub fn emit_cone(scene: &mut Scene, c: &pav_core::stealth::ConeView, group: u32) {
    if c.points.len() < 2 {
        return;
    }
    let calm = Vec3::new(1.0, 0.85, 0.3);
    let alarm = Vec3::new(1.0, 0.18, 0.12);
    let color = calm.lerp(alarm, c.alert.clamp(0.0, 1.0));
    let mut data = pav_render::MeshData::default();
    let v = |p: Vec3, a: f32| pav_render::mesh::Vertex { pos: p.to_array(), normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0], color: [a, a, a, 1.0] };
    data.vertices.push(v(c.origin, 1.0));
    for p in &c.points {
        data.vertices.push(v(*p, 0.75));
    }
    for i in 1..c.points.len() as u32 {
        data.indices.extend_from_slice(&[0, i, i + 1]);
    }
    scene.dynamic.push(rs::DynamicMesh {
        data,
        color,
        emissive: 0.15,
        style: Style::Unlit,
        flags: rs::flags::NO_SHADOW | rs::flags::TWO_SIDED | rs::flags::NO_CUT,
        group,
    });
}

/// Adds a vehicle's parts (and tyre smoke) to the scene.
pub fn emit_vehicle(scene: &mut Scene, o: &RenderObject, v: &VehicleView, style: Style, particles: bool) {
    let group = o.id.0 + 2;
    let rot = o.rot;
    let (fwd, right, up) = (rot * Vec3::Z, rot * Vec3::X, rot * Vec3::Y);
    let h = v.half;
    let body = hex(&v.color, "#e8443a");
    let accent = hex(&v.accent, "#2b2f3a");
    let mut lights = Vec::new();
    let mut part = |scene: &mut Scene, shape: Shape, pos: Vec3, r: Quat, color: Vec3, emissive: f32| {
        emit_shape(&mut scene.meshes, &mut scene.sdfs, &mut lights, &shape, pos, r, color, emissive, style, group, 0);
    };
    match v.kind {
        VehicleKind::Car => {
            // Cabin with dark glass, set back a little.
            let cabin = o.pos + up * (h.y * 1.55) - fwd * (h.z * 0.12);
            part(scene, Shape::RoundedBox { half: Vec3::new(h.x * 0.82, h.y * 0.7, h.z * 0.46), radius: h.y * 0.35 }, cabin, rot, accent, 0.0);
            // Wheels: cylinders turned onto the axle, spinning, front ones steering.
            let wr = (h.y * 1.1).max(0.25);
            for (i, (c, spin, steer)) in v.wheels.iter().enumerate() {
                let q = rot * Quat::from_rotation_y(-steer) * Quat::from_rotation_x(*spin) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
                part(scene, Shape::Cylinder { half_height: 0.13, radius: wr }, *c, q, Vec3::splat(0.08), 0.0);
                let hub = *c + right * if i % 2 == 0 { -0.14 } else { 0.14 };
                part(scene, Shape::Sphere { radius: wr * 0.35 }, hub, rot, Vec3::splat(0.75), 0.0);
            }
            // Head and tail lights (glowing but not lamps themselves: emissive stays at 0.5), and
            // one beam ahead while someone is driving.
            for s in [-1.0f32, 1.0] {
                let front = o.pos + fwd * (h.z * 0.98) + right * (s * h.x * 0.62) + up * (h.y * 0.15);
                part(scene, Shape::Sphere { radius: 0.11 }, front, rot, Vec3::new(1.0, 0.95, 0.8) * 1.6, 0.5);
                let back = o.pos - fwd * (h.z * 0.98) + right * (s * h.x * 0.62) + up * (h.y * 0.15);
                part(scene, Shape::Sphere { radius: 0.09 }, back, rot, Vec3::new(1.0, 0.12, 0.08) * 1.4, 0.5);
            }
            if v.driven {
                scene.point_lights.push(rs::PointLight {
                    position: o.pos + fwd * (h.z + 2.0) + up * 0.4,
                    color: Vec3::new(1.0, 0.92, 0.75) * 0.9,
                    radius: 5.0,
                    shadows: false,
                });
            }
            // Tyre smoke from the rear wheels while sliding.
            if particles && v.drift > 2.5 {
                for (c, _, _) in v.wheels.iter().skip(2) {
                    let (mut b, _) = crate::fx::preset("smoke");
                    b.pos = *c - Vec3::Y * (wr * 0.7);
                    b.count = ((v.drift - 2.0) * 0.6).clamp(1.0, 6.0) as u32;
                    b.size = (0.25, 1.0);
                    b.life = (0.8, 1.4);
                    b.color0 = glam::Vec4::new(0.85, 0.85, 0.88, 0.35);
                    b.color1 = glam::Vec4::new(0.9, 0.9, 0.92, 0.0);
                    b.vel = Vec3::Y * 0.5;
                    scene.particles.push(b);
                }
            }
        }
        VehicleKind::Helicopter => {
            // Cockpit bubble.
            let glass = o.pos + fwd * (h.z * 0.55) + up * (h.y * 0.25);
            part(scene, Shape::Sphere { radius: h.y * 0.82 }, glass, rot, Vec3::new(0.55, 0.75, 0.9), 0.0);
            // Tail boom, fin and tail rotor.
            let boom_a = o.pos - fwd * (h.z * 0.7) + up * (h.y * 0.2);
            let boom_b = o.pos - fwd * (h.z * 2.7) + up * (h.y * 0.45);
            scene.sdfs.push(rs::SdfInstance { a: boom_a, b: boom_b, ra: 0.16, rb: 0.09, color: body, emissive: 0.0, style, flags: 0, group });
            part(scene, Shape::Box { half: Vec3::new(0.04, 0.38, 0.22) }, boom_b + up * 0.3, rot, accent, 0.0);
            let tail_rot = rot * Quat::from_rotation_x(v.rotor * 1.7);
            part(scene, Shape::Box { half: Vec3::new(0.02, 0.45, 0.05) }, boom_b + right * 0.12 + up * 0.15, tail_rot, Vec3::splat(0.2), 0.0);
            // Mast and main rotor (two crossed blades).
            let mast = o.pos + up * (h.y + 0.25);
            scene.sdfs.push(rs::SdfInstance {
                a: o.pos + up * (h.y * 0.8),
                b: mast,
                ra: 0.07,
                rb: 0.07,
                color: Vec3::splat(0.25),
                emissive: 0.0,
                style,
                flags: 0,
                group,
            });
            for k in 0..2 {
                let q = rot * Quat::from_rotation_y(v.rotor + k as f32 * std::f32::consts::FRAC_PI_2);
                part(scene, Shape::Box { half: Vec3::new(3.1, 0.025, 0.11) }, mast, q, Vec3::splat(0.18), 0.0);
            }
            // Skids on struts.
            for s in [-1.0f32, 1.0] {
                let base = o.pos + right * (s * h.x * 0.85) - up * (h.y + 0.28);
                scene.sdfs.push(rs::SdfInstance {
                    a: base - fwd * (h.z * 0.95),
                    b: base + fwd * (h.z * 0.95),
                    ra: 0.06,
                    rb: 0.06,
                    color: accent,
                    emissive: 0.0,
                    style,
                    flags: 0,
                    group,
                });
                for z in [-0.5f32, 0.5] {
                    let top = o.pos + right * (s * h.x * 0.7) - up * (h.y * 0.8) + fwd * (z * h.z);
                    scene.sdfs.push(rs::SdfInstance {
                        a: top,
                        b: base + fwd * (z * h.z),
                        ra: 0.04,
                        rb: 0.04,
                        color: accent,
                        emissive: 0.0,
                        style,
                        flags: 0,
                        group,
                    });
                }
            }
            // Beacon.
            let blink = if v.driven && (v.rotor * 0.3).sin() > 0.6 { 0.5 } else { 0.1 };
            part(scene, Shape::Sphere { radius: 0.07 }, o.pos - up * (h.y * 0.9), rot, Vec3::new(1.0, 0.15, 0.1), blink);
        }
    }
    scene.point_lights.extend(lights);
}
