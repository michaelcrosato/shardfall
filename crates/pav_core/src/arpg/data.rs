//! Game data from /game (embedded at build time; `reload` re-reads the folder for live
//! editing). Every table is plain TOML: skills, monster families, and later items, the
//! passive tree, themes and levels.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use super::items::{AffixDef, BaseDef, Slot, UniqueDef};
use super::stats::Stat;
use crate::puppet::{ActKind, BodyPlan, PuppetDef};

include!(concat!(env!("OUT_DIR"), "/game_data.rs"));

/// The text of a data file (`"skills"`, `"levels/01_shrines"`), embedded or from disk.
pub fn source(key: &str) -> Option<String> {
    if let Some(dir) = disk_dir() {
        if let Ok(t) = std::fs::read_to_string(dir.join(format!("{key}.toml"))) {
            return Some(t);
        }
    }
    GAME_DATA.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
}

/// Keys of the data files under a folder (`"levels"`).
pub fn keys_under(prefix: &str) -> Vec<String> {
    let p = format!("{prefix}/");
    let mut v: Vec<String> = GAME_DATA.iter().filter(|(k, _)| k.starts_with(&p)).map(|(k, _)| k.to_string()).collect();
    if let Some(dir) = disk_dir() {
        if let Ok(rd) = std::fs::read_dir(dir.join(prefix)) {
            for e in rd.flatten() {
                let path = e.path();
                if path.extension().is_some_and(|x| x == "toml") {
                    let k = format!("{p}{}", path.file_stem().unwrap_or_default().to_string_lossy());
                    if !v.contains(&k) {
                        v.push(k);
                    }
                }
            }
        }
    }
    v.sort();
    v
}

/// The game folder on disk when running from the repository (live editing), if any.
fn disk_dir() -> Option<std::path::PathBuf> {
    if !USE_DISK.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let d = std::env::var("PAV_GAME").map(std::path::PathBuf::from).unwrap_or_else(|_| "game".into());
    d.join("skills.toml").exists().then_some(d)
}

static USE_DISK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Element {
    #[default]
    Physical,
    Fire,
    Cold,
    Lightning,
    Poison,
}

impl Element {
    pub const ALL: [Element; 5] = [Element::Physical, Element::Fire, Element::Cold, Element::Lightning, Element::Poison];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        ["Physical", "Fire", "Cold", "Lightning", "Poison"][self as usize]
    }
    /// Display colour (linear-ish RGB for effects and numbers).
    pub fn color(self) -> [f32; 3] {
        [[0.92, 0.9, 0.85], [1.0, 0.45, 0.12], [0.45, 0.8, 1.0], [1.0, 0.92, 0.3], [0.55, 0.9, 0.25]][self as usize]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Behavior {
    #[default]
    Melee,
    Slam,
    Leap,
    Dash,
    Projectile,
    Nova,
    Charge,
    /// Held: spins (or pulses) every `interval` while the button is down, paying `cost` per second.
    Channel,
    /// A rolling line of blasts along the aim (`count` segments).
    Wave,
    /// A war cry: `buff` stats for `duration` on the user (and allies near it), taunts.
    Buff,
    /// Something falls on the target after `delay`.
    Meteor,
    /// Hurting ground at the target for `duration` (blizzards, poison pools).
    Field,
    /// Teleport to the target (up to `range`).
    Blink,
    /// `count` small blasts scattered over `scatter` metres around the target.
    Rain,
}

/// One skill (hero skill or monster attack). See game/skills.toml for the fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SkillDef {
    #[serde(skip_deserializing)]
    pub key: String,
    pub name: String,
    pub about: String,
    pub tags: Vec<String>,
    pub behavior: Behavior,
    pub anim: ActKind,
    pub unlock: u32,
    pub cost: f32,
    pub cooldown: f32,
    pub time: f32,
    pub hit: f32,
    pub effect: f32,
    pub base: [f32; 2],
    pub element: Element,
    pub range: f32,
    pub angle: f32,
    pub radius: f32,
    pub speed: f32,
    pub count: u32,
    pub spread: f32,
    pub explode: f32,
    pub pierce: u32,
    pub combo: u32,
    pub knockback: f32,
    pub mana_gain: f32,
    pub hitstop: f32,
    pub shake: f32,
    pub ailment: f32,
    pub telegraph: bool,
    pub monster: bool,
    /// Internal skill behind a power (unique items, keystones): never on the skill bar.
    pub power: bool,
    pub color: String,
    /// Fields, buffs and channels: how long (s); channel pulses every `interval` (s).
    pub duration: f32,
    pub interval: f32,
    /// Projectiles: extra enemies to chain to.
    pub chain: u32,
    /// Meteors: seconds until it lands. Rain: area the drops scatter over (m).
    pub delay: f32,
    pub scatter: f32,
    /// War cries: stats granted while the buff lasts.
    pub buff: BTreeMap<String, f32>,
    #[serde(skip)]
    pub buff_mods: super::stats::Mods,
}

