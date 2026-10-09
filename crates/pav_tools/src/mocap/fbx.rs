//! Autodesk FBX, binary (version 7: what Mixamo, Blender, Maya and MotionBuilder export): the
//! node tree, its skeleton (`Model` nodes, bones and the empties above them) and each
//! animation stack's curves. A stack is sampled into a take of the skeleton (`super::bvh::Bvh`,
//! its joints' transforms given outright), so FBX takes are read, placed and cut as BVH ones
//! are.
//!
//! A bone's transform follows the SDK's formula: translation, rotation offset and pivot, pre-
//! rotation, rotation (in its own order), the post-rotation undone, the pivot undone, then
//! scaling with its own offset and pivot. Curves are in-betweened linearly (exporters bake a key
//! a frame). A file whose up axis is z or x is turned so y is up; forward and right are found
//! from the body, as for BVH.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Result, anyhow, bail};
use glam::{DMat3, DMat4, DVec3};

use super::bvh::Bvh;

/// One FBX node: its name, properties and children.
#[derive(Debug, Default)]
pub struct Node {
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Node>,
}

/// A node property.
#[derive(Debug, Clone)]
pub enum Prop {
    Int(i64),
    Float(f64),
    Bool(bool),
    Ints(Vec<i64>),
    Floats(Vec<f64>),
    Bools(Vec<bool>),
    Str(String),
    Raw(Vec<u8>),
}

impl Prop {
    fn int(&self) -> Option<i64> {
        match self {
            Prop::Int(v) => Some(*v),
            Prop::Float(v) => Some(*v as i64),
            Prop::Bool(b) => Some(*b as i64),
            _ => None,
        }
    }
    fn float(&self) -> Option<f64> {
        match self {
            Prop::Float(v) => Some(*v),
            Prop::Int(v) => Some(*v as f64),
            _ => None,
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            Prop::Str(s) => Some(s),
            _ => None,
        }
    }
}

impl Node {
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    fn prop(&self, i: usize) -> Option<&Prop> {
        self.props.get(i)
    }
    /// A `Properties70` entry's values (after its name, type, label and flags).
    fn p70(&self, name: &str) -> Option<&[Prop]> {
        self.child("Properties70")?
            .children
            .iter()
            .find(|p| p.prop(0).and_then(Prop::str) == Some(name))
            .map(|p| &p.props[4.min(p.props.len())..])
    }
    fn p70_vec(&self, name: &str) -> Option<DVec3> {
        let v = self.p70(name)?;
        Some(DVec3::new(v.first()?.float()?, v.get(1)?.float()?, v.get(2)?.float()?))
    }
    fn p70_num(&self, name: &str) -> Option<f64> {
        self.p70(name)?.first()?.float()
    }
}

/// FBX time: ticks a second.
const KTIME: f64 = 46_186_158_000.0;

struct Reader<'a> {
    b: &'a [u8],
    wide: bool,
}

