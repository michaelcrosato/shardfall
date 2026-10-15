//! Translation (plan §8.5): a normalised `Scene` into a format-2 prop, deterministic and
//! single-threaded per asset. A model more than 1% see-through (alpha mask or blend) is refused:
//! props are opaque.
//!
//! 1. `segment`: connected parts per (node, material), split by colour where a part clearly has
//!    two; face colours are 7-sample texture means in linear light.
//! 2. Equal shapes are fitted once.
//! 3. `frame` + `prims`: each piece in its own box frame, every candidate primitive scored by
//!    its three local silhouettes (holes filled: pieces are often open shells); the best one
//!    stays if its IoU is at least 0.85, else the piece goes into the asset's mesh part
//!    (`mesh`; mode `auto`). The plan's split before the mesh part was tried and dropped: on
//!    Kenney Mini Dungeon it made one more asset editable for 4% more tokens and 5x the time.
//! 4. `write`: angles snapped, cm to 0.1, the cheapest equivalent turn, mirror pairs and equal
//!    steps folded into `mirror` and `array`, names, a shared root colour; the engine's reader
//!    then reads the text and writes it canonically.
//! 5. The asset loop: while the six-view IoU is under 0.90 or ΔE over 6, the primitive piece
//!    with the largest error share moves into the mesh part.

pub mod frame;
pub mod mesh;
pub mod prims;
pub mod segment;
pub mod write;

use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use glam::DVec3;
use pav_core::issue;
use pav_core::props::compile::{self, CompiledProp};
use pav_core::props::mesh::MeshFile;
use pav_core::props::{Doc, lint};

use super::metrics::{self, Fidelity};
use super::normalize;
use super::raster::{self, Tri};
use super::scene::{Alpha, Scene};
use frame::Frame;
use prims::Cand;
use segment::Piece;

/// Keep a primitive whose local silhouettes reach this IoU.
pub const KEEP: f64 = 0.85;
/// The asset loop's goals.
pub const ASSET_IOU: f64 = 0.90;
pub const ASSET_DE: f64 = 6.0;

/// How pieces are translated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Primitives, with the mesh part for what they cannot fit.
    #[default]
    Auto,
    /// Primitives only.
    Prims,
    /// The whole model as one mesh part.
    Mesh,
}

impl Mode {
    pub fn parse(s: &str) -> Result<Mode> {
        Ok(match s {
            "auto" | "true" => Mode::Auto,
            "prims" => Mode::Prims,
            "mesh" => Mode::Mesh,
            _ => bail!("fit={s}: auto, prims or mesh"),
        })
    }
}

/// Where a translation comes from, for its `source`.
#[derive(Clone, Debug, Default)]
pub struct Source {
    pub library: Option<String>,
    pub title: Option<String>,
    pub license: Option<String>,
    pub url: Option<String>,
    /// The normalisation's words when `check` would not otherwise read the source the same way.
    pub norm: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub mode: Mode,
    /// The library pack the asset and its mesh belong to (`kenney_mini_dungeon`).
    pub pack: String,
    /// The asset's name (its file's leaf).
    pub name: String,
    pub source: Source,
    /// What retrieval reads (`assets find=`), from the catalog: a category, tags and a
    /// description.
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub description: String,
}

/// A translated asset.
pub struct Translation {
    /// The asset file's canonical text.
    pub text: String,
    pub doc: Doc,
    pub compiled: CompiledProp,
    /// Mesh files by mesh name (`<pack>/<name>`).
    pub meshes: Vec<(String, MeshFile)>,
    pub fidelity: Fidelity,
    /// Share of the source's area in the mesh part, %.
    pub mesh_pct: f64,
    pub chars: usize,
    /// Estimated o200k tokens of the asset file.
    pub tokens: usize,
    pub tier: &'static str,
    /// Authored nodes.
    pub parts: usize,
    /// `check` findings on the compiled parts.
    pub findings: Vec<String>,
}

/// What a piece became.
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)] // most pieces are primitives
pub enum Choice {
    /// A primitive: the candidate in the piece's frame.
    Prim {
        frame: Frame,
        cand: Cand,
    },
    Mesh,
}

/// A piece with its fit.
#[derive(Clone, Debug)]
pub struct Fitted {
    pub piece: Piece,
    pub choice: Choice,
    /// The best primitive's local IoU (1 for a whole-mesh translation).
    pub iou: f64,
}

