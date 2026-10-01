//! Powers: rule-bending effects carried by unique items (and later passive keystones and
//! monster affixes). Any actor can have them, so a monster wearing "corpse burst" works the
//! same as the hero. Each power is a kind and two numbers `a` and `b` (see game/uniques.toml
//! for what they mean); the hooks below are called from combat at the moments they act.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::combat::*;
use super::data::{Behavior, SkillDef, data};
use super::stats::{Mods, Stat};
use super::{Game, feet_of, flat};
use crate::entity::EntityId;
use crate::frame::SimEvent;
use crate::sim::Sim;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerKind {
    #[default]
    CorpseBurst,
    FrostCrits,
    BloodMagic,
    Execute,
    FireTrail,
    OrbitBlades,
    EchoStrike,
    DodgeReset,
    StormCall,
    BlockNova,
    IgniteAura,
    StandFirm,
    MeteorSlam,
    Frenzy,
    ManaShield,
    Quickening,
    /// All damage becomes element `a` (1 fire, 2 cold, 3 lightning, 4 poison).
    Convert,
    /// On death: bursts for a% of its life (radius b m) after a short warning.
    DeathBurst,
    /// Calls a brood of a creatures every b seconds while fighting.
    Summon,
    /// Below a% life: b% more damage and faster attacks.
    Enrage,
}

impl PowerKind {
    pub const ALL: [PowerKind; 20] = [
        PowerKind::CorpseBurst,
        PowerKind::FrostCrits,
        PowerKind::BloodMagic,
        PowerKind::Execute,
        PowerKind::FireTrail,
        PowerKind::OrbitBlades,
        PowerKind::EchoStrike,
        PowerKind::DodgeReset,
        PowerKind::StormCall,
        PowerKind::BlockNova,
        PowerKind::IgniteAura,
        PowerKind::StandFirm,
        PowerKind::MeteorSlam,
        PowerKind::Frenzy,
        PowerKind::ManaShield,
        PowerKind::Quickening,
        PowerKind::Convert,
        PowerKind::DeathBurst,
        PowerKind::Summon,
        PowerKind::Enrage,
    ];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Power {
    pub kind: PowerKind,
    #[serde(default)]
    pub a: f32,
    #[serde(default)]
    pub b: f32,
}

impl Power {
    pub fn new(kind: PowerKind, a: f32, b: f32) -> Self {
        Self { kind, a, b }
    }