impl Reader<'_> {
    fn u32(&self, o: usize) -> Result<u32> {
        let s = self.b.get(o..o + 4).ok_or_else(|| anyhow!("the file ends early"))?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&self, o: usize) -> Result<u64> {
        let s = self.b.get(o..o + 8).ok_or_else(|| anyhow!("the file ends early"))?;
        Ok(u64::from_le_bytes(s.try_into().unwrap()))
    }
    fn bytes(&self, o: usize, n: usize) -> Result<&[u8]> {
        self.b.get(o..o + n).ok_or_else(|| anyhow!("the file ends early"))
    }

    /// The node at `o` (`None`: the empty record that ends a list) and where the next starts.
    fn node(&self, o: usize, depth: usize) -> Result<(Option<Node>, usize)> {
        if depth > 64 {
            bail!("nodes nested too deep");
        }
        let (end, n, head) = if self.wide {
            (self.u64(o)? as usize, self.u64(o + 8)? as usize, 25)
        } else {
            (self.u32(o)? as usize, self.u32(o + 4)? as usize, 13)
        };
        let name_len = *self.b.get(o + head - 1).ok_or_else(|| anyhow!("the file ends early"))? as usize;
        if end == 0 {
            return Ok((None, o + head));
        }
        if end <= o || end > self.b.len() {
            bail!("a node runs past the end of the file");
        }
        let name = String::from_utf8_lossy(self.bytes(o + head, name_len)?).to_string();
        let mut p = o + head + name_len;
        let mut props = Vec::with_capacity(n);
        for _ in 0..n {
            let (prop, next) = self.prop(p)?;
            props.push(prop);
            p = next;
        }
        let mut children = Vec::new();
        while p < end {
            let (child, next) = self.node(p, depth + 1)?;
            p = next;
            match child {
                Some(c) => children.push(c),
                None => break,
            }
        }
        Ok((Some(Node { name, props, children }), end))
    }

    fn prop(&self, p: usize) -> Result<(Prop, usize)> {
        let t = *self.b.get(p).ok_or_else(|| anyhow!("the file ends early"))?;
        let p = p + 1;
        let le = |s: &[u8]| -> [u8; 8] {
            let mut a = [0u8; 8];
            a[..s.len()].copy_from_slice(s);
            a
        };
        Ok(match t {
            b'Y' => (Prop::Int(i16::from_le_bytes(self.bytes(p, 2)?.try_into().unwrap()) as i64), p + 2),
            b'C' => (Prop::Bool(self.bytes(p, 1)?[0] != 0), p + 1),
            b'I' => (Prop::Int(i32::from_le_bytes(self.bytes(p, 4)?.try_into().unwrap()) as i64), p + 4),
            b'F' => (Prop::Float(f32::from_le_bytes(self.bytes(p, 4)?.try_into().unwrap()) as f64), p + 4),
            b'D' => (Prop::Float(f64::from_le_bytes(le(self.bytes(p, 8)?))), p + 8),
            b'L' => (Prop::Int(i64::from_le_bytes(le(self.bytes(p, 8)?))), p + 8),
            b'S' | b'R' => {
                let n = self.u32(p)? as usize;
                let s = self.bytes(p + 4, n)?;
                let v = if t == b'S' { Prop::Str(String::from_utf8_lossy(s).to_string()) } else { Prop::Raw(s.to_vec()) };
                (v, p + 4 + n)
            }
            b'f' | b'd' | b'l' | b'i' | b'b' => {
                let (count, enc, len) = (self.u32(p)? as usize, self.u32(p + 4)?, self.u32(p + 8)? as usize);
                let raw = self.bytes(p + 12, len)?;
                let data = match enc {
                    0 => raw.to_vec(),
                    1 => miniz_oxide::inflate::decompress_to_vec_zlib(raw).map_err(|e| anyhow!("an array: inflate: {e:?}"))?,
                    e => bail!("array encoding {e} is not read"),
                };
                let size = match t {
                    b'f' | b'i' => 4,
                    b'd' | b'l' => 8,
                    _ => 1,
                };
                if data.len() < count * size {
                    bail!("an array is shorter than it says");
                }
                let at = |i: usize| &data[i * size..(i + 1) * size];
                let v = match t {
                    b'f' => Prop::Floats((0..count).map(|i| f32::from_le_bytes(at(i).try_into().unwrap()) as f64).collect()),
                    b'd' => Prop::Floats((0..count).map(|i| f64::from_le_bytes(at(i).try_into().unwrap())).collect()),
                    b'l' => Prop::Ints((0..count).map(|i| i64::from_le_bytes(at(i).try_into().unwrap())).collect()),
                    b'i' => Prop::Ints((0..count).map(|i| i32::from_le_bytes(at(i).try_into().unwrap()) as i64).collect()),
                    _ => Prop::Bools((0..count).map(|i| at(i)[0] != 0).collect()),
                };
                (v, p + 12 + len)
            }
            other => bail!("property type '{}' is not read", other as char),
        })
    }
}