impl Default for SkillDef {
    fn default() -> Self {
        Self {
            key: String::new(),
            name: String::new(),
            about: String::new(),
            tags: Vec::new(),
            behavior: Behavior::Melee,
            anim: ActKind::Slash,
            unlock: 1,
            cost: 0.0,
            cooldown: 0.0,
            time: 0.6,
            hit: 0.5,
            effect: 1.0,
            base: [0.0, 0.0],
            element: Element::Physical,
            range: 2.5,
            angle: 120.0,
            radius: 0.0,
            speed: 15.0,
            count: 1,
            spread: 12.0,
            explode: 0.0,
            pierce: 0,
            combo: 1,
            knockback: 0.0,
            mana_gain: 0.0,
            hitstop: 0.0,
            shake: 0.0,
            ailment: 0.0,
            telegraph: false,
            monster: false,
            power: false,
            color: "#ffffff".into(),
            duration: 0.0,
            interval: 0.25,
            chain: 0,
            delay: 0.8,
            scatter: 0.0,
            buff: BTreeMap::new(),
            buff_mods: Default::default(),
        }
    }
}

impl SkillDef {
    pub fn has(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
    pub fn is_attack(&self) -> bool {
        !self.has("spell")
    }
    pub fn tag_refs(&self) -> Vec<&str> {
        self.tags.iter().map(String::as_str).collect()
    }
    pub fn rgb(&self) -> [f32; 3] {
        crate::color::Color::try_hex(&self.color).map(|c| c.0).unwrap_or([1.0; 3])
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Archetype {
    #[default]
    Melee,
    Ranged,
    Charger,
}

/// A monster family (game/monsters.toml).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FamilyDef {
    #[serde(skip_deserializing)]
    pub key: String,
    pub name: String,
    pub body: BodyPlan,
    pub archetype: Archetype,
    pub skills: Vec<String>,
    pub scale: f32,
    pub life: f32,
    pub damage: f32,
    pub speed: f32,
    pub xp: f32,
    pub look: toml::Table,
    /// Resolved at load.
    #[serde(skip)]
    pub puppet: PuppetDef,
    #[serde(skip)]
    pub skill_ids: Vec<u16>,
}

impl Default for FamilyDef {
    fn default() -> Self {
        Self {
            key: String::new(),
            name: String::new(),
            body: BodyPlan::Biped,
            archetype: Archetype::Melee,
            skills: Vec::new(),
            scale: 1.0,
            life: 1.0,
            damage: 1.0,
            speed: 1.0,
            xp: 1.0,
            look: toml::Table::new(),
            puppet: PuppetDef::default(),
            skill_ids: Vec::new(),
        }
    }
}

/// A puppet preset with a table of settings applied over it.
pub fn puppet_with(plan: BodyPlan, look: &toml::Table, scale: f32) -> Result<PuppetDef, String> {
    let mut preset = PuppetDef::preset(plan);
    preset.scale = scale;
    if look.is_empty() {
        return Ok(preset);
    }
    let mut table = toml::Table::try_from(&preset).map_err(|e| e.to_string())?;
    for (k, v) in look {
        if !table.contains_key(k) {
            return Err(format!("unknown look setting '{k}'"));
        }
        table.insert(k.clone(), v.clone());
    }
    toml::Value::Table(table).try_into().map_err(|e: toml::de::Error| e.to_string())
}

/// Everything loaded.
#[derive(Debug)]
pub struct Data {
    pub skills: Vec<SkillDef>,
    pub families: Vec<FamilyDef>,
    pub bases: Vec<BaseDef>,
    pub affixes: Vec<AffixDef>,
    pub uniques: Vec<UniqueDef>,
    /// The passive tree, generated from game/tree.toml.
    pub tree: super::tree::Tree,
}

impl Data {
    pub fn skill_id(&self, key: &str) -> Option<u16> {
        self.skills.iter().position(|s| s.key == key).map(|i| i as u16)
    }
    pub fn skill(&self, id: u16) -> &SkillDef {
        &self.skills[(id as usize).min(self.skills.len() - 1)]
    }
    pub fn family(&self, key: &str) -> Option<&FamilyDef> {
        self.families.iter().find(|f| f.key == key)
    }
    pub fn base(&self, key: &str) -> Option<&BaseDef> {
        self.bases.iter().find(|b| b.key == key)
    }
    pub fn affix(&self, key: &str) -> Option<&AffixDef> {
        self.affixes.iter().find(|a| a.key == key)
    }
    pub fn unique(&self, key: &str) -> Option<&UniqueDef> {
        self.uniques.iter().find(|u| u.key == key)
    }
    /// Hero skills (not monster-only or internal), in unlock order.
    pub fn hero_skills(&self) -> Vec<u16> {
        let mut v: Vec<u16> =
            (0..self.skills.len() as u16).filter(|i| !self.skill(*i).monster && !self.skill(*i).power).collect();
        v.sort_by_key(|i| (self.skill(*i).unlock, self.skill(*i).key.clone()));
        v
    }
}

fn table<T: serde::de::DeserializeOwned>(key: &str) -> Result<BTreeMap<String, T>, String> {
    let text = source(key).ok_or_else(|| format!("game/{key}.toml is missing"))?;
    toml::from_str(&text).map_err(|e| format!("game/{key}.toml: {e}"))
}

pub fn load() -> Result<Data, String> {
    let mut skills = Vec::new();
    for (k, mut s) in table::<SkillDef>("skills")? {
        s.key = k;
        if s.name.is_empty() {
            s.name = s.key.clone();
        }
        for (st, v) in &s.buff {
            let st = Stat::from_key(st).ok_or_else(|| format!("skill '{}': unknown stat '{st}' in buff", s.key))?;
            s.buff_mods.add(st, *v);
        }
        skills.push(s);
    }
    let mut d = Data {
        skills,
        families: Vec::new(),
        bases: Vec::new(),
        affixes: Vec::new(),
        uniques: Vec::new(),
        tree: Default::default(),
    };
    for (k, mut f) in table::<FamilyDef>("monsters")? {
        f.key = k.clone();
        f.puppet = puppet_with(f.body, &f.look, f.scale).map_err(|e| format!("monster '{k}': {e}"))?;
        for s in &f.skills {
            f.skill_ids.push(d.skill_id(s).ok_or_else(|| format!("monster '{k}': unknown skill '{s}'"))?);
        }
        if f.skill_ids.is_empty() {
            return Err(format!("monster '{k}' has no skills"));
        }
        d.families.push(f);
    }
    let stat = |owner: &str, k: &str| Stat::from_key(k).ok_or_else(|| format!("{owner}: unknown stat '{k}'"));
    for (k, mut b) in table::<BaseDef>("items")? {
        b.key = k.clone();
        for (s, v) in &b.implicit {
            b.implicits.push((stat(&format!("item '{k}'"), s)?, *v));
        }
        if b.slot == Slot::Weapon && b.weapon_kind() == crate::puppet::WeaponKind::None {
            return Err(format!("item '{k}': unknown weapon kind '{}'", b.kind));
        }
        d.bases.push(b);
    }
    d.bases.sort_by_key(|b| (b.slot, b.kind.clone(), b.level));
    for (k, mut a) in table::<AffixDef>("affixes")? {
        a.key = k.clone();
        a.stat_id = Some(stat(&format!("affix '{k}'"), &a.stat)?);
        if a.tiers.is_empty() {
            return Err(format!("affix '{k}' has no tiers"));
        }
        if a.tiers.windows(2).any(|w| w[1][0] < w[0][0]) {
            return Err(format!("affix '{k}': tiers must go up in item level"));
        }
        for s in &a.slots {
            if !d.bases.iter().any(|b| b.matches(std::slice::from_ref(s))) && s != "armor" {
                return Err(format!("affix '{k}': slot '{s}' matches no item base"));
            }
        }
        d.affixes.push(a);
    }
    for (k, mut u) in table::<UniqueDef>("uniques")? {
        u.key = k.clone();
        if d.base(&u.base).is_none() {
            return Err(format!("unique '{k}': unknown base '{}'", u.base));
        }
        for (s, r) in &u.mods {
            u.stats.push((stat(&format!("unique '{k}'"), s)?, *r));
        }
        d.uniques.push(u);
    }
    for s in &d.skills {
        if s.power && !s.key.starts_with("power_") {
            return Err(format!("skill '{}': power skills are named power_*", s.key));
        }
    }
    let text = source("tree").ok_or("game/tree.toml is missing")?;
    let file: super::tree::TreeFile = toml::from_str(&text).map_err(|e| format!("game/tree.toml: {e}"))?;
    let names = |k: &str| d.skill_id(k).filter(|i| !d.skill(*i).monster).map(|i| d.skill(i).name.clone());
    d.tree = super::tree::Tree::build(&file, &names)?;
    Ok(d)
}

static DATA: RwLock<Option<Arc<Data>>> = RwLock::new(None);

/// The loaded data (embedded files the first time).
pub fn data() -> Arc<Data> {
    if let Some(d) = DATA.read().unwrap().as_ref() {
        return d.clone();
    }
    let d = Arc::new(load().unwrap_or_else(|e| panic!("game data: {e}")));
    *DATA.write().unwrap() = Some(d.clone());
    d
}

/// Re-reads the data (from ./game or $PAV_GAME when present). Returns the error and keeps the
/// old data if anything is invalid.
pub fn reload(from_disk: bool) -> Result<(), String> {
    USE_DISK.store(from_disk, std::sync::atomic::Ordering::Relaxed);
    let d = load()?;
    *DATA.write().unwrap() = Some(Arc::new(d));
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_data_loads() {
        let d = super::load().expect("game data loads");
        assert!(d.skill_id("slash").is_some());
        assert!(!d.families.is_empty());
        assert!(!d.hero_skills().is_empty());
        assert!(d.bases.len() > 100 && d.affixes.len() > 60 && d.uniques.len() > 15);
        for k in ["power_blade", "power_ember", "power_frost", "power_meteor", "power_storm", "power_corpse"] {
            assert!(d.skill_id(k).is_some_and(|i| d.skill(i).power), "{k}");
        }
    }
}
