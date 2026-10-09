//! Animation libraries in glTF binary form (.glb): a humanoid rig, usually its mesh, and its
//! clips. Every clip is sampled at a fixed rate, run through forward kinematics and taken as the
//! 36 body points the encoder measures, in the rig's own frame (forward from the heels toward
//! the toes, right toward the right hip, up), centred on the root on the ground.
//!
//! A port of the glTF half of my-3D2dge's tools/anim-import.mjs. Its rigs: the Rigify `DEF-`
//! deform bones (Quaternius' Universal Animation Library) and Unreal-style names (`pelvis`,
//! `spine_01`, `upperarm_l`: the Library 2, Mesh2Motion's humans), in any case. A rig is a short
//! table of bone names (`bones`): another skeleton is one more table.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use super::readable::{Cap, P, POINTS, hypot, js_round};
use libm::{acos, sin};

const MM: f64 = 1000.0;

/// Where a body point sits on its bone.
#[derive(Clone, Copy, Debug, PartialEq)]
enum At {
    /// The bone's joint.
    Joint,
    /// The bone's far end: as far along it as its mesh reaches (else its first child).
    Tip,
    /// 12 cm in front of the joint, the way the body faces at rest.
    Fwd,
}

/// The rigs this reader knows, by a bone only each has.
const RIGS: [(&str, &str); 2] = [("rigify", "DEF-hips"), ("unreal", "spine_01")];

/// Which bone gives each body point (in `POINTS` order).
fn bones(rig: &str) -> Vec<(String, At)> {
    let core: [&str; 6] = match rig {
        "rigify" => ["DEF-hips", "DEF-spine.001", "DEF-spine.002", "DEF-spine.003", "DEF-neck", "DEF-head"],
        _ => ["pelvis", "spine_01", "spine_02", "spine_03", "neck_01", "Head"],
    };
    let side = |s: char| -> Vec<(String, At)> {
        match rig {
            "rigify" => {
                let b = |n: &str| (format!("DEF-{n}.{s}"), At::Joint);
                vec![
                    b("shoulder"),
                    b("upper_arm"),
                    b("forearm"),
                    b("hand"),
                    b("f_index.01"),
                    b("f_middle.01"),
                    b("f_pinky.01"),
                    b("f_middle.02"),
                    b("thigh"),
                    b("shin"),
                    b("foot"),
                    b("toe"),
                    (format!("DEF-toe.{s}"), At::Tip),
                ]
            }
            _ => {
                let x = s.to_ascii_lowercase();
                let b = |n: &str| (format!("{n}_{x}"), At::Joint);
                vec![
                    b("clavicle"),
                    b("upperarm"),
                    b("lowerarm"),
                    b("hand"),
                    b("index_01"),
                    b("middle_01"),
                    b("pinky_01"),
                    b("middle_02"),
                    b("thigh"),
                    b("calf"),
                    b("foot"),
                    b("ball"),
                    b("ball_leaf"),
                ]
            }
        }
    };
    let j = |n: &str| (n.to_string(), At::Joint);
    let mut out = vec![j(core[0]), j(core[1]), j(core[2]), j(core[3]), j(core[4]), j(core[5])];
    out.push((core[5].to_string(), At::Tip));
    out.push((core[5].to_string(), At::Fwd));
    out.push((core[3].to_string(), At::Fwd));
    out.push((core[0].to_string(), At::Fwd));
    out.extend(side('L'));
    out.extend(side('R'));
    debug_assert_eq!(out.len(), POINTS.len());
    out
}

/// The root bone's name (else the first node's topmost parent).
const ROOT: &str = "root";

/* ---- column-major 4x4 matrices, quaternions [x, y, z, w] ---- */
type M4 = [f64; 16];
type V = [f64; 3];

