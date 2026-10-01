//! Stats: every number a build can change. Items, passive nodes, buffs and monster affixes all
//! add `Mods` (stat -> value); `Sheet::compute` turns base values plus mods into the numbers
//! combat reads. A stat's key is its name in data files; its template is how it reads on an
//! item ("{}" is the value).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

macro_rules! stats {
    ($($v:ident => $key:literal, $fmt:literal;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum Stat { $($v),* }
        impl Stat {
            pub const ALL: &'static [Stat] = &[$(Stat::$v),*];
            pub fn key(self) -> &'static str { match self { $(Stat::$v => $key),* } }
            /// How the stat reads: `{}` is the value.
            pub fn template(self) -> &'static str { match self { $(Stat::$v => $fmt),* } }
            pub fn from_key(k: &str) -> Option<Stat> { Self::ALL.iter().copied().find(|s| s.key() == k) }
        }
    };
}

stats! {
    Strength => "strength", "+{} to Strength";
    Dexterity => "dexterity", "+{} to Dexterity";
    Intelligence => "intelligence", "+{} to Intelligence";
    AllAttributes => "all_attributes", "+{} to all Attributes";
    Life => "life", "+{} to maximum Life";
    LifeInc => "life_inc", "{}% increased maximum Life";
    LifeRegen => "life_regen", "Regenerate {} Life per second";
    LifeOnHit => "life_on_hit", "Gain {} Life per enemy hit";
    LifeLeech => "life_leech", "{}% of damage dealt is leeched as Life";
    LifeOnKill => "life_on_kill", "Gain {} Life on kill";
    Mana => "mana", "+{} to maximum Mana";
    ManaInc => "mana_inc", "{}% increased maximum Mana";
    ManaRegen => "mana_regen", "{}% increased Mana Regeneration";
    ManaOnHit => "mana_on_hit", "Gain {} Mana per enemy hit";
    ManaCost => "mana_cost", "{}% reduced Mana cost of Skills";
    AddedPhys => "added_phys", "Adds {} Physical Damage to Attacks";
    AddedFire => "added_fire", "Adds {} Fire Damage to Attacks";
    AddedCold => "added_cold", "Adds {} Cold Damage to Attacks";
    AddedLightning => "added_lightning", "Adds {} Lightning Damage to Attacks";
    AddedPoison => "added_poison", "Adds {} Poison Damage to Attacks";
    AddedSpell => "added_spell", "Adds {} Damage to Spells";
    DamageInc => "damage_inc", "{}% increased Damage";
    PhysInc => "phys_inc", "{}% increased Physical Damage";
    FireInc => "fire_inc", "{}% increased Fire Damage";
    ColdInc => "cold_inc", "{}% increased Cold Damage";
    LightningInc => "lightning_inc", "{}% increased Lightning Damage";
    PoisonInc => "poison_inc", "{}% increased Poison Damage";
    ElementalInc => "elemental_inc", "{}% increased Elemental Damage";
    AttackInc => "attack_inc", "{}% increased Attack Damage";
    SpellInc => "spell_inc", "{}% increased Spell Damage";
    MeleeInc => "melee_inc", "{}% increased Melee Damage";
    ProjectileInc => "projectile_inc", "{}% increased Projectile Damage";
    AreaDamageInc => "area_damage_inc", "{}% increased Area Damage";
    DotInc => "dot_inc", "{}% increased Damage over Time";
    MinionInc => "minion_inc", "Minions deal {}% increased Damage";
    DamageMore => "damage_more", "{}% more Damage";
    AttackSpeed => "attack_speed", "{}% increased Attack Speed";
    CastSpeed => "cast_speed", "{}% increased Cast Speed";
    MoveSpeed => "move_speed", "{}% increased Movement Speed";
    CooldownRecovery => "cooldown_recovery", "{}% faster Cooldown Recovery";
    CritChance => "crit_chance", "{}% increased Critical Strike Chance";
    CritChanceFlat => "crit_chance_flat", "+{}% to Critical Strike Chance";
    CritMulti => "crit_multi", "+{}% to Critical Strike Multiplier";
    AreaInc => "area_inc", "{}% increased Area of Effect";
    ProjectileCount => "projectile_count", "+{} Projectiles";
    ProjectileSpeed => "projectile_speed", "{}% increased Projectile Speed";
    Pierce => "pierce", "Projectiles Pierce {} additional enemies";
    Chain => "chain", "Skills Chain +{} times";
    DurationInc => "duration_inc", "{}% increased Skill Effect Duration";
    Armor => "armor", "+{} to Armour";
    ArmorInc => "armor_inc", "{}% increased Armour";
    Evasion => "evasion", "{}% chance to Evade Attacks";
    Block => "block", "{}% chance to Block Damage";
    FireRes => "fire_res", "+{}% to Fire Resistance";
    ColdRes => "cold_res", "+{}% to Cold Resistance";
    LightningRes => "lightning_res", "+{}% to Lightning Resistance";
    PoisonRes => "poison_res", "+{}% to Poison Resistance";
    AllRes => "all_res", "+{}% to all Elemental Resistances";
    DamageTaken => "damage_taken", "{}% reduced Damage taken";
    Thorns => "thorns", "Reflects {} Damage to Melee Attackers";
    BleedChance => "bleed_chance", "{}% chance to cause Bleeding";
    IgniteChance => "ignite_chance", "{}% chance to Ignite";
    FreezeChance => "freeze_chance", "{}% chance to Freeze";
    ShockChance => "shock_chance", "{}% chance to Shock";
    PoisonChance => "poison_chance", "{}% chance to Poison";
    AilmentInc => "ailment_inc", "{}% increased Damage with Ailments";
    ItemRarity => "item_rarity", "{}% increased Rarity of Items found";
    GoldFind => "gold_find", "{}% increased Gold found";
    XpGain => "xp_gain", "{}% increased Experience gained";
    DodgeRecovery => "dodge_recovery", "{}% faster Dodge recovery";
    PotionInc => "potion_inc", "{}% increased Potion effect";
    SkillLevels => "skill_levels", "+{} to Level of all Skills";
    MeleeLevels => "melee_levels", "+{} to Level of Melee Skills";
    SpellLevels => "spell_levels", "+{} to Level of Spell Skills";
}

/// A bag of stat modifiers (values add up).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mods(pub BTreeMap<Stat, f32>);

impl Mods {
    pub fn add(&mut self, s: Stat, v: f32) {
        *self.0.entry(s).or_insert(0.0) += v;
    }
    pub fn get(&self, s: Stat) -> f32 {
        self.0.get(&s).copied().unwrap_or(0.0)
    }
    pub fn merge(&mut self, o: &Mods) {
        for (s, v) in &o.0 {
            self.add(*s, *v);
        }
    }
    pub fn with(mut self, s: Stat, v: f32) -> Self {
        self.add(s, v);
        self
    }
    /// Percent as a multiplier: 25 -> 1.25.
    pub fn inc(&self, s: Stat) -> f32 {
        1.0 + self.get(s) / 100.0
    }
}

/// Formats a stat line ("+12 to maximum Life").
pub fn describe(s: Stat, v: f32) -> String {
    let a = v.abs();
    let mut n = if (a - a.round()).abs() < 0.05 { format!("{}", a.round() as i64) } else { format!("{a:.1}") };
    let mut t = s.template().to_string();
    if v < 0.0 {
        // Negative values read naturally: "10% reduced", "20% less", "-15% to".
        let swaps = [("increased", "reduced"), ("reduced", "increased"), ("more", "less"), ("faster", "slower")];
        match swaps.iter().find(|(from, _)| t.contains(from)) {
            Some((from, to)) => t = t.replacen(from, to, 1),
            None if t.starts_with("+{}") => t = t.replacen("+{}", "-{}", 1),
            None => n = format!("-{n}"),
        }
    }
    t.replace("{}", &n)
}

/// Combat numbers derived from base values and mods.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub life_max: f32,
    pub mana_max: f32,
    pub life_regen: f32,
    pub mana_regen: f32,
    pub armor: f32,
    pub evasion: f32,
    pub block: f32,
    /// Resistances (percent) per element: physical (always 0), fire, cold, lightning, poison.
    pub res: [f32; 5],
    /// Multiplier on damage taken.
    pub taken: f32,
    pub attack_speed: f32,
    pub cast_speed: f32,
    pub move_speed: f32,
    pub cooldown: f32,
    pub crit_inc: f32,
    pub crit_flat: f32,
    pub crit_multi: f32,
    pub area: f32,
    pub proj_count: u32,
    pub proj_speed: f32,
    pub pierce: u32,
    pub chain: u32,
    pub duration: f32,
    /// Chance (0..1) per element to cause its ailment: bleed, ignite, freeze, shock, poison.
    pub ailment: [f32; 5],
    pub ailment_inc: f32,
    pub life_on_hit: f32,
    pub mana_on_hit: f32,
    pub life_leech: f32,
    pub life_on_kill: f32,
    pub thorns: f32,
    pub mana_cost: f32,
    pub item_rarity: f32,
    pub gold_find: f32,
    pub xp_gain: f32,
    pub dodge_recovery: f32,
    pub potion: f32,
    pub skill_levels: f32,
    pub melee_levels: f32,
    pub spell_levels: f32,
    /// The mods themselves (damage is computed per skill from its tags).
    pub mods: Mods,
}

