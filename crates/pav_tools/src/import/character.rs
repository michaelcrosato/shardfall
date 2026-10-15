//! Rigged characters translated into figures (`pav_core::figure`, docs/ASSET_IMPORT.md,
//! "Characters"): the model's bones recognised by name (`skeleton`), each triangle given to the
//! segment of the joint that moves it most, each segment's triangles written as a mesh part in
//! the segment's own frame, the rig (joints, tails, the look's proportions) read from the bones,
//! and the body placed in its standing display pose.
//!
//! What is left out: meshes held in a hand slot (weapons, shields, books: the engine draws a
//! look's weapon), and bones that play no humanoid bone (IK helpers, slots), whose triangles go
//! to their nearest mapped ancestor. A model whose skeleton no table recognises is refused with
//! the bones it lacks.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use glam::{DMat3, DVec3, Vec3};
use pav_core::figure::{self, Proportions, RigFile, Seg, SegFile};
use pav_core::props::mesh::MeshFile;
use pav_core::puppet::PuppetDef;
use serde_json::{Map, Value, json};

use super::fit::frame::euler;
use super::fit::mesh::mesh_file_at;
use super::normalize::face_colour;
use super::raster::Tri;
use super::scene::Scene;
use super::skeleton;

/// Triangles a figure is cut down to at most (vertex clustering on a coarser grid until it
/// fits), so its files stay small enough to commit: about 170 KB of mesh text. KayKit's
/// adventurers (4,100-4,700 triangles, about 100 KB) are written whole: clustering them blurs
/// helmets and moves colours onto the wrong faces.
pub const BUDGET: usize = 8_000;

/// What a character translation needs besides the model.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// The library pack and the figure's name (`kaykit_adventurers`, `knight`).
    pub pack: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    /// The prop's `source`: library, license, title, sha256 (as format 2 writes them).
    pub source: Map<String, Value>,
    /// Triangles at most (0: `BUDGET`).
    pub budget: usize,
}

/// A translated figure: its rig, its body prop's text and its meshes.
#[derive(Clone, Debug)]
pub struct Figure {
    pub rig: RigFile,
    /// The body prop's document (`write` makes it canonical once its meshes are installed).
    pub body: Value,
    /// Each segment's mesh, named `<pack>/<name>_<segment>`.
    pub meshes: Vec<(String, MeshFile)>,
    /// What the reply says: the table, triangles before and after, the cell, folded bones,
    /// meshes left out.
    pub report: Value,
}

/// Whether a scene holds a rig the tables recognise.
pub fn recognised(s: &Scene) -> Option<skeleton::Recognized> {
    skeleton::recognize(s.nodes.iter().map(|n| n.name.as_str())).filter(|r| r.missing.is_empty())
}

fn cm3(v: Vec3) -> [f32; 3] {
    (v * 100.0).to_array().map(|c| (c * 10.0).round() / 10.0)
}

fn round(x: f32, k: f32) -> f32 {
    (x * k).round() / k
}