/// A shape's key: its corners relative to its lowest, on a 0.1 mm grid.
fn shape_key(p: &Piece) -> (Vec<[i64; 3]>, DVec3) {
    let (lo, _) = p.bounds();
    let mut v: Vec<[i64; 3]> = p
        .tris
        .iter()
        .flat_map(|t| t.p)
        .map(|q| {
            let d = (q - lo) * 1e4;
            [d.x.round() as i64, d.y.round() as i64, d.z.round() as i64]
        })
        .collect();
    v.sort();
    v.dedup();
    (v, lo)
}

/// Fits one piece: its frame and the best candidate.
pub fn fit_piece(p: &Piece) -> (Frame, Cand) {
    let points: Vec<DVec3> = p.tris.iter().flat_map(|t| t.p).collect();
    let samples = metrics::surface_samples(&p.tris, 512);
    let fr = frame::frame(&points, &samples);
    let local = raster::transform(&p.tris, fr.rot.transpose(), -(fr.rot.transpose() * fr.centre));
    let mut cands = prims::candidates(&local, &fr);
    let best = cands.remove(0);
    (fr, best)
}

/// Fits every piece, equal shapes once: its best primitive, or (`auto`) the mesh part when that
/// falls short of `KEEP`.
pub fn fit_pieces(pieces: Vec<Piece>, mode: Mode) -> Vec<Fitted> {
    let mut seen: BTreeMap<Vec<[i64; 3]>, (DVec3, Frame, Cand)> = BTreeMap::new();
    pieces
        .into_iter()
        .map(|piece| {
            if mode == Mode::Mesh {
                return Fitted { piece, choice: Choice::Mesh, iou: 1.0 };
            }
            let (key, lo) = shape_key(&piece);
            let (fr, cand) = match seen.get(&key) {
                Some((lo0, fr, cand)) => (Frame { centre: fr.centre + (lo - *lo0), ..*fr }, cand.clone()),
                None => {
                    let (fr, cand) = fit_piece(&piece);
                    seen.insert(key, (lo, fr, cand.clone()));
                    (fr, cand)
                }
            };
            let iou = cand.iou;
            let choice = if mode == Mode::Auto && iou < KEEP { Choice::Mesh } else { Choice::Prim { frame: fr, cand } };
            Fitted { piece, choice, iou }
        })
        .collect()
}

/// Reads an asset text with the engine's reader and compiles it.
pub fn read_back(text: &str) -> Result<(Doc, CompiledProp)> {
    let doc = Doc::read(text).map_err(|e| anyhow!("the translation does not read back: {}", issue::render(&e)))?;
    let compiled = compile::compile(&doc, &|name| pav_core::props::get(name), 0)
        .map_err(|e| anyhow!("the translation does not compile: {}", issue::render(&e)))?;
    Ok((doc, compiled))
}

/// Builds the asset from fitted pieces and measures it against the source.
fn assemble(fitted: &[Fitted], o: &Options, source: &[Tri], total_area: f64, names: &[String]) -> Result<Translation> {
    let mesh_name = format!("{}/{}", o.pack, o.name);
    let mesh_tris: Vec<Tri> =
        fitted.iter().filter(|f| matches!(f.choice, Choice::Mesh)).flat_map(|f| f.piece.tris.iter().copied()).collect();
    let mut meshes = Vec::new();
    if !mesh_tris.is_empty() {
        let mut file = mesh::mesh_file(&o.name, &mesh_tris).ok_or_else(|| anyhow!("the mesh part has no faces"))?;
        // Open double-sided surfaces show their backs: drawn lit, not as cut-open caps.
        file.two_sided = fitted.iter().any(|f| matches!(f.choice, Choice::Mesh) && f.piece.two_sided);
        let mut src = serde_json::Map::new();
        if let Some(l) = &o.source.library {
            src.insert("library".into(), l.clone().into());
        }
        if let Some(t) = &o.source.title {
            src.insert("title".into(), t.clone().into());
        }
        if !src.is_empty() {
            file.source = Some(src);
        }
        pav_core::props::mesh::install(&mesh_name, file.clone()).map_err(|e| anyhow!(e))?;
        meshes.push((mesh_name.clone(), file));
    }
    let mesh_area: f64 = fitted.iter().filter(|f| matches!(f.choice, Choice::Mesh)).map(|f| f.piece.area).sum();
    let mesh_pct = if total_area > 0.0 { mesh_area / total_area * 100.0 } else { 0.0 };
    let body = write::document(fitted, o, (!mesh_tris.is_empty()).then_some(mesh_name.as_str()), names);
    // Measure the parts as the engine compiles them.
    let (_, compiled) = read_back(&body.to_string())?;
    let fit_tris = raster::prop_tris(&compiled);
    let fidelity = metrics::compare(source, &fit_tris);
    let mut value = body;
    value["fit"] = write::fit_field(&fidelity, mesh_pct);
    let (doc, compiled) = read_back(&value.to_string())?;
    let text = doc.text();
    let chars = text.chars().count();
    let whole_mesh = o.mode == Mode::Mesh;
    let tier = metrics::tier(&fidelity, chars, whole_mesh);
    let findings = lint::lint(&compiled, &lint::Options::default()).into_iter().map(|f| f.line).collect();
    let parts = count_nodes(&doc.parts);
    Ok(Translation {
        tokens: metrics::tokens(&text),
        text,
        doc,
        compiled,
        meshes,
        fidelity,
        mesh_pct,
        chars,
        tier,
        parts,
        findings,
    })
}

