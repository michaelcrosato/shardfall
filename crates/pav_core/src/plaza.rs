//! The plaza's stations: where the player finds the game and the engine's tools, as places on
//! the pavilion's plaza instead of a menu. The game opens here (the `world` scene). Each
//! station is something built (blocks, decor and a glowing piece with a light), a spot in
//! front of it where the player stands, and a key the window acts on when the player uses it
//! (interact, or a click or a tap on its prompt). The simulation only says which station the
//! player is at (`HudFrame::plaza`); what a key opens is the window's business (studios,
//! panels, other scenes), and its name and prompt are lines (`plaza.<key>`).
//!
//! The layout: the rift into Shardfall and the games stand north of the fountain, the studios
//! south-west, the looks, tuning and guide south-east; every station faces south, toward the
//! camera's usual side.

use glam::{Quat, Vec2, Vec3};

use crate::color::Color;
use crate::entity::{Behavior, Spawn};
use crate::fxdef::{DistortDef, EmitterDef, LightDef};
use crate::shape::{Look, Shape, Sway, Visual};
use crate::sim::Sim;
use crate::statics::{Block, Decor, RegionKey, block_flags};

/// What a station is built as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Build {
    /// Two pillars, a lintel and a glowing pane under falling shards: the way into Shardfall.
    Rift,
    /// A plinth with a glowing piece on it before a coloured back board.
    Booth(Icon),
}

/// The glowing piece on a booth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// A little figure: a body and a head.
    Figure,
    /// A cube on its corner.
    Cube,
    /// An egg with two small ones beside it.
    Egg,
    /// A game token: a short disc.
    Token,
    /// A prism of three colours.
    Prism,
    /// Three sliders.
    Sliders,
    /// A map of four coloured squares.
    Map,
    /// An open book.
    Book,
    /// A globe in a ring.
    Globe,
}

/// One station on the plaza.
#[derive(Clone, Copy, Debug)]
pub struct Station {
    /// What the window opens: `shardfall`, `animation`, `object`, `creature`, `starter`,
    /// `looks`, `tuning`, `rooms`, `guide` or `language`. Its lines are `plaza.<key>` (the
    /// name) and `plaza.<key>.use` (what using it does).
    pub key: &'static str,
    /// The middle of what is built, on the floor.
    pub at: Vec3,
    /// How far south of `at` the player stands to use it (m).
    pub front: f32,
    /// How far from that spot the player can be and still use it (m).
    pub reach: f32,
    /// How tall it stands: the name floats just above.
    pub height: f32,
    /// Its colour (sRGB hex): the glow, the light and the ring on the floor.
    pub color: &'static str,
    pub build: Build,
}

impl Station {
    /// Where the player stands to use it.
    pub fn stand(&self) -> Vec3 {
        self.at + Vec3::Z * self.front
    }

    /// Where its name floats: over its top, clear of what is built.
    pub fn tag(&self) -> Vec3 {
        self.at + Vec3::Y * (self.height + 1.1)
    }
}

const NORTH: f32 = -9.0;
const SOUTH: f32 = 8.6;

/// Every station, the game first.
pub const STATIONS: &[Station] = &[
    Station {
        key: "shardfall",
        at: Vec3::new(-8.2, 0.0, NORTH),
        front: 2.2,
        reach: 2.6,
        height: 5.4,
        color: "#7fd0ff",
        build: Build::Rift,
    },
    booth("starter", 5.0, NORTH, "#c48cff", Icon::Token),
    booth("rooms", 8.2, NORTH, "#e8704a", Icon::Map),
    booth("language", 11.4, NORTH, "#9fd8c8", Icon::Globe),
    booth("animation", -11.4, SOUTH, "#68b6ff", Icon::Figure),
    booth("creature", -8.2, SOUTH, "#78d68c", Icon::Egg),
    booth("object", -5.0, SOUTH, "#f0c45c", Icon::Cube),
    booth("guide", 5.0, SOUTH, "#d8c9a8", Icon::Book),
    booth("tuning", 8.2, SOUTH, "#b4bac8", Icon::Sliders),
    booth("looks", 11.4, SOUTH, "#ff80c4", Icon::Prism),
];

const fn booth(key: &'static str, x: f32, z: f32, color: &'static str, icon: Icon) -> Station {
    Station { key, at: Vec3::new(x, 0.0, z), front: 1.5, reach: 1.45, height: 2.2, color, build: Build::Booth(icon) }
}

/// The station by its key.
pub fn station(key: &str) -> Option<&'static Station> {
    STATIONS.iter().find(|s| s.key == key)
}

