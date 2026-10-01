//! Items: bases (game/items.toml), affixes (game/affixes.toml) and uniques
//! (game/uniques.toml); rolling an item for an item level; what an item does for the hero;
//! names, descriptions and prices.
//!
//! Everything scales with item level and never runs out: base numbers grow with
//! `base_scale`, affixes past their last tier keep growing by their `grow`, unique stats are
//! scaled the same way. A level-300 drop is the same design language as a level-3 one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::combat::Rarity;
use super::powers::Power;
use super::stats::{Mods, Stat, describe};
use crate::puppet::{OffhandKind, WeaponKind};
use crate::rng::Rng;

/// What kind of thing an item is (where it can be worn).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    #[default]
    Weapon,
    Offhand,
    Helmet,
    Body,
    Gloves,
    Boots,
    Belt,
    Amulet,
    Ring,
}

impl Slot {
    pub const ALL: [Slot; 9] =
        [Slot::Weapon, Slot::Offhand, Slot::Helmet, Slot::Body, Slot::Gloves, Slot::Boots, Slot::Belt, Slot::Amulet, Slot::Ring];
    pub fn key(self) -> &'static str {
        ["weapon", "offhand", "helmet", "body", "gloves", "boots", "belt", "amulet", "ring"][self as usize]
    }
    pub fn name(self) -> &'static str {
        ["Weapon", "Off-hand", "Helmet", "Body Armour", "Gloves", "Boots", "Belt", "Amulet", "Ring"][self as usize]
    }
    pub fn is_armor(self) -> bool {
        matches!(self, Slot::Helmet | Slot::Body | Slot::Gloves | Slot::Boots)
    }
}

/// Where an item is worn: ten places on the hero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipSlot {
    #[default]
    Weapon,
    Offhand,
    Helmet,
    Body,
    Gloves,
    Boots,
    Belt,
    Amulet,
    Ring1,
    Ring2,
}

impl EquipSlot {
    pub const ALL: [EquipSlot; 10] = [
        EquipSlot::Weapon,
        EquipSlot::Offhand,
        EquipSlot::Helmet,
        EquipSlot::Body,
        EquipSlot::Gloves,
        EquipSlot::Boots,
        EquipSlot::Belt,
        EquipSlot::Amulet,
        EquipSlot::Ring1,
        EquipSlot::Ring2,
    ];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn from_index(i: usize) -> Option<EquipSlot> {
        Self::ALL.get(i).copied()
    }
    pub fn slot(self) -> Slot {
        match self {
            EquipSlot::Weapon => Slot::Weapon,
            EquipSlot::Offhand => Slot::Offhand,
            EquipSlot::Helmet => Slot::Helmet,
            EquipSlot::Body => Slot::Body,
            EquipSlot::Gloves => Slot::Gloves,
            EquipSlot::Boots => Slot::Boots,
            EquipSlot::Belt => Slot::Belt,
            EquipSlot::Amulet => Slot::Amulet,
            EquipSlot::Ring1 | EquipSlot::Ring2 => Slot::Ring,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            EquipSlot::Ring1 => "Left Ring",
            EquipSlot::Ring2 => "Right Ring",
            s => s.slot().name(),
        }
    }
    pub fn key(self) -> &'static str {
        ["weapon", "offhand", "helmet", "body", "gloves", "boots", "belt", "amulet", "ring1", "ring2"][self as usize]
    }
    pub fn from_key(k: &str) -> Option<EquipSlot> {
        Self::ALL.iter().copied().find(|s| s.key() == k || (k == "ring" && *s == EquipSlot::Ring1))
    }
}

/// An item base (game/items.toml).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BaseDef {
    #[serde(skip_deserializing)]
    pub key: String,
    pub name: String,
    pub slot: Slot,
    pub kind: String,
    pub level: u32,
    pub phys: [f32; 2],
    pub aps: f32,
    pub crit: f32,
    pub reach: f32,
    pub two_hand: bool,
    pub armor: f32,
    pub implicit: BTreeMap<String, f32>,
    pub color: String,
    pub glow: f32,
    pub look: String,
    /// Resolved at load.
    #[serde(skip)]
    pub implicits: Vec<(Stat, f32)>,
}