fn mat(t: V, q: [f64; 4], s: V) -> M4 {
    let [x, y, z, w] = q;
    let (xx, yy, zz, xy, xz, yz, wx, wy, wz) = (x * x, y * y, z * z, x * y, x * z, y * z, w * x, w * y, w * z);
    [
        (1.0 - 2.0 * (yy + zz)) * s[0],
        2.0 * (xy + wz) * s[0],
        2.0 * (xz - wy) * s[0],
        0.0,
        2.0 * (xy - wz) * s[1],
        (1.0 - 2.0 * (xx + zz)) * s[1],
        2.0 * (yz + wx) * s[1],
        0.0,
        2.0 * (xz + wy) * s[2],
        2.0 * (yz - wx) * s[2],
        (1.0 - 2.0 * (xx + yy)) * s[2],
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ]
}
fn mul(a: &M4, b: &M4) -> M4 {
    let mut o = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            o[c * 4 + r] = a[r] * b[c * 4] + a[4 + r] * b[c * 4 + 1] + a[8 + r] * b[c * 4 + 2] + a[12 + r] * b[c * 4 + 3];
        }
    }
    o
}
fn xf(m: &M4, p: V) -> V {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}
fn slerp(a: &[f64], b: &[f64], t: f64) -> Vec<f64> {
    let mut d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let mut s = 1.0;
    if d < 0.0 {
        d = -d;
        s = -1.0;
    }
    if d > 0.9995 {
        let o: Vec<f64> = (0..4).map(|i| a[i] + (b[i] * s - a[i]) * t).collect();
        let l = hypot(&o);
        return o.iter().map(|v| v / l).collect();
    }
    let th = acos(d);
    let k0 = sin((1.0 - t) * th) / sin(th);
    let k1 = sin(t * th) / sin(th) * s;
    (0..4).map(|i| a[i] * k0 + b[i] * k1).collect()
}
/// A percentile of a list (0 when empty).
fn pct(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    s[(s.len() - 1).min((p * s.len() as f64).floor() as usize)]
}

struct Gltf {
    json: Value,
    buf: Vec<u8>,
    bin: usize,
}

impl Gltf {
    fn open(path: &Path) -> Result<Gltf> {
        let buf = std::fs::read(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        let u32le = |o: usize| u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]) as usize;
        if buf.len() < 20 || u32le(0) != 0x4654_6c67 {
            bail!("{} is not a binary glTF (.glb) file", path.display());
        }
        let jl = u32le(12);
        let json: Value = serde_json::from_slice(buf.get(20..20 + jl).ok_or_else(|| anyhow!("truncated .glb"))?)?;
        Ok(Gltf { json, buf, bin: 20 + jl + 8 })
    }

    /// An accessor's elements, each as its components.
    fn accessor(&self, i: &Value) -> Result<Vec<Vec<f64>>> {
        let a = &self.json["accessors"][i.as_u64().ok_or_else(|| anyhow!("an accessor index is missing"))? as usize];
        let n = match a["type"].as_str().unwrap_or("") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            t => bail!("accessor type {t} is not read"),
        };
        let count = a["count"].as_u64().unwrap_or(0) as usize;
        let ct = a["componentType"].as_u64().unwrap_or(0);
        let bytes = match ct {
            5126 | 5125 => 4,
            5123 | 5122 => 2,
            5121 | 5120 => 1,
            _ => bail!("component type {ct} is not read"),
        };
        let norm = if a["normalized"].as_bool().unwrap_or(false) {
            match ct {
                5123 => 65535.0,
                5121 => 255.0,
                5122 => 32767.0,
                5120 => 127.0,
                _ => 1.0,
            }
        } else {
            1.0
        };
        let Some(bvi) = a["bufferView"].as_u64() else {
            return Ok(vec![vec![0.0; n]; count]);
        };
        let bv = &self.json["bufferViews"][bvi as usize];
        let base = self.bin + bv["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let stride = bv["byteStride"].as_u64().map(|s| s as usize).filter(|&s| s > 0).unwrap_or(n * bytes);
        let b = &self.buf;
        let need = base + count.saturating_sub(1) * stride + n * bytes;
        if count > 0 && need > b.len() {
            bail!("an accessor reads past the end of the file");
        }
        let read = |o: usize| -> f64 {
            match ct {
                5126 => f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64,
                5125 => u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64,
                5123 => u16::from_le_bytes([b[o], b[o + 1]]) as f64,
                5122 => i16::from_le_bytes([b[o], b[o + 1]]) as f64,
                5121 => b[o] as f64,
                _ => b[o] as i8 as f64,
            }
        };
        Ok((0..count).map(|k| (0..n).map(|c| read(base + k * stride + c * bytes) / norm).collect()).collect())
    }
}

