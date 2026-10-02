//! Procedural characters: an invisible skeleton animated from the character's motion (walk
//! cycle, bob, lean, jump and land squash, swing/shoot actions), drawn as spheres and tapered
//! capsules. No animation files: change proportions and colours in `Puppet`.

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::entity::Look;
use crate::util::{Color, dir_of};

crate::choice_enum! {
    /// Skeleton type.
    #[derive(Default)]
    pub enum Plan {
        /// Two legs, two arms, a head.
        #[default]
        Biped => "biped",
        /// A hopping slime with eyes.
        Blob => "blob",
        /// Four legs, a long body, a tail (dogs, wolves, boars, lizards).
        Beast => "beast",
    }
}

crate::choice_enum! {
    /// What a biped holds in its right hand.
    #[derive(Default)]
    pub enum Held {
        #[default]
        None => "none",
        Sword => "sword",
        Gun => "gun",
        Staff => "staff",
    }
}

crate::choice_enum! {
    /// Action poses, played for a moment with `World::act`.
    #[derive(Default)]
    pub enum Act {
        #[default]
        None => "none",
        /// A sweeping strike with the right arm (or a lunge for blobs and beasts).
        Swing => "swing",
        /// Arms forward (shooting, casting).
        Shoot => "shoot",
        /// Arms up (cheering, roaring).
        Cheer => "cheer",
    }
}

/// How a character looks. All sizes are multiplied by `scale` (1 = a 1.7 m person).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Puppet {
    pub plan: Plan,
    pub scale: f32,
    pub skin: Color,
    /// Shirt / main body colour.
    pub body: Color,
    /// Trousers / legs.
    pub legs: Color,
    pub feet: Color,
    pub eyes: Color,
    /// Hat, stripes, tail tip.
    pub accent: Color,
    pub hat: bool,
    pub held: Held,
    pub look: Look,
    /// Metres travelled per full walk cycle (two steps).
    pub stride: f32,
}

impl Default for Puppet {
    fn default() -> Self {
        Self {
            plan: Plan::Biped,
            scale: 1.0,
            skin: Color::hex("#f2c9a0"),
            body: Color::hex("#e8704a"),
            legs: Color::hex("#2f4a7a"),
            feet: Color::hex("#262a33"),
            eyes: Color::hex("#15161a"),
            accent: Color::hex("#ffd34d"),
            hat: false,
            held: Held::None,
            look: Look::Cel,
            stride: 1.5,
        }
    }
}

impl Puppet {
    pub fn biped(body: &str) -> Self {
        Self { body: Color::hex(body), ..Default::default() }
    }
    pub fn blob(body: &str) -> Self {
        Self { plan: Plan::Blob, body: Color::hex(body), stride: 1.2, ..Default::default() }
    }
    pub fn beast(body: &str) -> Self {
        let c = Color::hex(body);
        Self { plan: Plan::Beast, body: c, legs: c.scale(0.7), stride: 1.8, ..Default::default() }
    }
    pub fn scale(mut self, k: f32) -> Self {
        self.scale = k;
        self
    }
    pub fn held(mut self, h: Held) -> Self {
        self.held = h;
        self
    }
    pub fn hat(mut self, accent: &str) -> Self {
        self.hat = true;
        self.accent = Color::hex(accent);
        self
    }
    pub fn colors(mut self, skin: &str, legs: &str) -> Self {
        self.skin = Color::hex(skin);
        self.legs = Color::hex(legs);
        self
    }
}

/// Animation state, advanced every tick by the character controller (part of the snapshot).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Anim {
    /// Walk cycle 0..1.
    pub phase: f32,
    /// Smoothed ground speed (m/s).
    pub speed: f32,
    /// 0 = on the ground, 1 = airborne (smoothed).
    pub air: f32,
    /// Squash and stretch spring (+ stretch, - squash).
    pub squash: f32,
    squash_vel: f32,
    /// Forward lean (radians-ish) from acceleration and speed.
    pub lean: f32,
    pub vy: f32,
    pub facing: f32,
    pub act: Act,
    /// Action time left and its length (s).
    pub act_left: f32,
    pub act_len: f32,
    pub time: f32,
    last_vel: Vec3,
}

