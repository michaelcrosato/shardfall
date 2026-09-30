//! Character puppet v1: an invisible skeleton driven by procedural animation, with sphere /
//! rounded-cone parts attached. The simulation advances a small `PuppetState` every tick; the
//! view turns (interpolated) state into parts with `pose()`, so any camera and style works.

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::params::{ChoiceParam, ParamVisitor, Tunable};
use crate::shape::Look;

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
        }
    }
}

impl Tunable for PuppetDef {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.float("scale", &mut self.scale, 0.3, 3.0, "Overall size");
        v.float("head_radius", &mut self.head_radius, 0.08, 0.5, "Head size (m)");
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
fn ik(root: Vec3, target: Vec3, l1: f32, l2: f32, bend: Vec3) -> (Vec3, Vec3) {
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
/// the camera's forward vector (for camera-aware tweaks).
pub fn pose(def: &PuppetDef, st: &PuppetState, feet: Vec3, cam_fwd: Vec3) -> Vec<PuppetPart> {
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

    let crouch = st.crouch.max(st.crawl * 0.0);
    let crawl = st.crawl;
    let climb = st.climb;

    // Pelvis height: standing -> crouch -> crawl.
    let stand_pelvis = leg * 0.97 + bob;
    let pelvis_h = stand_pelvis * (1.0 - 0.38 * crouch) * (1.0 - 0.62 * crawl);
    let lean_f = st.lean_fwd + 0.25 * crouch - 0.12 * climb;
    let pelvis = Vec3::new(0.0, pelvis_h, -0.05 * crawl * leg);
    // Torso direction: upright, leaning, or horizontal when crawling.
    let torso_dir =
        Vec3::new(st.lean_side, 1.0, lean_f).normalize().lerp(Vec3::new(0.0, 0.18, 1.0).normalize(), crawl).normalize();
    let chest = pelvis + torso_dir * def.torso_length * k;
    let neck = chest + torso_dir * (def.head_radius * 0.55 * k);
    let head = neck + torso_dir.lerp(Vec3::new(0.0, 0.5, 1.0).normalize(), crawl) * def.head_radius * k * 0.95;

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
        let mut foot = Vec3::new(s * def.hip_width * k * 1.1, (ph.cos().max(0.0)) * def.step_height * k * walk, ph.sin() * amp);
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
        hand.z -= st.recoil * 0.3;
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
        let parts = pose(&PuppetDef::default(), &PuppetState::default(), Vec3::ZERO, Vec3::NEG_Y);
        assert!(parts.len() > 10);
        // Standing puppet is roughly 1.6-1.9 m tall.
        let top = parts.iter().map(|p| p.a.y.max(p.b.y) + p.ra.max(p.rb)).fold(0.0, f32::max);
        assert!(top > 1.4 && top < 2.1, "height {top}");
    }
}
