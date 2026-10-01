//! Creature rigs and secondary motion. Creatures (spider, lizard, beetle) plant their feet on
//! the ground and step when a foot falls too far behind its rest spot, neighbours never lifting
//! together; lizards drag a follow-the-leader spine; tails, antennae and abdomens are verlet
//! chains that bend back toward a rest direction. The simulation steps the rig every tick (it
//! needs ground probes); the view turns it into parts with `creature_parts` / `chain_parts`.

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::puppet::{BodyPlan, PuppetDef, PuppetPart, PuppetState, ik};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainKind {
    Spine,
    Tail,
    AntennaL,
    AntennaR,
    Abdomen,
}

/// A verlet chain: the first point is pinned to its root, the rest swing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    pub kind: ChainKind,
    pub pts: Vec<Vec3>,
    pub prev: Vec<Vec3>,
    pub seg: f32,
}

impl Chain {
    fn new(kind: ChainKind, root: Vec3, dir: Vec3, len: f32, n: usize) -> Self {
        let n = n.max(2);
        let seg = len.max(0.01) / (n - 1) as f32;
        let pts: Vec<Vec3> = (0..n).map(|i| root + dir * seg * i as f32).collect();
        Self { kind, prev: pts.clone(), pts, seg }
    }

    /// One verlet step: the root follows `root`; every other point keeps its distance to the
    /// previous one and bends back toward `rest` (a direction) by `stiff` (0..1 per tick).
    fn step(&mut self, root: Vec3, rest: Vec3, stiff: f32, gravity: f32, floor: f32, dt: f32) {
        if self.pts.is_empty() {
            return;
        }
        self.prev[0] = self.pts[0];
        self.pts[0] = root;
        for i in 1..self.pts.len() {
            let p = self.pts[i];
            let v = (p - self.prev[i]) * 0.94;
            self.prev[i] = p;
            let anchor = self.pts[i - 1];
            let mut q = p + v - Vec3::Y * gravity * dt * dt;
            q = q.lerp(anchor + rest * self.seg, stiff);
            q = anchor + (q - anchor).normalize_or(rest) * self.seg;
            q.y = q.y.max(floor);
            self.pts[i] = q;
        }
    }
}

/// One planted foot. `t` < 0: planted at `pos`; otherwise stepping from `from` to `to`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Foot {
    pub pos: Vec3,
    pub from: Vec3,
    pub to: Vec3,
    pub t: f32,
}

/// Simulated rig state of one character (creature feet and/or swinging chains).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rig {
    pub plan: BodyPlan,
    pub feet: Vec<Foot>,
    pub chains: Vec<Chain>,
    /// Body centre (world).
    pub body: Vec3,
    /// Body pitch (nose up +) and roll (left side up +), radians.
    pub tilt: Vec2,
    /// Feet planted so far (for footstep sounds).
    pub steps: u32,
}

/// What the view needs of a rig (interpolated between ticks).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RigView {
    pub feet: Vec<Vec3>,
    pub chains: Vec<(ChainKind, Vec<Vec3>)>,
    pub body: Vec3,
    pub tilt: Vec2,
}

impl RigView {
    pub fn lerp(&self, o: &RigView, t: f32) -> RigView {
        let same = self.feet.len() == o.feet.len()
            && self.chains.len() == o.chains.len()
            && self.chains.iter().zip(&o.chains).all(|(a, b)| a.1.len() == b.1.len());
        if !same || self.body.distance_squared(o.body) > 4.0 {
            return o.clone();
        }
        RigView {
            feet: self.feet.iter().zip(&o.feet).map(|(a, b)| a.lerp(*b, t)).collect(),
            chains: self
                .chains
                .iter()
                .zip(&o.chains)
                .map(|(a, b)| (b.0, a.1.iter().zip(&b.1).map(|(p, q)| p.lerp(*q, t)).collect()))
                .collect(),
            body: self.body.lerp(o.body, t),
            tilt: self.tilt.lerp(o.tilt, t),
        }
    }
}

/// Leg geometry in the body frame (x right, y up, z forward; origin on the ground under the
/// body centre).
struct Layout {
    body_h: f32,
    /// (hip, rest foot) per leg; legs go pair by pair, left then right.
    legs: Vec<(Vec3, Vec3)>,
    l1: f32,
    l2: f32,
}

