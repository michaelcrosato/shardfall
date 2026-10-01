//! The world: a pavilion hub at the centre (plaza + four corridors), rooms placed along the
//! corridors, and an infinite procedural wilderness around it. Everything streams around
//! interest points (the player, plus any extra points agents add), never around the camera.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use glam::{Quat, Vec2, Vec3};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::entity::{BodyKind, Entity, EntityId, Spawn};
use crate::frame::SimEvent;
use crate::level::Placement;
use crate::params::{self, ParamValue, Tunable};
use crate::physics::entity_tag;
use crate::room::RoomDef;
use crate::shape::{Look, Shape, Visual};
use crate::sim::{Sim, SimConfig};
use crate::statics::{Block, CHUNK_SIZE, Decor, Facing, RegionKey, chunk_ivec};
use crate::terrain::{Footprint, TerrainGen};

pub const PLAZA_HALF: f32 = 14.0;
pub const CORRIDOR_HALF: f32 = 3.0;
const ROOM_GAP: f32 = 3.0;
/// Rooms and the hub stay active within this distance of an interest point (m).
const ROOM_RADIUS: f32 = 40.0;

/// Corridor directions: east, north, west, south.
const DIRS: [Facing; 4] = [Facing::East, Facing::North, Facing::West, Facing::South];

