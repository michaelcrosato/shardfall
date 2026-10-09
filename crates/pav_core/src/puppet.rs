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
    /// What a character holds (Shardfall weapons).
    #[derive(Default)]
    pub enum WeaponKind {
        #[default]
        None => "none",
        Sword => "sword",
        Axe => "axe",
        Mace => "mace",
        Dagger => "dagger",
        Spear => "spear",
        Staff => "staff",
        Greatsword => "greatsword",
        Maul => "maul",
        Wand => "wand",
        Claw => "claw",
    }
}

choice_enum! {
    /// What the off hand holds.
    #[derive(Default)]
    pub enum OffhandKind {
        #[default]
        None => "none",
        Shield => "shield",
        Focus => "focus",
    }
}

/// A held weapon's look: drawn from the hand along the arm, swinging with attacks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeaponLook {
    pub kind: WeaponKind,
    /// Blade / head colour and grip / haft colour.
    pub color: String,
    pub grip: String,
    /// Glow of the blade (enchanted and unique weapons).
    pub glow: f32,
    /// Length multiplier.
    pub size: f32,
    pub offhand: OffhandKind,
    pub offhand_color: String,
}

impl Default for WeaponLook {
    fn default() -> Self {
        Self {
            kind: WeaponKind::None,
            color: "#c9ced8".into(),
            grip: "#5a3b26".into(),
            glow: 0.0,
            size: 1.0,
            offhand: OffhandKind::None,
            offhand_color: "#8a6a3a".into(),
        }
    }
}

choice_enum! {
    /// Helmet shapes (Shardfall armour).
    #[derive(Default)]
    pub enum HelmKind {
        #[default]
        None => "none",
        Cap => "cap",
        Helm => "helm",
        Great => "great",
        Crown => "crown",
        Horned => "horned",
        Halo => "halo",
        /// A cloth hood with a drape down the back (townsfolk).
        Hood => "hood",
    }
}

/// Worn armour's look: helmet, shoulder plates, cape, gloves, belt buckle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GearLook {
    pub helm: HelmKind,
    pub helm_color: String,
    /// Shoulder plates (0 = none, 1 = full size) and their colour.
    pub pauldrons: f32,
    pub armor_color: String,
    /// Cape (0 = none) and its colour.
    pub cape: f32,
    pub cape_color: String,
    /// Glove and belt colours ("" = none).
    pub gloves: String,
    pub belt: String,
    /// Enchanted shine on the helmet.
    pub glow: f32,
}