pub const RES_CAP: f32 = 75.0;

/// Base values before mods.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Base {
    pub life: f32,
    pub mana: f32,
    pub life_regen: f32,
    pub mana_regen: f32,
    pub armor: f32,
    pub res: f32,
}

impl Sheet {
    pub fn compute(base: Base, m: &Mods) -> Sheet {
        let attr = m.get(Stat::AllAttributes);
        let (str_, dex, int) = (m.get(Stat::Strength) + attr, m.get(Stat::Dexterity) + attr, m.get(Stat::Intelligence) + attr);
        // Attributes: Strength gives life and melee damage, Dexterity attack speed and
        // evasion, Intelligence mana and spell damage (applied in `damage_inc`).
        let life_max = (base.life + m.get(Stat::Life) + str_ * 0.5) * (m.inc(Stat::LifeInc));
        let mana_max = (base.mana + m.get(Stat::Mana) + int * 0.5) * m.inc(Stat::ManaInc);
        let all = m.get(Stat::AllRes);
        let res = |s: Stat| (base.res + m.get(s) + all).min(RES_CAP);
        Sheet {
            life_max: life_max.max(1.0),
            mana_max: mana_max.max(0.0),
            life_regen: base.life_regen + m.get(Stat::LifeRegen),
            mana_regen: base.mana_regen * m.inc(Stat::ManaRegen),
            armor: (base.armor + m.get(Stat::Armor)) * m.inc(Stat::ArmorInc),
            evasion: (m.get(Stat::Evasion) + dex * 0.04).min(60.0),
            block: m.get(Stat::Block).min(60.0),
            res: [
                0.0,
                res(Stat::FireRes),
                res(Stat::ColdRes),
                res(Stat::LightningRes),
                (base.res + m.get(Stat::PoisonRes)).min(RES_CAP),
            ],
            taken: (1.0 - m.get(Stat::DamageTaken) / 100.0).max(0.1),
            attack_speed: m.inc(Stat::AttackSpeed) + dex * 0.002,
            cast_speed: m.inc(Stat::CastSpeed),
            move_speed: m.inc(Stat::MoveSpeed).max(0.2),
            cooldown: m.inc(Stat::CooldownRecovery),
            crit_inc: m.inc(Stat::CritChance),
            crit_flat: m.get(Stat::CritChanceFlat),
            crit_multi: 1.5 + m.get(Stat::CritMulti) / 100.0,
            area: m.inc(Stat::AreaInc),
            proj_count: m.get(Stat::ProjectileCount).max(0.0) as u32,
            proj_speed: m.inc(Stat::ProjectileSpeed),
            pierce: m.get(Stat::Pierce).max(0.0) as u32,
            chain: m.get(Stat::Chain).max(0.0) as u32,
            duration: m.inc(Stat::DurationInc),
            ailment: [
                m.get(Stat::BleedChance) / 100.0,
                m.get(Stat::IgniteChance) / 100.0,
                m.get(Stat::FreezeChance) / 100.0,
                m.get(Stat::ShockChance) / 100.0,
                m.get(Stat::PoisonChance) / 100.0,
            ],
            ailment_inc: m.inc(Stat::AilmentInc),
            life_on_hit: m.get(Stat::LifeOnHit),
            mana_on_hit: m.get(Stat::ManaOnHit),
            life_leech: m.get(Stat::LifeLeech) / 100.0,
            life_on_kill: m.get(Stat::LifeOnKill),
            thorns: m.get(Stat::Thorns),
            mana_cost: (1.0 - m.get(Stat::ManaCost) / 100.0).max(0.0),
            item_rarity: m.get(Stat::ItemRarity),
            gold_find: m.get(Stat::GoldFind),
            xp_gain: m.inc(Stat::XpGain),
            dodge_recovery: m.inc(Stat::DodgeRecovery),
            potion: m.inc(Stat::PotionInc),
            skill_levels: m.get(Stat::SkillLevels),
            melee_levels: m.get(Stat::MeleeLevels),
            spell_levels: m.get(Stat::SpellLevels),
            mods: m.clone(),
        }
        .with_attributes(str_, int)
    }

