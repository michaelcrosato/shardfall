//! Character puppet v1: an invisible skeleton driven by procedural animation, with sphere /
//! rounded-cone parts attached. The simulation advances a small `PuppetState` every tick; the
//! view turns (interpolated) state into parts with `pose()`, so any camera and style works.

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::choice_enum;
use crate::color::Color;
use crate::params::{ChoiceParam, ParamVisitor, Tunable};
use crate::rig::RigView;
use crate::shape::Look;

choice_enum! {
    /// Body plan: the skeleton the puppet is built on.
    #[derive(Default)]
    pub enum BodyPlan {
        #[default]
        Biped => "biped",
        Spider => "spider",
        Lizard => "lizard",
        Beetle => "beetle",
        Blob => "blob",
    }
}

/// Proportions, colours and animation settings. Everything is a slider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PuppetDef {
    pub scale: f32,
    pub head_radius: f32,
    pub torso_length: f32,
    pub torso_radius: f32,
    pub hip_width: f32,
    pub shoulder_width: f32,
    pub leg_length: f32,
    pub arm_length: f32,
    pub limb_radius: f32,
    pub skin: String,
    pub shirt: String,
    pub pants: String,
    pub shoes: String,
    pub eyes: String,
    pub look: Look,
    /// Metres travelled per full walk cycle (two steps).
    pub stride: f32,
    pub step_height: f32,
    pub arm_swing: f32,
    pub bob: f32,
    pub lean: f32,
    pub squash: f32,
    /// Stepped animation: 0 = smooth, 12 = "on twos" at 24 fps, etc.
    pub anim_fps: f32,
    /// Tilt the eyes toward the camera so they stay visible from above (0..1).
    pub eyes_to_camera: f32,
    /// Lean the whole body toward the camera (0..1): a 2D "cheat" for high camera angles.
    pub face_camera: f32,
    /// Skeleton: biped, or a creature with planted feet (spider, lizard, beetle) or a blob.
    pub body: BodyPlan,
    /// Leg pairs of creatures (0 = the body plan's usual number).
    pub legs: i32,
    /// Creature body length (m).
    pub body_length: f32,
    /// Tail length (m, 0 = none); swings with secondary motion.
    pub tail_length: f32,
    /// Antenna length (m, 0 = none).
    pub antenna_length: f32,
    /// Seconds a creature foot takes for one step.
    pub step_time: f32,
    /// Floppiness of tails, antennae and abdomens (0 = stiff, 2 = very floppy).
    pub wobble: f32,
    /// Second colour (shells, stripes, tail tips).
    pub accent: String,
}

impl Default for PuppetDef {
    fn default() -> Self {
        Self {
            scale: 1.0,
            head_radius: 0.21,
            torso_length: 0.42,
            torso_radius: 0.19,
            hip_width: 0.11,
            shoulder_width: 0.22,
            leg_length: 0.78,
            arm_length: 0.56,
            limb_radius: 0.07,
            skin: "#f2c9a0".into(),
            shirt: "#e8704a".into(),
            pants: "#2f4a7a".into(),
            shoes: "#262a33".into(),
            eyes: "#15161a".into(),
            look: Look::Cel,
            stride: 1.5,
            step_height: 0.14,
            arm_swing: 0.7,
            bob: 0.035,
            lean: 0.9,
            squash: 1.0,
            anim_fps: 0.0,
            eyes_to_camera: 0.6,
            face_camera: 0.0,
            body: BodyPlan::Biped,
            legs: 0,
            body_length: 0.7,
            tail_length: 0.0,
            antenna_length: 0.0,
            step_time: 0.15,
            wobble: 1.0,
            accent: "#3a3f4b".into(),
        }
    }
}