impl Default for BaseDef {
    fn default() -> Self {
        Self {
            key: String::new(),
            name: String::new(),
            slot: Slot::Weapon,
            kind: String::new(),
            level: 1,
            phys: [0.0, 0.0],
            aps: 1.2,
            crit: 5.0,
            reach: 1.0,
            two_hand: false,
            armor: 0.0,
            implicit: BTreeMap::new(),
            color: "#c9ced8".into(),
            glow: 0.0,
            look: String::new(),
            implicits: Vec::new(),
        }
    }
}

impl BaseDef {
    pub fn weapon_kind(&self) -> WeaponKind {
        use crate::params::ChoiceParam;
        WeaponKind::NAMES.iter().position(|n| *n == self.kind).map(WeaponKind::from_index).unwrap_or(WeaponKind::None)
    }
    pub fn offhand_kind(&self) -> OffhandKind {
        match self.kind.as_str() {
            "shield" => OffhandKind::Shield,
            "focus" => OffhandKind::Focus,
            _ => OffhandKind::None,
        }
    }
    /// The words an affix's `slots` list can name to include this base.
    pub fn matches(&self, slots: &[String]) -> bool {
        slots.iter().any(|s| s == self.slot.key() || s == &self.kind || (s == "armor" && self.slot.is_armor()))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AffixKind {
    #[default]
    Prefix,
    Suffix,
}

/// An affix (game/affixes.toml).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AffixDef {
    #[serde(skip_deserializing)]
    pub key: String,
    pub kind: AffixKind,
    pub stat: String,
    pub names: Vec<String>,
    pub slots: Vec<String>,
    /// [item level, min, max] per tier.
    pub tiers: Vec<[f32; 3]>,
    pub grow: f32,
    pub weight: u32,
    pub local: bool,
    #[serde(skip)]
    pub stat_id: Option<Stat>,
}

impl Default for AffixDef {
    fn default() -> Self {
        Self {
            key: String::new(),
            kind: AffixKind::Prefix,
            stat: String::new(),
            names: Vec::new(),
            slots: Vec::new(),
            tiers: Vec::new(),
            grow: 0.0,
            weight: 100,
            local: false,
            stat_id: None,
        }
    }
}

impl AffixDef {
    /// Tiers this item level reaches (indices).
    pub fn reached(&self, ilvl: u32) -> usize {
        self.tiers.iter().filter(|t| t[0] as u32 <= ilvl).count()
    }
    /// Value range of a tier at an item level (growing past the last tier).
    pub fn range(&self, tier: usize, ilvl: u32) -> (f32, f32) {
        let t = self.tiers[tier.min(self.tiers.len() - 1)];
        let last = self.tiers.last().map(|t| t[0]).unwrap_or(1.0);
        let k = if tier + 1 == self.tiers.len() { 1.0 + self.grow * (ilvl as f32 - last).max(0.0) } else { 1.0 };
        (t[1] * k, t[2] * k)
    }
    pub fn tier_name(&self, tier: usize) -> &str {
        self.names.get(tier).or(self.names.last()).map(String::as_str).unwrap_or("")
    }
}

/// A unique item (game/uniques.toml).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UniqueDef {
    #[serde(skip_deserializing)]
    pub key: String,
    pub name: String,
    pub base: String,
    pub level: u32,
    pub mods: BTreeMap<String, [f32; 2]>,
    pub power: Option<Power>,
    pub lore: String,
    #[serde(skip)]
    pub stats: Vec<(Stat, [f32; 2])>,
}

impl Default for UniqueDef {
    fn default() -> Self {
        Self {
            key: String::new(),
            name: String::new(),
            base: String::new(),
            level: 1,
            mods: BTreeMap::new(),
            power: None,
            lore: String::new(),
            stats: Vec::new(),
        }
    }
}

/// One rolled modifier on an item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rolled {
    pub stat: Stat,
    pub value: f32,
    /// Affix key (empty on uniques) and tier (0 = lowest).
    #[serde(default)]
    pub affix: String,
    #[serde(default)]
    pub tier: u8,
    #[serde(default)]
    pub local: bool,
    #[serde(default)]
    pub prefix: bool,
}

/// An item: a base, an item level, a rarity and rolled modifiers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: u32,
    pub base: String,
    pub level: u32,
    pub rarity: Rarity,
    pub name: String,
    #[serde(default)]
    pub mods: Vec<Rolled>,
    /// Unique key (empty for other items).
    #[serde(default)]
    pub unique: String,
}

