//! The monster genome: Spore-style creatures from a seed. A genome is a body plan with
//! stretched proportions, bolted-on parts (horns, wings, mandibles...), an element's palette,
//! an archetype (brain + skill pools + stat shape) and a generated name. Designed families
//! become the same `MonsterSpec` as generated genomes, so spawning, affixes, bosses and tools
//! treat them alike. Monster affixes (elite modifiers) live here too.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::data::{Archetype, Data, Element};
use super::powers::Power;
use super::skills::{Tweak, TweakField};
use super::stats::{Mods, Stat};
use crate::params::ChoiceParam;
use crate::parts::{Attach, AttachKind};
use crate::puppet::{BodyPlan, PuppetDef, WeaponKind};
use crate::rng::Rng;

// ------------------------------------------------------------------------------ data file

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BodyGene {
    pub weight: f32,
    pub scale: [f32; 2],
    pub torso: [f32; 2],
    pub head: [f32; 2],
    pub legs: [f32; 2],
    pub arms: [f32; 2],
    pub limbs: [f32; 2],
    pub tail: [f32; 2],
    pub tail_chance: f32,
    pub antenna: [f32; 2],
    pub antenna_chance: f32,
    pub leg_pairs: [i32; 2],
    pub body_length: [f32; 2],
    pub weapons: Vec<String>,
    pub radius: f32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PartGene {
    pub weight: f32,
    pub bodies: Vec<String>,
    pub size: [f32; 2],
    pub count: [u32; 2],
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PaletteGene {
    pub skin: Vec<String>,
    pub body: Vec<String>,
    pub accent: Vec<String>,
    pub eyes: Vec<String>,
    pub glow: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArchGene {
    pub brain: Archetype,
    pub weight: f32,
    pub skills: Vec<Vec<String>>,
    pub life: f32,
    pub damage: f32,
    pub speed: f32,
    pub xp: f32,
    pub size: f32,
    /// Pack size multiplier (swarms come in crowds).
    pub pack: f32,
    pub power: Option<Power>,
}

impl Default for ArchGene {
    fn default() -> Self {
        Self {
            brain: Archetype::Melee,
            weight: 1.0,
            skills: Vec::new(),
            life: 1.0,
            damage: 1.0,
            speed: 1.0,
            xp: 1.0,
            size: 1.0,
            pack: 1.0,
            power: None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GenomeData {
    pub body: BTreeMap<String, BodyGene>,
    pub part: BTreeMap<String, PartGene>,
    pub palette: BTreeMap<String, PaletteGene>,
    pub archetype: BTreeMap<String, ArchGene>,
    pub names: BTreeMap<String, Vec<String>>,
}

/// A monster affix (game/monster_affixes.toml): magic and rare monsters get some.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MonsterAffix {
    #[serde(skip_deserializing)]
    pub key: String,
    pub name: String,
    pub weight: f32,
    pub level: u32,
    pub mods: BTreeMap<String, f32>,
    pub power: Option<Power>,
    pub tweaks: BTreeMap<String, f32>,
    pub skills: Vec<String>,
    pub life: f32,
    pub scale: f32,
    pub speed: f32,
    pub tint: String,
    #[serde(skip)]
    pub stats: Vec<(Stat, f32)>,
}

impl Default for MonsterAffix {
    fn default() -> Self {
        Self {
            key: String::new(),
            name: String::new(),
            weight: 100.0,
            level: 1,
            mods: BTreeMap::new(),
            power: None,
            tweaks: BTreeMap::new(),
            skills: Vec::new(),
            life: 1.0,
            scale: 1.0,
            speed: 1.0,
            tint: String::new(),
            stats: Vec::new(),
        }
    }
}

// ----------------------------------------------------------------------------- the genome

/// What to fix when generating (anything left open is rolled).
#[derive(Clone, Debug, Default)]
pub struct GenomeOpts {
    pub body: Option<BodyPlan>,
    pub archetype: Option<String>,
    pub element: Option<Element>,
}

/// A generated (or designed) creature.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Genome {
    pub seed: u64,
    pub name: String,
    pub body: BodyPlan,
    pub archetype: String,
    pub brain: Archetype,
    pub element: Element,
    pub puppet: PuppetDef,
    pub skills: Vec<String>,
    pub life: f32,
    pub damage: f32,
    pub speed: f32,
    pub xp: f32,
    pub pack: f32,
    pub radius: f32,
    pub powers: Vec<Power>,
}

/// Everything needed to spawn a monster (from a designed family or a genome).
#[derive(Clone, Debug)]
pub struct MonsterSpec {
    pub key: String,
    pub name: String,
    pub puppet: PuppetDef,
    pub brain: Archetype,
    pub skills: Vec<u16>,
    pub life: f32,
    pub damage: f32,
    pub speed: f32,
    pub xp: f32,
    pub radius: f32,
    pub powers: Vec<Power>,
    pub tweaks: Vec<Tweak>,
    pub genome: Option<u64>,
}

fn pick<'a, T>(rng: &mut Rng, v: &'a [T]) -> Option<&'a T> {
    if v.is_empty() { None } else { Some(&v[rng.below(v.len() as u32) as usize]) }
}

fn pick_weighted<'a, T>(rng: &mut Rng, v: &[(&'a str, &'a T, f32)]) -> Option<(&'a str, &'a T)> {
    let total: f32 = v.iter().map(|x| x.2.max(0.0)).sum();
    if total <= 0.0 {
        return None;
    }
    let mut r = rng.f32() * total;
    for x in v {
        if r < x.2 {
            return Some((x.0, x.1));
        }
        r -= x.2.max(0.0);
    }
    v.last().map(|x| (x.0, x.1))
}

fn range(rng: &mut Rng, r: [f32; 2]) -> f32 {
    if r[1] <= r[0] { r[0] } else { rng.range(r[0], r[1]) }
}

pub fn body_key(b: BodyPlan) -> &'static str {
    b.name()
}