/// The station whose spot the feet are on (the nearest, if spots ever overlap).
pub fn near(feet: Vec3) -> Option<usize> {
    if feet.y.abs() > 1.5 {
        return None;
    }
    let p = Vec2::new(feet.x, feet.z);
    STATIONS
        .iter()
        .enumerate()
        .map(|(i, s)| (i, p.distance(Vec2::new(s.stand().x, s.stand().z)), s.reach))
        .filter(|(_, d, reach)| d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _, _)| i)
}

/// Where a player arriving from somewhere else stands: on the station's spot, facing it.
pub fn arrival(key: &str) -> Option<(Vec3, Vec3)> {
    station(key).map(|s| (s.stand() + Vec3::Z * 0.6, -Vec3::Z))
}

fn decor(shape: Shape, pos: Vec3, rot: Quat, color: Color, look: Look, emissive: f32, solid: bool) -> Decor {
    Decor { shape, pos, rot, color, look, emissive, solid, collider: None, sway: Sway::None, material: None }
}

/// The plaza's stations as blocks and decor, for the hub region.
pub fn blocks() -> (Vec<Block>, Vec<Decor>) {
    let mut blocks = Vec::new();
    let mut out = Vec::new();
    for s in STATIONS {
        let glow = Color::hex(s.color);
        // A ring on the floor where the player stands.
        out.push(decor(
            Shape::Cylinder { half_height: 0.012, radius: (s.reach * 0.75).min(1.5) },
            s.stand() + Vec3::Y * 0.014,
            Quat::IDENTITY,
            glow.lerp(Color::hex("#d8d2c4"), 0.7),
            Look::Flat,
            0.0,
            false,
        ));
        match s.build {
            Build::Rift => rift_blocks(s, glow, &mut blocks, &mut out),
            Build::Booth(icon) => booth_blocks(s, icon, glow, &mut blocks, &mut out),
        }
    }
    (blocks, out)
}

fn rift_blocks(s: &Station, glow: Color, blocks: &mut Vec<Block>, out: &mut Vec<Decor>) {
    let at = s.at;
    let stone = Color::hex("#5a5866");
    let trim = Color::hex("#c9a54a");
    for x in [-2.1f32, 2.1] {
        blocks.push(
            Block::new(at + Vec3::new(x - 0.4, 0.0, -0.4), at + Vec3::new(x + 0.4, 3.9, 0.4), stone)
                .with_flags(block_flags::ROUNDED),
        );
        blocks.push(Block::new(at + Vec3::new(x - 0.48, 3.9, -0.48), at + Vec3::new(x + 0.48, 4.06, 0.48), trim));
        blocks.push(Block::new(at + Vec3::new(x - 0.5, 0.0, -0.5), at + Vec3::new(x + 0.5, 0.3, 0.5), trim));
    }
    blocks.push(Block::new(at + Vec3::new(-2.75, 4.06, -0.5), at + Vec3::new(2.75, 4.66, 0.5), stone));
    blocks.push(Block::new(at + Vec3::new(-0.6, 4.66, -0.25), at + Vec3::new(0.6, 5.1, 0.25), trim));
    // A step up to the pane.
    blocks.push(Block::new(at + Vec3::new(-1.7, 0.0, -0.3), at + Vec3::new(1.7, 0.12, 0.9), Color::hex("#bfb6a3")));
    // Three shards hang above the lintel, as on the emblem.
    for (dx, dy, tilt, scale) in [(-1.05f32, 5.55f32, -0.35f32, 0.6f32), (0.0, 5.95, 0.0, 1.0), (1.0, 5.6, 0.38, 0.65)] {
        out.push(decor(
            Shape::Box { half: Vec3::new(0.16, 0.5, 0.16) * scale },
            at + Vec3::new(dx, dy, 0.0),
            Quat::from_rotation_z(tilt) * Quat::from_rotation_y(0.785),
            glow,
            Look::Unlit,
            1.2,
            false,
        ));
    }
}

