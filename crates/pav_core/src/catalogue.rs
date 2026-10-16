//! The Catalogue room's pages (docs/AGENT_ASSET_STORE.md §10): every object row of the store, a
//! page at a time, each prop beside a 1.8 m figure and signed with its row. Rooms may name only
//! assets every build carries, so the room names none: a pad's signal makes the `catalogue` behaviour read
//! the rows (the embedded indexes, then the committed packs on disk, never `local/`) and place a
//! page of props as entities. A page is simulation state, so rewind, snapshots and replays keep
//! it (a snapshot carries each placed definition). A build with no asset files (the browser, a
//! lone exe) pages through the rows it carries: the embedded tier and the shipped packs.

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::Sim;
use crate::color::Color;
use crate::entity::{BodyKind, EntityId, Spawn};
use crate::prop_instance::PropInstance;
use crate::props::index::{self, Row};
use crate::props::{self, PropDef, Tier};
use crate::shape::{Shape, Visual};
use crate::statics::RegionKey;
use crate::zones::{Label, LabelMode};

/// Pages through the store's object rows: `signal` shows the next page and `back` the one
/// before (both wrap round). In a room file: `behavior = { type = "catalogue" }` on an object
/// with `body = "none"`; the page grows from the object toward its +x and +z.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogueDef {
    pub signal: String,
    pub back: String,
    /// Rows a page shows, and how many stand side by side before the next line.
    pub per_page: u32,
    pub columns: u32,
    /// Which way a page grows from the object: 1 toward +z (its first line at the object), -1
    /// toward -z with its first line still at the object (put the object by the pads, and the
    /// page starts where the player stands). Props face +z either way.
    pub toward: i32,
    /// The page on show (-1: none yet), what it placed and the signs it put up.
    pub page: i32,
    pub spawned: Vec<EntityId>,
    pub signs: Vec<Label>,
}

impl Default for CatalogueDef {
    fn default() -> Self {
        Self {
            signal: "next".into(),
            back: "back".into(),
            per_page: 20,
            columns: 5,
            toward: 1,
            page: -1,
            spawned: Vec::new(),
            signs: Vec::new(),
        }
    }
}

impl CatalogueDef {
    /// As authored: no page on show.
    pub fn fresh(&self) -> Self {
        Self { page: -1, spawned: Vec::new(), signs: Vec::new(), ..self.clone() }
    }

    /// +1 for the next page, -1 for the one before, 0 when no signal of its came this tick.
    pub fn step(&self, signals: &[String]) -> i32 {
        let sent = |name: &String| !name.is_empty() && signals.contains(name);
        if sent(&self.signal) {
            1
        } else if sent(&self.back) {
            -1
        } else {
            0
        }
    }
}

/// The store's object rows the catalogue pages through, in order: the embedded indexes (the
/// templates, then `LIB/core`), then the committed packs under the props root, then the shipped
/// packs' rows the disk lacks (a build with no asset files: the browser, a lone exe). Native
/// builds only read packs; `local/` rows are never store entries.
pub fn rows() -> Vec<Row> {
    let mut rows = index::embedded().to_vec();
    let (disk, _) = index::disk(&props::disk_root());
    rows.extend(disk.into_iter().filter(|row| row.tier == Tier::Disk && !crate::store::local_path(&row.name)));
    for row in index::shipped() {
        if !rows.iter().any(|r| r.name == row.name) {
            rows.push(row.clone());
        }
    }
    rows
}

/// A row's share of mesh parts, in percent (`mesh40` in its `fid` column; 0 for the rest).
pub fn mesh_share(row: &Row) -> u32 {
    row.fid.strip_prefix("mesh").and_then(|pct| pct.parse().ok()).unwrap_or(0)
}

/// What `asset_edit action=check` lints in an asset: its geometry, size band and sockets (check
/// also counts render budgets).
pub fn findings(def: &PropDef) -> usize {
    let lint = props::lint::lint(&def.compiled, &props::lint::Options::default());
    lint.iter().filter(|f| !crate::figure::exempt(def, f)).count() + props::lint::archetype(def).len()
}