fn count_nodes(parts: &BTreeMap<String, pav_core::props::doc::Node>) -> usize {
    parts.values().map(|n| 1 + n.children().map_or(0, count_nodes)).sum()
}

/// Node names for the pieces when the model has several named nodes with triangles.
fn node_names(s: &Scene) -> Vec<String> {
    let used: std::collections::BTreeSet<usize> = s.meshes.iter().filter(|m| !m.tris.is_empty()).map(|m| m.node).collect();
    if used.len() <= 1 {
        return Vec::new();
    }
    s.nodes.iter().map(|n| n.name.clone()).collect()
}

/// A model whose see-through share is above this is refused.
pub const SEE_THROUGH: f64 = 0.01;

/// The share of a scene's area that is see-through: faces of an alpha-mask or blend material
/// whose alpha (base × texture × vertex colour, the face's mean) is under 0.99. Props are
/// opaque, and the metrics compare opaque silhouettes, so such models are refused rather than
/// translated with their holes and glass filled in.
pub fn see_through(s: &Scene) -> f64 {
    let (mut total, mut clear) = (0.0, 0.0);
    for (mi, m) in s.meshes.iter().enumerate() {
        let blended = m.material.and_then(|i| s.materials.get(i)).is_some_and(|mt| mt.alpha != Alpha::Opaque);
        for (ti, t) in m.tris.iter().enumerate() {
            let p = t.map(|i| DVec3::from(m.pos[i as usize].map(f64::from)));
            let area = (p[1] - p[0]).cross(p[2] - p[0]).length() * 0.5;
            total += area;
            if blended && normalize::face_colour(s, mi, ti)[3] < 0.99 {
                clear += area;
            }
        }
    }
    if total > 0.0 { clear / total } else { 0.0 }
}

/// Translates a normalised scene.
pub fn translate(s: &Scene, o: &Options) -> Result<Translation> {
    // A texture it cannot draw would render white, and the compare image (drawn from the same
    // scene) would not show it: refuse the model instead.
    let mut used = s.meshes.iter().filter_map(|m| m.material).filter_map(|i| s.materials.get(i));
    if let Some((m, fault)) = used.find_map(|m| m.tex_fault.as_ref().map(|fault| (m, fault))) {
        bail!("material {}: texture {fault}; a textured model translates only with its textures (it would draw white)", m.name);
    }
    let source = raster::scene_tris(s);
    if source.is_empty() {
        bail!("the model has no triangles");
    }
    let clear = see_through(s);
    if clear > SEE_THROUGH {
        bail!(
            "alpha: {:.0}% of its surface is see-through (alpha mask or blend); props are opaque, so it is not translated",
            (clear * 100.0).max(1.0)
        );
    }
    let pieces: Vec<Piece> = segment::pieces(s).into_iter().filter(|p| p.area > 0.0).collect();
    if pieces.is_empty() {
        bail!("the model has no surface (every triangle is flat)");
    }
    let total_area: f64 = pieces.iter().map(|p| p.area).sum();
    let names = node_names(s);
    let mut fitted = fit_pieces(pieces, o.mode);
    let mut t = assemble(&fitted, o, &source, total_area, &names)?;
    if o.mode != Mode::Auto {
        return Ok(t);
    }
    // The asset loop: while the silhouettes are off, the primitive piece with the largest
    // shape error (its local IoU short of 1, times its area) moves into the mesh part; while the
    // colours are off, the one with the largest colour error (its spread times its area).
    loop {
        let shape = t.fidelity.iou < ASSET_IOU;
        if !shape && t.fidelity.de <= ASSET_DE {
            break;
        }
        let worst = fitted
            .iter()
            .enumerate()
            .filter(|(_, f)| matches!(f.choice, Choice::Prim { .. }))
            .map(|(i, f)| (i, if shape { (1.0 - f.iou) * f.piece.area } else { f.piece.spread * f.piece.area }))
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
        let Some((i, _)) = worst else { break };
        fitted[i].choice = Choice::Mesh;
        t = assemble(&fitted, o, &source, total_area, &names)?;
    }
    Ok(t)
}
