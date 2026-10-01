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
use crate::params::ParamValue;
use crate::shape::{Look, Shape, Visual};
use crate::sim::Sim;
use crate::statics::{Block, Facing, Ladder, RegionKey, block_flags};
use crate::zones::{CameraCue, Label, LabelMode, Zone, ZoneKind, default_zone_color};

/// Where a layout goes in the world: translation plus quarter turns (clockwise from above).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub origin: Vec3,
    pub quarters: u8,
}

impl Placement {
    pub fn new(origin: Vec3, quarters: u8) -> Self {
        Self { origin, quarters: quarters % 4 }
    }
    /// Rotates a layout-space offset (x = column, z = row).
    pub fn rotate(&self, v: Vec3) -> Vec3 {
        let mut v = v;
        for _ in 0..self.quarters {
            v = Vec3::new(-v.z, v.y, v.x);
        }
        v
    }
    pub fn point(&self, local: Vec3) -> Vec3 {
        self.origin + self.rotate(local)
    }
    /// Transforms a layout-space box to a world-space axis-aligned box.
    pub fn aabb(&self, min: Vec3, max: Vec3) -> (Vec3, Vec3) {
        let (a, b) = (self.point(min), self.point(max));
        (a.min(b), a.max(b))
    }
    pub fn facing(&self, f: Facing) -> Facing {
        f.rotated(self.quarters)
    }
    pub fn quat(&self) -> Quat {
        Quat::from_rotation_y(-(self.quarters as f32) * std::f32::consts::FRAC_PI_2)
    }
}

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

/// A trigger zone covering the tile (identical neighbours merge into one rectangle).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ZoneDef {
    pub kind: ZoneKind,
    #[serde(default)]
    pub course: String,
    #[serde(default)]
    pub index: i32,
    /// Bottom and top relative to the layer (default 0 .. 3 m).
    #[serde(default)]
    pub y0: f32,
    #[serde(default = "three")]
    pub y1: f32,
    #[serde(default)]
    pub params: BTreeMap<String, ParamValue>,
    #[serde(default)]
    pub camera: Option<CameraCue>,
    #[serde(default)]
    pub label: String,
    /// Letter height of the label (default: fits the zone).
    #[serde(default)]
    pub label_size: Option<f32>,
    /// Floor marking colour; "none" hides it.
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub facing: Option<Facing>,
}

fn three() -> f32 {
    3.0
}

/// In-world text. In a legend it sits on the tile; in a room's `[[label]]` list `pos` is in
/// layout space (x = column, y = height, z = row).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LabelDef {
    pub text: String,
    #[serde(default)]
    pub pos: Option<Vec3>,
    /// Height above the layer when placed on a tile.
    #[serde(default = "label_y")]
    pub y: f32,
    #[serde(default = "label_size")]
    pub size: f32,
    #[serde(default = "label_color")]
    pub color: String,
    #[serde(default)]
    pub mode: LabelMode,
    /// Wall labels: the direction the text faces.
    #[serde(default = "south")]
    pub facing: Facing,
}

fn label_y() -> f32 {
    0.03
}
fn label_size() -> f32 {
    0.5
}
fn label_color() -> String {
    "#2b2d35".into()
}
fn south() -> Facing {
    Facing::South
}

impl LabelDef {
    pub fn to_label(&self, place: &Placement, local: Vec3) -> Label {
        Label {
            text: self.text.clone(),
            pos: place.point(local),
            size: self.size,
            color: Color::hex(&self.color),
            mode: self.mode,
            facing: place.facing(self.facing),
        }
    }
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
    #[serde(default)]
    pub zone: Option<ZoneDef>,
    #[serde(default)]
    pub label: Option<LabelDef>,
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
    pub zones: usize,
    pub props: usize,
    pub warnings: Vec<String>,
}

impl BuiltLayout {
    pub fn marker(&self, name: &str) -> Option<Vec3> {
        self.markers.iter().find(|m| m.0 == name).map(|m| m.1)
    }
}

/// Map rows. A leading empty line (common in multi-line strings) and trailing blank lines are
/// ignored; blank lines in between (and a leading row of spaces) are kept so rows stay aligned.
pub(crate) fn rows(map: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = map.lines().collect();
    if lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines
}

impl Layout {
    /// Size in cells (columns, rows) over all layers.
    pub fn extent(&self) -> (i32, i32) {
        let (mut w, mut h) = (0, 0);
        for l in &self.layers {
            let r = rows(&l.map);
            h = h.max(l.at[1] + r.len() as i32);
            w = w.max(l.at[0] + r.iter().map(|r| r.chars().count() as i32).max().unwrap_or(0));
        }
        (w, h)
    }

    /// Builds all layers at the layout's own origin, unrotated, into terrain chunks.
    pub fn build(&self, sim: &mut Sim) -> BuiltLayout {
        self.build_at(sim, &Placement::new(self.origin, 0), None)
    }