impl Anim {
    pub(crate) fn update(&mut self, vel: Vec3, grounded: bool, jumped: bool, landed: f32, facing: f32, stride: f32, dt: f32) {
        let hs = Vec2::new(vel.x, vel.z).length();
        let k = |rate: f32| 1.0 - (-rate * dt).exp();
        self.speed += (hs - self.speed) * k(12.0);
        if grounded {
            self.phase = (self.phase + hs * dt / stride.max(0.1)).rem_euclid(1.0);
        }
        self.air += (if grounded { 0.0 } else { 1.0 } - self.air) * k(14.0);
        let acc = (vel - self.last_vel) / dt.max(1e-4);
        let want = (acc.dot(dir_of(facing)) * 0.012).clamp(-0.25, 0.35) + (hs * 0.025).min(0.2);
        self.lean += (want - self.lean) * k(10.0);
        if landed > 0.0 {
            self.squash_vel -= landed * 0.12;
        }
        if jumped {
            self.squash_vel += 1.6;
        }
        let a = -220.0 * self.squash - 14.0 * self.squash_vel;
        self.squash_vel += a * dt;
        self.squash = (self.squash + self.squash_vel * dt).clamp(-0.45, 0.45);
        self.act_left = (self.act_left - dt).max(0.0);
        if self.act_left <= 0.0 {
            self.act = Act::None;
        }
        self.vy = vel.y;
        self.facing = facing;
        self.time += dt;
        self.last_vel = vel;
    }

    /// Starts an action pose lasting `seconds`.
    pub fn play(&mut self, act: Act, seconds: f32) {
        self.act = act;
        self.act_len = seconds.max(0.05);
        self.act_left = self.act_len;
    }

    /// Action weight 0..1..0 over its time (quick in, slower out).
    fn act_weight(&self) -> f32 {
        if self.act == Act::None || self.act_len <= 0.0 {
            return 0.0;
        }
        let t = 1.0 - self.act_left / self.act_len;
        if t < 0.25 { t / 0.25 } else { 1.0 - (t - 0.25) / 0.75 }
    }
}

/// One drawn piece: a tapered capsule from `a` (radius `ra`) to `b` (radius `rb`). Equal ends
/// make a sphere.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub a: Vec3,
    pub b: Vec3,
    pub ra: f32,
    pub rb: f32,
    pub color: Color,
}

/// Two-bone IK: elbow/knee and end point reaching from `a` toward `t`, bending toward `bend`.
fn ik(a: Vec3, t: Vec3, l1: f32, l2: f32, bend: Vec3) -> (Vec3, Vec3) {
    let d = t - a;
    let dir = d.normalize_or(Vec3::NEG_Y);
    let dist = d.length().clamp(0.01, (l1 + l2) * 0.999);
    let x = (l1 * l1 - l2 * l2 + dist * dist) / (2.0 * dist);
    let h = (l1 * l1 - x * x).max(0.0).sqrt();
    let side = (bend - dir * bend.dot(dir)).normalize_or(dir.any_orthonormal_vector());
    (a + dir * x + side * h, a + dir * dist)
}