    /// How the power reads on a tooltip.
    pub fn describe(&self) -> String {
        let (a, b) = (num(self.a), num(self.b));
        match self.kind {
            PowerKind::CorpseBurst => format!("{a}% chance on kill: the corpse explodes for {b}% of its Life as Fire Damage"),
            PowerKind::FrostCrits => format!("Critical Strikes Freeze. {a}% more Critical Damage against Frozen enemies"),
            PowerKind::BloodMagic => format!("Skills cost Life instead of Mana. {a}% of Mana is added to Life"),
            PowerKind::Execute => format!("Your hits kill enemies below {a}% Life (not bosses)"),
            PowerKind::FireTrail => {
                format!("Dodging leaves burning ground for {a}s that burns for {b}% of weapon damage per second")
            }
            PowerKind::OrbitBlades => format!("{a} spectral blades orbit you, each hit dealing {b}% of weapon damage"),
            PowerKind::EchoStrike => format!("Melee skills strike again a moment later for {a}% damage"),
            PowerKind::DodgeReset => format!("Kills have a {a}% chance to reset your Dodge"),
            PowerKind::StormCall => {
                format!("Spells call lightning on {a} enemies near the target for {b}% of the spell's damage")
            }
            PowerKind::BlockNova => format!("Blocking releases a frost nova ({a} m) dealing {b}% of weapon damage"),
            PowerKind::IgniteAura => format!("Enemies within {a} m burn for {b}% of your maximum Life per second"),
            PowerKind::StandFirm => format!("Cannot be knocked back. {a}% less damage taken while using skills"),
            PowerKind::MeteorSlam => format!("Slams and leaps call down a meteor for {a}% damage"),
            PowerKind::Frenzy => format!("Kills grant {a}% Attack and Movement Speed for 4s (up to 3 times)"),
            PowerKind::ManaShield => format!("{a}% of damage taken is paid from Mana first"),
            PowerKind::Quickening => format!("Kills take {a}s off all skill cooldowns"),
            PowerKind::DeathBurst => format!("On death, bursts for {a}% of its Life ({b} m)"),
            PowerKind::Summon => format!("Calls {a} of its brood every {b}s"),
            PowerKind::Enrage => format!("Below {a}% Life: {b}% more Damage and faster attacks"),
            PowerKind::Convert => {
                format!("All your damage is converted to {}", super::data::Element::ALL[(self.a.max(0.0) as usize).min(4)].name())
            }
        }
    }
}

fn num(v: f32) -> String {
    if (v - v.round()).abs() < 0.05 { format!("{}", v.round() as i64) } else { format!("{v:.1}") }
}

impl Actor {
    /// The actor's power of a kind (several sources add their `a`; `b` takes the largest).
    pub fn power(&self, k: PowerKind) -> Option<Power> {
        let mut out: Option<Power> = None;
        for p in self.powers.iter().filter(|p| p.kind == k) {
            out = Some(match out {
                None => *p,
                Some(o) => Power { kind: k, a: o.a + p.a, b: o.b.max(p.b) },
            });
        }
        out
    }
    pub fn has_power(&self, k: PowerKind) -> bool {
        self.powers.iter().any(|p| p.kind == k)
    }
}

/// A hit or effect from a power, rolled like a skill (the internal `power_*` skills).
fn power_damage(g: &Game, sim: &mut Sim, id: EntityId, skill: &str, mult: f32) -> Option<(SkillDef, Damage)> {
    let d = data();
    let def = d.skill(d.skill_id(skill)?).clone();
    let dmg = super::skills::roll_damage(g, sim, id, &def, mult);
    Some((def, dmg))
}

fn delayed(g: &mut Game, team: Team, pos: Vec3, radius: f32, delay: f32, color: [f32; 3], dmg: Damage) {
    g.effects.push(Effect {
        kind: EffectKind::Delayed,
        pos,
        radius,
        t: 0.0,
        dur: delay,
        color,
        team,
        dmg: Some(dmg),
        dir: Vec3::Z,
        angle: 360.0,
    });
}

/// A pending follow-up strike (echo strike).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Echo {
    pub actor: EntityId,
    pub skill: u16,
    pub cast: Cast,
    pub mult: f32,
    pub at: f32,
}

impl Game {
    /// A skill just fired (its hit landed or its projectiles left). `center` is where it hit.
    pub(crate) fn power_on_fire(
        &mut self,
        sim: &mut Sim,
        id: EntityId,
        def: &SkillDef,
        cast: &Cast,
        center: Vec3,
        echo: bool,
        events: &mut Vec<SimEvent>,
    ) {
        let Some(a) = self.actors.get(&id) else { return };
        let team = a.team;
        let area = a.sheet.area.max(0.1).sqrt();
        let (echo_p, meteor_p, storm_p) =
            (a.power(PowerKind::EchoStrike), a.power(PowerKind::MeteorSlam), a.power(PowerKind::StormCall));
        if !echo && def.behavior == Behavior::Melee {
            if let Some(p) = echo_p {
                self.echoes.push(Echo {
                    actor: id,
                    skill: cast.skill,
                    cast: cast.clone(),
                    mult: p.a / 100.0,
                    at: self.time + 0.14,
                });
            }
        }
        if matches!(def.behavior, Behavior::Slam | Behavior::Leap) {
            if let Some(p) = meteor_p {
                if let Some((pd, dmg)) = power_damage(self, sim, id, "power_meteor", p.a / 100.0) {
                    let r = def.radius.max(2.0) * area;
                    delayed(self, team, center, r, 0.55, pd.rgb(), dmg);
                }
            }
        }
        if def.has("spell") {
            if let Some(p) = storm_p {
                let near: Vec<Vec3> = {
                    let mut v: Vec<(f32, Vec3)> = self
                        .actors
                        .iter()
                        .filter(|(_, o)| !o.dead && team.hostile(o.team))
                        .filter_map(|(oid, _)| feet_of(sim, *oid).map(|f| f.0))
                        .map(|f| (flat(f - cast.target).length(), f))
                        .filter(|x| x.0 < 7.0)
                        .collect();
                    v.sort_by(|a, b| a.0.total_cmp(&b.0));
                    v.into_iter().take(p.a.max(1.0) as usize).map(|x| x.1).collect()
                };
                for at in near {
                    if let Some((pd, dmg)) = power_damage(self, sim, id, "power_storm", p.b / 100.0) {
                        delayed(self, team, at, 1.4, 0.3, pd.rgb(), dmg);
                    }
                }
                let _ = events;
            }
        }
    }