impl Genome {
    /// Rolls a creature from a seed. `level` nudges it toward elements and parts deeper down.
    pub fn generate(d: &Data, seed: u64, level: u32, opts: &GenomeOpts) -> Result<Genome, String> {
        let gd = &d.genome;
        let mut rng = Rng::new(seed ^ 0x9e37_79b9_7f4a_7c15);
        // Body.
        let body = match opts.body {
            Some(b) => b,
            None => {
                let bodies: Vec<(&str, &BodyGene, f32)> = gd.body.iter().map(|(k, b)| (k.as_str(), b, b.weight)).collect();
                let (k, _) = pick_weighted(&mut rng, &bodies).ok_or("genome: no body plans")?;
                BodyPlan::NAMES
                    .iter()
                    .position(|n| *n == k)
                    .map(BodyPlan::from_index)
                    .ok_or(format!("genome: unknown body '{k}'"))?
            }
        };
        let bg = gd.body.get(body_key(body)).ok_or_else(|| format!("genome: no [body.{}]", body_key(body)))?;
        // Archetype (some bodies suit some archetypes: blobs don't skirmish).
        let arch_key = match &opts.archetype {
            Some(a) => a.clone(),
            None => {
                let arches: Vec<(&str, &ArchGene, f32)> = gd
                    .archetype
                    .iter()
                    .map(|(k, a)| {
                        let fit = match (body, a.brain) {
                            (BodyPlan::Blob, Archetype::Skirmisher) => 0.2,
                            (BodyPlan::Spider, Archetype::Swarm) => 2.0,
                            (BodyPlan::Biped, Archetype::Caster | Archetype::Summoner) => 1.6,
                            _ => 1.0,
                        };
                        (k.as_str(), a, a.weight * fit)
                    })
                    .collect();
                pick_weighted(&mut rng, &arches).ok_or("genome: no archetypes")?.0.to_string()
            }
        };
        let ag = gd.archetype.get(&arch_key).ok_or_else(|| format!("genome: unknown archetype '{arch_key}'"))?;
        // Element: more elemental the deeper.
        let element = match opts.element {
            Some(e) => e,
            None => {
                let phys = (3.0 - level as f32 * 0.04).max(0.8);
                let w = [phys, 1.6, 1.6, 1.4, 1.4];
                let total: f32 = w.iter().sum();
                let mut r = rng.f32() * total;
                let mut e = Element::Physical;
                for (i, x) in w.iter().enumerate() {
                    if r < *x {
                        e = Element::ALL[i];
                        break;
                    }
                    r -= x;
                }
                e
            }
        };
        let pal = gd
            .palette
            .get(&element.name().to_lowercase())
            .ok_or_else(|| format!("genome: no [palette.{}]", element.name().to_lowercase()))?;
        // The puppet: preset, stretched, painted.
        let mut p = PuppetDef::preset(body);
        let size = range(&mut rng, bg.scale) * ag.size;
        p.scale = size;
        p.torso_radius *= range(&mut rng, bg.torso);
        p.head_radius *= range(&mut rng, bg.head);
        p.leg_length *= range(&mut rng, bg.legs);
        p.arm_length *= range(&mut rng, bg.arms);
        p.limb_radius *= range(&mut rng, bg.limbs);
        if bg.leg_pairs[1] > 0 {
            p.legs = bg.leg_pairs[0] + rng.below((bg.leg_pairs[1] - bg.leg_pairs[0] + 1).max(1) as u32) as i32;
        }
        if bg.body_length[1] > 0.0 {
            p.body_length *= range(&mut rng, bg.body_length);
        }
        if rng.f32() < bg.tail_chance {
            p.tail_length = range(&mut rng, bg.tail);
        }
        if rng.f32() < bg.antenna_chance {
            p.antenna_length = range(&mut rng, bg.antenna);
        }
        let col = |rng: &mut Rng, v: &Vec<String>, fallback: &str| pick(rng, v).cloned().unwrap_or_else(|| fallback.to_string());
        p.skin = col(&mut rng, &pal.skin, "#8a8a8a");
        p.shirt = col(&mut rng, &pal.body, "#5a5a5a");
        p.pants = p.shirt.clone();
        p.shoes = p.shirt.clone();
        p.accent = col(&mut rng, &pal.accent, "#c0c0c0");
        p.eyes = col(&mut rng, &pal.eyes, "#ffe066");
        if body == BodyPlan::Blob {
            p.shirt = p.skin.clone();
        }
        if body == BodyPlan::Biped {
            p.lean = rng.range(0.6, 1.8);
            p.arm_swing = rng.range(0.5, 1.3);
            if let Some(w) = pick(&mut rng, &bg.weapons) {
                p.weapon.kind =
                    WeaponKind::NAMES.iter().position(|n| n == w).map(WeaponKind::from_index).unwrap_or(WeaponKind::None);
                p.weapon.color = p.accent.clone();
                p.weapon.glow = pal.glow * 0.5;
            }
        }
        // Parts: 0-3 (more deeper down).
        let parts_n = (rng.below(3) + if level > 10 { 1 } else { 0 } + if rng.f32() < 0.5 { 1 } else { 0 }).min(3);
        let fitting: Vec<(&str, &PartGene, f32)> = gd
            .part
            .iter()
            .filter(|(_, g)| g.bodies.iter().any(|b| b == body_key(body)))
            .map(|(k, g)| (k.as_str(), g, g.weight))
            .collect();
        let mut taken: Vec<&str> = Vec::new();
        for _ in 0..parts_n {
            let avail: Vec<(&str, &PartGene, f32)> = fitting.iter().filter(|x| !taken.contains(&x.0)).copied().collect();
            let Some((k, g)) = pick_weighted(&mut rng, &avail) else { break };
            taken.push(k);
            let kind = AttachKind::NAMES
                .iter()
                .position(|n| *n == k)
                .map(AttachKind::from_index)
                .ok_or(format!("genome: unknown part '{k}'"))?;
            let count = if g.count[1] > 0 { g.count[0] + rng.below(g.count[1] - g.count[0] + 1) } else { 0 };
            let color = if matches!(kind, AttachKind::Horns | AttachKind::Tusks | AttachKind::Antlers | AttachKind::Spikes)
                && rng.f32() < 0.5
            {
                "#e8e0c8".to_string()
            } else {
                String::new()
            };
            p.parts.push(Attach { kind, size: range(&mut rng, g.size), count, color, glow: pal.glow });
        }
        // Skills: one from each pool.
        let mut skills = Vec::new();
        for pool in &ag.skills {
            if let Some(s) = pick(&mut rng, pool) {
                if d.skill_id(s).is_none() {
                    return Err(format!("genome archetype '{arch_key}': unknown skill '{s}'"));
                }
                if !skills.contains(s) {
                    skills.push(s.clone());
                }
            }
        }
        if skills.is_empty() {
            return Err(format!("genome archetype '{arch_key}' has no skills"));
        }
        // Name: element word + body word (+ archetype word).
        let names = &gd.names;
        let w = |rng: &mut Rng, k: &str| names.get(k).and_then(|v| pick(rng, v).cloned()).unwrap_or_default();
        let e = w(&mut rng, &element.name().to_lowercase());
        let b = w(&mut rng, body_key(body));
        let a = w(&mut rng, &arch_key);
        let mut name = format!("{e} {b}").trim().to_string();
        if !a.is_empty() {
            if a.chars().next().is_some_and(|c| c.is_lowercase()) {
                name.push_str(&a);
            } else {
                name = format!("{name} {a}");
            }
        }
        // Bigger is tougher and slower.
        let bulk = (size / 1.0).powf(1.5);
        Ok(Genome {
            seed,
            name,
            body,
            archetype: arch_key,
            brain: ag.brain,
            element,
            puppet: p,
            skills,
            life: ag.life * bulk.clamp(0.4, 3.0),
            damage: ag.damage * size.sqrt(),
            speed: ag.speed * (1.15 - 0.15 * size).clamp(0.6, 1.3),
            xp: ag.xp * bulk.clamp(0.5, 2.5),
            pack: ag.pack,
            radius: bg.radius,
            powers: ag.power.into_iter().collect(),
        })
    }

