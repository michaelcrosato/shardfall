//! Built-in scenes (code-defined worlds). Rooms from data files arrive in M3.

use anyhow::{Result, bail};
use glam::{Quat, Vec3};

use crate::color::Color;
use crate::entity::{Behavior, BodyKind, Spawn};
use crate::level::Layout;
use crate::shape::{Look, Shape, Visual};
use crate::sim::Sim;
use crate::statics::Block;

pub const SCENES: &[(&str, &str)] = &[
    ("playground", "Core movement test: crates, stairs, gap, crawl tunnel, ladder, destructible upper floor."),
    ("test", "Lit test scene: a small plaza, every primitive and style, props raining onto a pyramid."),
    ("empty", "A single floor slab with a player."),
];

/// A room description (M2 subset of the M3 room format).
#[derive(Clone, Debug, serde::Deserialize)]
pub struct RoomFile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub about: String,
    pub layout: Layout,
}

pub const PLAYGROUND: &str = include_str!("../../../rooms/playground.toml");

/// Builds a room from TOML text and spawns the player at its "player" marker.
pub fn build_room_text(sim: &mut Sim, text: &str) -> Result<()> {
    let room: RoomFile = toml::from_str(text).map_err(|e| anyhow::anyhow!("room file: {e}"))?;
    let built = room.layout.build(sim);
    for w in &built.warnings {
        log::warn!("{w}");
    }
    sim.state.spawn = built.marker("player").unwrap_or(Vec3::new(0.0, 0.0, 0.0));
    sim.spawn_player();
    sim.state.focus = sim.state.spawn;
    Ok(())
}

pub fn build(sim: &mut Sim, name: &str) -> Result<()> {
    match name {
        "playground" => build_room_text(sim, PLAYGROUND)?,
        "test" => test_scene(sim),
        "empty" => {
            floor(sim, 20);
            sim.state.spawn = Vec3::new(0.5, 0.0, 0.5);
            sim.spawn_player();
        }
        _ => bail!("unknown scene '{name}' (known: {})", SCENES.iter().map(|s| s.0).collect::<Vec<_>>().join(", ")),
    }
    Ok(())
}

/// A checkered floor of 1 m tiles from -half..half.
fn floor(sim: &mut Sim, half: i32) {
    let a = Color::hex("#8fbf7a");
    let b = Color::hex("#86b872");
    let st = &mut sim.state;
    for x in -half..half {
        for z in -half..half {
            let c = if (x + z) % 2 == 0 { a } else { b };
            st.statics.add(
                &mut st.physics,
                Block::new(Vec3::new(x as f32, -0.5, z as f32), Vec3::new(x as f32 + 1.0, 0.0, z as f32 + 1.0), c),
            );
        }
    }
}

fn test_scene(sim: &mut Sim) {
    floor(sim, 16);
    let st = &mut sim.state;
    // Low walls around a plaza and a stepped platform (heights: 0.5, 1.0, 1.5 m).
    let wall = Color::hex("#d9cbb0");
    for i in -6..6 {
        for (x, z) in [(i, -8), (i, 7), (-8, i), (7, i)] {
            st.statics.add(
                &mut st.physics,
                Block::new(Vec3::new(x as f32, 0.0, z as f32), Vec3::new(x as f32 + 1.0, 1.0, z as f32 + 1.0), wall),
            );
        }
    }
    let step = Color::hex("#c9a27a");
    for (k, h) in [0.5f32, 1.0, 1.5].iter().enumerate() {
        let x0 = 3.0 + k as f32 * 1.0;
        st.statics.add(&mut st.physics, Block::new(Vec3::new(x0, 0.0, -5.0), Vec3::new(x0 + 1.0, *h, -2.0), step));
    }

    // Style gallery: the same shapes in flat / cel / lit.
    let looks = [Look::Flat, Look::Cel, Look::Lit];
    for (i, look) in looks.iter().enumerate() {
        let x = -5.5 + i as f32 * 1.6;
        let mut v = Visual::new(Shape::Sphere { radius: 0.5 }, Color::hex("#f2c14e"));
        v.look = *look;
        sim.spawn(Spawn::new("gallery_sphere", Vec3::new(x, 0.5, 4.5)).visual(v.clone()).body(BodyKind::Fixed));
        v.shape = Shape::Capsule { half_height: 0.35, radius: 0.3 };
        v.color = Color::hex("#9b5de5");
        sim.spawn(Spawn::new("gallery_capsule", Vec3::new(x, 0.65, 2.8)).visual(v.clone()).body(BodyKind::Fixed));
        v.shape = Shape::RoundedBox { half: Vec3::new(0.5, 0.35, 0.5), radius: 0.12 };
        v.color = Color::hex("#5b8def");
        sim.spawn(Spawn::new("gallery_rbox", Vec3::new(x, 0.35, 1.1)).visual(v.clone()).body(BodyKind::Fixed));
        v.shape = Shape::Cylinder { half_height: 0.6, radius: 0.35 };
        v.color = Color::hex("#b8b8c8");
        sim.spawn(Spawn::new("gallery_cylinder", Vec3::new(x, 0.6, -0.6)).visual(v).body(BodyKind::Fixed));
    }

    // Pyramid of crates.
    let crate_color = Color::hex("#e8704a");
    for layer in 0..4 {
        let n = 4 - layer;
        for i in 0..n {
            for j in 0..n {
                let p = Vec3::new(
                    1.0 + (i as f32 - (n - 1) as f32 * 0.5) * 0.62,
                    0.3 + layer as f32 * 0.6,
                    3.0 + (j as f32 - (n - 1) as f32 * 0.5) * 0.62,
                );
                sim.spawn(
                    Spawn::new("crate", p)
                        .visual(Visual::new(Shape::Box { half: Vec3::splat(0.3) }, crate_color))
                        .body(BodyKind::Dynamic),
                );
            }
        }
    }

    // A spinning platform (kinematic) and the prop rain above it.
    let mut plat = Visual::new(Shape::Cylinder { half_height: 0.15, radius: 2.0 }, Color::hex("#6c7a89"));
    plat.look = Look::Cel;
    sim.spawn(
        Spawn::new("spinner", Vec3::new(1.0, 0.4, 3.0))
            .visual(plat)
            .body(BodyKind::Kinematic)
            .behavior(Behavior::Spin { speed: 0.6 }),
    );
    sim.spawn(Spawn::new("rain", Vec3::new(1.0, 0.0, 3.0)).behavior(Behavior::Rain {
        interval: 20,
        max: 40,
        area: 2.5,
        height: 8.0,
        timer: 0,
        spawned: Default::default(),
    }));

    // Glowing marker.
    let mut lamp = Visual::new(Shape::Sphere { radius: 0.25 }, Color::hex("#ffd27a"));
    lamp.look = Look::Unlit;
    lamp.emissive = 1.5;
    sim.spawn(Spawn::new("lamp", Vec3::new(-3.0, 2.2, -3.0)).visual(lamp).rot(Quat::IDENTITY));
    sim.state.focus = Vec3::new(0.0, 0.0, 1.0);
}