    /// Something died with `killer` credited: on-kill powers.
    pub(crate) fn power_on_kill(&mut self, sim: &mut Sim, killer: EntityId, at: Vec3, life_max: f32, events: &mut Vec<SimEvent>) {
        let Some(k) = self.actors.get(&killer) else { return };
        if k.dead {
            return;
        }
        let team = k.team;
        let powers = k.powers.clone();
        let mut refresh = false;
        for p in powers {
            match p.kind {
                PowerKind::CorpseBurst if sim.state.rng.f32() * 100.0 < p.a => {
                    let d = data();
                    let Some(skill) = d.skill_id("power_corpse") else { continue };
                    let mut dmg = Damage { skill, source: Some(killer), knockback: 2.0, ailment_mult: 1.0, ..Default::default() };
                    let k =
                        if team == Team::Hero { sim.config.difficulty.player_damage } else { sim.config.difficulty.enemy_damage };
                    dmg.amount[1] = life_max * p.b / 100.0 * k;
                    dmg.ailment[1] = 0.25;
                    delayed(self, team, at, 3.0, 0.08, [1.0, 0.45, 0.12], dmg);
                    events.push(SimEvent::Blast { pos: at, element: 1 });
                }
                PowerKind::DodgeReset if sim.state.rng.f32() * 100.0 < p.a => {
                    if let Some(k) = self.actors.get_mut(&killer) {
                        if k.dodge_cd > 0.0 {
                            k.dodge_cd = 0.0;
                            self.float_text(at + Vec3::Y * 2.0, "Dodge ready");
                        }
                    }
                }
                PowerKind::Quickening => {
                    if let Some(k) = self.actors.get_mut(&killer) {
                        for c in &mut k.cooldowns {
                            c.1 = (c.1 - p.a).max(0.0);
                        }
                    }
                }
                PowerKind::Frenzy => {
                    if let Some(k) = self.actors.get_mut(&killer) {
                        let mods = Mods::default().with(Stat::AttackSpeed, p.a).with(Stat::MoveSpeed, p.a);
                        let stacks = k.buffs.iter().filter(|b| b.kind == BuffKind::Frenzy).count();
                        if stacks >= 3 {
                            if let Some(b) =
                                k.buffs.iter_mut().filter(|b| b.kind == BuffKind::Frenzy).min_by(|a, b| a.time.total_cmp(&b.time))
                            {
                                b.time = 4.0;
                            }
                        } else {
                            k.buffs.push(Buff { kind: BuffKind::Frenzy, name: "Frenzy".into(), time: 4.0, mods });
                            refresh = true;
                        }
                    }
                }
                _ => {}
            }
        }
        if refresh && Some(killer) == self.hero_id {
            super::refresh_hero(sim, self, false);
        }
    }