/// Bone names, matched whatever their case (Quaternius' Library 2 says `Head`, Mesh2Motion's
/// copy of the rig `head`).
struct Names {
    exact: HashMap<String, usize>,
    lower: HashMap<String, usize>,
}

impl Names {
    fn new(nodes: &[Value]) -> Names {
        let (mut exact, mut order) = (HashMap::new(), Vec::new());
        for (i, n) in nodes.iter().enumerate() {
            if let Some(name) = n["name"].as_str() {
                if exact.insert(name.to_string(), i).is_none() {
                    order.push(name.to_string());
                }
            }
        }
        let mut lower = HashMap::new();
        for k in &order {
            lower.entry(k.to_lowercase()).or_insert(exact[k]);
        }
        Names { exact, lower }
    }
    fn get(&self, k: &str) -> Option<usize> {
        self.exact.get(k).or_else(|| self.lower.get(&k.to_lowercase())).copied()
    }
}

/// One library read from a .glb: its rig and its clips as captured body points.
pub struct Library {
    pub rig: &'static str,
    pub clips: Vec<Cap>,
    /// Things worth knowing about the file (no skinned mesh, unread interpolation).
    pub notes: Vec<String>,
}

#[derive(Clone)]
struct Trs {
    t: V,
    r: [f64; 4],
    s: V,
}

fn arr<const N: usize>(v: &Value, d: [f64; N]) -> [f64; N] {
    match v.as_array() {
        Some(a) if a.len() == N => std::array::from_fn(|i| a[i].as_f64().unwrap_or(d[i])),
        _ => d,
    }
}

/// The file's clips, bones and the rig it is recognised as (nothing is converted).
pub fn list(path: &Path) -> Result<Value> {
    let g = Gltf::open(path)?;
    let nodes = g.json["nodes"].as_array().cloned().unwrap_or_default();
    let names = Names::new(&nodes);
    let rig = RIGS.iter().find(|(_, test)| names.get(test).is_some()).map(|(r, _)| *r);
    Ok(json!({
        "file": path.display().to_string(),
        "rig": rig.unwrap_or("not recognised"),
        "clips": g.json["animations"].as_array().map(|a| a.iter().map(|x| x["name"].clone()).collect::<Vec<_>>()).unwrap_or_default(),
        "bones": nodes.iter().filter_map(|n| n["name"].as_str()).collect::<Vec<_>>(),
    }))
}