impl PuppetDef {
    /// Sensible proportions and colours for a body plan.
    pub fn preset(plan: BodyPlan) -> Self {
        let d = Self::default();
        match plan {
            BodyPlan::Biped => d,
            BodyPlan::Spider => Self {
                body: plan,
                legs: 4,
                leg_length: 0.8,
                torso_radius: 0.2,
                head_radius: 0.12,
                limb_radius: 0.035,
                skin: "#3b3540".into(),
                shirt: "#4a4252".into(),
                accent: "#c0563b".into(),
                eyes: "#f2e6c9".into(),
                step_time: 0.12,
                ..d
            },
            BodyPlan::Lizard => Self {
                body: plan,
                legs: 2,
                leg_length: 0.42,
                torso_radius: 0.15,
                head_radius: 0.14,
                limb_radius: 0.05,
                body_length: 0.9,
                tail_length: 1.0,
                skin: "#6fae5a".into(),
                shirt: "#5d9a4c".into(),
                accent: "#e8c547".into(),
                step_time: 0.14,
                ..d
            },
            BodyPlan::Beetle => Self {
                body: plan,
                legs: 3,
                leg_length: 0.5,
                torso_radius: 0.24,
                head_radius: 0.13,
                limb_radius: 0.035,
                body_length: 0.75,
                antenna_length: 0.45,
                skin: "#26303b".into(),
                shirt: "#2e6f8e".into(),
                accent: "#9fd3e6".into(),
                step_time: 0.1,
                ..d
            },
            BodyPlan::Blob => Self {
                body: plan,
                torso_radius: 0.42,
                head_radius: 0.0,
                skin: "#7fd6a4".into(),
                shirt: "#7fd6a4".into(),
                squash: 1.6,
                stride: 1.4,
                ..d
            },
        }
    }

    /// Leg pairs actually used.
    pub fn leg_pairs(&self) -> usize {
        let n = match self.body {
            BodyPlan::Biped | BodyPlan::Blob => return 0,
            BodyPlan::Spider => 4,
            BodyPlan::Lizard => 2,
            BodyPlan::Beetle => 3,
        };
        if self.legs > 0 { self.legs.clamp(1, 6) as usize } else { n }
    }
}

impl Tunable for PuppetDef {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.float("scale", &mut self.scale, 0.3, 3.0, "Overall size");
        v.float("head_radius", &mut self.head_radius, 0.0, 0.5, "Head size (m)");
        v.float("torso_length", &mut self.torso_length, 0.15, 0.9, "Torso length (m)");
        v.float("torso_radius", &mut self.torso_radius, 0.08, 0.45, "Torso thickness (m)");
        v.float("hip_width", &mut self.hip_width, 0.04, 0.3, "Half distance between hips (m)");
        v.float("shoulder_width", &mut self.shoulder_width, 0.08, 0.45, "Half distance between shoulders (m)");
        v.float("leg_length", &mut self.leg_length, 0.3, 1.4, "Leg length (m)");
        v.float("arm_length", &mut self.arm_length, 0.2, 1.2, "Arm length (m)");
        v.float("limb_radius", &mut self.limb_radius, 0.02, 0.2, "Limb thickness (m)");
        self.look.visit_choice(v, "look", "Surface style of the puppet");
        v.float("stride", &mut self.stride, 0.4, 3.0, "Metres per full walk cycle");
        v.float("step_height", &mut self.step_height, 0.0, 0.5, "Foot lift (m)");
        v.float("arm_swing", &mut self.arm_swing, 0.0, 1.5, "Arm swing amount");
        v.float("bob", &mut self.bob, 0.0, 0.2, "Body bob while walking (m)");
        v.float("lean", &mut self.lean, 0.0, 3.0, "Lean into acceleration");
        v.float("squash", &mut self.squash, 0.0, 3.0, "Squash & stretch amount");
        v.float("anim_fps", &mut self.anim_fps, 0.0, 30.0, "Stepped animation rate (0 = smooth)");
        v.float("eyes_to_camera", &mut self.eyes_to_camera, 0.0, 1.0, "Keep eyes visible from above");
        v.float("face_camera", &mut self.face_camera, 0.0, 1.0, "Lean the body toward the camera");
        self.body.visit_choice(v, "body", "Body plan: biped, spider, lizard, beetle or blob");
        v.int("legs", &mut self.legs, 0, 6, "Creature leg pairs (0 = usual)");
        v.float("body_length", &mut self.body_length, 0.2, 2.0, "Creature body length (m)");
        v.float("tail_length", &mut self.tail_length, 0.0, 2.5, "Tail length (m, 0 = none)");
        v.float("antenna_length", &mut self.antenna_length, 0.0, 1.0, "Antenna length (m, 0 = none)");
        v.float("step_time", &mut self.step_time, 0.04, 0.5, "Creature step duration (s)");
        v.float("wobble", &mut self.wobble, 0.0, 2.0, "Floppiness of tails and antennae");
    }
}