/// Which corridor a wing lives on.
pub fn wing_direction(wing: &str) -> Facing {
    match wing {
        "movement" | "aesthetic" => Facing::East,
        "physics" | "genre" => Facing::North,
        "animation" => Facing::West,
        _ => Facing::South,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoomSlot {
    pub id: u16,
    pub key: String,
    pub def: Arc<RoomDef>,
    pub place: Placement,
    pub min: Vec3,
    pub max: Vec3,
    /// Just inside the door / just outside it in the corridor (feet positions).
    pub inside: Vec3,
    pub outside: Vec3,
    /// Direction from outside to inside.
    pub inward: Vec3,
    pub built: bool,
}

impl RoomSlot {
    pub fn contains(&self, p: Vec3) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.z >= self.min.z
            && p.z <= self.max.z
            && p.y > self.min.y - 2.0
            && p.y < self.max.y + 4.0
    }
    pub fn distance(&self, p: Vec3) -> f32 {
        let d = (self.min - p).max(p - self.max).max(Vec3::ZERO);
        Vec2::new(d.x, d.z).length()
    }
}

/// A sleeping entity (its region is dormant).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DormantEntity {
    pub entity: Entity,
    pub linvel: Vec3,
    pub angvel: Vec3,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct World {
    /// False for small test scenes (no hub, no terrain, no streaming).
    pub enabled: bool,
    pub rooms: Vec<RoomSlot>,
    pub hub_min: Vec3,
    pub hub_max: Vec3,
    pub hub_built: bool,
    pub footprint: Vec<(Vec2, Vec2)>,
    /// Corridor ends (feet positions) leading out into the wilderness.
    pub exits: Vec<Vec3>,
    pub current_room: Option<u16>,
    /// Values replaced by the current room's overrides (restored on exit).
    pub saved_params: BTreeMap<String, ParamValue>,
    /// Extra streaming interest points (agents).
    pub interest: Vec<Vec3>,
    pub dormant_entities: BTreeMap<RegionKey, Vec<DormantEntity>>,
    /// Terrain chunks that already spawned their loose props.
    pub props_spawned: BTreeSet<(i32, i32)>,
    /// Problems from loading room files.
    pub errors: Vec<String>,
}

impl World {
    pub fn room(&self, key: &str) -> Option<&RoomSlot> {
        self.rooms.iter().find(|r| r.key == key)
    }
    pub fn footprint(&self) -> Footprint {
        Footprint { rects: self.footprint.clone() }
    }
}

/// Lays out the pavilion: places every room along its wing's corridor.
pub fn layout_pavilion(defs: &[(String, RoomDef)]) -> World {
    let mut w = World { enabled: true, ..Default::default() };
    let mut sorted: Vec<&(String, RoomDef)> = defs.iter().collect();
    let wing_rank =
        |w: &str| ["movement", "physics", "animation", "vfx", "aesthetic", "genre"].iter().position(|x| *x == w).unwrap_or(9);
    sorted.sort_by(|a, b| (wing_rank(&a.1.wing), a.1.order, &a.0).cmp(&(wing_rank(&b.1.wing), b.1.order, &b.0)));
    // cursor[direction][side] = distance along the corridor already used
    let mut cursor = [[ROOM_GAP; 2]; 4];
    for (key, def) in sorted {
        let di = DIRS.iter().position(|d| *d == wing_direction(&def.wing)).unwrap_or(3);
        let d = DIRS[di].dir();
        let side = if cursor[di][0] <= cursor[di][1] { 0 } else { 1 };
        // Normal pointing from the corridor into the room's side.
        let n = if side == 0 { Vec3::new(-d.z, 0.0, d.x) } else { Vec3::new(d.z, 0.0, -d.x) };
        // Rotate so the entrance faces the corridor (-n).
        let want = [Facing::North, Facing::East, Facing::South, Facing::West]
            .into_iter()
            .find(|f| f.dir().abs_diff_eq(-n, 1e-4))
            .unwrap_or(Facing::South);
        let q = (0..4u8).find(|q| def.entrance.facing.rotated(*q) == want).unwrap_or(0);
        let (cols, rows) = def.layout.extent();
        let rot = Placement::new(Vec3::ZERO, q);
        let (lo, hi) = rot.aabb(Vec3::ZERO, Vec3::new(cols as f32, 0.0, rows as f32));
        let span_d = (hi - lo).dot(d.abs());
        // The door should sit at the room's edge facing the corridor, roughly centred is not
        // required: we align the room's near edge with the corridor wall.
        let start = PLAZA_HALF + cursor[di][side];
        let corners = [lo, hi, Vec3::new(lo.x, 0.0, hi.z), Vec3::new(hi.x, 0.0, lo.z)];
        let min_d = corners.iter().map(|c| c.dot(d)).fold(f32::MAX, f32::min);
        let min_n = corners.iter().map(|c| c.dot(n)).fold(f32::MAX, f32::min);
        let origin = d * (start - min_d) + n * (CORRIDOR_HALF + 0.0 - min_n);
        let place = Placement::new(origin, q);
        let (wmin, wmax) = place.aabb(Vec3::new(0.0, -1.0, 0.0), Vec3::new(cols as f32, 8.0, rows as f32));
        let [ec, er] = def.entrance.at;
        let door = place.point(Vec3::new(ec as f32 + 0.5, 0.0, er as f32 + 0.5));
        let inward = n;
        w.rooms.push(RoomSlot {
            id: w.rooms.len() as u16,
            key: key.clone(),
            def: Arc::new(def.clone()),
            place,
            min: wmin,
            max: wmax,
            inside: door + inward * 1.6,
            outside: door - inward * 2.5,
            inward,
            built: false,
        });
        cursor[di][side] += span_d + ROOM_GAP;
    }
    // Hub bounds: plaza plus corridors long enough for their rooms.
    let mut lo = Vec3::new(-PLAZA_HALF, -1.0, -PLAZA_HALF);
    let mut hi = Vec3::new(PLAZA_HALF, 6.0, PLAZA_HALF);
    w.footprint.push((Vec2::splat(-PLAZA_HALF), Vec2::splat(PLAZA_HALF)));
    for (di, f) in DIRS.iter().enumerate() {
        let d = f.dir();
        let len = PLAZA_HALF + cursor[di][0].max(cursor[di][1]) + 6.0;
        let side = Vec3::new(-d.z, 0.0, d.x) * CORRIDOR_HALF;
        let a = d * PLAZA_HALF - side;
        let b = d * len + side;
        let (cmin, cmax) = (a.min(b), a.max(b));
        lo = lo.min(cmin);
        hi = hi.max(cmax + Vec3::Y * 6.0);
        w.footprint.push((Vec2::new(cmin.x, cmin.z), Vec2::new(cmax.x, cmax.z)));
        w.exits.push(d * (len + 1.5));
    }
    for r in &w.rooms {
        w.footprint.push((Vec2::new(r.min.x - 0.5, r.min.z - 0.5), Vec2::new(r.max.x + 0.5, r.max.z + 0.5)));
    }
    w.hub_min = lo;
    w.hub_max = hi;
    w
}

/// Corridor length along direction index (from the layout's exits).
fn corridor_len(w: &World, di: usize) -> f32 {
    w.exits[di].length() - 1.5
}

fn hub_blocks(w: &World) -> (Vec<Block>, Vec<Decor>) {
    let mut blocks = Vec::new();
    let mut decor = Vec::new();
    let stone_a = Color::hex("#d8d2c4");
    let stone_b = Color::hex("#cbc3b2");
    let wall = Color::hex("#e2d6bd");
    // Plaza floor: 4x4 big tiles.
    let t = PLAZA_HALF * 2.0 / 4.0;
    for i in 0..4 {
        for j in 0..4 {
            let x0 = -PLAZA_HALF + i as f32 * t;
            let z0 = -PLAZA_HALF + j as f32 * t;
            let c = if (i + j) % 2 == 0 { stone_a } else { stone_b };
            blocks.push(Block::new(Vec3::new(x0, -0.5, z0), Vec3::new(x0 + t, 0.0, z0 + t), c));
        }
    }
    // Fountain.
    decor.push(Decor {
        shape: Shape::Cylinder { half_height: 0.3, radius: 2.4 },
        pos: Vec3::new(0.0, 0.3, 0.0),
        rot: Quat::IDENTITY,
        color: Color::hex("#bfb6a3"),
        look: Look::Cel,
        emissive: 0.0,
        solid: true,
        collider: None,
    });
    decor.push(Decor {
        shape: Shape::Cylinder { half_height: 0.05, radius: 2.0 },
        pos: Vec3::new(0.0, 0.58, 0.0),
        rot: Quat::IDENTITY,
        color: Color::hex("#5d9bd6"),
        look: Look::Flat,
        emissive: 0.0,
        solid: false,
        collider: None,
    });
    decor.push(Decor {
        shape: Shape::Sphere { radius: 0.45 },
        pos: Vec3::new(0.0, 1.4, 0.0),
        rot: Quat::IDENTITY,
        color: Color::hex("#ffe2a0"),
        look: Look::Unlit,
        emissive: 1.4,
        solid: false,
        collider: None,
    });
    decor.push(Decor {
        shape: Shape::Cylinder { half_height: 0.5, radius: 0.18 },
        pos: Vec3::new(0.0, 0.9, 0.0),
        rot: Quat::IDENTITY,
        color: Color::hex("#bfb6a3"),
        look: Look::Cel,
        emissive: 0.0,
        solid: true,
        collider: None,
    });
    // Plaza perimeter: low walls with openings for the corridors.
    for f in DIRS {
        let d = f.dir();
        let side = Vec3::new(-d.z, 0.0, d.x);
        for s in [-1.0f32, 1.0] {
            // From the corridor opening to the plaza corner.
            let a = d * PLAZA_HALF + side * s * (CORRIDOR_HALF + 0.0);
            let b = d * (PLAZA_HALF - 0.6) + side * s * PLAZA_HALF;
            blocks.push(Block::new(a.min(b), a.max(b) + Vec3::Y * 0.9, wall));
        }
    }
    // Corridors: floor, low side walls with door gaps, lamps.
    let floor = Color::hex("#c9bfa9");
    for (di, f) in DIRS.iter().enumerate() {
        let d = f.dir();
        let side = Vec3::new(-d.z, 0.0, d.x);
        let len = corridor_len(w, di);
        let a = d * PLAZA_HALF - side * CORRIDOR_HALF;
        let b = d * len + side * CORRIDOR_HALF;
        blocks.push(Block::new(a.min(b) - Vec3::Y * 0.5, a.max(b), floor));
        for s in [-1.0f32, 1.0] {
            // Door gaps on this side (positions along d).
            let mut gaps: Vec<f32> = w
                .rooms
                .iter()
                .filter(|r| (r.inward - side * s).length() < 1e-3 && (r.outside - Vec3::ZERO).dot(d) > PLAZA_HALF - 1.0)
                .map(|r| r.outside.dot(d))
                .filter(|g| {
                    // only rooms on this corridor
                    *g > PLAZA_HALF - 1.0 && *g < len + 1.0
                })
                .collect();
            gaps.retain(|g| {
                w.rooms.iter().any(|r| (r.outside.dot(d) - g).abs() < 1e-3 && r.outside.dot(side).abs() < CORRIDOR_HALF + 3.0)
            });
            gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut start = PLAZA_HALF;
            let mut segs = Vec::new();
            for g in gaps {
                segs.push((start, g - 1.6));
                start = g + 1.6;
            }
            segs.push((start, len));
            for (s0, s1) in segs {
                if s1 - s0 < 0.3 {
                    continue;
                }
                let p0 = d * s0 + side * s * CORRIDOR_HALF;
                let p1 = d * s1 + side * s * (CORRIDOR_HALF - 0.4);
                blocks.push(Block::new(p0.min(p1), p0.max(p1) + Vec3::Y * 0.8, wall));
            }
        }
        let mut x = PLAZA_HALF + 4.0;
        while x < len {
            for s in [-1.0f32, 1.0] {
                let p = d * x + side * s * (CORRIDOR_HALF - 0.2);
                decor.push(Decor {
                    shape: Shape::Cylinder { half_height: 1.1, radius: 0.07 },
                    pos: p + Vec3::Y * 1.1,
                    rot: Quat::IDENTITY,
                    color: Color::hex("#5b5f66"),
                    look: Look::Cel,
                    emissive: 0.0,
                    solid: true,
                    collider: None,
                });
                decor.push(Decor {
                    shape: Shape::Sphere { radius: 0.16 },
                    pos: p + Vec3::Y * 2.3,
                    rot: Quat::IDENTITY,
                    color: Color::hex("#ffe7b0"),
                    look: Look::Unlit,
                    emissive: 1.0,
                    solid: false,
                    collider: None,
                });
            }
            x += 10.0;
        }
    }
    (blocks, decor)
}

impl Sim {
    /// Sets up the full world (hub, rooms, wilderness) and spawns the player on the plaza.
    pub fn build_world(&mut self, defs: Vec<(String, RoomDef)>, errors: Vec<(String, String)>) {
        let mut w = layout_pavilion(&defs);
        w.errors = errors.into_iter().map(|(k, e)| format!("{k}: {e}")).collect();
        self.state.world = w;
        self.state.spawn = Vec3::new(0.0, 0.0, 6.0);
        self.spawn_player();
        self.state.focus = self.state.spawn;
        self.update_streaming(usize::MAX);
    }

    /// Builds a room's content into its region (layout + objects).
    pub fn build_room(&mut self, id: u16) {
        let Some(slot) = self.state.world.rooms.get(id as usize).cloned() else { return };
        let region = RegionKey::Room(id);
        let built = slot.def.layout.build_at(self, &slot.place, Some(region));
        for warn in built.warnings {
            log::warn!("room {}: {warn}", slot.key);
        }
        let mut named: BTreeMap<String, EntityId> = BTreeMap::new();
        for o in &slot.def.objects {
            let mut v = Visual::new(o.shape, Color::hex(&o.color));
            v.look = o.look;
            v.emissive = o.emissive;
            let rot = slot.place.quat() * o.local_rot();
            let mut sp = Spawn::new(if o.name.is_empty() { "object" } else { &o.name }, slot.place.point(o.pos))
                .visual(v)
                .body(o.body)
                .rot(rot)
                .behavior(o.behavior.clone())
                .density(o.density)
                .friction(o.friction)
                .restitution(o.restitution);
            sp.region = Some(region);
            sp.hazard = o.hazard.clone();
            sp.soft = o.soft.clone();
            let id = self.spawn(sp);
            if !o.name.is_empty() {
                named.entry(o.name.clone()).or_insert(id);
            }
        }
        self.tie_soft_bodies(&named);
        for j in &slot.def.joints {
            let Some(&a) = named.get(&j.a) else {
                log::warn!("room {}: joint object '{}' not found", slot.key, j.a);
                continue;
            };
            let b = if j.b.is_empty() { None } else { named.get(&j.b).copied() };
            if !j.b.is_empty() && b.is_none() {
                log::warn!("room {}: joint object '{}' not found", slot.key, j.b);
                continue;
            }
            let mut kind = j.joint.clone();
            match &mut kind {
                crate::joints::JointKind::Hinge { axis, .. } | crate::joints::JointKind::Slider { axis, .. } => {
                    *axis = slot.place.rotate(*axis)
                }
                _ => {}
            }
            let pose = |id: EntityId| self.state.entities.get(id).map(|e| (e.pos, e.rot));
            let Some(pa) = pose(a) else { continue };
            let pb = b.and_then(|b| pose(b).map(|(p, q)| (b, p, q)));
            let link = crate::joints::JointLink::at(kind, slot.place.point(j.at), pa, pb);
            self.add_joint(a, link);
        }
        for c in &slot.def.chains {
            self.build_chain(c, &slot.place, region, &named);
        }
        for l in &slot.def.labels {
            if let Some(p) = l.pos {
                let label = l.to_label(&slot.place, p);
                self.state.statics.add_label_to(region, label);
            }
        }
        self.state.world.rooms[id as usize].built = true;
    }

    /// Ties soft bodies with `attach` to the named object's body.
    fn tie_soft_bodies(&mut self, named: &BTreeMap<String, EntityId>) {
        let ties: Vec<(EntityId, String)> = self
            .state
            .entities
            .iter()
            .filter_map(|e| e.soft.as_ref().filter(|s| !s.def.attach.is_empty()).map(|s| (e.id, s.def.attach.clone())))
            .filter(|(_, n)| named.contains_key(n))
            .collect();
        for (id, name) in ties {
            let Some(body) = named.get(&name).and_then(|o| self.state.entities.get(*o)).and_then(|e| e.body) else { continue };
            let Some(part) = self.state.entities.get(id).and_then(|e| e.soft.clone()) else { continue };
            let ph = &mut self.state.physics;
            if let Some(sb) = part.handle.and_then(|h| ph.soft_bodies.get_mut(h)) {
                for &i in &part.attach_particles {
                    sb.attach_particle(i as usize, body, &ph.bodies);
                }
            }
        }
    }

    /// A chain of capsule links, or a rope bridge of hinged planks, between two points.
    fn build_chain(&mut self, c: &crate::room::ChainDef, place: &Placement, region: RegionKey, named: &BTreeMap<String, EntityId>) {
        use crate::joints::{JointKind, JointLink};
        use crate::room::ChainStyle;
        let (from, to) = (place.point(c.from), place.point(c.to));
        let n = c.links.max(1) as usize;
        let span = (to - from).length();
        let dip = c.sag * span;
        let point = |t: f32| from.lerp(to, t) - Vec3::Y * 4.0 * dip * t * (1.0 - t);
        let pts: Vec<Vec3> = (0..=n).map(|i| point(i as f32 / n as f32)).collect();
        let color = Color::hex(&c.color);
        let bridge = c.style == ChainStyle::Bridge;
        // Lateral direction for bridges (horizontal, across the span).
        let along = (to - from).normalize_or(Vec3::X);
        let side = Vec3::Y.cross(along).normalize_or(Vec3::Z) * (c.width * 0.5);
        let mut ids: Vec<EntityId> = Vec::with_capacity(n);
        for i in 0..n {
            let (a, b) = (pts[i], pts[i + 1]);
            let dir = (b - a).normalize_or(along);
            let len = (b - a).length();
            let mid = (a + b) * 0.5;
            let (shape, rot, name) = if bridge {
                let rot = Quat::from_mat3(&glam::Mat3::from_cols(dir, dir.cross(side.normalize()).normalize() * -1.0, side.normalize()))
                    .normalize();
                (Shape::Box { half: Vec3::new(len * 0.46, c.radius * 0.5, c.width * 0.5) }, rot, "~plank")
            } else {
                let rot = Quat::from_rotation_arc(Vec3::Y, dir);
                (Shape::Capsule { half_height: (len * 0.5 - c.radius).max(0.01), radius: c.radius }, rot, "~link")
            };
            let mut sp = Spawn::new(name, mid).visual(Visual::new(shape, color)).body(BodyKind::Dynamic).rot(rot).density(c.density);
            sp.region = Some(region);
            ids.push(self.spawn(sp));
        }
        let pose = |s: &Sim, id: EntityId| s.state.entities.get(id).map(|e| (e.pos, e.rot)).unwrap_or_default();
        // Anchors: one in the middle for chains, two (left/right edges) for bridges.
        let anchors = |p: Vec3| if bridge { vec![p - side, p + side] } else { vec![p] };
        for i in 0..n.saturating_sub(1) {
            let (a, b) = (ids[i], ids[i + 1]);
            for p in anchors(pts[i + 1]) {
                let link = JointLink::at(JointKind::Ball, p, pose(self, a), Some((b, pose(self, b).0, pose(self, b).1)));
                self.add_joint(a, link);
            }
        }
        let fix_to = c.fix_to.unwrap_or(bridge);
        let ends = [(ids[0], pts[0], c.fix_from, &c.attach_from), (ids[n - 1], pts[n], fix_to, &c.attach_to)];
        for (id, p, fix, attach) in ends {
            let other = named.get(attach.as_str()).copied();
            if other.is_none() && !fix {
                continue;
            }
            for q in anchors(p) {
                let pb = other.map(|o| (o, pose(self, o).0, pose(self, o).1));
                let link = JointLink::at(JointKind::Ball, q, pose(self, id), pb);
                self.add_joint(id, link);
            }
        }
    }

    /// Removes a room's content (statics and its entities, active or dormant).
    fn clear_room(&mut self, id: u16) {
        let region = RegionKey::Room(id);
        let st = &mut self.state;
        st.statics.remove_region(&mut st.physics, region);
        st.world.dormant_entities.remove(&region);
        let ids: Vec<EntityId> = st.entities.iter().filter(|e| e.region == Some(region)).map(|e| e.id).collect();
        for id in ids {
            self.despawn(id);
        }
        if let Some(r) = self.state.world.rooms.get_mut(id as usize) {
            r.built = false;
        }
    }

    /// Rebuilds a room from its definition (the player is moved inside the entrance if they
    /// were in it).
    pub fn reset_room(&mut self, id: u16) {
        if !self.state.world.enabled {
            let _ = self.reset();
            return;
        }
        self.clear_room(id);
        self.build_room(id);
        if self.state.world.current_room == Some(id) {
            if let Some(slot) = self.state.world.rooms.get(id as usize) {
                let (p, inward) = (slot.inside, slot.inward);
                if let Some(pid) = self.state.player {
                    self.set_position(pid, p);
                    self.face(pid, inward);
                }
            }
        }
    }

    /// Replaces a room's definition (hot reload) and rebuilds it.
    pub fn replace_room(&mut self, key: &str, def: RoomDef) -> bool {
        let Some(id) = self.state.world.rooms.iter().position(|r| r.key == key) else { return false };
        let old = self.state.world.rooms[id].clone();
        let (oc, or) = old.def.layout.extent();
        let (nc, nr) = def.layout.extent();
        if (oc, or) != (nc, nr) || old.def.entrance != def.entrance || old.def.wing != def.wing {
            // Footprint changed: lay the whole pavilion out again.
            let defs: Vec<(String, RoomDef)> = self
                .state
                .world
                .rooms
                .iter()
                .map(|r| (r.key.clone(), if r.key == key { def.clone() } else { (*r.def).clone() }))
                .collect();
            self.rebuild_pavilion(defs);
            return true;
        }
        self.state.world.rooms[id].def = Arc::new(def);
        let was_built = old.built;
        self.clear_room(id as u16);
        if was_built {
            self.build_room(id as u16);
        }
        true
    }

    /// Lays out the pavilion again (rooms added/removed/resized). Terrain near it regenerates.
    pub fn rebuild_pavilion(&mut self, defs: Vec<(String, RoomDef)>) {
        let player_room =
            self.state.world.current_room.and_then(|i| self.state.world.rooms.get(i as usize)).map(|r| (r.key.clone(), r.place));
        for id in 0..self.state.world.rooms.len() as u16 {
            self.clear_room(id);
        }
        let st = &mut self.state;
        st.statics.remove_region(&mut st.physics, RegionKey::Hub);
        // Terrain around the old and new footprint is regenerated.
        let keys: Vec<RegionKey> = st
            .statics
            .chunks
            .keys()
            .chain(st.statics.dormant.keys())
            .copied()
            .filter(|k| matches!(k, RegionKey::Chunk(..)))
            .collect();
        for k in keys {
            st.statics.remove_region(&mut st.physics, k);
        }
        let mut w = layout_pavilion(&defs);
        w.errors = std::mem::take(&mut st.world.errors);
        w.dormant_entities = std::mem::take(&mut st.world.dormant_entities);
        w.dormant_entities.retain(|k, _| matches!(k, RegionKey::Chunk(..)));
        w.props_spawned = std::mem::take(&mut st.world.props_spawned);
        w.interest = std::mem::take(&mut st.world.interest);
        st.world = w;
        // Keep the player in the same spot of their room if it moved.
        if let (Some((key, old)), Some(pid)) = (player_room, self.state.player) {
            if let Some(new) = self.state.world.room(&key).map(|r| r.place) {
                if let Some(e) = self.state.entities.get(pid) {
                    let h = e.character.as_ref().map(|c| c.height() * 0.5).unwrap_or(0.0);
                    let local = old.quat().inverse() * (e.pos - Vec3::Y * h - old.origin);
                    let p = new.point(local);
                    self.set_position(pid, p);
                }
            }
        }
        self.state.world.current_room = None;
        self.update_streaming(usize::MAX);
    }

    pub fn face(&mut self, id: EntityId, dir: Vec3) {
        if let Some(ch) = self.state.entities.get_mut(id).and_then(|e| e.character.as_mut()) {
            ch.facing = dir.x.atan2(dir.z);
            ch.anim.facing = ch.facing;
        }
    }

    /// Teleports the player just inside a room's entrance.
    pub fn teleport_to_room(&mut self, key: &str) -> bool {
        let Some(slot) = self.state.world.room(key).cloned() else { return false };
        let Some(pid) = self.state.player else { return false };
        self.state.focus = slot.inside;
        self.state.world.interest.push(slot.inside);
        self.update_streaming(usize::MAX);
        self.state.world.interest.pop();
        self.set_position(pid, slot.inside);
        self.face(pid, slot.inward);
        true
    }

    /// Teleports the player to the plaza (or just outside the current room's door).
    pub fn leave_room(&mut self) -> bool {
        let Some(pid) = self.state.player else { return false };
        let (p, dir) = match self.state.world.current_room.and_then(|i| self.state.world.rooms.get(i as usize)) {
            Some(r) => (r.outside, -r.inward),
            None => (Vec3::new(0.0, 0.0, 6.0), Vec3::Z),
        };
        self.set_position(pid, p);
        self.face(pid, dir);
        true
    }

    /// Loads/wakes regions near interest points and puts far ones to sleep. `budget` limits
    /// expensive operations (generation/activation) per call.
    pub fn update_streaming(&mut self, budget: usize) {
        if !self.state.world.enabled {
            return;
        }
        let mut points: Vec<Vec3> = self.state.world.interest.clone();
        if let Some(p) = self.player() {
            points.push(p.pos);
        }
        if points.is_empty() {
            points.push(self.state.focus);
        }
        let mut budget = budget;
        let tp = self.config.terrain.clone();

        // Hub.
        let w = &self.state.world;
        let hub_near = points.iter().any(|p| {
            let d = (w.hub_min - *p).max(*p - w.hub_max).max(Vec3::ZERO);
            Vec2::new(d.x, d.z).length() < ROOM_RADIUS
        });
        if hub_near && !self.state.statics.is_active(RegionKey::Hub) {
            let st = &mut self.state;
            if !st.statics.activate(&mut st.physics, RegionKey::Hub) {
                let (blocks, decor) = hub_blocks(&st.world);
                for b in blocks {
                    st.statics.add_to(&mut st.physics, RegionKey::Hub, b);
                }
                for d in decor {
                    st.statics.add_decor(&mut st.physics, RegionKey::Hub, d);
                }
                // Room names on the corridor floor in front of each door.
                for r in &st.world.rooms {
                    let name = if r.def.name.is_empty() { r.key.clone() } else { r.def.name.clone() };
                    st.statics.add_label_to(
                        RegionKey::Hub,
                        crate::zones::Label {
                            text: name,
                            pos: r.outside + r.inward * 0.6 + Vec3::Y * 0.03,
                            size: 0.55,
                            color: Color::hex("#5a4a36"),
                            mode: crate::zones::LabelMode::Floor,
                            facing: Facing::South,
                        },
                    );
                }
            }
            self.state.world.hub_built = true;
        } else if !hub_near && self.state.statics.is_active(RegionKey::Hub) {
            let st = &mut self.state;
            st.statics.deactivate(&mut st.physics, RegionKey::Hub);
        }

        // Rooms.
        for id in 0..self.state.world.rooms.len() as u16 {
            let slot = &self.state.world.rooms[id as usize];
            let near = points.iter().map(|p| slot.distance(*p)).fold(f32::MAX, f32::min);
            let key = RegionKey::Room(id);
            let active = self.state.statics.is_active(key);
            if near < ROOM_RADIUS && !active {
                if !slot.built {
                    self.build_room(id);
                } else {
                    let st = &mut self.state;
                    st.statics.activate(&mut st.physics, key);
                    self.wake_entities(key);
                }
            } else if near > ROOM_RADIUS + 16.0 && active {
                self.sleep_region(key, None);
            }
        }

        // Terrain chunks.
        let foot = self.state.world.footprint();
        let mut wanted: BTreeSet<(i32, i32)> = BTreeSet::new();
        for p in &points {
            let c = chunk_ivec(*p);
            for dx in -tp.load_radius..=tp.load_radius {
                for dz in -tp.load_radius..=tp.load_radius {
                    wanted.insert((c.x + dx, c.y + dz));
                }
            }
        }
        // Nearest first.
        let mut order: Vec<(i32, i32)> = wanted.iter().copied().collect();
        let p0 = points[0];
        order.sort_by_key(|(x, z)| {
            let c = Vec3::new((*x as f32 + 0.5) * CHUNK_SIZE, 0.0, (*z as f32 + 0.5) * CHUNK_SIZE);
            (c.distance_squared(p0) * 10.0) as i64
        });
        for (x, z) in order {
            let key = RegionKey::Chunk(x, z);
            if self.state.statics.is_active(key) {
                continue;
            }
            if budget == 0 {
                break;
            }
            budget -= 1;
            let st = &mut self.state;
            if st.statics.activate(&mut st.physics, key) {
                self.wake_entities(key);
                continue;
            }
            let tg = TerrainGen { seed: self.state.seed, p: &tp, footprint: &foot };
            let patch = tg.generate(x, z);
            let decor = tg.decor(x, z);
            let props = if self.state.world.props_spawned.insert((x, z)) { tg.props(x, z) } else { Vec::new() };
            let st = &mut self.state;
            st.statics.set_terrain(&mut st.physics, key, Arc::new(patch));
            for d in decor {
                st.statics.add_decor(&mut st.physics, key, d);
            }
            for p in props {
                let rng = &mut self.state.rng;
                let crate_like = rng.below(2) == 0;
                let (shape, color) = if crate_like {
                    (Shape::Box { half: Vec3::splat(0.35) }, Color::hex("#c98a4b"))
                } else {
                    (Shape::Cylinder { half_height: 0.45, radius: 0.32 }, Color::hex("#a65d3f"))
                };
                let mut sp = Spawn::new("wild_prop", p + Vec3::Y * shape.half_extents().y)
                    .visual(Visual::new(shape, color))
                    .body(BodyKind::Dynamic);
                sp.region = Some(key);
                self.spawn(sp);
            }
        }
        let far: Vec<RegionKey> = self
            .state
            .statics
            .chunks
            .keys()
            .copied()
            .filter(|k| match k {
                RegionKey::Chunk(x, z) => points.iter().all(|p| {
                    let c = chunk_ivec(*p);
                    (x - c.x).abs() > tp.unload_radius || (z - c.y).abs() > tp.unload_radius
                }),
                _ => false,
            })
            .collect();
        for k in far {
            if let RegionKey::Chunk(x, z) = k {
                let lo = Vec3::new(x as f32 * CHUNK_SIZE, -100.0, z as f32 * CHUNK_SIZE);
                self.sleep_region(k, Some((lo, lo + Vec3::new(CHUNK_SIZE, 300.0, CHUNK_SIZE))));
            }
        }
    }

    /// Puts a region to sleep: statics go dormant, its entities are stored. For terrain chunks
    /// the entities inside `area` (any loose prop) are stored with it.
    fn sleep_region(&mut self, key: RegionKey, area: Option<(Vec3, Vec3)>) {
        let player = self.state.player;
        let ids: Vec<EntityId> = self
            .state
            .entities
            .iter()
            .filter(|e| Some(e.id) != player && e.character.is_none() && e.bomb.is_none() && e.lifetime.is_none())
            .filter(|e| match area {
                Some((lo, hi)) => {
                    let p = e.pos;
                    let inside = p.x >= lo.x && p.x < hi.x && p.z >= lo.z && p.z < hi.z;
                    inside && !matches!(e.region, Some(RegionKey::Room(_)) | Some(RegionKey::Hub))
                }
                None => e.region == Some(key),
            })
            .map(|e| e.id)
            .collect();
        let mut stored = Vec::new();
        for id in ids {
            let Some(mut e) = self.state.entities.map.remove(&id) else { continue };
            let (mut linvel, mut angvel) = (Vec3::ZERO, Vec3::ZERO);
            if let Some(h) = e.body.take() {
                if let Some(b) = self.state.physics.bodies.get(h) {
                    linvel = b.linvel();
                    angvel = b.angvel();
                }
                self.state.physics.remove_body(h);
            }
            if let Some(part) = &mut e.soft {
                if let Some(h) = part.handle.take() {
                    if let Some(sb) = self.state.physics.soft_bodies.get(h) {
                        part.saved = sb.particle_positions().zip(sb.particle_velocities()).collect();
                    }
                    self.state.physics.remove_soft(h);
                }
            }
            e.region = Some(key);
            stored.push(DormantEntity { entity: e, linvel, angvel });
        }
        let st = &mut self.state;
        let modified = st.statics.chunks.get(&key).is_some_and(|c| c.modified);
        if matches!(key, RegionKey::Chunk(..)) && stored.is_empty() && !modified && !st.world.dormant_entities.contains_key(&key)
        {
            // Untouched terrain: drop it, it regenerates from the seed.
            st.statics.remove_region(&mut st.physics, key);
        } else {
            st.statics.deactivate(&mut st.physics, key);
            if !stored.is_empty() {
                st.world.dormant_entities.entry(key).or_default().extend(stored);
            }
        }
    }

    /// Restores a region's stored entities.
    fn wake_entities(&mut self, key: RegionKey) {
        let Some(list) = self.state.world.dormant_entities.remove(&key) else { return };
        let woken: Vec<EntityId> = list.iter().map(|d| d.entity.id).collect();
        for d in list {
            let mut e = d.entity;
            if let Some(part) = e.soft.take() {
                let mut fresh = self.state.physics.insert_soft(&part.def, e.pos, e.rot);
                if let Some(sb) = fresh.handle.and_then(|h| self.state.physics.soft_bodies.get_mut(h)) {
                    for (i, (p, v)) in part.saved.iter().enumerate().take(sb.num_particles()) {
                        sb.set_particle_position(i, *p);
                        sb.set_particle_velocity(i, *v);
                    }
                    let tag = entity_tag(e.id.0);
                    if let Some(b) = self.state.physics.bodies.get(sb.root_body()) {
                        for c in b.colliders().to_vec() {
                            if let Some(col) = self.state.physics.colliders.get_mut(c) {
                                col.user_data = tag;
                            }
                        }
                    }
                }
                fresh.saved.clear();
                e.soft = Some(fresh);
            }
            if e.body_kind != BodyKind::None {
                let shape = e.visual.as_ref().map(|v| v.shape).unwrap_or(Shape::Sphere { radius: 0.25 });
                let builder = match e.body_kind {
                    BodyKind::Fixed => RigidBodyBuilder::fixed(),
                    BodyKind::Kinematic => RigidBodyBuilder::kinematic_velocity_based(),
                    _ => RigidBodyBuilder::dynamic(),
                }
                .pose(Pose::from_parts(e.pos, e.rot))
                .linvel(d.linvel)
                .angvel(d.angvel);
                let m = e.material;
                let collider = shape
                    .collider()
                    .density(m.density as Real)
                    .friction(m.friction as Real)
                    .restitution(m.restitution as Real)
                    .user_data(entity_tag(e.id.0));
                let (b, _) = self.state.physics.insert(builder, collider);
                e.body = Some(b);
            }
            self.state.entities.map.insert(e.id, e);
        }
        self.restore_joints(&woken);
    }

    /// Tracks which room the player is in; applies/restores room overrides; emits events.
    pub(crate) fn update_room_tracking(&mut self, events: &mut Vec<SimEvent>) {
        if !self.state.world.enabled {
            return;
        }
        let Some(p) = self.player().map(|e| e.pos) else { return };
        let now = self.state.world.rooms.iter().find(|r| r.contains(p)).map(|r| r.id);
        let prev = self.state.world.current_room;
        if now == prev {
            return;
        }
        if let Some(id) = prev {
            let saved = std::mem::take(&mut self.state.world.saved_params);
            apply_params(&mut self.config, &saved);
            events.push(SimEvent::ExitRoom { room: id });
        }
        if let Some(id) = now {
            let slot = &self.state.world.rooms[id as usize];
            let (def, quarters) = (slot.def.clone(), slot.place.quarters);
            self.state.world.saved_params = enter_overrides_rotated(&mut self.config, &def, quarters);
            events.push(SimEvent::EnterRoom { room: id });
        }
        self.state.world.current_room = now;
        self.courses_on_room_change(now);
    }
}

/// Wraps the config so parameter paths (movement.model, ...) can be set.
pub(crate) struct ConfigRoot<'a>(pub &'a mut SimConfig);