/// How much base numbers have grown at an item level (1 at level 1, ~45 at level 75).
pub fn base_scale(level: u32) -> f32 {
    let l = (level.max(1) - 1) as f32;
    1.0 + 0.15 * l + 0.006 * l * l
}

/// Growth of flat implicit stats with item level.
pub fn flat_scale(level: u32) -> f32 {
    1.0 + 0.04 * (level.max(1) - 1) as f32
}

/// Stats whose numbers are flat amounts (they grow with item level); the rest are
/// percentages and counts (they don't).
pub fn is_flat(s: Stat) -> bool {
    use Stat::*;
    matches!(
        s,
        Strength
            | Dexterity
            | Intelligence
            | AllAttributes
            | Life
            | Mana
            | LifeRegen
            | LifeOnHit
            | ManaOnHit
            | LifeOnKill
            | AddedPhys
            | AddedFire
            | AddedCold
            | AddedLightning
            | AddedPoison
            | AddedSpell
            | Armor
            | Thorns
    )
}

/// What an item does: stats for the hero, plus its own weapon or armour numbers.
#[derive(Clone, Debug, Default)]
pub struct ItemStats {
    pub mods: Mods,
    /// Weapons: physical damage, attacks per second, crit chance, reach.
    pub phys: [f32; 2],
    pub aps: f32,
    pub crit: f32,
    pub reach: f32,
    /// Armour pieces and shields.
    pub armor: f32,
    pub power: Option<Power>,
}

fn round_value(v: f32, hi: f32) -> f32 {
    if hi.abs() < 5.0 { (v * 10.0).round() / 10.0 } else { v.round() }
}

impl Item {
    /// A plain (normal) item of a base.
    pub fn plain(id: u32, base: &BaseDef, level: u32) -> Item {
        Item {
            id,
            base: base.key.clone(),
            level,
            rarity: Rarity::Normal,
            name: base.name.clone(),
            mods: Vec::new(),
            unique: String::new(),
        }
    }

