//! Levels as text: ASCII map layers plus a legend, in TOML. Two map planes:
//!
//! * `plane = "xz"` (default): a top-down map. Columns go east (+x), rows go south (+z).
//!   Each character's legend entry stacks blocks with heights (`y0`..`y1`, metres above the
//!   layer's `y`).
//! * `plane = "xy"`: a side view. Columns go east (+x), rows go DOWN; the bottom row sits on
//!   the layer's `y`. Blocks fill their cell (`y0`..`y1` inside it) and are `z0`..`z1` deep.
//!
//! Identical neighbouring blocks merge into one box, so maps stay cheap. AGENTS.md documents
//! every field; the sample games' levels are complete examples.

use std::collections::BTreeMap;

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::entity::{Body, Look, Shape, Spawn};
use crate::params::{self, ParamValue};
use crate::util::Color;
use crate::world::World;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelFile {
    #[serde(default)]
    pub name: String,
    /// Metres per map character.
    #[serde(default = "one")]
    pub cell: f32,
    /// World position of column 0, row 0 (xz) or of the bottom-left cell (xy).
    #[serde(default)]
    pub origin: Vec3,
    #[serde(default)]
    pub sky: Option<Color>,
    #[serde(default)]
    pub horizon: Option<Color>,
    /// Parameter overrides applied when the level loads, e.g. `"camera.tilt" = 10`.
    #[serde(default)]
    pub params: BTreeMap<String, toml::Value>,
    #[serde(default, rename = "layer")]
    pub layers: Vec<Layer>,
    /// One entry per map character. '.' and ' ' without an entry are empty.
    #[serde(default)]
    pub legend: BTreeMap<String, Tile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    #[serde(default)]
    pub plane: Plane,
    /// Base height (xz: of the whole layer; xy: of the bottom row).
    #[serde(default)]
    pub y: f32,
    /// xy plane: depth offset.
    #[serde(default)]
    pub z: f32,
    pub map: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Plane {
    #[default]
    Xz,
    Xy,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tile {
    #[serde(default)]
    pub block: Option<Piece>,
    #[serde(default)]
    pub blocks: Vec<Piece>,
    #[serde(default)]
    pub spawn: Option<SpawnDef>,
    #[serde(default)]
    pub trigger: Option<TriggerDef>,
    /// A named point (e.g. "player"), read with `World::marker`.
    #[serde(default)]
    pub marker: Option<String>,
}

/// A solid box in the cell.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Piece {
    /// Bottom and top (m): above the layer's y (xz) or inside the cell (xy).
    #[serde(default)]
    pub y0: f32,
    #[serde(default = "one")]
    pub y1: f32,
    /// Depth for xy layers (m).
    #[serde(default = "minus_one")]
    pub z0: f32,
    #[serde(default = "one")]
    pub z1: f32,
    #[serde(default = "grey")]
    pub color: Color,
    #[serde(default)]
    pub look: Look,
    /// Entity kind (default "block").
    #[serde(default = "block_kind")]
    pub kind: String,
    /// Shrinks the box on each side (m), e.g. pillars.
    #[serde(default)]
    pub inset: f32,
    /// Health: shots damage it (`Event::Killed` at 0). Such blocks never merge.
    #[serde(default)]
    pub hp: f32,
    /// Moves back and forth by this offset (a kinematic platform).
    #[serde(default, rename = "move")]
    pub move_by: Option<Vec3>,
    #[serde(default = "three")]
    pub period: f32,
    #[serde(default)]
    pub hold: f32,
    #[serde(default)]
    pub phase: f32,
}

/// An entity created in the cell (pickups, crates, enemies' spots...).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnDef {
    pub kind: String,
    /// box | sphere | capsule | cylinder
    #[serde(default = "box_shape")]
    pub shape: String,
    /// box: edge or [x, y, z]; sphere: radius; capsule / cylinder: [height, radius].
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default = "dynamic")]
    pub body: Body,
    #[serde(default = "grey")]
    pub color: Color,
    #[serde(default)]
    pub look: Look,
    /// Height of its centre above the cell floor (default: resting on it).
    #[serde(default)]
    pub y: Option<f32>,
    #[serde(default)]
    pub hp: f32,
    #[serde(default)]
    pub team: u8,
    /// Not drawn (invisible triggers, spawn points that still collide).
    #[serde(default)]
    pub hidden: bool,
    /// Starting rotation: [x, y, z] degrees (applied in that order).
    #[serde(default)]
    pub rot: Option<Vec3>,
    /// Turns around the vertical axis (degrees per second).
    #[serde(default)]
    pub spin: f32,
    #[serde(default, rename = "move")]
    pub move_by: Option<Vec3>,
    #[serde(default = "three")]
    pub period: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Size {
    One(f32),
    Many(Vec<f32>),
}