/// The parts of a posed character standing at `feet`.
pub fn pose(p: &Puppet, an: &Anim, feet: Vec3) -> Vec<Part> {
    let k = p.scale;
    let rot = Quat::from_rotation_y(an.facing);
    let sy = 1.0 + an.squash;
    let sxz = 1.0 / sy.max(0.3).sqrt();
    // Character space: x right, y up, z forward (relative to the feet).
    let to_world = |v: Vec3| feet + rot * Vec3::new(-v.x * sxz, v.y * sy, v.z * sxz);
    let mut parts = Vec::with_capacity(24);
    let mut push = |a: Vec3, b: Vec3, ra: f32, rb: f32, color: Color| {
        parts.push(Part { a: to_world(a), b: to_world(b), ra: ra * sxz.max(0.8), rb: rb * sxz.max(0.8), color });
    };
    let walk = (an.speed / 5.0).min(1.0) * (1.0 - an.air);
    let cyc = an.phase * TAU;
    let aw = an.act_weight();
    match p.plan {
        Plan::Biped => biped(p, an, k, walk, cyc, aw, &mut push),
        Plan::Blob => {
            let r = 0.45 * k;
            let hop = (cyc * 2.0).sin().abs() * 0.3 * k * (an.speed / 4.0).min(1.0) + an.air * 0.1 * k;
            let lunge = if an.act == Act::Swing { aw * 0.35 * k } else { 0.0 };
            let c = Vec3::new(0.0, r * 0.9 + hop, lunge);
            let wide = 0.08 * k + (-an.squash).max(0.0) * r;
            push(c - Vec3::X * wide, c + Vec3::X * wide, r, r, p.body);
            for s in [-1.0f32, 1.0] {
                let eye = c + Vec3::new(s * 0.16 * k, 0.12 * k, r * 0.86);
                push(eye, eye, 0.085 * k, 0.085 * k, Color::WHITE);
                push(eye + Vec3::Z * 0.05 * k, eye + Vec3::Z * 0.05 * k, 0.045 * k, 0.045 * k, p.eyes);
            }
        }
        Plan::Beast => {
            let len = 0.9 * k;
            let hip = 0.52 * k + (cyc * 2.0).cos().abs() * 0.03 * k * walk;
            let lunge = if an.act == Act::Swing { aw * 0.3 * k } else { 0.0 };
            let back = Vec3::new(0.0, hip, -len * 0.45 + lunge);
            let front = Vec3::new(0.0, hip + 0.06 * k, len * 0.45 + lunge);
            push(back, front, 0.22 * k, 0.25 * k, p.body);
            let head = front + Vec3::new(0.0, 0.2 * k + aw * 0.05 * k, 0.24 * k);
            push(head, head + Vec3::new(0.0, -0.05 * k, 0.2 * k), 0.17 * k, 0.1 * k, p.body);
            for s in [-1.0f32, 1.0] {
                let ear = head + Vec3::new(s * 0.1 * k, 0.14 * k, -0.04 * k);
                push(ear, ear + Vec3::new(s * 0.03 * k, 0.1 * k, -0.04 * k), 0.05 * k, 0.02 * k, p.legs);
                let eye = head + Vec3::new(s * 0.08 * k, 0.05 * k, 0.12 * k);
                push(eye, eye, 0.035 * k, 0.035 * k, p.eyes);
            }
            // Trot: diagonal legs move together.
            for (i, (sx, sz)) in [(-1.0f32, 1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, -1.0)].into_iter().enumerate() {
                let ph = cyc + if i < 2 { 0.0 } else { PI };
                let hip_p = Vec3::new(sx * 0.15 * k, hip - 0.05 * k, sz * len * 0.38 + lunge);
                let amp = p.stride * k * 0.18 * walk;
                let foot =
                    Vec3::new(sx * 0.17 * k, ph.cos().max(0.0) * 0.12 * k * walk + an.air * 0.15 * k, hip_p.z + ph.sin() * amp);
                let (knee, foot) = ik(hip_p, foot, hip * 0.55, hip * 0.55, Vec3::Z * -sz);
                push(hip_p, knee, 0.08 * k, 0.065 * k, p.legs);
                push(knee, foot, 0.065 * k, 0.05 * k, p.legs);
            }
            let sway = (an.time * 3.0 + cyc).sin() * 0.15 * k;
            let tail = back + Vec3::new(0.0, 0.05 * k, -0.15 * k);
            push(tail, tail + Vec3::new(sway, 0.25 * k, -0.35 * k), 0.07 * k, 0.03 * k, p.accent);
        }
    }
    parts
}

