//! The world: every entity, the physics, projectiles, particles, the camera and the look.
//! Game code gets `&mut World` in `Game::update` and changes it through the methods below.
//! Cloning a World is a snapshot (rewind uses that).

use std::collections::BTreeMap;

use glam::{Quat, Vec3};
use rapier3d::prelude::*;
use serde::Serialize;

use crate::character::{self, MovementParams};
use crate::entity::{Body, Entity, Id, Shape, Spawn};
use crate::input::Input;
use crate::params::{ParamVisitor, Tunable, nested};
use crate::puppet::Act;
use crate::util::{Color, Rng};

/// Something that happened during the last tick. Read them in `Game::update` (`w.events`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A character or moving body started overlapping a trigger entity.
    Enter {
        trigger: Id,
        other: Id,
    },
    /// ...and stopped overlapping it.
    Exit {
        trigger: Id,
        other: Id,
    },
    /// A projectile hit an entity (walls are entities too). Damage was already applied if the
    /// target has hp and is on another team.
    Hit {
        target: Id,
        owner: Option<Id>,
        pos: Vec3,
        damage: f32,
    },
    /// An entity's hp reached 0 from a projectile. It stays until you despawn it.
    Killed {
        id: Id,
        by: Option<Id>,
    },
    Jump {
        id: Id,
    },
    /// A character landed (impact speed m/s).
    Land {
        id: Id,
        speed: f32,
    },
    /// A character fell below `config.kill_y` (respawn it or end the game).
    Fell {
        id: Id,
    },
}

/// The camera rig. "2D" is just a camera: `tilt = 90, ortho = true` is top-down; `tilt = 0`
/// is a side view.
#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    /// Degrees above the horizon: 90 = straight down, 0 = side view.
    pub tilt: f32,
    /// Degrees; 0 = looking north (-Z) with east (+X) to the right.
    pub yaw: f32,
    /// Distance from the target (m). In orthographic mode it sets the zoom.
    pub distance: f32,
    /// Vertical field of view (degrees).
    pub fov: f32,
    pub ortho: bool,
    /// Look this far above the target (m).
    pub height: f32,
    /// Entity to follow (default: the player). None = look at `target`.
    pub follow: Option<Id>,
    pub target: Vec3,
    /// Seconds to catch up with the followed entity (window only; captures snap).
    pub lag: f32,
    /// Screen shake strength (decays by itself).
    pub shake: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            tilt: 62.0,
            yaw: 0.0,
            distance: 18.0,
            fov: 40.0,
            ortho: false,
            height: 0.8,
            follow: None,
            target: Vec3::ZERO,
            lag: 0.12,
            shake: 0.0,
        }
    }
}

impl Tunable for Camera {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.float("tilt", &mut self.tilt, 0.0, 90.0, "Degrees above the horizon (90 = top-down, 0 = side view)");
        v.float("yaw", &mut self.yaw, -180.0, 180.0, "Rotation around the target (degrees)");
        v.float("distance", &mut self.distance, 2.0, 200.0, "Distance / zoom (m)");
        v.float("fov", &mut self.fov, 5.0, 100.0, "Vertical field of view (degrees)");
        v.bool("ortho", &mut self.ortho, "Orthographic projection (flat 2D look)");
        v.float("height", &mut self.height, -5.0, 10.0, "Look above the target (m)");
        v.float("lag", &mut self.lag, 0.0, 1.0, "Seconds to catch up with the target");
    }
}

/// Lighting and atmosphere.
#[derive(Clone, Debug, PartialEq)]
pub struct Env {
    pub sky: Color,
    pub horizon: Color,
    /// Sun height (degrees above the horizon) and direction (degrees, 0 = from the north).
    pub sun_elevation: f32,
    pub sun_azimuth: f32,
    pub sun: f32,
    pub sun_color: Color,
    /// Light from the sky in shadow.
    pub ambient: f32,
    /// Distance where fog covers everything (0 = no fog).
    pub fog: f32,
    pub shadows: bool,
    pub outlines: bool,
}

impl Default for Env {
    fn default() -> Self {
        Self {
            sky: Color::hex("#7fb2e5"),
            horizon: Color::hex("#dfe9f2"),
            sun_elevation: 55.0,
            sun_azimuth: 210.0,
            sun: 1.0,
            sun_color: Color::hex("#fff4e0"),
            ambient: 0.45,
            fog: 0.0,
            shadows: true,
            outlines: true,
        }
    }
}