/// A prop's sign: its name, then its row.
pub fn sign(row: &Row, findings: Option<usize>) -> String {
    let (pack, leaf) = row.name.rsplit_once('/').unwrap_or(("", &row.name));
    let found = match findings {
        Some(1) => "1 finding".to_owned(),
        Some(n) => format!("{n} findings"),
        None => "does not load".to_owned(),
    };
    format!("{leaf}\n{pack}\n{} cm · {} · {}\nmesh {}% · {found}", row.size_cm, row.level, row.licence, mesh_share(row))
}

/// The text shown in place of a page when the store has no rows at all.
pub const NO_ROWS: &str = "THE STORE HAS NO OBJECT ROWS";

/// Height and body radius of the scale figure (as on contact sheets).
const FIGURE_HEIGHT: f32 = 1.8;
const FIGURE_RADIUS: f32 = 0.22;
const HEAD: f32 = 0.125;
/// Room a sign needs across and in front of its prop; the gap between cells and lines (m).
const SIGN_WIDTH: f32 = 3.4;
const SIGN_DEPTH: f32 = 1.4;
const GAP: f32 = 0.6;

/// Where one row of a page goes, in the catalogue's own frame (x across, z down the page, y up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    /// The centre of the prop's footprint, on the floor.
    pub prop: Vec3,
    /// The figure's feet, past the prop's back-right corner.
    pub figure: Vec3,
    /// The sign's centre, in front of the prop.
    pub sign: Vec3,
}

/// Lays out props of these sizes (m) in lines of `columns`, each line centred on x = 0 and the
/// first starting at z = 0: each cell as wide as its prop and figure (at least a sign's width),
/// each line as deep as its deepest prop plus its signs.
pub fn layout(sizes: &[Vec3], columns: usize) -> Vec<Spot> {
    let mut spots = Vec::with_capacity(sizes.len());
    let mut z = 0.0;
    let cell = |size: &Vec3| (size.x + 2.0 * FIGURE_RADIUS + 0.35).max(SIGN_WIDTH);
    for line in sizes.chunks(columns.max(1)) {
        let deep = line.iter().map(|s| s.z).fold(0.0, f32::max);
        let wide: f32 = line.iter().map(|size| cell(size) + GAP).sum::<f32>() - GAP;
        let mut x = -wide * 0.5;
        for size in line {
            spots.push(Spot {
                prop: Vec3::new(x + size.x * 0.5, 0.0, z + deep - size.z * 0.5),
                figure: Vec3::new(x + size.x + 0.35 + FIGURE_RADIUS, 0.0, z + deep - size.z + FIGURE_RADIUS),
                sign: Vec3::new(x + cell(size) * 0.5, 0.03, z + deep + SIGN_DEPTH * 0.5),
            });
            x += cell(size) + GAP;
        }
        z += deep + SIGN_DEPTH + GAP;
    }
    spots
}

/// `layout`'s spots for a page that grows toward -z with its first line at z = 0: each line
/// keeps its own arrangement (props behind, signs toward +z) and moves back past the lines
/// before it.
pub fn toward_back(spots: Vec<Spot>, sizes: &[Vec3], columns: usize) -> Vec<Spot> {
    let mut out = Vec::with_capacity(spots.len());
    let mut start = 0.0;
    for (line, sizes) in spots.chunks(columns.max(1)).zip(sizes.chunks(columns.max(1))) {
        let span = sizes.iter().map(|s| s.z).fold(0.0, f32::max) + SIGN_DEPTH + GAP;
        // This line occupied [start, start + span]; it moves to [-(start + span), -start].
        let shift = Vec3::new(0.0, 0.0, -(2.0 * start + span));
        out.extend(line.iter().map(|s| Spot { prop: s.prop + shift, figure: s.figure + shift, sign: s.sign + shift }));
        start += span;
    }
    out
}

