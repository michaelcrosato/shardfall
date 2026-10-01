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
        // Spend passive points (one per tick, every few ticks).
        if g.hero.points() > 0 && self.stats.ticks.is_multiple_of(6) {
            f.cmd = self.pick_passive(g);
        } else if self.stats.ticks % 6 == 3 {
            // Masteries taken without a choice: the first free option.
            let t = &d.tree;
            for id in &g.hero.tree {
                let Some(n) = t.node(*id).filter(|n| !n.mastery.is_empty() && !g.hero.masteries.contains_key(id)) else {
                    continue;
                };
                let used: Vec<u8> = g
                    .hero
                    .masteries
                    .iter()
                    .filter(|(nid, _)| t.node(**nid).is_some_and(|x| x.mastery == n.mastery))
                    .map(|(_, o)| *o)
                    .collect();
                if let Some(o) = (0..4u8).find(|o| !used.contains(o)) {
                    f.cmd = Some(super::GameCmd::Mastery(*id, o));
                    break;
                }
            }
        }
        // Wear upgrades now and then.
        if f.cmd.is_none() && self.stats.ticks % 90 == 45 {
            f.cmd = self.pick_upgrade(g);
        }
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
        // Keep spinning while something is close.
        if let Some(c) = &hero.cast {
            let def = d.skill(c.skill);
            if def.behavior == Behavior::Channel && c.button != 0 && dist <= def.radius + r + 0.5 {
                f.held |= c.button;
                let n = flat(t - me).normalize_or_zero();
                f.move_dir = Vec2::new(n.x, n.z) * 0.5;
                return f;
            }
        }
        // A ready skill that reaches, best (longest cooldown) first; else the basic attack.
        let mut best: Option<(usize, f32)> = None;
        for (slot, key) in g.hero.bar.iter().enumerate() {
            let Some(id) = d.skill_id(key) else { continue };
            let s = super::skills::skill_of(hero, id);
            if hero.cooldown(id) > 0.0 || hero.mana < s.cost * hero.sheet.mana_cost {
                continue;
            }
            let reach = match s.behavior {
                Behavior::Projectile => s.range * 0.7,
                Behavior::Nova | Behavior::Channel | Behavior::Buff => s.radius,
                Behavior::Wave => 1.2 + s.count as f32 * s.radius * 1.2,
                Behavior::Leap | Behavior::Dash | Behavior::Meteor | Behavior::Field | Behavior::Rain => s.range,
                // Blink only to close a long gap.
                Behavior::Blink if dist < 7.0 => continue,
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

    /// The next passive to take: notables and skill upgrades for skills on the bar first, then
    /// life and damage, never keystones (they change how the bot would have to play).
    fn pick_passive(&self, g: &super::Game) -> Option<super::GameCmd> {
        use super::stats::Stat;
        use super::tree::NodeKind;
        let d = data();
        let t = &d.tree;
        let alloc = &g.hero.tree;
        let mut best: Option<(f32, u32)> = None;
        for n in &t.nodes {
            if alloc.contains(&n.id) || n.kind == NodeKind::Keystone || !t.can_allocate(alloc, n.id) {
                continue;
            }
            let mut score = match n.kind {
                NodeKind::Notable => 3.0,
                NodeKind::Mastery => 2.5,
                NodeKind::Skill => {
                    if n.tweaks.iter().any(|tw| g.hero.bar.contains(&tw.skill) && tw.field != super::skills::TweakField::Element)
                    {
                        3.0
                    } else {
                        0.3
                    }
                }
                _ => 1.0,
            };
            for (s, _) in &n.stats {
                score += match s {
                    Stat::Life | Stat::LifeInc | Stat::MeleeInc | Stat::PhysInc | Stat::AttackSpeed | Stat::DamageInc => 0.6,
                    Stat::FireRes | Stat::ColdRes | Stat::LightningRes | Stat::AllRes | Stat::Armor | Stat::ArmorInc => 0.3,
                    _ => 0.0,
                };
            }
            // Closer to the start first (cheaper paths later), a little.
            score -= n.pos.length() * 0.02;
            if best.is_none_or(|b| score > b.0) {
                best = Some((score, n.id));
            }
        }
        best.map(|b| super::GameCmd::Allocate(b.1))
    }

    /// The best bag item that beats what is worn in its slot.
    fn pick_upgrade(&self, g: &super::Game) -> Option<super::GameCmd> {
        let d = data();
        let mut best: Option<(f32, u32)> = None;
        for it in &g.hero.inventory {
            let slot = it.slot(&d);
            let worn = super::items::EquipSlot::ALL
                .iter()
                .filter(|e| e.slot() == slot)
                .map(|e| g.hero.worn(*e).map(|w| w.score(&d)).unwrap_or(0.0))
                .fold(f32::MAX, f32::min);
            // Two-handers cost the off-hand too.
            let off = if it.base_def(&d).is_some_and(|b| b.two_hand) {
                g.hero.worn(super::items::EquipSlot::Offhand).map(|w| w.score(&d)).unwrap_or(0.0)
            } else {
                0.0
            };
            let gain = it.score(&d) - worn - off;
            if gain > 1.0 && best.is_none_or(|b| gain > b.0) {
                best = Some((gain, it.id));
            }
        }
        best.map(|b| super::GameCmd::Equip(b.1))
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