    /// Every tick: burning trails, auras, orbiting blades, echo strikes.
    pub(crate) fn power_tick(&mut self, sim: &mut Sim, dt: f32, events: &mut Vec<SimEvent>) {
        let before = self.power_clock;
        self.power_clock += dt;
        let half_second = (before / 0.5).floor() != (self.power_clock / 0.5).floor();
        let trail_step = (before / 0.07).floor() != (self.power_clock / 0.07).floor();
        // Echo strikes due now.
        let due: Vec<Echo> = {
            let t = self.time;
            let (now, later): (Vec<Echo>, Vec<Echo>) = std::mem::take(&mut self.echoes).into_iter().partition(|e| e.at <= t);
            self.echoes = later;
            now
        };
        for e in due {
            if let Some(def) = self.actors.get(&e.actor).filter(|a| !a.dead).map(|a| super::skills::skill_of(a, e.skill)) {
                super::skills::fire_cast(self, sim, e.actor, &def, &e.cast, e.mult, true, events);
            }
        }
        let ids: Vec<EntityId> = self.actors.iter().filter(|(_, a)| !a.powers.is_empty() && !a.dead).map(|(id, _)| *id).collect();
        for id in ids {
            let Some((feet, _)) = feet_of(sim, id) else { continue };
            let a = &self.actors[&id];
            let team = a.team;
            // Fire trail while rolling.
            if let Some(p) = a.power(PowerKind::FireTrail) {
                let rolling = sim
                    .state
                    .entities
                    .get(id)
                    .and_then(|e| e.character.as_ref())
                    .is_some_and(|c| c.dash_time > 0.0 && c.dash_roll);
                if rolling && trail_step {
                    if let Some((pd, dmg)) = power_damage(self, sim, id, "power_ember", p.b / 100.0 * 0.25) {
                        self.effects.push(Effect {
                            kind: EffectKind::Field,
                            pos: feet,
                            radius: 1.1,
                            t: 0.0,
                            dur: p.a.max(0.5),
                            color: pd.rgb(),
                            team,
                            dmg: Some(dmg),
                            dir: Vec3::Z,
                            angle: 360.0,
                        });
                    }
                }
            }
            // Burning aura.
            let a = &self.actors[&id];
            if let Some(p) = a.power(PowerKind::IgniteAura).filter(|_| half_second) {
                let dps = a.sheet.life_max * p.b / 100.0;
                let k = if team == Team::Hero { sim.config.difficulty.player_damage } else { sim.config.difficulty.enemy_damage };
                let near: Vec<EntityId> = self
                    .actors
                    .iter()
                    .filter(|(_, o)| !o.dead && team.hostile(o.team))
                    .filter_map(|(oid, o)| feet_of(sim, *oid).map(|f| (*oid, flat(f.0 - feet).length() - o.radius)))
                    .filter(|x| x.1 <= p.a)
                    .map(|x| x.0)
                    .collect();
                for o in near {
                    if let Some(t) = self.actors.get_mut(&o) {
                        let dps = dps * k * (1.0 - t.sheet.res[1] / 100.0);
                        if t.ailments.ignite.time < 0.6 || t.ailments.ignite.dps < dps {
                            t.ailments.ignite = Dot { dps: dps.max(t.ailments.ignite.dps), time: 1.0, source: Some(id) };
                        }
                    }
                }
                if (self.power_clock / 0.5).floor() as i64 % 2 == 0 {
                    self.effects.push(Effect {
                        kind: EffectKind::Ring,
                        pos: feet + Vec3::Y * 0.1,
                        radius: p.a,
                        t: 0.0,
                        dur: 0.9,
                        color: [1.0, 0.4, 0.1],
                        team,
                        dmg: None,
                        dir: Vec3::Z,
                        angle: 360.0,
                    });
                }
            }
        }
        self.sync_orbits(sim);
        let _ = events;
    }

    /// Blocking with a frost-nova power: a burst of cold around the blocker.
    pub(crate) fn block_nova(&mut self, sim: &mut Sim, id: EntityId, feet: Vec3, p: Power, events: &mut Vec<SimEvent>) {
        let Some(team) = self.actors.get(&id).map(|a| a.team) else { return };
        if let Some((pd, dmg)) = power_damage(self, sim, id, "power_frost", p.b / 100.0) {
            delayed(self, team, feet, p.a, 0.02, pd.rgb(), dmg);
            self.effects.push(Effect {
                kind: EffectKind::Ring,
                pos: feet,
                radius: p.a,
                t: 0.0,
                dur: 0.45,
                color: pd.rgb(),
                team,
                dmg: None,
                dir: Vec3::Z,
                angle: 360.0,
            });
            events.push(SimEvent::Spell { pos: feet, element: 2 });
        }
    }

