//! A simple player bot: fights the nearest monster, uses skills when they are ready, drinks
//! potions when low and rolls out of monster telegraphs. Agents use it (the `autoplay` tool)
//! to balance and speed-test levels; tests use it to prove the loop works.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::combat::Team;
use super::data::{Behavior, data};
use super::{TeleShape, feet_of, flat};
use crate::input::{InputFrame, buttons};
use crate::sim::Sim;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BotStats {
    pub ticks: u64,
    pub kills: u64,
    pub deaths: u32,
    pub dodges: u32,
    pub potions: u32,
    pub damage_taken: f32,
    pub xp: f64,
    pub gold: u64,
    pub levels: u32,
}

/// Bot memory between ticks.
#[derive(Clone, Debug, Default)]
pub struct Bot {
    last_life: f32,
    pub stats: BotStats,
    /// Where to walk when nothing is around (level exits; set by the caller).
    pub goal: Option<Vec3>,
}

const SLOT_BUTTONS: [u32; 6] =
    [buttons::PRIMARY, buttons::SECONDARY, buttons::SKILL3, buttons::SKILL4, buttons::SKILL5, buttons::SKILL6];

impl Bot {
    /// The input for this tick.
    pub fn input(&mut self, sim: &Sim) -> InputFrame {
        let Some(g) = sim.state.game.as_ref() else { return InputFrame::default() };
        let Some(hid) = g.hero_id else { return InputFrame::default() };
        let Some(hero) = g.actors.get(&hid) else { return InputFrame::default() };
        let Some((me, _)) = feet_of(sim, hid) else { return InputFrame::default() };
        let d = data();
        self.stats.ticks += 1;
        if self.last_life > hero.life {
            self.stats.damage_taken += self.last_life - hero.life;
        }
        self.last_life = hero.life;
        if hero.dead {
            return InputFrame::default();
        }
        let mut f = InputFrame::default();
        // Low life: drink.
        if hero.life < hero.sheet.life_max * 0.35 && g.hero.potions > 0 {
            f.pressed |= buttons::POTION;
            f.held |= buttons::POTION;
            self.stats.potions += 1;
        }
        // Standing in a telegraph: roll out of it.
        let frame = g.frame(sim);
        for t in &frame.telegraphs {
            let (inside, away) = match t.shape {
                TeleShape::Circle { center, radius } => (flat(me - center).length() < radius + 0.4, flat(me - center)),
                TeleShape::Arc { center, dir, range, angle } => {
                    let to = flat(me - center);
                    (to.length() < range + 0.4 && to.normalize_or_zero().dot(dir) > (angle.to_radians() * 0.5).cos() - 0.2, to)
                }
                TeleShape::Line { from, dir, length, width } => {
                    let to = flat(me - from);
                    let along = to.dot(dir);
                    let side = Vec3::new(-dir.z, 0.0, dir.x);
                    (along > -0.5 && along < length && to.dot(side).abs() < width * 0.5 + 0.4, side * to.dot(side).signum())
                }
            };
            if inside && t.progress > 0.35 && hero.dodge_cd <= 0.0 {
                let a = away.normalize_or(Vec3::X);
                f.move_dir = Vec2::new(a.x, a.z);
                f.pressed |= buttons::DODGE;
                f.held |= buttons::DODGE;
                self.stats.dodges += 1;
                return f;
            }
        }
        // Target: the nearest living monster.
        let target = g
            .actors
            .iter()
            .filter(|(_, a)| a.team == Team::Monster && !a.dead)
            .filter_map(|(id, a)| feet_of(sim, *id).map(|(p, h)| (p, h, a.radius)))
            .min_by(|a, b| flat(a.0 - me).length().total_cmp(&flat(b.0 - me).length()));
        let Some((t, h, r)) = target else {
            if let Some(goal) = self.goal {
                let d = flat(goal - me);
                if d.length() > 0.6 {
                    let n = d.normalize();
                    f.move_dir = Vec2::new(n.x, n.z);
                }
            }
            return f;
        };
        f.aim = Some(t + Vec3::Y * h * 0.5);
        let dist = flat(t - me).length();
        // A ready skill that reaches, best (longest cooldown) first; else the basic attack.
        let mut best: Option<(usize, f32)> = None;
        for (slot, key) in g.hero.bar.iter().enumerate() {
            let Some(id) = d.skill_id(key) else { continue };
            let s = d.skill(id);
            if hero.cooldown(id) > 0.0 || hero.mana < s.cost * hero.sheet.mana_cost {
                continue;
            }
            let reach = match s.behavior {
                Behavior::Projectile => s.range * 0.7,
                Behavior::Nova => s.radius,
                Behavior::Leap | Behavior::Dash => s.range,
                _ => s.range,
            } + r;
            if dist <= reach && best.is_none_or(|b| s.cooldown + s.cost * 0.01 > b.1) {
                best = Some((slot, s.cooldown + s.cost * 0.01));
            }
        }
        match best {
            Some((slot, _)) => {
                f.held |= SLOT_BUTTONS[slot];
                f.pressed |= SLOT_BUTTONS[slot];
                if dist > 2.0 && slot == 0 {
                    let n = flat(t - me).normalize_or_zero();
                    f.move_dir = Vec2::new(n.x, n.z);
                }
            }
            None => {
                let n = flat(t - me).normalize_or_zero();
                f.move_dir = Vec2::new(n.x, n.z);
            }
        }
        f
    }

    /// Runs the bot for `ticks` and returns what happened.
    pub fn run(&mut self, sim: &mut Sim, ticks: u64) -> BotStats {
        let start = sim.state.game.as_ref().map(|g| (g.hero.kills, g.hero.deaths, g.hero.gold, g.hero.level));
        let xp0 = sim.state.game.as_ref().map(|g| total_xp(g.hero.level, g.hero.xp)).unwrap_or(0.0);
        for _ in 0..ticks {
            let f = self.input(sim);
            sim.step(&f);
        }
        if let (Some((k, dth, gold, lvl)), Some(g)) = (start, sim.state.game.as_ref()) {
            self.stats.kills = g.hero.kills - k;
            self.stats.deaths = g.hero.deaths - dth;
            self.stats.gold = g.hero.gold - gold;
            self.stats.levels = g.hero.level - lvl;
            self.stats.xp = total_xp(g.hero.level, g.hero.xp) - xp0;
        }
        self.stats.clone()
    }
}

fn total_xp(level: u32, xp: f64) -> f64 {
    (1..level).map(super::hero::xp_to_next).sum::<f64>() + xp
}