    pub fn base_def<'a>(&self, d: &'a super::data::Data) -> Option<&'a BaseDef> {
        d.base(&self.base)
    }

    pub fn unique_def<'a>(&self, d: &'a super::data::Data) -> Option<&'a UniqueDef> {
        if self.unique.is_empty() { None } else { d.unique(&self.unique) }
    }

    pub fn slot(&self, d: &super::data::Data) -> Slot {
        self.base_def(d).map(|b| b.slot).unwrap_or_default()
    }

    /// Implicit stats of the base at this item level.
    pub fn implicits(&self, d: &super::data::Data) -> Vec<(Stat, f32)> {
        let Some(b) = self.base_def(d) else { return Vec::new() };
        b.implicits
            .iter()
            .map(|(s, v)| {
                let v = if is_flat(*s) { round_value(v * flat_scale(self.level), *v * 10.0) } else { *v };
                (*s, v)
            })
            .collect()
    }

    /// Everything the item does.
    pub fn stats(&self, d: &super::data::Data) -> ItemStats {
        let mut out = ItemStats::default();
        let Some(b) = self.base_def(d) else { return out };
        for (s, v) in self.implicits(d) {
            out.mods.add(s, v);
        }
        let k = base_scale(self.level);
        let mut phys = [b.phys[0] * k, b.phys[1] * k];
        let (mut phys_inc, mut aps_inc, mut crit_inc, mut armor_inc) = (0.0, 0.0, 0.0, 0.0);
        let mut armor = b.armor * k.sqrt() * flat_scale(self.level).sqrt();
        for m in &self.mods {
            if m.local {
                match m.stat {
                    Stat::AddedPhys => {
                        phys[0] += m.value * 0.7;
                        phys[1] += m.value * 1.3;
                    }
                    Stat::PhysInc => phys_inc += m.value,
                    Stat::AttackSpeed => aps_inc += m.value,
                    Stat::CritChance => crit_inc += m.value,
                    Stat::Armor => armor += m.value,
                    Stat::ArmorInc => armor_inc += m.value,
                    s => out.mods.add(s, m.value),
                }
            } else {
                out.mods.add(m.stat, m.value);
            }
        }
        if b.slot == Slot::Weapon {
            out.phys = [phys[0] * (1.0 + phys_inc / 100.0), phys[1] * (1.0 + phys_inc / 100.0)];
            out.aps = b.aps * (1.0 + aps_inc / 100.0);
            out.crit = b.crit * (1.0 + crit_inc / 100.0);
            out.reach = b.reach;
        }
        if armor > 0.0 {
            out.armor = (armor * (1.0 + armor_inc / 100.0)).round();
            out.mods.add(Stat::Armor, out.armor);
        }
        out.power = self.unique_def(d).and_then(|u| u.power);
        out
    }

    /// Damage per second of a weapon (physical, before the hero's modifiers).
    pub fn dps(&self, d: &super::data::Data) -> f32 {
        let s = self.stats(d);
        (s.phys[0] + s.phys[1]) * 0.5 * s.aps
    }

    /// Lines for a tooltip: header numbers, implicits, then modifiers (and the power).
    pub fn describe(&self, d: &super::data::Data) -> ItemText {
        let mut t = ItemText { name: self.name.clone(), ..Default::default() };
        let Some(b) = self.base_def(d) else {
            t.base = format!("unknown base '{}'", self.base);
            return t;
        };
        let s = self.stats(d);
        t.base = if self.name != b.name { b.name.clone() } else { String::new() };
        let hand = if b.slot == Slot::Weapon { if b.two_hand { "Two-Handed " } else { "One-Handed " } } else { "" };
        let kind = if b.slot == Slot::Weapon || b.slot == Slot::Offhand { cap(&b.kind) } else { b.slot.name().to_string() };
        t.kind = format!("{} {hand}{kind}", ["Normal", "Magic", "Rare", "Unique"][self.rarity as usize]);
        t.level = self.level;
        if b.slot == Slot::Weapon {
            t.header.push(format!("Physical Damage: {:.0}-{:.0}", s.phys[0], s.phys[1]));
            t.header.push(format!("Attacks per Second: {:.2}", s.aps));
            t.header.push(format!("Critical Strike Chance: {:.1}%", s.crit));
            t.header.push(format!("Damage per Second: {:.1}", (s.phys[0] + s.phys[1]) * 0.5 * s.aps));
        }
        if s.armor > 0.0 {
            t.header.push(format!("Armour: {:.0}", s.armor));
        }
        for (st, v) in self.implicits(d) {
            t.implicit.push(describe(st, v));
        }
        for m in &self.mods {
            t.mods.push((describe(m.stat, m.value), m.tier, m.local));
        }
        if let Some(u) = self.unique_def(d) {
            if let Some(p) = &u.power {
                t.power = p.describe();
            }
            t.lore = u.lore.clone();
        }
        t
    }

    /// A rough worth for comparing items in the same slot (bots and the "upgrade" frame):
    /// weapon damage counts most, then stats, armour and powers.
    pub fn score(&self, d: &super::data::Data) -> f32 {
        let s = self.stats(d);
        let dps = (s.phys[0] + s.phys[1]) * 0.5 * s.aps;
        s.mods.0.iter().map(|(k, v)| if *k == Stat::Armor { 0.0 } else { *v }).sum::<f32>() * 0.4
            + dps * 2.0
            + s.armor * 0.5
            + if s.power.is_some() { 50.0 } else { 0.0 }
    }

    /// Gold value (what vendors pay; they charge more).
    pub fn value(&self) -> u64 {
        let r = [1.0, 2.5, 6.0, 18.0][self.rarity as usize];
        ((6.0 + self.level as f32 * 3.0) * r * (1.0 + 0.08 * self.mods.len() as f32)).round() as u64
    }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// An item's tooltip text.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ItemText {
    pub name: String,
    /// The base name when the item has its own name.
    pub base: String,
    pub kind: String,
    pub level: u32,
    pub header: Vec<String>,
    pub implicit: Vec<String>,
    /// (line, tier, local)
    pub mods: Vec<(String, u8, bool)>,
    pub power: String,
    pub lore: String,
}

// ------------------------------------------------------------------------------- rolling