impl Tunable for ConfigRoot<'_> {
    fn visit(&mut self, v: &mut dyn params::ParamVisitor) {
        self.0.visit_groups(v);
    }
}

fn apply_params(cfg: &mut SimConfig, values: &BTreeMap<String, ParamValue>) {
    params::apply_map(&mut ConfigRoot(cfg), values);
}

/// Applies a room's overrides and returns the previous values.
pub fn enter_overrides(cfg: &mut SimConfig, def: &RoomDef) -> BTreeMap<String, ParamValue> {
    enter_overrides_rotated(cfg, def, 0)
}

/// Same, for a room placed with `quarters` turns (axis locks follow the room).
pub fn enter_overrides_rotated(cfg: &mut SimConfig, def: &RoomDef, quarters: u8) -> BTreeMap<String, ParamValue> {
    let mut wanted = def.params.clone();
    crate::course::rotate_axis_params(&mut wanted, quarters);
    if let Some(m) = def.movement_model {
        wanted.insert("movement.model".into(), ParamValue::Text(params::ChoiceParam::name(m).to_string()));
    }
    let mut saved = BTreeMap::new();
    let mut root = ConfigRoot(cfg);
    for k in wanted.keys() {
        if let Some(v) = params::get(&mut root, k) {
            saved.insert(k.clone(), v);
        }
    }
    let unknown = params::apply_map(&mut root, &wanted);
    for u in unknown {
        log::warn!("room '{}': unknown parameter '{u}'", def.name);
    }
    saved
}