    /// The spawnable form.
    pub fn spec(&self, d: &Data) -> MonsterSpec {
        let mut tweaks = Vec::new();
        if self.element != Element::Physical {
            tweaks.push(Tweak { skill: "*".into(), field: TweakField::Element, value: self.element.index() as f32 });
        }
        MonsterSpec {
            key: format!("genome:{}", self.seed),
            name: self.name.clone(),
            puppet: self.puppet.clone(),
            brain: self.brain,
            skills: self.skills.iter().filter_map(|s| d.skill_id(s)).collect(),
            life: self.life,
            damage: self.damage,
            speed: self.speed,
            xp: self.xp,
            radius: self.radius,
            powers: self.powers.clone(),
            tweaks,
            genome: Some(self.seed),
        }
    }
}

impl super::data::FamilyDef {
    /// A designed family as a spawnable spec.
    pub fn spec(&self) -> MonsterSpec {
        MonsterSpec {
            key: self.key.clone(),
            name: self.name.clone(),
            puppet: self.puppet.clone(),
            brain: self.archetype,
            skills: self.skill_ids.clone(),
            life: self.life,
            damage: self.damage,
            speed: self.speed,
            xp: self.xp,
            radius: match self.body {
                BodyPlan::Biped => 0.42,
                BodyPlan::Blob => 0.55,
                _ => 0.5,
            },
            powers: Vec::new(),
            tweaks: Vec::new(),
            genome: None,
        }
    }
}