fn layout(def: &PuppetDef) -> Layout {
    let k = def.scale;
    let rb = def.torso_radius * k;
    let leg = def.leg_length * k;
    let bl = def.body_length * k;
    let n = def.leg_pairs();
    let t = |p: usize| if n > 1 { p as f32 / (n - 1) as f32 } else { 0.5 };
    let mut legs = Vec::with_capacity(n * 2);
    let (body_h, l1, l2) = match def.body {
        BodyPlan::Spider => {
            let h = rb * 0.6 + leg * 0.3;
            for p in 0..n {
                let z = rb * 0.5 * (1.0 - 2.0 * t(p));
                for s in [-1.0f32, 1.0] {
                    legs.push((
                        Vec3::new(s * rb * 0.7, h, z),
                        Vec3::new(s * (rb + leg * 0.55), 0.0, z + (0.5 - t(p)) * leg * 0.9),
                    ));
                }
            }
            (h, leg * 0.5, leg * 0.6)
        }
        BodyPlan::Beetle => {
            let h = rb * 0.5 + leg * 0.28;
            for p in 0..n {
                let z = bl * 0.3 * (1.0 - 2.0 * t(p));
                for s in [-1.0f32, 1.0] {
                    legs.push((
                        Vec3::new(s * rb * 0.75, h, z),
                        Vec3::new(s * (rb + leg * 0.5), 0.0, z * 1.4 + (0.5 - t(p)) * leg * 0.5),
                    ));
                }
            }
            (h, leg * 0.5, leg * 0.55)
        }
        BodyPlan::Lizard => {
            let h = rb + leg * 0.15;
            for p in 0..n {
                let z = bl * 0.3 * (1.0 - 2.0 * t(p));
                let reach = if t(p) < 0.5 { 0.25 } else { -0.1 };
                for s in [-1.0f32, 1.0] {
                    legs.push((Vec3::new(s * rb * 0.8, h, z), Vec3::new(s * (rb + leg * 0.6), 0.0, z + reach * leg)));
                }
            }
            (h, leg * 0.5, leg * 0.55)
        }
        BodyPlan::Blob => (def.torso_radius * k, 0.0, 0.0),
        BodyPlan::Biped => (def.leg_length * k * 0.97, 0.0, 0.0),
    };
    Layout { body_h, legs, l1, l2 }
}

/// Which chains a def has, with (root, rest direction, length, points) in the body frame.
fn chain_specs(def: &PuppetDef, crouch: f32) -> Vec<(ChainKind, Vec3, Vec3, f32, usize)> {
    let k = def.scale;
    let lay = layout(def);
    let rb = def.torso_radius * k;
    let bl = def.body_length * k;
    let mut out = Vec::new();
    let (tail_root, head_top, head_r) = match def.body {
        BodyPlan::Biped => {
            let s = 1.0 - 0.35 * crouch;
            let pelvis = lay.body_h * s;
            let head = pelvis + (def.torso_length + def.head_radius * 1.5) * k * s;
            (
                Vec3::new(0.0, pelvis, -rb * 0.9),
                Vec3::new(0.0, head + def.head_radius * k * 0.8, def.head_radius * k * 0.2),
                def.head_radius * k,
            )
        }
        BodyPlan::Spider => {
            (Vec3::new(0.0, lay.body_h, -rb * 0.7), Vec3::new(0.0, lay.body_h + rb * 0.4, rb * 1.1), def.head_radius * k)
        }
        BodyPlan::Beetle => (
            Vec3::new(0.0, lay.body_h, -bl * 0.35),
            Vec3::new(0.0, lay.body_h + rb * 0.2, bl * 0.25 + rb * 1.1),
            def.head_radius * k,
        ),
        BodyPlan::Lizard => {
            (Vec3::new(0.0, lay.body_h, -bl * 0.5), Vec3::new(0.0, lay.body_h + rb * 0.6, bl * 0.5 + rb), def.head_radius * k)
        }
        BodyPlan::Blob => (Vec3::new(0.0, rb * 0.5, -rb), Vec3::new(0.0, rb * 1.9, rb * 0.2), rb * 0.6),
    };
    if def.body == BodyPlan::Lizard {
        out.push((ChainKind::Spine, Vec3::new(0.0, lay.body_h, bl * 0.5), Vec3::NEG_Z, bl, 6));
    }
    if def.body == BodyPlan::Spider {
        out.push((ChainKind::Abdomen, tail_root, Vec3::new(0.0, 0.5, -0.85).normalize(), rb * 1.4, 2));
    }
    if def.tail_length > 0.01 {
        let segs = ((def.tail_length * k / 0.15) as usize).clamp(3, 9);
        out.push((ChainKind::Tail, tail_root, Vec3::new(0.0, 0.25, -1.0).normalize(), def.tail_length * k, segs));
    }
    if def.antenna_length > 0.01 {
        for (kind, s) in [(ChainKind::AntennaL, -1.0f32), (ChainKind::AntennaR, 1.0)] {
            let root = head_top + Vec3::new(s * head_r * 0.45, 0.0, 0.0);
            out.push((kind, root, Vec3::new(s * 0.35, 0.8, 0.5).normalize(), def.antenna_length * k, 4));
        }
    }
    out
}

