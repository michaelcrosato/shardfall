//! Loot on the ground: what monsters drop when they die (items and gold), how drops pop out
//! and land, the gold magnet, picking things up and the auto-loot filter.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::combat::{FloatKind, Rarity};
use super::data::data;
use super::items::{Item, RollSpec, roll_item};
use super::{Game, feet_of, flat};
use crate::frame::SimEvent;
use crate::sim::Sim;

/// An item lying on the ground.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroundItem {
    pub item: Item,
    pub pos: Vec3,
    pub vel: Vec3,
    pub rest: bool,
    pub age: f32,
}

/// A pile of gold (picked up by walking near; it flies to you).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GoldPile {
    pub amount: u64,
    pub pos: Vec3,
    pub vel: Vec3,
    pub rest: bool,
    pub age: f32,
}

const GRAVITY: f32 = 22.0;
/// Gold flies to the hero within this distance; items are auto-looted within `LOOT_REACH`.
pub const GOLD_MAGNET: f32 = 4.5;
pub const LOOT_REACH: f32 = 1.4;
/// Clicking an item label picks it up from this far.
pub const CLICK_REACH: f32 = 5.0;

fn pop(sim: &mut Sim, at: Vec3) -> (Vec3, Vec3) {
    let rng = &mut sim.state.rng;
    let a = rng.range(0.0, std::f32::consts::TAU);
    let s = rng.range(1.2, 3.2);
    (at + Vec3::Y * 0.6, Vec3::new(a.cos() * s, rng.range(5.0, 7.0), a.sin() * s))
}

impl Game {
    /// Drops from a slain monster (item level = its level).
    pub(crate) fn drop_loot(&mut self, sim: &mut Sim, at: Vec3, level: u32, rarity: Rarity, events: &mut Vec<SimEvent>) {
        let d = data();
        let (item_rarity, gold_find) = match self.hero_actor() {
            Some(h) => (h.sheet.item_rarity, h.sheet.gold_find),
            None => (0.0, 0.0),
        };
        // How many items: bosses shower, rares drop a few, the rest sometimes.
        let rng = &mut sim.state.rng;
        let n = match rarity {
            Rarity::Normal => (rng.f32() < 0.11 * self.loot_rate) as u32,
            Rarity::Magic => (rng.f32() < 0.35 * self.loot_rate) as u32 + (rng.f32() < 0.08) as u32,
            Rarity::Rare => 1 + (rng.f32() < 0.6) as u32 + (rng.f32() < 0.25) as u32,
            Rarity::Unique => 3 + rng.below(3),
        };
        let bonus = item_rarity + [0.0, 40.0, 120.0, 300.0][rarity as usize];
        for i in 0..n {
            let id = self.hero.new_id();
            let mut spec = RollSpec { level, rarity_bonus: bonus, ..Default::default() };
            if i == 0 && rarity >= Rarity::Rare {
                // The first drop of a rare (boss) is at least magic (rare).
                let r = super::items::roll_rarity(&mut sim.state.rng, bonus);
                spec.rarity = Some(r.max(if rarity == Rarity::Unique { Rarity::Rare } else { Rarity::Magic }));
            }
            let Some(item) = roll_item(&d, &mut sim.state.rng, spec, id) else { continue };
            self.drop_item(sim, item, at, events);
        }
        // Gold.
        let rng = &mut sim.state.rng;
        let chance = [0.4, 0.75, 1.0, 1.0][rarity as usize];
        if rng.f32() < chance {
            let piles = [1, 1, 2, 5][rarity as usize];
            let total = ((2.0 + level as f32 * 1.3 + 0.02 * (level as f32).powi(2))
                * rng.range(0.6, 1.4)
                * [1.0, 2.0, 4.0, 15.0][rarity as usize]
                * (1.0 + gold_find / 100.0))
                .round()
                .max(1.0) as u64;
            for _ in 0..piles {
                let (pos, vel) = pop(sim, at);
                self.gold.push(GoldPile { amount: (total / piles).max(1), pos, vel: vel * 0.8, rest: false, age: 0.0 });
            }
        }
    }

    /// Puts an item on the ground near `at` (popping out).
    pub fn drop_item(&mut self, sim: &mut Sim, item: Item, at: Vec3, events: &mut Vec<SimEvent>) {
        let (pos, vel) = pop(sim, at);
        events.push(SimEvent::Loot { pos: at, rarity: item.rarity as u8 });
        self.loot.push(GroundItem { item, pos, vel, rest: false, age: 0.0 });
        self.inv_changed();
    }