impl Env {
    /// Direction toward the sun.
    pub fn sun_dir(&self) -> Vec3 {
        let (e, a) = (self.sun_elevation.to_radians(), self.sun_azimuth.to_radians());
        Vec3::new(a.sin() * e.cos(), e.sin(), -a.cos() * e.cos()).normalize()
    }
}

impl Tunable for Env {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.float("sun_elevation", &mut self.sun_elevation, 5.0, 90.0, "Sun height (degrees)");
        v.float("sun_azimuth", &mut self.sun_azimuth, 0.0, 360.0, "Sun direction (degrees, 0 = north)");
        v.float("sun", &mut self.sun, 0.0, 3.0, "Sun strength");
        v.float("ambient", &mut self.ambient, 0.0, 2.0, "Sky light in shadow");
        v.float("fog", &mut self.fog, 0.0, 500.0, "Distance fully covered by fog (0 = off)");
        v.bool("shadows", &mut self.shadows, "Sun shadows");
        v.bool("outlines", &mut self.outlines, "Dark outlines around objects");
    }
}

/// Simulation settings (tunable as `sim.*` and `movement.*`).
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Gravity for props, projectiles and particles (m/s²).
    pub gravity: f32,
    /// Below this height characters get `Event::Fell` and props are removed.
    pub kill_y: f32,
    pub movement: MovementParams,
}

impl Default for Config {
    fn default() -> Self {
        Self { gravity: 9.81, kill_y: -30.0, movement: MovementParams::default() }
    }
}

/// A lightweight projectile (not a physics body): thousands are fine.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Shot {
    pub pos: Vec3,
    pub vel: Vec3,
    pub radius: f32,
    pub color: Color,
    pub owner: Option<Id>,
    /// Passes through entities of the same team (0 = hits everything).
    pub team: u8,
    pub damage: f32,
    /// Velocity added to a hit character (m/s along the shot).
    pub knockback: f32,
    /// Seconds left.
    pub life: f32,
    /// Share of `config.gravity` pulling it down (0 = straight line).
    pub gravity: f32,
}

impl Shot {
    pub fn new(pos: Vec3, vel: Vec3) -> Self {
        Self {
            pos,
            vel,
            radius: 0.12,
            color: Color::hex("#9ef0ff"),
            owner: None,
            team: 0,
            damage: 1.0,
            knockback: 0.0,
            life: 3.0,
            gravity: 0.0,
        }
    }
    pub fn owner(mut self, id: Id) -> Self {
        self.owner = Some(id);
        self
    }
    pub fn team(mut self, t: u8) -> Self {
        self.team = t;
        self
    }
    pub fn damage(mut self, d: f32) -> Self {
        self.damage = d;
        self
    }
    pub fn radius(mut self, r: f32) -> Self {
        self.radius = r;
        self
    }
    pub fn color(mut self, hex: &str) -> Self {
        self.color = Color::hex(hex);
        self
    }
    pub fn knockback(mut self, k: f32) -> Self {
        self.knockback = k;
        self
    }
    pub fn life(mut self, s: f32) -> Self {
        self.life = s;
        self
    }
    pub fn gravity(mut self, g: f32) -> Self {
        self.gravity = g;
        self
    }
}

/// A visual spark (part of the world state, so rewinds show them too).
#[derive(Clone, Debug, PartialEq)]
pub struct Particle {
    pub pos: Vec3,
    pub vel: Vec3,
    pub color: Color,
    pub size: f32,
    pub life: f32,
    pub max_life: f32,
    pub gravity: f32,
}

/// Rapier's state (cloneable, so it rides in snapshots). The pipeline is only scratch space.
#[derive(Clone)]
pub(crate) struct Physics {
    pub params: IntegrationParameters,
    pub islands: IslandManager,
    pub broad: DefaultBroadPhase,
    pub narrow: NarrowPhase,
    pub bodies: RigidBodySet,
    pub colliders: ColliderSet,
    pub joints: ImpulseJointSet,
    pub multibody: MultibodyJointSet,
    pub soft: SoftBodySet,
    pub ccd: CCDSolver,
    pub pipeline: Scratch,
}

/// A physics pipeline whose clone is a fresh one (it holds no simulation state).
pub(crate) struct Scratch(pub PhysicsPipeline);

impl Clone for Scratch {
    fn clone(&self) -> Self {
        Scratch(PhysicsPipeline::new())
    }
}