/// An invisible trigger volume over the cell (neighbours merge): goals, hazards, checkpoints.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerDef {
    pub kind: String,
    #[serde(default)]
    pub y0: f32,
    #[serde(default = "two")]
    pub y1: f32,
    #[serde(default = "minus_one")]
    pub z0: f32,
    #[serde(default = "one")]
    pub z1: f32,
    /// Draw it (glowing) in this colour; default invisible.
    #[serde(default)]
    pub color: Option<Color>,
}

fn one() -> f32 {
    1.0
}
fn two() -> f32 {
    2.0
}
fn three() -> f32 {
    3.0
}
fn minus_one() -> f32 {
    -1.0
}
fn grey() -> Color {
    Color::hex("#b0b0b0")
}
fn block_kind() -> String {
    "block".into()
}
fn box_shape() -> String {
    "box".into()
}
fn dynamic() -> Body {
    Body::Dynamic
}

/// What a level load produced.
#[derive(Clone, Debug, Default, Serialize)]
pub struct LevelInfo {
    pub name: String,
    /// Columns and rows of the largest layer.
    pub size: [usize; 2],
    pub blocks: usize,
    pub spawns: usize,
    pub triggers: usize,
    pub markers: Vec<(String, Vec3)>,
    pub warnings: Vec<String>,
}

/// Map rows: a leading empty line and trailing blank lines are dropped.
fn rows(map: &str) -> Vec<Vec<char>> {
    let mut lines: Vec<&str> = map.lines().collect();
    if lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines.iter().map(|l| l.chars().collect()).collect()
}

/// Merges equal cells into rectangles: (col0, col1, row0, row1, value). `across` allows
/// merging along a row, `down` merging rows.
fn rects<T: PartialEq + Clone>(
    cells: &[(usize, usize, T)],
    across: impl Fn(&T) -> bool,
    down: impl Fn(&T) -> bool,
) -> Vec<(usize, usize, usize, usize, T)> {
    let mut by_row: BTreeMap<usize, Vec<(usize, usize, T)>> = BTreeMap::new();
    for (r, c, v) in cells {
        let runs = by_row.entry(*r).or_default();
        match runs.iter_mut().rev().find(|run| run.1 == *c && run.2 == *v) {
            Some(run) if across(v) => run.1 = c + 1,
            _ => runs.push((*c, c + 1, v.clone())),
        }
    }
    let mut out: Vec<(usize, usize, usize, usize, T)> = Vec::new();
    for (r, runs) in by_row {
        for (c0, c1, v) in runs {
            match out.iter_mut().find(|q| q.0 == c0 && q.1 == c1 && q.3 == r && q.4 == v) {
                Some(q) if down(&v) => q.3 = r + 1,
                _ => out.push((c0, c1, r, r + 1, v)),
            }
        }
    }
    out
}

impl Size {
    fn get(&self, i: usize, default: f32) -> f32 {
        match self {
            Size::One(x) => *x,
            Size::Many(v) => v.get(i).copied().unwrap_or(default),
        }
    }
}