/// Picks `n` affixes for a monster of this level (no repeats).
pub fn roll_affixes(d: &Data, rng: &mut Rng, level: u32, n: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for _ in 0..n {
        let cands: Vec<(&str, &MonsterAffix, f32)> = d
            .monster_affixes
            .iter()
            .filter(|a| a.level <= level && !out.contains(&a.key))
            .map(|a| (a.key.as_str(), a, a.weight))
            .collect();
        match pick_weighted(rng, &cands) {
            Some((k, _)) => out.push(k.to_string()),
            None => break,
        }
    }
    out
}

/// Applies affixes to a spec before spawning: stats, powers, tweaks, extra skills, size.
pub fn apply_affixes(d: &Data, spec: &mut MonsterSpec, keys: &[String]) -> (Mods, Vec<String>) {
    let mut mods = Mods::default();
    let mut names = Vec::new();
    for k in keys {
        let Some(a) = d.monster_affixes.iter().find(|a| &a.key == k) else { continue };
        for (s, v) in &a.stats {
            mods.add(*s, *v);
        }
        if let Some(p) = a.power {
            spec.powers.push(p);
        }
        for (f, v) in &a.tweaks {
            if let Some(field) = TweakField::from_key(f) {
                spec.tweaks.push(Tweak { skill: "*".into(), field, value: *v });
            }
        }
        for s in &a.skills {
            if let Some(id) = d.skill_id(s) {
                if !spec.skills.contains(&id) {
                    spec.skills.push(id);
                }
            }
        }
        spec.life *= a.life;
        spec.speed *= a.speed;
        spec.puppet.scale *= a.scale;
        if !a.tint.is_empty() {
            spec.puppet.accent = a.tint.clone();
        }
        names.push(a.name.clone());
    }
    (mods, names)
}