impl Physics {
    fn new(dt: f32) -> Self {
        Self {
            params: IntegrationParameters { dt, ..Default::default() },
            islands: IslandManager::new(),
            broad: DefaultBroadPhase::default(),
            narrow: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            joints: ImpulseJointSet::new(),
            multibody: MultibodyJointSet::new(),
            soft: SoftBodySet::new(),
            ccd: CCDSolver::new(),
            pipeline: Scratch(PhysicsPipeline::new()),
        }
    }

    fn query<'a>(&'a self, filter: QueryFilter<'a>) -> QueryPipeline<'a> {
        self.broad.as_query_pipeline(self.narrow.query_dispatcher(), &self.bodies, &self.colliders, filter)
    }
}

/// Where a ray hit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RayHit {
    pub id: Id,
    pub point: Vec3,
    pub normal: Vec3,
    pub dist: f32,
}

#[derive(Clone)]
pub struct World {
    pub tick: u64,
    /// Seconds per tick (1/60).
    pub dt: f32,
    pub seed: u64,
    /// The only randomness game code may use.
    pub rng: Rng,
    /// All entities by id (ordered, so iteration is deterministic).
    pub entities: BTreeMap<Id, Entity>,
    /// The entity the window controls and the camera follows.
    pub player: Option<Id>,
    /// What happened during the last tick.
    pub events: Vec<Event>,
    pub camera: Camera,
    pub env: Env,
    pub config: Config,
    pub shots: Vec<Shot>,
    pub particles: Vec<Particle>,
    /// Named points from the level (`marker = "name"` in a legend), e.g. "player".
    pub markers: Vec<(String, Vec3)>,
    pub(crate) phys: Physics,
    pub(crate) inside: BTreeMap<Id, Vec<Id>>,
    next_id: Id,
}

impl World {
    pub fn new(seed: u64) -> Self {
        let dt = 1.0 / 60.0;
        Self {
            tick: 0,
            dt,
            seed,
            rng: Rng::new(seed),
            entities: BTreeMap::new(),
            player: None,
            events: Vec::new(),
            camera: Camera::default(),
            env: Env::default(),
            config: Config::default(),
            shots: Vec::new(),
            particles: Vec::new(),
            markers: Vec::new(),
            phys: Physics::new(dt),
            inside: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// Seconds since the game started.
    pub fn time(&self) -> f32 {
        self.tick as f32 * self.dt
    }

    // ------------------------------------------------------------------ entities

    /// Creates an entity and returns its id.
    pub fn spawn(&mut self, s: Spawn) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        let tag = id as u128;
        let handle = if let Some(ch) = &s.character {
            let center = s.pos + Vec3::Y * ch.height * 0.5;
            let rb = RigidBodyBuilder::kinematic_position_based().translation(center);
            let col = ColliderBuilder::capsule_y(ch.half_height(), ch.radius).friction(0.0).user_data(tag);
            Some(self.insert(rb.build(), col.build()))
        } else {
            let rb = match s.body {
                Body::None => None,
                Body::Static | Body::Trigger => Some(RigidBodyBuilder::fixed()),
                Body::Dynamic => Some(RigidBodyBuilder::dynamic().ccd_enabled(s.ccd)),
                Body::Kinematic => Some(RigidBodyBuilder::kinematic_velocity_based()),
            };
            rb.map(|rb| {
                let rb = rb.pose(Pose::from_parts(s.pos, s.rot)).linear_damping(s.damping).angular_damping(s.damping);
                let col = s
                    .shape
                    .collider()
                    .density(s.density)
                    .friction(s.friction)
                    .restitution(s.restitution)
                    .sensor(s.body == Body::Trigger)
                    .user_data(tag);
                let h = self.insert(rb.build(), col.build());
                if s.body == Body::Dynamic {
                    if let Some(b) = self.phys.bodies.get_mut(h) {
                        b.set_linvel(s.vel, true);
                        b.set_angvel(s.spin, true);
                    }
                }
                h
            })
        };
        let character = s.character.map(|mut c| {
            c.facing = crate::util::yaw_of(s.rot * Vec3::Z);
            Box::new(c)
        });
        let hp = s.hp;
        self.entities.insert(
            id,
            Entity {
                id,
                name: s.name,
                kind: s.kind,
                pos: s.pos,
                rot: s.rot,
                vel: s.vel,
                spin: s.spin,
                shape: s.shape,
                body: if character.is_some() { Body::Kinematic } else { s.body },
                color: s.color,
                look: s.look,
                visible: s.visible,
                team: s.team,
                hp,
                max_hp: hp,
                flash: 0.0,
                invuln: 0.0,
                oneway: s.oneway,
                life: s.life,
                mover: s.mover,
                character,
                puppet: s.puppet.map(Box::new),
                handle,
            },
        );
        id
    }

    fn insert(&mut self, rb: RigidBody, col: Collider) -> RigidBodyHandle {
        let p = &mut self.phys;
        let h = p.bodies.insert(rb);
        let c = p.colliders.insert_with_parent(col, h, &mut p.bodies);
        // Queryable right away (raycasts in `setup`), not only after the first physics step.
        if let Some(aabb) = p.colliders.get(c).map(|c| c.compute_aabb()) {
            p.broad.set_aabb(&p.params, c, aabb);
        }
        h
    }

    /// Removes an entity (and its physics body). Returns false if it did not exist.
    pub fn despawn(&mut self, id: Id) -> bool {
        let Some(e) = self.entities.remove(&id) else { return false };
        if let Some(h) = e.handle {
            let p = &mut self.phys;
            p.bodies.remove(h, &mut p.islands, &mut p.colliders, &mut p.joints, &mut p.multibody, &mut p.soft, true);
        }
        self.inside.remove(&id);
        if self.player == Some(id) {
            self.player = None;
        }
        true
    }

    pub fn get(&self, id: Id) -> Option<&Entity> {
        self.entities.get(&id)
    }

    /// For changing plain fields (color, hp, kind, visible, puppet...). Move things with
    /// `set_pos` / `set_vel` / `push`, not by writing `pos`.
    pub fn get_mut(&mut self, id: Id) -> Option<&mut Entity> {
        self.entities.get_mut(&id)
    }

    /// The player entity.
    pub fn player(&self) -> Option<&Entity> {
        self.player.and_then(|id| self.entities.get(&id))
    }

    /// All entities of a kind.
    pub fn each<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Entity> + 'a {
        self.entities.values().filter(move |e| e.kind == kind)
    }

