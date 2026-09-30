//! Tile levels with heights, written as ASCII layers plus a legend. The same structure is used
//! from code (scenes) and from room data files (M3).
//!
//! ```text
//! [[layer]]            # one horizontal slice; y = base elevation of this layer
//! y = 0
//! map = """
//! ##########
//! #..B..L..#
//! """
//! [legend.'#']         # each character maps to blocks (relative heights), a ladder, a prop...
//! blocks = [{ y0 = 0, y1 = 3, color = "#d9cbb0" }]
//! ```

use std::collections::BTreeMap;

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::entity::{BodyKind, Spawn};
use crate::shape::{Look, Shape, Visual};
use crate::sim::Sim;
use crate::statics::{Block, Facing, Ladder, block_flags};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Piece {
    /// Bottom and top, relative to the layer's `y`.
    pub y0: f32,
    pub y1: f32,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub look: Look,
    /// Explosions remove it.
    #[serde(default)]
    pub destructible: bool,
    #[serde(default)]
    pub rounded: bool,
    /// No collision.
    #[serde(default)]
    pub ghost: bool,
    /// Shrinks the block horizontally on every side (m), e.g. for pillars.
    #[serde(default)]
    pub inset: f32,
}

fn default_color() -> String {
    "#b0b0b0".into()
}