/// Whether a def needs a rig at all (creatures, or bipeds with tails/antennae).
pub fn needs_rig(def: &PuppetDef) -> bool {
    def.body != BodyPlan::Biped || def.tail_length > 0.01 || def.antenna_length > 0.01
}

impl Rig {
    /// A rig standing at rest on flat ground.
    pub fn at_rest(def: &PuppetDef, feet: Vec3, facing: f32) -> Rig {
        let rot = Quat::from_rotation_y(facing);
        let lay = layout(def);
        let feet_v = lay
            .legs
            .iter()
            .map(|(_, rest)| {
                let p = feet + rot * *rest;
                Foot { pos: p, from: p, to: p, t: -1.0 }
            })
            .collect();
        let chains = chain_specs(def, 0.0)
            .into_iter()
            .map(|(kind, root, dir, len, n)| Chain::new(kind, feet + rot * root, rot * dir, len, n))
            .collect();
        Rig { plan: def.body, feet: feet_v, chains, body: feet + Vec3::Y * lay.body_h, tilt: Vec2::ZERO, steps: 0 }
    }

    pub fn view(&self) -> RigView {
        RigView {
            feet: self.feet.iter().map(|f| f.pos).collect(),
            chains: self.chains.iter().map(|c| (c.kind, c.pts.clone())).collect(),
            body: self.body,
            tilt: self.tilt,
        }
    }

