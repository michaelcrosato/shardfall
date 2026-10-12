//! Compile: a prop document to resolved parts, the form that is drawn, collided and measured.
//! Pure and deterministic: f32 throughout, ids in BTreeMap order, depth first, copies in index
//! order, lengths in metres. A seed picks the variant: `chance`, `jitter` and `pick` draw from
//! FNV-1a of `"{seed}:{resolved id}:{field}"`, so adding a part never reshuffles the others.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use glam::{EulerRot, Mat3, Quat, Vec3};
use serde::Serialize;

use super::doc::{Anchor, Attach, Axis, Dir, Doc, Kind, Material, MaterialEntry, Node, Prim, PrimType, Replicate, Surface};
use super::geom::{self, Generated, Primitive};
use super::lint::Finding;
use super::{Bounds, PropDef, fnv};
use crate::issue::{Code, Issue};
use crate::shape::{Look, Sway};

/// Parts after copies, mirrors and references.
pub const MAX_PARTS: usize = 1_024;
/// Distinct generated meshes per asset.
pub const MAX_MESHES: usize = 64;
/// References inside references.
pub const MAX_REF_DEPTH: u8 = 3;
/// Spheres and capsules (signed distance fields) before `sdf_heavy`.
const SDF_HEAVY: usize = 64;
/// Parts this close count as touching (`floating`).
const JOIN: f32 = 0.005;

/// Theme colour roles when no palette is given: neutral stone and a warm light.
pub const DEFAULT_ROLES: &[(&str, &str)] = &[
    ("$floor", "#7a7268"),
    ("$floor2", "#6f675e"),
    ("$wall", "#9a948a"),
    ("$pillar", "#a89f92"),
    ("$accent", "#c8a24a"),
    ("$light", "#ffc870"),
];

/// A prop's resolved parts and what they carry, in metres in the asset's frame.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CompiledProp {
    pub parts: Vec<ResolvedPart>,
    /// The parts' boxes; zero at the origin when no part is drawn.
    pub bounds: Bounds,
    pub sockets: BTreeMap<String, ResolvedSocket>,
    pub lights: Vec<PartLight>,
    pub fx: Vec<PartFx>,
    pub warnings: Vec<Finding>,
}

/// One primitive placed in the asset's frame.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedPart {
    /// `seat`, `leg_back~x`, `stones[1].ring[3]`, `crate.lid` (inside a ref).
    pub id: String,
    pub prim: Primitive,
    pub pos: Vec3,
    /// Always a proper rotation; a mirror copy sets `mirrored` instead.
    pub rot: Quat,
    /// Uniform scale on the primitive (references and jitter).
    pub scale: f32,
    /// `#rrggbb` or a theme role such as `$wall`.
    pub color: String,
    /// Linear RGB multiplier from `jitter.color`.
    pub tint: f32,
    pub look: Look,
    pub emissive: f32,
    /// The nearest material key, for patterned surfaces.
    pub mat: Option<String>,
    pub solid: bool,
    /// Collision only: not drawn.
    pub hidden: bool,
    pub sway: Sway,
    /// A mirror copy: local x is reflected (it matters for extrusions).
    pub mirrored: bool,
    /// The emissive glow may light the surroundings (the part has no light of its own).
    pub glow_light: bool,
    pub aabb: Bounds,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedSocket {
    pub pos: Vec3,
    /// +y is the socket's working direction.
    pub rot: Quat,
}

/// A part's point light (`LightDef` with its offset at the part).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PartLight {
    pub part: String,
    pub pos: Vec3,
    pub color: String,
    pub radius: f32,
    pub intensity: f32,
    pub flicker: f32,
    pub pulse: f32,
    pub shadows: bool,
}

/// A part's particle emitter (`EmitterDef` at the part).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PartFx {
    pub part: String,
    pub pos: Vec3,
    pub preset: String,
    pub rate: f32,
    pub color: String,
    pub size: f32,
    pub speed: f32,
    pub life: f32,
    pub area: Vec3,
}

/// The assets a document's refs name.
pub type Deps<'a> = &'a dyn Fn(&str) -> Option<Arc<PropDef>>;

/// The seeded number in [-1, 1] for one field of one resolved part.
pub fn noise(seed: u32, id: &str, field: &str) -> f32 {
    let hash = fnv(format!("{seed}:{id}:{field}").as_bytes());
    ((hash >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) as f32
}

/// Resolve a document's parts for one seed.
pub fn compile(doc: &Doc, deps: Deps, seed: u32) -> Result<CompiledProp, Vec<Issue>> {
    let mut compiler = Compiler {
        doc,
        deps,
        seed,
        unit: 1.0 / doc.unit.per_metre(),
        siblings: HashMap::new(),
        out: CompiledProp::default(),
        issues: Vec::new(),
    };
    let mut levels = vec![Level::of(&doc.surface)];
    compiler.nodes(&doc.parts, None, &Frame::IDENTITY, &mut levels, 1.0, "", "parts");
    compiler.sockets();
    compiler.finish();
    if compiler.issues.is_empty() { Ok(compiler.out) } else { Err(compiler.issues) }
}

/// A frame: a point is placed at `t + s · q · (d ∘ p)`, a rotation after the mirror signs `d`.
/// Keeping the rotation a quaternion makes a format-1 part's transform exactly its own.
#[derive(Clone, Copy, Debug)]
struct Frame {
    t: Vec3,
    q: Quat,
    d: Vec3,
    s: f32,
}

impl Frame {
    const IDENTITY: Self = Self { t: Vec3::ZERO, q: Quat::IDENTITY, d: Vec3::ONE, s: 1.0 };

    fn vector(&self, v: Vec3) -> Vec3 {
        self.q * (self.d * v)
    }

    fn point(&self, p: Vec3) -> Vec3 {
        self.t + self.vector(p) * self.s
    }

    /// A frame inside this one: (Q1 D1)(Q2 D2) = Q1 (D1 Q2 D1) D1 D2.
    fn then(&self, local: &Frame) -> Frame {
        Frame { t: self.point(local.t), q: self.q * reflected(self.d, local.q), d: self.d * local.d, s: self.s * local.s }
    }

    /// An odd number of mirrors: the shape is drawn with local x reflected.
    fn mirrored(&self) -> bool {
        self.d.x * self.d.y * self.d.z < 0.0
    }

    /// The proper rotation a part is drawn with: a mirrored frame reflects local x instead.
    fn rotation(&self) -> Quat {
        let d = if self.mirrored() { self.d * Vec3::new(-1.0, 1.0, 1.0) } else { self.d };
        self.q * half_turns(d)
    }
}

/// D q D for mirror signs D: a rotation seen through mirrors.
fn reflected(d: Vec3, q: Quat) -> Quat {
    let v = Vec3::new(q.x, q.y, q.z) * d * (d.x * d.y * d.z);
    Quat::from_xyzw(v.x, v.y, v.z, q.w)
}

/// The rotation of mirror signs with an even number of flips: none, or a half turn.
fn half_turns(d: Vec3) -> Quat {
    match (d.x < 0.0, d.y < 0.0, d.z < 0.0) {
        (false, true, true) => Quat::from_xyzw(1.0, 0.0, 0.0, 0.0),
        (true, false, true) => Quat::from_xyzw(0.0, 1.0, 0.0, 0.0),
        (true, true, false) => Quat::from_xyzw(0.0, 0.0, 1.0, 0.0),
        _ => Quat::IDENTITY,
    }
}

/// One level of the surface cascade, borrowed from the document: a node's own fields with its
/// copy's cycle entry.
#[derive(Clone, Copy, Debug, Default)]
struct Level<'a> {
    mat: Option<&'a str>,
    color: Option<&'a str>,
    look: Option<Look>,
    emissive: Option<f32>,
    solid: Option<bool>,
}