    /// Lands drops, pulls gold in, auto-loots.
    pub(crate) fn update_loot(&mut self, sim: &mut Sim, dt: f32, events: &mut Vec<SimEvent>) {
        let hero =
            self.hero_id.filter(|h| self.actors.get(h).is_some_and(|a| !a.dead)).and_then(|h| feet_of(sim, h)).map(|f| f.0);
        let physics = &sim.state.physics;
        let ignore = self.hero_id.and_then(|h| sim.state.entities.get(h)).and_then(|e| e.body);
        let fly = |pos: &mut Vec3, vel: &mut Vec3, rest: &mut bool| {
            if *rest {
                return;
            }
            vel.y -= GRAVITY * dt;
            let side = Vec3::new(vel.x, 0.0, vel.z) * dt;
            if side.length() > 1e-5
                && crate::projectile::static_hit(physics, *pos + Vec3::Y * 0.2, side.normalize(), side.length() + 0.25, ignore)
                    .is_some()
            {
                vel.x = 0.0;
                vel.z = 0.0;
            }
            let ground = crate::projectile::static_hit(physics, *pos + Vec3::Y * 0.5, Vec3::NEG_Y, 30.0, ignore)
                .map(|t| pos.y + 0.5 - t)
                .unwrap_or(pos.y - 100.0);
            *pos += *vel * dt;
            if pos.y <= ground + 0.05 && vel.y < 0.0 {
                pos.y = ground + 0.05;
                if vel.y < -3.0 {
                    vel.y *= -0.3;
                    vel.x *= 0.5;
                    vel.z *= 0.5;
                } else {
                    *vel = Vec3::ZERO;
                    *rest = true;
                }
            }
        };
        for g in &mut self.loot {
            g.age += dt;
            fly(&mut g.pos, &mut g.vel, &mut g.rest);
        }
        let mut got_gold = 0u64;
        let mut gold_at = Vec3::ZERO;
        self.gold.retain_mut(|g| {
            g.age += dt;
            if let Some(h) = hero.filter(|h| g.age > 0.35 && flat(*h - g.pos).length() < GOLD_MAGNET) {
                // The magnet: fly to the hero's middle, faster as it closes in.
                let target = h + Vec3::Y * 0.9;
                let to = target - g.pos;
                if to.length() < 0.6 {
                    got_gold += g.amount;
                    gold_at = h;
                    return false;
                }
                let speed = 6.0 + g.age * 10.0;
                g.pos += to.normalize() * (speed * dt).min(to.length());
                g.rest = true;
                return true;
            }
            fly(&mut g.pos, &mut g.vel, &mut g.rest);
            g.age < 300.0
        });
        if got_gold > 0 {
            self.hero.gold += got_gold;
            self.float(gold_at + Vec3::Y * 1.8, got_gold as f32, FloatKind::Gold);
            events.push(SimEvent::Coin { pos: gold_at });
            self.inv_changed();
        }
        // Auto-loot: walk over it (items at least as rare as the filter).
        if let Some(h) = hero.filter(|_| !self.auto_loot_off) {
            let min = self.auto_loot;
            let near: Vec<u32> = self
                .loot
                .iter()
                .filter(|g| {
                    g.rest && g.item.rarity >= min && flat(g.pos - h).length() < LOOT_REACH && (g.pos.y - h.y).abs() < 1.5
                })
                .map(|g| g.item.id)
                .collect();
            for id in near {
                if self.hero.bag_full() {
                    if self.time - self.full_warned > 3.0 {
                        self.full_warned = self.time;
                        self.float_text(h + Vec3::Y * 2.3, "Inventory full");
                    }
                    break;
                }
                self.take_ground(sim, id, events);
            }
        }
    }

    /// Moves a ground item into the bag.
    pub(crate) fn take_ground(&mut self, sim: &mut Sim, id: u32, events: &mut Vec<SimEvent>) -> bool {
        let Some(i) = self.loot.iter().position(|g| g.item.id == id) else { return false };
        if self.hero.bag_full() {
            return false;
        }
        let g = self.loot.remove(i);
        events.push(SimEvent::Pickup { pos: g.pos, rarity: g.item.rarity as u8 });
        let _ = sim;
        self.hero.inventory.push(g.item);
        self.inv_changed();
        true
    }

    /// The `Pickup` command: an item label was clicked.
    pub(crate) fn pickup(&mut self, sim: &mut Sim, id: u32, events: &mut Vec<SimEvent>) -> Result<(), String> {
        let hero = self.hero_id.and_then(|h| feet_of(sim, h)).map(|f| f.0).ok_or("no hero")?;
        let g = self.loot.iter().find(|g| g.item.id == id).ok_or("it's gone")?;
        if flat(g.pos - hero).length() > CLICK_REACH {
            return Err("Too far away".into());
        }
        if self.hero.bag_full() {
            return Err("Inventory full".into());
        }
        self.take_ground(sim, id, events);
        Ok(())
    }

    /// Nearest ground item to a point (within `reach`).
    pub fn loot_near(&self, at: Vec3, reach: f32) -> Option<u32> {
        self.loot
            .iter()
            .map(|g| (g.item.id, flat(g.pos - at).length()))
            .filter(|x| x.1 <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|x| x.0)
    }
}