    /// Keeps the right number of orbiting blades around actors with that power.
    fn sync_orbits(&mut self, sim: &mut Sim) {
        let want: Vec<(EntityId, u32, f32)> = self
            .actors
            .iter()
            .filter(|(_, a)| !a.dead)
            .filter_map(|(id, a)| a.power(PowerKind::OrbitBlades).map(|p| (*id, p.a.max(0.0) as u32, p.b)))
            .collect();
        // Remove blades of owners that lost the power or died.
        self.shots.retain(|s| s.orbit.is_none() || want.iter().any(|w| w.0 == s.owner && w.1 > 0));
        for (owner, n, b) in want {
            let have = self.shots.iter().filter(|s| s.owner == owner && s.orbit.is_some()).count() as u32;
            if have == n {
                continue;
            }
            self.shots.retain(|s| !(s.owner == owner && s.orbit.is_some()));
            let Some((feet, _)) = feet_of(sim, owner) else { continue };
            let team = self.actors[&owner].team;
            let d = data();
            let skill = d.skill_id("power_blade").unwrap_or(0);
            let color = d.skill(skill).rgb();
            for i in 0..n {
                let angle = i as f32 / n.max(1) as f32 * std::f32::consts::TAU;
                self.shots.push(Shot {
                    owner,
                    team,
                    skill,
                    pos: feet + Vec3::Y,
                    vel: Vec3::ZERO,
                    radius: 0.45,
                    life: 1.0,
                    dmg: Damage { skill, source: Some(owner), ..Default::default() },
                    pierce: 0,
                    chain: 0,
                    explode: 0.0,
                    hit: Vec::new(),
                    color,
                    orbit: Some(Orbit { angle, radius: 1.55, speed: 4.2, rehit: 0.0, mult: b / 100.0 }),
                });
            }
        }
    }
}

/// An orbiting projectile: circles its owner and hits each enemy again every half second.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Orbit {
    pub angle: f32,
    pub radius: f32,
    /// Radians per second.
    pub speed: f32,
    pub rehit: f32,
    pub mult: f32,
}

/// Moves an orbiting blade and lands its hits. Returns false if its owner is gone.
pub(crate) fn update_orbit(g: &mut Game, sim: &mut Sim, s: &mut Shot, dt: f32, events: &mut Vec<SimEvent>) -> bool {
    let Some(mut o) = s.orbit else { return false };
    let Some((feet, h)) = feet_of(sim, s.owner) else { return false };
    o.angle += o.speed * dt;
    o.rehit -= dt;
    if o.rehit <= 0.0 {
        s.hit.clear();
        o.rehit = 0.5;
    }
    let center = feet + Vec3::Y * (h * 0.55);
    let pos = center + Vec3::new(o.angle.sin(), 0.0, o.angle.cos()) * o.radius;
    s.vel = (pos - s.pos) / dt.max(1e-4);
    s.pos = pos;
    s.life = 1.0;
    s.orbit = Some(o);
    let team = s.team;
    let hits: Vec<EntityId> = g
        .actors
        .iter()
        .filter(|(id, a)| !a.dead && team.hostile(a.team) && !s.hit.contains(id))
        .filter_map(|(id, a)| feet_of(sim, *id).map(|f| (*id, flat(f.0 - pos).length() - a.radius)))
        .filter(|x| x.1 <= s.radius)
        .map(|x| x.0)
        .collect();
    for t in hits {
        s.hit.push(t);
        if let Some((_, dmg)) = power_damage(g, sim, s.owner, "power_blade", o.mult) {
            g.hit(sim, t, &dmg, center, events);
        }
    }
    true
}