    pub fn ids(&self, kind: &str) -> Vec<Id> {
        self.each(kind).map(|e| e.id).collect()
    }

    pub fn count(&self, kind: &str) -> usize {
        self.each(kind).count()
    }

    /// The nearest entity of a kind within `max` metres of `pos`.
    pub fn nearest(&self, kind: &str, pos: Vec3, max: f32) -> Option<Id> {
        self.each(kind)
            .map(|e| (e.id, e.pos.distance(pos)))
            .filter(|(_, d)| *d <= max)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Teleports an entity and stops it. For characters `pos` is the feet.
    pub fn set_pos(&mut self, id: Id, pos: Vec3) {
        let Some(e) = self.entities.get_mut(&id) else { return };
        e.pos = pos;
        e.vel = Vec3::ZERO;
        let mut center = pos;
        if let Some(c) = &mut e.character {
            c.vel = Vec3::ZERO;
            c.impulse = Vec3::ZERO;
            c.dash_time = 0.0;
            c.fell = false;
            center = pos + Vec3::Y * c.height * 0.5;
        }
        if let Some(m) = &mut e.mover {
            m.from = pos;
        }
        if let Some(b) = e.handle.and_then(|h| self.phys.bodies.get_mut(h)) {
            b.set_translation(center, true);
            if b.is_kinematic() {
                b.set_next_kinematic_translation(center);
            }
            b.set_linvel(Vec3::ZERO, true);
            b.set_angvel(Vec3::ZERO, true);
        }
    }

    pub fn set_rot(&mut self, id: Id, rot: Quat) {
        let Some(e) = self.entities.get_mut(&id) else { return };
        e.rot = rot;
        if let Some(b) = e.handle.and_then(|h| self.phys.bodies.get_mut(h)) {
            b.set_rotation(rot, true);
        }
    }

    /// Sets the velocity: dynamic bodies directly, characters (horizontal and vertical), and
    /// kinematic / None entities (the engine moves them).
    pub fn set_vel(&mut self, id: Id, v: Vec3) {
        let Some(e) = self.entities.get_mut(&id) else { return };
        e.vel = v;
        if let Some(c) = &mut e.character {
            c.vel = v;
        } else if e.body == Body::Dynamic {
            if let Some(b) = e.handle.and_then(|h| self.phys.bodies.get_mut(h)) {
                b.set_linvel(v, true);
            }
        }
    }

    /// Adds velocity: a kick for dynamic bodies, a knockback for characters (they lose control
    /// for `stun` seconds; 0 keeps control).
    pub fn push(&mut self, id: Id, dv: Vec3, stun: f32) {
        let Some(e) = self.entities.get_mut(&id) else { return };
        if let Some(c) = &mut e.character {
            c.impulse += dv;
            c.stun = c.stun.max(stun);
            if dv.y > 0.0 {
                c.grounded = false;
            }
        } else if let Some(b) = e.handle.and_then(|h| self.phys.bodies.get_mut(h)) {
            if b.is_dynamic() {
                let v = b.linvel();
                b.set_linvel(v + dv, true);
            }
        } else {
            e.vel += dv;
        }
    }

    /// Sets what a character does this tick (NPC brains): call it every tick in `update`;
    /// an NPC that isn't driven stands still. `pressed` buttons fire once.
    pub fn drive(&mut self, id: Id, input: Input) {
        if let Some(c) = self.entities.get_mut(&id).and_then(|e| e.character.as_mut()) {
            c.input = input;
        }
    }

    /// A burst of speed for `seconds` (dodge rolls, lunges). Direction on the ground plane.
    pub fn dash(&mut self, id: Id, dir: Vec3, speed: f32, seconds: f32) {
        if let Some(c) = self.entities.get_mut(&id).and_then(|e| e.character.as_mut()) {
            let d = Vec3::new(dir.x, 0.0, dir.z).normalize_or(c.forward());
            c.dash_vel = d * speed;
            c.dash_time = seconds;
        }
    }

    /// Plays an action pose on a puppet (attack swing, shooting, cheering).
    pub fn act(&mut self, id: Id, act: Act, seconds: f32) {
        if let Some(c) = self.entities.get_mut(&id).and_then(|e| e.character.as_mut()) {
            c.anim.play(act, seconds);
        }
    }

    /// Takes hp away (flashes white). Returns true if this blow killed it (hp crossed 0).
    /// Ignored while the entity is `invuln`.
    pub fn damage(&mut self, id: Id, amount: f32) -> bool {
        let Some(e) = self.entities.get_mut(&id) else { return false };
        if e.hp <= 0.0 || e.invuln > 0.0 {
            return false;
        }
        e.hp -= amount;
        e.flash = 0.12;
        e.hp <= 0.0
    }

    // ------------------------------------------------------------------ queries

    /// First solid thing along a ray (triggers are ignored).
    pub fn raycast(&self, from: Vec3, dir: Vec3, max: f32, ignore: Option<Id>) -> Option<RayHit> {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return None;
        }
        let skip = ignore.and_then(|i| self.entities.get(&i)).and_then(|e| e.handle);
        let mut filter = QueryFilter::default().exclude_sensors();
        if let Some(h) = skip {
            filter = filter.exclude_rigid_body(h);
        }
        let qp = self.phys.query(filter);
        let (c, hit) = qp.cast_ray_and_get_normal(&Ray::new(from, dir), max, true)?;
        let id = self.phys.colliders.get(c)?.user_data as Id;
        Some(RayHit { id, point: from + dir * hit.time_of_impact, normal: hit.normal, dist: hit.time_of_impact })
    }