/// Result of a world raycast.
#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub point: Vec3,
    pub normal: Vec3,
    pub entity: Option<EntityId>,
}

impl Sim {
    /// Casts a ray against everything except the player (and `exclude`).
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max: f32, exclude: Option<EntityId>) -> Option<RayHit> {
        let mut filter = QueryFilter::default().exclude_sensors();
        let skip: Vec<RigidBodyHandle> = [self.state.player, exclude]
            .into_iter()
            .flatten()
            .filter_map(|id| self.state.entities.get(id).and_then(|e| e.body))
            .collect();
        if let Some(b) = skip.first() {
            filter = filter.exclude_rigid_body(*b);
        }
        let pred = |_: ColliderHandle, c: &Collider| !skip.iter().any(|b| c.parent() == Some(*b));
        let filter = filter.predicate(&pred);
        let ray = Ray::new(origin, dir.normalize_or(Vec3::NEG_Y));
        let qp = self.state.physics.query_filtered(filter);
        let (h, hit) = qp.cast_ray_and_get_normal(&ray, max as Real, true)?;
        let point = origin + dir.normalize_or(Vec3::NEG_Y) * hit.time_of_impact as f32;
        let entity = self.state.physics.colliders.get(h).and_then(|c| crate::physics::entity_from_tag(c.user_data)).map(EntityId);
        Some(RayHit { point, normal: hit.normal, entity })
    }

    /// The room's free objects as they are now (for saving sandbox edits), in room space.
    pub fn room_objects(&self, id: u16) -> Vec<crate::room::ObjectDef> {
        let Some(slot) = self.state.world.rooms.get(id as usize) else { return Vec::new() };
        let inv = slot.place.quat().inverse();
        let region = Some(RegionKey::Room(id));
        let dormant = self.state.world.dormant_entities.get(&RegionKey::Room(id)).into_iter().flatten().map(|d| &d.entity);
        self.state
            .entities
            .iter()
            .filter(|e| e.region == region)
            .chain(dormant)
            .filter(|e| e.character.is_none() && e.bomb.is_none() && e.lifetime.is_none() && !e.name.starts_with('~'))
            .filter_map(|e| {
                let v = e.visual.as_ref()?;
                // Animated objects are saved at their start pose, without running state.
                let (pos, rot0) = match &e.behavior {
                    crate::entity::Behavior::Move(m) => m.origin.unwrap_or((e.pos, e.rot)),
                    crate::entity::Behavior::Rotate(r) => r.origin.unwrap_or((e.pos, e.rot)),
                    _ => (e.pos, e.rot),
                };
                let local = inv * (pos - slot.place.origin);
                let rot = inv * rot0;
                let (yaw, pitch, roll) = rot.to_euler(glam::EulerRot::YXZ);
                let r1 = |a: f32| (a.to_degrees() * 10.0).round() / 10.0;
                Some(crate::room::ObjectDef {
                    name: e.name.clone(),
                    shape: v.shape,
                    pos: (local * 1000.0).round() / 1000.0,
                    yaw: r1(yaw),
                    pitch: r1(pitch),
                    roll: r1(roll),
                    color: crate::color::to_hex(v.color),
                    body: e.body_kind,
                    look: v.look,
                    emissive: v.emissive,
                    behavior: fresh_behavior(&e.behavior),
                    hazard: e.hazard.clone(),
                    density: e.material.density,
                    friction: e.material.friction,
                    restitution: e.material.restitution,
                    soft: e.soft.as_ref().map(|s| s.def.clone()),
                })
            })
            .collect()
    }
}