impl Default for GearLook {
    fn default() -> Self {
        Self {
            helm: HelmKind::None,
            helm_color: "#8a8f99".into(),
            pauldrons: 0.0,
            armor_color: "#8a8f99".into(),
            cape: 0.0,
            cape_color: "#3a2f4a".into(),
            gloves: String::new(),
            belt: String::new(),
            glow: 0.0,
        }
    }
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a).max(1e-4)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

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
        /// Four legs under a raised body, a head on a neck: hounds, wolves, boars, the town dog.
        Quadruped => "quadruped",
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
    /// Held weapon and off hand.
    pub weapon: WeaponLook,
    /// Worn armour.
    pub gear: GearLook,
    /// Horns, spikes, wings... (any body plan).
    pub parts: Vec<crate::parts::Attach>,
    /// A motion clip (`SET/Clip`, see the `clips` tool) the character plays while it stands
    /// still with nothing to do, in place of the procedural idle (bipeds). Empty = none.
    pub idle_clip: String,
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
            weapon: WeaponLook::default(),
            gear: GearLook::default(),
            parts: Vec::new(),
            idle_clip: String::new(),
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
            BodyPlan::Quadruped => Self {
                body: plan,
                legs: 2,
                leg_length: 0.5,
                torso_radius: 0.17,
                head_radius: 0.13,
                limb_radius: 0.045,
                body_length: 0.75,
                tail_length: 0.45,
                skin: "#8a6a4a".into(),
                shirt: "#9a7650".into(),
                accent: "#e8d2b0".into(),
                eyes: "#2a1e14".into(),
                step_time: 0.13,
                wobble: 1.2,
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
            BodyPlan::Quadruped => 2,
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
    /// Action animation: kind (`crate::moves` index), progress 0..1, side (alternating swings, ±1)
    /// and where the strike lands within the action (0..1).
    #[serde(default)]
    pub act_kind: u8,
    #[serde(default)]
    pub act: f32,
    #[serde(default)]
    pub act_side: f32,
    #[serde(default)]
    pub act_hit: f32,
    /// Extra height (leaps, m), spin about the vertical (whirlwind, radians) and dying (0..1:
    /// topples over and sinks).
    #[serde(default)]
    pub lift: f32,
    #[serde(default)]
    pub spin: f32,
    #[serde(default)]
    pub down: f32,
    /// Where the hands were when this action began (relative to the centre of the shoulders,
    /// in arm lengths, character space) and how firmly: a chained attack winds up from there
    /// instead of snapping back to the walk first.
    #[serde(default)]
    pub chain_l: [f32; 3],
    #[serde(default)]
    pub chain_r: [f32; 3],
    #[serde(default)]
    pub chain_w: f32,
    /// Direction of travel relative to facing (radians): the legs stride along it.
    #[serde(default)]
    pub travel: f32,
    /// This character's own beat (0..1), so idle breathing and blinks don't march in step.
    #[serde(default)]
    pub seed: f32,
    /// Motion clip (`crate::clips`): id (0 = none), time (seconds, not wrapped), how much of the
    /// body it holds (fades in and out), play flags (`clips::UPPER`...) and speed (0 = 1).
    #[serde(default)]
    pub clip: u32,
    #[serde(default)]
    pub clip_t: f32,
    #[serde(default)]
    pub clip_w: f32,
    #[serde(default)]
    pub clip_flags: u8,
    #[serde(default)]
    pub clip_speed: f32,
    /// The clip before, held under the new one while it fades in.
    #[serde(default)]
    pub clip2: u32,
    #[serde(default)]
    pub clip2_t: f32,
    #[serde(default)]
    pub clip2_w: f32,
    #[serde(default)]
    pub clip2_flags: u8,
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
        // Direction of travel relative to facing: the legs stride along it (backwards, sideways).
        if hspeed > 0.4 && !i.climbing {
            let local = Quat::from_rotation_y(-self.facing) * Vec3::new(i.vel.x, 0.0, i.vel.z);
            let d = wrap_angle(local.x.atan2(local.z) - self.travel);
            self.travel = wrap_angle(self.travel + d * (1.0 - (-12.0 * dt).exp()));
        } else {
            self.travel *= (-6.0 * dt).exp();
        }
        // Clips: time runs on, the current one fades in (or out once stopped); the one before
        // stays under it until it is fully in.
        if self.clip != 0 {
            self.clip_t += dt * if self.clip_speed > 0.0 { self.clip_speed } else { 1.0 };
            if self.clip_flags & crate::clips::ONCE != 0
                && crate::clips::with(self.clip, |c| self.clip_t >= c.dur - crate::clips::FADE).unwrap_or(true)
            {
                self.clip_flags |= crate::clips::STOP;
            }
            let step = dt / crate::clips::FADE;
            if self.clip_flags & crate::clips::STOP != 0 {
                self.clip_w = (self.clip_w - step).max(0.0);
                self.clip2_w = (self.clip2_w - step).max(0.0);
                if self.clip_w <= 0.0 {
                    self.clip = 0;
                    self.clip_flags = 0;
                }
            } else {
                self.clip_w = (self.clip_w + step).min(1.0);
            }
            if self.clip_w >= 1.0 {
                self.clip2 = 0;
            }
        } else {
            self.clip_w = 0.0;
        }
        if self.clip2 != 0 {
            self.clip2_t += dt;
            if self.clip2_w <= 0.0 {
                self.clip2 = 0;
            }
        }
    }

    /// Starts (or continues) an action: move `id` at progress `t` (0..1), its hit landing at
    /// `hit`, alternate swings on `side < 0`. An action that begins while another still holds
    /// the arms remembers where the hands were, so it winds up from there.
    pub fn set_action(&mut self, id: crate::moves::MoveId, t: f32, hit: f32, side: f32) {
        let id = id.index();
        if id == 0 {
            self.act_kind = 0;
            self.act = 0.0;
            self.chain_w = 0.0;
            return;
        }
        if id != self.act_kind || t + 0.02 < self.act {
            // An action ends a gesture clip (a cheer never holds up an attack).
            if self.clip_flags & crate::clips::ONCE != 0 {
                self.stop_clip();
            }
            let table = crate::moves::table();
            let was = crate::moves::frame(&table, self.act_kind, self.act, self.act_hit, self.act_side);
            let (old_w, from) = (self.chain_w, [Vec3::from(self.chain_l), Vec3::from(self.chain_r)]);
            self.chain_w = 0.0;
            if let Some(f) = was.filter(|f| f.hand != crate::moves::Hand::Kick && f.w > 0.3) {
                // Where the old move had the hands: on its arc, or still winding up from its own
                // starting point.
                let hands = f.hands(!f.trail);
                let at = |i: usize| if old_w > 0.0 { from[i].lerp(hands[i].0, f.wind_k) } else { hands[i].0 };
                self.chain_l = at(0).into();
                self.chain_r = at(1).into();
                self.chain_w = f.w * if old_w > 0.0 { 1.0 } else { f.wind_k.max(0.5) };
            }
        }
        self.act_kind = id;
        self.act = t;
        self.act_hit = hit;
        self.act_side = side;
    }

    /// Plays motion clip `id` (`crate::clips::find`) with play `flags` at `speed` (0 = 1). The
    /// clip playing before stays underneath while the new one fades in. Asking for the clip
    /// already playing only updates its flags and speed.
    pub fn play_clip(&mut self, id: u32, flags: u8, speed: f32) {
        if id == 0 {
            self.stop_clip();
            return;
        }
        if id == self.clip && self.clip_flags & crate::clips::STOP == 0 {
            self.clip_flags = flags & !crate::clips::STOP;
            self.clip_speed = speed;
            return;
        }
        if self.clip != 0 && self.clip_w > 0.0 {
            self.clip2 = self.clip;
            self.clip2_t = self.clip_t;
            self.clip2_w = self.clip_w;
            self.clip2_flags = self.clip_flags & !crate::clips::STOP;
        }
        self.clip = id;
        self.clip_t = 0.0;
        self.clip_w = 0.0;
        self.clip_flags = flags & !crate::clips::STOP;
        self.clip_speed = speed;
    }

    /// Fades the clip out; the procedural animation takes over again.
    pub fn stop_clip(&mut self) {
        if self.clip != 0 {
            self.clip_flags |= crate::clips::STOP;
        }
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
            // A new action starts from its beginning (no sweep backwards).
            act_kind: o.act_kind,
            act: if o.act_kind == self.act_kind && o.act >= self.act { l(self.act, o.act) } else { o.act },
            act_side: o.act_side,
            act_hit: o.act_hit,
            lift: l(self.lift, o.lift),
            spin: l(self.spin, o.spin),
            down: l(self.down, o.down),
            chain_l: o.chain_l,
            chain_r: o.chain_r,
            chain_w: o.chain_w,
            travel: self.travel + wrap_angle(o.travel - self.travel) * t,
            seed: o.seed,
            clip: o.clip,
            clip_t: if o.clip == self.clip && o.clip_t >= self.clip_t { l(self.clip_t, o.clip_t) } else { o.clip_t },
            clip_w: if o.clip == self.clip { l(self.clip_w, o.clip_w) } else { o.clip_w },
            clip_flags: o.clip_flags,
            clip_speed: o.clip_speed,
            clip2: o.clip2,
            clip2_t: if o.clip2 == self.clip2 && o.clip2_t >= self.clip2_t { l(self.clip2_t, o.clip2_t) } else { o.clip2_t },
            clip2_w: if o.clip2 == self.clip2 { l(self.clip2_w, o.clip2_w) } else { o.clip2_w },
            clip2_flags: o.clip2_flags,
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
    /// Glow (emissive), for enchanted blades, orbs and eyes.
    pub glow: f32,
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
    pose_ex(def, st, rig, feet, cam_fwd).0
}