fn biped(p: &Puppet, an: &Anim, k: f32, walk: f32, cyc: f32, aw: f32, push: &mut impl FnMut(Vec3, Vec3, f32, f32, Color)) {
    let (leg, arm, lr, head_r) = (0.78 * k, 0.56 * k, 0.07 * k, 0.21 * k);
    let bob = (cyc * 2.0).cos().abs() * 0.035 * k * walk;
    let pelvis_h = leg * 0.97 + bob;
    let pelvis = Vec3::new(0.0, pelvis_h, 0.0);
    let lean = an.lean + if an.act == Act::Swing { aw * 0.25 } else { 0.0 };
    let torso_dir = Vec3::new(0.0, 1.0, lean).normalize();
    let chest = pelvis + torso_dir * 0.42 * k;
    let head = chest + torso_dir * head_r * 1.5;
    push(pelvis, chest, 0.17 * k, 0.19 * k, p.body);
    push(head, head, head_r, head_r, p.skin);
    for s in [-1.0f32, 1.0] {
        let eye = head + Vec3::new(s * 0.075 * k, 0.03 * k, head_r * 0.86);
        push(eye, eye, 0.035 * k, 0.035 * k, p.eyes);
    }
    if p.hat {
        let top = head + Vec3::Y * head_r * 0.75;
        push(top - Vec3::Y * 0.05 * k, top + Vec3::new(0.0, 0.16 * k, -0.02 * k), head_r * 0.95, head_r * 0.45, p.accent);
    }
    for s in [-1.0f32, 1.0] {
        let hip = pelvis + Vec3::new(s * 0.11 * k, 0.0, 0.0);
        let ph = cyc + if s > 0.0 { 0.0 } else { PI };
        let amp = p.stride * k * 0.25 * walk;
        let mut foot = Vec3::new(s * 0.12 * k, ph.cos().max(0.0) * 0.14 * k * walk, ph.sin() * amp);
        let tuck = Vec3::new(s * 0.11 * k, pelvis_h * 0.35, if an.vy > 0.0 { 0.05 } else { -0.1 } * k);
        foot = foot.lerp(tuck, an.air);
        let (knee, foot) = ik(hip, foot, leg * 0.5, leg * 0.5, Vec3::Z);
        push(hip, knee, lr * 1.25, lr * 1.05, p.legs);
        push(knee, foot, lr * 1.05, lr * 0.9, p.legs);
        push(foot + Vec3::new(0.0, 0.02, -0.02), foot + Vec3::new(0.0, 0.02, 0.12 * k), lr * 0.95, lr * 0.85, p.feet);
    }
    for s in [-1.0f32, 1.0] {
        let shoulder = chest + Vec3::new(s * 0.22 * k, -0.05 * k, 0.0);
        let ph = cyc + if s > 0.0 { PI } else { 0.0 };
        let swing = ph.sin() * 0.7 * walk;
        let mut hand = shoulder + Vec3::new(s * 0.08 * k, -arm * 0.88, swing * arm * 0.45);
        hand = hand.lerp(shoulder + Vec3::new(s * arm * 0.55, arm * 0.25, 0.05), an.air * 0.7);
        let right = s > 0.0;
        let target = match an.act {
            Act::Swing if right => {
                // Wind up across the body, then sweep out in front.
                let t = 1.0 - an.act_left / an.act_len.max(1e-3);
                let a = -1.2 + t * 2.6;
                Some(shoulder + Vec3::new(a.sin() * arm * 0.7, arm * 0.1, a.cos() * arm * 0.75))
            }
            Act::Shoot => Some(shoulder + Vec3::new(-s * 0.08 * k, 0.0, arm * 0.95)),
            Act::Cheer => Some(shoulder + Vec3::new(s * 0.2 * k, arm * 0.9, 0.05 * k)),
            _ => None,
        };
        if let Some(t) = target {
            hand = hand.lerp(t, aw);
        }
        let (elbow, hand) = ik(shoulder, hand, arm * 0.5, arm * 0.5, Vec3::new(s * 0.3, 0.0, -1.0));
        push(shoulder, elbow, lr * 1.05, lr * 0.95, p.body);
        push(elbow, hand, lr * 0.95, lr * 0.85, p.skin);
        push(hand, hand, lr * 1.1, lr * 1.1, p.skin);
        if right && p.held != Held::None {
            let fore = (hand - elbow).normalize_or(Vec3::Z);
            match p.held {
                Held::Sword => {
                    let dir = (fore + Vec3::Z * 0.6).normalize();
                    push(hand, hand + dir * 0.12 * k, 0.05 * k, 0.05 * k, p.accent);
                    push(hand + dir * 0.12 * k, hand + dir * 0.85 * k, 0.045 * k, 0.015 * k, Color::hex("#d8dde6"));
                }
                Held::Gun => {
                    let dir = Vec3::new(fore.x * 0.3, 0.0, 1.0).normalize();
                    push(hand, hand + dir * 0.35 * k, 0.07 * k, 0.06 * k, Color::hex("#3a3f4b"));
                }
                Held::Staff => {
                    push(hand - Vec3::Y * 0.7 * k, hand + Vec3::Y * 0.8 * k, 0.035 * k, 0.03 * k, Color::hex("#8a5a32"));
                    let gem = hand + Vec3::Y * 0.85 * k;
                    push(gem, gem, 0.09 * k, 0.09 * k, p.accent);
                }
                Held::None => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_plan_poses_near_its_feet() {
        for plan in [Plan::Biped, Plan::Blob, Plan::Beast] {
            let p = Puppet { plan, held: Held::Sword, hat: true, ..Default::default() };
            let mut an = Anim::default();
            an.update(Vec3::new(4.0, 0.0, 0.0), true, false, 0.0, 1.0, p.stride, 1.0 / 60.0);
            an.play(Act::Swing, 0.3);
            let parts = pose(&p, &an, Vec3::new(5.0, 1.0, 5.0));
            assert!(parts.len() >= 3, "{plan:?}");
            for part in parts {
                assert!(part.a.is_finite() && part.b.is_finite());
                assert!(part.a.distance(Vec3::new(5.0, 1.0, 5.0)) < 2.5, "{plan:?} part too far");
            }
        }
    }
}