impl<'a> Level<'a> {
    fn of(surface: &'a Surface) -> Self {
        Self {
            mat: surface.mat.as_deref(),
            color: surface.color.as_deref(),
            look: surface.look,
            emissive: surface.emissive,
            solid: surface.solid,
        }
    }

    /// A cycle entry: a material key sets `mat`; a colour or role sets `color` and clears `mat`.
    fn with_entry(mut self, entry: &'a str) -> Self {
        if entry.starts_with('#') || entry.starts_with('$') {
            self.color = Some(entry);
            self.mat = None;
        } else {
            self.mat = Some(entry);
        }
        self
    }
}

/// A node placed in its parent's frame, before copies: its position, its rotation, and its box
/// in its own frame.
#[derive(Clone, Copy, Debug)]
struct Placed {
    pos: Vec3,
    rot: Quat,
    centre: Vec3,
    half: Vec3,
}

/// One copy of a node in its parent's frame.
struct Repeat {
    suffix: String,
    frame: Frame,
    entry: Option<usize>,
}

/// A node among its siblings, placed once with its copies: every copy of its parent reuses it.
struct Child<'a> {
    name: &'a str,
    node: &'a Node,
    placed: Placed,
    copies: Vec<Repeat>,
}

struct Compiler<'a> {
    doc: &'a Doc,
    deps: Deps<'a>,
    seed: u32,
    /// Metres per document unit.
    unit: f32,
    /// Placed children per sibling map, keyed by the map's address in the borrowed document.
    siblings: HashMap<usize, Rc<Vec<Child<'a>>>>,
    out: CompiledProp,
    issues: Vec<Issue>,
}

/// Format 1's angles: YXZ, degrees.
fn euler(yaw: f32, pitch: f32, roll: f32) -> Quat {
    Quat::from_euler(EulerRot::YXZ, yaw.to_radians(), pitch.to_radians(), roll.to_radians())
}

/// Mirror signs for the chosen planes.
fn mirror_signs(axes: &[Axis]) -> Vec3 {
    let mut signs = Vec3::ONE;
    for axis in axes {
        match axis {
            Axis::X => signs.x = -1.0,
            Axis::Y => signs.y = -1.0,
            Axis::Z => signs.z = -1.0,
        }
    }
    signs
}

fn axis_vector(axis: Axis) -> Vec3 {
    match axis {
        Axis::X => Vec3::X,
        Axis::Y => Vec3::Y,
        Axis::Z => Vec3::Z,
    }
}

/// The frame of a part placed by its two ends: local +y runs from → to; local +z is the parent's
/// +y made perpendicular to it (+z when the axis is exactly vertical); x = y × z.
fn ends_frame(from: Vec3, to: Vec3) -> Quat {
    let y = (to - from).normalize();
    let z = if y.y.abs() >= 1.0 - 1e-6 { Vec3::Z } else { (Vec3::Y - y * y.y).normalize() };
    Quat::from_mat3(&Mat3::from_cols(y.cross(z), y, z)).normalize()
}

/// Box corners in a frame.
fn corners(frame: &Frame, centre: Vec3, half: Vec3) -> [Vec3; 8] {
    std::array::from_fn(|i| {
        let sign = |bit: usize| if i & bit == 0 { -1.0 } else { 1.0 };
        frame.point(centre + half * Vec3::new(sign(1), sign(2), sign(4)))
    })
}

/// The axis-aligned box around a box in a frame: its centre placed, its half extents through
/// the absolute rotation (mirrors only flip signs, so they drop out).
fn placed_box(frame: &Frame, centre: Vec3, half: Vec3) -> (Vec3, Vec3) {
    let m = Mat3::from_quat(frame.q);
    let extent = Mat3::from_cols(m.x_axis.abs(), m.y_axis.abs(), m.z_axis.abs()) * (half * frame.s);
    let centre = frame.point(centre);
    (centre - extent, centre + extent)
}

fn aabb(points: impl IntoIterator<Item = Vec3>) -> Option<(Vec3, Vec3)> {
    points.into_iter().fold(None, |acc, p| Some(acc.map_or((p, p), |(lo, hi): (Vec3, Vec3)| (lo.min(p), hi.max(p)))))
}

/// The direction a side word names in the parent frame: `top_front` is (0, 1, 1).
fn word_direction(word: &str) -> Vec3 {
    word.split('_').fold(Vec3::ZERO, |v, part| {
        v + match part {
            "top" => Vec3::Y,
            "bottom" => Vec3::NEG_Y,
            "front" => Vec3::Z,
            "back" => Vec3::NEG_Z,
            "left" => Vec3::NEG_X,
            "right" => Vec3::X,
            _ => Vec3::ZERO,
        }
    })
}

/// `name[i]~xz` → (name, copy, mirror axes).
fn segment(text: &str) -> (&str, Option<usize>, Vec<Axis>) {
    let (rest, axes) = text.split_once('~').unwrap_or((text, ""));
    let axes = axes.chars().filter_map(|c| Axis::parse(&c.to_string())).collect();
    match rest.split_once('[') {
        Some((name, index)) => (name, index.trim_end_matches(']').parse().ok(), axes),
        None => (rest, None, axes),
    }
}

impl<'a> Compiler<'a> {
    fn issue(&mut self, path: &str, code: Code, message: impl Into<String>) {
        self.issues.push(Issue::error(path, code, message));
    }

    fn length(&self, x: f32) -> f32 {
        x * self.unit
    }

    fn vector(&self, v: Vec3) -> Vec3 {
        v * self.unit
    }