/// `pose`, plus the held weapon's span (hand, tip) in world space when there is one (for swing
/// trails).
pub fn pose_ex(
    def: &PuppetDef,
    st: &PuppetState,
    rig: Option<&RigView>,
    feet: Vec3,
    cam_fwd: Vec3,
) -> (Vec<PuppetPart>, Option<(Vec3, Vec3)>) {
    let (mut parts, span) = match def.body {
        BodyPlan::Biped => biped_ex(def, st, feet, cam_fwd),
        _ => (crate::rig::creature_parts(def, st, rig, feet, cam_fwd), None),
    };
    if let Some(r) = rig {
        crate::rig::chain_parts(def, r, crate::rig::lunge_offset(def, st), &mut parts);
    }
    // The span rides along through the whole-body motion as an extra part.
    let n = parts.len();
    if let Some((a, b)) = span {
        parts.push(PuppetPart { a, b, ra: 0.0, rb: 0.0, color: Color::WHITE, glow: 0.0 });
    }
    motion(def, st, feet, &mut parts);
    camera_rules(def, st.facing, feet, cam_fwd, &mut parts);
    let span = (parts.len() > n).then(|| {
        let p = parts.pop().unwrap();
        (p.a, p.b)
    });
    (parts, span)
}

/// Whole-body motion on top of any body plan: lunges, whirlwind spins, leap height and dying
/// (topple over backwards, then sink into the ground).
fn motion(def: &PuppetDef, st: &PuppetState, feet: Vec3, parts: &mut [PuppetPart]) {
    let k = def.scale;
    let fwd = Quat::from_rotation_y(st.facing) * Vec3::Z;
    let right = Quat::from_rotation_y(st.facing) * Vec3::X;
    let mut shift = Vec3::Y * st.lift;
    if def.body != BodyPlan::Biped && st.act_kind != 0 {
        // A creature's "lunge" springs the whole body forward (other moves lunge through the rig).
        let table = crate::moves::table();
        if table.get(st.act_kind).is_some_and(|m| m.name == "lunge") {
            if let Some(f) = crate::moves::frame(&table, st.act_kind, st.act, st.act_hit, st.act_side) {
                shift += fwd * (f.lunge / 0.25).clamp(-0.4, 1.0) * 0.45 * k;
            }
        }
    }
    let spin = (st.spin.abs() > 1e-4).then(|| Quat::from_rotation_y(st.spin));
    let down = st.down.clamp(0.0, 1.0);
    // A clip (a death fall) lays the body down itself; the procedural topple gives way to it.
    let topple = (down > 0.0).then(|| Quat::from_axis_angle(right, -1.45 * smooth(0.0, 0.45, down) * (1.0 - st.clip_w)));
    let sink = Vec3::Y * (-1.2 * k * smooth(0.55, 1.0, down) * (1.0 - st.clip_w));
    for p in parts.iter_mut() {
        for v in [&mut p.a, &mut p.b] {
            let mut x = *v - feet;
            if let Some(q) = spin {
                x = q * x;
            }
            if let Some(q) = topple {
                x = q * x;
            }
            *v = feet + x + shift + sink;
        }
    }
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

/// The biped's joints for one frame, in character space (x right, y up, z forward, the feet at
/// the origin; squash and stretch come when it is dressed). The procedural animation (walking,
/// moves, hits), motion clips and named poses each make one; `crate::clips` crossfades them; and
/// `dress` turns the result into parts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Skel {
    pub pelvis: Vec3,
    /// Centre of the shoulder line.
    pub chest: Vec3,
    pub neck: Vec3,
    pub head: Vec3,
    /// How the pelvis, chest and head are turned (character space): belts and hips, the back
    /// (capes, wings, pauldrons), the face (eyes, helmets).
    pub pelvis_rot: Quat,
    pub chest_rot: Quat,
    pub head_rot: Quat,
    /// The top of the head points this way (helmets and hats sit along it).
    pub crown: Vec3,
    /// `[left, right]`.
    pub shoulder: [Vec3; 2],
    pub elbow: [Vec3; 2],
    pub hand: [Vec3; 2],
    pub hip: [Vec3; 2],
    pub knee: [Vec3; 2],
    pub ankle: [Vec3; 2],
    /// Which way each shoe points (unit).
    pub toe: [Vec3; 2],
    /// Which way the held weapon points from the right hand (unit).
    pub weapon: Vec3,
    /// Eyes shut (blinks): 0..1.
    pub blink: f32,
}

impl Skel {
    /// Every point turned by `q` about the feet and moved by `lift`; every direction turned by `q`
    /// (whole-body turns and hops).
    pub fn transform(&mut self, q: Quat, lift: Vec3) {
        let p = |v: &mut Vec3| *v = q * *v + lift;
        for v in [&mut self.pelvis, &mut self.chest, &mut self.neck, &mut self.head] {
            p(v);
        }
        for i in 0..2 {
            for v in [
                &mut self.shoulder[i],
                &mut self.elbow[i],
                &mut self.hand[i],
                &mut self.hip[i],
                &mut self.knee[i],
                &mut self.ankle[i],
            ] {
                p(v);
            }
            self.toe[i] = q * self.toe[i];
        }
        self.pelvis_rot = q * self.pelvis_rot;
        self.chest_rot = q * self.chest_rot;
        self.head_rot = q * self.head_rot;
        self.crown = q * self.crown;
        self.weapon = q * self.weapon;
    }
}