    /// Builds all layers with a placement. Blocks go into `region` (or the chunk under them).
    pub fn build_at(&self, sim: &mut Sim, place: &Placement, region: Option<RegionKey>) -> BuiltLayout {
        let mut out = BuiltLayout::default();
        for layer in &self.layers {
            let base = Vec3::new(layer.at[0] as f32, layer.y, layer.at[1] as f32);
            // Zone rectangles: (c0, c1, r0, r1, def), merged across columns and rows.
            let mut rects: Vec<(usize, usize, usize, usize, &ZoneDef)> = Vec::new();
            let mut brects: Vec<(usize, usize, usize, usize, &Piece)> = Vec::new();
            for (r, row) in rows(&layer.map).iter().enumerate() {
                let mut zruns: Vec<(usize, usize, &ZoneDef)> = Vec::new();
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
                    let cell = base + Vec3::new(c as f32, 0.0, r as f32); // layout space
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
                        let (min, max) = place.aabb(Vec3::new(min.x, cell.y, min.z), Vec3::new(max.x, cell.y + l.height, max.z));
                        let ladder = Ladder { min, max, facing: place.facing(l.facing), color: Color::hex(&l.color) };
                        match region {
                            Some(k) => sim.state.statics.add_ladder_to(k, ladder),
                            None => sim.state.statics.add_ladder(ladder),
                        }
                    }
                    if let Some(p) = &def.prop {
                        let half = p.shape.half_extents();
                        let y = p.y.unwrap_or(half.y);
                        let mut v = Visual::new(p.shape, Color::hex(&p.color));
                        v.look = p.look;
                        let name = if p.name.is_empty() { "prop".to_string() } else { p.name.clone() };
                        let mut sp = Spawn::new(&name, place.point(cell + Vec3::new(0.5, y, 0.5)))
                            .visual(v)
                            .body(p.body)
                            .rot(place.quat());
                        sp.region = region;
                        sim.spawn(sp);
                        out.props += 1;
                    }
                    if let Some(m) = &def.marker {
                        out.markers.push((m.clone(), place.point(cell + Vec3::new(0.5, 0.0, 0.5))));
                    }
                    if let Some(z) = &def.zone {
                        match zruns.last_mut() {
                            Some(run) if run.1 == c && run.2 == z => run.1 = c + 1,
                            _ => zruns.push((c, c + 1, z)),
                        }
                    }
                    if let Some(l) = &def.label {
                        let local = cell + Vec3::new(0.5, l.y, 0.5);
                        let label = l.to_label(place, local);
                        if let Some(k) = region.or(Some(RegionKey::chunk_of(label.pos))) {
                            sim.state.statics.add_label_to(k, label);
                        }
                    }
                }
                for (c0, c1, z) in zruns {
                    match rects.iter_mut().find(|q| q.0 == c0 && q.1 == c1 && q.3 == r && q.4 == z) {
                        Some(q) => q.3 = r + 1,
                        None => rects.push((c0, c1, r, r + 1, z)),
                    }
                }
                for (c0, c1, p) in runs {
                    // Merge with the same run on the row above (not for destructible tiles).
                    match brects.iter_mut().find(|q| q.0 == c0 && q.1 == c1 && q.3 == r && q.4 == p && !p.destructible) {
                        Some(q) => q.3 = r + 1,
                        None => brects.push((c0, c1, r, r + 1, p)),
                    }
                }
            }
            for (c0, c1, r0, r1, p) in brects {
                let i = p.inset;
                let (min, max) = place.aabb(
                    Vec3::new(base.x + c0 as f32 + i, base.y + p.y0, base.z + r0 as f32 + i),
                    Vec3::new(base.x + c1 as f32 - i, base.y + p.y1, base.z + r1 as f32 - i),
                );
                let mut b = Block::new(min, max, Color::hex(&p.color)).with_look(p.look);
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
                match region {
                    Some(k) => st.statics.add_to(&mut st.physics, k, b),
                    None => st.statics.add(&mut st.physics, b),
                };
                out.blocks += 1;
            }
            for (c0, c1, r0, r1, z) in rects {
                let (min, max) = place.aabb(
                    Vec3::new(base.x + c0 as f32, base.y + z.y0, base.z + r0 as f32),
                    Vec3::new(base.x + c1 as f32, base.y + z.y1, base.z + r1 as f32),
                );
                let color = match z.color.as_deref() {
                    Some("none") => None,
                    Some(c) => Color::try_hex(c),
                    None => default_zone_color(z.kind),
                };
                let zone = Zone {
                    min,
                    max,
                    kind: z.kind,
                    course: z.course.clone(),
                    index: z.index,
                    params: z.params.clone(),
                    camera: z.camera.clone(),
                    label: z.label.clone(),
                    label_size: z.label_size,
                    color,
                    facing: z.facing.map(|f| place.facing(f)),
                };
                let key = region.unwrap_or(RegionKey::chunk_of(zone.center()));
                sim.state.statics.add_zone_to(key, zone);
                out.zones += 1;
            }
        }
        out
    }
}
