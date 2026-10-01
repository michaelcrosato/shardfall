//! The hero's profile: everything that survives from level to level (and is saved): level and
//! experience, gold, the skill bar, equipment and the passive tree.

use serde::{Deserialize, Serialize};

use super::combat::Rarity;
use super::data::Data;
use super::items::{EquipSlot, Item, Slot};
use super::powers::Power;
use super::stats::{Base, Mods, Sheet, Stat};
use crate::puppet::WeaponKind;

/// Bag and stash sizes.
pub const INVENTORY_SIZE: usize = 40;
pub const STASH_SIZE: usize = 120;

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

impl WeaponStats {
    /// Bare hands.
    pub fn fists() -> Self {
        Self { name: "Fists".into(), kind: WeaponKind::None, phys: [2.0, 5.0], aps: 1.6, crit: 4.0, reach: 0.8 }
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
    /// Extra stat modifiers (tools, tests, blessings); gear and passives add theirs on top.
    pub mods: Mods,
    /// Worn items, by `EquipSlot` index.
    pub equipment: [Option<Item>; 10],
    pub inventory: Vec<Item>,
    pub stash: Vec<Item>,
    /// Next item id.
    pub next_item: u32,
    /// Powers from gear (recomputed with the sheet).
    pub powers: Vec<Power>,
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
            equipment: [
                Some(Item {
                    id: 1,
                    base: "rusted_sword".into(),
                    level: 1,
                    rarity: Rarity::Normal,
                    name: "Rusted Sword".into(),
                    mods: Vec::new(),
                    unique: String::new(),
                }),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            inventory: Vec::new(),
            stash: Vec::new(),
            next_item: 2,
            powers: Vec::new(),
        }
    }
}

/// What the hero's gear adds up to.
#[derive(Clone, Debug, Default)]
pub struct Gear {
    pub mods: Mods,
    pub weapon: WeaponStats,
    pub powers: Vec<Power>,
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

    pub fn worn(&self, s: EquipSlot) -> Option<&Item> {
        self.equipment[s.index()].as_ref()
    }

    /// Sums the worn items: stats, the weapon in hand and powers.
    pub fn gear(&self, d: &Data) -> Gear {
        let mut g = Gear { weapon: WeaponStats::fists(), ..Default::default() };
        for (i, it) in self.equipment.iter().enumerate() {
            let Some(it) = it else { continue };
            let st = it.stats(d);
            g.mods.merge(&st.mods);
            if let Some(p) = st.power {
                g.powers.push(p);
            }
            if i == EquipSlot::Weapon.index() {
                if let Some(b) = it.base_def(d) {
                    g.weapon = WeaponStats {
                        name: it.name.clone(),
                        kind: b.weapon_kind(),
                        phys: st.phys,
                        aps: st.aps,
                        crit: st.crit,
                        reach: st.reach,
                    };
                }
            }
        }
        g
    }

    /// A new item id.
    pub fn new_id(&mut self) -> u32 {
        let id = self.next_item.max(1);
        self.next_item = id + 1;
        id
    }

    pub fn bag_full(&self) -> bool {
        self.inventory.len() >= INVENTORY_SIZE
    }

    /// Where an item would go when equipped (rings: the empty hand first).
    pub fn slot_for(&self, d: &Data, item: &Item) -> EquipSlot {
        match item.slot(d) {
            Slot::Ring => {
                if self.equipment[EquipSlot::Ring1.index()].is_none() || self.equipment[EquipSlot::Ring2.index()].is_some() {
                    EquipSlot::Ring1
                } else {
                    EquipSlot::Ring2
                }
            }
            s => EquipSlot::ALL.iter().copied().find(|e| e.slot() == s).unwrap_or(EquipSlot::Weapon),
        }
    }

    /// Wears an item from the bag. Returns an error message when it can't.
    pub fn equip(&mut self, d: &Data, id: u32, to: Option<EquipSlot>) -> Result<(), String> {
        let i = self.inventory.iter().position(|it| it.id == id).ok_or("no such item in the bag")?;
        let slot = to.unwrap_or_else(|| self.slot_for(d, &self.inventory[i]));
        if slot.slot() != self.inventory[i].slot(d) {
            return Err(format!("{} can't be worn as {}", self.inventory[i].name, slot.name()));
        }
        let two_hand = |it: &Item| it.base_def(d).is_some_and(|b| b.two_hand);
        // Two-handed weapons and off-hands exclude each other: the other one goes to the bag.
        let mut also = None;
        if slot == EquipSlot::Weapon && two_hand(&self.inventory[i]) {
            also = Some(EquipSlot::Offhand);
        }
        if slot == EquipSlot::Offhand && self.worn(EquipSlot::Weapon).is_some_and(two_hand) {
            also = Some(EquipSlot::Weapon);
        }
        let freed =
            self.equipment[slot.index()].is_some() as usize + also.is_some_and(|s| self.equipment[s.index()].is_some()) as usize;
        if self.inventory.len() - 1 + freed > INVENTORY_SIZE {
            return Err("Inventory full".into());
        }
        let item = self.inventory.remove(i);
        if let Some(old) = self.equipment[slot.index()].replace(item) {
            self.inventory.insert(i.min(self.inventory.len()), old);
        }
        if let Some(o) = also {
            if let Some(old) = self.equipment[o.index()].take() {
                self.inventory.push(old);
            }
        }
        Ok(())
    }

    /// Takes off an item into the bag.
    pub fn unequip(&mut self, slot: EquipSlot) -> Result<(), String> {
        if self.equipment[slot.index()].is_none() {
            return Err("nothing worn there".into());
        }
        if self.bag_full() {
            return Err("Inventory full".into());
        }
        let it = self.equipment[slot.index()].take().unwrap();
        self.inventory.push(it);
        Ok(())
    }

    /// Finds an item anywhere on the hero (bag, worn, stash).
    pub fn find(&self, id: u32) -> Option<&Item> {
        self.inventory.iter().chain(self.equipment.iter().flatten()).chain(self.stash.iter()).find(|i| i.id == id)
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