fn biped_ex(def: &PuppetDef, st: &PuppetState, feet: Vec3, cam_fwd: Vec3) -> (Vec<PuppetPart>, Option<(Vec3, Vec3)>) {
    let mut st = *st;
    if def.anim_fps > 0.5 {
        // Stepped animation: hold poses between discrete animation frames.
        let step = 1.0 / def.anim_fps;
        let q = (st.time / step).floor() * step;
        let dt = st.time - q;
        st.phase = (st.phase - dt * st.speed / (def.stride * def.scale).max(0.1)).rem_euclid(1.0);
        st.clip_t -= dt * if st.clip_speed > 0.0 { st.clip_speed } else { 1.0 };
    }
    let table = crate::moves::table();
    let skel = crate::clips::over(def, &st, procedural(def, &st, &table));
    dress(def, &st, &skel, feet, cam_fwd)
}

/// The procedural pose: standing, walking and running (the legs stride the way the body
/// travels), crouch, crawl, climb, swim, air, hit flinches, idle breathing and the current move.
pub fn procedural(def: &PuppetDef, st: &PuppetState, table: &crate::moves::MoveTable) -> Skel {
    use std::f32::consts::{PI, TAU};
    let k = def.scale;
    let leg = def.leg_length * k;
    let arm = def.arm_length * k;
    let hr = def.head_radius * k;
    let walk = (st.speed / 5.0).min(1.0) * (1.0 - st.air) * (1.0 - st.climb);
    let cyc = st.phase * TAU;
    let bob = (cyc * 2.0).cos().abs() * def.bob * k * walk;
    let mv = crate::moves::frame(table, st.act_kind, st.act, st.act_hit, st.act_side);
    let mw = mv.map_or(0.0, |m| m.w);
    let crouch = st.crouch.max(st.roll).max(mv.map_or(0.0, |m| m.crouch));
    let crawl = st.crawl;
    let climb = st.climb;
    let swim = st.swim;
    let stroke = st.time * 5.0;

    // Idle life while standing still with nothing to do: breathing and a slow shift of weight.
    let still = (1.0 - st.speed / 0.6).clamp(0.0, 1.0)
        * (1.0 - st.air)
        * (1.0 - climb)
        * (1.0 - swim)
        * (1.0 - crawl)
        * (1.0 - mw)
        * (1.0 - st.down.min(1.0));
    let breath = (st.time * TAU / 3.4 + st.seed * TAU).sin();
    let sway = (st.time * TAU / 6.3 + st.seed * 17.0).sin();

    // Foot IK: each foot sits on the ground under it; the pelvis drops so the lower one reaches.
    let plant = (1.0 - st.air) * (1.0 - climb) * (1.0 - swim) * (1.0 - crawl);
    let (fl, fr) = (st.foot_l * plant, st.foot_r * plant);
    let drop = (-fl.min(fr)).clamp(0.0, 0.35 * leg);

    // Pelvis height: standing -> crouch -> crawl. A move lunges it forward.
    let stand_pelvis = leg * 0.97 + bob - drop;
    let pelvis_h = stand_pelvis * (1.0 - 0.38 * crouch) * (1.0 - 0.62 * crawl) * (1.0 - 0.05 * swim);
    let lean_f = st.lean_fwd + 0.25 * crouch - 0.12 * climb + st.hit_fwd + mv.map_or(0.0, |m| m.lean);
    let lunge = mv.map_or(0.0, |m| m.lunge) * k;
    let pelvis = Vec3::new(sway * 0.012 * k * still, pelvis_h, -0.05 * crawl * leg + lunge);
    // Torso direction: upright, leaning, or horizontal when crawling.
    let torso_dir = Vec3::new(st.lean_side + st.hit_side, 1.0, lean_f)
        .normalize()
        .lerp(Vec3::new(0.0, 0.18, 1.0).normalize(), crawl)
        .lerp(Vec3::new(0.0, 0.45, 1.0).normalize(), swim)
        .normalize();
    let chest = pelvis + torso_dir * def.torso_length * k * (1.0 + 0.012 * breath * still);
    let neck = chest + torso_dir * (hr * 0.55);
    // The head snaps a little further than the torso on a hit.
    let head_dir = (torso_dir + Vec3::new(st.hit_side, 0.0, st.hit_fwd) * 0.6).normalize();
    let head = neck + head_dir.lerp(Vec3::new(0.0, 0.5, 1.0).normalize(), crawl) * hr * 0.95;
    let twist = Quat::from_rotation_y(mv.map_or(0.0, |m| m.twist));
    // Blinks: a quick shut every few seconds, each character on its own beat.
    let bt = (st.time / 4.1 + st.seed * 3.7).fract();
    let blink = if st.down > 0.0 { 0.0 } else { (1.0 - (bt / 0.04 - 1.0).abs()).max(0.0) };

    let mut sk = Skel {
        pelvis,
        chest,
        neck,
        head,
        pelvis_rot: Quat::IDENTITY,
        chest_rot: twist,
        head_rot: Quat::from_rotation_arc(Vec3::Y, head_dir.lerp(Vec3::Y, crawl).normalize()),
        crown: head_dir,
        shoulder: [Vec3::ZERO; 2],
        elbow: [Vec3::ZERO; 2],
        hand: [Vec3::ZERO; 2],
        hip: [Vec3::ZERO; 2],
        knee: [Vec3::ZERO; 2],
        ankle: [Vec3::ZERO; 2],
        toe: [Vec3::Z; 2],
        weapon: Vec3::new(0.12, -0.35, 1.0).normalize(),
        blink,
    };

    // Legs: stride along the direction of travel; a kick takes the right foot to its target and
    // the lead foot steps into a lunge.
    let travel = Vec3::new(st.travel.sin(), 0.0, st.travel.cos());
    let kick_foot = mv.and_then(|m| m.foot(leg, leg * 0.97));
    let lead = if mv.is_some_and(|m| m.hand == crate::moves::Hand::Left) { 1 } else { 0 };
    for (i, s) in [(0usize, -1.0f32), (1, 1.0)] {
        let hip = pelvis + Vec3::new(s * def.hip_width * k, 0.0, 0.0);
        let ph = cyc + if s > 0.0 { 0.0 } else { PI };
        let amp = def.stride * k * 0.25 * walk;
        let ground = if s > 0.0 { fr } else { fl };
        let lift = ph.cos().max(0.0) * def.step_height * k * walk;
        let mut foot = Vec3::new(s * def.hip_width * k * 1.1, ground + lift, 0.0) + travel * (ph.sin() * amp);
        // Air: tuck feet; crouch: feet under hips; crawl: knees on the ground behind.
        foot =
            foot.lerp(Vec3::new(s * def.hip_width * k, pelvis_h * 0.35, -0.05 + if st.vy > 0.0 { 0.1 } else { -0.05 }), st.air);
        let crawl_foot = Vec3::new(s * def.hip_width * k * 1.3, 0.05, pelvis.z - leg * 0.55 + ph.sin() * amp * 0.5);
        foot = foot.lerp(crawl_foot, crawl);
        let climb_foot = Vec3::new(
            s * def.hip_width * k * 1.2,
            0.1 + (st.climb_phase * TAU + if s > 0.0 { 0.0 } else { PI }).sin().max(0.0) * 0.35 * k,
            0.12,
        );
        foot = foot.lerp(climb_foot, climb);
        let kick = (stroke * 2.0 + if s > 0.0 { 0.0 } else { PI }).sin();
        let swim_foot = Vec3::new(s * def.hip_width * k, pelvis_h - leg * 0.35 + kick * 0.12 * k, -leg * 0.85);
        foot = foot.lerp(swim_foot, swim);
        if let Some(m) = mv {
            match kick_foot {
                Some(t) if s > 0.0 => foot = foot.lerp(t, m.w * m.wind_k),
                _ if i == lead => foot.z += lunge * 1.1,
                _ => {}
            }
        }
        let bend = Vec3::Z.lerp(Vec3::NEG_Y, crawl * 0.8);
        let (knee, ankle) = ik(hip, foot, leg * 0.5, leg * 0.5, bend);
        sk.hip[i] = hip;
        sk.knee[i] = knee;
        sk.ankle[i] = ankle;
    }

    // Arms (the right hand holds the weapon). A move twists the shoulders and steers the hands
    // along its arc, winding up from wherever the hands were.
    let fists = def.weapon.kind == WeaponKind::None;
    let shoulder_at = |s: f32| {
        pelvis + twist * (chest + Vec3::new(s * def.shoulder_width * k, -0.05 * k + breath * 0.005 * k * still, 0.0) - pelvis)
    };
    let shoulders = [shoulder_at(-1.0), shoulder_at(1.0)];
    let centre = (shoulders[0] + shoulders[1]) * 0.5;
    let targets = mv.map(|m| m.hands(fists));
    let chain = [Vec3::from(st.chain_l), Vec3::from(st.chain_r)];
    for (i, s) in [(0usize, -1.0f32), (1, 1.0)] {
        let shoulder = shoulders[i];
        let ph = cyc + if s > 0.0 { PI } else { 0.0 };
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
        let cp = st.climb_phase * TAU + if s > 0.0 { PI } else { 0.0 };
        let climb_hand = shoulder + Vec3::new(s * 0.1, arm * (0.35 + 0.3 * cp.sin().max(0.0)), 0.22);
        hand = hand.lerp(climb_hand, climb);
        // Swim: alternating front-crawl strokes.
        let sp = stroke + if s > 0.0 { 0.0 } else { PI };
        let swim_hand = shoulder + Vec3::new(s * 0.18, sp.sin() * 0.25 * k, sp.cos() * arm * 0.75);
        hand = hand.lerp(swim_hand, swim);
        hand.z -= st.recoil * 0.3;
        // Arms fling out on a hit.
        let fling = (st.hit_side.abs() + st.hit_fwd.abs()).min(0.8);
        hand += Vec3::new(s * 0.5, 0.6, 0.0) * fling * arm;
        if let (Some(m), Some(t)) = (mv, targets) {
            let (rel, firm) = t[i];
            let from = hand.lerp(centre + chain[i] * arm, st.chain_w);
            hand = hand.lerp(from.lerp(centre + rel * arm, m.wind_k), m.w * firm);
        }
        let bend = Vec3::new(0.0, 0.0, -1.0).lerp(Vec3::new(s, 0.0, 0.0), 0.3).lerp(Vec3::NEG_Y, crawl * 0.5);
        let (elbow, hand) = ik(shoulder, hand, arm * 0.5, arm * 0.5, bend);
        sk.shoulder[i] = shoulder;
        sk.elbow[i] = elbow;
        sk.hand[i] = hand;
    }
    // The weapon points forward and down, or along the arc while a move swings it.
    if let Some(m) = mv {
        use crate::moves::Hand;
        if matches!(m.hand, Hand::Right | Hand::Both | Hand::Pair) {
            let rest = sk.weapon;
            sk.weapon = rest.lerp(rest.lerp(m.weapon_dir(), m.wind_k), m.w).normalize_or(rest);
        }
        // Hops lift the whole body; spins turn it.
        if m.hop != 0.0 || m.spin != 0.0 {
            sk.transform(Quat::from_rotation_y(m.spin), Vec3::Y * m.hop * k);
        }
    }
    sk
}

