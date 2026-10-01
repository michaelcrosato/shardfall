//! Scenes: the full world, small code-defined test scenes, and single rooms on their own.

use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use glam::{Quat, Vec3};

use crate::color::Color;
use crate::entity::{Behavior, BodyKind, Spawn};
use crate::level::Placement;
use crate::room::{self, RoomDef};
use crate::shape::{Look, Shape, Visual};
use crate::sim::Sim;
use crate::statics::Block;
use crate::world::{RoomSlot, World};

pub const SCENES: &[(&str, &str)] = &[
    ("world", "The pavilion with every room, surrounded by streaming wilderness. 'world/<room>' starts in a room."),
    ("test", "Lit test scene: a small plaza, every primitive and style, props raining onto a pyramid."),
    ("empty", "A single floor slab with a player."),
    ("arena", "Shardfall: the Proving Grounds, a wave arena (combat test ground)."),
    ("town", "Shardfall: Emberwatch, the town (smith, stash, portal)."),
    ("lab", "Shardfall: the Menagerie, a gallery of designed and generated creatures."),
    ("level", "Shardfall: a level of the descent. 'level/<n>' is depth n (1-12 designed, then the endless Depths)."),
];

/// All scene names, including standalone rooms (any room key builds just that room).
pub fn names() -> Vec<String> {
    let mut v: Vec<String> = SCENES.iter().map(|s| s.0.to_string()).collect();
    let (rooms, _) = room::parse_all(&room::load_sources(room::rooms_dir().as_deref()));
    v.extend(rooms.into_iter().map(|(k, _)| k));
    v
}

/// Builds one room on its own at its layout origin (fast; used by tests and agents).
pub fn build_standalone_room(sim: &mut Sim, key: &str, def: RoomDef) {
    let place = Placement::new(def.layout.origin, 0);
    let (cols, rows) = def.layout.extent();
    let (min, max) = place.aabb(Vec3::new(0.0, -1.0, 0.0), Vec3::new(cols as f32, def.height.max(2.0), rows as f32));
    let start = def.local_start();
    let inward = -def.entrance.facing.dir();
    let def = Arc::new(def);
    sim.state.world = World {
        enabled: false,
        rooms: vec![RoomSlot {
            id: 0,
            key: key.to_string(),
            def: def.clone(),
            place,
            min,
            max,
            inside: place.point(start),
            outside: place.point(start) - inward * 4.0,
            inward,
            built: false,
        }],
        current_room: Some(0),
        ..Default::default()
    };
    sim.build_room(0);
    // Player at the "player" marker if the layout has one, else inside the entrance.
    let marker = def
        .layout
        .layers
        .iter()
        .any(|l| def.layout.legend.iter().any(|(k, t)| t.marker.as_deref() == Some("player") && l.map.contains(k.as_str())));
    sim.state.spawn = if marker { find_marker(&def, &place, "player").unwrap_or(place.point(start)) } else { place.point(start) };
    sim.spawn_player();
    sim.state.focus = sim.state.spawn;
    sim.state.world.saved_params = crate::world::enter_overrides(&mut sim.config, &def);
}

fn find_marker(def: &RoomDef, place: &Placement, name: &str) -> Option<Vec3> {
    for l in &def.layout.layers {
        let lines = crate::level::rows(&l.map);
        for (r, row) in lines.iter().enumerate() {
            for (c, ch) in row.chars().enumerate() {
                if def.layout.legend.get(&ch.to_string()).and_then(|t| t.marker.as_deref()) == Some(name) {
                    let local = Vec3::new(l.at[0] as f32 + c as f32 + 0.5, l.y, l.at[1] as f32 + r as f32 + 0.5);
                    return Some(place.point(local));
                }
            }
        }
    }
    None
}

pub fn build(sim: &mut Sim, name: &str) -> Result<()> {
    let (base, start_room) = match name.split_once('/') {
        Some((b, r)) => (b, Some(r)),
        None => (name, None),
    };
    match base {
        "world" => {
            let sources = room::load_sources(room::rooms_dir().as_deref());
            let (defs, errors) = room::parse_all(&sources);
            for (k, e) in &errors {
                log::error!("room file '{k}': {e}");
            }
            sim.build_world(defs, errors);
            if let Some(r) = start_room {
                if !sim.teleport_to_room(r) {
                    bail!(
                        "unknown room '{r}' (known: {})",
                        sim.state.world.rooms.iter().map(|r| r.key.as_str()).collect::<Vec<_>>().join(", ")
                    );
                }
            }
        }
        "test" => test_scene(sim),
        "arena" => crate::arpg::scene::build_arena(sim),
        "town" => crate::arpg::scene::build_town(sim, None),
        "lab" => crate::arpg::scene::build_lab(sim, None),
        "level" => {
            let n: u32 = match start_room {
                Some(r) => r.parse().map_err(|_| anyhow!("level/<n>: '{r}' is not a depth"))?,
                None => 1,
            };
            crate::arpg::world::build_level(sim, n.max(1), None);
        }
        "empty" => {
            floor(sim, 20);
            sim.state.spawn = Vec3::new(0.5, 0.0, 0.5);
            sim.spawn_player();
        }
        key => {
            let sources = room::load_sources(room::rooms_dir().as_deref());
            let Some(src) = sources.iter().find(|s| s.key == key) else {
                bail!(
                    "unknown scene or room '{key}' (scenes: world, test, empty; rooms: {})",
                    sources.iter().map(|s| s.key.as_str()).collect::<Vec<_>>().join(", ")
                );
            };
            let def = RoomDef::parse(&src.text).map_err(|e| anyhow!("room '{key}': {e}"))?;
            build_standalone_room(sim, key, def);
        }
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