/// The top-level nodes of a binary FBX file.
pub fn parse(bytes: &[u8]) -> Result<Vec<Node>> {
    if bytes.len() < 27 || !bytes.starts_with(b"Kaydara FBX Binary") {
        bail!("not a binary FBX file (an ASCII one: export it as binary)");
    }
    let version = u32::from_le_bytes(bytes[23..27].try_into().unwrap());
    if version < 7000 {
        bail!("FBX version {version}: the reader takes version 7 files");
    }
    let r = Reader { b: bytes, wide: version >= 7500 };
    let mut out = Vec::new();
    let mut o = 27;
    while o + 13 <= bytes.len() {
        let (n, next) = r.node(o, 0)?;
        match n {
            Some(n) => out.push(n),
            None => break,
        }
        o = next;
    }
    Ok(out)
}

/// A name without its class (`Hips\0\x01Model`) or namespace stays as the file writes it but
/// for the class.
fn object_name(n: &Node) -> String {
    let s = n.prop(1).and_then(Prop::str).unwrap_or("");
    s.split("\u{0}\u{1}").next().unwrap_or(s).split("::").last().unwrap_or(s).to_string()
}

/// A rotation from Euler angles in degrees, applied in `order` (FBX's enum: 0 XYZ, 1 XZY, 2 YZX,
/// 3 YXZ, 4 ZXY, 5 ZYX; the first letter turns first).
fn euler(v: DVec3, order: i64) -> DMat3 {
    let (x, y, z) = (
        DMat3::from_rotation_x(v.x.to_radians()),
        DMat3::from_rotation_y(v.y.to_radians()),
        DMat3::from_rotation_z(v.z.to_radians()),
    );
    match order {
        1 => y * z * x,
        2 => x * z * y,
        3 => z * x * y,
        4 => y * x * z,
        5 => x * y * z,
        _ => z * y * x,
    }
}

/// A bone (or an empty above the bones) and its transform's parts.
struct Model {
    id: i64,
    name: String,
    t: DVec3,
    r: DVec3,
    s: DVec3,
    pre: DVec3,
    post: DVec3,
    r_off: DVec3,
    r_piv: DVec3,
    s_off: DVec3,
    s_piv: DVec3,
    order: i64,
}

impl Model {
    fn matrix(&self, t: DVec3, r: DVec3, s: DVec3) -> DMat4 {
        let rot = |m: DMat3| DMat4::from_mat3(m);
        DMat4::from_translation(t)
            * DMat4::from_translation(self.r_off)
            * DMat4::from_translation(self.r_piv)
            * rot(euler(self.pre, self.order))
            * rot(euler(r, self.order))
            * rot(euler(self.post, self.order)).inverse()
            * DMat4::from_translation(-self.r_piv)
            * DMat4::from_translation(self.s_off)
            * DMat4::from_translation(self.s_piv)
            * DMat4::from_scale(s)
            * DMat4::from_translation(-self.s_piv)
    }
}

/// One animation curve: key times (seconds) and values.
struct Curve {
    t: Vec<f64>,
    v: Vec<f64>,
}

impl Curve {
    fn at(&self, time: f64) -> f64 {
        let n = self.t.len().min(self.v.len());
        if n == 0 {
            return 0.0;
        }
        if time <= self.t[0] {
            return self.v[0];
        }
        if time >= self.t[n - 1] {
            return self.v[n - 1];
        }
        let k = self.t[..n].partition_point(|&x| x <= time).max(1) - 1;
        let span = self.t[k + 1] - self.t[k];
        let u = if span > 0.0 { (time - self.t[k]) / span } else { 0.0 };
        self.v[k] + (self.v[k + 1] - self.v[k]) * u
    }
}

