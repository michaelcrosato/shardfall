//! Bosses: a genome made huge, with phases. Designed bosses (game/bosses.toml) start from a
//! genome seed and override what matters (body, element, parts, look); each phase fires at a
//! share of life: a line of speech, new skills, summons, stat changes, a power. Past the
//! designed ones, bosses are generated from a seed with the same pieces.

use std::collections::BTreeMap;

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::combat::*;
use super::data::{Data, Element, data};
use super::genome::{Genome, GenomeOpts, MonsterSpec};
use super::powers::{Power, PowerKind};
use super::stats::Stat;
use super::{Game, feet_of};
use crate::entity::EntityId;
use crate::frame::SimEvent;
use crate::parts::Attach;
use crate::puppet::BodyPlan;
use crate::rng::Rng;
use crate::sim::Sim;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SummonDef {
    /// A family key, or "brood" (born from the boss's own genome).
    pub family: String,
    pub count: u32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PhaseDef {
    /// Life share at which it starts (0.66 = two thirds left).
    pub at: f32,
    pub say: String,
    pub skills: Vec<String>,
    pub summon: Option<SummonDef>,
    pub mods: BTreeMap<String, f32>,
    pub power: Option<Power>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BossDef {
    #[serde(skip_deserializing)]
    pub key: String,
    pub name: String,
    pub title: String,
    pub lore: String,
    /// The genome it grows from, then what is forced.
    pub seed: u64,
    pub body: Option<BodyPlan>,
    pub archetype: Option<String>,
    pub element: Option<Element>,
    pub scale: f32,
    pub parts: Vec<Attach>,
    pub look: toml::Table,
    pub skills: Vec<String>,
    pub life: f32,
    pub damage: f32,
    pub phases: Vec<PhaseDef>,
}

/// A boss's progress through its phases.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BossState {
    pub key: String,
    pub phase: u8,
    /// Generated bosses carry their own definition seed.
    #[serde(default)]
    pub seed: u64,
}

/// The boss definition for a key ("gen:<seed>" makes one).
pub fn boss_def(d: &Data, key: &str, level: u32) -> Option<BossDef> {
    if let Some(seed) = key.strip_prefix("gen:").and_then(|s| s.parse::<u64>().ok()) {
        return Some(generated(d, seed, level));
    }
    d.bosses.iter().find(|b| b.key == key).cloned()
}

/// A boss from nothing but a seed.
pub fn generated(d: &Data, seed: u64, level: u32) -> BossDef {
    let mut rng = Rng::new(seed ^ 0xb055_b055);
    let arches = ["brute", "tank", "caster", "summoner", "charger"];
    let arch = arches[rng.below(arches.len() as u32) as usize].to_string();
    let opts = GenomeOpts { archetype: Some(arch.clone()), ..Default::default() };
    let g = Genome::generate(d, seed, level, &opts).ok();
    let big = ["meteor", "earthsplitter", "blizzard", "war_cry", "toxic_rain", "chain_lightning", "leap_slam"];
    let extra = big[rng.below(big.len() as u32) as usize].to_string();
    let base_name = g.map(|g| g.name).unwrap_or_else(|| "Horror".into());
    let name = format!("{}, {}", super::genome::rare_name(&mut rng).split(' ').next().unwrap_or("Vor"), base_name);
    BossDef {
        key: format!("gen:{seed}"),
        name,
        title: "a horror of the depths".into(),
        lore: String::new(),
        seed,
        archetype: Some(arch),
        scale: rng.range(2.0, 2.6),
        life: 1.6,
        damage: 1.0,
        phases: vec![
            PhaseDef {
                at: 0.66,
                say: "It calls its brood!".into(),
                summon: Some(SummonDef { family: "brood".into(), count: 4 }),
                ..Default::default()
            },
            PhaseDef {
                at: 0.4,
                say: "It changes its ways...".into(),
                skills: vec![extra],
                mods: BTreeMap::from([("attack_speed".into(), 25.0), ("cast_speed".into(), 25.0)]),
                ..Default::default()
            },
            PhaseDef {
                at: 0.15,
                say: "Enraged!".into(),
                mods: BTreeMap::from([("damage_more".into(), 50.0)]),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

/// The spawnable form of a boss at a level.
pub fn boss_spec(d: &Data, b: &BossDef, level: u32) -> Result<MonsterSpec, String> {
    let opts = GenomeOpts { body: b.body, archetype: b.archetype.clone(), element: b.element, parts: None };
    let mut g = Genome::generate(d, b.seed, level, &opts)?;
    if b.scale > 0.0 {
        g.puppet.scale = b.scale;
    }
    if !b.parts.is_empty() {
        g.puppet.parts = b.parts.clone();
    }
    if !b.look.is_empty() {
        let mut table = toml::Table::try_from(&g.puppet).map_err(|e| e.to_string())?;
        for (k, v) in &b.look {
            if !table.contains_key(k) {
                return Err(format!("boss '{}': unknown look setting '{k}'", b.key));
            }
            table.insert(k.clone(), v.clone());
        }
        g.puppet = toml::Value::Table(table).try_into().map_err(|e: toml::de::Error| e.to_string())?;
    }
    for s in &b.skills {
        if d.skill_id(s).is_none() {
            return Err(format!("boss '{}': unknown skill '{s}'", b.key));
        }
        if !g.skills.contains(s) {
            g.skills.push(s.clone());
        }
    }
    let mut spec = g.spec(d);
    spec.key = format!("boss:{}", b.key);
    spec.name = b.name.clone();
    spec.life = g.life.max(1.0) * if b.life > 0.0 { b.life } else { 1.0 };
    spec.damage = g.damage.max(0.8) * if b.damage > 0.0 { b.damage } else { 1.0 };
    spec.speed = g.speed.max(0.7);
    spec.xp = 3.0;
    Ok(spec)
}

impl Game {
    /// Spawns a boss (designed key, or "gen:<seed>").
    pub fn spawn_boss(&mut self, sim: &mut Sim, key: &str, level: u32, feet: Vec3) -> Option<EntityId> {
        let d = data();
        let def = boss_def(&d, key, level)?;
        let spec = boss_spec(&d, &def, level).ok()?;
        let pack = self.next_pack;
        self.next_pack += 1;
        let id =
            super::spawn_actor(sim, self, &spec, &def.name, level, Rarity::Unique, feet, pack, Default::default(), Vec::new())?;
        if let Some(a) = self.actors.get_mut(&id) {
            a.boss = Some(BossState { key: def.key.clone(), phase: 0, seed: def.seed });
            a.immovable = true;
            if let Some(b) = a.brain.as_mut() {
                b.aggro = true;
            }
        }
        let title = if def.title.is_empty() { String::new() } else { format!(", {}", def.title) };
        self.say(format!("{}{}", def.name, title), 3.5);
        Some(id)
    }

    /// Bosses move to their next phase as their life drops.
    pub(crate) fn update_bosses(&mut self, sim: &mut Sim, events: &mut Vec<SimEvent>) {
        let d = data();
        let ids: Vec<EntityId> = self.actors.iter().filter(|(_, a)| a.boss.is_some() && !a.dead).map(|(id, _)| *id).collect();
        for id in ids {
            let a = &self.actors[&id];
            let st = a.boss.clone().unwrap();
            let Some(def) = boss_def(&d, &st.key, a.level) else { continue };
            let Some(ph) = def.phases.get(st.phase as usize) else { continue };
            if a.life > a.sheet.life_max * ph.at {
                continue;
            }
            let ph = ph.clone();
            let level = a.level;
            let Some((feet, h)) = feet_of(sim, id) else { continue };
            let a = self.actors.get_mut(&id).unwrap();
            a.boss.as_mut().unwrap().phase += 1;
            for s in &ph.skills {
                if let Some(sid) = d.skill_id(s) {
                    if !a.skills.contains(&sid) {
                        a.skills.push(sid);
                    }
                }
            }
            for (k, v) in &ph.mods {
                if let Some(s) = Stat::from_key(k) {
                    a.mods.add(s, *v);
                }
            }
            if let Some(p) = ph.power {
                a.powers.push(p);
            }
            a.recompute();
            let team = a.team;
            if !ph.say.is_empty() {
                self.say(format!("{}: {}", def.name, ph.say), 3.0);
            }
            self.effects.push(Effect {
                kind: EffectKind::Ring,
                pos: feet + Vec3::Y * 0.1,
                radius: 7.0,
                t: 0.0,
                dur: 0.8,
                color: [1.0, 0.3, 0.2],
                team,
                dmg: None,
                dir: Vec3::Z,
                angle: 360.0,
            });
            self.shake = (self.shake + 0.6 * sim.config.difficulty.shake).min(1.5);
            events.push(SimEvent::Slam { pos: feet, radius: 6.0 });
            if let Some(s) = &ph.summon {
                if s.family == "brood" {
                    self.spawn_brood(sim, id, s.count);
                } else if let Some(spec) = d.family(&s.family).map(|f| f.spec()) {
                    let pack = self.actors[&id].pack;
                    for i in 0..s.count {
                        let ang = i as f32 / s.count.max(1) as f32 * std::f32::consts::TAU;
                        let at = feet + Vec3::new(ang.cos(), 0.0, ang.sin()) * 3.0;
                        if let Some(m) = super::spawn_spec_into(sim, self, &spec, level, Rarity::Normal, at, pack) {
                            if let Some(x) = self.actors.get_mut(&m) {
                                x.master = Some(id);
                                x.xp *= 0.3;
                                if let Some(b) = x.brain.as_mut() {
                                    b.aggro = true;
                                }
                            }
                        }
                    }
                }
            }
            let _ = h;
        }
    }

    /// A summoner's brood: small swarmers born from its own genome (same element).
    pub(crate) fn spawn_brood(&mut self, sim: &mut Sim, master: EntityId, n: u32) {
        let d = data();
        let Some(m) = self.actors.get(&master) else { return };
        let Some((feet, _)) = feet_of(sim, master) else { return };
        let alive = self.actors.values().filter(|x| x.master == Some(master) && !x.dead).count();
        if alive >= 10 {
            return;
        }
        let (level, pack) = (m.level, m.pack);
        let element = m
            .tweaks
            .iter()
            .find(|t| t.field == super::skills::TweakField::Element)
            .map(|t| Element::ALL[(t.value.max(0.0) as usize).min(4)])
            .unwrap_or(Element::Physical);
        let seed = m.genome.unwrap_or(master.0 as u64).wrapping_mul(0x9e37_79b9).wrapping_add(0x5eed);
        let opts = GenomeOpts { archetype: Some("swarm".into()), element: Some(element), ..Default::default() };
        let spec = match Genome::generate(&d, seed, level, &opts) {
            Ok(g) => g.spec(&d),
            Err(_) => match d.family("skitterer") {
                Some(f) => f.spec(),
                None => return,
            },
        };
        for i in 0..n.min(10 - alive as u32) {
            let ang = i as f32 / n.max(1) as f32 * std::f32::consts::TAU + sim.state.rng.range(0.0, 0.6);
            let at = feet + Vec3::new(ang.cos(), 0.0, ang.sin()) * 2.0;
            if let Some(id) = super::spawn_spec_into(sim, self, &spec, level, Rarity::Normal, at, pack) {
                if let Some(x) = self.actors.get_mut(&id) {
                    x.master = Some(master);
                    x.xp *= 0.3;
                    if let Some(b) = x.brain.as_mut() {
                        b.aggro = true;
                    }
                }
                self.effects.push(Effect {
                    kind: EffectKind::Burst,
                    pos: at,
                    radius: 1.5,
                    t: 0.0,
                    dur: 0.3,
                    color: element.color(),
                    team: Team::Monster,
                    dmg: None,
                    dir: Vec3::Z,
                    angle: 0.0,
                });
            }
        }
    }

    /// On death, a burst (after a short warning so it can be dodged).
    pub(crate) fn death_burst(&mut self, sim: &mut Sim, team: Team, at: Vec3, life: f32, p: Power) {
        let d = data();
        let skill = d.skill_id("power_corpse").unwrap_or(0);
        let k = if team == Team::Hero { sim.config.difficulty.player_damage } else { sim.config.difficulty.enemy_damage };
        let mut dmg = Damage { skill, knockback: 2.5, ailment_mult: 1.0, ..Default::default() };
        dmg.amount[1] = life * p.a / 100.0 * k;
        dmg.ailment[1] = 0.3;
        self.effects.push(Effect {
            kind: EffectKind::Delayed,
            pos: at,
            radius: p.b.max(1.0),
            t: 0.0,
            dur: 0.65,
            color: [1.0, 0.45, 0.12],
            team,
            dmg: Some(dmg),
            dir: Vec3::Z,
            angle: 360.0,
        });
    }

    /// Summoners call their brood; the enraged get angry.
    pub(crate) fn tick_monster_powers(&mut self, sim: &mut Sim, dt: f32) {
        let ids: Vec<EntityId> = self
            .actors
            .iter()
            .filter(|(_, a)| !a.dead && a.team == Team::Monster && !a.powers.is_empty())
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let a = self.actors.get_mut(&id).unwrap();
            let aggro = a.brain.as_ref().is_some_and(|b| b.aggro);
            if let Some(p) = a.power(PowerKind::Summon).filter(|_| aggro) {
                a.power_t += dt;
                if a.power_t >= p.b.max(1.0) {
                    a.power_t = 0.0;
                    self.spawn_brood(sim, id, p.a.max(1.0) as u32);
                }
            }
            let a = self.actors.get_mut(&id).unwrap();
            if let Some(p) = a.power(PowerKind::Enrage) {
                if a.life < a.sheet.life_max * p.a / 100.0 && !a.buffs.iter().any(|b| b.name == "Enraged") {
                    let mods = super::stats::Mods::default().with(Stat::DamageMore, p.b).with(Stat::AttackSpeed, p.b * 0.5);
                    a.buffs.push(Buff { kind: BuffKind::Fury, name: "Enraged".into(), time: 1.0e6, mods });
                    a.recompute();
                    if let Some((feet, h)) = feet_of(sim, id) {
                        self.float_text(feet + Vec3::Y * (h + 0.6), "Enraged!");
                    }
                }
            }
        }
    }
}