    /// True if nothing solid blocks the line (entities in `ignore` don't count).
    pub fn can_see(&self, from: Vec3, to: Vec3, ignore: &[Id]) -> bool {
        let d = to - from;
        let len = d.length();
        if len < 1e-4 {
            return true;
        }
        let handles: Vec<RigidBodyHandle> = ignore.iter().filter_map(|i| self.entities.get(i)?.handle).collect();
        let pred = |_: ColliderHandle, c: &Collider| !c.parent().is_some_and(|p| handles.contains(&p));
        let qp = self.phys.query(QueryFilter::default().exclude_sensors().predicate(&pred));
        qp.cast_ray(&Ray::new(from, d / len), len, true).is_none()
    }

    /// Entities whose collider overlaps a sphere (triggers included).
    pub fn overlap(&self, center: Vec3, radius: f32) -> Vec<Id> {
        let ball = SharedShape::ball(radius);
        let qp = self.phys.query(QueryFilter::default());
        let mut out: Vec<Id> =
            qp.intersect_shape(Pose::from_translation(center), &*ball).map(|(_, c)| c.user_data as Id).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The static or kinematic entity (a wall, floor, platform) filling a small box at `p`,
    /// if any. Characters, props and triggers don't count.
    pub fn solid_at(&self, p: Vec3, half: f32) -> Option<Id> {
        self.solid_box(p, Vec3::splat(half))
    }

    /// Like `solid_at` for a box of half size `half` (axis-aligned).
    pub fn solid_box(&self, p: Vec3, half: Vec3) -> Option<Id> {
        let cube = SharedShape::cuboid(half.x, half.y, half.z);
        let qp = self.phys.query(QueryFilter::from(QueryFilterFlags::EXCLUDE_DYNAMIC | QueryFilterFlags::EXCLUDE_SENSORS));
        qp.intersect_shape(Pose::from_translation(p), &*cube)
            .map(|(_, c)| c.user_data as Id)
            .find(|id| self.entities.get(id).is_some_and(|e| e.character.is_none()))
    }

    /// Height of the level surface below `(x, from_y, z)`: static and moving blocks count;
    /// characters, props (dynamic bodies) and triggers don't.
    pub fn ground_at(&self, x: f32, z: f32, from_y: f32) -> Option<f32> {
        let ents = &self.entities;
        let level = |_: ColliderHandle, c: &Collider| ents.get(&(c.user_data as Id)).is_none_or(|e| e.character.is_none());
        let flags = QueryFilterFlags::EXCLUDE_DYNAMIC | QueryFilterFlags::EXCLUDE_SENSORS;
        let qp = self.phys.query(QueryFilter::from(flags).predicate(&level));
        qp.cast_ray(&Ray::new(Vec3::new(x, from_y, z), Vec3::NEG_Y), from_y + 100.0, true).map(|(_, t)| from_y - t)
    }

    /// Triggers of `kind` the player started touching during the last tick (pickups, goals,
    /// hazards). The same as filtering `w.events` for `Event::Enter` by the player.
    pub fn player_entered(&self, kind: &str) -> Vec<Id> {
        let Some(p) = self.player else { return Vec::new() };
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::Enter { trigger, other } if *other == p && self.get(*trigger).is_some_and(|t| t.kind == kind) => {
                    Some(*trigger)
                }
                _ => None,
            })
            .collect()
    }

    // ------------------------------------------------------------------ effects

    pub fn shoot(&mut self, shot: Shot) {
        if self.shots.len() < 5000 {
            self.shots.push(shot);
        }
    }

    /// Sparks flying out from `pos` (visual only).
    pub fn burst(&mut self, pos: Vec3, color: &str, count: u32, speed: f32) {
        let color = Color::hex(color);
        for _ in 0..count.min(400) {
            let dir = self.rng.in_sphere();
            let life = self.rng.range(0.35, 0.8);
            let size = self.rng.range(0.05, 0.12);
            self.particles.push(Particle {
                pos,
                vel: dir * speed + Vec3::Y * speed * 0.4,
                color,
                size,
                life,
                max_life: life,
                gravity: 1.0,
            });
        }
        if self.particles.len() > 3000 {
            let extra = self.particles.len() - 3000;
            self.particles.drain(..extra);
        }
    }

    /// Shakes the camera (window and captures).
    pub fn shake(&mut self, strength: f32) {
        self.camera.shake = self.camera.shake.max(strength);
    }

    // ------------------------------------------------------------------ the tick

    /// Advances physics and everything the engine moves by one tick. Called by `Sim::step`
    /// after `Game::update`.
    pub(crate) fn simulate(&mut self) {
        let dt = self.dt;
        let t_next = (self.tick + 1) as f32 * dt;
        self.events.clear();
        self.phys.params.dt = dt;

        // Moving things the engine drives: kinematic bodies get the velocity that takes them
        // where they should be next tick (characters riding them read it).
        let ph = &mut self.phys;
        for e in self.entities.values_mut() {
            if e.character.is_some() {
                continue;
            }
            match e.body {
                Body::Kinematic => {
                    let Some(b) = e.handle.and_then(|h| ph.bodies.get_mut(h)) else { continue };
                    let v = match &e.mover {
                        Some(m) => (m.at(t_next) - e.pos) / dt,
                        None => e.vel,
                    };
                    b.set_linvel(v, true);
                    b.set_angvel(e.spin, true);
                }
                Body::None => {
                    e.pos += e.vel * dt;
                    if e.spin != Vec3::ZERO {
                        e.rot = (Quat::from_scaled_axis(e.spin * dt) * e.rot).normalize();
                    }
                }
                // Spinning pickups and decorations that don't move.
                Body::Static | Body::Trigger if e.spin != Vec3::ZERO => {
                    e.rot = (Quat::from_scaled_axis(e.spin * dt) * e.rot).normalize();
                    if let Some(b) = e.handle.and_then(|h| ph.bodies.get_mut(h)) {
                        b.set_rotation(e.rot, false);
                    }
                }
                _ => {}
            }
        }

        let ids: Vec<Id> = self.entities.values().filter(|e| e.character.is_some()).map(|e| e.id).collect();
        for &id in &ids {
            character::tick(self, id, dt);
        }

        let ph = &mut self.phys;
        ph.pipeline.0.step(
            Vec3::new(0.0, -self.config.gravity, 0.0),
            &ph.params,
            &mut ph.islands,
            &mut ph.broad,
            &mut ph.narrow,
            &mut ph.bodies,
            &mut ph.colliders,
            &mut ph.joints,
            &mut ph.multibody,
            &mut ph.soft,
            &mut ph.ccd,
            &(),
            &(),
        );
        for e in self.entities.values_mut() {
            if let Some(b) = e.handle.and_then(|h| ph.bodies.get(h)) {
                if e.character.is_none() {
                    e.pos = b.translation();
                    e.rot = *b.rotation();
                    if e.body == Body::Dynamic || e.mover.is_some() {
                        e.vel = b.linvel();
                    }
                }
            }
        }

        self.step_shots(dt);
        self.step_triggers();

        // Falling out of the world, lifetimes, hit flashes.
        let kill_y = self.config.kill_y;
        let mut gone = Vec::new();
        for e in self.entities.values_mut() {
            e.flash = (e.flash - dt).max(0.0);
            e.invuln = (e.invuln - dt).max(0.0);
            if let Some(l) = &mut e.life {
                *l -= dt;
                if *l <= 0.0 {
                    gone.push(e.id);
                }
            }
            if e.pos.y < kill_y {
                match &mut e.character {
                    Some(c) if !c.fell => {
                        c.fell = true;
                        self.events.push(Event::Fell { id: e.id });
                    }
                    Some(_) => {}
                    None if e.body == Body::Dynamic => gone.push(e.id),
                    None => {}
                }
            }
            if let Some(c) = &mut e.character {
                c.input.pressed = 0;
            }
        }
        for id in gone {
            self.despawn(id);
        }

        let g = self.config.gravity;
        for p in &mut self.particles {
            p.vel.y -= g * p.gravity * dt;
            p.vel *= 1.0 - 1.5 * dt;
            p.pos += p.vel * dt;
            p.life -= dt;
        }
        self.particles.retain(|p| p.life > 0.0);
        self.camera.shake = (self.camera.shake - dt * 2.5).max(0.0);
        self.tick += 1;
    }

    fn step_shots(&mut self, dt: f32) {
        if self.shots.is_empty() {
            return;
        }
        let mut shots = std::mem::take(&mut self.shots);
        let mut hits = Vec::new();
        {
            let ents = &self.entities;
            let g = self.config.gravity;
            for (i, s) in shots.iter_mut().enumerate() {
                s.life -= dt;
                s.vel.y -= g * s.gravity * dt;
                let step = s.vel * dt;
                let owner = s.owner.and_then(|o| ents.get(&o)).and_then(|e| e.handle);
                let team = s.team;
                let pred = |_: ColliderHandle, c: &Collider| {
                    let id = c.user_data as Id;
                    if owner.is_some() && c.parent() == owner {
                        return false;
                    }
                    !(team != 0 && ents.get(&id).is_some_and(|e| e.team == team))
                };
                let qp = self.phys.query(QueryFilter::default().exclude_sensors().predicate(&pred));
                let ball = Ball::new(s.radius);
                let opts = rapier3d::parry::query::ShapeCastOptions::with_max_time_of_impact(1.0);
                if let Some((c, hit)) = qp.cast_shape(&Pose::from_translation(s.pos), step, &ball, opts) {
                    let at = s.pos + step * hit.time_of_impact;
                    let target = self.phys.colliders.get(c).map(|c| c.user_data as Id).unwrap_or(0);
                    hits.push((i, target, at));
                    s.life = 0.0;
                } else {
                    s.pos += step;
                }
            }
        }
        for (i, target, at) in hits {
            let s = shots[i].clone();
            self.events.push(Event::Hit { target, owner: s.owner, pos: at, damage: s.damage });
            let other_team = self.entities.get(&target).is_some_and(|e| e.team == 0 || e.team != s.team || s.team == 0);
            if other_team && s.damage > 0.0 && self.damage(target, s.damage) {
                self.events.push(Event::Killed { id: target, by: s.owner });
            }
            if s.knockback > 0.0 {
                let dir = Vec3::new(s.vel.x, 0.0, s.vel.z).normalize_or_zero();
                let is_char = self.entities.get(&target).is_some_and(|e| e.character.is_some());
                if is_char {
                    self.push(target, dir * s.knockback + Vec3::Y * s.knockback * 0.3, 0.15);
                } else {
                    self.push(target, dir * s.knockback * 0.3, 0.0);
                }
            }
        }
        shots.retain(|s| s.life > 0.0);
        shots.extend(std::mem::take(&mut self.shots));
        self.shots = shots;
    }

    fn step_triggers(&mut self) {
        let triggers: Vec<(Id, Vec3, Quat, Shape)> =
            self.entities.values().filter(|e| e.body == Body::Trigger).map(|e| (e.id, e.pos, e.rot, e.shape)).collect();
        for (id, pos, rot, shape) in triggers {
            let shared = match shape {
                Shape::Box { half } => SharedShape::cuboid(half.x, half.y, half.z),
                Shape::Sphere { radius } => SharedShape::ball(radius),
                Shape::Capsule { half_height, radius } => SharedShape::capsule_y(half_height, radius),
                Shape::Cylinder { half_height, radius } => SharedShape::cylinder(half_height, radius),
            };
            let qp = self.phys.query(QueryFilter::from(QueryFilterFlags::EXCLUDE_FIXED | QueryFilterFlags::EXCLUDE_SENSORS));
            let mut now: Vec<Id> = qp
                .intersect_shape(Pose::from_parts(pos, rot), &*shared)
                .map(|(_, c)| c.user_data as Id)
                .filter(|o| *o != id)
                .collect();
            now.sort_unstable();
            now.dedup();
            let before = self.inside.remove(&id).unwrap_or_default();
            for o in &now {
                if !before.contains(o) {
                    self.events.push(Event::Enter { trigger: id, other: *o });
                }
            }
            for o in &before {
                if !now.contains(o) && self.entities.contains_key(o) {
                    self.events.push(Event::Exit { trigger: id, other: *o });
                }
            }
            if !now.is_empty() {
                self.inside.insert(id, now);
            }
        }
    }

    /// Entities overlapping a trigger right now.
    pub fn inside(&self, trigger: Id) -> &[Id] {
        self.inside.get(&trigger).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Where a named level marker is (e.g. "player").
    pub fn marker(&self, name: &str) -> Option<Vec3> {
        self.markers.iter().find(|m| m.0 == name).map(|m| m.1)
    }

    /// All markers with this name.
    pub fn markers_named(&self, name: &str) -> Vec<Vec3> {
        self.markers.iter().filter(|m| m.0 == name).map(|m| m.1).collect()
    }

    /// Hash of the moving state (repeatability checks: replays compare it).
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |x: u64| {
            h ^= x;
            h = h.wrapping_mul(0x0100_0000_01b3);
        };
        mix(self.tick);
        for e in self.entities.values() {
            mix(e.id as u64);
            for f in e.pos.to_array().into_iter().chain(e.rot.to_array()) {
                mix(f.to_bits() as u64);
            }
            mix(e.hp.to_bits() as u64);
        }
        for s in &self.shots {
            for f in s.pos.to_array() {
                mix(f.to_bits() as u64);
            }
        }
        h
    }
}

/// Every engine tunable: `sim.*`, `movement.*`, `camera.*`, `env.*`.
impl Tunable for World {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.enter("sim");
        v.float("gravity", &mut self.config.gravity, 0.0, 60.0, "Gravity for props, shots, particles (m/s²)");
        v.float("kill_y", &mut self.config.kill_y, -1000.0, 0.0, "Fall limit (m)");
        v.exit();
        nested(v, "movement", &mut self.config.movement);
        nested(v, "camera", &mut self.camera);
        nested(v, "env", &mut self.env);
    }
}