    /// Advances the rig one tick. `feet` = the character's ground point, `ground(p)` = ground
    /// height near `p` (None over a drop). Returns how many feet landed this tick.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        def: &PuppetDef,
        anim: &PuppetState,
        feet: Vec3,
        vel: Vec3,
        grounded: bool,
        ground: &dyn Fn(Vec3) -> Option<f32>,
        dt: f32,
    ) -> u32 {
        let facing = anim.facing;
        let rot = Quat::from_rotation_y(facing);
        let lay = layout(def);
        let specs = chain_specs(def, anim.crouch.max(anim.crawl));
        let rebuild = self.plan != def.body
            || self.feet.len() != lay.legs.len()
            || self.chains.len() != specs.len()
            || self
                .chains
                .iter()
                .zip(&specs)
                .any(|(c, s)| c.kind != s.0 || c.pts.len() != s.4 || (c.seg * (s.4 - 1) as f32 - s.3).abs() > 1e-3)
            || self.body.distance(feet + Vec3::Y * lay.body_h) > 3.0;
        if rebuild {
            let steps = self.steps;
            *self = Rig::at_rest(def, feet, facing);
            self.steps = steps;
        }
        let hv = Vec3::new(vel.x, 0.0, vel.z);
        let speed = hv.length();
        let step_time = def.step_time.max(0.03);
        let reach = lay.l1 + lay.l2;
        let mut landed = 0;

        // Feet in flight move along an arc; planted feet stay put.
        for (i, f) in self.feet.iter_mut().enumerate() {
            if !grounded {
                // Airborne: legs tuck under the body.
                let (hip, rest) = lay.legs[i];
                let tuck = feet + rot * Vec3::new(rest.x * 0.7, hip.y * 0.35, rest.z * 0.7);
                f.pos = f.pos.lerp(tuck, 1.0 - (-14.0 * dt).exp());
                f.t = -1.0;
                continue;
            }
            if f.t >= 0.0 {
                f.t += dt / step_time;
                let s = f.t.min(1.0);
                let e = s * s * (3.0 - 2.0 * s);
                f.pos = f.from.lerp(f.to, e) + Vec3::Y * (s * PI).sin() * def.step_height * def.scale * 1.4;
                if f.t >= 1.0 {
                    f.pos = f.to;
                    f.t = -1.0;
                    landed += 1;
                }
            }
        }
        if grounded && !self.feet.is_empty() {
            // Start steps: the furthest-behind feet first; a foot never lifts while a
            // neighbour (same side next pair, or its partner) is in the air.
            let lead = hv * step_time * 1.1;
            let thresh = if speed > 0.2 { reach * 0.3 } else { 0.06 };
            let mut want: Vec<(usize, f32, Vec3)> = Vec::new();
            for (i, f) in self.feet.iter().enumerate() {
                if f.t >= 0.0 {
                    continue;
                }
                let target = feet + rot * lay.legs[i].1 + lead;
                let d = Vec2::new(f.pos.x - target.x, f.pos.z - target.z).length();
                if d > thresh {
                    want.push((i, d, target));
                }
            }
            want.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            for (i, d, target) in want {
                let lifted = |j: usize| self.feet.get(j).is_some_and(|f| f.t >= 0.0);
                let (pair, side) = (i / 2, i % 2);
                let partner = pair * 2 + (1 - side);
                let blocked = lifted(partner) || (pair > 0 && lifted((pair - 1) * 2 + side)) || lifted((pair + 1) * 2 + side);
                if blocked && d < reach * 0.9 {
                    continue;
                }
                let y = ground(target).unwrap_or(feet.y);
                let f = &mut self.feet[i];
                f.from = f.pos;
                f.to = Vec3::new(target.x, y, target.z);
                f.t = 0.0;
            }
        }

        // Body: rides at its height over the feet, tilted to match them.
        let mut body_y = feet.y + lay.body_h;
        if grounded && !self.feet.is_empty() {
            let n = self.feet.len() as f32;
            let mean = self.feet.iter().map(|f| f.pos.y - feet.y).sum::<f32>() / n;
            body_y += mean.clamp(-0.3, 0.3) * 0.6;
            let (mut front, mut back, mut left, mut right) = (Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO);
            for (i, f) in self.feet.iter().enumerate() {
                let h = f.pos.y;
                let z = lay.legs[i].1.z;
                if z >= 0.0 {
                    front += Vec2::new(h, 1.0);
                } else {
                    back += Vec2::new(h, 1.0);
                }
                if i % 2 == 0 {
                    left += Vec2::new(h, 1.0);
                } else {
                    right += Vec2::new(h, 1.0);
                }
            }
            let avg = |v: Vec2| if v.y > 0.0 { v.x / v.y } else { feet.y };
            let span = lay.legs.iter().map(|l| l.1.z).fold(0.0f32, |a, z| a.max(z.abs())).max(0.2) * 2.0;
            let width = lay.legs.iter().map(|l| l.1.x.abs()).fold(0.2f32, f32::max) * 2.0;
            let pitch = ((avg(front) - avg(back)) / span).atan().clamp(-0.6, 0.6);
            let roll = ((avg(left) - avg(right)) / width).atan().clamp(-0.5, 0.5);
            self.tilt = self.tilt.lerp(Vec2::new(pitch, roll), 1.0 - (-10.0 * dt).exp());
        } else {
            self.tilt = self.tilt.lerp(Vec2::ZERO, 1.0 - (-6.0 * dt).exp());
        }
        let target = Vec3::new(feet.x, body_y, feet.z);
        self.body = if rebuild { target } else { self.body.lerp(target, 1.0 - (-25.0 * dt).exp()) };

        // Chains.
        let wobble = def.wobble.max(0.0);
        let soft = |base: f32| (base / (0.3 + wobble * 0.7)).clamp(0.02, 1.0);
        let tilt = Quat::from_rotation_x(-self.tilt.x) * Quat::from_rotation_z(-self.tilt.y);
        let frame = rot * tilt;
        let floor = feet.y + 0.03;
        let mut spine_end = None;
        for (c, (kind, root, dir, _, _)) in self.chains.iter_mut().zip(specs) {
            let body_off = self.body - (feet + Vec3::Y * lay.body_h);
            let root_w = match (kind, spine_end) {
                (ChainKind::Tail, Some(end)) => end,
                _ => feet + body_off + frame * root,
            };
            let (stiff, gravity) = match kind {
                ChainKind::Spine => (0.35, 0.0),
                ChainKind::Tail => (soft(0.1), 6.0),
                ChainKind::AntennaL | ChainKind::AntennaR => (soft(0.25), 2.0),
                ChainKind::Abdomen => (soft(0.35), 4.0),
            };
            // Tails wag (more when floppy, faster when moving); antennae bob a little.
            let lively = 0.4 + (speed / 3.0).min(1.0);
            let sway = match kind {
                ChainKind::Tail => Quat::from_rotation_y((anim.time * 3.2).sin() * 0.28 * wobble * lively),
                ChainKind::AntennaL | ChainKind::AntennaR => Quat::from_rotation_x((anim.time * 4.1).sin() * 0.12 * wobble),
                _ => Quat::IDENTITY,
            };
            c.step(root_w, frame * (sway * dir), stiff, gravity, floor, dt);
            if kind == ChainKind::Spine {
                spine_end = c.pts.last().copied();
            }
        }
        self.steps += landed;
        landed
    }
}