/// A rare monster's own name ("Gorefang the Unyielding").
pub fn rare_name(rng: &mut Rng) -> String {
    const A: &[&str] = &["Gore", "Grim", "Rot", "Skull", "Dread", "Blood", "Ash", "Hollow", "Vile", "Iron", "Black", "Wrath"];
    const B: &[&str] = &["fang", "maw", "gut", "claw", "hide", "spine", "eye", "tooth", "heart", "horn", "scale", "bane"];
    const C: &[&str] = &[
        "the Unyielding",
        "the Ravenous",
        "the Cruel",
        "the Undying",
        "the Hungering",
        "the Vast",
        "the Swift",
        "the Wretched",
        "the Burning",
        "the Patient",
    ];
    format!(
        "{}{} {}",
        A[rng.below(A.len() as u32) as usize],
        B[rng.below(B.len() as u32) as usize],
        C[rng.below(C.len() as u32) as usize]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genomes_are_varied_valid_and_repeatable() {
        let d = super::super::data::data();
        let mut bodies = std::collections::BTreeSet::new();
        let mut arches = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        let mut with_parts = 0;
        for seed in 0..300u64 {
            let g = Genome::generate(&d, seed, 20, &GenomeOpts::default()).unwrap();
            assert!(!g.skills.is_empty() && !g.name.is_empty());
            assert!(g.life > 0.0 && g.speed > 0.0 && g.puppet.scale > 0.2);
            bodies.insert(format!("{:?}", g.body));
            arches.insert(g.archetype.clone());
            names.insert(g.name.clone());
            with_parts += (!g.puppet.parts.is_empty()) as u32;
            let again = Genome::generate(&d, seed, 20, &GenomeOpts::default()).unwrap();
            assert_eq!(serde_json::to_string(&g).unwrap(), serde_json::to_string(&again).unwrap(), "same seed, same creature");
            assert!(!g.spec(&d).skills.is_empty());
        }
        assert_eq!(bodies.len(), 5, "{bodies:?}");
        assert!(arches.len() >= 8, "{arches:?}");
        assert!(names.len() > 120, "{} distinct names", names.len());
        assert!(with_parts > 150, "{with_parts} with parts");
    }
}