impl Piece {
    pub fn new(y0: f32, y1: f32, color: &str) -> Self {
        Self { y0, y1, color: color.into(), look: Look::Cel, destructible: false, rounded: false, ghost: false, inset: 0.0 }
    }
    pub fn destructible(mut self) -> Self {
        self.destructible = true;
        self
    }
    pub fn rounded(mut self) -> Self {
        self.rounded = true;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LadderDef {
    /// Direction from the climber toward the wall.
    pub facing: Facing,
    /// Height above the layer's `y` (usually the top of the wall).
    pub height: f32,
    #[serde(default = "ladder_color")]
    pub color: String,
}

fn ladder_color() -> String {
    "#a0703c".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropDef {
    pub shape: Shape,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "dynamic")]
    pub body: BodyKind,
    #[serde(default)]
    pub look: Look,
    /// Height of the prop's centre above the layer (default: resting on it).
    #[serde(default)]
    pub y: Option<f32>,
    #[serde(default)]
    pub name: String,
}

fn dynamic() -> BodyKind {
    BodyKind::Dynamic
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TileDef {
    #[serde(default)]
    pub blocks: Vec<Piece>,
    #[serde(default)]
    pub ladder: Option<LadderDef>,
    #[serde(default)]
    pub prop: Option<PropDef>,
    /// Named marker, e.g. "player" for the player start.
    #[serde(default)]
    pub marker: Option<String>,
}

impl TileDef {
    pub fn blocks(pieces: Vec<Piece>) -> Self {
        Self { blocks: pieces, ..Default::default() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    #[serde(default)]
    pub y: f32,
    /// Column and row of this layer's first character in the layout grid.
    #[serde(default)]
    pub at: [i32; 2],
    pub map: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Layout {
    /// World position of the top-left map character (column 0, row 0).
    #[serde(default)]
    pub origin: Vec3,
    #[serde(default, rename = "layer")]
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub legend: BTreeMap<String, TileDef>,
}

/// Where named markers ended up.
#[derive(Clone, Debug, Default)]
pub struct BuiltLayout {
    pub markers: Vec<(String, Vec3)>,
    pub blocks: usize,
    pub props: usize,
    pub warnings: Vec<String>,
}

impl BuiltLayout {
    pub fn marker(&self, name: &str) -> Option<Vec3> {
        self.markers.iter().find(|m| m.0 == name).map(|m| m.1)
    }
}

/// Map rows. A leading empty line (common in multi-line strings) and trailing blank lines are
/// ignored; blank lines in between are kept as empty rows so rows stay aligned.
fn rows(map: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = map.lines().collect();
    if lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines
}

impl Layout {
    /// Builds all layers into the simulation (blocks, ladders, props, markers).
    pub fn build(&self, sim: &mut Sim) -> BuiltLayout {
        let mut out = BuiltLayout::default();
        for layer in &self.layers {
            let base = self.origin + Vec3::new(layer.at[0] as f32, layer.y, layer.at[1] as f32);
            for (r, row) in rows(&layer.map).iter().enumerate() {
                // Collect (column, piece) runs so identical neighbours merge into one block.
                let mut runs: Vec<(usize, usize, &Piece)> = Vec::new();
                for (c, ch) in row.chars().enumerate() {
                    if ch == ' ' {
                        continue;
                    }
                    let Some(def) = self.legend.get(&ch.to_string()) else {
                        if ch != '.' {
                            out.warnings.push(format!("layer y={}: no legend entry for '{ch}'", layer.y));
                        }
                        continue;
                    };
                    let cell = base + Vec3::new(c as f32, 0.0, r as f32);
                    for p in &def.blocks {
                        match runs.iter_mut().rev().find(|(_, end, rp)| *end == c && *rp == p && !p.destructible) {
                            Some(run) => run.1 = c + 1,
                            None => runs.push((c, c + 1, p)),
                        }
                    }
                    if let Some(l) = &def.ladder {
                        let (x, z) = (cell.x, cell.z);
                        let (min, max) = match l.facing {
                            Facing::North => (Vec3::new(x + 0.2, 0.0, z), Vec3::new(x + 0.8, 0.0, z + 0.3)),
                            Facing::South => (Vec3::new(x + 0.2, 0.0, z + 0.7), Vec3::new(x + 0.8, 0.0, z + 1.0)),
                            Facing::East => (Vec3::new(x + 0.7, 0.0, z + 0.2), Vec3::new(x + 1.0, 0.0, z + 0.8)),
                            Facing::West => (Vec3::new(x, 0.0, z + 0.2), Vec3::new(x + 0.3, 0.0, z + 0.8)),
                        };
                        sim.state.statics.add_ladder(Ladder {
                            min: Vec3::new(min.x, cell.y, min.z),
                            max: Vec3::new(max.x, cell.y + l.height, max.z),
                            facing: l.facing,
                            color: Color::hex(&l.color),
                        });
                    }
                    if let Some(p) = &def.prop {
                        let half = p.shape.half_extents();
                        let y = p.y.unwrap_or(half.y);
                        let mut v = Visual::new(p.shape, Color::hex(&p.color));
                        v.look = p.look;
                        let name = if p.name.is_empty() { "prop".to_string() } else { p.name.clone() };
                        sim.spawn(Spawn::new(&name, cell + Vec3::new(0.5, y, 0.5)).visual(v).body(p.body).rot(Quat::IDENTITY));
                        out.props += 1;
                    }
                    if let Some(m) = &def.marker {
                        out.markers.push((m.clone(), cell + Vec3::new(0.5, 0.0, 0.5)));
                    }
                }
                let z = base.z + r as f32;
                for (c0, c1, p) in runs {
                    let i = p.inset;
                    let mut b = Block::new(
                        Vec3::new(base.x + c0 as f32 + i, base.y + p.y0, z + i),
                        Vec3::new(base.x + c1 as f32 - i, base.y + p.y1, z + 1.0 - i),
                        Color::hex(&p.color),
                    )
                    .with_look(p.look);
                    if p.destructible {
                        b = b.with_flags(block_flags::DESTRUCTIBLE);
                    }
                    if p.rounded {
                        b = b.with_flags(block_flags::ROUNDED);
                    }
                    if p.ghost {
                        b = b.with_flags(block_flags::GHOST);
                    }
                    let st = &mut sim.state;
                    st.statics.add(&mut st.physics, b);
                    out.blocks += 1;
                }
            }
        }
        out
    }
}