/// Translates a skinned model, read and normalised (`asset_import::read`), into a figure.
pub fn translate(s: &Scene, o: &Options) -> Result<Figure> {
    let recognized = skeleton::recognize(s.nodes.iter().map(|n| n.name.as_str())).ok_or_else(|| {
        anyhow!("rigged, but no bone-name table knows its skeleton (vrm, unity, mixamo, unreal, rigify, kaykit...)")
    })?;
    if !recognized.missing.is_empty() {
        bail!("its skeleton ({} names) lacks {}", recognized.table, recognized.missing.join(", "));
    }
    // Each node's segment: its own VRM bone's, else its nearest mapped ancestor's.
    let node_of = |bone: &str| s.nodes.iter().position(|n| n.name == bone);
    let mut seg_of_node: Vec<Option<Seg>> = vec![None; s.nodes.len()];
    let mut joint_node: BTreeMap<Seg, usize> = BTreeMap::new();
    for (vrm, bone) in &recognized.bones {
        let (Some(seg), Some(n)) = (Seg::of_vrm(vrm), node_of(bone)) else { continue };
        seg_of_node[n] = Some(seg);
        if seg.vrm() == vrm {
            joint_node.insert(seg, n);
        }
    }
    let mapped: Vec<Option<Seg>> = seg_of_node.clone();
    let mut folded: BTreeMap<String, Seg> = BTreeMap::new();
    for i in 0..s.nodes.len() {
        if mapped[i].is_some() {
            continue;
        }
        let mut p = s.nodes[i].parent;
        while let Some(q) = p {
            if let Some(seg) = mapped[q] {
                seg_of_node[i] = Some(seg);
                break;
            }
            p = s.nodes[q].parent;
        }
    }
    if !joint_node.contains_key(&Seg::Chest) {
        // No chest: the spine carries what rides with it.
        for seg in seg_of_node.iter_mut().filter(|s| **s == Some(Seg::Chest)) {
            *seg = Some(Seg::Spine);
        }
    }
    // Triangles, by the segment that moves them most; rigid meshes by their node's.
    let mut by_seg: BTreeMap<Seg, Vec<Tri>> = BTreeMap::new();
    let mut left_out: Vec<String> = Vec::new();
    let (mut orphans, mut source_tris) = (0usize, 0usize);
    for (mi, m) in s.meshes.iter().enumerate() {
        let rigid = m.joints.is_empty();
        let rigid_seg = seg_of_node[m.node];
        if rigid {
            let held = matches!(rigid_seg, Some(Seg::HandL | Seg::HandR) | None);
            if held {
                let name = s.nodes[m.node].name.clone();
                if !left_out.contains(&name) {
                    left_out.push(name);
                }
                continue;
            }
        }
        for (ti, t) in m.tris.iter().enumerate() {
            source_tris += 1;
            let seg = if rigid {
                rigid_seg
            } else {
                let mut w: BTreeMap<Seg, f32> = BTreeMap::new();
                for &v in t {
                    for k in 0..4 {
                        let (j, wt) = (m.joints[v as usize][k], m.weights[v as usize][k]);
                        if let Some(seg) = seg_of_node.get(j as usize).copied().flatten().filter(|_| wt > 0.0) {
                            *w.entry(seg).or_default() += wt;
                        }
                    }
                }
                w.into_iter().max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0))).map(|(seg, _)| seg)
            };
            let seg = seg.unwrap_or_else(|| {
                orphans += 1;
                Seg::Hips
            });
            let c = face_colour(s, mi, ti);
            let p = t.map(|i| DVec3::from(m.pos[i as usize].map(f64::from)));
            by_seg.entry(seg).or_default().push(Tri { p, c: [c[0], c[1], c[2]] });
        }
    }
    for (i, n) in s.nodes.iter().enumerate() {
        let Some(seg) = seg_of_node[i].filter(|_| mapped[i].is_none() && !left_out.contains(&n.name)) else { continue };
        // Only bones that move triangles, and meshes kept, are worth naming.
        let weighted = s
            .meshes
            .iter()
            .any(|m| m.joints.iter().zip(&m.weights).any(|(j, w)| (0..4).any(|k| j[k] as usize == i && w[k] > 0.0)));
        let kept = s.meshes.iter().any(|m| m.joints.is_empty() && m.node == i);
        if (weighted || kept) && by_seg.contains_key(&seg) {
            folded.insert(n.name.clone(), seg);
        }
    }
    // Joints at rest, and each bone's far end.
    let at = |n: usize| -> Vec3 { s.nodes[n].world.w_axis.truncate().as_vec3() };
    let joint = |seg: Seg| joint_node.get(&seg).map(|&n| at(n));
    let pts = |seg: Seg| -> Vec<Vec3> {
        by_seg.get(&seg).map(|ts| ts.iter().flat_map(|t| t.p.map(|p| p.as_vec3())).collect()).unwrap_or_default()
    };
    let far = |seg: Seg, from: Vec3, dir: Vec3| -> Vec3 {
        let reach = pts(seg).iter().map(|p| (*p - from).dot(dir)).fold(0.0f32, f32::max);
        from + dir * reach.max(0.02)
    };
    // Facing: the toes ahead of the ankles.
    let ankle = joint(Seg::FootL).unwrap_or_default();
    let feet: Vec<Vec3> = [Seg::FootL, Seg::FootR].iter().flat_map(|s| pts(*s)).collect();
    if !feet.is_empty() && feet.iter().map(|p| p.z - ankle.z).sum::<f32>() / (feet.len() as f32) < 0.0 {
        bail!("it faces -z (its toes point back): import it with yaw=180");
    }
    let mut segments: BTreeMap<String, SegFile> = BTreeMap::new();
    for seg in Seg::ALL {
        let Some(j) = joint(seg) else { continue };
        let child = |a: Seg, b: Seg| joint(a).or_else(|| joint(b));
        let tail = match seg {
            Seg::Hips => joint(Seg::Spine).unwrap_or(j + Vec3::Y * 0.1),
            Seg::Spine => child(Seg::Chest, Seg::Head).unwrap_or(j + Vec3::Y * 0.1),
            Seg::Chest => joint(Seg::Head).unwrap_or(j + Vec3::Y * 0.1),
            Seg::Head => far(seg, j, Vec3::Y),
            Seg::UpperArmL => joint(Seg::LowerArmL).unwrap_or(j),
            Seg::UpperArmR => joint(Seg::LowerArmR).unwrap_or(j),
            Seg::LowerArmL => joint(Seg::HandL).unwrap_or(j),
            Seg::LowerArmR => joint(Seg::HandR).unwrap_or(j),
            Seg::UpperLegL => joint(Seg::LowerLegL).unwrap_or(j),
            Seg::UpperLegR => joint(Seg::LowerLegR).unwrap_or(j),
            Seg::LowerLegL => joint(Seg::FootL).unwrap_or(j),
            Seg::LowerLegR => joint(Seg::FootR).unwrap_or(j),
            Seg::HandL | Seg::HandR => {
                let elbow = joint(if seg == Seg::HandL { Seg::LowerArmL } else { Seg::LowerArmR }).unwrap_or(j);
                far(seg, j, (j - elbow).normalize_or(Vec3::NEG_Y))
            }
            Seg::FootL | Seg::FootR => {
                let tip = far(seg, j, Vec3::Z);
                let sole = pts(seg).iter().map(|p| p.y).fold(j.y, f32::min);
                Vec3::new(j.x, sole, tip.z)
            }
        };
        let mut bones: Vec<String> = recognized
            .bones
            .iter()
            .filter(|(vrm, _)| {
                Seg::of_vrm(vrm) == Some(seg)
                    || (seg == Seg::Spine && !joint_node.contains_key(&Seg::Chest) && Seg::of_vrm(vrm) == Some(Seg::Chest))
            })
            .map(|(_, b)| b.clone())
            .collect();
        let own = &s.nodes[joint_node[&seg]].name;
        bones.sort_by_key(|b| (b != own, b.clone()));
        let tris = by_seg.get(&seg).map_or(0, Vec::len) as u32;
        segments.insert(seg.name().into(), SegFile { bones, joint: cm3(j), tail: cm3(tail), tris });
    }
    // The look, from the bones (m).
    let len = |a: Seg, b: Seg| -> f32 { joint(a).zip(joint(b)).map_or(0.0, |(a, b)| (b - a).length()) };
    let (thigh, shin) = (
        (len(Seg::UpperLegL, Seg::LowerLegL) + len(Seg::UpperLegR, Seg::LowerLegR)) * 0.5,
        (len(Seg::LowerLegL, Seg::FootL) + len(Seg::LowerLegR, Seg::FootR)) * 0.5,
    );
    let (upper, fore) = (
        (len(Seg::UpperArmL, Seg::LowerArmL) + len(Seg::UpperArmR, Seg::LowerArmR)) * 0.5,
        (len(Seg::LowerArmL, Seg::HandL) + len(Seg::LowerArmR, Seg::HandR)) * 0.5,
    );
    let (hl, hr) = (joint(Seg::UpperLegL).unwrap_or_default(), joint(Seg::UpperLegR).unwrap_or_default());
    let (sl, sr) = (joint(Seg::UpperArmL).unwrap_or_default(), joint(Seg::UpperArmR).unwrap_or_default());
    let pelvis = (hl + hr) * 0.5;
    let shoulders = (sl + sr) * 0.5;
    let head_top = segments.get("head").map_or(shoulders.y + 0.3, |h| h.tail[1] * 0.01);
    let half_width = |seg: Seg| -> f32 {
        let p = pts(seg);
        if p.is_empty() {
            return 0.0;
        }
        let (lo, hi) = p.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v.x), hi.max(v.x)));
        (hi - lo) * 0.5
    };
    let limb_radius = {
        let r: Vec<f32> = [Seg::LowerArmL, Seg::LowerArmR, Seg::LowerLegL, Seg::LowerLegR]
            .iter()
            .filter_map(|&seg| {
                let (j, t) = (joint(seg)?, segments.get(seg.name()).map(|f| Vec3::from(f.tail) * 0.01)?);
                let d = (t - j).normalize_or(Vec3::NEG_Y);
                let p = pts(seg);
                (!p.is_empty()).then(|| p.iter().map(|v| ((*v - j) - d * (*v - j).dot(d)).length()).sum::<f32>() / p.len() as f32)
            })
            .collect();
        if r.is_empty() { 0.05 } else { r.iter().sum::<f32>() / r.len() as f32 }
    };
    let r3 = |x: f32| round(x, 1000.0);
    let look = Proportions {
        scale: 1.0,
        leg_length: r3(thigh + shin),
        arm_length: r3(upper + fore),
        torso_length: r3(shoulders.y + 0.05 - pelvis.y),
        torso_radius: r3((half_width(Seg::Spine).max(half_width(Seg::Hips)) * 0.8).clamp(0.08, 0.45)),
        hip_width: r3((hl.x - hr.x).abs() * 0.5),
        shoulder_width: r3((sl.x - sr.x).abs() * 0.5),
        head_radius: r3(((head_top - (shoulders.y + 0.05)) / 2.5).clamp(0.05, 0.5)),
        limb_radius: r3(limb_radius.clamp(0.02, 0.2)),
    };
    let body = format!("LIB/{}/{}", o.pack, o.name);
    let mut rig = RigFile {
        format: 1,
        name: o.name.clone(),
        body: body.clone(),
        unit: "cm".into(),
        table: recognized.table.to_string(),
        look,
        segments,
        folded: folded.iter().map(|(b, s)| format!("{b} > {}", s.name())).collect(),
        left_out: left_out.clone(),
    };
    // Each segment's triangles in its own frame (from the joints as written).
    let mut local: BTreeMap<Seg, Vec<Tri>> = BTreeMap::new();
    for (seg, tris) in &by_seg {
        let Some((rot, j)) = figure::rest_frame(&rig, *seg) else { continue };
        let (inv, j) = (rot.inverse().as_dquat(), j.as_dvec3());
        local.insert(*seg, tris.iter().map(|t| Tri { p: t.p.map(|p| inv * (p - j)), c: t.c }).collect());
    }
    // Cut to the budget by clustering corners on one cell for the whole body.
    let budget = if o.budget == 0 { BUDGET } else { o.budget };
    let mesh_name = |seg: Seg| format!("{}/{}_{}", o.pack, o.name, seg.name());
    let build = |cell: f64| -> Vec<(Seg, MeshFile)> {
        local
            .iter()
            .filter_map(|(seg, tris)| {
                let leaf = format!("{}_{}", o.name, seg.name());
                mesh_file_at(&leaf, tris, cell).map(|m| (*seg, m))
            })
            .collect()
    };
    let mut cell = 1.0;
    let mut meshes = build(cell);
    while meshes.iter().map(|(_, m)| m.f.len()).sum::<usize>() > budget && cell < 200.0 {
        cell *= 1.15;
        meshes = build(cell);
    }
    for (seg, m) in &meshes {
        if let Some(f) = rig.segments.get_mut(seg.name()) {
            f.tris = m.f.len() as u32;
        }
    }
    // Segments the cut left without a triangle go.
    let drawn: Vec<Seg> = meshes.iter().map(|(s, _)| *s).collect();
    for seg in Seg::ALL {
        if !drawn.contains(&seg) {
            if let Some(f) = rig.segments.get_mut(seg.name()) {
                f.tris = 0;
            }
        }
    }
    let mut source = o.source.clone();
    let source_line = json!({"library": source.get("library"), "title": source.get("title")});
    let meshes: Vec<(String, MeshFile)> = meshes
        .into_iter()
        .map(|(seg, mut m)| {
            m.source = source_line
                .as_object()
                .map(|o| o.iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k.clone(), v.clone())).collect());
            (mesh_name(seg), m)
        })
        .collect();
    // The display pose: the body standing, each segment placed by the posing code itself.
    let def = figure::fit(&rig, &PuppetDef::default());
    let sk = pav_core::clips::skel(&def, &figure::display_key(), false);
    let lows: BTreeMap<Seg, Vec<Vec3>> = meshes
        .iter()
        .filter_map(|(name, m)| {
            let seg = Seg::parse(name.rsplit_once(&format!("{}_", o.name)).map_or("", |(_, s)| s))?;
            let points: Vec<Vec3> = m.v.iter().map(|v| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32) * 0.001).collect();
            Some((seg, figure::extremes(&points)))
        })
        .collect();
    let poses = figure::pose(&rig, &def, &sk, &|s| lows.get(&s).cloned().unwrap_or_default());
    let mut parts = Map::new();
    for p in &poses {
        if !lows.contains_key(&p.seg) {
            continue;
        }
        let e = euler(DMat3::from_quat(p.rot.normalize().as_dquat()));
        let r1 = |x: f64| (x * 10.0).round() / 10.0;
        let mut node = Map::new();
        node.insert("mesh".into(), json!(mesh_name(p.seg)));
        node.insert("pos".into(), json!(cm3(p.pos)));
        for (k, v) in ["yaw", "pitch", "roll"].iter().zip(e) {
            if r1(v) != 0.0 {
                node.insert(k.to_string(), json!(r1(v)));
            }
        }
        parts.insert(p.seg.name().into(), Value::Object(node));
    }
    source.retain(|_, v| !v.is_null());
    let doc = json!({
        "format": 2,
        "name": o.name,
        "unit": "cm",
        "description": o.description,
        "tags": o.tags,
        "category": "character",
        "source": source,
        "fit": {"mesh_pct": 100},
        "parts": parts,
    });
    let tris_out: usize = meshes.iter().map(|(_, m)| m.f.len()).sum();
    rig.left_out.sort();
    let report = json!({
        "figure": body,
        "table": rig.table,
        "segments": meshes.len(),
        "tris": {"source": source_tris, "written": tris_out, "cell_mm": round(cell as f32, 10.0)},
        "look": rig.look,
        "folded": rig.folded,
        "left_out": rig.left_out,
        "orphans": orphans,
    });
    Ok(Figure { rig, body: doc, meshes, report })
}