/// A behaviour as authored (running state cleared).
fn fresh_behavior(b: &crate::entity::Behavior) -> crate::entity::Behavior {
    use crate::entity::Behavior;
    match b {
        Behavior::Move(m) => Behavior::Move(crate::entity::MoverDef { origin: None, ..m.clone() }),
        Behavior::Rotate(r) => Behavior::Rotate(crate::entity::RotatorDef { origin: None, ..r.clone() }),
        Behavior::Emitter(e) => {
            Behavior::Emitter(crate::entity::EmitterDef { timer: 0.0, angle: 0.0, shots: 0, ..e.clone() })
        }
        Behavior::Spawner(s) => Behavior::Spawner(crate::entity::SpawnerDef {
            timer: 0.0,
            pending: 0,
            spawned: Default::default(),
            ..s.clone()
        }),
        Behavior::Rain { interval, max, area, height, .. } => {
            Behavior::Rain { interval: *interval, max: *max, area: *area, height: *height, timer: 0, spawned: Default::default() }
        }
        other => other.clone(),
    }
}

/// Replaces the `[[object]]` tables of a room file with `objects`, keeping everything else
/// (maps, comments) as written. Objects are always written at the end of the file.
pub fn rewrite_room_objects(text: &str, objects: &[crate::room::ObjectDef]) -> String {
    let mut keep = String::new();
    let mut in_obj = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("[[object]]") {
            in_obj = true;
            continue;
        }
        if in_obj && t.starts_with('[') {
            in_obj = false;
        }
        if !in_obj {
            keep.push_str(line);
            keep.push('\n');
        }
    }
    while keep.ends_with("\n\n") {
        keep.pop();
    }
    for o in objects {
        keep.push('\n');
        keep.push_str(&object_toml(o));
    }
    keep
}