/// Animation state advanced by the simulation each tick. All fields interpolate linearly
/// (except `phase`/`climb_phase`, which wrap).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PuppetState {
    /// Walk cycle 0..1.
    pub phase: f32,
    /// Smoothed horizontal speed (m/s).
    pub speed: f32,
    /// Lean from acceleration: x = sideways (right +), y = forward (+).
    pub lean_side: f32,
    pub lean_fwd: f32,
    /// Squash & stretch spring (0 = rest, >0 stretch, <0 squash).
    pub squash: f32,
    pub squash_vel: f32,
    pub crouch: f32,
    pub crawl: f32,
    pub climb: f32,
    pub climb_phase: f32,
    /// 0 = grounded, 1 = airborne.
    pub air: f32,
    pub vy: f32,
    /// Smoothed facing (radians, 0 = +Z).
    pub facing: f32,
    pub time: f32,
    /// Recoil kick (decays).
    pub recoil: f32,
    #[serde(default)]
    pub swim: f32,
    /// Dodge roll blend and tumble angle (radians).
    #[serde(default)]
    pub roll: f32,
    #[serde(default)]
    pub roll_angle: f32,
    /// Hit recoil: flinch lean (sideways, forward; radians-ish) and its spring velocity.
    #[serde(default)]
    pub hit_side: f32,
    #[serde(default)]
    pub hit_fwd: f32,
    #[serde(default)]
    pub hit_vs: f32,
    #[serde(default)]
    pub hit_vf: f32,
    /// Foot IK: ground height under each foot relative to the feet (left, right).
    #[serde(default)]
    pub foot_l: f32,
    #[serde(default)]
    pub foot_r: f32,
}

/// What the character is doing this tick (input to the animator).
#[derive(Clone, Copy, Debug, Default)]
pub struct AnimInput {
    pub vel: Vec3,
    pub accel: Vec3,
    pub grounded: bool,
    pub crouch: bool,
    pub crawl: bool,
    pub climbing: bool,
    pub facing: f32,
    /// Landing impact speed this tick (m/s), 0 if none.
    pub landed: f32,
    pub jumped: bool,
    pub swimming: bool,
    pub rolling: bool,
}

fn approach(cur: f32, target: f32, rate: f32, dt: f32) -> f32 {
    cur + (target - cur) * (1.0 - (-rate * dt).exp())
}