fn booth_blocks(s: &Station, icon: Icon, glow: Color, blocks: &mut Vec<Block>, out: &mut Vec<Decor>) {
    let at = s.at;
    let stone = Color::hex("#d8d2c4");
    let dark = Color::hex("#4a3b2c");
    // The back board, a strip of the station's colour along its top, and the plinth.
    blocks.push(Block::new(at + Vec3::new(-0.95, 0.0, -0.55), at + Vec3::new(0.95, 2.0, -0.4), dark));
    blocks.push(Block::new(at + Vec3::new(-0.95, 2.0, -0.57), at + Vec3::new(0.95, 2.18, -0.38), glow));
    blocks.push(
        Block::new(at + Vec3::new(-0.62, 0.0, -0.35), at + Vec3::new(0.62, 0.95, 0.35), stone).with_flags(block_flags::ROUNDED),
    );
    out.push(decor(
        Shape::Box { half: Vec3::new(0.66, 0.03, 0.39) },
        at + Vec3::new(0.0, 0.98, 0.0),
        Quat::IDENTITY,
        Color::hex("#c9a54a"),
        Look::Cel,
        0.0,
        false,
    ));
    let top = at + Vec3::Y * 1.0;
    let d = |shape: Shape, pos: Vec3, rot: Quat, color: Color, emissive: f32| {
        decor(shape, pos, rot, color, if emissive > 0.0 { Look::Unlit } else { Look::Cel }, emissive, false)
    };
    // The parts of the icon that stand still (the glowing piece is an entity: `spawn`).
    match icon {
        Icon::Figure => {
            out.push(d(Shape::Box { half: Vec3::new(0.3, 0.04, 0.2) }, top + Vec3::Y * 0.04, Quat::IDENTITY, dark, 0.0));
        }
        Icon::Egg => {
            for x in [-0.32f32, 0.32] {
                out.push(d(Shape::Sphere { radius: 0.11 }, top + Vec3::new(x, 0.11, 0.08), Quat::IDENTITY, glow, 0.6));
            }
        }
        Icon::Sliders => {
            for (i, x) in [-0.3f32, 0.0, 0.3].into_iter().enumerate() {
                out.push(d(
                    Shape::Cylinder { half_height: 0.32, radius: 0.025 },
                    top + Vec3::new(x, 0.32, 0.0),
                    Quat::IDENTITY,
                    dark,
                    0.0,
                ));
                let h = [0.18f32, 0.46, 0.3][i];
                out.push(d(
                    Shape::Box { half: Vec3::new(0.08, 0.04, 0.06) },
                    top + Vec3::new(x, h, 0.0),
                    Quat::IDENTITY,
                    glow,
                    0.8,
                ));
            }
        }
        Icon::Map => {
            out.push(d(
                Shape::Box { half: Vec3::new(0.42, 0.02, 0.3) },
                top + Vec3::new(0.0, 0.02, 0.0),
                Quat::IDENTITY,
                Color::hex("#e2d6bd"),
                0.0,
            ));
            for (k, c) in ["#e8704a", "#68b6ff", "#78d68c", "#f0c45c"].iter().enumerate() {
                let (x, z) = ([-0.18f32, 0.18][k % 2], [-0.12f32, 0.12][k / 2]);
                out.push(d(
                    Shape::Box { half: Vec3::new(0.14, 0.02, 0.09) },
                    top + Vec3::new(x, 0.05, z),
                    Quat::IDENTITY,
                    Color::hex(c),
                    0.5,
                ));
            }
        }
        Icon::Book => {
            for (x, a) in [(-0.2f32, 0.22f32), (0.2, -0.22)] {
                out.push(d(
                    Shape::Box { half: Vec3::new(0.2, 0.02, 0.28) },
                    top + Vec3::new(x, 0.1, 0.0),
                    Quat::from_rotation_z(a),
                    Color::hex("#f3e7cf"),
                    0.0,
                ));
            }
        }
        Icon::Prism => {
            for (k, c) in ["#ff6b6b", "#6bd88a", "#6ba8ff"].iter().enumerate() {
                let a = k as f32 * std::f32::consts::TAU / 3.0;
                out.push(d(
                    Shape::Box { half: Vec3::new(0.1, 0.1, 0.1) },
                    top + Vec3::new(a.cos() * 0.3, 0.1, a.sin() * 0.2),
                    Quat::IDENTITY,
                    Color::hex(c),
                    0.7,
                ));
            }
        }
        Icon::Cube | Icon::Token | Icon::Globe => {}
    }
}