const RARE_FIRST: &[&str] = &[
    "Doom", "Grim", "Storm", "Blood", "Bone", "Dread", "Rune", "Soul", "Star", "Ash", "Ember", "Frost", "Gloom", "Hate", "Iron",
    "Night", "Raven", "Shadow", "Skull", "Spirit", "Viper", "Wraith", "Dusk", "Corpse", "Glyph", "Havoc", "Pain", "Rage",
    "Sorrow", "Tempest", "Vortex", "Woe", "Kraken", "Dragon", "Eagle", "Gale", "Hollow", "Mind", "Plague",
];

fn rare_second(slot: Slot) -> &'static [&'static str] {
    match slot {
        Slot::Weapon => &["Bite", "Edge", "Fang", "Song", "Thirst", "Reaver", "Gnash", "Scar", "Sever", "Wound", "Hunger"],
        Slot::Offhand => &["Guard", "Ward", "Wall", "Lamp", "Beacon", "Bulwark", "Mirror", "Shelter"],
        Slot::Helmet => &["Crown", "Visage", "Brow", "Cowl", "Dome", "Halo", "Peak", "Veil"],
        Slot::Body => &["Shell", "Coat", "Hide", "Mantle", "Carapace", "Pelt", "Wrap", "Husk"],
        Slot::Gloves => &["Grasp", "Clutch", "Fist", "Paw", "Hold", "Grip", "Knuckle", "Touch"],
        Slot::Boots => &["Stride", "Track", "Trail", "March", "Spur", "Tread", "Road", "Pace"],
        Slot::Belt => &["Lash", "Cord", "Coil", "Strap", "Clasp", "Girdle", "Bind", "Buckle"],
        Slot::Amulet => &["Charm", "Heart", "Locket", "Eye", "Talisman", "Beads", "Collar", "Torc"],
        Slot::Ring => &["Loop", "Spiral", "Band", "Circle", "Knot", "Turn", "Whorl", "Grip"],
    }
}

/// How an item rolls: the item level, a forced rarity or slot, and a rarity bonus (the
/// killer's item rarity and the monster's rank).
#[derive(Clone, Copy, Debug, Default)]
pub struct RollSpec {
    pub level: u32,
    pub rarity: Option<Rarity>,
    pub slot: Option<Slot>,
    /// Percent more chance of better rarities.
    pub rarity_bonus: f32,
}

/// Picks a rarity: normal 70, magic 24, rare 5.5, unique 0.5 (better with the bonus).
pub fn roll_rarity(rng: &mut Rng, bonus: f32) -> Rarity {
    let k = 1.0 + bonus.max(-90.0) / 100.0;
    let w = [70.0, 24.0 * k, 5.5 * k * k.sqrt(), 0.5 * k * k];
    let total: f32 = w.iter().sum();
    let mut r = rng.f32() * total;
    for (i, x) in w.iter().enumerate() {
        if r < *x {
            return [Rarity::Normal, Rarity::Magic, Rarity::Rare, Rarity::Unique][i];
        }
        r -= x;
    }
    Rarity::Normal
}

fn pick_weighted<T>(rng: &mut Rng, items: &[(T, f32)]) -> Option<usize> {
    let total: f32 = items.iter().map(|x| x.1).sum();
    if total <= 0.0 {
        return None;
    }
    let mut r = rng.f32() * total;
    for (i, x) in items.iter().enumerate() {
        if r < x.1 {
            return Some(i);
        }
        r -= x.1;
    }
    Some(items.len() - 1)
}

/// A base for an item level: recent tiers are the most common.
pub fn roll_base<'a>(d: &'a super::data::Data, rng: &mut Rng, level: u32, slot: Option<Slot>) -> Option<&'a BaseDef> {
    let eligible = |b: &&BaseDef| b.level <= level.max(1) && slot.is_none_or(|s| s == b.slot);
    // Each slot gets its share of drops however many bases it has; within a slot, recent
    // tiers are the most common.
    let share = |s: Slot| match s {
        Slot::Weapon => 0.2,
        Slot::Ring => 0.11,
        Slot::Amulet => 0.07,
        Slot::Offhand | Slot::Belt => 0.08,
        _ => 0.09,
    };
    let mut weight_sum = [0.0f32; 9];
    let recency = |b: &BaseDef| 1.0 / (1.0 + (level - b.level.min(level)) as f32 / 8.0);
    for b in d.bases.iter().filter(eligible) {
        weight_sum[b.slot as usize] += recency(b);
    }
    let cands: Vec<(&BaseDef, f32)> = d
        .bases
        .iter()
        .filter(eligible)
        .map(|b| (b, share(b.slot) * recency(b) / weight_sum[b.slot as usize].max(1e-6)))
        .collect();
    pick_weighted(rng, &cands).map(|i| cands[i].0)
}