fn wrap_angle(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

impl PuppetState {
    pub fn update(&mut self, def: &PuppetDef, i: &AnimInput, dt: f32) {
        self.time += dt;
        let hspeed = Vec3::new(i.vel.x, 0.0, i.vel.z).length();
        self.speed = approach(self.speed, hspeed, 14.0, dt);
        let stride = (def.stride * def.scale).max(0.1);
        if i.grounded && !i.climbing {
            self.phase = (self.phase + hspeed * dt / stride).rem_euclid(1.0);
        }
        if i.climbing {
            self.climb_phase = (self.climb_phase + i.vel.y.abs() * dt / (0.9 * def.scale)).rem_euclid(1.0);
        }
        // Lean into acceleration, in the character's local frame.
        let (s, c) = self.facing.sin_cos();
        let fwd = Vec3::new(s, 0.0, c);
        let right = Vec3::new(c, 0.0, -s);
        let a = i.accel;
        self.lean_fwd = approach(self.lean_fwd, (a.dot(fwd) * 0.012 * def.lean).clamp(-0.35, 0.35), 10.0, dt);
        self.lean_side = approach(self.lean_side, (a.dot(right) * 0.012 * def.lean).clamp(-0.35, 0.35), 10.0, dt);
        // Squash & stretch spring.
        if i.landed > 0.0 {
            self.squash_vel -= (i.landed * 0.9).min(12.0) * def.squash;
        }
        if i.jumped {
            self.squash_vel += 6.0 * def.squash;
        }
        let target = if i.grounded { 0.0 } else { (i.vel.y * 0.02 * def.squash).clamp(-0.12, 0.2) };
        let k = 260.0;
        let damp = 18.0;
        self.squash_vel += ((target - self.squash) * k - self.squash_vel * damp) * dt;
        self.squash = (self.squash + self.squash_vel * dt).clamp(-0.45, 0.45);
        self.crouch = approach(self.crouch, if i.crouch { 1.0 } else { 0.0 }, 16.0, dt);
        self.crawl = approach(self.crawl, if i.crawl { 1.0 } else { 0.0 }, 12.0, dt);
        self.climb = approach(self.climb, if i.climbing { 1.0 } else { 0.0 }, 14.0, dt);
        self.air = approach(self.air, if i.grounded || i.climbing { 0.0 } else { 1.0 }, 18.0, dt);
        self.vy = i.vel.y;
        let df = wrap_angle(i.facing - self.facing);
        self.facing = wrap_angle(self.facing + df * (1.0 - (-20.0 * dt).exp()));
        self.recoil = approach(self.recoil, 0.0, 8.0, dt);
        self.swim = approach(self.swim, if i.swimming { 1.0 } else { 0.0 }, 8.0, dt);
        self.roll = approach(self.roll, if i.rolling { 1.0 } else { 0.0 }, 30.0, dt);
        if i.rolling {
            self.roll_angle += hspeed * dt / 0.55;
        } else {
            // Finish the tumble to the nearest full turn.
            let full = (self.roll_angle / std::f32::consts::TAU).round() * std::f32::consts::TAU;
            self.roll_angle = approach(self.roll_angle, full, 20.0, dt);
        }
        // Hit recoil: a damped spring pulls the flinch back upright.
        let (k, c) = (150.0, 9.0);
        self.hit_vs += (-self.hit_side * k - self.hit_vs * c) * dt;
        self.hit_vf += (-self.hit_fwd * k - self.hit_vf * c) * dt;
        self.hit_side = (self.hit_side + self.hit_vs * dt).clamp(-0.9, 0.9);
        self.hit_fwd = (self.hit_fwd + self.hit_vf * dt).clamp(-0.9, 0.9);
    }

    /// Flinch away from a hit coming along `dir` (world), `strength` ~ 1 for a normal hit.
    pub fn hit(&mut self, dir: Vec3, strength: f32) {
        let (s, c) = self.facing.sin_cos();
        let fwd = Vec3::new(s, 0.0, c);
        let right = Vec3::new(c, 0.0, -s);
        let d = Vec3::new(dir.x, 0.0, dir.z).normalize_or(-fwd);
        let kick = 7.0 * strength.clamp(0.2, 3.0);
        self.hit_vs += d.dot(right) * kick;
        self.hit_vf += d.dot(fwd) * kick;
        self.recoil = self.recoil.max(0.6 * strength.min(1.5));
    }

    /// Blends two states (for render interpolation).
    pub fn lerp(&self, o: &PuppetState, t: f32) -> PuppetState {
        let l = |a: f32, b: f32| a + (b - a) * t;
        let lw = |a: f32, b: f32| {
            let mut d = b - a;
            if d > 0.5 {
                d -= 1.0;
            } else if d < -0.5 {
                d += 1.0;
            }
            (a + d * t).rem_euclid(1.0)
        };
        PuppetState {
            phase: lw(self.phase, o.phase),
            speed: l(self.speed, o.speed),
            lean_side: l(self.lean_side, o.lean_side),
            lean_fwd: l(self.lean_fwd, o.lean_fwd),
            squash: l(self.squash, o.squash),
            squash_vel: o.squash_vel,
            crouch: l(self.crouch, o.crouch),
            crawl: l(self.crawl, o.crawl),
            climb: l(self.climb, o.climb),
            climb_phase: lw(self.climb_phase, o.climb_phase),
            air: l(self.air, o.air),
            vy: l(self.vy, o.vy),
            facing: self.facing + wrap_angle(o.facing - self.facing) * t,
            time: l(self.time, o.time),
            recoil: l(self.recoil, o.recoil),
            swim: l(self.swim, o.swim),
            roll: l(self.roll, o.roll),
            roll_angle: l(self.roll_angle, o.roll_angle),
            hit_side: l(self.hit_side, o.hit_side),
            hit_fwd: l(self.hit_fwd, o.hit_fwd),
            hit_vs: o.hit_vs,
            hit_vf: o.hit_vf,
            foot_l: l(self.foot_l, o.foot_l),
            foot_r: l(self.foot_r, o.foot_r),
        }
    }
}

/// One rendered part: a rounded cone from `a` (radius `ra`) to `b` (radius `rb`).
#[derive(Clone, Copy, Debug)]
pub struct PuppetPart {
    pub a: Vec3,
    pub b: Vec3,
    pub ra: f32,
    pub rb: f32,
    pub color: Color,
}

/// Two-bone IK: returns the joint (knee/elbow) position.
pub(crate) fn ik(root: Vec3, target: Vec3, l1: f32, l2: f32, bend: Vec3) -> (Vec3, Vec3) {
    let mut d = target - root;
    let dist = d.length().max(1e-4);
    let reach = (l1 + l2) * 0.999;
    let target = if dist > reach {
        d = d / dist * reach;
        root + d
    } else {
        target
    };
    let dist = d.length().max(1e-4);
    let dir = d / dist;
    // Law of cosines: distance along dir to the joint's projection, and its offset.
    let a = (l1 * l1 - l2 * l2 + dist * dist) / (2.0 * dist);
    let h = (l1 * l1 - a * a).max(0.0).sqrt();
    let side = (bend - dir * bend.dot(dir)).normalize_or(Vec3::Z);
    (root + dir * a + side * h, target)
}

/// Builds the puppet's parts in world space. `feet` is the ground contact point, `cam_fwd`
/// the camera's forward vector (for camera-aware tweaks), `rig` the simulated feet and chains
/// (creatures, tails, antennae).
pub fn pose(def: &PuppetDef, st: &PuppetState, rig: Option<&RigView>, feet: Vec3, cam_fwd: Vec3) -> Vec<PuppetPart> {
    let mut parts = match def.body {
        BodyPlan::Biped => biped(def, st, feet, cam_fwd),
        _ => crate::rig::creature_parts(def, st, rig, feet, cam_fwd),
    };
    if let Some(r) = rig {
        crate::rig::chain_parts(def, r, &mut parts);
    }
    camera_rules(def, st.facing, feet, cam_fwd, &mut parts);
    parts
}

/// Camera-aware cheats applied to the finished parts: lean toward the camera, and the cutout
/// look (the side view drawn flat on a card that faces the camera, mirrored by facing).
fn camera_rules(def: &PuppetDef, facing: f32, feet: Vec3, cam_fwd: Vec3, parts: &mut [PuppetPart]) {
    let flat_fwd = Vec3::new(cam_fwd.x, 0.0, cam_fwd.z);
    let cam_right = Vec3::new(-flat_fwd.z, 0.0, flat_fwd.x).normalize_or(Vec3::X);
    if def.face_camera > 0.0 {
        let elev = (-cam_fwd.y).clamp(0.0, 1.0).asin();
        let q = Quat::from_axis_angle(cam_right, elev * def.face_camera * 0.5);
        for p in parts.iter_mut() {
            p.a = feet + q * (p.a - feet);
            p.b = feet + q * (p.b - feet);
        }
    }
    if def.look == Look::Cutout {
        // Character space -> card: forward maps to screen right (or left when facing left),
        // up to the camera's up; depth shrinks to a thin layering offset.
        let inv = Quat::from_rotation_y(facing).inverse();
        let fwd_w = Quat::from_rotation_y(facing) * Vec3::Z;
        let sign = if fwd_w.dot(cam_right) >= 0.0 { 1.0 } else { -1.0 };
        let cam_up = cam_right.cross(cam_fwd).normalize_or(Vec3::Y);
        let toward = -cam_fwd;
        let card = |p: Vec3| -> Vec3 {
            let l = inv * (p - feet);
            feet + cam_right * (l.z * sign) + cam_up * l.y + toward * (-l.x * sign * 0.25)
        };
        for p in parts.iter_mut() {
            p.a = card(p.a);
            p.b = card(p.b);
        }
        // Small features (eyes, markings) inside a bigger ball sit on its front, so the flat
        // drawing keeps them visible.
        let balls: Vec<(Vec3, f32)> = parts.iter().filter(|p| p.a == p.b).map(|p| (p.a, p.ra)).collect();
        for p in parts.iter_mut().filter(|p| p.a == p.b) {
            if let Some((c, r)) = balls.iter().find(|(c, r)| *r > p.ra * 2.0 && (p.a - *c).length() < *r) {
                let flat = p.a - toward * (p.a - *c).dot(toward);
                let lift = (r * r - (flat - *c).length_squared()).max(0.0).sqrt();
                p.a = flat + toward * (lift + p.ra * 0.2);
                p.b = p.a;
            }
        }
    }
}

fn biped(def: &PuppetDef, st: &PuppetState, feet: Vec3, cam_fwd: Vec3) -> Vec<PuppetPart> {
    let mut st = *st;
    if def.anim_fps > 0.5 {
        // Stepped animation: hold poses between discrete animation frames.
        let step = 1.0 / def.anim_fps;
        let q = (st.time / step).floor() * step;
        let dt = st.time - q;
        st.phase = (st.phase - dt * st.speed / (def.stride * def.scale).max(0.1)).rem_euclid(1.0);
    }
    let k = def.scale;
    let rot = Quat::from_rotation_y(st.facing);
    let fwd = rot * Vec3::Z;
    let right = rot * Vec3::X;
    let up = Vec3::Y;
    let skin = Color::hex(&def.skin);
    let shirt = Color::hex(&def.shirt);
    let pants = Color::hex(&def.pants);
    let shoes = Color::hex(&def.shoes);
    let eyes = Color::hex(&def.eyes);

    let leg = def.leg_length * k;
    let arm = def.arm_length * k;
    let lr = def.limb_radius * k;
    let walk = (st.speed / 5.0).min(1.0) * (1.0 - st.air) * (1.0 - st.climb);
    let cyc = st.phase * std::f32::consts::TAU;
    let bob = (cyc * 2.0).cos().abs() * def.bob * k * walk;

    // Squash & stretch about the feet.
    let sq = st.squash;
    let sy = 1.0 + sq;
    let sxz = 1.0 / sy.max(0.3).sqrt();
    let local = |v: Vec3| -> Vec3 {
        // v is in character space (x right, y up, z forward) relative to feet.
        let w = right * v.x * sxz + up * v.y * sy + fwd * v.z * sxz;
        feet + w
    };

    let crouch = st.crouch.max(st.roll);
    let crawl = st.crawl;
    let climb = st.climb;
    let swim = st.swim;
    let stroke = st.time * 5.0;

    // Foot IK: each foot sits on the ground under it; the pelvis drops so the lower one reaches.
    let plant = (1.0 - st.air) * (1.0 - climb) * (1.0 - swim) * (1.0 - crawl);
    let (fl, fr) = (st.foot_l * plant, st.foot_r * plant);
    let drop = (-fl.min(fr)).clamp(0.0, 0.35 * leg);

    // Pelvis height: standing -> crouch -> crawl.
    let stand_pelvis = leg * 0.97 + bob - drop;
    let pelvis_h = stand_pelvis * (1.0 - 0.38 * crouch) * (1.0 - 0.62 * crawl) * (1.0 - 0.05 * swim);
    let lean_f = st.lean_fwd + 0.25 * crouch - 0.12 * climb + st.hit_fwd;
    let pelvis = Vec3::new(0.0, pelvis_h, -0.05 * crawl * leg);
    // Torso direction: upright, leaning, or horizontal when crawling.
    let torso_dir = Vec3::new(st.lean_side + st.hit_side, 1.0, lean_f)
        .normalize()
        .lerp(Vec3::new(0.0, 0.18, 1.0).normalize(), crawl)
        .lerp(Vec3::new(0.0, 0.45, 1.0).normalize(), swim)
        .normalize();
    let chest = pelvis + torso_dir * def.torso_length * k;
    let neck = chest + torso_dir * (def.head_radius * 0.55 * k);
    // The head snaps a little further than the torso on a hit.
    let head_dir = (torso_dir + Vec3::new(st.hit_side, 0.0, st.hit_fwd) * 0.6).normalize();
    let head = neck + head_dir.lerp(Vec3::new(0.0, 0.5, 1.0).normalize(), crawl) * def.head_radius * k * 0.95;

    let mut parts = Vec::with_capacity(20);
    let mut push = |a: Vec3, b: Vec3, ra: f32, rb: f32, color: Color| {
        parts.push(PuppetPart { a: local(a), b: local(b), ra: ra * sxz.max(0.8), rb: rb * sxz.max(0.8), color });
    };

    // Torso and head.
    push(pelvis, chest, def.torso_radius * k * 0.92, def.torso_radius * k, shirt);
    push(head, head, def.head_radius * k, def.head_radius * k, skin);

    // Legs.
    for s in [-1.0f32, 1.0] {
        let hip = pelvis + Vec3::new(s * def.hip_width * k, 0.0, 0.0);
        let ph = cyc + if s > 0.0 { 0.0 } else { std::f32::consts::PI };
        let amp = def.stride * k * 0.25 * walk;
        let ground = if s > 0.0 { fr } else { fl };
        let mut foot =
            Vec3::new(s * def.hip_width * k * 1.1, ground + (ph.cos().max(0.0)) * def.step_height * k * walk, ph.sin() * amp);
        // Air: tuck feet; crouch: feet under hips; crawl: knees on the ground behind.
        foot =
            foot.lerp(Vec3::new(s * def.hip_width * k, pelvis_h * 0.35, -0.05 + if st.vy > 0.0 { 0.1 } else { -0.05 }), st.air);
        let crawl_foot = Vec3::new(s * def.hip_width * k * 1.3, 0.05, pelvis.z - leg * 0.55 + ph.sin() * amp * 0.5);
        foot = foot.lerp(crawl_foot, crawl);
        let climb_foot = Vec3::new(
            s * def.hip_width * k * 1.2,
            0.1 + (st.climb_phase * std::f32::consts::TAU + if s > 0.0 { 0.0 } else { std::f32::consts::PI }).sin().max(0.0)
                * 0.35
                * k,
            0.12,
        );
        foot = foot.lerp(climb_foot, climb);
        let kick = (stroke * 2.0 + if s > 0.0 { 0.0 } else { std::f32::consts::PI }).sin();
        let swim_foot = Vec3::new(s * def.hip_width * k, pelvis_h - leg * 0.35 + kick * 0.12 * k, -leg * 0.85);
        foot = foot.lerp(swim_foot, swim);
        let bend = Vec3::Z.lerp(Vec3::NEG_Y, crawl * 0.8);
        let (knee, foot) = ik(hip, foot, leg * 0.5, leg * 0.5, bend);
        push(hip, knee, lr * 1.25, lr * 1.05, pants);
        push(knee, foot, lr * 1.05, lr * 0.9, pants);
        push(foot + Vec3::new(0.0, 0.02, -0.02), foot + Vec3::new(0.0, 0.02, 0.12 * k), lr * 0.95, lr * 0.85, shoes);
    }

    // Arms.
    for s in [-1.0f32, 1.0] {
        let shoulder = chest + Vec3::new(s * def.shoulder_width * k, -0.05 * k, 0.0);
        let ph = cyc + if s > 0.0 { std::f32::consts::PI } else { 0.0 };
        let swing = ph.sin() * def.arm_swing * walk;
        let mut hand = shoulder + Vec3::new(s * 0.08 * k, -arm * 0.88, swing * arm * 0.45);
        // Air: arms up and out.
        hand = hand.lerp(shoulder + Vec3::new(s * arm * 0.55, arm * 0.25, 0.05), st.air * 0.7);
        // Crouch: hands forward a bit.
        hand = hand.lerp(shoulder + Vec3::new(s * 0.1, -arm * 0.6, arm * 0.45), crouch * 0.6);
        // Crawl: hands on the ground ahead, alternating.
        let crawl_hand = Vec3::new(s * def.shoulder_width * k * 1.1, 0.04, chest.z + arm * 0.35 + ph.sin() * 0.12 * k);
        hand = hand.lerp(crawl_hand, crawl);
        // Climb: reaching up alternately.
        let cp = st.climb_phase * std::f32::consts::TAU + if s > 0.0 { std::f32::consts::PI } else { 0.0 };
        let climb_hand = shoulder + Vec3::new(s * 0.1, arm * (0.35 + 0.3 * cp.sin().max(0.0)), 0.22);
        hand = hand.lerp(climb_hand, climb);
        // Swim: alternating front-crawl strokes.
        let sp = stroke + if s > 0.0 { 0.0 } else { std::f32::consts::PI };
        let swim_hand = shoulder + Vec3::new(s * 0.18, sp.sin() * 0.25 * k, sp.cos() * arm * 0.75);
        hand = hand.lerp(swim_hand, swim);
        hand.z -= st.recoil * 0.3;
        // Arms fling out on a hit.
        let fling = (st.hit_side.abs() + st.hit_fwd.abs()).min(0.8);
        hand += Vec3::new(s * 0.5, 0.6, 0.0) * fling * arm;
        let bend = Vec3::new(0.0, 0.0, -1.0).lerp(Vec3::new(s, 0.0, 0.0), 0.3).lerp(Vec3::NEG_Y, crawl * 0.5);
        let (elbow, hand) = ik(shoulder, hand, arm * 0.5, arm * 0.5, bend);
        push(shoulder, elbow, lr * 1.05, lr * 0.95, shirt);
        push(elbow, hand, lr * 0.95, lr * 0.85, skin);
        push(hand, hand, lr * 1.1, lr * 1.1, skin);
    }

    // Eyes: on the face, pushed toward the camera so they read from high angles.
    let cam_up = (-cam_fwd).dot(up).clamp(0.0, 1.0) * def.eyes_to_camera;
    let face_dir = (Vec3::Z * (1.0 - cam_up) + Vec3::Y * cam_up * 1.2).normalize();
    let face_dir = face_dir.lerp(Vec3::new(0.0, 0.3 + cam_up * 0.5, 1.0).normalize(), crawl).normalize();
    let hr = def.head_radius * k;
    for s in [-1.0f32, 1.0] {
        let side = Vec3::new(s * 0.36, 0.08, 0.0) * hr;
        let e = head + (face_dir * hr * 0.93 + side).normalize() * hr * 0.9;
        push(e, e, hr * 0.17, hr * 0.17, eyes);
    }
    if st.roll > 0.01 || st.roll_angle.rem_euclid(std::f32::consts::TAU) > 0.01 {
        // Dodge roll: tumble forward about the side axis through the curled-up body.
        let q = Quat::from_axis_angle(right, st.roll_angle);
        let pivot = feet + up * 0.42 * k;
        for p in &mut parts {
            p.a = pivot + q * (p.a - pivot);
            p.b = pivot + q * (p.b - pivot);
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ik_reaches_and_keeps_lengths() {
        let (j, t) = ik(Vec3::ZERO, Vec3::new(0.0, -0.8, 0.2), 0.5, 0.5, Vec3::Z);
        assert!((j.length() - 0.5).abs() < 1e-3);
        assert!(((t - j).length() - 0.5).abs() < 1e-3);
    }

    #[test]
    fn pose_produces_parts() {
        let parts = pose(&PuppetDef::default(), &PuppetState::default(), None, Vec3::ZERO, Vec3::NEG_Y);
        assert!(parts.len() > 10);
        // Standing puppet is roughly 1.6-1.9 m tall.
        let top = parts.iter().map(|p| p.a.y.max(p.b.y) + p.ra.max(p.rb)).fold(0.0, f32::max);
        assert!(top > 1.4 && top < 2.1, "height {top}");
    }

    #[test]
    fn every_body_plan_poses() {
        for plan in [BodyPlan::Spider, BodyPlan::Lizard, BodyPlan::Beetle, BodyPlan::Blob] {
            let mut def = PuppetDef::preset(plan);
            let parts = pose(&def, &PuppetState::default(), None, Vec3::ZERO, Vec3::NEG_Y);
            assert!(!parts.is_empty(), "{plan:?}");
            def.look = Look::Cutout;
            let flat = pose(&def, &PuppetState::default(), None, Vec3::ZERO, Vec3::new(0.0, -0.5, -0.86).normalize());
            assert!(flat.iter().all(|p| p.a.is_finite() && p.b.is_finite()));
        }
    }

    #[test]
    fn hit_recoil_springs_back() {
        let mut st = PuppetState::default();
        st.hit(Vec3::X, 1.0);
        let def = PuppetDef::default();
        let mut peak = 0.0f32;
        for _ in 0..120 {
            st.update(&def, &AnimInput { grounded: true, ..Default::default() }, 1.0 / 60.0);
            peak = peak.max(st.hit_side.abs());
        }
        assert!(peak > 0.15, "flinched {peak}");
        assert!(st.hit_side.abs() < 0.02, "back upright: {}", st.hit_side);
    }
}
