//! Character controller: a kinematic capsule moved by rapier's character controller. Two
//! movement models (instant, momentum), jumps with coyote time, buffering and variable height,
//! stairs and slopes, riding moving platforms, pushing props, knockback, dashes, an axis lock
//! for side views. All numbers are tunable (`movement.*`).

use glam::{Vec2, Vec3};
use rapier3d::control::{CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController};
use rapier3d::prelude::*;

use crate::entity::Id;
use crate::input::{Input, buttons};
use crate::params::{Choice, ParamVisitor, Tunable};
use crate::puppet::Anim;
use crate::util::{dir_of, yaw_of};
use crate::world::{Event, World};

crate::choice_enum! {
    /// How input turns into motion.
    #[derive(Default)]
    pub enum MoveModel {
        /// Full speed at once, stops at once (top-down action, precise platforming).
        #[default]
        Instant => "instant",
        /// Accelerates, skids when reversing, keeps momentum in the air.
        Momentum => "momentum",
    }
}

crate::choice_enum! {
    /// Lock motion along a world axis: `z` for side-view games (movement only along x).
    #[derive(Default)]
    pub enum LockAxis {
        #[default]
        None => "none",
        X => "x",
        Z => "z",
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MovementParams {
    pub model: MoveModel,
    /// Run speed (m/s).
    pub speed: f32,
    /// Momentum model: acceleration, slow-down without input, braking when reversing (m/s²).
    pub accel: f32,
    pub decel: f32,
    pub skid: f32,
    /// Share of control in the air (0..1).
    pub air_control: f32,
    /// Character gravity (m/s², positive = down). Separate from prop gravity: jumps feel snappy.
    pub gravity: f32,
    pub jump_height: f32,
    pub allow_jump: bool,
    /// Upward speed kept when jump is released early (variable jump height).
    pub jump_cut: f32,
    /// Jump grace after walking off a ledge, and how early a jump press is remembered (s).
    pub coyote_time: f32,
    pub jump_buffer: f32,
    pub max_fall: f32,
    /// Walks up steps this high (m).
    pub step_height: f32,
    /// How hard characters push dynamic props (kg).
    pub push_mass: f32,
    pub lock_axis: LockAxis,
    /// Face the aim point instead of the movement direction (twin-stick shooters).
    pub face_aim: bool,
}

impl Default for MovementParams {
    fn default() -> Self {
        Self {
            model: MoveModel::Instant,
            speed: 6.0,
            accel: 45.0,
            decel: 32.0,
            skid: 90.0,
            air_control: 0.55,
            gravity: 32.0,
            jump_height: 1.35,
            allow_jump: true,
            jump_cut: 0.45,
            coyote_time: 0.1,
            jump_buffer: 0.12,
            max_fall: 26.0,
            step_height: 0.32,
            push_mass: 70.0,
            lock_axis: LockAxis::None,
            face_aim: false,
        }
    }
}

impl Tunable for MovementParams {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        self.model.visit_choice(v, "model", "Movement model");
        v.float("speed", &mut self.speed, 0.5, 30.0, "Run speed (m/s)");
        v.float("accel", &mut self.accel, 1.0, 300.0, "Momentum: acceleration (m/s²)");
        v.float("decel", &mut self.decel, 1.0, 300.0, "Momentum: slow-down with no input (m/s²)");
        v.float("skid", &mut self.skid, 1.0, 400.0, "Momentum: braking when reversing (m/s²)");
        v.float("air_control", &mut self.air_control, 0.0, 1.0, "Share of control while airborne");
        v.float("gravity", &mut self.gravity, 1.0, 120.0, "Character gravity (m/s²)");
        v.float("jump_height", &mut self.jump_height, 0.0, 8.0, "Jump apex height (m)");
        v.bool("allow_jump", &mut self.allow_jump, "Jumping allowed");
        v.float("jump_cut", &mut self.jump_cut, 0.0, 1.0, "Upward speed kept when jump is released early");
        v.float("coyote_time", &mut self.coyote_time, 0.0, 0.5, "Jump grace after leaving a ledge (s)");
        v.float("jump_buffer", &mut self.jump_buffer, 0.0, 0.5, "Early jump presses are remembered (s)");
        v.float("max_fall", &mut self.max_fall, 2.0, 100.0, "Terminal fall speed (m/s)");
        v.float("step_height", &mut self.step_height, 0.0, 1.0, "Walk up steps this high (m)");
        v.float("push_mass", &mut self.push_mass, 1.0, 1000.0, "How hard characters push props (kg)");
        self.lock_axis.visit_choice(v, "lock_axis", "Lock motion along an axis (z = side view)");
        v.bool("face_aim", &mut self.face_aim, "Face the aim point instead of the movement direction");
    }
}

/// A walking body. Lives in `Entity::character`; the entity's `pos` is the feet.
#[derive(Clone, Debug)]
pub struct Character {
    pub height: f32,
    pub radius: f32,
    pub vel: Vec3,
    pub grounded: bool,
    /// Seconds since leaving the ground.
    pub air_time: f32,
    /// Yaw in radians (0 = facing +Z / south, PI/2 = +X / east).
    pub facing: f32,
    /// What drives this character this tick. The player gets the tick's input automatically;
    /// set it for NPCs with `World::drive`. `pressed` clears after each tick.
    pub input: Input,
    /// Speed and jump multipliers (1 = the shared `movement.*` values; 0 jump = cannot jump).
    pub speed: f32,
    pub jump: f32,
    /// Seconds without control (after a knockback).
    pub stun: f32,
    /// Seconds of dash left and its velocity.
    pub dash_time: f32,
    pub dash_vel: Vec3,
    pub anim: Anim,
    pub(crate) impulse: Vec3,
    pub(crate) since_jump_press: f32,
    pub(crate) jumping: bool,
    pub(crate) lock: Option<f32>,
    pub(crate) fell: bool,
}

impl Character {
    pub fn new(height: f32, radius: f32) -> Self {
        Self {
            height,
            radius: radius.min(height * 0.5 - 0.01),
            vel: Vec3::ZERO,
            grounded: false,
            air_time: 0.0,
            facing: 0.0,
            input: Input::default(),
            speed: 1.0,
            jump: 1.0,
            stun: 0.0,
            dash_time: 0.0,
            dash_vel: Vec3::ZERO,
            anim: Anim::default(),
            impulse: Vec3::ZERO,
            since_jump_press: 10.0,
            jumping: false,
            lock: None,
            fell: false,
        }
    }
    pub(crate) fn half_height(&self) -> f32 {
        (self.height * 0.5 - self.radius).max(0.01)
    }
    /// Facing direction on the ground plane.
    pub fn forward(&self) -> Vec3 {
        dir_of(self.facing)
    }
}

/// Moves one character by one tick.
pub(crate) fn tick(w: &mut World, id: Id, dt: f32) {
    let mp = &w.config.movement;
    let Some(e) = w.entities.get_mut(&id) else { return };
    let (Some(body), Some(mut ch)) = (e.handle, e.character.take()) else { return };
    let center = e.pos + Vec3::Y * ch.height * 0.5;
    let input = ch.input;
    let (last_vel, was_grounded) = (ch.vel, ch.grounded);

    ch.stun = (ch.stun - dt).max(0.0);
    ch.dash_time = (ch.dash_time - dt).max(0.0);
    let stunned = ch.stun > 0.0;
    let dashing = ch.dash_time > 0.0;
    let mut wish = if stunned { Vec3::ZERO } else { input.move3() };
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

    // --- horizontal
    let speed = mp.speed * ch.speed;
    let mut vh = Vec3::new(ch.vel.x, 0.0, ch.vel.z);
    if dashing {
        vh = Vec3::new(ch.dash_vel.x, 0.0, ch.dash_vel.z);
    } else if stunned {
        vh *= (-3.0 * dt).exp(); // knocked back: slide to a stop
    } else {
        match mp.model {
            MoveModel::Instant => {
                let target = wish * speed;
                vh = if ch.grounded || mp.air_control >= 0.99 {
                    target
                } else {
                    vh.lerp(target, 1.0 - (-mp.air_control * 25.0 * dt).exp())
                };
            }
            MoveModel::Momentum => {
                let target = wish * speed;
                let ctrl = if ch.grounded { 1.0 } else { mp.air_control };
                let rate = if wish.length() < 0.05 {
                    mp.decel
                } else if vh.dot(wish) < 0.0 {
                    mp.skid
                } else {
                    mp.accel
                };
                vh += (target - vh).clamp_length_max(rate * ctrl * dt);
            }
        }
    }
    ch.vel.x = vh.x;
    ch.vel.z = vh.z;

    // --- vertical
    let mut jumped = false;
    let can_jump = (ch.grounded || ch.air_time < mp.coyote_time) && mp.allow_jump && ch.jump > 0.0;
    if ch.since_jump_press <= mp.jump_buffer && can_jump && !ch.jumping {
        ch.vel.y = (2.0 * mp.gravity * mp.jump_height * ch.jump).sqrt();
        ch.jumping = true;
        ch.grounded = false;
        ch.air_time = mp.coyote_time + 1.0;
        ch.since_jump_press = 10.0;
        jumped = true;
    }
    if ch.jumping && !held(buttons::JUMP) && ch.vel.y > 0.0 {
        ch.vel.y *= mp.jump_cut;
        ch.jumping = false;
    }
    if !jumped {
        ch.vel.y -= mp.gravity * dt;
    }
    if ch.vel.y <= 0.0 {
        ch.jumping = false;
    }
    ch.vel.y = ch.vel.y.max(-mp.max_fall);
    ch.vel += ch.impulse;
    ch.impulse = Vec3::ZERO;

    // --- axis lock: no motion along the axis, drift corrected
    let mut fix = Vec3::ZERO;
    if let Some(v) = ch.lock {
        match mp.lock_axis {
            LockAxis::X => {
                ch.vel.x = 0.0;
                fix.x = v - center.x;
            }
            LockAxis::Z => {
                ch.vel.z = 0.0;
                fix.z = v - center.z;
            }
            LockAxis::None => {}
        }
    }

    // --- move with rapier's kinematic character controller
    let kcc = KinematicCharacterController {
        up: Vector::Y,
        offset: CharacterLength::Absolute(0.02),
        slide: true,
        autostep: Some(CharacterAutostep {
            max_height: CharacterLength::Absolute(mp.step_height),
            min_width: CharacterLength::Absolute(0.12),
            include_dynamic_bodies: true,
        }),
        max_slope_climb_angle: 50f32.to_radians(),
        min_slope_slide_angle: 40f32.to_radians(),
        snap_to_ground: (was_grounded && ch.vel.y <= 0.0).then_some(CharacterLength::Absolute(0.25)),
        normal_nudge_factor: 1.0e-4,
    };
    let shape = SharedShape::capsule_y(ch.half_height(), ch.radius);
    let mut desired = ch.vel * dt + fix;
    if desired.length_squared() < 1e-8 {
        // A hair of downward motion keeps the controller's ground checks running; that is
        // where it carries the character along with moving platforms.
        desired.y -= 1e-3;
    }
    let filter = QueryFilter::default().exclude_rigid_body(body).exclude_sensors();
    let mut collisions: Vec<CharacterCollision> = Vec::new();
    let ph = &mut w.phys;
    let mv = {
        let qp = ph.broad.as_query_pipeline(ph.narrow.query_dispatcher(), &ph.bodies, &ph.colliders, filter);
        kcc.move_shape(dt, &qp, shape.as_ref(), &Pose::from_translation(center), desired, |c| collisions.push(c))
    };
    {
        let mut qpm = ph.broad.as_query_pipeline_mut(ph.narrow.query_dispatcher(), &mut ph.bodies, &mut ph.colliders, filter);
        kcc.solve_character_collision_impulses(dt, &mut qpm, shape.as_ref(), mp.push_mass, collisions.iter());
    }
    let moved = mv.translation;
    let new_center = center + moved;
    if let Some(b) = ph.bodies.get_mut(body) {
        b.set_next_kinematic_translation(new_center);
    }

    // --- what actually happened
    let mut landed = 0.0;
    ch.grounded = mv.grounded && !(ch.jumping && ch.vel.y > 0.0);
    if ch.grounded {
        if !was_grounded && last_vel.y < -2.0 {
            landed = -last_vel.y;
        }
        ch.vel.y = 0.0;
        ch.air_time = 0.0;
    } else {
        ch.air_time += dt;
        if ch.vel.y > 0.0 && moved.y < desired.y * 0.5 {
            ch.vel.y = 0.0; // bumped the ceiling
            ch.jumping = false;
        }
    }
    if dt > 0.0 {
        // Blocked by a wall: keep only the motion that happened (sliding along it).
        let actual = Vec2::new(moved.x - fix.x, moved.z - fix.z) / dt;
        if actual.length() + 0.01 < Vec2::new(ch.vel.x, ch.vel.z).length() {
            ch.vel.x = actual.x;
            ch.vel.z = actual.y;
        }
    }

    // --- facing
    if !stunned {
        let aim = input.aim.map(|a| a - new_center).filter(|d| Vec2::new(d.x, d.z).length() > 0.2);
        if let (true, Some(d)) = (mp.face_aim, aim) {
            ch.facing = yaw_of(d);
        } else if dashing {
            ch.facing = yaw_of(ch.dash_vel);
        } else if wish.length() > 0.1 {
            ch.facing = yaw_of(wish);
        }
    }

    let stride = w.entities.get(&id).and_then(|e| e.puppet.as_ref()).map(|p| p.stride * p.scale).unwrap_or(1.5);
    ch.anim.update(ch.vel, ch.grounded, jumped, landed, ch.facing, stride, dt);
    if jumped {
        w.events.push(Event::Jump { id });
    }
    if landed > 0.0 {
        w.events.push(Event::Land { id, speed: landed });
    }
    if let Some(e) = w.entities.get_mut(&id) {
        e.pos = new_center - Vec3::Y * ch.height * 0.5;
        e.vel = ch.vel;
        e.character = Some(ch);
    }
}