/// Ground offsets under a biped's two feet (left, right), relative to its feet height.
pub fn biped_feet(def: &PuppetDef, anim: &PuppetState, feet: Vec3, ground: &dyn Fn(Vec3) -> Option<f32>) -> (f32, f32) {
    let k = def.scale;
    let rot = Quat::from_rotation_y(anim.facing);
    let walk = (anim.speed / 5.0).min(1.0);
    let cyc = anim.phase * TAU;
    let amp = def.stride * k * 0.25 * walk;
    let mut out = [0.0f32; 2];
    for (n, s) in [-1.0f32, 1.0].into_iter().enumerate() {
        let ph = cyc + if s > 0.0 { 0.0 } else { PI };
        let p = feet + rot * Vec3::new(s * def.hip_width * k * 1.1, 0.0, ph.sin() * amp);
        let off = ground(p).map(|y| y - feet.y).unwrap_or(0.0);
        out[n] = if off.abs() < 0.04 { 0.0 } else { off.clamp(-0.45, 0.45) };
    }
    (out[0], out[1])
}

/// Two eyes on a head, pushed toward the camera when seen from above.
fn eyes(parts: &mut Vec<PuppetPart>, head: Vec3, r: f32, fwd: Vec3, right: Vec3, cam_fwd: Vec3, def: &PuppetDef, size: f32) {
    let up = Vec3::Y;
    let cam_up = (-cam_fwd).dot(up).clamp(0.0, 1.0) * def.eyes_to_camera;
    let face = (fwd * (1.0 - cam_up) + up * cam_up * 1.2 + up * 0.25).normalize();
    let color = Color::hex(&def.eyes);
    for s in [-1.0f32, 1.0] {
        let e = head + (face * 0.93 + right * s * 0.38).normalize() * r * 0.92;
        parts.push(PuppetPart { a: e, b: e, ra: r * size, rb: r * size, color, glow: 0.0 });
    }
}