/// Rolls one affix value into the item (none if nothing fits).
fn roll_affix(d: &super::data::Data, rng: &mut Rng, item: &mut Item, base: &BaseDef, kind: AffixKind) -> Option<usize> {
    let taken: Vec<&str> = item.mods.iter().map(|m| m.affix.as_str()).collect();
    let taken_stats: Vec<(Stat, bool)> = item.mods.iter().map(|m| (m.stat, m.local)).collect();
    let cands: Vec<(usize, f32)> = d
        .affixes
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            a.kind == kind
                && a.reached(item.level) > 0
                && base.matches(&a.slots)
                && !taken.contains(&a.key.as_str())
                && a.stat_id.is_some_and(|s| !taken_stats.contains(&(s, a.local)))
        })
        .map(|(i, a)| (i, a.weight as f32))
        .collect();
    let i = cands[pick_weighted(rng, &cands)?].0;
    let a = &d.affixes[i];
    // The best reached tier half the time, then lower ones.
    let top = a.reached(item.level) - 1;
    let r = rng.f32();
    let drop = if r < 0.5 {
        0
    } else if r < 0.8 {
        1
    } else {
        2
    };
    let tier = top.saturating_sub(drop);
    let (lo, hi) = a.range(tier, item.level);
    let v = round_value(rng.range(lo, hi.max(lo)), hi);
    item.mods.push(Rolled {
        stat: a.stat_id.unwrap(),
        value: v,
        affix: a.key.clone(),
        tier: tier as u8,
        local: a.local,
        prefix: kind == AffixKind::Prefix,
    });
    Some(i)
}

/// Rolls an item. `id` is the item's id in the game.
pub fn roll_item(d: &super::data::Data, rng: &mut Rng, spec: RollSpec, id: u32) -> Option<Item> {
    let level = spec.level.max(1);
    let mut rarity = spec.rarity.unwrap_or_else(|| roll_rarity(rng, spec.rarity_bonus));
    if rarity == Rarity::Unique {
        let cands: Vec<(&UniqueDef, f32)> = d
            .uniques
            .iter()
            .filter(|u| u.level <= level && spec.slot.is_none_or(|s| d.base(&u.base).is_some_and(|b| b.slot == s)))
            .map(|u| (u, 1.0))
            .collect();
        match pick_weighted(rng, &cands) {
            Some(i) => return Some(unique_item(d, rng, cands[i].0, level, id)),
            None => rarity = Rarity::Rare,
        }
    }
    let base = roll_base(d, rng, level, spec.slot)?;
    let mut item = Item::plain(id, base, level);
    item.rarity = rarity;
    match rarity {
        Rarity::Magic => {
            let n = 1 + rng.below(2);
            let first = if rng.f32() < 0.5 { AffixKind::Prefix } else { AffixKind::Suffix };
            let mut parts = (None, None);
            for k in 0..n {
                let kind = if k == 0 {
                    first
                } else if first == AffixKind::Prefix {
                    AffixKind::Suffix
                } else {
                    AffixKind::Prefix
                };
                if let Some(i) = roll_affix(d, rng, &mut item, base, kind) {
                    let t = item.mods.last().unwrap().tier as usize;
                    let name = d.affixes[i].tier_name(t).to_string();
                    if kind == AffixKind::Prefix {
                        parts.0 = Some(name);
                    } else {
                        parts.1 = Some(name);
                    }
                }
            }
            item.name = match parts {
                (Some(p), Some(s)) => format!("{p} {} {s}", base.name),
                (Some(p), None) => format!("{p} {}", base.name),
                (None, Some(s)) => format!("{} {s}", base.name),
                (None, None) => base.name.clone(),
            };
        }
        Rarity::Rare => {
            let n = 3 + rng.below(4);
            let (mut pre, mut suf) = (0, 0);
            for _ in 0..n {
                // Prefix or suffix at random, until one side has three.
                let kind = if suf >= 3 || (pre < 3 && rng.f32() < 0.5) { AffixKind::Prefix } else { AffixKind::Suffix };
                if roll_affix(d, rng, &mut item, base, kind).is_some() {
                    if kind == AffixKind::Prefix {
                        pre += 1;
                    } else {
                        suf += 1;
                    }
                }
            }
            let a = RARE_FIRST[rng.below(RARE_FIRST.len() as u32) as usize];
            let second = rare_second(base.slot);
            let b = second[rng.below(second.len() as u32) as usize];
            item.name = format!("{a} {b}");
        }
        _ => {}
    }
    // Prefixes first, then suffixes (how they read on the tooltip).
    item.mods.sort_by_key(|m| !m.prefix);
    Some(item)
}