    /// The primitive for a part node in metres, given its ends' distance when placed by them.
    fn primitive(&self, prim: &Prim, ends: Option<f32>) -> Primitive {
        let u = self.unit;
        let size = |i: usize| prim.size.as_ref().and_then(|s| s.get(i)).copied().unwrap_or(0.0) * u;
        let half = || match (&prim.size, prim.half, ends) {
            (Some(_), _, Some(length)) => Vec3::new(size(0) * 0.5, length * 0.5, size(1) * 0.5),
            (Some(_), _, None) => Vec3::new(size(0), size(1), size(2)) * 0.5,
            (None, Some(half), _) => half * u,
            _ => Vec3::splat(0.0),
        };
        let radius = prim.radius.unwrap_or(0.0) * u;
        let arc = prim.arc.unwrap_or(360.0);
        // Half the length along the axis: from/to, height, or format 1's half_height.
        let half_height = match (ends, prim.height, prim.half_height) {
            (Some(length), ..) => length * 0.5,
            (None, Some(height), _) => height * u * 0.5,
            (None, None, Some(half_height)) => half_height * u,
            _ => 0.0,
        };
        match prim.kind {
            PrimType::Box => Primitive::Box { half: half() },
            PrimType::RoundedBox => Primitive::RoundedBox { half: half(), radius },
            PrimType::Sphere => Primitive::Sphere { radius },
            PrimType::Ellipsoid => Primitive::Ellipsoid { half: half() },
            PrimType::Capsule => {
                let radius_end = prim.radius_end.map_or(radius, |r| r * u);
                match (ends, prim.height, prim.half_height) {
                    (None, None, Some(half_height)) => {
                        let h = half_height * u;
                        Primitive::Capsule { a: -h, b: h, radius, radius_end }
                    }
                    _ => Primitive::Capsule { a: -half_height + radius, b: half_height - radius_end, radius, radius_end },
                }
            }
            PrimType::Cylinder | PrimType::Cone => {
                let default_end = if prim.kind == PrimType::Cone { 0.0 } else { radius };
                Primitive::Cylinder {
                    half_height,
                    radius,
                    radius_end: prim.radius_end.map_or(default_end, |r| r * u),
                    arc,
                    segments: prim.segments.unwrap_or(geom::SEGMENTS),
                }
            }
            PrimType::Tube => Primitive::Tube { half_height, radius, thickness: prim.thickness.unwrap_or(0.0) * u, arc },
            PrimType::Torus => Primitive::Torus { radius, minor_radius: prim.minor_radius.unwrap_or(0.0) * u, arc },
            PrimType::Wedge => Primitive::Wedge { half: half(), peak: prim.peak.unwrap_or(0.0) },
            PrimType::Lathe => Primitive::Lathe {
                profile: prim.profile.iter().flatten().map(|p| [p[0] * u, p[1] * u]).collect(),
                segments: prim.segments.unwrap_or(geom::LATHE_SEGMENTS),
            },
            PrimType::Extrude => Primitive::Extrude {
                points: prim.points.iter().flatten().map(|p| [p[0] * u, p[1] * u]).collect(),
                half_depth: prim.depth.unwrap_or(0.0) * u * 0.5,
            },
        }
    }

    /// A node's own rotation in its parent's frame.
    fn rotation(&self, node: &Node) -> Quat {
        if let (Some(from), Some(to)) = (node.from, node.to) {
            return ends_frame(from, to) * Quat::from_rotation_y(node.spin.unwrap_or(0.0).to_radians());
        }
        let turn = euler(node.yaw.unwrap_or(0.0), node.pitch.unwrap_or(0.0), node.roll.unwrap_or(0.0));
        let Some(prim) = node.prim() else { return turn };
        let extrude = prim.kind == PrimType::Extrude;
        let rot = match node.axis {
            Some(axis) if prim.kind.takes_axis() => turn * geom::base_rotation(axis.vector(), extrude),
            _ => turn,
        };
        // An arc stays centred on its facing: turn the part about its own axis until local +z
        // points that way.
        if prim.kind.takes_arc() && prim.arc.is_some_and(|arc| arc < 360.0) {
            let authored = node.axis.unwrap_or(prim.kind.default_axis());
            let default = if matches!(authored, Dir::Y | Dir::NegY) { Dir::Z } else { Dir::Y };
            let facing = prim.facing.unwrap_or(default).vector();
            let g = rot.inverse() * facing;
            return rot * Quat::from_rotation_y(g.x.atan2(g.z));
        }
        rot
    }

    /// The box of a node in its own frame: a part's shape, a reference's bounds, a group's
    /// copies of its children.
    fn extent(&mut self, node: &'a Node) -> (Vec3, Vec3) {
        match &node.kind {
            Kind::Prim(prim) => {
                let ends = node.from.zip(node.to).map(|(from, to)| self.vector(to - from).length());
                self.primitive(prim, ends).local_box()
            }
            // Unscaled: a copy's frame carries the reference's scale.
            Kind::Ref(name) => (self.deps)(name).map_or((Vec3::ZERO, Vec3::ZERO), |def| {
                let b = def.compiled.bounds;
                (b.center(), b.size() * 0.5)
            }),
            Kind::Mesh(_) => (Vec3::ZERO, Vec3::ZERO),
            Kind::Group(children) => {
                let children = self.siblings(children);
                let points = children.iter().flat_map(|child| {
                    child.copies.iter().flat_map(|copy| corners(&copy.frame, child.placed.centre, child.placed.half))
                });
                aabb(points).map_or((Vec3::ZERO, Vec3::ZERO), |(lo, hi)| ((lo + hi) * 0.5, (hi - lo) * 0.5))
            }
        }
    }

