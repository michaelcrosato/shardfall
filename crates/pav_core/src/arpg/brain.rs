//! Monster brains: an archetype decides how a monster moves and when it uses its skills. The
//! skills themselves (with their wind-ups and telegraphs) are the same ones the hero uses.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::data::{Archetype, Behavior, SkillDef};
use crate::rng::Rng;

/// How far monsters notice the hero, and how close packmates must be to join in.
pub const AGGRO_RANGE: f32 = 12.0;
pub const PACK_RANGE: f32 = 9.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Brain {
    pub arch: Archetype,
    pub aggro: bool,
    pub home: Vec3,
    /// Ranged: strafing direction (±1) and seconds until it flips.
    pub strafe: f32,
    pub strafe_t: f32,
    /// Seconds before the next attack decision (a little hesitation between attacks).
    pub think: f32,
    /// Idle wandering target.
    pub wander: Option<Vec3>,
    pub wait: f32,
}

impl Brain {
    pub fn new(arch: Archetype, home: Vec3) -> Self {
        Self { arch, aggro: false, home, strafe: 1.0, strafe_t: 0.0, think: 0.3, wander: None, wait: 1.0 }
    }
}

/// What a brain sees of one of its skills.
pub struct SkillOption<'a> {
    pub id: u16,
    pub def: &'a SkillDef,
    pub ready: bool,
}

/// A decision: where to move (world XZ, length <= 1) and maybe a skill to use at a point.
pub struct Decision {
    pub move_dir: Vec2,
    pub cast: Option<(u16, Vec3)>,
}

fn flat(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

impl Brain {
    /// `me` and `target` are feet positions; `reach` is the sum of both bodies' radii.
    pub fn think(
        &mut self,
        me: Vec3,
        target: Option<Vec3>,
        reach: f32,
        skills: &[SkillOption],
        rng: &mut Rng,
        dt: f32,
    ) -> Decision {
        self.think = (self.think - dt).max(0.0);
        let Some(t) = target.filter(|_| self.aggro) else {
            return self.idle(me, rng, dt);
        };
        let d = flat(t - me);
        let dist = d.length();
        let dir = d.normalize_or_zero();
        // Attack: a ready skill whose range reaches the target (the special ones first).
        if self.think <= 0.0 {
            let mut best: Option<&SkillOption> = None;
            for s in skills.iter().filter(|s| s.ready) {
                let range = s.def.range + reach;
                let min = if s.def.behavior == Behavior::Charge { 3.0 } else { 0.0 };
                if dist <= range && dist >= min && best.is_none_or(|b| s.def.cooldown > b.def.cooldown) {
                    best = Some(s);
                }
            }
            if let Some(s) = best {
                self.think = rng.range(0.25, 0.7);
                return Decision { move_dir: Vec2::ZERO, cast: Some((s.id, t)) };
            }
        }
        let move_dir = match self.arch {
            Archetype::Melee | Archetype::Charger => {
                if dist > reach * 0.9 + 0.3 {
                    dir
                } else {
                    Vec2::ZERO
                }
            }
            Archetype::Ranged => {
                self.strafe_t -= dt;
                if self.strafe_t <= 0.0 {
                    self.strafe = if rng.f32() < 0.5 { -1.0 } else { 1.0 };
                    self.strafe_t = rng.range(1.2, 2.8);
                }
                let range = skills.iter().map(|s| s.def.range).fold(6.0, f32::max);
                let side = Vec2::new(-dir.y, dir.x) * self.strafe;
                if dist < range * 0.45 {
                    (-dir + side * 0.4).normalize_or_zero()
                } else if dist > range * 0.85 {
                    (dir + side * 0.2).normalize_or_zero()
                } else {
                    side * 0.7
                }
            }
        };
        Decision { move_dir, cast: None }
    }

    fn idle(&mut self, me: Vec3, rng: &mut Rng, dt: f32) -> Decision {
        if self.wait > 0.0 {
            self.wait -= dt;
            return Decision { move_dir: Vec2::ZERO, cast: None };
        }
        let goal = *self.wander.get_or_insert_with(|| {
            let a = rng.range(0.0, std::f32::consts::TAU);
            self.home + Vec3::new(a.cos(), 0.0, a.sin()) * rng.range(1.0, 3.5)
        });
        let d = flat(goal - me);
        if d.length() < 0.4 {
            self.wander = None;
            self.wait = rng.range(1.0, 3.5);
            return Decision { move_dir: Vec2::ZERO, cast: None };
        }
        Decision { move_dir: d.normalize_or_zero() * 0.35, cast: None }
    }
}