impl SpawnDef {
    pub fn shape(&self) -> Result<Shape, String> {
        let s = self.size.as_ref();
        Ok(match self.shape.as_str() {
            "box" => match s {
                Some(Size::Many(v)) if v.len() == 3 => Shape::cube(Vec3::new(v[0], v[1], v[2])),
                Some(sz) => Shape::cube(Vec3::splat(sz.get(0, 1.0))),
                None => Shape::cube(Vec3::ONE),
            },
            "sphere" => Shape::Sphere { radius: s.map(|z| z.get(0, 0.5)).unwrap_or(0.5) },
            "capsule" | "cylinder" => {
                let h = s.map(|z| z.get(0, 1.0)).unwrap_or(1.0);
                let r = s.map(|z| z.get(1, 0.3)).unwrap_or(0.3);
                if self.shape == "capsule" {
                    Shape::Capsule { half_height: (h * 0.5 - r).max(0.01), radius: r }
                } else {
                    Shape::Cylinder { half_height: h * 0.5, radius: r }
                }
            }
            other => return Err(format!("unknown shape '{other}' (box, sphere, capsule, cylinder)")),
        })
    }
}

/// Parses a level without building it (errors carry line and column).
pub fn parse(text: &str) -> Result<LevelFile, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}

impl World {
    /// Builds a level from TOML text into this world: blocks, spawns, triggers, markers,
    /// parameter overrides. Usually called from `Game::setup` with `include_str!`.
    pub fn load_level(&mut self, text: &str) -> Result<LevelInfo, String> {
        let lf = parse(text)?;
        self.build_level(&lf)
    }

