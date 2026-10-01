//! Combat state: actors (anything with life: the hero, monsters, breakable kegs), damage
//! packets, ailments and buffs, projectiles, ground effects and floating numbers.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::brain::Brain;
use super::data::Element;
use super::stats::{Mods, Sheet};
use crate::entity::EntityId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Team {
    #[default]
    Hero,
    Monster,
    /// Hit by everyone (kegs, totems of the level).
    Neutral,
}

impl Team {
    /// Whether `self` can damage `other`.
    pub fn hostile(self, other: Team) -> bool {
        self != other || self == Team::Neutral
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rarity {
    #[default]
    Normal,
    Magic,
    Rare,
    Unique,
}

impl Rarity {
    pub fn life_mult(self) -> f32 {
        [1.0, 2.2, 4.5, 14.0][self as usize]
    }
    pub fn damage_mult(self) -> f32 {
        [1.0, 1.25, 1.6, 2.0][self as usize]
    }
    pub fn xp_mult(self) -> f32 {
        [1.0, 2.5, 6.0, 30.0][self as usize]
    }
    pub fn color(self) -> [f32; 3] {
        [[0.9, 0.9, 0.9], [0.45, 0.6, 1.0], [1.0, 0.85, 0.3], [1.0, 0.5, 0.15]][self as usize]
    }
}

/// One hit's damage, per element, before the target's defences.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Damage {
    pub amount: [f32; 5],
    pub crit: bool,
    /// Chance (0..1) per element to apply its ailment.
    pub ailment: [f32; 5],
    pub ailment_mult: f32,
    pub knockback: f32,
    pub source: Option<EntityId>,
    pub skill: u16,
    pub attack: bool,
    pub melee: bool,
}

impl Damage {
    pub fn total(&self) -> f32 {
        self.amount.iter().sum()
    }
    pub fn scaled(mut self, k: f32) -> Self {
        for a in &mut self.amount {
            *a *= k;
        }
        self
    }
    /// The element doing the most damage (for colours and sounds).
    pub fn main_element(&self) -> Element {
        let mut best = 0;
        for i in 1..5 {
            if self.amount[i] > self.amount[best] {
                best = i;
            }
        }
        Element::ALL[best]
    }
}

/// Damage over time: damage per second and seconds left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Dot {
    pub dps: f32,
    pub time: f32,
    pub source: Option<EntityId>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ailments {
    pub bleed: Dot,
    pub ignite: Dot,
    pub poison: Vec<Dot>,
    /// Slow (0..1) and seconds left.
    pub chill: (f32, f32),
    pub freeze: f32,
    /// Extra damage taken (0..1) and seconds left.
    pub shock: (f32, f32),
}

impl Ailments {
    pub fn any(&self) -> bool {
        self.bleed.time > 0.0
            || self.ignite.time > 0.0
            || !self.poison.is_empty()
            || self.chill.1 > 0.0
            || self.freeze > 0.0
            || self.shock.1 > 0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuffKind {
    /// Shrines and skills.
    Frenzy,
    Fury,
    Haste,
    Ward,
    Custom,
}

/// A timed bundle of stat changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Buff {
    pub kind: BuffKind,
    pub name: String,
    pub time: f32,
    pub mods: Mods,
}

/// A skill being performed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cast {
    pub skill: u16,
    /// Seconds since it began, its length and when it hits.
    pub t: f32,
    pub dur: f32,
    pub hit_at: f32,
    pub dir: Vec3,
    pub target: Vec3,
    pub origin: Vec3,
    pub fired: bool,
    /// Combo swing (0, 1, 2 ...) and side (±1) for alternating animations.
    pub combo: u32,
    pub side: f32,
    /// Already hit by this cast (dashes and charges hit each thing once).
    pub hits: Vec<EntityId>,
}

/// Anything with life.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Actor {
    pub team: Team,
    pub name: String,
    pub level: u32,
    pub rarity: Rarity,
    /// Monster family key (empty for the hero and props).
    pub family: String,
    pub sheet: Sheet,
    pub life: f32,
    pub mana: f32,
    /// Hit radius (m) and height of the body's centre above the feet.
    pub radius: f32,
    /// Attack damage before multipliers (monsters; the hero uses the weapon).
    pub base_damage: f32,
    /// Movement speed multiplier (1 = the hero's base speed).
    pub speed: f32,
    pub skills: Vec<u16>,
    pub cast: Option<Cast>,
    /// Next skill to perform after this one (buffered input): skill and target.
    pub queued: Option<(u16, Vec3)>,
    pub cooldowns: Vec<(u16, f32)>,
    pub combo: u32,
    pub combo_timer: f32,
    pub iframes: f32,
    pub dodge_cd: f32,
    pub ailments: Ailments,
    pub buffs: Vec<Buff>,
    pub brain: Option<Brain>,
    pub flash: f32,
    pub dead: bool,
    pub death_t: f32,
    pub xp: f32,
    /// Pack id (monsters aggro together).
    pub pack: u32,
    /// Monster affix names (shown on the nameplate).
    pub affixes: Vec<String>,
    /// Does not move or get knocked around (kegs, totems, bosses resist).
    pub immovable: bool,
    /// Last attacker (kill credit) and the tick of the last damage.
    pub last_hit: Option<EntityId>,
}

