//! Player/NPC character controller: a kinematic capsule moved by rapier's character controller,
//! with pluggable movement models, jumping, crouch/crawl, ladders, ledge grabs, swimming,
//! moving platforms, knockback and bomb throwing.

use glam::{Vec2, Vec3};
use rapier::control::{CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::choice_enum;
use crate::entity::EntityId;
use crate::frame::SimEvent;
use crate::input::{InputFrame, buttons};
use crate::params::{ChoiceParam, ParamVisitor, Tunable};
use crate::physics::entity_from_tag;
use crate::puppet::{AnimInput, PuppetDef, PuppetState};
use crate::sim::SimState;
use crate::statics::Ladder;
use crate::zones::ZoneKind;

choice_enum! {
    /// How input turns into motion. Rooms mark the model they were designed for.
    #[derive(Default)]
    pub enum MovementModel {
        #[default]
        Instant => "instant",
        Momentum => "momentum",
        /// Step by step on the 1 m tile grid.
        Grid => "grid",
        /// Turning takes time, jumps keep their take-off velocity, crouch while moving rolls.
        Committed => "committed",
    }
}

choice_enum! {
    /// Lock motion along a world axis (side-view rooms).
    #[derive(Default)]
    pub enum LockAxis {
        #[default]
        None => "none",
        X => "x",
        Z => "z",
    }
}

choice_enum! {
    #[derive(Default)]
    pub enum Posture {
        #[default]
        Stand => "stand",
        Crouch => "crouch",
        Crawl => "crawl",
    }
}

/// Capsule radius of characters (m).
pub const RADIUS: f32 = 0.32;
/// Feet below the ledge top while hanging (m).
const HANG_DROP: f32 = 1.62;

impl Posture {
    pub fn height(self) -> f32 {
        match self {
            Posture::Stand => 1.7,
            Posture::Crouch => 1.15,
            Posture::Crawl => 0.66,
        }
    }
    pub fn half_height(self) -> f32 {
        (self.height() * 0.5 - RADIUS).max(0.01)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MovementParams {
    pub model: MovementModel,
    /// Instant model: run speed and hold-to-slow "focus" speed (m/s).
    pub speed: f32,
    pub focus_speed: f32,
    /// Momentum / committed models.
    pub max_speed: f32,
    pub accel: f32,
    pub decel: f32,
    pub skid_decel: f32,
    pub air_control: f32,
    /// Grid model: seconds per 1 m step.
    pub grid_step_time: f32,
    /// Committed model: turning speed (deg/s) and the dodge roll.
    pub turn_rate: f32,
    pub roll_speed: f32,
    pub roll_time: f32,
    /// Shared.
    pub gravity: f32,
    pub jump_height: f32,
    pub allow_jump: bool,
    pub jump_cut: f32,
    pub coyote_time: f32,
    pub jump_buffer: f32,
    pub max_fall: f32,
    pub step_height: f32,
    pub crouch_mult: f32,
    pub crawl_mult: f32,
    pub climb_speed: f32,
    pub ledge_grab: bool,
    pub ledge_climb_time: f32,
    pub swim_speed: f32,
    pub wade_mult: f32,
    pub hit_stun: f32,
    pub lock_axis: LockAxis,
    pub face_aim: bool,
    pub push_mass: f32,
}

impl Default for MovementParams {
    fn default() -> Self {
        Self {
            model: MovementModel::Instant,
            speed: 6.0,
            focus_speed: 2.2,
            max_speed: 7.5,
            accel: 45.0,
            decel: 32.0,
            skid_decel: 90.0,
            air_control: 0.55,
            grid_step_time: 0.16,
            turn_rate: 540.0,
            roll_speed: 8.5,
            roll_time: 0.42,
            gravity: 32.0,
            jump_height: 1.35,
            allow_jump: true,
            jump_cut: 0.45,
            coyote_time: 0.1,
            jump_buffer: 0.12,
            max_fall: 26.0,
            step_height: 0.32,
            crouch_mult: 0.45,
            crawl_mult: 0.32,
            climb_speed: 3.2,
            ledge_grab: true,
            ledge_climb_time: 0.38,
            swim_speed: 3.2,
            wade_mult: 0.65,
            hit_stun: 0.35,
            lock_axis: LockAxis::None,
            face_aim: false,
            push_mass: 70.0,
        }
    }
}

impl Tunable for MovementParams {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        self.model.visit_choice(v, "model", "Movement model");
        v.float("speed", &mut self.speed, 0.5, 20.0, "Instant: run speed (m/s)");
        v.float("focus_speed", &mut self.focus_speed, 0.2, 10.0, "Instant: speed while holding focus (m/s)");
        v.float("max_speed", &mut self.max_speed, 0.5, 25.0, "Momentum/committed: top speed (m/s)");
        v.float("accel", &mut self.accel, 1.0, 200.0, "Momentum/committed: acceleration (m/s²)");
        v.float("decel", &mut self.decel, 1.0, 200.0, "Momentum: slowdown with no input (m/s²)");
        v.float("skid_decel", &mut self.skid_decel, 1.0, 300.0, "Momentum: braking when reversing (m/s²)");
        v.float("air_control", &mut self.air_control, 0.0, 1.0, "Share of control while airborne");
        v.float("grid_step_time", &mut self.grid_step_time, 0.04, 1.0, "Grid: seconds per tile");
        v.float("turn_rate", &mut self.turn_rate, 30.0, 2000.0, "Committed: turning speed (deg/s)");
        v.float("roll_speed", &mut self.roll_speed, 1.0, 20.0, "Committed: dodge roll speed (m/s)");
        v.float("roll_time", &mut self.roll_time, 0.1, 1.5, "Committed: dodge roll duration (s)");
        v.float("gravity", &mut self.gravity, 1.0, 80.0, "Character gravity (m/s²)");
        v.float("jump_height", &mut self.jump_height, 0.0, 5.0, "Jump apex height (m)");
        v.bool("allow_jump", &mut self.allow_jump, "Jumping allowed (off for flat rooms)");
        v.float("jump_cut", &mut self.jump_cut, 0.0, 1.0, "Upward speed kept when jump is released early");
        v.float("coyote_time", &mut self.coyote_time, 0.0, 0.5, "Jump grace after leaving a ledge (s)");
        v.float("jump_buffer", &mut self.jump_buffer, 0.0, 0.5, "Early jump presses are remembered (s)");
        v.float("max_fall", &mut self.max_fall, 2.0, 80.0, "Terminal fall speed (m/s)");
        v.float("step_height", &mut self.step_height, 0.0, 0.8, "Auto-step over ledges up to (m)");
        v.float("crouch_mult", &mut self.crouch_mult, 0.05, 1.0, "Speed multiplier when crouching");
        v.float("crawl_mult", &mut self.crawl_mult, 0.05, 1.0, "Speed multiplier when crawling");
        v.float("climb_speed", &mut self.climb_speed, 0.5, 10.0, "Ladder climb speed (m/s)");
        v.bool("ledge_grab", &mut self.ledge_grab, "Grab ledges while falling next to them");
        v.float("ledge_climb_time", &mut self.ledge_climb_time, 0.05, 1.5, "Seconds to pull up onto a ledge");
        v.float("swim_speed", &mut self.swim_speed, 0.5, 10.0, "Swimming speed (m/s)");
        v.float("wade_mult", &mut self.wade_mult, 0.1, 1.0, "Speed multiplier in shallow water");
        v.float("hit_stun", &mut self.hit_stun, 0.0, 2.0, "Seconds without control after a hit");
        self.lock_axis.visit_choice(v, "lock_axis", "Lock motion along an axis (side view)");
        v.bool("face_aim", &mut self.face_aim, "Face the aim point instead of the movement direction");
        v.float("push_mass", &mut self.push_mass, 1.0, 500.0, "How hard the character pushes props (kg)");
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BombParams {
    pub fuse: f32,
    pub radius: f32,
    pub cooldown: f32,
    pub throw_range: f32,
    pub flight_time: f32,
    pub push: f32,
}

impl Default for BombParams {
    fn default() -> Self {
        Self { fuse: 1.4, radius: 1.35, cooldown: 0.3, throw_range: 8.0, flight_time: 0.55, push: 9.0 }
    }
}

impl Tunable for BombParams {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.float("fuse", &mut self.fuse, 0.1, 6.0, "Seconds until a bomb explodes");
        v.float("radius", &mut self.radius, 0.3, 6.0, "Blast radius (m): destructible tiles inside are removed");
        v.float("cooldown", &mut self.cooldown, 0.0, 3.0, "Seconds between throws");
        v.float("throw_range", &mut self.throw_range, 0.0, 30.0, "Maximum throw distance (m)");
        v.float("flight_time", &mut self.flight_time, 0.1, 2.0, "Throw arc duration (s)");
        v.float("push", &mut self.push, 0.0, 40.0, "Blast push strength");
    }
}

/// Hanging from a ledge.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Hang {
    /// Wall normal (horizontal, pointing at the character).
    pub normal: Vec3,
    /// Point on the wall face at the ledge.
    pub wall: Vec3,
    /// Height of the ledge top.
    pub top: f32,
    /// Pull-up progress 0..1, or < 0 while just hanging.
    pub climb: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Character {
    pub vel: Vec3,
    pub grounded: bool,
    pub air_time: f32,
    pub since_jump_press: f32,
    pub jumping: bool,
    pub posture: Posture,
    pub crawl_toggle: bool,
    pub climbing: Option<Ladder>,
    /// Yaw in radians (0 = facing +Z).
    pub facing: f32,
    pub anim: PuppetState,
    pub bomb_cooldown: f32,
    /// Knockback applied on the next tick (explosions, hits).
    pub impulse: Vec3,
    #[serde(default)]
    pub hang: Option<Hang>,
    /// Seconds without ledge grabbing (after letting go).
    #[serde(default)]
    pub hang_cooldown: f32,
    /// Committed model: dodge roll time left and direction.
    #[serde(default)]
    pub roll: f32,
    #[serde(default)]
    pub roll_dir: Vec3,
    /// Grid model: tile centre being walked to, and ticks spent blocked.
    #[serde(default)]
    pub grid_target: Option<Vec3>,
    #[serde(default)]
    pub grid_blocked: u32,
    /// Water depth at the feet (0 = dry) and whether swimming.
    #[serde(default)]
    pub water_depth: f32,
    #[serde(default)]
    pub swimming: bool,
    /// Seconds without control (hits) and of invulnerability.
    #[serde(default)]
    pub stun: f32,
    #[serde(default)]
    pub invuln: f32,
    /// Axis-lock coordinate (captured when the lock turns on).
    #[serde(default)]
    pub lock: Option<f32>,
    /// Feet height when last on the ground, and whether this airtime started with a jump
    /// (walking off an edge only grabs ledges above where you came from).
    #[serde(default)]
    pub air_from: f32,
    #[serde(default)]
    pub air_jumped: bool,
}

impl Character {
    pub fn new() -> Self {
        Self { since_jump_press: 10.0, ..Default::default() }
    }
    pub fn height(&self) -> f32 {
        self.posture.height()
    }
}

/// Requests the simulation carries out after a character update.
pub enum Action {
    ThrowBomb { from: Vec3, vel: Vec3, owner: EntityId },
    /// Touched a hazard or got crushed.
    Hit { id: EntityId, at: Vec3, dir: Vec3, knockback: f32, respawn: bool },
}

fn yaw_of(d: Vec3) -> f32 {
    d.x.atan2(d.z)
}

fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn dir_of(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

/// Finds a grabbable ledge in front of a character: returns (wall point, wall normal, top).
fn find_ledge(st: &SimState, filter: QueryFilter, feet: Vec3, dir: Vec3) -> Option<(Vec3, Vec3, f32)> {
    let qp = st.physics.query_filtered(filter);
    let reach = RADIUS + 0.45;
    // A wall in front at chest height, or at the waist when the top is close.
    let hit = [1.25f32, 0.55].into_iter().find_map(|h| {
        let from = feet + Vec3::Y * h;
        qp.cast_ray_and_get_normal(&Ray::new(from, dir), reach as Real, true).map(|(_, hit)| (from, hit))
    });
    let (chest, hit) = hit?;
    let n = hit.normal;
    if n.y.abs() > 0.3 {
        return None;
    }
    let n = Vec3::new(n.x, 0.0, n.z).normalize_or(-dir);
    let wall = chest + dir * hit.time_of_impact as f32;
    // The top: cast down just behind the wall face, from above the highest reachable point.
    let start = Vec3::new(wall.x, feet.y + 2.15, wall.z) - n * 0.15;
    let span = 2.15 - 0.35;
    let (_, down) = qp.cast_ray_and_get_normal(&Ray::new(start, Vec3::NEG_Y), span as Real, true)?;
    if down.time_of_impact < 0.02 || down.normal.y < 0.7 {
        return None; // the wall continues above: no ledge
    }
    let top = start.y - down.time_of_impact as f32;
    if top < feet.y + 0.4 {
        return None;
    }
    // Room for the head above the ledge and for standing on it.
    let head = Vec3::new(wall.x, top + 0.35, wall.z) + n * reach;
    if qp.cast_ray(&Ray::new(head, dir), (reach + 0.35) as Real, true).is_some() {
        return None;
    }
    let stand = SharedShape::capsule_y(Posture::Stand.half_height(), RADIUS - 0.04);
    let pos = Vec3::new(wall.x, top + Posture::Stand.height() * 0.5 + 0.03, wall.z) - n * (RADIUS + 0.1);
    if qp.intersect_shape(Pose::from_translation(pos), stand.as_ref()).next().is_some() {
        return None;
    }
    Some((wall, n, top))
}

/// Advances one character by one tick.
#[allow(clippy::too_many_arguments)]
pub fn tick(
    st: &mut SimState,
    id: EntityId,
    input: &InputFrame,
    mp: &MovementParams,
    bp: &BombParams,
    puppet: &PuppetDef,
    prop_gravity: f32,
    dt: f32,
    events: &mut Vec<SimEvent>,
    actions: &mut Vec<Action>,
) {
    let Some(ent) = st.entities.map.get_mut(&id) else { return };
    let (Some(body_h), Some(mut ch)) = (ent.body, ent.character.take()) else { return };
    let Some(col_h) = st.physics.bodies.get(body_h).and_then(|b| b.colliders().first().copied()) else {
        ent.character = Some(ch);
        return;
    };
    let mut center = st.physics.bodies[body_h].translation();
    let last_vel = ch.vel;
    let was_grounded = ch.grounded;
    let filter = QueryFilter::default().exclude_rigid_body(body_h).exclude_sensors();

    // --- timers and input
    ch.stun = (ch.stun - dt).max(0.0);
    ch.invuln = (ch.invuln - dt).max(0.0);
    ch.hang_cooldown = (ch.hang_cooldown - dt).max(0.0);
    ch.roll = (ch.roll - dt).max(0.0);
    let stunned = ch.stun > 0.0;
    let mut wish = if stunned { Vec3::ZERO } else { Vec3::new(input.move_dir.x, 0.0, input.move_dir.y) };
    if wish.length() > 1.0 {
        wish = wish.normalize();
    }
    match mp.lock_axis {
        LockAxis::None => ch.lock = None,
        LockAxis::X => {
            wish.x = 0.0;
            ch.lock.get_or_insert(center.x);
        }
        LockAxis::Z => {
            wish.z = 0.0;
            ch.lock.get_or_insert(center.z);
        }
    }
    let pressed = |b: u32| !stunned && input.just(b);
    let held = |b: u32| !stunned && input.down(b);
    ch.since_jump_press = if pressed(buttons::JUMP) { 0.0 } else { ch.since_jump_press + dt };
    if pressed(buttons::CRAWL) {
        ch.crawl_toggle = !ch.crawl_toggle;
    }
    ch.bomb_cooldown = (ch.bomb_cooldown - dt).max(0.0);

    // --- moving platforms: get pushed out of kinematic objects, ride the one underneath
    let kcc_basic = KinematicCharacterController {
        up: Vector::Y,
        offset: CharacterLength::Absolute(0.02),
        slide: true,
        autostep: None,
        snap_to_ground: None,
        ..Default::default()
    };
    {
        let shape = st.physics.colliders[col_h].shared_shape().clone();
        let pose = Pose::from_translation(center);
        let mut push = Vec3::ZERO;
        let mut pushers = Vec::new();
        let mut hazard: Option<(Vec3, f32, bool)> = None;
        {
            let qp = st.physics.query_filtered(filter);
            // Hazards: a slightly fatter capsule (the controller keeps a small gap to walls).
            let fat = SharedShape::capsule_y(ch.posture.half_height(), RADIUS + 0.08);
            for (_, c) in qp.intersect_shape(pose, fat.as_ref()) {
                let Some(e) = entity_from_tag(c.user_data).and_then(|i| st.entities.get(EntityId(i))) else { continue };
                if let Some(hz) = &e.hazard {
                    hazard.get_or_insert((e.pos, hz.knockback, hz.respawn));
                }
            }
            for (h, c) in qp.intersect_shape(pose, shape.as_ref()) {
                let Some(eid) = entity_from_tag(c.user_data).map(EntityId) else { continue };
                let Some(e) = st.entities.get(eid) else { continue };
                let kinematic = c.parent().and_then(|b| st.physics.bodies.get(b)).is_some_and(|b| b.is_kinematic());
                if kinematic && e.character.is_none() {
                    if let Ok(Some(ct)) = rapier::parry::query::contact(&pose, shape.as_ref(), c.position(), c.shape(), 0.0) {
                        if ct.dist < 0.0 {
                            push += ct.normal2 * (-ct.dist + 0.01);
                            pushers.push(h);
                        }
                    }
                }
            }
        }
        if let Some((from, kb, respawn)) = hazard {
            if ch.invuln <= 0.0 {
                actions.push(Action::Hit { id, at: center, dir: center - from, knockback: kb, respawn });
            }
        }
        // Ride: the kinematic object under the feet carries the character.
        let mut ride = Vec3::ZERO;
        let mut carrier = None;
        if was_grounded && ch.vel.y <= 0.5 {
            let qp = st.physics.query_filtered(filter);
            let ray = Ray::new(center, Vec3::NEG_Y);
            if let Some((h, _toi)) = qp.cast_ray(&ray, (ch.height() * 0.5 + 0.15) as Real, true) {
                let c = &st.physics.colliders[h];
                if let Some(b) = c.parent().and_then(|b| st.physics.bodies.get(b)) {
                    let is_char = entity_from_tag(c.user_data).and_then(|e| st.entities.get(EntityId(e))).is_some_and(|e| e.character.is_some());
                    if b.is_kinematic() && !is_char {
                        let feet = center - Vec3::Y * ch.height() * 0.5;
                        ride = b.velocity_at_point(feet) * dt;
                        carrier = Some(h);
                    }
                }
            }
        }
        let carry = push + ride;
        if carry.length_squared() > 1e-10 {
            let mut f = filter;
            let skip: Vec<ColliderHandle> = pushers.iter().copied().chain(carrier).collect();
            let pred = |h: ColliderHandle, _: &Collider| !skip.contains(&h);
            f = f.predicate(&pred);
            let qp = st.physics.query_filtered(f);
            let mv = kcc_basic.move_shape(dt as Real, &qp, shape.as_ref(), &pose, carry, |_| {});
            center += mv.translation;
            if push.length() > 0.05 && (mv.translation - carry).length() > 0.25 && ch.invuln <= 0.0 {
                // Squeezed between a moving object and a wall.
                actions.push(Action::Hit { id, at: center, dir: push, knockback: 2.0, respawn: true });
            }
        }
    }

    // --- water
    let feet0 = center - Vec3::Y * (ch.height() * 0.5);
    ch.water_depth = st
        .statics
        .zones_at(feet0 + Vec3::Y * 0.05)
        .filter(|(_, z)| z.kind == ZoneKind::Water)
        .map(|(_, z)| z.max.y - feet0.y)
        .fold(0.0, f32::max);
    let was_swimming = ch.swimming;
    ch.swimming = ch.water_depth > if was_swimming { 1.05 } else { 1.25 } && ch.climbing.is_none() && ch.hang.is_none();
    let rolling = ch.roll > 0.0;

    // --- posture (never stand up into a ceiling)
    let want = if ch.climbing.is_some() || ch.hang.is_some() || ch.swimming {
        Posture::Stand
    } else if rolling || ch.crawl_toggle {
        Posture::Crawl
    } else if held(buttons::CROUCH) && !(mp.model == MovementModel::Committed && wish.length() > 0.3) {
        Posture::Crouch
    } else {
        Posture::Stand
    };
    if want != ch.posture {
        let feet = center - Vec3::Y * (ch.height() * 0.5);
        let fits = |p: Posture| -> bool {
            if p.height() <= ch.posture.height() {
                return true;
            }
            let shape = SharedShape::capsule_y(p.half_height(), RADIUS - 0.02);
            let pose = Pose::from_translation(feet + Vec3::Y * (p.height() * 0.5 + 0.01));
            st.physics.query_filtered(filter).intersect_shape(pose, shape.as_ref()).next().is_none()
        };
        let next = if fits(want) {
            Some(want)
        } else if want == Posture::Stand && fits(Posture::Crouch) {
            Some(Posture::Crouch)
        } else {
            None
        };
        if let Some(p) = next.filter(|p| *p != ch.posture) {
            ch.posture = p;
            if let Some(c) = st.physics.colliders.get_mut(col_h) {
                c.set_shape(SharedShape::capsule_y(p.half_height(), RADIUS));
            }
            center = feet + Vec3::Y * (p.height() * 0.5);
        }
    }
    let height = ch.height();
    let feet = center - Vec3::Y * (height * 0.5);

    // --- ladders
    let mut jumped = false;
    if let Some(l) = ch.climbing.clone() {
        let wall = l.facing.dir();
        let up_in = (wish.dot(wall) + input.vertical).clamp(-1.0, 1.0);
        if !l.overlaps(feet, RADIUS, height) {
            ch.climbing = None;
        } else if pressed(buttons::JUMP) {
            ch.climbing = None;
            ch.vel = -wall * 4.0 + Vec3::Y * 6.0;
            jumped = true;
        } else if feet.y >= l.top() - 0.2 && up_in > 0.1 {
            // Pull up over the lip.
            ch.climbing = None;
            ch.vel = wall * 3.0 + Vec3::Y * 5.2;
        } else if ch.grounded && up_in < -0.3 {
            ch.climbing = None;
        } else {
            let c = l.center();
            let target = Vec3::new(c.x, feet.y, c.z) - wall * (RADIUS * 0.6);
            let pull = (target - feet) * 12.0;
            ch.vel = Vec3::new(pull.x, up_in * mp.climb_speed, pull.z);
            ch.facing = yaw_of(wall);
            ch.jumping = false;
        }
    } else if ch.posture != Posture::Crawl && wish.length() > 0.3 && ch.hang.is_none() && !ch.swimming {
        if let Some(l) = st
            .statics
            .ladders_near(feet)
            .find(|l| l.overlaps(feet, RADIUS, height) && wish.normalize().dot(l.facing.dir()) > 0.5 && feet.y < l.top() - 0.3)
        {
            ch.climbing = Some(l.clone());
            ch.vel = Vec3::ZERO;
            ch.jumping = false;
        }
    }
    let climbing = ch.climbing.is_some();

    // --- ledges: grab while falling against a wall top, then hang / shimmy / pull up / drop
    if !climbing
        && ch.hang.is_none()
        && mp.ledge_grab
        && !ch.grounded
        && !ch.swimming
        && ch.posture == Posture::Stand
        && ch.vel.y < 1.0
        && ch.hang_cooldown <= 0.0
        && wish.length() > 0.3
    {
        let dir = wish.normalize();
        if let Some((wall, n, top)) = find_ledge(st, filter, feet, dir) {
            if top - feet.y <= 2.1 && (ch.air_jumped || top > ch.air_from + 0.3) {
                // Close to the top: mantle straight up instead of hanging.
                let climb = if top - feet.y < HANG_DROP - 0.3 { 0.0 } else { -1.0 };
                ch.hang = Some(Hang { normal: n, wall, top, climb });
                ch.vel = Vec3::ZERO;
                ch.jumping = false;
                ch.facing = yaw_of(-n);
                events.push(SimEvent::Grab { pos: Vec3::new(wall.x, top, wall.z) });
            }
        }
    }
    let mut hang_move: Option<Vec3> = None;
    if let Some(mut hg) = ch.hang {
        let rest = Vec3::new(hg.wall.x, hg.top - HANG_DROP, hg.wall.z) + hg.normal * (RADIUS + 0.08);
        let tangent = Vec3::new(-hg.normal.z, 0.0, hg.normal.x);
        if hg.climb >= 0.0 {
            hg.climb += dt / mp.ledge_climb_time.max(0.02);
            let target = if hg.climb < 0.6 {
                Vec3::new(rest.x, hg.top + 0.04, rest.z)
            } else {
                Vec3::new(hg.wall.x, hg.top + 0.04, hg.wall.z) - hg.normal * 0.45
            };
            let left = if hg.climb < 0.6 { 0.6 - hg.climb } else { 1.0 - hg.climb };
            let steps = (left * mp.ledge_climb_time / dt).max(1.0);
            hang_move = Some((target - feet) / steps);
            if hg.climb >= 1.0 {
                ch.hang = None;
                ch.vel = Vec3::ZERO;
                ch.grounded = true;
            } else {
                ch.hang = Some(hg);
            }
        } else if pressed(buttons::JUMP) || wish.dot(-hg.normal) > 0.5 {
            hg.climb = 0.0;
            ch.hang = Some(hg);
            hang_move = Some(Vec3::ZERO);
        } else if pressed(buttons::CROUCH) || wish.dot(hg.normal) > 0.6 {
            ch.hang = None;
            ch.hang_cooldown = 0.35;
            ch.vel = hg.normal * 1.5;
            ch.air_jumped = false;
            ch.air_from = hg.top;
        } else {
            // Shimmy along the ledge while there is one.
            let side = wish.dot(tangent);
            let mut target = rest;
            if side.abs() > 0.3 {
                let probe = feet + tangent * side.signum() * 0.25;
                if let Some((wall, n, top)) = find_ledge(st, filter, Vec3::new(probe.x, hg.top - HANG_DROP, probe.z), -hg.normal) {
                    if (top - hg.top).abs() < 0.3 && n.dot(hg.normal) > 0.9 {
                        let step = tangent * side.signum() * (1.6 * dt).min(0.25);
                        hg.wall = Vec3::new(hg.wall.x, wall.y, hg.wall.z) + step;
                        hg.wall = hg.wall - n * (hg.wall - wall).dot(n);
                        hg.top = top;
                        hg.normal = n;
                        target = Vec3::new(hg.wall.x, hg.top - HANG_DROP, hg.wall.z) + n * (RADIUS + 0.08);
                    }
                }
            }
            ch.hang = Some(hg);
            hang_move = Some((target - feet) * (1.0 - (-25.0 * dt).exp()));
            ch.facing = yaw_of(-hg.normal);
        }
    }
    let hanging = ch.hang.is_some();

    // --- horizontal movement
    if !climbing && !hanging {
        let mult = match ch.posture {
            Posture::Stand => 1.0,
            Posture::Crouch => mp.crouch_mult,
            Posture::Crawl => mp.crawl_mult,
        } * if ch.water_depth > 0.3 && !ch.swimming { mp.wade_mult } else { 1.0 };
        let mut vh = Vec3::new(ch.vel.x, 0.0, ch.vel.z);
        if stunned {
            // Knocked back: slide to a stop, no control.
            vh *= (-3.0 * dt).exp();
        } else if ch.swimming {
            let target = wish * mp.swim_speed;
            vh = vh.lerp(target, 1.0 - (-5.0 * dt).exp());
        } else {
            match mp.model {
                MovementModel::Instant => {
                    let s = if held(buttons::FOCUS) { mp.focus_speed } else { mp.speed } * mult;
                    let target = wish * s;
                    if ch.grounded || mp.air_control >= 0.99 {
                        vh = target;
                    } else {
                        vh = vh.lerp(target, 1.0 - (-mp.air_control * 25.0 * dt).exp());
                    }
                }
                MovementModel::Momentum => {
                    let max = mp.max_speed * mult * if held(buttons::FOCUS) { 0.5 } else { 1.0 };
                    let target = wish * max;
                    let ctrl = if ch.grounded { 1.0 } else { mp.air_control };
                    let rate = if wish.length() < 0.05 {
                        mp.decel
                    } else if vh.dot(wish) < 0.0 {
                        mp.skid_decel
                    } else {
                        mp.accel
                    };
                    let dv = (target - vh).clamp_length_max(rate * ctrl * dt);
                    vh += dv;
                }
                MovementModel::Committed => {
                    if rolling {
                        vh = ch.roll_dir * mp.roll_speed;
                    } else if ch.grounded {
                        if wish.length() > 0.1 {
                            let d = wrap(yaw_of(wish) - ch.facing);
                            let max_turn = mp.turn_rate.to_radians() * dt;
                            ch.facing = wrap(ch.facing + d.clamp(-max_turn, max_turn));
                        }
                        let fwd = dir_of(ch.facing);
                        let align = wish.normalize_or_zero().dot(fwd).max(0.0);
                        let target = mp.max_speed * mult * wish.length() * align * if held(buttons::FOCUS) { 0.5 } else { 1.0 };
                        let cur = vh.dot(fwd).max(0.0);
                        let rate = if target > cur { mp.accel } else { mp.decel };
                        let s = cur + (target - cur).clamp(-rate * dt, rate * dt);
                        vh = fwd * s;
                        if pressed(buttons::CROUCH) && wish.length() > 0.3 {
                            ch.roll = mp.roll_time;
                            ch.roll_dir = wish.normalize();
                            ch.facing = yaw_of(ch.roll_dir);
                            events.push(SimEvent::Roll { pos: feet });
                        }
                    }
                    // Airborne: keep the take-off velocity (no air control).
                }
                MovementModel::Grid => {
                    let tile = |p: Vec3| Vec3::new(p.x.floor() + 0.5, 0.0, p.z.floor() + 0.5);
                    let mut target = ch.grid_target.unwrap_or(tile(feet));
                    let d = Vec3::new(target.x - feet.x, 0.0, target.z - feet.z);
                    let speed = mult / mp.grid_step_time.max(0.02);
                    if d.length() < 0.03 {
                        let w = Vec2::new(wish.x, wish.z);
                        if w.length() > 0.4 {
                            let step = if w.x.abs() > w.y.abs() { Vec3::X * w.x.signum() } else { Vec3::Z * w.y.signum() };
                            target = tile(feet) + step;
                            ch.facing = yaw_of(step);
                        }
                    }
                    let d = Vec3::new(target.x - feet.x, 0.0, target.z - feet.z);
                    let dist = d.length();
                    vh = if dist > 1e-4 { d / dist * speed.min(dist / dt) } else { Vec3::ZERO };
                    // Blocked for a while: go back to the tile we're on.
                    let moved = Vec2::new(last_vel.x, last_vel.z).length();
                    if dist > 0.05 && moved < speed * 0.2 && ch.grid_target == Some(target) {
                        ch.grid_blocked += 1;
                        if ch.grid_blocked > 6 {
                            target = tile(feet);
                            ch.grid_blocked = 0;
                        }
                    } else {
                        ch.grid_blocked = 0;
                    }
                    ch.grid_target = Some(target);
                }
            }
        }
        if mp.model != MovementModel::Grid {
            ch.grid_target = None;
        }
        ch.vel.x = vh.x;
        ch.vel.z = vh.z;

        // --- vertical movement
        if ch.swimming {
            ch.jumping = false;
            let surface = feet.y + ch.water_depth;
            let float_feet = surface - 1.3;
            let target_vy = if held(buttons::CROUCH) {
                -mp.swim_speed * 0.8
            } else if held(buttons::JUMP) && ch.water_depth > 1.6 {
                mp.swim_speed * 0.8
            } else {
                ((float_feet - feet.y) * 4.0).clamp(-3.0, 3.0)
            };
            ch.vel.y += (target_vy - ch.vel.y) * (1.0 - (-6.0 * dt).exp());
            // Climb out: jump while floating at the surface.
            if pressed(buttons::JUMP) && ch.water_depth < 1.6 && mp.allow_jump {
                ch.vel.y = (2.0 * mp.gravity * mp.jump_height * 0.9).sqrt();
                ch.swimming = false;
                jumped = true;
            }
        } else {
            let can_jump = (ch.grounded || ch.air_time < mp.coyote_time) && ch.posture != Posture::Crawl && mp.allow_jump && !rolling;
            if ch.since_jump_press <= mp.jump_buffer && can_jump && !ch.jumping {
                ch.vel.y = (2.0 * mp.gravity * mp.jump_height).sqrt();
                ch.jumping = true;
                ch.grounded = false;
                ch.air_time = mp.coyote_time + 1.0;
                ch.since_jump_press = 10.0;
                jumped = true;
            }
            let variable = mp.model != MovementModel::Committed;
            if variable && ch.jumping && !held(buttons::JUMP) && ch.vel.y > 0.0 {
                ch.vel.y *= mp.jump_cut;
                ch.jumping = false;
            }
            if !jumped || ch.vel.y <= 0.0 {
                ch.vel.y -= mp.gravity * dt;
            }
            if ch.vel.y <= 0.0 {
                ch.jumping = false;
            }
            ch.vel.y = ch.vel.y.max(-mp.max_fall);
        }
    }
    if hanging {
        ch.vel = Vec3::ZERO;
    }
    ch.vel += ch.impulse;
    ch.impulse = Vec3::ZERO;
    if jumped {
        events.push(SimEvent::Jump { pos: feet });
        ch.air_jumped = true;
    }
    // Axis lock: no motion along the axis, drift corrected.
    let mut lock_fix = Vec3::ZERO;
    if let Some(v) = ch.lock {
        match mp.lock_axis {
            LockAxis::X => {
                ch.vel.x = 0.0;
                lock_fix.x = v - center.x;
            }
            LockAxis::Z => {
                ch.vel.z = 0.0;
                lock_fix.z = v - center.z;
            }
            LockAxis::None => {}
        }
    }

    // --- move with the kinematic character controller
    let kcc = KinematicCharacterController {
        up: Vector::Y,
        offset: CharacterLength::Absolute(0.02),
        slide: true,
        autostep: (!climbing && !hanging).then_some(CharacterAutostep {
            max_height: CharacterLength::Absolute(mp.step_height as Real),
            min_width: CharacterLength::Absolute(0.12),
            include_dynamic_bodies: false,
        }),
        max_slope_climb_angle: 50f32.to_radians() as Real,
        min_slope_slide_angle: 40f32.to_radians() as Real,
        snap_to_ground: (was_grounded && ch.vel.y <= 0.0 && !climbing && !hanging && !ch.swimming)
            .then_some(CharacterLength::Absolute(0.25)),
        normal_nudge_factor: 1.0e-4,
    };
    let shape = st.physics.colliders[col_h].shared_shape().clone();
    let desired = match hang_move {
        Some(m) => m,
        None => ch.vel * dt + lock_fix,
    };
    let mut collisions: Vec<CharacterCollision> = Vec::new();
    let mv = {
        let qp = st.physics.query_filtered(filter);
        kcc.move_shape(dt as Real, &qp, shape.as_ref(), &Pose::from_translation(center), desired, |c| collisions.push(c))
    };
    {
        let ph = &mut st.physics;
        let mut qpm =
            ph.broad_phase.as_query_pipeline_mut(ph.narrow_phase.query_dispatcher(), &mut ph.bodies, &mut ph.colliders, filter);
        kcc.solve_character_collision_impulses(dt as Real, &mut qpm, shape.as_ref(), mp.push_mass as Real, collisions.iter());
    }
    let moved = mv.translation;
    let new_center = center + moved;
    if let Some(b) = st.physics.bodies.get_mut(body_h) {
        b.set_translation(new_center, true);
        b.set_next_kinematic_translation(new_center);
    }

    // --- resolve velocity from what actually happened
    ch.grounded = (mv.grounded && !(ch.jumping && ch.vel.y > 0.0)) || (ch.grounded && hanging);
    if hanging {
        ch.grounded = false;
    }
    let mut landed = 0.0;
    if ch.grounded {
        if !was_grounded && last_vel.y < -2.0 {
            landed = -last_vel.y;
            events.push(SimEvent::Land { pos: new_center - Vec3::Y * height * 0.5, speed: landed });
        }
        ch.vel.y = ch.vel.y.max(0.0).min(if climbing { ch.vel.y } else { 0.0 });
        ch.air_time = 0.0;
        ch.air_from = new_center.y - height * 0.5;
        ch.air_jumped = false;
    } else {
        ch.air_time += dt;
        if ch.vel.y > 0.0 && moved.y < desired.y * 0.5 && !climbing && !hanging && !ch.swimming {
            ch.vel.y = 0.0; // bumped the ceiling
            ch.jumping = false;
        }
    }
    if dt > 0.0 && !climbing && !hanging {
        // Blocked by a wall: keep only the motion that actually happened (sliding along it).
        let actual = Vec2::new(moved.x - lock_fix.x, moved.z - lock_fix.z) / dt;
        if actual.length() + 0.01 < Vec2::new(ch.vel.x, ch.vel.z).length() {
            ch.vel.x = actual.x;
            ch.vel.z = actual.y;
        }
    }

    // --- facing
    if !climbing && !hanging && !stunned && matches!(mp.model, MovementModel::Instant | MovementModel::Momentum) {
        if mp.face_aim {
            if let Some(a) = input.aim {
                let d = a - new_center;
                if Vec2::new(d.x, d.z).length() > 0.2 {
                    ch.facing = yaw_of(d);
                }
            }
        } else if wish.length() > 0.1 {
            ch.facing = yaw_of(wish);
        }
    }

    // --- bombs
    if (pressed(buttons::USE) || pressed(buttons::PRIMARY)) && ch.bomb_cooldown <= 0.0 && !climbing && !hanging && !ch.swimming {
        ch.bomb_cooldown = bp.cooldown;
        let from = new_center + Vec3::Y * 0.35;
        let dir3 = dir_of(ch.facing);
        let (dir, dist) = match input.aim {
            Some(a) => {
                let d = Vec3::new(a.x - from.x, 0.0, a.z - from.z);
                (d.normalize_or(dir3), d.length().min(bp.throw_range))
            }
            None => (dir3, 0.0),
        };
        let t = bp.flight_time.max(0.05);
        let target_y = input.aim.map(|a| a.y).unwrap_or(feet.y);
        let vy = (target_y - from.y + 0.5 * prop_gravity * t * t) / t;
        let vel = dir * (dist / t) + Vec3::Y * vy;
        actions.push(Action::ThrowBomb { from: from + dir * 0.3, vel, owner: id });
        ch.anim.recoil = 1.0;
    }

    // --- animation
    let accel = (ch.vel - last_vel) / dt.max(1e-4);
    let phase_before = ch.anim.phase;
    ch.anim.update(
        puppet,
        &AnimInput {
            vel: ch.vel,
            accel,
            grounded: ch.grounded,
            crouch: ch.posture == Posture::Crouch,
            crawl: ch.posture == Posture::Crawl && !rolling,
            climbing: climbing || hanging,
            facing: ch.facing,
            landed,
            jumped,
            swimming: ch.swimming,
            rolling,
        },
        dt,
    );
    if hanging {
        ch.anim.climb_phase = 0.25;
    }
    // Footsteps when the walk cycle passes a contact point.
    if ch.grounded && !climbing && Vec2::new(ch.vel.x, ch.vel.z).length() > 0.5 {
        let (a, b) = (phase_before, ch.anim.phase);
        let crossed = |x: f32| if b >= a { a < x && b >= x } else { a < x || b >= x };
        if crossed(0.25) || crossed(0.75) {
            events.push(SimEvent::Step { pos: new_center - Vec3::Y * height * 0.5 });
        }
    }

    if let Some(e) = st.entities.map.get_mut(&id) {
        e.pos = new_center;
        e.character = Some(ch);
    }
}