impl Sim {
    /// Clears the catalogue's page and shows the one `step` away (wrapping round), placing its
    /// props, figures and signs at `origin`, turned by `rot`.
    pub(crate) fn catalogue_page(
        &mut self,
        region: Option<RegionKey>,
        origin: Vec3,
        rot: Quat,
        cat: &mut CatalogueDef,
        step: i32,
    ) {
        for id in std::mem::take(&mut cat.spawned) {
            self.despawn(id);
        }
        if let Some(region) = region {
            self.state.statics.remove_labels_from(region, &std::mem::take(&mut cat.signs));
        }
        let at = |local: Vec3| origin + rot * local;
        let label = |text: String, pos: Vec3, size: f32| Label {
            text,
            pos,
            size,
            color: Color::hex("#f1f4fa"),
            mode: LabelMode::Floor,
            facing: Default::default(),
        };
        let rows = rows();
        let per_page = cat.per_page.max(1) as usize;
        let pages = rows.len().div_ceil(per_page) as i32;
        let mut signs = Vec::new();
        if pages == 0 {
            signs.push(label(NO_ROWS.into(), at(Vec3::new(0.0, 0.03, -1.0)), 0.3));
        } else {
            cat.page = if cat.page < 0 && step < 0 { pages - 1 } else { (cat.page + step).rem_euclid(pages) };
            let first = cat.page as usize * per_page;
            let page = &rows[first..(first + per_page).min(rows.len())];
            let defs: Vec<Result<std::sync::Arc<PropDef>, String>> =
                page.iter().map(|row| props::resolve_ref(&row.name)).collect();
            let sizes: Vec<Vec3> =
                defs.iter().map(|def| def.as_ref().map_or(Vec3::splat(1.0), |def| def.compiled.bounds.size())).collect();
            let header = format!("PAGE {} / {pages} · ROWS {}-{} OF {}", cat.page + 1, first + 1, first + page.len(), rows.len());
            let toward = if cat.toward < 0 { -1.0 } else { 1.0 };
            signs.push(label(header, at(Vec3::new(0.0, 0.03, -toward)), 0.36));
            let spots = layout(&sizes, cat.columns as usize);
            let spots = if toward < 0.0 { toward_back(spots, &sizes, cat.columns as usize) } else { spots };
            for ((row, def), spot) in page.iter().zip(&defs).zip(spots) {
                let found = def.as_ref().ok().map(|def| findings(def));
                signs.push(label(sign(row, found), at(spot.sign), 0.16));
                if let Ok(def) = def {
                    let centre = def.compiled.bounds.center();
                    let pos = at(spot.prop - Vec3::new(centre.x, 0.0, centre.z));
                    let copy = PropInstance::new(&row.name, def.clone(), 1.0, false);
                    if let Ok(id) = copy.and_then(|copy| self.place_prop(copy, pos, rot, region)) {
                        // `~` keeps it out of a saved room (edit mode).
                        self.state.entities.get_mut(id).unwrap().name = format!("~{}", row.name);
                        cat.spawned.push(id);
                    }
                }
                cat.spawned.extend(self.catalogue_figure(at(spot.figure), region));
            }
        }
        if let Some(region) = region {
            for sign in &signs {
                self.state.statics.add_label_to(region, sign.clone());
            }
        }
        cat.signs = signs;
    }

    /// A grey capsule and a head, 1.8 m tall, standing at `feet`.
    fn catalogue_figure(&mut self, feet: Vec3, region: Option<RegionKey>) -> [EntityId; 2] {
        let body_top = FIGURE_HEIGHT - 2.0 * HEAD;
        let parts = [
            (
                feet + Vec3::Y * body_top * 0.5,
                Shape::Capsule { half_height: body_top * 0.5 - FIGURE_RADIUS, radius: FIGURE_RADIUS },
                "#8d949b",
            ),
            (feet + Vec3::Y * (FIGURE_HEIGHT - HEAD), Shape::Sphere { radius: HEAD }, "#a7aeb5"),
        ];
        parts.map(|(pos, shape, color)| {
            let mut spawn = Spawn::new("~figure", pos).visual(Visual::new(shape, Color::hex(color))).body(BodyKind::None);
            spawn.region = region;
            self.spawn(spawn)
        })
    }
}