/// Reads a library: every clip sampled at `fps` as body points (mm, rig frame), with the root's
/// travel.
pub fn read(path: &Path, fps: f64) -> Result<Library> {
    let file = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let g = Gltf::open(path)?;
    let nodes = g.json["nodes"].as_array().cloned().unwrap_or_default();
    let mut parent = vec![usize::MAX; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        for c in n["children"].as_array().into_iter().flatten() {
            if let Some(c) = c.as_u64().filter(|&c| (c as usize) < nodes.len()) {
                parent[c as usize] = i;
            }
        }
    }
    let names = Names::new(&nodes);
    let rig = RIGS
        .iter()
        .find(|(_, test)| names.get(test).is_some())
        .map(|(r, _)| *r)
        .ok_or_else(|| anyhow!("{file}: the rig is not one the importer knows (Rigify DEF- bones or Unreal-style names)"))?;
    let map = bones(rig);
    let mut missing: Vec<&str> = map.iter().filter(|(b, _)| names.get(b).is_none()).map(|(b, _)| b.as_str()).collect();
    missing.dedup();
    if !missing.is_empty() {
        bail!("{file} lacks bones the {rig} map needs: {}", missing.join(", "));
    }
    let node = |b: &str| names.get(b).unwrap();
    let rest: Vec<Trs> = nodes
        .iter()
        .map(|n| {
            let m = arr::<16>(&n["matrix"], [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
            if n.get("matrix").is_some() && n.get("translation").is_none() && n.get("rotation").is_none() {
                // A node placed by a matrix: its translation, rotation and scale.
                let (s, r, t) = glam::DMat4::from_cols_array(&m).to_scale_rotation_translation();
                return Trs { t: t.to_array(), r: r.to_array(), s: s.to_array() };
            }
            Trs {
                t: arr(&n["translation"], [0.0; 3]),
                r: arr(&n["rotation"], [0.0, 0.0, 0.0, 1.0]),
                s: arr(&n["scale"], [1.0; 3]),
            }
        })
        .collect();
    // Parents before children.
    let mut order = Vec::with_capacity(nodes.len());
    let mut seen = vec![false; nodes.len()];
    for i in 0..nodes.len() {
        let mut chain = vec![i];
        while let Some(&p) = chain.last().map(|&c| &parent[c]) {
            if p == usize::MAX || seen[p] || chain.contains(&p) {
                break;
            }
            chain.push(p);
        }
        for &c in chain.iter().rev() {
            if !seen[c] {
                seen[c] = true;
                order.push(c);
            }
        }
    }
    let world = |trs: &[Trs]| -> Vec<M4> {
        let mut w = vec![[0.0; 16]; trs.len()];
        for &i in &order {
            let l = mat(trs[i].t, trs[i].r, trs[i].s);
            w[i] = if parent[i] != usize::MAX { mul(&w[parent[i]], &l) } else { l };
        }
        w
    };

    // The bind pose's mesh, each vertex in the frame of the bone that moves it most: it gives
    // the tip bones (the head, Rigify's toes) their length. A file without a skinned mesh uses
    // the bones alone.
    let mut notes = Vec::new();
    let mut ys: HashMap<usize, Vec<f64>> = HashMap::new();
    let skin = &g.json["skins"][0];
    let mesh_node = nodes.iter().position(|n| n.get("mesh").is_some() && n.get("skin").is_some());
    match (skin.is_object(), mesh_node) {
        (true, Some(mn)) => {
            let joints: Vec<usize> =
                skin["joints"].as_array().into_iter().flatten().filter_map(|j| j.as_u64()).map(|j| j as usize).collect();
            let ibm = match skin.get("inverseBindMatrices") {
                Some(i) => g.accessor(i)?,
                None => vec![mat([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]).to_vec(); joints.len()],
            };
            let mesh = &g.json["meshes"][nodes[mn]["mesh"].as_u64().unwrap_or(0) as usize];
            for prim in mesh["primitives"].as_array().into_iter().flatten() {
                let at = &prim["attributes"];
                let (Some(pos), Some(jn), Some(wt)) = (at.get("POSITION"), at.get("JOINTS_0"), at.get("WEIGHTS_0")) else {
                    continue;
                };
                let (pos, jn, wt) = (g.accessor(pos)?, g.accessor(jn)?, g.accessor(wt)?);
                for v in 0..pos.len() {
                    let mut best = 0;
                    for k in 1..4 {
                        if wt[v][k] > wt[v][best] {
                            best = k;
                        }
                    }
                    let ji = jn[v][best] as usize;
                    let (Some(&bone), Some(m)) = (joints.get(ji), ibm.get(ji)) else { continue };
                    let m: M4 = std::array::from_fn(|i| m[i]);
                    ys.entry(bone).or_default().push(xf(&m, [pos[v][0], pos[v][1], pos[v][2]])[1]);
                }
            }
        }
        _ => notes.push(format!("{file}: no skinned mesh: the body is measured from the bones alone")),
    }
    let bind = world(&rest);
    let origin = |w: &[M4], i: usize| xf(&w[i], [0.0; 3]);
    let bone_len = |i: usize| -> f64 {
        match nodes[i]["children"].as_array().and_then(|k| k.first()).and_then(|c| c.as_u64()) {
            Some(c) => {
                let (a, b) = (origin(&bind, i), origin(&bind, c as usize));
                hypot(&[b[0] - a[0], b[1] - a[1], b[2] - a[2]])
            }
            None => pct(ys.get(&i).map_or(&[][..], |v| v), 0.98),
        }
    };
    let mut tip: HashMap<usize, f64> = HashMap::new();
    for (b, at) in &map {
        if *at == At::Tip {
            let i = node(b);
            let t = pct(ys.get(&i).map_or(&[][..], |v| v), 0.98);
            tip.insert(i, if t != 0.0 { t } else { bone_len(i) });
        }
    }
    // The rig's frame from the rest pose: up is +y, forward from the heel toward the toes,
    // right toward the right hip.
    let mut fwd_local: HashMap<usize, V> = HashMap::new();
    let pos = |w: &[M4], i: usize, at: At, fwd_local: &HashMap<usize, V>| -> V {
        match at {
            At::Joint => xf(&w[i], [0.0; 3]),
            At::Tip => xf(&w[i], [0.0, tip[&i], 0.0]),
            At::Fwd => xf(&w[i], fwd_local[&i]),
        }
    };
    let (ankle_l, toe_l) =
        (&map[super::readable::side(0, super::readable::ANKLE)], &map[super::readable::side(0, super::readable::TOE)]);
    let fwd0 = {
        let a = pos(&bind, node(&ankle_l.0), ankle_l.1, &fwd_local);
        let b = pos(&bind, node(&toe_l.0), toe_l.1, &fwd_local);
        let v = [b[0] - a[0], 0.0, b[2] - a[2]];
        let l = hypot(&v);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let mut right = [-(fwd0[1] * 0.0 - fwd0[2] * 1.0), -(fwd0[2] * 0.0 - fwd0[0] * 0.0), -(fwd0[0] * 1.0 - fwd0[1] * 0.0)];
    {
        let (hl, hr) =
            (&map[super::readable::side(0, super::readable::HIP)], &map[super::readable::side(1, super::readable::HIP)]);
        let (l, r) = (origin(&bind, node(&hl.0)), origin(&bind, node(&hr.0)));
        if (r[0] - l[0]) * right[0] + (r[2] - l[2]) * right[2] < 0.0 {
            right = [-right[0], -right[1], -right[2]];
        }
    }
    // Forward written in each `Fwd` bone's own frame (the transpose of its rotation), 12 cm.
    for (b, at) in &map {
        if *at == At::Fwd {
            let i = node(b);
            let m = &bind[i];
            let n: [f64; 3] = std::array::from_fn(|c| hypot(&[m[c * 4], m[c * 4 + 1], m[c * 4 + 2]]));
            let f: V = std::array::from_fn(|c| {
                (m[c * 4] * fwd0[0] + m[c * 4 + 1] * fwd0[1] + m[c * 4 + 2] * fwd0[2]) / (n[c] * n[c]) * 0.12
            });
            fwd_local.insert(i, f);
        }
    }
    let to_local = |p: V, o: V| -> V {
        let d = [p[0] - o[0], p[1], p[2] - o[2]];
        [d[0] * fwd0[0] + d[2] * fwd0[2], d[0] * right[0] + d[2] * right[2], d[1]]
    };

    // Every clip, sampled and turned into body points.
    let root_node = names.get(ROOT).unwrap_or(order[0]);
    let targets: Vec<(usize, At)> = map.iter().map(|(b, at)| (node(b), *at)).collect();
    let mut clips: Vec<Cap> = Vec::new();
    for (ai, an) in g.json["animations"].as_array().into_iter().flatten().enumerate() {
        struct Track {
            node: usize,
            path: u8,
            t: Vec<f64>,
            v: Vec<Vec<f64>>,
            step: bool,
        }
        let mut tracks = Vec::new();
        for c in an["channels"].as_array().into_iter().flatten() {
            let Some(nd) = c["target"]["node"].as_u64().map(|n| n as usize).filter(|&n| n < nodes.len()) else { continue };
            let path = match c["target"]["path"].as_str() {
                Some("translation") => 0,
                Some("rotation") => 1,
                Some("scale") => 2,
                _ => continue,
            };
            let s = &an["samplers"][c["sampler"].as_u64().unwrap_or(0) as usize];
            let t: Vec<f64> = g.accessor(&s["input"])?.into_iter().map(|x| x[0]).collect();
            let mut v = g.accessor(&s["output"])?;
            let interp = s["interpolation"].as_str().unwrap_or("LINEAR");
            if interp == "CUBICSPLINE" {
                // In-tangent, value, out-tangent per key: the values, in-betweened linearly.
                if !notes.iter().any(|n| n.contains("CUBICSPLINE")) {
                    notes.push(format!("{file}: CUBICSPLINE tracks are read as linear"));
                }
                v = v.chunks(3).filter_map(|c| c.get(1).cloned()).collect();
            }
            if t.is_empty() || v.len() < t.len() {
                continue;
            }
            tracks.push(Track { node: nd, path, t, v, step: interp == "STEP" });
        }
        let name = an["name"].as_str().map(str::to_string).unwrap_or_else(|| format!("Clip{ai}"));
        let dur = tracks.iter().map(|k| k.t[k.t.len() - 1]).fold(f64::NEG_INFINITY, f64::max);
        let dur = if dur.is_finite() { dur } else { 0.0 };
        let n = (js_round(dur * fps) as i64 + 1).max(1) as usize;
        let mut data = vec![0f32; n * P * 3];
        let mut travel = vec![0f32; n * 2];
        let mut moved = false;
        let mut trs = rest.clone();
        for f in 0..n {
            let time = dur.min(f as f64 / fps);
            trs.clone_from_slice(&rest);
            for k in &tracks {
                let tt = &k.t;
                let mut j = 0;
                while j + 2 < tt.len() && tt[j + 1] <= time {
                    j += 1;
                }
                let u = if tt.len() < 2 || k.step {
                    0.0
                } else {
                    let span = tt[j + 1] - tt[j];
                    ((time - tt[j]) / if span != 0.0 { span } else { 1.0 }).clamp(0.0, 1.0)
                };
                let (a, b) = (&k.v[j], &k.v[(j + 1).min(k.v.len() - 1)]);
                let lerp = |n: usize| -> Vec<f64> { (0..n).map(|i| a[i] + (b[i] - a[i]) * u).collect() };
                let x = &mut trs[k.node];
                match k.path {
                    0 if a.len() >= 3 => x.t = std::array::from_fn(|i| lerp(3)[i]),
                    1 if a.len() >= 4 => {
                        let q = slerp(a, b, u);
                        x.r = [q[0], q[1], q[2], q[3]];
                    }
                    2 if a.len() >= 3 => x.s = std::array::from_fn(|i| lerp(3)[i]),
                    _ => {}
                }
            }
            let w = world(&trs);
            let root = origin(&w, root_node);
            let rl = to_local(root, [0.0; 3]);
            travel[f * 2] = (rl[0] * MM) as f32;
            travel[f * 2 + 1] = (rl[1] * MM) as f32;
            if rl[0].abs() + rl[1].abs() > 0.01 {
                moved = true;
            }
            for (i, &(nd, at)) in targets.iter().enumerate() {
                let p = to_local(pos(&w, nd, at, &fwd_local), root);
                for c in 0..3 {
                    data[(f * P + i) * 3 + c] = (p[c] * MM) as f32;
                }
            }
        }
        let looping = name.ends_with("_Loop") || name == "Idle" || name.ends_with("_Idle");
        let cap =
            Cap { name: name.clone(), n, dur, looping, fps, data, travel: moved.then_some(travel), take: None, stride: None };
        match clips.iter_mut().find(|c| c.name == name) {
            Some(c) => *c = cap,
            None => clips.push(cap),
        }
    }
    Ok(Library { rig, clips, notes })
}