/// Parts of a creature (any body plan but biped).
pub fn creature_parts(def: &PuppetDef, st: &PuppetState, rig: Option<&RigView>, feet: Vec3, cam_fwd: Vec3) -> Vec<PuppetPart> {
    let rest;
    let rig = match rig {
        Some(r) if !r.feet.is_empty() || def.leg_pairs() == 0 => r,
        _ => {
            rest = Rig::at_rest(def, feet, st.facing).view();
            &rest
        }
    };
    let k = def.scale;
    let lay = layout(def);
    let yaw = Quat::from_rotation_y(st.facing);
    let rot = yaw * Quat::from_rotation_x(-rig.tilt.x) * Quat::from_rotation_z(-rig.tilt.y);
    let (fwd, right, up) = (rot * Vec3::Z, rot * Vec3::X, rot * Vec3::Y);
    let hit = (yaw * Vec3::X * st.hit_side + yaw * Vec3::Z * st.hit_fwd) * 0.3 * k;
    let walk = (st.speed / 3.0).min(1.0) * (1.0 - st.air);
    let sq = (1.0 + st.squash).max(0.4);
    let sxz = 1.0 / sq.sqrt();
    // Walking bob, and slow breathing when standing still.
    let breathe = (st.time * 2.4).sin() * 0.012 * k * (1.0 - walk);
    let body = rig.body + hit + Vec3::Y * ((st.phase * TAU * 4.0).sin() * 0.015 * k * walk + breathe);
    let skin = Color::hex(&def.skin);
    let shirt = Color::hex(&def.shirt);
    let accent = Color::hex(&def.accent);
    let rb = def.torso_radius * k;
    let bl = def.body_length * k;
    let hr = def.head_radius * k;
    let lr = def.limb_radius * k;
    let mut parts: Vec<PuppetPart> = Vec::with_capacity(32);
    let push = |parts: &mut Vec<PuppetPart>, a: Vec3, b: Vec3, ra: f32, rb: f32, color: Color| {
        parts.push(PuppetPart { a, b, ra, rb, color, glow: 0.0 });
    };
    let chain = |kind: ChainKind| rig.chains.iter().find(|c| c.0 == kind).map(|c| &c.1);

    // Legs: two-bone IK from the hip to the planted foot, knees up and out.
    let hip_of = |i: usize, spine: Option<&Vec<Vec3>>| -> Vec3 {
        let (hip, _) = lay.legs[i];
        if let Some(sp) = spine.filter(|s| s.len() >= 4) {
            // Lizards: hips sit on the spine (front pair near the shoulders).
            let n = def.leg_pairs().max(1);
            let pair = i / 2;
            let t = if n > 1 { pair as f32 / (n - 1) as f32 } else { 0.5 };
            let idx = 1.0 + t * (sp.len() as f32 - 3.0);
            let (a, b) = (sp[idx.floor() as usize], sp[(idx.floor() as usize + 1).min(sp.len() - 1)]);
            let c = a.lerp(b, idx.fract()) + hit;
            let side = if i.is_multiple_of(2) { -1.0 } else { 1.0 };
            return c + right * side * rb * 0.8;
        }
        body + rot * (hip - Vec3::Y * lay.body_h)
    };
    let spine = chain(ChainKind::Spine);
    let bend_up = match def.body {
        BodyPlan::Spider => 1.0,
        BodyPlan::Beetle => 0.8,
        _ => 0.6,
    };
    for (i, foot) in rig.feet.iter().enumerate().take(lay.legs.len()) {
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let hip = hip_of(i, spine);
        let bend = Vec3::Y * bend_up + right * side * (1.2 - bend_up * 0.5);
        let (knee, f) = ik(hip, *foot, lay.l1, lay.l2, bend);
        push(&mut parts, hip, knee, lr * 1.25, lr, skin);
        push(&mut parts, knee, f, lr, lr * 0.6, skin);
    }

    let mut anchors: Option<crate::parts::Anchors> = None;
    let mk = |head: Vec3, head_r: f32, hf: Vec3, back: Vec<Vec3>, back_r: f32, center: Vec3| crate::parts::Anchors {
        head,
        head_r,
        fwd: hf,
        up,
        right,
        back,
        back_r,
        back_out: up,
        shoulders: [center + up * back_r * 0.6 - right * back_r * 0.6, center + up * back_r * 0.6 + right * back_r * 0.6],
        center,
        k,
    };
    match def.body {
        BodyPlan::Spider => {
            let abdomen = chain(ChainKind::Abdomen).and_then(|c| c.last().copied()).unwrap_or(body - fwd * rb * 1.6) + hit;
            push(&mut parts, abdomen, abdomen, rb * 1.35 * sxz, rb * 1.35 * sxz, shirt);
            let mark = abdomen + (abdomen - body).normalize_or(-fwd) * rb * 0.3 + Vec3::Y * rb * 1.05;
            push(&mut parts, mark, mark, rb * 0.42, rb * 0.42, accent);
            push(&mut parts, body, body, rb * sxz, rb * sxz, shirt);
            let head = body + fwd * rb * 0.95 + up * rb * 0.15;
            push(&mut parts, head, head, hr, hr, skin);
            eyes(&mut parts, head, hr, fwd, right, cam_fwd, def, 0.26);
            anchors = Some(mk(head, hr, fwd, vec![body + up * rb * 0.3, abdomen + up * rb * 0.5], rb * 1.2, body));
        }
        BodyPlan::Beetle => {
            let front = body + fwd * bl * 0.28;
            let back = body - fwd * bl * 0.32;
            push(&mut parts, back, front, rb * 1.05 * sxz, rb * 0.85 * sxz, shirt);
            let stripe_a = back + up * rb * 0.95;
            let stripe_b = front + up * rb * 0.78;
            push(&mut parts, stripe_a, stripe_b, rb * 0.13, rb * 0.13, accent);
            let head = front + fwd * rb * 0.85;
            push(&mut parts, head, head, hr, hr, skin);
            eyes(&mut parts, head, hr, fwd, right, cam_fwd, def, 0.24);
            anchors = Some(mk(head, hr, fwd, vec![front + up * rb * 0.25, back + up * rb * 0.3], rb, body));
        }
        BodyPlan::Lizard => {
            let sp: Vec<Vec3> = match spine {
                Some(s) if s.len() >= 2 => s.clone(),
                _ => (0..6).map(|i| body + fwd * (bl * 0.5 - bl * i as f32 / 5.0)).collect(),
            };
            // Sideways S-wiggle while walking.
            let n = sp.len();
            let wiggled: Vec<Vec3> = sp
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let dir = if i + 1 < n { sp[i] - sp[i + 1] } else { sp[i - 1] - sp[i] };
                    let side = Vec3::new(dir.z, 0.0, -dir.x).normalize_or(right);
                    *p + hit + side * (st.phase * TAU * 2.0 - i as f32 * 1.1).sin() * 0.07 * k * walk
                })
                .collect();
            for i in 0..n - 1 {
                let t = i as f32 / (n - 1) as f32;
                let r = |t: f32| rb * (0.75 + 0.5 * (t * PI).sin()) * sxz;
                push(&mut parts, wiggled[i], wiggled[i + 1], r(t), r(t + 1.0 / (n - 1) as f32), shirt);
            }
            let d0 = (wiggled[0] - wiggled[1]).normalize_or(fwd);
            let head = wiggled[0] + d0 * hr * 0.55 + Vec3::Y * hr * 0.15;
            push(&mut parts, head, head + d0 * hr * 1.2, hr, hr * 0.55, skin);
            let r0 = Vec3::new(d0.z, 0.0, -d0.x).normalize_or(right);
            eyes(&mut parts, head, hr, d0, r0, cam_fwd, def, 0.22);
            // Back stripe dots.
            for w in &wiggled[1..n - 1] {
                let p = *w + Vec3::Y * rb * 0.85;
                push(&mut parts, p, p, rb * 0.16, rb * 0.16, accent);
            }
            let back: Vec<Vec3> = wiggled[1..n - 1].iter().map(|w| *w + Vec3::Y * rb * 0.15).collect();
            anchors = Some(mk(head, hr, d0, back, rb * 0.85, wiggled[n / 2]));
        }
        BodyPlan::Blob => {
            // Hops as it goes: stretched in the air, squashed on landing.
            let hop = (st.phase * TAU).sin().abs();
            let lift = hop * 0.28 * k * walk + st.air * 0.1 * k;
            let s = (1.0 + st.squash * 1.2 + (hop - 0.4) * 0.45 * walk * def.squash).clamp(0.45, 1.8);
            let r = rb;
            let h = 2.0 * r * s;
            let w = 2.0 * r / s.sqrt();
            let c = feet + hit + Vec3::Y * (h * 0.5 + lift);
            if h >= w {
                let half = (h - w) * 0.5;
                push(&mut parts, c - Vec3::Y * half, c + Vec3::Y * half, w * 0.5, w * 0.5, shirt);
            } else {
                let half = (w - h) * 0.5;
                push(&mut parts, c - right * half, c + right * half, h * 0.5, h * 0.5, shirt);
                push(&mut parts, c - fwd * half, c + fwd * half, h * 0.5, h * 0.5, shirt);
            }
            let face = c + Vec3::Y * h * 0.12;
            eyes(&mut parts, face, (w * 0.5).min(h * 0.5), yaw * Vec3::Z, yaw * Vec3::X, cam_fwd, def, 0.16);
            let top = c + Vec3::Y * h * 0.35;
            let mut a = mk(
                face,
                (w * 0.5).min(h * 0.5) * 0.7,
                yaw * Vec3::Z,
                vec![top + yaw * Vec3::Z * w * 0.2, top - yaw * Vec3::Z * w * 0.3],
                w * 0.35,
                c,
            );
            a.up = Vec3::Y;
            a.back_out = Vec3::Y;
            anchors = Some(a);
        }
        BodyPlan::Biped => {}
    }
    if let Some(a) = anchors.filter(|_| !def.parts.is_empty()) {
        crate::parts::attach(&def.parts, accent, &a, st.time, &mut |a, b, ra, rb, color, glow| {
            parts.push(PuppetPart { a, b, ra, rb, color, glow });
        });
    }
    parts
}