fn num(x: f32) -> String {
    let r = (x * 1000.0).round() / 1000.0;
    if r == r.trunc() { format!("{r:.1}") } else { format!("{r}") }
}

fn vec3(v: Vec3) -> String {
    format!("[{}, {}, {}]", num(v.x), num(v.y), num(v.z))
}

/// One `[[object]]` table in the same compact style as hand-written room files.
pub fn object_toml(o: &crate::room::ObjectDef) -> String {
    let shape = match o.shape {
        Shape::Box { half } => format!("{{ type = \"box\", half = {} }}", vec3(half)),
        Shape::RoundedBox { half, radius } => {
            format!("{{ type = \"rounded_box\", half = {}, radius = {} }}", vec3(half), num(radius))
        }
        Shape::Sphere { radius } => format!("{{ type = \"sphere\", radius = {} }}", num(radius)),
        Shape::Capsule { half_height, radius } => {
            format!("{{ type = \"capsule\", half_height = {}, radius = {} }}", num(half_height), num(radius))
        }
        Shape::Cylinder { half_height, radius } => {
            format!("{{ type = \"cylinder\", half_height = {}, radius = {} }}", num(half_height), num(radius))
        }
    };
    let mut t = format!("[[object]]\nname = {:?}\nshape = {shape}\npos = {}\n", o.name, vec3(o.pos));
    for (k, v) in [("yaw", o.yaw), ("pitch", o.pitch), ("roll", o.roll)] {
        if v.abs() > 0.05 {
            t.push_str(&format!("{k} = {}\n", num(v)));
        }
    }
    t.push_str(&format!("color = {:?}\n", o.color));
    if o.body != BodyKind::Dynamic {
        t.push_str(&format!(
            "body = {:?}\n",
            serde_json::to_value(o.body).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
        ));
    }
    if o.look != Look::Cel {
        t.push_str(&format!("look = {:?}\n", params::ChoiceParam::name(o.look)));
    }
    if o.emissive != 0.0 {
        t.push_str(&format!("emissive = {}\n", num(o.emissive)));
    }
    if o.behavior != crate::entity::Behavior::None {
        if let Ok(v) = toml::Value::try_from(&o.behavior) {
            t.push_str(&format!("behavior = {}\n", inline(&v)));
        }
    }
    if let Some(h) = &o.hazard {
        if let Ok(v) = toml::Value::try_from(h) {
            t.push_str(&format!("hazard = {}\n", inline(&v)));
        }
    }
    for (k, v, d) in [("density", o.density, 1.0), ("friction", o.friction, 0.5), ("restitution", o.restitution, 0.0)] {
        if (v - d).abs() > 1e-4 {
            t.push_str(&format!("{k} = {}\n", num(v)));
        }
    }
    if let Some(s) = &o.soft {
        if let Ok(v) = toml::Value::try_from(s) {
            t.push_str(&format!("soft = {}\n", inline(&v)));
        }
    }
    t
}

fn inline(v: &toml::Value) -> String {
    match v {
        toml::Value::Table(m) => {
            let parts: Vec<String> = m.iter().map(|(k, v)| format!("{k} = {}", inline(v))).collect();
            format!("{{ {} }}", parts.join(", "))
        }
        toml::Value::Array(a) => format!("[{}]", a.iter().map(inline).collect::<Vec<_>>().join(", ")),
        other => other.to_string(),
    }
}