    pub fn build_level(&mut self, lf: &LevelFile) -> Result<LevelInfo, String> {
        let mut info = LevelInfo { name: lf.name.clone(), ..Default::default() };
        let cell = lf.cell.max(0.05);
        if let Some(c) = lf.sky {
            self.env.sky = c;
        }
        if let Some(c) = lf.horizon {
            self.env.horizon = c;
        }
        for (path, v) in &lf.params {
            let value = match v {
                toml::Value::Boolean(b) => ParamValue::Bool(*b),
                toml::Value::Integer(i) => ParamValue::Float(*i as f64),
                toml::Value::Float(f) => ParamValue::Float(*f),
                toml::Value::String(s) => ParamValue::Text(s.clone()),
                other => ParamValue::Text(other.to_string()),
            };
            params::set(self, path, &value).map_err(|e| format!("[params] {e}"))?;
        }
        for (key, tile) in &lf.legend {
            if key.chars().count() != 1 {
                return Err(format!("legend key '{key}' must be a single character"));
            }
            if let Some(s) = &tile.spawn {
                s.shape().map_err(|e| format!("legend '{key}': {e}"))?;
            }
        }
        for (li, layer) in lf.layers.iter().enumerate() {
            let grid = rows(&layer.map);
            let nrows = grid.len();
            info.size[0] = info.size[0].max(grid.iter().map(|r| r.len()).max().unwrap_or(0));
            info.size[1] = info.size[1].max(nrows);
            // Cell geometry: (x0, x1) east-west, bottom y, and z range or row for xz.
            let x_of = |c: usize| lf.origin.x + c as f32 * cell;
            let base_y = |r: usize| match layer.plane {
                Plane::Xz => lf.origin.y + layer.y,
                Plane::Xy => lf.origin.y + layer.y + (nrows - 1 - r) as f32 * cell,
            };
            let z_of = |r: usize| lf.origin.z + r as f32 * cell;
            let zc = lf.origin.z + layer.z;
            let mut blocks: Vec<(usize, usize, Piece)> = Vec::new();
            let mut triggers: Vec<(usize, usize, TriggerDef)> = Vec::new();
            for (r, row) in grid.iter().enumerate() {
                for (c, ch) in row.iter().enumerate() {
                    let Some(tile) = lf.legend.get(&ch.to_string()) else {
                        if *ch != '.' && *ch != ' ' {
                            let w = format!("layer {li}: no legend entry for '{ch}' (row {r}, column {c})");
                            if !info.warnings.contains(&w) && info.warnings.len() < 20 {
                                info.warnings.push(w);
                            }
                        }
                        continue;
                    };
                    for p in tile.block.iter().chain(tile.blocks.iter()) {
                        blocks.push((r, c, p.clone()));
                    }
                    if let Some(t) = &tile.trigger {
                        triggers.push((r, c, t.clone()));
                    }
                    let cx = x_of(c) + cell * 0.5;
                    let floor = base_y(r);
                    let (cz, top) = match layer.plane {
                        Plane::Xz => {
                            let top = tile.block.iter().chain(tile.blocks.iter()).map(|p| p.y1).fold(0.0f32, f32::max);
                            (z_of(r) + cell * 0.5, floor + top)
                        }
                        Plane::Xy => (zc, floor),
                    };
                    if let Some(m) = &tile.marker {
                        let p = Vec3::new(cx, top, cz);
                        self.markers.push((m.clone(), p));
                        info.markers.push((m.clone(), p));
                    }
                    if let Some(s) = &tile.spawn {
                        let shape = s.shape()?;
                        let y = s.y.unwrap_or(shape.half_extents().y);
                        let pos = Vec3::new(cx, top + y, cz);
                        let mut sp = Spawn::new(&s.kind, pos).shape(shape).body(s.body).look(s.look).team(s.team).hp(s.hp);
                        sp.color = s.color;
                        if s.hidden {
                            sp = sp.hidden();
                        }
                        if let Some(r) = s.rot {
                            let r = r * std::f32::consts::PI / 180.0;
                            sp = sp.rot(glam::Quat::from_euler(glam::EulerRot::XYZ, r.x, r.y, r.z));
                        }
                        if s.spin != 0.0 {
                            sp = sp.spin(Vec3::Y * s.spin.to_radians());
                        }
                        if let Some(m) = s.move_by {
                            sp = sp.mover(m, s.period, 0.0);
                        }
                        self.spawn(sp);
                        info.spawns += 1;
                    }
                }
            }
            // Side views only stack rows into one box when the piece fills its cell's height.
            let full = |p: &Piece| p.hp <= 0.0 && (layer.plane == Plane::Xz || (p.y0 <= 0.0 && p.y1 >= cell));
            for (c0, c1, r0, r1, p) in rects(&blocks, |p: &Piece| p.hp <= 0.0, full) {
                let (min, max) = match layer.plane {
                    Plane::Xz => (
                        Vec3::new(x_of(c0) + p.inset, base_y(r0) + p.y0, z_of(r0) + p.inset),
                        Vec3::new(x_of(c1) - p.inset, base_y(r0) + p.y1, z_of(r1) - p.inset),
                    ),
                    // Rows grow downward: r1 - 1 is the lowest row of the rectangle.
                    Plane::Xy => (
                        Vec3::new(x_of(c0) + p.inset, base_y(r1 - 1) + p.y0, zc + p.z0),
                        Vec3::new(x_of(c1) - p.inset, base_y(r0) + p.y1, zc + p.z1),
                    ),
                };
                let (min, max) = (min.min(max), min.max(max));
                let size = max - min;
                if size.min_element() <= 0.0 {
                    info.warnings.push(format!("layer {li}: block '{}' has no volume (check y0/y1, inset)", p.kind));
                    continue;
                }
                let mut sp = Spawn::new(&p.kind, (min + max) * 0.5).cube(size).look(p.look).hp(p.hp);
                sp.color = p.color;
                if let Some(m) = p.move_by {
                    sp = sp.mover(m, p.period, p.hold);
                    if let Some(mv) = &mut sp.mover {
                        mv.phase = p.phase;
                    }
                }
                self.spawn(sp);
                info.blocks += 1;
            }
            for (c0, c1, r0, r1, t) in rects(&triggers, |_| true, |_| true) {
                let (min, max) = match layer.plane {
                    Plane::Xz => {
                        (Vec3::new(x_of(c0), base_y(r0) + t.y0, z_of(r0)), Vec3::new(x_of(c1), base_y(r0) + t.y1, z_of(r1)))
                    }
                    Plane::Xy => (
                        Vec3::new(x_of(c0), base_y(r1 - 1) + t.y0.min(cell * 0.99), zc + t.z0),
                        Vec3::new(x_of(c1), base_y(r0) + t.y1.min(cell), zc + t.z1),
                    ),
                };
                let (min, max) = (min.min(max), min.max(max).max(min + Vec3::splat(0.01)));
                let mut sp = Spawn::new(&t.kind, (min + max) * 0.5).cube(max - min).body(Body::Trigger).look(Look::Glow);
                match t.color {
                    Some(c) => sp.color = c,
                    None => sp.visible = false,
                }
                self.spawn(sp);
                info.triggers += 1;
            }
        }
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOP: &str = r##"
name = "test"
[[layer]]
map = """
#####
#P.c#
#####
"""
[legend]
"#" = { block = { y0 = 0, y1 = 2, color = "#808080" } }
"." = { block = { y0 = -0.5, y1 = 0 } }
"P" = { marker = "player", block = { y0 = -0.5, y1 = 0 } }
"c" = { spawn = { kind = "coin", shape = "sphere", size = 0.3, body = "trigger", y = 1 }, block = { y0 = -0.5, y1 = 0 } }
"##;

    #[test]
    fn top_down_map_merges_and_places() {
        let mut w = World::new(1);
        let info = w.load_level(TOP).unwrap();
        assert!(info.warnings.is_empty(), "{:?}", info.warnings);
        // Walls: top row, bottom row, two side pieces; floor: one merged strip of 3.
        assert_eq!(info.blocks, 4 + 1);
        assert_eq!(info.spawns, 1);
        let p = w.marker("player").unwrap();
        assert!((p - Vec3::new(1.5, 0.0, 1.5)).length() < 1e-5, "{p}");
        let coin = w.each("coin").next().unwrap();
        assert!((coin.pos - Vec3::new(3.5, 1.0, 1.5)).length() < 1e-5);
    }

    #[test]
    fn side_view_rows_grow_down() {
        let text = r##"
[[layer]]
plane = "xy"
map = """
P..
.=.
###
"""
[legend]
"#" = { block = { color = "#808080" } }
"=" = { block = { y0 = 0.7, y1 = 1.0, move = [2, 0, 0], period = 2 } }
"P" = { marker = "player" }
"##;
        let mut w = World::new(1);
        let info = w.load_level(text).unwrap();
        assert_eq!(info.blocks, 2);
        let p = w.marker("player").unwrap();
        assert!((p - Vec3::new(0.5, 2.0, 0.0)).length() < 1e-5, "{p}");
        let ground = w.entities.values().find(|e| e.mover.is_none() && e.kind == "block").unwrap();
        assert!((ground.pos - Vec3::new(1.5, 0.5, 0.0)).length() < 1e-5, "{}", ground.pos);
        let plat = w.entities.values().find(|e| e.mover.is_some()).unwrap();
        assert!((plat.pos.y - 1.85).abs() < 1e-5, "{}", plat.pos);
        assert_eq!(plat.body, Body::Kinematic);
    }

    #[test]
    fn errors_are_helpful() {
        assert!(parse("[[layer]]\nmapp = \"x\"").unwrap_err().contains("mapp"));
        let mut w = World::new(1);
        let e = w.load_level("[params]\n\"camera.tlt\" = 3").unwrap_err();
        assert!(e.contains("camera.tlt"), "{e}");
    }
}