/// A unique item at an item level.
pub fn unique_item(d: &super::data::Data, rng: &mut Rng, u: &UniqueDef, level: u32, id: u32) -> Item {
    let level = level.max(u.level);
    let grow = 1.0 + 0.012 * (level - u.level) as f32;
    let mods = u
        .stats
        .iter()
        .map(|(s, r)| {
            let k = if is_flat(*s) { grow } else { 1.0 };
            let v = round_value(rng.range(r[0], r[1].max(r[0])) * k, r[1] * k);
            Rolled {
                stat: *s,
                value: v,
                affix: String::new(),
                tier: 0,
                local: matches!(s, Stat::PhysInc | Stat::ArmorInc) && is_local_slot(d, &u.base, *s),
                prefix: true,
            }
        })
        .collect();
    Item { id, base: u.base.clone(), level, rarity: Rarity::Unique, name: u.name.clone(), mods, unique: u.key.clone() }
}

/// Unique weapons' physical damage and armour pieces' armour work on the item itself.
fn is_local_slot(d: &super::data::Data, base: &str, s: Stat) -> bool {
    match d.base(base).map(|b| b.slot) {
        Some(Slot::Weapon) => s == Stat::PhysInc,
        Some(slot) => s == Stat::ArmorInc && (slot.is_armor() || slot == Slot::Offhand),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arpg::data::load;

    #[test]
    fn items_roll_at_every_level_and_scale() {
        let d = load().unwrap();
        let mut rng = Rng::new(7);
        let mut seen = [0; 4];
        for (i, level) in [1u32, 5, 20, 60, 75, 120, 300].iter().cycle().take(700).enumerate() {
            let it =
                roll_item(&d, &mut rng, RollSpec { level: *level, rarity_bonus: 200.0, ..Default::default() }, i as u32).unwrap();
            seen[it.rarity as usize] += 1;
            let b = it.base_def(&d).unwrap();
            assert!(b.level <= it.level.max(1), "{} at {}", b.key, it.level);
            match it.rarity {
                Rarity::Magic => assert!((1..=2).contains(&it.mods.len()), "{:?}", it),
                Rarity::Rare => assert!((3..=6).contains(&it.mods.len()), "{:?}", it),
                _ => {}
            }
            let text = it.describe(&d);
            assert!(!text.name.is_empty());
            for m in &it.mods {
                assert!(m.value.is_finite() && m.value != 0.0, "{:?}", m);
            }
        }
        assert!(seen.iter().all(|n| *n > 0), "all rarities seen: {seen:?}");
        // A sword of level 100 hits much harder than one of level 1.
        let b = d.base("rusted_sword").unwrap();
        let lo = Item::plain(1, b, 1).dps(&d);
        let hi = Item::plain(2, b, 100).dps(&d);
        assert!(hi > lo * 30.0, "{lo} -> {hi}");
    }

    #[test]
    fn local_affixes_change_the_item() {
        let d = load().unwrap();
        let b = d.base("rusted_sword").unwrap();
        let mut it = Item::plain(1, b, 10);
        let before = it.stats(&d);
        it.mods.push(Rolled {
            stat: Stat::PhysInc,
            value: 100.0,
            affix: "weapon_phys_pct".into(),
            tier: 0,
            local: true,
            prefix: true,
        });
        let after = it.stats(&d);
        assert!((after.phys[1] - before.phys[1] * 2.0).abs() < 1e-3);
        assert_eq!(after.mods.get(Stat::PhysInc), 0.0, "local: not a hero stat");
    }
}