    /// Every node of a sibling map placed, with its copies; computed once per map.
    fn siblings(&mut self, nodes: &'a BTreeMap<String, Node>) -> Rc<Vec<Child<'a>>> {
        let key = nodes as *const BTreeMap<String, Node> as usize;
        if let Some(children) = self.siblings.get(&key) {
            return children.clone();
        }
        let mut memo = BTreeMap::new();
        let mut children = Vec::with_capacity(nodes.len());
        for (name, node) in nodes {
            let placed = self.place(nodes, name, &mut memo, 0);
            let copies = self.copies(node, &placed);
            children.push(Child { name, node, placed, copies });
        }
        let children = Rc::new(children);
        self.siblings.insert(key, children.clone());
        children
    }

    /// Place a node among its siblings (memoised): its rotation, its position from pos, its
    /// ends or what it attaches to, and its own box.
    fn place(
        &mut self,
        siblings: &'a BTreeMap<String, Node>,
        name: &str,
        memo: &mut BTreeMap<String, Placed>,
        depth: usize,
    ) -> Placed {
        if let Some(placed) = memo.get(name) {
            return *placed;
        }
        let node = &siblings[name];
        let rot = self.rotation(node);
        let (centre, half) = self.extent(node);
        let mut placed = Placed { pos: Vec3::ZERO, rot, centre, half };
        if let (Some(from), Some(to)) = (node.from, node.to) {
            placed.pos = self.vector(from + to) * 0.5;
        } else if let Some(pos) = node.pos {
            placed.pos = self.vector(pos);
        }
        // The checker refuses attach cycles; the depth bound only guards unchecked documents.
        if let (Some(attach), true) = (&node.attach, depth < 64) {
            if let Some((point, direction)) = self.anchor(siblings, attach, memo, depth) {
                let own = Frame { t: Vec3::ZERO, q: rot, d: Vec3::ONE, s: node.scale.unwrap_or(1.0) };
                let offset = match &attach.own {
                    Some(Anchor::Point(uvw)) => own.point(centre + half * Vec3::from_array(*uvw)),
                    Some(Anchor::Word(word)) => box_point(&corners(&own, centre, half), word_direction(word)),
                    None if direction == Vec3::ZERO => own.point(centre),
                    None => box_point(&corners(&own, centre, half), -direction),
                };
                placed.pos = point - offset + node.offset.map_or(Vec3::ZERO, |o| self.vector(o));
            }
        }
        memo.insert(name.to_owned(), placed);
        placed
    }

    /// The point an attach names in the parent frame, and the direction it faces (which picks
    /// the attaching node's own anchor by default).
    fn anchor(
        &mut self,
        siblings: &'a BTreeMap<String, Node>,
        attach: &Attach,
        memo: &mut BTreeMap<String, Placed>,
        depth: usize,
    ) -> Option<(Vec3, Vec3)> {
        let segments: Vec<(&str, Option<usize>, Vec<Axis>)> = attach.to.split('.').map(segment).collect();
        // The target's frame in the parent frame, walking down through groups.
        let (first, ..) = segments.first()?;
        siblings.get(*first)?;
        let mut current = siblings;
        let mut frame = Frame::IDENTITY;
        let mut target: Option<(&Node, Placed)> = None;
        for (index, (name, copy, mirror)) in segments.iter().enumerate() {
            let node = current.get(*name)?;
            let suffix = copy.map(|i| format!("[{i}]")).unwrap_or_default()
                + &if mirror.is_empty() {
                    String::new()
                } else {
                    format!("~{}", mirror.iter().map(|a| a.name()).collect::<String>())
                };
            // The first step is among the siblings being placed; deeper ones are cached groups.
            let (placed, chosen) = if index == 0 {
                let placed = self.place(siblings, name, memo, depth + 1);
                (placed, self.copies(node, &placed).into_iter().find(|c| c.suffix == suffix)?.frame)
            } else {
                let children = self.siblings(current);
                let child = children.iter().find(|c| c.name == *name)?;
                (child.placed, child.copies.iter().find(|c| c.suffix == suffix)?.frame)
            };
            frame = frame.then(&chosen);
            target = Some((node, placed));
            if index + 1 < segments.len() {
                current = node.children()?;
            }
        }
        let (node, placed) = target?;
        let box_corners = corners(&frame, placed.centre, placed.half);
        Some(match &attach.at {
            None => (box_point(&box_corners, Vec3::Y), Vec3::Y),
            Some(Anchor::Word(word)) if word == "from" || word == "to" => {
                let sign = if word == "to" { 1.0 } else { -1.0 };
                let length = node.from.zip(node.to).map_or(placed.half.y, |(from, to)| self.vector(to - from).length() * 0.5);
                let point = frame.point(Vec3::Y * sign * length);
                let direction = frame.vector(Vec3::Y * sign).normalize_or_zero();
                (point, direction)
            }
            Some(Anchor::Word(word)) => {
                let direction = word_direction(word);
                (box_point(&box_corners, direction), direction)
            }
            Some(Anchor::Point(uvw)) => {
                let uvw = Vec3::from_array(*uvw);
                (frame.point(placed.centre + placed.half * uvw), frame.vector(uvw).normalize_or_zero())
            }
        })
    }

    /// A node's copies in its parent's frame: replication first, then each copy's mirrors.
    fn copies(&self, node: &Node, placed: &Placed) -> Vec<Repeat> {
        let mut copies: Vec<(String, Vec3, Quat, Option<usize>)> = Vec::new();
        let (pos, rot) = (placed.pos, placed.rot);
        match &node.replicate {
            None => copies.push((String::new(), pos, rot, None)),
            Some(Replicate::Array(array)) => {
                let n = array.count as f32;
                let step = match (array.step, array.span) {
                    (Some(step), _) => self.vector(step),
                    (None, Some(span)) => self.vector(span) / (n - 1.0).max(1.0),
                    _ => Vec3::ZERO,
                };
                for i in 0..array.count as usize {
                    copies.push((format!("[{i}]"), pos + step * (i as f32 - (n - 1.0) * 0.5), rot, Some(i)));
                }
            }
            Some(Replicate::Grid(grid)) => {
                let [a, b] = grid.plane.unwrap_or([Axis::X, Axis::Z]).map(axis_vector);
                let [na, nb] = grid.count;
                let [sa, sb] = grid.step.map(|s| self.length(s));
                for ib in 0..nb {
                    for ia in 0..na {
                        let k = (ia + na * ib) as usize;
                        let offset =
                            a * sa * (ia as f32 - (na as f32 - 1.0) * 0.5) + b * sb * (ib as f32 - (nb as f32 - 1.0) * 0.5);
                        copies.push((format!("[{k}]"), pos + offset, rot, Some(k)));
                    }
                }
            }
            Some(Replicate::Radial(radial)) => {
                let axis = radial.axis.unwrap_or(Dir::Y);
                let zero = if matches!(axis, Dir::Y | Dir::NegY) { Vec3::Z } else { Vec3::Y };
                let (n, arc) = (radial.count as f32, radial.arc.unwrap_or(360.0));
                let step = if arc >= 360.0 { arc / n } else { arc / (n - 1.0).max(1.0) };
                let radius = self.length(radial.radius);
                for i in 0..radial.count as usize {
                    let angle = radial.start.unwrap_or(0.0) + step * i as f32;
                    let turn = Quat::from_axis_angle(axis.vector(), angle.to_radians());
                    copies.push((format!("[{i}]"), pos + turn * (zero * radius), turn * rot, Some(i)));
                }
            }
        }
        let axes = &node.mirror;
        let mut out = Vec::with_capacity(copies.len() << axes.len());
        for (suffix, pos, rot, entry) in copies {
            for subset in 0..1usize << axes.len() {
                let chosen: Vec<Axis> =
                    axes.iter().enumerate().filter(|(i, _)| subset & (1 << i) != 0).map(|(_, a)| *a).collect();
                let m = mirror_signs(&chosen);
                let suffix = if chosen.is_empty() {
                    suffix.clone()
                } else {
                    format!("{suffix}~{}", chosen.iter().map(|a| a.name()).collect::<String>())
                };
                let scale = node.scale.unwrap_or(1.0);
                out.push(Repeat { suffix, frame: Frame { t: m * pos, q: reflected(m, rot), d: m, s: scale }, entry });
            }
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn nodes(
        &mut self,
        nodes: &'a BTreeMap<String, Node>,
        keep: Option<&BTreeSet<&str>>,
        parent: &Frame,
        levels: &mut Vec<Level<'a>>,
        tint: f32,
        prefix: &str,
        path: &str,
    ) {
        let children = self.siblings(nodes);
        for child in children.iter() {
            if keep.is_some_and(|keep| !keep.contains(child.name)) {
                continue;
            }
            let node = child.node;
            let node_path = || format!("{path}.{}", child.name);
            for copy in &child.copies {
                let mut id = String::with_capacity(prefix.len() + child.name.len() + copy.suffix.len());
                id.push_str(prefix);
                id.push_str(child.name);
                id.push_str(&copy.suffix);
                if node.chance.is_some_and(|chance| chance < 1.0 && (noise(self.seed, &id, "chance") + 1.0) * 0.5 >= chance) {
                    continue;
                }
                let mut local = copy.frame;
                let mut tint = tint;
                if let Some(jitter) = &node.jitter {
                    let n = |field: &str| noise(self.seed, &id, field);
                    if let Some(pos) = jitter.pos {
                        local.t += self.vector(pos * Vec3::new(n("pos.x"), n("pos.y"), n("pos.z")));
                    }
                    if let Some(rot) = jitter.rot {
                        // A turn in the node's own frame: through its mirrors, like any child.
                        let turn = euler(rot.x * n("rot.yaw"), rot.y * n("rot.pitch"), rot.z * n("rot.roll"));
                        local.q *= reflected(local.d, turn);
                    }
                    if let Some([lo, hi]) = jitter.scale {
                        local.s *= lo + (hi - lo) * (n("scale") + 1.0) * 0.5;
                    }
                    if let Some(color) = jitter.color {
                        tint *= 1.0 + color * n("color");
                    }
                }
                let world = parent.then(&local);
                let mut level = Level::of(&node.surface);
                if let (Some(cycle), Some(i)) = (node.replicate.as_ref().and_then(|r| r.cycle()), copy.entry) {
                    level = level.with_entry(&cycle[i % cycle.len()]);
                }
                levels.push(level);
                match &node.kind {
                    Kind::Group(grandchildren) => {
                        let keep = self.pick(node, grandchildren, &id);
                        self.nodes(
                            grandchildren,
                            keep.as_ref(),
                            &world,
                            levels,
                            tint,
                            &format!("{id}."),
                            &format!("{}.parts", node_path()),
                        );
                    }
                    Kind::Prim(prim) => {
                        let ends = node.from.zip(node.to).map(|(from, to)| self.vector(to - from).length());
                        let prim = self.primitive(prim, ends);
                        self.part(&node_path, &id, node, prim, &world, levels, tint);
                    }
                    Kind::Ref(name) => self.reference(&node_path, &id, node, name, &world, tint),
                    Kind::Mesh(_) => self.issue(&node_path(), Code::UnknownAsset, "mesh parts are not available yet"),
                }
                levels.pop();
            }
        }
    }

    /// A group with `pick: n` keeps the n children its seed ranks first; None keeps them all.
    fn pick(&self, node: &Node, children: &'a BTreeMap<String, Node>, id: &str) -> Option<BTreeSet<&'a str>> {
        let n = node.pick.filter(|n| (*n as usize) < children.len())? as usize;
        let mut ranked: Vec<(u64, &'a str)> =
            children.keys().map(|name| (fnv(format!("{}:{id}:pick:{name}", self.seed).as_bytes()), name.as_str())).collect();
        ranked.sort();
        Some(ranked.into_iter().take(n).map(|(_, name)| name).collect())
    }

    /// The material behind a key, when it is written inline.
    fn material(&self, key: &str) -> Option<&Material> {
        match self.doc.materials.get(key) {
            Some(MaterialEntry::Inline(material)) => Some(material),
            _ => None,
        }
    }

    /// A field from the surface cascade: at each level, nearest first, an explicit value wins,
    /// then that level's material.
    fn cascade<T>(&self, levels: &[Level], own: impl Fn(&Level) -> Option<T>, mat: impl Fn(&Material) -> Option<T>) -> Option<T> {
        levels.iter().rev().find_map(|level| own(level).or_else(|| level.mat.and_then(|key| self.material(key)).and_then(&mat)))
    }

    #[allow(clippy::too_many_arguments)]
    fn part(
        &mut self,
        path: &dyn Fn() -> String,
        id: &str,
        node: &Node,
        prim: Primitive,
        world: &Frame,
        levels: &[Level],
        tint: f32,
    ) {
        let color = self.cascade(levels, |l| l.color.map(String::from), |m| m.color.clone());
        let Some(color) = color else {
            self.issue(&path(), Code::Missing, "no colour: nothing gives this part color or mat");
            return;
        };
        let look = self.cascade(levels, |l| l.look, |m| m.look).unwrap_or_default();
        let emissive = self.cascade(levels, |l| l.emissive, |m| m.emissive).unwrap_or(0.0);
        let mat = levels.iter().rev().find_map(|l| l.mat).map(String::from);
        let solid = levels.iter().rev().find_map(|l| l.solid).unwrap_or(true);
        let (mirrored, rot) = (world.mirrored(), world.rotation());
        let (centre, half) = prim.local_box();
        let (min, max) = placed_box(world, centre, half);
        let pos = world.t;
        if let Some(light) = &node.light {
            self.out.lights.push(PartLight {
                part: id.to_owned(),
                pos,
                color: light.color.clone().unwrap_or_else(|| "#ffd9a0".into()),
                radius: light.radius.map_or(6.0, |r| self.length(r)),
                intensity: light.intensity.unwrap_or(1.5),
                flicker: light.flicker.unwrap_or(0.0),
                pulse: light.pulse.unwrap_or(0.0),
                shadows: light.shadows.unwrap_or(false),
            });
        }
        if let Some(fx) = &node.fx {
            self.out.fx.push(PartFx {
                part: id.to_owned(),
                pos,
                preset: fx.preset.clone(),
                rate: fx.rate.unwrap_or(0.0),
                color: fx.color.clone().unwrap_or_default(),
                size: fx.size.unwrap_or(1.0),
                speed: fx.speed.unwrap_or(1.0),
                life: fx.life.unwrap_or(1.0),
                area: fx.area.map_or(Vec3::ZERO, |a| self.vector(a)),
            });
        }
        self.out.parts.push(ResolvedPart {
            id: id.to_owned(),
            prim,
            pos,
            rot,
            scale: world.s,
            color,
            tint,
            look,
            emissive,
            mat,
            solid,
            hidden: node.hidden.unwrap_or(false),
            sway: node.sway.unwrap_or_default(),
            mirrored,
            glow_light: node.light.is_none(),
            aabb: Bounds { min, max },
        });
    }

    /// A reference inlines its asset's resolved parts as `id.part`; `remap` swaps its materials
    /// for this document's.
    fn reference(&mut self, path: &dyn Fn() -> String, id: &str, node: &Node, name: &str, world: &Frame, tint: f32) {
        let Some(def) = (self.deps)(name) else {
            self.issue(&path(), Code::UnknownAsset, format!("no asset \"{name}\""));
            return;
        };
        if def.ref_depth >= MAX_REF_DEPTH {
            self.issue(&path(), Code::Limit, format!("{name} nests references {} deep", def.ref_depth + 1));
            return;
        }
        for part in &def.compiled.parts {
            let d = if part.mirrored { Vec3::new(-1.0, 1.0, 1.0) } else { Vec3::ONE };
            let frame = world.then(&Frame { t: part.pos, q: part.rot, d, s: part.scale });
            let (centre, half) = part.prim.local_box();
            let (min, max) = placed_box(&frame, centre, half);
            let mut resolved = ResolvedPart {
                id: format!("{id}.{}", part.id),
                pos: frame.t,
                rot: frame.rotation(),
                scale: frame.s,
                tint: part.tint * tint,
                mirrored: frame.mirrored(),
                aabb: Bounds { min, max },
                ..part.clone()
            };
            let swap = part.mat.as_ref().and_then(|key| node.remap.as_ref()?.get(key)).cloned();
            if let Some(material) = swap.as_deref().and_then(|key| self.material(key)) {
                resolved.color = material.color.clone().unwrap_or(resolved.color);
                resolved.look = material.look.unwrap_or(resolved.look);
                resolved.emissive = material.emissive.unwrap_or(resolved.emissive);
                resolved.mat = swap;
            }
            self.out.parts.push(resolved);
        }
        for light in &def.compiled.lights {
            self.out.lights.push(PartLight {
                part: format!("{id}.{}", light.part),
                pos: world.point(light.pos),
                radius: light.radius * world.s,
                ..light.clone()
            });
        }
        for fx in &def.compiled.fx {
            self.out.fx.push(PartFx {
                part: format!("{id}.{}", fx.part),
                pos: world.point(fx.pos),
                area: fx.area * world.s,
                ..fx.clone()
            });
        }
    }

    fn sockets(&mut self) {
        let doc = self.doc;
        let mut memo = BTreeMap::new();
        for (name, socket) in &doc.sockets {
            let mut pos = socket.pos.map_or(Vec3::ZERO, |p| self.vector(p));
            if let Some(attach) = &socket.attach {
                if let Some((point, _)) = self.anchor(&doc.parts, attach, &mut memo, 0) {
                    pos = point + socket.offset.map_or(Vec3::ZERO, |o| self.vector(o));
                }
            }
            let rot = match socket.axis {
                Some(axis) => geom::base_rotation(axis.vector(), false),
                None => Quat::from_euler(
                    EulerRot::YXZ,
                    socket.yaw.unwrap_or(0.0).to_radians(),
                    socket.pitch.unwrap_or(0.0).to_radians(),
                    socket.roll.unwrap_or(0.0).to_radians(),
                ),
            };
            self.out.sockets.insert(name.clone(), ResolvedSocket { pos, rot });
        }
    }

    /// Bounds, limits and warnings over the resolved parts.
    fn finish(&mut self) {
        let parts = &self.out.parts;
        if parts.len() > MAX_PARTS {
            let count = parts.len();
            self.issues.push(
                Issue::error("parts", Code::Limit, format!("{count} parts after copies"))
                    .expected(format!("at most {MAX_PARTS}")),
            );
            return;
        }
        let mut meshes = BTreeMap::new();
        for part in parts {
            // The checker's limits keep each generated mesh within geom::MAX_TRIANGLES.
            if let Some(generated) = Generated::of(&part.prim, part.mirrored) {
                meshes.entry(generated.key).or_insert(());
            }
        }
        if meshes.len() > MAX_MESHES {
            self.issues.push(
                Issue::error("parts", Code::Limit, format!("{} different generated shapes", meshes.len()))
                    .expected(format!("at most {MAX_MESHES}"))
                    .fix("reuse sizes and angles: equal tapers, arcs and profiles share one mesh"),
            );
        }
        self.out.bounds =
            parts.iter().map(|p| p.aabb).reduce(Bounds::union).unwrap_or(Bounds { min: Vec3::ZERO, max: Vec3::ZERO });
        self.warnings();
    }

    fn warnings(&mut self) {
        let parts = &self.out.parts;
        let mut warnings = Vec::new();
        let extent = self.out.bounds.size().max_element();
        if !parts.is_empty() && !(0.05..=30.0).contains(&extent) {
            let cm = extent * 100.0;
            warnings.push(Finding {
                code: "unit_suspect",
                ids: Vec::new(),
                line: format!("unit_suspect: the asset is {cm:.1} cm across; check the unit ({})", self.doc.unit.name()),
            });
        }
        // Parts whose boxes touch form groups; more than one group floats.
        let mut group: Vec<usize> = (0..parts.len()).collect();
        fn root(group: &mut [usize], i: usize) -> usize {
            let mut i = i;
            while group[i] != i {
                group[i] = group[group[i]];
                i = group[i];
            }
            i
        }
        // Sweep along x: only boxes that overlap in x are compared.
        let mut order: Vec<usize> = (0..parts.len()).collect();
        order.sort_by(|&i, &j| parts[i].aabb.min.x.total_cmp(&parts[j].aabb.min.x).then(i.cmp(&j)));
        for (k, &i) in order.iter().enumerate() {
            let a = parts[i].aabb;
            for &j in &order[k + 1..] {
                let b = parts[j].aabb;
                if b.min.x > a.max.x + JOIN {
                    break;
                }
                if (a.min - JOIN).cmple(b.max).all() && (b.min - JOIN).cmple(a.max).all() {
                    let (ri, rj) = (root(&mut group, i), root(&mut group, j));
                    group[ri.max(rj)] = ri.min(rj);
                }
            }
        }
        let roots: Vec<usize> = (0..parts.len()).map(|i| root(&mut group, i)).collect();
        let connected = roots.iter().all(|r| *r == roots[0]);
        if !connected {
            let mut components: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
            for (i, r) in roots.iter().enumerate() {
                components.entry(*r).or_default().push(&parts[i].id);
            }
            // Name the smaller groups, up to the lines a reply shows: the biggest is the object.
            let mut sets: Vec<Vec<&str>> = components.into_values().collect();
            sets.sort_by_key(|set| std::cmp::Reverse(set.len()));
            for set in sets.iter().skip(1).take(super::lint::MAX_LINES) {
                let ids: Vec<String> = set.iter().map(|s| s.to_string()).collect();
                warnings.push(Finding {
                    code: "floating",
                    line: format!("floating: {} touch nothing else", ids.join(", ")),
                    ids,
                });
            }
            let more = sets.len().saturating_sub(1 + super::lint::MAX_LINES);
            if more > 0 {
                warnings.push(Finding {
                    code: "floating",
                    ids: Vec::new(),
                    line: format!("floating: {more} more groups touch nothing else"),
                });
            }
        }
        let sdf = parts.iter().filter(|p| matches!(p.prim, Primitive::Sphere { .. } | Primitive::Capsule { .. })).count();
        if sdf > SDF_HEAVY {
            warnings.push(Finding {
                code: "sdf_heavy",
                ids: Vec::new(),
                line: format!(
                    "sdf_heavy: {sdf} spheres and capsules (more than {SDF_HEAVY} draw slowly); use boxes or cylinders for some"
                ),
            });
        }
        // A mirror copy of a part (`leg~x`, not `arm~x.hand`) against its source.
        fn mirror(id: &str) -> Option<&str> {
            id.rsplit_once('~').filter(|(_, axes)| !axes.contains('.')).map(|(source, _)| source)
        }
        if parts.iter().any(|part| mirror(&part.id).is_some()) {
            let sources: HashMap<&str, &ResolvedPart> = parts.iter().map(|part| (part.id.as_str(), part)).collect();
            for part in parts {
                let Some(original) = mirror(&part.id).and_then(|source| sources.get(source)) else { continue };
                if original.pos.distance(part.pos) < 0.0005 {
                    warnings.push(Finding {
                        code: "mirror_overlap",
                        ids: vec![part.id.clone()],
                        line: format!(
                            "mirror_overlap: {} lies on its mirror image; move it off the mirror plane or drop mirror",
                            part.id
                        ),
                    });
                }
            }
        }
        self.frame_turns(&mut warnings);
        self.out.warnings = warnings;
    }

    /// `frame_turn`: a flat from/to board within 10 degrees of vertical, leaning more in x than in
    /// z, has its width turned front-to-back by the end frame.
    fn frame_turns(&self, warnings: &mut Vec<Finding>) {
        fn walk(nodes: &BTreeMap<String, Node>, prefix: &str, out: &mut Vec<String>) {
            for (name, node) in nodes {
                let id = format!("{prefix}{name}");
                if let (Some(from), Some(to), Some(prim)) = (node.from, node.to, node.prim()) {
                    let size = prim.size.as_deref().unwrap_or(&[]);
                    let d = (to - from).normalize_or_zero();
                    let lean = d.y.clamp(-1.0, 1.0).abs().acos().to_degrees();
                    let flat = matches!(prim.kind, PrimType::Box | PrimType::RoundedBox) && size.len() == 2 && size[0] != size[1];
                    if flat && lean > 0.0 && lean <= 10.0 && d.x.abs() > d.z.abs() {
                        out.push(id.clone());
                    }
                }
                if let Some(children) = node.children() {
                    walk(children, &format!("{id}."), out);
                }
            }
        }
        let mut ids = Vec::new();
        walk(&self.doc.parts, "", &mut ids);
        for id in ids {
            warnings.push(Finding {
                code: "frame_turn",
                line: format!(
                    "frame_turn: {id} leans sideways, so its width turns front-to-back; use spin:90 or a vertical axis"
                ),
                ids: vec![id],
            });
        }
    }
}

/// The point of a box's bounding box (corners in the parent frame) at `uvw` in -1..1.
fn box_point(corners: &[Vec3; 8], uvw: Vec3) -> Vec3 {
    let (lo, hi) = aabb(corners.iter().copied()).expect("eight corners");
    (lo + hi) * 0.5 + (hi - lo) * 0.5 * uvw
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::{PropAsset, get};
    use crate::shape::Shape;

    fn doc(text: &str) -> Doc {
        Doc::read(text).unwrap_or_else(|issues| panic!("{}", crate::issue::render(&issues)))
    }

    fn none(_: &str) -> Option<Arc<PropDef>> {
        None
    }

    #[test]
    fn format1_assets_compile_to_exactly_their_own_parts() {
        let mut assets: Vec<Arc<PropAsset>> =
            ["box", "crate", "bench", "lantern"].iter().map(|n| get(&format!("BUILTIN/{n}")).unwrap()).collect();
        for text in [
            include_str!("../../tests/fixtures/props/dogfood_stall_v1.json"),
            include_str!("../../tests/fixtures/props/dogfood_well_v1.json"),
        ] {
            assets.push(Arc::new(PropAsset::parse(text).unwrap()));
        }
        for asset in assets {
            let compiled = compile(&Doc::from_v1(&asset), &none, 0).unwrap();
            assert_eq!(compiled.parts.len(), asset.parts.len());
            for (part, (name, v1)) in compiled.parts.iter().zip(&asset.parts) {
                assert_eq!(&part.id, name);
                assert_eq!(part.pos, v1.pos, "{name}: positions are exact");
                assert_eq!(part.rot, v1.rotation(), "{name}: rotations are exact");
                let expected = match v1.shape {
                    Shape::Box { half } => Primitive::Box { half },
                    Shape::RoundedBox { half, radius } => Primitive::RoundedBox { half, radius },
                    Shape::Sphere { radius } => Primitive::Sphere { radius },
                    Shape::Capsule { half_height, radius } => {
                        Primitive::Capsule { a: -half_height, b: half_height, radius, radius_end: radius }
                    }
                    Shape::Cylinder { half_height, radius } => {
                        Primitive::Cylinder { half_height, radius, radius_end: radius, arc: 360.0, segments: geom::SEGMENTS }
                    }
                };
                assert_eq!(part.prim, expected, "{name}");
                assert_eq!(
                    (part.color.as_str(), part.look, part.emissive, part.solid),
                    (v1.color.as_str(), v1.look, v1.emissive, v1.solid)
                );
                assert!(!part.mirrored && !part.hidden && part.scale == 1.0 && part.tint == 1.0 && part.glow_light);
                let v1_bounds = v1.bounds(1.0);
                assert!(
                    part.aabb.min.cmple(v1_bounds.min + 1e-6).all() && part.aabb.max.cmpge(v1_bounds.max - 1e-6).all(),
                    "{name}"
                );
                assert!(Generated::of(&part.prim, false).is_none(), "{name}: format 1 needs no generated mesh");
            }
        }
    }

    const VARIED: &str = r##"{"format":2,"name":"orchard","unit":"cm","color":"#5a8a3a","parts":{
"tree":{"type":"sphere","radius":40,"pos":[0,150,0],"array":{"count":6,"step":[200,0,0]},"chance":0.5,"jitter":{"pos":[30,0,30],"rot":[180,0,0],"scale":[0.8,1.2],"color":0.2}},
"bed":{"pos":[0,0,300],"pick":2,"parts":{
  "a":{"type":"box","size":[50,20,50]},
  "b":{"type":"box","size":[50,20,50],"pos":[60,0,0]},
  "c":{"type":"box","size":[50,20,50],"pos":[120,0,0]},
  "d":{"type":"box","size":[50,20,50],"pos":[180,0,0]}}}}}"##;

    #[test]
    fn a_seed_varies_chance_jitter_and_pick_and_nothing_else() {
        let orchard = doc(VARIED);
        let variant = |seed| compile(&orchard, &none, seed).unwrap();
        assert_eq!(variant(3), variant(3), "a seed always gives the same parts");
        let seeds: Vec<CompiledProp> = (0..8).map(variant).collect();
        let trees: Vec<usize> = seeds.iter().map(|p| p.parts.iter().filter(|p| p.id.starts_with("tree")).count()).collect();
        assert!(trees.iter().any(|n| *n < 6) && trees.iter().any(|n| *n > 1), "chance keeps some trees: {trees:?}");
        for prop in &seeds {
            assert_eq!(prop.parts.iter().filter(|p| p.id.starts_with("bed.")).count(), 2, "pick keeps two beds");
            for tree in prop.parts.iter().filter(|p| p.id.starts_with("tree")) {
                let i: f32 = tree.id[5..tree.id.len() - 1].parse().unwrap();
                let home = Vec3::new((i - 2.5) * 2.0, 1.5, 0.0);
                assert!((tree.pos - home).abs().cmple(Vec3::new(0.3, 0.0, 0.3) + 1e-5).all(), "{} jitters within 30 cm", tree.id);
                assert!((0.8..=1.2).contains(&tree.scale) && (0.8..=1.2).contains(&tree.tint));
            }
        }
        // Adding a part does not reshuffle the others: their numbers depend only on their own id.
        let mut grown = orchard.clone();
        let mut fence = orchard.parts["bed"].clone();
        fence.pick = None;
        grown.parts.insert("aaa_fence".into(), fence);
        let before = variant(5);
        let after = compile(&grown, &none, 5).unwrap();
        for part in &before.parts {
            assert_eq!(Some(part), after.parts.iter().find(|p| p.id == part.id), "{}", part.id);
        }
        assert!(noise(1, "a", "b") != noise(2, "a", "b") && (-1.0..=1.0).contains(&noise(9, "tree[3]", "pos.x")));
    }

    #[test]
    fn references_inline_their_parts_through_scale_and_mirrors() {
        let lantern = crate::props::def("BUILTIN/lantern").unwrap();
        let deps = |name: &str| (name == "BUILTIN/lantern").then(|| lantern.clone());
        let pair = doc(r#"{"format":2,"name":"pair","unit":"m","parts":{
"lamp":{"ref":"BUILTIN/lantern","pos":[1,0,0],"scale":0.5,"mirror":"x"}}}"#);
        let compiled = compile(&pair, &deps, 0).unwrap();
        let count = lantern.compiled.parts.len();
        assert_eq!(compiled.parts.len(), 2 * count);
        for source in &lantern.compiled.parts {
            let plain = compiled.parts.iter().find(|p| p.id == format!("lamp.{}", source.id)).unwrap();
            let mirrored = compiled.parts.iter().find(|p| p.id == format!("lamp~x.{}", source.id)).unwrap();
            assert!((plain.pos - (Vec3::X + source.pos * 0.5)).length() < 1e-6);
            assert!((mirrored.pos - plain.pos * Vec3::new(-1.0, 1.0, 1.0)).length() < 1e-6);
            assert_eq!((plain.scale, plain.mirrored, mirrored.mirrored), (0.5, false, true));
            assert_eq!(plain.color, source.color);
        }
        assert!(compile(&pair, &none, 0).is_err(), "an unknown reference does not compile");
    }

    #[test]
    fn limits_hold_after_copies() {
        // The checker estimates copies too; past it, the compiler counts them exactly. 17 nodes of
        // 64 copies are 1,088 parts.
        let mut many = doc(r##"{"format":2,"name":"many","unit":"cm","color":"#808080","parts":{
"n0":{"type":"box","size":[1,1,1],"array":{"count":64,"step":[2,0,0]}}}}"##);
        let row = many.parts["n0"].clone();
        for i in 1..17 {
            many.parts.insert(format!("n{i}"), Node { pos: Some(Vec3::new(0.0, i as f32, 0.0)), ..row.clone() });
        }
        let issues = compile(&many, &none, 0).unwrap_err();
        assert_eq!(crate::issue::render(&issues), "parts: 1088 parts after copies (expected at most 1024)");
        // Every taper is its own mesh: 65 distinct ones are refused.
        let cones: Vec<String> = (0..65)
            .map(|i| {
                format!(
                    r#""c{i}":{{"type":"cone","radius":10,"radius_end":{},"height":10,"pos":[{},0,0]}}"#,
                    i as f32 * 0.1 + 0.1,
                    i * 30
                )
            })
            .collect();
        let text = format!(r##"{{"format":2,"name":"cones","unit":"cm","color":"#808080","parts":{{{}}}}}"##, cones.join(","));
        let issues = compile(&doc(&text), &none, 0).unwrap_err();
        assert!(crate::issue::render(&issues).starts_with("parts: 65 different generated shapes (expected at most 64)"));
    }

    #[test]
    fn a_1024_part_asset_compiles_quickly() {
        let kinds = [
            r#""type":"box","size":[4,4,4]"#,
            r#""type":"cylinder","radius":2,"height":4"#,
            r#""type":"cone","radius":2,"radius_end":1,"height":4"#,
            r#""type":"tube","radius":2,"thickness":0.5,"height":4,"arc":180"#,
        ];
        // Rows of stacked parts that touch, as a big real asset would.
        let groups: Vec<String> = (0..4)
            .map(|g| {
                let children: Vec<String> = (0..4).map(|c| format!(r#""p{c}":{{{},"pos":[0,{},0]}}"#, kinds[c], c * 4)).collect();
                format!(
                    r#""g{g}":{{"pos":[0,0,{}],"mat":"m","array":{{"count":64,"step":[4,0,0]}},"parts":{{{}}}}}"#,
                    g * 4,
                    children.join(",")
                )
            })
            .collect();
        let text = format!(
            r##"{{"format":2,"name":"stress","unit":"cm","materials":{{"m":{{"color":"#808080"}}}},"parts":{{{}}}}}"##,
            groups.join(",")
        );
        let stress = doc(&text);
        let mut best = std::time::Duration::MAX;
        for _ in 0..5 {
            let start = std::time::Instant::now();
            let compiled = compile(&stress, &none, 0).unwrap();
            best = best.min(start.elapsed());
            assert_eq!(compiled.parts.len(), 1024);
            assert_eq!(compiled.warnings, []);
        }
        // The plan's budget is 1 ms in release builds; debug builds get room for opt-level 1.
        let budget = if cfg!(debug_assertions) { 30.0 } else { 1.0 };
        assert!(best.as_secs_f64() * 1000.0 < budget, "1,024 parts took {best:?}");
    }
}