/// Parts for a skeleton: body, head, gear, limbs, weapon and eyes, squashed and stretched about
/// the feet, then tumbled by a dodge roll.
fn dress(def: &PuppetDef, st: &PuppetState, sk: &Skel, feet: Vec3, cam_fwd: Vec3) -> (Vec<PuppetPart>, Option<(Vec3, Vec3)>) {
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
    let lr = def.limb_radius * k;
    let hr = def.head_radius * k;
    let tr = def.torso_radius * k;
    let walk = (st.speed / 5.0).min(1.0) * (1.0 - st.air) * (1.0 - st.climb);
    let cyc = st.phase * std::f32::consts::TAU;

    // Squash & stretch about the feet.
    let sq = st.squash;
    let sy = 1.0 + sq;
    let sxz = 1.0 / sy.max(0.3).sqrt();
    let local = |v: Vec3| -> Vec3 {
        // v is in character space (x right, y up, z forward) relative to feet.
        feet + right * v.x * sxz + up * v.y * sy + fwd * v.z * sxz
    };

    let mut parts = Vec::with_capacity(24);
    // Parts pushed while `glow_now` is set shine (enchanted helmets).
    let glow_now = std::cell::Cell::new(0.0f32);
    let mut push = |a: Vec3, b: Vec3, ra: f32, rb: f32, color: Color| {
        parts.push(PuppetPart {
            a: local(a),
            b: local(b),
            ra: ra * sxz.max(0.8),
            rb: rb * sxz.max(0.8),
            color,
            glow: glow_now.get(),
        });
    };
    let (pelvis, chest, head) = (sk.pelvis, sk.chest, sk.head);
    let (cr, pr, hq) = (sk.chest_rot, sk.pelvis_rot, sk.head_rot);
    let torso_dir = (chest - pelvis).normalize_or(Vec3::Y);

    // Torso and head.
    push(pelvis, chest, tr * 0.92, tr, shirt);
    push(head, head, hr, hr, skin);
    let gear = &def.gear;
    glow_now.set(gear.glow);
    if gear.helm != HelmKind::None {
        let hc = Color::try_hex(&gear.helm_color).unwrap_or(Color::hex("#8a8f99"));
        let gold = Color::hex("#e8c45a");
        let hu = sk.crown;
        let (f, x) = (hq * Vec3::Z, hq * Vec3::X);
        match gear.helm {
            HelmKind::Cap => push(head + hu * hr * 0.3, head + hu * hr * 0.3, hr * 0.9, hr * 0.9, hc),
            HelmKind::Hood => {
                let c = head + hu * hr * 0.2 - f * hr * 0.32;
                push(c, c, hr * 1.12, hr * 1.12, hc);
                push(head - f * hr * 0.75 - hu * hr * 0.1, head - f * hr * 0.95 - hu * hr * 1.5, hr * 0.8, hr * 0.5, hc);
            }
            HelmKind::Crown => {
                push(head + hu * hr * 0.3, head + hu * hr * 0.3, hr * 0.92, hr * 0.92, hc);
                for i in 0..5 {
                    let a = i as f32 / 5.0 * std::f32::consts::TAU;
                    let b = head + hu * hr * 0.8 + (x * a.cos() + f * a.sin()) * hr * 0.62;
                    push(b, b + hu * hr * 0.42, hr * 0.1, hr * 0.05, gold);
                }
            }
            _ => {
                let full = matches!(gear.helm, HelmKind::Great);
                push(
                    head + hu * hr * 0.12,
                    head + hu * hr * 0.12,
                    hr * if full { 1.12 } else { 1.05 },
                    hr * if full { 1.12 } else { 1.05 },
                    hc,
                );
                if !full {
                    // Nose guard.
                    push(head + f * hr * 1.0 + hu * hr * 0.25, head + f * hr * 1.02 - hu * hr * 0.3, hr * 0.11, hr * 0.09, hc);
                } else {
                    // Crest.
                    push(
                        head + hu * hr * 1.05 + f * hr * 0.5,
                        head + hu * hr * 0.95 - f * hr * 0.9,
                        hr * 0.14,
                        hr * 0.08,
                        Color::hex("#9a2a2a"),
                    );
                }
                if gear.helm == HelmKind::Horned {
                    for sd in [-1.0f32, 1.0] {
                        let b = head + hu * hr * 0.55 + x * sd * hr * 0.8;
                        let m = b + x * sd * hr * 0.55 + hu * hr * 0.25;
                        let t = m + hu * hr * 0.65 - f * hr * 0.15;
                        push(b, m, hr * 0.2, hr * 0.15, Color::hex("#e8e0c8"));
                        push(m, t, hr * 0.15, hr * 0.05, Color::hex("#e8e0c8"));
                    }
                }
                if gear.helm == HelmKind::Halo {
                    let c = head + hu * hr * 1.7;
                    for i in 0..12 {
                        let a = i as f32 / 12.0 * std::f32::consts::TAU;
                        let a2 = (i + 1) as f32 / 12.0 * std::f32::consts::TAU;
                        let p0 = c + (x * a.cos() + f * a.sin()) * hr * 0.85;
                        let p1 = c + (x * a2.cos() + f * a2.sin()) * hr * 0.85;
                        push(p0, p1, hr * 0.07, hr * 0.07, Color::hex("#ffe9a0"));
                    }
                }
            }
        }
    }
    glow_now.set(0.0);
    if !def.parts.is_empty() {
        let anchors = crate::parts::Anchors {
            head,
            head_r: hr,
            fwd: hq * Vec3::Z,
            up: sk.crown,
            right: hq * Vec3::X,
            back: vec![chest - cr * Vec3::Z * tr * 0.6, pelvis - pr * Vec3::Z * tr * 0.6],
            back_r: tr,
            back_out: cr * (-Vec3::Z + Vec3::Y * 0.35).normalize(),
            shoulders: [
                chest + cr * Vec3::new(-def.shoulder_width * k * 0.7, 0.0, -tr * 0.5),
                chest + cr * Vec3::new(def.shoulder_width * k * 0.7, 0.0, -tr * 0.5),
            ],
            center: pelvis.lerp(chest, 0.5),
            k,
        };
        let accent = Color::try_hex(&def.accent).unwrap_or(shirt);
        let mut add = |a: Vec3, b: Vec3, ra: f32, rb: f32, c: Color, g: f32| {
            glow_now.set(g);
            push(a, b, ra, rb, c);
        };
        crate::parts::attach(&def.parts, accent, &anchors, st.time, &mut add);
        glow_now.set(0.0);
    }
    if !gear.belt.is_empty() {
        let b = Color::try_hex(&gear.belt).unwrap_or(Color::hex("#5a4030"));
        let at = pelvis + torso_dir * def.torso_length * k * 0.12 + pr * Vec3::Z * tr * 0.92;
        push(at, at, lr * 0.75, lr * 0.75, b);
    }
    if gear.cape > 0.0 {
        // The cape hangs from the back of the shoulders and streams out behind as the body moves.
        let cc = Color::try_hex(&gear.cape_color).unwrap_or(Color::hex("#3a2f4a"));
        let top = chest - cr * Vec3::Z * tr * 1.05 - Vec3::Y * 0.05 * k;
        let flow = (st.speed / 6.0).min(1.0) * 0.55 + (cyc * 2.0).sin() * 0.05 * walk + st.lean_fwd.max(0.0) * 0.4;
        let back = (cr * Vec3::NEG_Z * Vec3::new(1.0, 0.0, 1.0)).normalize_or(Vec3::NEG_Z);
        let hang = def.torso_length * k + leg * 0.55 - 0.05 * k;
        let mut bottom = top - Vec3::Y * hang + back * (0.1 * k + flow * leg * 0.75);
        bottom.y = bottom.y.max(0.12 * k);
        for xs in [-1.0f32, 0.0, 1.0] {
            let off = cr * Vec3::X * xs * tr * 0.62;
            push(top + off, bottom + off * 1.35, lr * 0.85 * gear.cape, lr * 1.05 * gear.cape, cc);
        }
    }

    // Legs.
    for i in 0..2 {
        let (hip, knee, ankle, toe) = (sk.hip[i], sk.knee[i], sk.ankle[i], sk.toe[i]);
        push(hip, knee, lr * 1.25, lr * 1.05, pants);
        push(knee, ankle, lr * 1.05, lr * 0.9, pants);
        push(ankle + Vec3::Y * 0.02 - toe * 0.02, ankle + Vec3::Y * 0.02 + toe * 0.12 * k, lr * 0.95, lr * 0.85, shoes);
    }

    // Arms.
    let glove = Color::try_hex(&def.gear.gloves).unwrap_or(skin);
    let fore = if def.gear.gloves.is_empty() { skin } else { glove };
    for (i, s) in [(0usize, -1.0f32), (1, 1.0)] {
        let (shoulder, elbow, hand) = (sk.shoulder[i], sk.elbow[i], sk.hand[i]);
        push(shoulder, elbow, lr * 1.05, lr * 0.95, shirt);
        push(elbow, hand, lr * 0.95, lr * if def.gear.gloves.is_empty() { 0.85 } else { 1.05 }, fore);
        push(hand, hand, lr * 1.1, lr * 1.1, glove);
        if def.gear.pauldrons > 0.0 {
            let pc = Color::try_hex(&def.gear.armor_color).unwrap_or(shirt);
            let p = shoulder + cr * Vec3::new(s * 0.03 * k, 0.04 * k, 0.0);
            let r = lr * 1.9 * def.gear.pauldrons;
            push(p, p + cr * Vec3::new(s * 0.05 * k, -0.02 * k, 0.0), r, r * 0.85, pc);
        }
    }
    // Weapon in the right hand, off-hand item in the left.
    let mut held_parts = Vec::new();
    let mut span: Option<(Vec3, Vec3)> = None;
    if def.weapon.kind != WeaponKind::None || def.weapon.offhand != OffhandKind::None {
        let (hand, dir) = (sk.hand[1], sk.weapon);
        weapon_parts(&def.weapon, k, hand, dir, sk.hand[0], &mut held_parts);
        for p in held_parts.iter_mut() {
            p.a = local(p.a);
            p.b = local(p.b);
        }
        if def.weapon.kind != WeaponKind::None {
            span = Some((local(hand), local(hand + dir * weapon_length(def.weapon.kind) * def.weapon.size.max(0.3) * k)));
        }
    }

    // Eyes: on the face, pushed toward the camera so they read from high angles; shut in a blink.
    let cam_up = (-cam_fwd).dot(up).clamp(0.0, 1.0) * def.eyes_to_camera;
    let face_dir = (Vec3::Z * (1.0 - cam_up) + Vec3::Y * cam_up * 1.2).normalize();
    let face_dir = face_dir.lerp(Vec3::new(0.0, 0.3 + cam_up * 0.5, 1.0).normalize(), st.crawl).normalize();
    let out = if def.gear.helm == HelmKind::Great { 1.13 } else { 0.9 };
    for s in [-1.0f32, 1.0] {
        let side = Vec3::new(s * 0.36, 0.08, 0.0) * hr;
        let e = head + hq * ((face_dir * hr * 0.93 + side).normalize() * hr * out);
        if sk.blink > 0.5 {
            let x = hq * Vec3::X * hr * 0.12;
            push(e - x, e + x, hr * 0.06, hr * 0.06, eyes);
        } else {
            push(e, e, hr * 0.17, hr * 0.17, eyes);
        }
    }
    parts.extend(held_parts);
    if st.roll > 0.01 || st.roll_angle.rem_euclid(std::f32::consts::TAU) > 0.01 {
        // Dodge roll: tumble forward about the side axis through the curled-up body.
        let q = Quat::from_axis_angle(right, st.roll_angle);
        let pivot = feet + up * 0.42 * k;
        for p in &mut parts {
            p.a = pivot + q * (p.a - pivot);
            p.b = pivot + q * (p.b - pivot);
        }
        span = span.map(|(a, b)| (pivot + q * (a - pivot), pivot + q * (b - pivot)));
    }
    (parts, span)
}