/// The glowing pieces (each with its light): entities in the hub region, made when the hub
/// is first built.
pub fn spawn(sim: &mut Sim) {
    for s in STATIONS {
        let glow = Color::hex(s.color);
        let light = |radius: f32, intensity: f32, y: f32| {
            Some(Box::new(LightDef {
                color: s.color.into(),
                radius,
                intensity,
                pulse: 0.35,
                offset: Vec3::new(0.0, y, 0.6),
                ..Default::default()
            }))
        };
        let mut piece = match s.build {
            Build::Rift => {
                let mut pane = Visual::new(Shape::Box { half: Vec3::new(1.65, 1.95, 0.05) }, Color::hex("#4f9fe8"));
                pane.look = Look::Unlit;
                pane.emissive = 0.35;
                pane.light = light(6.0, 0.9, -1.0);
                pane.particles = Some(Box::new(EmitterDef {
                    preset: "magic".into(),
                    color: "#b8e8ff".into(),
                    area: Vec3::new(1.5, 1.8, 0.1),
                    size: 0.8,
                    ..Default::default()
                }));
                pane.distortion = Some(Box::new(DistortDef {
                    kind: "ripple".into(),
                    radius: 1.8,
                    strength: 0.3,
                    period: 1.4,
                    offset: Vec3::ZERO,
                }));
                Spawn::new("plaza: shardfall", s.at + Vec3::Y * 2.07).visual(pane)
            }
            Build::Booth(icon) => {
                let (shape, y, rot, spin) = match icon {
                    Icon::Figure => (Shape::Capsule { half_height: 0.16, radius: 0.12 }, 0.4, Quat::IDENTITY, 0.0),
                    Icon::Cube => (
                        Shape::RoundedBox { half: Vec3::splat(0.2), radius: 0.03 },
                        0.42,
                        Quat::from_rotation_z(0.785) * Quat::from_rotation_x(0.615),
                        0.7,
                    ),
                    Icon::Egg => (Shape::Capsule { half_height: 0.07, radius: 0.17 }, 0.27, Quat::IDENTITY, 0.0),
                    Icon::Token => (Shape::Cylinder { half_height: 0.06, radius: 0.26 }, 0.45, Quat::from_rotation_x(1.2), 0.9),
                    Icon::Prism => (Shape::Sphere { radius: 0.13 }, 0.48, Quat::IDENTITY, 0.0),
                    Icon::Sliders => (Shape::Box { half: Vec3::new(0.42, 0.015, 0.1) }, 0.02, Quat::IDENTITY, 0.0),
                    Icon::Map => (Shape::Sphere { radius: 0.06 }, 0.42, Quat::IDENTITY, 0.0),
                    Icon::Book => (Shape::Sphere { radius: 0.08 }, 0.46, Quat::IDENTITY, 0.0),
                    Icon::Globe => (Shape::Sphere { radius: 0.24 }, 0.42, Quat::IDENTITY, 0.5),
                };
                let mut v = Visual::new(shape, glow);
                v.look = Look::Unlit;
                v.emissive = 0.9;
                v.light = light(2.6, 0.4, 0.8);
                let mut spawn = Spawn::new(&format!("plaza: {}", s.key), s.at + Vec3::Y * (1.0 + y)).visual(v).rot(rot);
                if spin != 0.0 {
                    spawn = spawn.behavior(Behavior::Spin { speed: spin });
                }
                if icon == Icon::Figure {
                    // The figure's head.
                    let mut head = Visual::new(Shape::Sphere { radius: 0.11 }, glow);
                    head.look = Look::Unlit;
                    head.emissive = 0.9;
                    let mut h = Spawn::new("plaza: animation head", s.at + Vec3::Y * 1.83).visual(head);
                    h.region = Some(RegionKey::Hub);
                    sim.spawn(h);
                }
                if icon == Icon::Globe {
                    let mut ring = Visual::new(Shape::Cylinder { half_height: 0.012, radius: 0.33 }, Color::hex("#c9a54a"));
                    ring.look = Look::Cel;
                    let mut r =
                        Spawn::new("plaza: globe ring", s.at + Vec3::Y * 1.42).visual(ring).rot(Quat::from_rotation_x(0.4));
                    r.region = Some(RegionKey::Hub);
                    sim.spawn(r);
                }
                spawn
            }
        };
        piece.region = Some(RegionKey::Hub);
        sim.spawn(piece);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stations_stand_clear_of_the_lanes_and_each_other() {
        let lane = crate::world::CORRIDOR_HALF + 0.4;
        for s in STATIONS {
            let half = if s.build == Build::Rift { 2.8 } else { 1.0 };
            assert!(s.at.x.abs() - half > lane, "{} stands in a north-south lane", s.key);
            assert!(
                s.at.z.abs() - 0.6 > lane && s.stand().z.abs() - s.reach > lane - 1.0,
                "{} stands in an east-west lane",
                s.key
            );
            assert!(s.at.x.abs() + half < crate::world::PLAZA_HALF - 0.8, "{} is in the wall", s.key);
            assert!(s.stand().z.abs() + s.reach.min(1.6) < crate::world::PLAZA_HALF, "{}'s spot is in the wall", s.key);
        }
        for (i, a) in STATIONS.iter().enumerate() {
            for b in &STATIONS[i + 1..] {
                let d = Vec2::new(a.stand().x - b.stand().x, a.stand().z - b.stand().z).length();
                assert!(d > a.reach + b.reach, "{} and {} share ground", a.key, b.key);
            }
        }
    }

    #[test]
    fn standing_on_a_spot_finds_its_station() {
        for (i, s) in STATIONS.iter().enumerate() {
            assert_eq!(near(s.stand()), Some(i), "{}", s.key);
            assert_eq!(near(s.stand() + Vec3::X * (s.reach + 0.1)), None, "{}", s.key);
        }
        assert_eq!(near(Vec3::new(0.0, 0.0, 6.0)), None, "the spawn is no station");
        let (feet, facing) = arrival("shardfall").unwrap();
        assert_eq!(near(feet), Some(0));
        assert!(facing.z < 0.0);
    }
}
