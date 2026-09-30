//! Player/NPC character controller: a kinematic capsule moved by rapier's character controller,
//! with pluggable movement models, jumping, crouch/crawl, ladder climbing and bomb throwing.

use glam::{Vec2, Vec3};
use rapier::control::{CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::choice_enum;
use crate::entity::EntityId;
use crate::frame::SimEvent;
use crate::input::{InputFrame, buttons};
use crate::params::{ChoiceParam, ParamVisitor, Tunable};
use crate::puppet::{AnimInput, PuppetDef, PuppetState};
use crate::sim::SimState;
use crate::statics::Ladder;

choice_enum! {
    /// How input turns into motion. Rooms mark the model they were designed for.
    #[derive(Default)]
    pub enum MovementModel {
        #[default]
        Instant => "instant",
        Momentum => "momentum",
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
    /// Momentum model.
    pub max_speed: f32,
    pub accel: f32,
    pub decel: f32,
    pub skid_decel: f32,
    pub air_control: f32,
    /// Shared.
    pub gravity: f32,
    pub jump_height: f32,
    pub jump_cut: f32,
    pub coyote_time: f32,
    pub jump_buffer: f32,
    pub max_fall: f32,
    pub step_height: f32,
    pub crouch_mult: f32,
    pub crawl_mult: f32,
    pub climb_speed: f32,
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
            gravity: 32.0,
            jump_height: 1.35,
            jump_cut: 0.45,
            coyote_time: 0.1,
            jump_buffer: 0.12,
            max_fall: 26.0,
            step_height: 0.32,
            crouch_mult: 0.45,
            crawl_mult: 0.32,
            climb_speed: 3.2,
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
        v.float("max_speed", &mut self.max_speed, 0.5, 25.0, "Momentum: top speed (m/s)");
        v.float("accel", &mut self.accel, 1.0, 200.0, "Momentum: acceleration (m/s²)");
        v.float("decel", &mut self.decel, 1.0, 200.0, "Momentum: slowdown with no input (m/s²)");
        v.float("skid_decel", &mut self.skid_decel, 1.0, 300.0, "Momentum: braking when reversing (m/s²)");
        v.float("air_control", &mut self.air_control, 0.0, 1.0, "Share of control while airborne");
        v.float("gravity", &mut self.gravity, 1.0, 80.0, "Character gravity (m/s²)");
        v.float("jump_height", &mut self.jump_height, 0.0, 5.0, "Jump apex height (m)");
        v.float("jump_cut", &mut self.jump_cut, 0.0, 1.0, "Upward speed kept when jump is released early");
        v.float("coyote_time", &mut self.coyote_time, 0.0, 0.5, "Jump grace after leaving a ledge (s)");
        v.float("jump_buffer", &mut self.jump_buffer, 0.0, 0.5, "Early jump presses are remembered (s)");
        v.float("max_fall", &mut self.max_fall, 2.0, 80.0, "Terminal fall speed (m/s)");
        v.float("step_height", &mut self.step_height, 0.0, 0.8, "Auto-step over ledges up to (m)");
        v.float("crouch_mult", &mut self.crouch_mult, 0.05, 1.0, "Speed multiplier when crouching");
        v.float("crawl_mult", &mut self.crawl_mult, 0.05, 1.0, "Speed multiplier when crawling");
        v.float("climb_speed", &mut self.climb_speed, 0.5, 10.0, "Ladder climb speed (m/s)");
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
    /// Knockback applied on the next tick (explosions).
    pub impulse: Vec3,
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
}

fn yaw_of(d: Vec3) -> f32 {
    d.x.atan2(d.z)
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

    // --- input
    let mut wish = Vec3::new(input.move_dir.x, 0.0, input.move_dir.y);
    if wish.length() > 1.0 {
        wish = wish.normalize();
    }
    ch.since_jump_press = if input.just(buttons::JUMP) { 0.0 } else { ch.since_jump_press + dt };
    if input.just(buttons::CRAWL) {
        ch.crawl_toggle = !ch.crawl_toggle;
    }
    ch.bomb_cooldown = (ch.bomb_cooldown - dt).max(0.0);

    // --- posture (never stand up into a ceiling)
    let filter = QueryFilter::default().exclude_rigid_body(body_h).exclude_sensors();
    let want = if ch.climbing.is_some() {
        Posture::Stand
    } else if ch.crawl_toggle {
        Posture::Crawl
    } else if input.down(buttons::CROUCH) {
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
        } else if input.just(buttons::JUMP) {
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
    } else if ch.posture != Posture::Crawl && wish.length() > 0.3 {
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

    // --- horizontal movement
    if !climbing {
        let mult = match ch.posture {
            Posture::Stand => 1.0,
            Posture::Crouch => mp.crouch_mult,
            Posture::Crawl => mp.crawl_mult,
        };
        let mut vh = Vec3::new(ch.vel.x, 0.0, ch.vel.z);
        match mp.model {
            MovementModel::Instant => {
                let s = if input.down(buttons::FOCUS) { mp.focus_speed } else { mp.speed } * mult;
                let target = wish * s;
                if ch.grounded || mp.air_control >= 0.99 {
                    vh = target;
                } else {
                    vh = vh.lerp(target, 1.0 - (-mp.air_control * 25.0 * dt).exp());
                }
            }
            MovementModel::Momentum => {
                let max = mp.max_speed * mult * if input.down(buttons::FOCUS) { 0.5 } else { 1.0 };
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
        }
        ch.vel.x = vh.x;
        ch.vel.z = vh.z;

        // --- vertical movement
        let can_jump = (ch.grounded || ch.air_time < mp.coyote_time) && ch.posture != Posture::Crawl;
        if ch.since_jump_press <= mp.jump_buffer && can_jump && !ch.jumping {
            ch.vel.y = (2.0 * mp.gravity * mp.jump_height).sqrt();
            ch.jumping = true;
            ch.grounded = false;
            ch.air_time = mp.coyote_time + 1.0;
            ch.since_jump_press = 10.0;
            jumped = true;
        }
        if ch.jumping && !input.down(buttons::JUMP) && ch.vel.y > 0.0 {
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
    ch.vel += ch.impulse;
    ch.impulse = Vec3::ZERO;
    if jumped {
        events.push(SimEvent::Jump { pos: feet });
    }

    // --- move with the kinematic character controller
    let kcc = KinematicCharacterController {
        up: Vector::Y,
        offset: CharacterLength::Absolute(0.02),
        slide: true,
        autostep: (!climbing).then_some(CharacterAutostep {
            max_height: CharacterLength::Absolute(mp.step_height as Real),
            min_width: CharacterLength::Absolute(0.12),
            include_dynamic_bodies: false,
        }),
        max_slope_climb_angle: 50f32.to_radians() as Real,
        min_slope_slide_angle: 40f32.to_radians() as Real,
        snap_to_ground: (was_grounded && ch.vel.y <= 0.0 && !climbing).then_some(CharacterLength::Absolute(0.25)),
        normal_nudge_factor: 1.0e-4,
    };
    let shape = st.physics.colliders[col_h].shared_shape().clone();
    let desired = ch.vel * dt;
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
        b.set_next_kinematic_translation(new_center);
    }

    // --- resolve velocity from what actually happened
    ch.grounded = mv.grounded && !(ch.jumping && ch.vel.y > 0.0);
    let mut landed = 0.0;
    if ch.grounded {
        if !was_grounded && last_vel.y < -2.0 {
            landed = -last_vel.y;
            events.push(SimEvent::Land { pos: new_center - Vec3::Y * height * 0.5, speed: landed });
        }
        ch.vel.y = ch.vel.y.max(0.0).min(if climbing { ch.vel.y } else { 0.0 });
        ch.air_time = 0.0;
    } else {
        ch.air_time += dt;
        if ch.vel.y > 0.0 && moved.y < desired.y * 0.5 && !climbing {
            ch.vel.y = 0.0; // bumped the ceiling
            ch.jumping = false;
        }
    }
    if dt > 0.0 && !climbing {
        // Blocked by a wall: keep only the motion that actually happened (sliding along it).
        let actual = Vec2::new(moved.x, moved.z) / dt;
        if actual.length() + 0.01 < Vec2::new(ch.vel.x, ch.vel.z).length() {
            ch.vel.x = actual.x;
            ch.vel.z = actual.y;
        }
    }

    // --- facing
    if !climbing {
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
    if (input.just(buttons::USE) || input.just(buttons::PRIMARY)) && ch.bomb_cooldown <= 0.0 && !climbing {
        ch.bomb_cooldown = bp.cooldown;
        let from = new_center + Vec3::Y * 0.35;
        let dir3 = Vec3::new(ch.facing.sin(), 0.0, ch.facing.cos());
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
    ch.anim.update(
        puppet,
        &AnimInput {
            vel: ch.vel,
            accel,
            grounded: ch.grounded,
            crouch: ch.posture == Posture::Crouch,
            crawl: ch.posture == Posture::Crawl,
            climbing,
            facing: ch.facing,
            landed,
            jumped,
        },
        dt,
    );

    if let Some(e) = st.entities.map.get_mut(&id) {
        e.character = Some(ch);
    }
}