/// A channel of a bone in a stack: translation, rotation or scaling, each axis a curve or a
/// value.
type Channel = [Result<i64, f64>; 3];

/// Every animation stack of a file, each sampled at `fps` into a take of its skeleton (stacks
/// with nothing animated are left out), named as the stack is (Blender's `Armature|` dropped; a
/// file's lone stack named for no clip, Mixamo's `mixamo.com`, by the file).
pub fn read(path: &Path, fps: f64) -> Result<Vec<(String, Bvh)>> {
    let bytes = std::fs::read(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    let top = parse(&bytes).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    let get = |n: &str| top.iter().find(|t| t.name == n);
    let objects = get("Objects").ok_or_else(|| anyhow!("{}: no Objects", path.display()))?;
    let settings = get("GlobalSettings");
    let up = settings.and_then(|g| g.p70_num("UpAxis")).unwrap_or(1.0) as i64;
    let up_sign = settings.and_then(|g| g.p70_num("UpAxisSign")).unwrap_or(1.0).signum();
    // y up: z up turns a quarter about x, x up a quarter about z.
    let to_y = match up {
        2 => DMat3::from_rotation_x(-std::f64::consts::FRAC_PI_2 * up_sign),
        0 => DMat3::from_rotation_z(std::f64::consts::FRAC_PI_2 * up_sign),
        _ if up_sign < 0.0 => DMat3::from_rotation_x(std::f64::consts::PI),
        _ => DMat3::IDENTITY,
    };

    // The skeleton: bones and the empties they hang from.
    let mut models: Vec<Model> = Vec::new();
    for n in objects.children.iter().filter(|n| n.name == "Model") {
        let kind = n.prop(2).and_then(Prop::str).unwrap_or("");
        if !matches!(kind, "LimbNode" | "Limb" | "Null" | "Root" | "") {
            continue;
        }
        let v = |k: &str, d: DVec3| n.p70_vec(k).unwrap_or(d);
        models.push(Model {
            id: n.prop(0).and_then(Prop::int).unwrap_or(0),
            name: object_name(n),
            t: v("Lcl Translation", DVec3::ZERO),
            r: v("Lcl Rotation", DVec3::ZERO),
            s: v("Lcl Scaling", DVec3::ONE),
            pre: v("PreRotation", DVec3::ZERO),
            post: v("PostRotation", DVec3::ZERO),
            r_off: v("RotationOffset", DVec3::ZERO),
            r_piv: v("RotationPivot", DVec3::ZERO),
            s_off: v("ScalingOffset", DVec3::ZERO),
            s_piv: v("ScalingPivot", DVec3::ZERO),
            order: n.p70_num("RotationOrder").unwrap_or(0.0) as i64,
        });
    }
    if models.is_empty() {
        bail!("{}: no skeleton (no bones)", path.display());
    }
    let index: HashMap<i64, usize> = models.iter().enumerate().map(|(i, m)| (m.id, i)).collect();

    // Connections: child to parent, and which property a curve drives.
    let mut parent: Vec<Option<usize>> = vec![None; models.len()];
    let mut links: Vec<(i64, i64, String)> = Vec::new();
    for c in get("Connections").map(|c| c.children.as_slice()).unwrap_or(&[]) {
        let (kind, a, b) = (c.prop(0).and_then(Prop::str), c.prop(1).and_then(Prop::int), c.prop(2).and_then(Prop::int));
        let (Some(kind), Some(a), Some(b)) = (kind, a, b) else { continue };
        if kind == "OO" {
            if let (Some(&ci), Some(&pi)) = (index.get(&a), index.get(&b)) {
                parent[ci] = Some(pi);
            }
        }
        links.push((a, b, c.prop(3).and_then(Prop::str).unwrap_or("").to_string()));
    }

    // Parents before children.
    let mut order: Vec<usize> = Vec::new();
    let mut placed = vec![false; models.len()];
    while order.len() < models.len() {
        let before = order.len();
        for i in 0..models.len() {
            if !placed[i] && parent[i].is_none_or(|p| placed[p]) {
                placed[i] = true;
                order.push(i);
            }
        }
        if order.len() == before {
            bail!("{}: the skeleton's parents loop", path.display());
        }
    }
    let pos: HashMap<usize, usize> = order.iter().enumerate().map(|(k, &i)| (i, k)).collect();
    let names: Vec<String> = order.iter().map(|&i| models[i].name.clone()).collect();
    let parents: Vec<Option<usize>> = order.iter().map(|&i| parent[i].map(|p| pos[&p])).collect();

    // Curves, curve nodes (a bone's T, R or S), layers and stacks.
    let mut curves: HashMap<i64, Curve> = HashMap::new();
    let mut node_defaults: HashMap<i64, DVec3> = HashMap::new();
    let mut stacks: Vec<(i64, String, Option<(f64, f64)>)> = Vec::new();
    for n in &objects.children {
        let id = n.prop(0).and_then(Prop::int).unwrap_or(0);
        match n.name.as_str() {
            "AnimationCurve" => {
                let times = match n.child("KeyTime").and_then(|k| k.prop(0)) {
                    Some(Prop::Ints(t)) => t.iter().map(|&k| k as f64 / KTIME).collect(),
                    _ => Vec::new(),
                };
                let values = match n.child("KeyValueFloat").or_else(|| n.child("KeyValueDouble")).and_then(|k| k.prop(0)) {
                    Some(Prop::Floats(v)) => v.clone(),
                    _ => Vec::new(),
                };
                curves.insert(id, Curve { t: times, v: values });
            }
            "AnimationCurveNode" => {
                let d = |k: &str| n.p70_num(k).unwrap_or(0.0);
                node_defaults.insert(id, DVec3::new(d("d|X"), d("d|Y"), d("d|Z")));
            }
            "AnimationStack" => {
                let span = match (n.p70("LocalStart"), n.p70("LocalStop")) {
                    (Some(a), Some(b)) => a
                        .first()
                        .and_then(Prop::int)
                        .zip(b.first().and_then(Prop::int))
                        .map(|(a, b)| (a as f64 / KTIME, b as f64 / KTIME)),
                    _ => None,
                };
                stacks.push((id, object_name(n), span.filter(|(a, b)| b > a)));
            }
            _ => {}
        }
    }
    // Each object's children and parents (with the property a link drives).
    let mut kids: HashMap<i64, Vec<(i64, &str)>> = HashMap::new();
    let mut ups: HashMap<i64, Vec<(i64, &str)>> = HashMap::new();
    for (a, b, prop) in &links {
        kids.entry(*b).or_default().push((*a, prop.as_str()));
        ups.entry(*a).or_default().push((*b, prop.as_str()));
    }
    let none = Vec::new();
    let kids_of = |id: i64| kids.get(&id).unwrap_or(&none);
    let layers_of = |stack: i64| -> Vec<i64> { kids_of(stack).iter().filter(|(_, p)| p.is_empty()).map(|(a, _)| *a).collect() };
    // A layer's curve nodes.
    let nodes_of =
        |layer: i64| -> Vec<i64> { kids_of(layer).iter().map(|(a, _)| *a).filter(|a| node_defaults.contains_key(a)).collect() };

    let animated: Vec<&(i64, String, Option<(f64, f64)>)> =
        stacks.iter().filter(|(id, _, _)| layers_of(*id).iter().any(|&l| !nodes_of(l).is_empty())).collect();
    let mut out = Vec::new();
    let stem = path.file_stem().map(|s| s.to_string_lossy().replace(' ', "_")).unwrap_or_default();
    for (sid, sname, span) in &animated {
        // This stack's curve nodes on each bone: T, R, S.
        let mut chans: HashMap<(usize, u8), Channel> = HashMap::new();
        let mut keys: Vec<f64> = Vec::new();
        // The first layer's (more are blended over it: not read).
        for cn in layers_of(*sid).first().map(|&l| nodes_of(l)).unwrap_or_default() {
            for &(b, prop) in ups.get(&cn).unwrap_or(&none) {
                let Some(&mi) = index.get(&b) else { continue };
                let which = match prop {
                    "Lcl Translation" => 0,
                    "Lcl Rotation" => 1,
                    "Lcl Scaling" => 2,
                    _ => continue,
                };
                let d = node_defaults[&cn];
                let mut ch: Channel = [Err(d.x), Err(d.y), Err(d.z)];
                for &(cv, axis) in kids_of(cn) {
                    if let (Some(c), Some(k)) = (curves.get(&cv), ["d|X", "d|Y", "d|Z"].iter().position(|x| *x == axis)) {
                        keys.extend(c.t.first().into_iter().chain(c.t.last()));
                        ch[k] = Ok(cv);
                    }
                }
                chans.insert((mi, which), ch);
            }
        }
        let (start, stop) = span.unwrap_or_else(|| {
            (keys.iter().copied().fold(f64::INFINITY, f64::min), keys.iter().copied().fold(f64::NEG_INFINITY, f64::max))
        });
        if !start.is_finite() || !stop.is_finite() {
            continue;
        }
        let frames = ((stop - start) * fps).round().max(0.0) as usize + 1;
        let value = |mi: usize, which: u8, d: DVec3, time: f64| -> DVec3 {
            match chans.get(&(mi, which)) {
                None => d,
                Some(ch) => {
                    let get = |k: usize| match ch[k] {
                        Ok(c) => curves[&c].at(time),
                        Err(v) => v,
                    };
                    DVec3::new(get(0), get(1), get(2))
                }
            }
        };
        let pose = |time: Option<f64>| -> Vec<(DMat3, DVec3)> {
            // World matrices (scale and all), then each bone's rotation and place without the
            // scale, and from those each bone's own, relative to its parent.
            let mut world: Vec<DMat4> = Vec::with_capacity(order.len());
            for (k, &mi) in order.iter().enumerate() {
                let m = &models[mi];
                let (t, r, s) = match time {
                    Some(tm) => (value(mi, 0, m.t, tm), value(mi, 1, m.r, tm), value(mi, 2, m.s, tm)),
                    None => (m.t, m.r, m.s),
                };
                let local = m.matrix(t, r, s);
                world.push(match parents[k] {
                    Some(p) => world[p] * local,
                    None => DMat4::from_mat3(to_y) * local,
                });
            }
            let rigid: Vec<(DMat3, DVec3)> = world
                .iter()
                .map(|w| {
                    let m = DMat3::from_mat4(*w);
                    let r = DMat3::from_cols(
                        m.x_axis.normalize_or_zero(),
                        m.y_axis.normalize_or_zero(),
                        m.z_axis.normalize_or_zero(),
                    );
                    (r, w.w_axis.truncate())
                })
                .collect();
            (0..order.len())
                .map(|k| match parents[k] {
                    Some(p) => (rigid[p].0.transpose() * rigid[k].0, rigid[p].0.transpose() * (rigid[k].1 - rigid[p].1)),
                    None => rigid[k],
                })
                .collect()
        };
        let bind = pose(None);
        let mut local = Vec::with_capacity(frames * order.len());
        for f in 0..frames {
            local.extend(pose(Some(start + f as f64 / fps)));
        }
        let name = match sname.rsplit_once('|') {
            Some((_, n)) => n.to_string(),
            None if animated.len() == 1 || sname == "mixamo.com" => stem.clone(),
            None => sname.clone(),
        };
        out.push((name, Bvh::from_poses(names.clone(), parents.clone(), bind, local, 1.0 / fps)));
    }
    if out.is_empty() {
        bail!("{}: no animation (no stack moves the skeleton)", path.display());
    }
    Ok(out)
}