    fn with_attributes(mut self, str_: f32, int: f32) -> Self {
        // Kept as mods so damage picks them up by tag.
        self.mods.add(Stat::MeleeInc, str_ * 0.2);
        self.mods.add(Stat::SpellInc, int * 0.2);
        self
    }

    /// Damage multiplier for a hit with these tags and element ("increased" adds, "more"
    /// multiplies).
    pub fn damage_mult(&self, tags: &[&str], element: usize) -> f32 {
        let m = &self.mods;
        let mut inc = m.get(Stat::DamageInc);
        inc += match element {
            0 => m.get(Stat::PhysInc),
            1 => m.get(Stat::FireInc) + m.get(Stat::ElementalInc),
            2 => m.get(Stat::ColdInc) + m.get(Stat::ElementalInc),
            3 => m.get(Stat::LightningInc) + m.get(Stat::ElementalInc),
            _ => m.get(Stat::PoisonInc),
        };
        for t in tags {
            inc += match *t {
                "attack" => m.get(Stat::AttackInc),
                "spell" => m.get(Stat::SpellInc),
                "melee" => m.get(Stat::MeleeInc),
                "projectile" => m.get(Stat::ProjectileInc),
                "area" => m.get(Stat::AreaDamageInc),
                "minion" => m.get(Stat::MinionInc),
                _ => 0.0,
            };
        }
        (1.0 + inc / 100.0).max(0.0) * (1.0 + m.get(Stat::DamageMore) / 100.0).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_read() {
        for s in Stat::ALL {
            assert_eq!(Stat::from_key(s.key()), Some(*s));
            let json = serde_json::to_string(s).unwrap();
            assert_eq!(json, format!("\"{}\"", s.key()), "serde name matches the key");
        }
        assert_eq!(describe(Stat::Life, 12.0), "+12 to maximum Life");
        assert_eq!(describe(Stat::FireInc, 7.5), "7.5% increased Fire Damage");
        assert_eq!(describe(Stat::MoveSpeed, -10.0), "10% reduced Movement Speed");
        assert_eq!(describe(Stat::DamageTaken, -10.0), "10% increased Damage taken");
        assert_eq!(describe(Stat::DamageMore, -20.0), "20% less Damage");
        assert_eq!(describe(Stat::AllRes, -10.0), "-10% to all Elemental Resistances");
    }

    #[test]
    fn sheet_applies_mods() {
        let base = Base { life: 100.0, mana: 50.0, life_regen: 1.0, mana_regen: 4.0, armor: 0.0, res: 0.0 };
        let m = Mods::default().with(Stat::Life, 20.0).with(Stat::LifeInc, 50.0).with(Stat::FireRes, 90.0);
        let s = Sheet::compute(base, &m);
        assert!((s.life_max - 180.0).abs() < 1e-3);
        assert_eq!(s.res[1], RES_CAP);
        let m = Mods::default().with(Stat::FireInc, 50.0).with(Stat::DamageMore, 20.0);
        let s = Sheet::compute(base, &m);
        assert!((s.damage_mult(&["spell"], 1) - 1.8).abs() < 1e-3);
    }
}