/// Tails and antennae (any body plan); spines and abdomens are drawn by the body.
pub fn chain_parts(def: &PuppetDef, rig: &RigView, parts: &mut Vec<PuppetPart>) {
    let k = def.scale;
    for (kind, pts) in &rig.chains {
        if pts.len() < 2 {
            continue;
        }
        let (r0, r1, color, tip) = match kind {
            ChainKind::Tail => {
                let r0 = match def.body {
                    BodyPlan::Lizard => def.torso_radius * k * 0.6,
                    BodyPlan::Biped => def.limb_radius * k * 1.3,
                    _ => def.limb_radius * k * 1.6,
                };
                let color = Color::hex(if def.body == BodyPlan::Biped { &def.pants } else { &def.shirt });
                (r0, r0 * 0.25, color, None)
            }
            ChainKind::AntennaL | ChainKind::AntennaR => {
                let r = def.limb_radius * k * 0.45;
                let color = Color::hex(if def.body == BodyPlan::Biped { &def.eyes } else { &def.skin });
                (r, r * 0.7, color, Some((r * 2.4, Color::hex(&def.accent))))
            }
            ChainKind::Spine | ChainKind::Abdomen => continue,
        };
        let n = pts.len() - 1;
        for i in 0..n {
            let t0 = i as f32 / n as f32;
            let t1 = (i + 1) as f32 / n as f32;
            parts.push(PuppetPart {
                a: pts[i],
                b: pts[i + 1],
                ra: r0 + (r1 - r0) * t0,
                rb: r0 + (r1 - r0) * t1,
                color,
                glow: 0.0,
            });
        }
        if let Some((r, c)) = tip {
            let p = pts[n];
            parts.push(PuppetPart { a: p, b: p, ra: r, rb: r, color: c, glow: 0.0 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(_: Vec3) -> Option<f32> {
        Some(0.0)
    }

    #[test]
    fn spider_walks_with_alternating_steps() {
        let def = PuppetDef::preset(BodyPlan::Spider);
        let mut rig = Rig::at_rest(&def, Vec3::ZERO, 0.0);
        let anim = PuppetState { speed: 2.0, ..Default::default() };
        let dt = 1.0 / 60.0;
        let mut feet = Vec3::ZERO;
        let mut max_lifted = 0;
        for _ in 0..180 {
            feet += Vec3::Z * 2.0 * dt;
            rig.update(&def, &anim, feet, Vec3::Z * 2.0, true, &flat, dt);
            max_lifted = max_lifted.max(rig.feet.iter().filter(|f| f.t >= 0.0).count());
        }
        assert!(rig.steps > 20, "stepped {} times", rig.steps);
        assert!(max_lifted <= 4, "never more than half the legs up ({max_lifted})");
        // Every foot kept up with the body.
        for f in &rig.feet {
            assert!((f.pos.z - feet.z).abs() < 1.2, "foot {f:?} body {feet}");
        }
    }

    #[test]
    fn tail_swings_behind() {
        let def = PuppetDef { tail_length: 1.0, ..PuppetDef::preset(BodyPlan::Lizard) };
        let mut rig = Rig::at_rest(&def, Vec3::ZERO, 0.0);
        let anim = PuppetState::default();
        for _ in 0..120 {
            rig.update(&def, &anim, Vec3::ZERO, Vec3::ZERO, true, &flat, 1.0 / 60.0);
        }
        let tail = rig.chains.iter().find(|c| c.kind == ChainKind::Tail).unwrap();
        let tip = *tail.pts.last().unwrap();
        assert!(tip.z < -0.9, "tail points back: {tip}");
        assert!(tip.is_finite());
    }
}