/// Reach of a weapon from the hand to its tip (before size and scale).
pub fn weapon_length(k: WeaponKind) -> f32 {
    match k {
        WeaponKind::None => 0.0,
        WeaponKind::Sword => 0.88,
        WeaponKind::Greatsword => 1.35,
        WeaponKind::Dagger => 0.43,
        WeaponKind::Axe => 0.75,
        WeaponKind::Maul => 1.0,
        WeaponKind::Mace => 0.66,
        WeaponKind::Spear => 1.25,
        WeaponKind::Staff => 1.1,
        WeaponKind::Wand => 0.38,
        WeaponKind::Claw => 0.3,
    }
}

/// A held weapon (and off-hand) as parts in character space: `h` is the hand, `d` the
/// direction the weapon points.
pub fn weapon_parts(w: &WeaponLook, k: f32, h: Vec3, d: Vec3, off: Vec3, out: &mut Vec<PuppetPart>) {
    let c = Color::try_hex(&w.color).unwrap_or(Color::hex("#c9ced8"));
    let g = Color::try_hex(&w.grip).unwrap_or(Color::hex("#5a3b26"));
    let sz = w.size.max(0.3) * k;
    let p = d.cross(Vec3::Y).normalize_or(Vec3::X);
    let glow = w.glow;
    let mut seg = |a: Vec3, b: Vec3, ra: f32, rb: f32, color: Color, glow: f32| {
        out.push(PuppetPart { a, b, ra: ra * k, rb: rb * k, color, glow });
    };
    match w.kind {
        WeaponKind::None => {}
        WeaponKind::Sword | WeaponKind::Greatsword => {
            let big = w.kind == WeaponKind::Greatsword;
            let (len, r, grip) = if big { (1.25, 0.06, 0.18) } else { (0.78, 0.045, 0.07) };
            seg(h - d * grip, h + d * 0.07, 0.028, 0.028, g, 0.0);
            seg(h + d * 0.08 - p * 0.12 * k, h + d * 0.08 + p * 0.12 * k, 0.024, 0.024, c, 0.0);
            seg(h + d * 0.1, h + d * (0.1 + len * sz), r, r * 0.4, c, glow);
        }
        WeaponKind::Dagger => {
            seg(h - d * 0.05, h + d * 0.05, 0.025, 0.025, g, 0.0);
            seg(h + d * 0.07, h + d * (0.07 + 0.36 * sz), 0.035, 0.012, c, glow);
        }
        WeaponKind::Axe => {
            let top = h + d * 0.55 * sz;
            seg(h - d * 0.12, h + d * 0.62 * sz, 0.028, 0.028, g, 0.0);
            seg(top - p * 0.04 * k, top + p * 0.2 * sz, 0.1, 0.05, c, glow);
        }
        WeaponKind::Maul => {
            let top = h + d * 0.85 * sz;
            seg(h - d * 0.25, top, 0.035, 0.035, g, 0.0);
            seg(top - p * 0.17 * sz, top + p * 0.17 * sz, 0.14, 0.14, c, glow);
        }
        WeaponKind::Mace => {
            seg(h - d * 0.08, h + d * 0.5 * sz, 0.03, 0.03, g, 0.0);
            let top = h + d * 0.55 * sz;
            seg(top, top, 0.11, 0.11, c, glow);
        }
        WeaponKind::Spear => {
            seg(h - d * 0.7 * sz, h + d * 1.0 * sz, 0.03, 0.03, g, 0.0);
            seg(h + d * 1.0 * sz, h + d * 1.25 * sz, 0.05, 0.005, c, glow);
        }
        WeaponKind::Staff => {
            seg(h - d * 0.6 * sz, h + d * 0.95 * sz, 0.035, 0.035, g, 0.0);
            let top = h + d * 1.03 * sz;
            seg(top, top, 0.085, 0.085, c, 1.5 + glow);
        }
        WeaponKind::Wand => {
            seg(h - d * 0.04, h + d * 0.32 * sz, 0.025, 0.015, g, 0.0);
            let top = h + d * 0.35 * sz;
            seg(top, top, 0.04, 0.04, c, 1.2 + glow);
        }
        WeaponKind::Claw => {
            for i in [-1.0f32, 0.0, 1.0] {
                seg(h + p * 0.04 * i * k, h + d * 0.3 * sz + p * 0.06 * i * k, 0.016, 0.006, c, glow);
            }
        }
    }
    let oc = Color::try_hex(&w.offhand_color).unwrap_or(Color::hex("#8a6a3a"));
    match w.offhand {
        OffhandKind::None => {}
        OffhandKind::Shield => {
            let at = off + Vec3::new(-0.06, 0.05, 0.1) * k;
            seg(at, at, 0.24, 0.24, oc, 0.0);
            seg(at + Vec3::new(-0.05, 0.0, 0.06) * k, at + Vec3::new(-0.05, 0.0, 0.06) * k, 0.07, 0.07, c, 0.0);
        }
        OffhandKind::Focus => {
            let at = off + Vec3::new(0.0, 0.18, 0.08) * k;
            seg(at, at, 0.08, 0.08, oc, 1.6);
        }
    }
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
        for plan in [BodyPlan::Spider, BodyPlan::Lizard, BodyPlan::Beetle, BodyPlan::Blob, BodyPlan::Quadruped] {
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
