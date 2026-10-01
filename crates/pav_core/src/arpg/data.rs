//! Game data from /game (embedded at build time; `reload` re-reads the folder for live
//! editing). Every table is plain TOML: skills, monster families, and later items, the
//! passive tree, themes and levels.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

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
    pub color: String,
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
            color: "#ffffff".into(),
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
    /// Hero skills (not monster-only), in unlock order.
    pub fn hero_skills(&self) -> Vec<u16> {
        let mut v: Vec<u16> = (0..self.skills.len() as u16).filter(|i| !self.skill(*i).monster).collect();
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
        skills.push(s);
    }
    let mut d = Data { skills, families: Vec::new() };
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
    }
}