impl Actor {
    pub fn new(team: Team, name: &str, level: u32, sheet: Sheet) -> Self {
        Self {
            team,
            name: name.to_string(),
            level,
            rarity: Rarity::Normal,
            family: String::new(),
            life: sheet.life_max,
            mana: sheet.mana_max,
            sheet,
            radius: 0.45,
            base_damage: 0.0,
            speed: 1.0,
            skills: Vec::new(),
            cast: None,
            queued: None,
            cooldowns: Vec::new(),
            combo: 0,
            combo_timer: 0.0,
            iframes: 0.0,
            dodge_cd: 0.0,
            ailments: Ailments::default(),
            buffs: Vec::new(),
            brain: None,
            flash: 0.0,
            dead: false,
            death_t: 0.0,
            xp: 0.0,
            pack: 0,
            affixes: Vec::new(),
            immovable: false,
            last_hit: None,
        }
    }

    pub fn cooldown(&self, skill: u16) -> f32 {
        self.cooldowns.iter().find(|c| c.0 == skill).map(|c| c.1).unwrap_or(0.0)
    }

    pub fn set_cooldown(&mut self, skill: u16, t: f32) {
        match self.cooldowns.iter_mut().find(|c| c.0 == skill) {
            Some(c) => c.1 = t,
            None => self.cooldowns.push((skill, t)),
        }
    }

    pub fn frozen(&self) -> bool {
        self.ailments.freeze > 0.0
    }

    pub fn alive(&self) -> bool {
        !self.dead
    }
}

/// A projectile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shot {
    pub owner: EntityId,
    pub team: Team,
    pub skill: u16,
    pub pos: Vec3,
    pub vel: Vec3,
    pub radius: f32,
    pub life: f32,
    pub dmg: Damage,
    pub pierce: u32,
    pub chain: u32,
    pub explode: f32,
    pub hit: Vec<EntityId>,
    pub color: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    /// An expanding ring (novas, slams, explosions): visual only.
    Ring,
    /// A flash of light and sparks at a point: visual only.
    Burst,
    /// A delayed hit: a warning circle, then damage (meteors, monster slams at a point).
    Delayed,
}

/// Something happening on the ground.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    pub pos: Vec3,
    pub radius: f32,
    pub t: f32,
    pub dur: f32,
    pub color: [f32; 3],
    pub team: Team,
    pub dmg: Option<Damage>,
    /// Facing and arc (degrees; 360 = full ring) for swing arcs.
    pub dir: Vec3,
    pub angle: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloatKind {
    Damage(u8),
    Crit(u8),
    Heal,
    Gold,
    Xp,
    Text,
}

/// A floating number or word over the battlefield.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Floater {
    pub pos: Vec3,
    pub value: f32,
    pub kind: FloatKind,
    pub text: String,
    pub age: f32,
}

/// Final damage to a target after armour, resistances and shock, per element.
pub fn mitigate(d: &Damage, target: &Actor) -> [f32; 5] {
    let mut out = [0.0; 5];
    let shock = 1.0 + target.ailments.shock.0;
    for (i, a) in d.amount.iter().enumerate() {
        if *a <= 0.0 {
            continue;
        }
        let after = if i == 0 {
            // Armour stops a share of physical hits, less against big hits.
            let red = (target.sheet.armor / (target.sheet.armor + 6.0 * a)).clamp(0.0, 0.85);
            a * (1.0 - red)
        } else {
            a * (1.0 - target.sheet.res[i] / 100.0)
        };
        out[i] = after.max(0.0) * shock * target.sheet.taken;
    }
    out
}

/// Monster life and attack damage at a level (before family, rarity and difficulty).
pub fn monster_life(level: u32) -> f32 {
    let l = level as f32;
    14.0 + 5.0 * l + 0.55 * l * l
}

pub fn monster_damage(level: u32) -> f32 {
    let l = level as f32;
    3.0 + 1.4 * l + 0.04 * l * l
}

/// Experience for killing a normal monster of this level.
pub fn monster_xp(level: u32) -> f32 {
    4.0 + 2.2 * (level as f32).powf(1.25)
}

/// Spell damage growth with the caster's level (and skill levels).
pub fn spell_scale(level: f32) -> f32 {
    let l = (level - 1.0).max(0.0);
    1.0 + 0.2 * l + 0.01 * l * l
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arpg::stats::{Base, Mods, Sheet, Stat};

    #[test]
    fn armour_and_resistance_reduce_damage() {
        let base = Base { life: 100.0, mana: 0.0, life_regen: 0.0, mana_regen: 0.0, armor: 0.0, res: 0.0 };
        let sheet = Sheet::compute(base, &Mods::default().with(Stat::Armor, 60.0).with(Stat::FireRes, 50.0));
        let a = Actor::new(Team::Monster, "x", 1, sheet);
        let d = Damage { amount: [10.0, 10.0, 0.0, 0.0, 0.0], ..Default::default() };
        let m = mitigate(&d, &a);
        assert!((m[0] - 5.0).abs() < 1e-3, "armour 60 vs a 10 hit stops half: {}", m[0]);
        assert!((m[1] - 5.0).abs() < 1e-3);
    }

    #[test]
    fn scaling_grows_forever() {
        assert!(monster_life(200) > monster_life(100) * 3.0);
        assert!(monster_damage(1) < 5.0);
        assert!(spell_scale(1.0) == 1.0);
    }
}
