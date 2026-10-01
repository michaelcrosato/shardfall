//! The hero's profile: everything that survives from level to level (and is saved): level and
//! experience, gold, the skill bar, equipment and the passive tree.

use serde::{Deserialize, Serialize};

use super::stats::{Base, Mods, Sheet, Stat};
use crate::puppet::WeaponKind;

/// What the equipped weapon contributes to attacks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponStats {
    pub name: String,
    pub kind: WeaponKind,
    pub phys: [f32; 2],
    /// Attacks per second.
    pub aps: f32,
    /// Base critical strike chance (%).
    pub crit: f32,
    /// Melee reach multiplier.
    pub reach: f32,
}

impl Default for WeaponStats {
    fn default() -> Self {
        Self { name: "Rusted Sword".into(), kind: WeaponKind::Sword, phys: [6.0, 11.0], aps: 1.4, crit: 5.0, reach: 1.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hero {
    pub name: String,
    pub level: u32,
    pub xp: f64,
    pub gold: u64,
    /// Skill bar: skill keys for the slots (mouse left, mouse right, Q, E, R, F).
    pub bar: [String; 6],
    pub weapon: WeaponStats,
    pub potions: u32,
    pub potion_max: u32,
    pub kills: u64,
    pub deaths: u32,
    /// Stat modifiers from gear and passives (recomputed when they change).
    pub mods: Mods,
}

impl Default for Hero {
    fn default() -> Self {
        Self {
            name: "Wanderer".into(),
            level: 1,
            xp: 0.0,
            gold: 0,
            bar: [
                "slash".into(),
                "fireball".into(),
                "cleave".into(),
                "leap_slam".into(),
                "frost_nova".into(),
                "blade_dash".into(),
            ],
            weapon: WeaponStats::default(),
            potions: 3,
            potion_max: 3,
            kills: 0,
            deaths: 0,
            mods: Mods::default(),
        }
    }
}

/// Experience needed to go from `level` to the next.
pub fn xp_to_next(level: u32) -> f64 {
    let l = level as f64;
    30.0 * l.powf(1.9) + 20.0 * l
}

impl Hero {
    pub fn base(&self) -> Base {
        let l = (self.level - 1) as f32;
        Base {
            life: 70.0 + 12.0 * l,
            mana: 50.0 + 4.0 * l,
            life_regen: 1.0 + 0.1 * l,
            mana_regen: 5.0 + 0.15 * l,
            armor: 0.0,
            res: 0.0,
        }
    }

    pub fn sheet(&self) -> Sheet {
        Sheet::compute(self.base(), &self.mods)
    }

    /// Adds experience; returns how many levels were gained.
    pub fn gain_xp(&mut self, xp: f64) -> u32 {
        self.xp += xp;
        let mut ups = 0;
        while self.xp >= xp_to_next(self.level) {
            self.xp -= xp_to_next(self.level);
            self.level += 1;
            ups += 1;
        }
        ups
    }

    /// Skill level of a skill with these tags (hero level based, plus gear).
    pub fn skill_level(&self, sheet: &Sheet, spell: bool, melee: bool) -> f32 {
        self.level as f32
            + sheet.skill_levels * 2.0
            + if spell { sheet.spell_levels * 2.0 } else { 0.0 }
            + if melee { sheet.melee_levels * 2.0 } else { 0.0 }
    }

    pub fn has_stat(&self, s: Stat) -> bool {
        self.mods.get(s) != 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levelling_carries_over() {
        let mut h = Hero::default();
        let ups = h.gain_xp(xp_to_next(1) + xp_to_next(2) + 5.0);
        assert_eq!(ups, 2);
        assert_eq!(h.level, 3);
        assert!((h.xp - 5.0).abs() < 1e-6);
    }
}
