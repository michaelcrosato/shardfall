//! Skills in action: starting a cast (cost, cooldown, speed), steering the ones that move you
//! (leap, dash, charge), landing hits when the swing connects, projectiles and ground effects.
//! Hero skills and monster attacks run through the same code.

use glam::Vec3;

use super::combat::*;
use super::data::{Behavior, SkillDef, data};
use super::{Game, TeleShape, Telegraph, feet_of, flat};
use crate::entity::EntityId;
use crate::frame::SimEvent;
use crate::sim::Sim;

/// Seconds a dash or charge takes once it fires.
const DASH_TIME: f32 = 0.18;
const CHARGE_SPEED: f32 = 14.0;

fn yaw_dir(y: f32) -> Vec3 {
    Vec3::new(y.sin(), 0.0, y.cos())
}

/// Starts `skill` toward `target` if the actor can (alive, off cooldown, enough mana).
pub fn try_cast(g: &mut Game, sim: &mut Sim, id: EntityId, skill: u16, target: Vec3) -> bool {
    let d = data();
    let def = d.skill(skill);
    let Some((feet, _)) = feet_of(sim, id) else { return false };
    let facing = sim.state.entities.get(id).and_then(|e| e.character.as_ref()).map(|c| c.facing).unwrap_or(0.0);
    let enemy_speed = sim.config.difficulty.enemy_speed;
    let weapon_aps = g.hero.weapon.aps;
    let is_hero = Some(id) == g.hero_id;
    let Some(a) = g.actors.get_mut(&id) else { return false };
    if a.dead || a.frozen() || a.cooldown(skill) > 0.0 {
        return false;
    }
    let cost = def.cost * a.sheet.mana_cost;
    let blood = a.has_power(super::powers::PowerKind::BloodMagic);
    let pool = if blood { a.life - 1.0 } else { a.mana };
    if pool < cost {
        if is_hero {
            g.float_text(feet + Vec3::Y * 2.2, if blood { "Not enough life" } else { "Not enough mana" });
        }
        return false;
    }
    if blood {
        a.life -= cost;
    } else {
        a.mana -= cost;
    }
    if def.cooldown > 0.0 {
        a.set_cooldown(skill, def.cooldown / a.sheet.cooldown.max(0.1));
    }
    let speed =
        if def.is_attack() { a.sheet.attack_speed * if is_hero { weapon_aps / 1.4 } else { 1.0 } } else { a.sheet.cast_speed }
            * if a.team == Team::Monster { enemy_speed } else { 1.0 };
    let dur = def.time / speed.max(0.1);
    let combo = if def.combo > 1 && a.combo_timer > 0.0 { (a.combo + 1) % def.combo } else { 0 };
    a.combo = combo;
    a.combo_timer = 0.6;
    let mut to = flat(target - feet);
    if to.length() < 0.1 {
        to = yaw_dir(facing);
    }
    let dir = to.normalize();
    let mut target = Vec3::new(target.x, feet.y, target.z);
    if def.behavior == Behavior::Leap && to.length() > def.range {
        target = feet + dir * def.range;
    }
    a.cast = Some(Cast {
        skill,
        t: 0.0,
        dur,
        hit_at: dur * def.hit,
        dir,
        target,
        origin: feet,
        fired: false,
        combo,
        side: if combo % 2 == 0 { 1.0 } else { -1.0 },
        hits: Vec::new(),
    });
    true
}

/// Moves actors whose skill carries them (each tick before characters move).
pub fn steer_cast(g: &mut Game, sim: &mut Sim, id: EntityId) {
    let d = data();
    let Some(a) = g.actors.get(&id) else { return };
    let Some(c) = &a.cast else { return };
    let def = d.skill(c.skill);
    let Some(ch) = sim.state.entities.get_mut(id).and_then(|e| e.character.as_mut()) else { return };
    match def.behavior {
        Behavior::Leap => {
            let start = c.dur * 0.15;
            if c.t >= start && c.t < c.hit_at {
                let left = (c.hit_at - c.t).max(1e-3);
                let from = ch.anim.facing; // keep facing the landing point
                let _ = from;
                let span = c.hit_at - start;
                ch.dash_vel = (c.target - c.origin) / span.max(1e-3);
                ch.dash_time = left;
                ch.dash_roll = false;
            }
        }
        Behavior::Dash | Behavior::Charge if c.fired => {
            let len = if def.behavior == Behavior::Dash { DASH_TIME } else { def.range / CHARGE_SPEED };
            if c.t < c.hit_at + len {
                let speed = if def.behavior == Behavior::Dash { def.range / DASH_TIME } else { CHARGE_SPEED };
                ch.dash_vel = c.dir * speed;
                ch.dash_time = (c.hit_at + len - c.t).max(1e-3);
                ch.dash_roll = false;
            }
        }
        _ => {}
    }
}

/// Advances an actor's cast; lands its hit at the right moment and starts the buffered one.
pub fn advance_cast(g: &mut Game, sim: &mut Sim, id: EntityId, dt: f32, events: &mut Vec<SimEvent>) {
    let d = data();
    let Some(a) = g.actors.get_mut(&id) else { return };
    if a.dead {
        return;
    }
    if a.frozen() {
        return;
    }
    let Some(c) = &mut a.cast else { return };
    c.t += dt;
    let def = d.skill(c.skill).clone();
    let fire = !c.fired && c.t >= c.hit_at;
    if fire {
        c.fired = true;
    }
    let (t, dur, hit_at) = (c.t, c.dur, c.hit_at);
    if fire {
        fire_skill(g, sim, id, &def, events);
    }
    // Dashes and charges cut through whatever they pass.
    if matches!(def.behavior, Behavior::Dash | Behavior::Charge) && t >= hit_at {
        let len = if def.behavior == Behavior::Dash { DASH_TIME } else { def.range / CHARGE_SPEED };
        if t <= hit_at + len + 0.02 {
            sweep_hits(g, sim, id, &def, events);
            if def.behavior == Behavior::Dash && Some(id) == g.hero_id {
                if let Some(a) = g.actors.get_mut(&id) {
                    a.iframes = a.iframes.max(0.05);
                }
            }
        }
    }
    let Some(a) = g.actors.get_mut(&id) else { return };
    if t >= dur {
        a.cast = None;
        if let Some((skill, target)) = a.queued.take() {
            try_cast(g, sim, id, skill, target);
        }
    }
}

/// Rolls a hit's damage for an attacker using a skill (`mult`: combo and other bonuses).
pub fn roll_damage(g: &Game, sim: &mut Sim, id: EntityId, def: &SkillDef, mult: f32) -> Damage {
    let d = data();
    let skill = d.skill_id(&def.key).unwrap_or(0);
    let Some(a) = g.actors.get(&id) else { return Damage::default() };
    let rng = &mut sim.state.rng;
    let sheet = &a.sheet;
    let is_hero = Some(id) == g.hero_id;
    let mut amt = [0.0f32; 5];
    let crit_base;
    let el = def.element.index();
    if def.is_attack() {
        if is_hero {
            let w = &g.hero.weapon;
            amt[0] = rng.range(w.phys[0], w.phys[1]);
            crit_base = w.crit;
            use super::stats::Stat::*;
            for (i, s) in [(0, AddedPhys), (1, AddedFire), (2, AddedCold), (3, AddedLightning), (4, AddedPoison)] {
                let v = sheet.mods.get(s);
                if v > 0.0 {
                    amt[i] += v * rng.range(0.6, 1.4);
                }
            }
            if el != 0 {
                // Elemental attack skills convert the physical part.
                amt[el] += amt[0];
                amt[0] = 0.0;
            }
        } else {
            amt[el] = a.base_damage * rng.range(0.85, 1.15);
            crit_base = 5.0;
        }
        for x in &mut amt {
            *x *= def.effect;
        }
    } else {
        let v = if is_hero {
            let lvl = g.hero.skill_level(sheet, true, false);
            rng.range(def.base[0], def.base[1].max(def.base[0])) * super::combat::spell_scale(lvl)
                + sheet.mods.get(super::stats::Stat::AddedSpell) * rng.range(0.6, 1.4)
        } else {
            a.base_damage * def.effect * rng.range(0.85, 1.15)
        };
        amt[el] = v;
        crit_base = 6.0;
    }
    let tags = def.tag_refs();
    for (i, x) in amt.iter_mut().enumerate() {
        *x *= sheet.damage_mult(&tags, i) * mult;
    }
    let diff = &sim.config.difficulty;
    let k = if a.team == Team::Hero { diff.player_damage } else { diff.enemy_damage };
    for x in &mut amt {
        *x *= k;
    }
    let crit = rng.f32() * 100.0 < crit_base * sheet.crit_inc + sheet.crit_flat;
    if crit {
        for x in &mut amt {
            *x *= sheet.crit_multi;
        }
    }
    let mut ailment = sheet.ailment;
    ailment[el] += def.ailment;
    Damage {
        amount: amt,
        crit,
        ailment,
        ailment_mult: sheet.ailment_inc,
        knockback: def.knockback,
        source: Some(id),
        skill,
        attack: def.is_attack(),
        melee: def.has("melee"),
    }
}

/// Hostile, living actors that `pred(feet, radius)` accepts.
fn targets(g: &Game, sim: &Sim, team: Team, pred: impl Fn(Vec3, f32) -> bool) -> Vec<(EntityId, Vec3)> {
    g.actors
        .iter()
        .filter(|(_, a)| !a.dead && team.hostile(a.team))
        .filter_map(|(id, a)| {
            let (feet, _) = feet_of(sim, *id)?;
            pred(feet, a.radius).then_some((*id, feet))
        })
        .collect()
}

/// Damages everything hostile in a circle (each target rolls its own damage).
pub fn circle_hit(
    g: &mut Game,
    sim: &mut Sim,
    id: EntityId,
    def: &SkillDef,
    center: Vec3,
    radius: f32,
    mult: f32,
    events: &mut Vec<SimEvent>,
) -> usize {
    let Some(team) = g.actors.get(&id).map(|a| a.team) else { return 0 };
    let hit = targets(g, sim, team, |f, r| flat(f - center).length() <= radius + r && (f.y - center.y).abs() < 3.0);
    for (t, _) in &hit {
        let dmg = roll_damage(g, sim, id, def, mult);
        g.hit(sim, *t, &dmg, center, events);
    }
    hit.len()
}

fn fire_skill(g: &mut Game, sim: &mut Sim, id: EntityId, def: &SkillDef, events: &mut Vec<SimEvent>) {
    let Some(c) = g.actors.get(&id).and_then(|a| a.cast.clone()) else { return };
    fire_cast(g, sim, id, def, &c, 1.0, false, events);
}

/// Lands a skill's hit (or launches its projectiles) for a cast. `extra` scales the damage;
/// `echo` marks a repeat (echo strikes) so it doesn't echo again.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fire_cast(
    g: &mut Game,
    sim: &mut Sim,
    id: EntityId,
    def: &SkillDef,
    c: &Cast,
    extra: f32,
    echo: bool,
    events: &mut Vec<SimEvent>,
) {
    let Some((feet, height)) = feet_of(sim, id) else { return };
    let Some(a) = g.actors.get(&id) else { return };
    let team = a.team;
    let sheet = a.sheet.clone();
    let is_hero = Some(id) == g.hero_id;
    let reach = if is_hero { g.hero.weapon.reach } else { 1.0 };
    let area = sheet.area.max(0.1);
    let color = def.rgb();
    let el_color = def.element.color();
    let last = def.combo > 1 && c.combo + 1 == def.combo;
    let mult = if last { 1.5 } else { 1.0 } * extra;
    let mut center = feet;
    match def.behavior {
        Behavior::Melee => {
            let range = def.range
                * reach
                * area.sqrt()
                * if last { 1.25 } else { 1.0 }
                * if is_hero { 1.0 } else { a.radius / 0.42 * 0.5 + 0.5 };
            let half = (def.angle * if last { 1.2 } else { 1.0 }).to_radians() * 0.5;
            let dir = c.dir;
            center = feet + dir * range * 0.6;
            let hit = targets(g, sim, team, |f, r| {
                let to = flat(f - feet);
                let dist = to.length();
                dist <= range + r
                    && (dist < r + 0.3 || to.normalize_or_zero().dot(dir) >= half.cos())
                    && (f.y - feet.y).abs() < 2.5
            });
            for (t, _) in &hit {
                let mut dmg = roll_damage(g, sim, id, def, mult);
                if last {
                    dmg.knockback *= 1.8;
                }
                g.hit(sim, *t, &dmg, feet, events);
            }
            g.effects.push(Effect {
                kind: EffectKind::Ring,
                pos: feet + Vec3::Y * (height * 0.55),
                radius: range,
                t: 0.0,
                dur: 0.18,
                color,
                team,
                dmg: None,
                dir,
                angle: def.angle * if last { 1.2 } else { 1.0 },
            });
            events.push(SimEvent::Swing { pos: feet, heavy: def.knockback >= 2.5 || last });
        }
        Behavior::Slam | Behavior::Leap => {
            center = if def.behavior == Behavior::Slam { feet + c.dir * def.range * 0.8 } else { feet };
            let r = def.radius * area.sqrt();
            circle_hit(g, sim, id, def, center, r, mult, events);
            g.effects.push(Effect {
                kind: EffectKind::Ring,
                pos: center,
                radius: r,
                t: 0.0,
                dur: 0.45,
                color,
                team,
                dmg: None,
                dir: c.dir,
                angle: 360.0,
            });
            g.effects.push(Effect {
                kind: EffectKind::Burst,
                pos: center,
                radius: r,
                t: 0.0,
                dur: 0.3,
                color,
                team,
                dmg: None,
                dir: c.dir,
                angle: 0.0,
            });
            g.shake = (g.shake + def.shake * sim.config.difficulty.shake * if is_hero { 1.0 } else { 0.5 }).min(1.5);
            events.push(SimEvent::Slam { pos: center, radius: r });
        }
        Behavior::Nova => {
            let r = def.radius * area.sqrt();
            circle_hit(g, sim, id, def, feet, r, mult, events);
            g.effects.push(Effect {
                kind: EffectKind::Ring,
                pos: feet,
                radius: r,
                t: 0.0,
                dur: 0.5,
                color: el_color,
                team,
                dmg: None,
                dir: c.dir,
                angle: 360.0,
            });
            g.shake = (g.shake + def.shake * sim.config.difficulty.shake).min(1.5);
            events.push(SimEvent::Spell { pos: feet, element: def.element as u8 });
        }
        Behavior::Projectile => {
            let n = def.count.max(1) + if is_hero { sheet.proj_count } else { 0 };
            let spread = def.spread.to_radians();
            let base_yaw = c.dir.x.atan2(c.dir.z);
            let from = feet + Vec3::Y * (height * 0.65) + c.dir * 0.5;
            let speed = def.speed * if is_hero { sheet.proj_speed } else { 1.0 };
            for i in 0..n {
                let off = (i as f32 - (n - 1) as f32 * 0.5) * spread;
                let dir = yaw_dir(base_yaw + off);
                let dmg = roll_damage(g, sim, id, def, mult);
                g.shots.push(Shot {
                    owner: id,
                    team,
                    skill: dmg.skill,
                    pos: from,
                    vel: dir * speed,
                    radius: def.radius.max(0.1),
                    life: def.range / speed.max(1.0),
                    dmg,
                    pierce: def.pierce + if is_hero { sheet.pierce } else { 0 },
                    chain: if is_hero { sheet.chain } else { 0 },
                    explode: def.explode * area.sqrt(),
                    hit: Vec::new(),
                    color: if def.color == "#ffffff" { el_color } else { color },
                    orbit: None,
                });
            }
            events.push(SimEvent::Spell { pos: from, element: def.element as u8 });
            center = c.target;
        }
        Behavior::Dash | Behavior::Charge => {
            events.push(SimEvent::Swing { pos: feet, heavy: def.behavior == Behavior::Charge });
        }
    }
    g.power_on_fire(sim, id, def, c, center, echo, events);
}

/// Dashes and charges: everything along the way, once each.
fn sweep_hits(g: &mut Game, sim: &mut Sim, id: EntityId, def: &SkillDef, events: &mut Vec<SimEvent>) {
    let Some((feet, _)) = feet_of(sim, id) else { return };
    let Some(a) = g.actors.get(&id) else { return };
    let team = a.team;
    let already = a.cast.as_ref().map(|c| c.hits.clone()).unwrap_or_default();
    let width = def.radius.max(0.5);
    let hit: Vec<(EntityId, Vec3)> = targets(g, sim, team, |f, r| flat(f - feet).length() <= width + r)
        .into_iter()
        .filter(|(t, _)| !already.contains(t))
        .collect();
    for (t, _) in hit {
        if let Some(c) = g.actors.get_mut(&id).and_then(|a| a.cast.as_mut()) {
            c.hits.push(t);
        }
        let dmg = roll_damage(g, sim, id, def, 1.0);
        g.hit(sim, t, &dmg, feet, events);
        if def.behavior == Behavior::Charge {
            // A charge stops at the first thing it hits.
            if let Some(ch) = sim.state.entities.get_mut(id).and_then(|e| e.character.as_mut()) {
                ch.dash_time = 0.0;
            }
            if let Some(c) = g.actors.get_mut(&id).and_then(|a| a.cast.as_mut()) {
                c.t = c.t.max(c.hit_at + def.range / CHARGE_SPEED);
            }
            break;
        }
    }
}

/// Projectiles: fly, stop at walls, hit, pierce, chain, explode.
pub fn update_shots(g: &mut Game, sim: &mut Sim, dt: f32, events: &mut Vec<SimEvent>) {
    let d = data();
    let mut shots = std::mem::take(&mut g.shots);
    let mut keep = Vec::with_capacity(shots.len());
    for mut s in shots.drain(..) {
        if s.orbit.is_some() {
            if super::powers::update_orbit(g, sim, &mut s, dt, events) {
                keep.push(s);
            }
            continue;
        }
        let step = s.vel * dt;
        let len = step.length();
        let dir = step / len.max(1e-6);
        let mut end = false;
        let mut boom: Option<Vec3> = None;
        // Walls.
        let ignore = sim.state.entities.get(s.owner).and_then(|e| e.body);
        let wall = crate::projectile::static_hit(&sim.state.physics, s.pos, dir, len + s.radius, ignore);
        // Actors along the segment.
        let mut best: Option<(f32, EntityId)> = None;
        for (id, a) in &g.actors {
            if a.dead || !s.team.hostile(a.team) || s.hit.contains(id) || *id == s.owner {
                continue;
            }
            let Some((feet, h)) = feet_of(sim, *id) else { continue };
            let c = feet + Vec3::Y * (h * 0.55);
            let t = (c - s.pos).dot(dir).clamp(0.0, len);
            let p = s.pos + dir * t;
            let reach = a.radius + s.radius;
            if flat(p - c).length() <= reach && (p.y - c.y).abs() <= h * 0.6 + s.radius {
                if best.is_none_or(|b| t < b.0) {
                    best = Some((t, *id));
                }
            }
        }
        if let Some(w) = wall {
            if best.is_none_or(|b| w < b.0) {
                s.pos += dir * w;
                end = true;
                boom = Some(s.pos);
                best = None;
            }
        }
        if let Some((t, id)) = best {
            let at = s.pos + dir * t;
            s.hit.push(id);
            let mut dmg = s.dmg.clone();
            dmg.knockback = dmg.knockback.max(0.4);
            g.hit(sim, id, &dmg, at - dir, events);
            if s.explode > 0.0 {
                boom = Some(at);
            }
            if s.pierce > 0 {
                s.pierce -= 1;
            } else if s.chain > 0 {
                // Chain to the nearest other enemy.
                s.chain -= 1;
                let next = g
                    .actors
                    .iter()
                    .filter(|(o, a)| !a.dead && s.team.hostile(a.team) && !s.hit.contains(o))
                    .filter_map(|(o, _)| feet_of(sim, *o).map(|f| (*o, f.0)))
                    .map(|(o, f)| (o, f, flat(f - at).length()))
                    .filter(|x| x.2 < 9.0)
                    .min_by(|a, b| a.2.total_cmp(&b.2));
                match next {
                    Some((_, f, _)) => {
                        let speed = s.vel.length();
                        s.pos = at;
                        s.vel = (Vec3::new(f.x, at.y, f.z) - at).normalize_or(dir) * speed;
                        keep.push(s);
                        if let Some(b) = boom {
                            explode(g, sim, &d, &keep.last().unwrap().clone(), b, events);
                        }
                        continue;
                    }
                    None => end = true,
                }
            } else {
                end = true;
            }
        }
        if let Some(b) = boom {
            explode(g, sim, &d, &s, b, events);
        }
        if !end {
            s.pos += step;
            s.life -= dt;
            if s.life <= 0.0 {
                end = true;
            }
        }
        if !end {
            keep.push(s);
        }
    }
    keep.append(&mut g.shots);
    g.shots = keep;
}

fn explode(g: &mut Game, sim: &mut Sim, d: &super::data::Data, s: &Shot, at: Vec3, events: &mut Vec<SimEvent>) {
    if s.explode <= 0.0 {
        return;
    }
    let def = d.skill(s.skill).clone();
    let team = s.team;
    let hit = targets(g, sim, team, |f, r| flat(f - at).length() <= s.explode + r);
    for (t, _) in hit {
        if s.hit.first() == Some(&t) {
            continue; // the direct hit already took the full blow
        }
        let mut dmg = s.dmg.clone().scaled(0.75);
        dmg.knockback = 1.5;
        g.hit(sim, t, &dmg, at, events);
    }
    g.effects.push(Effect {
        kind: EffectKind::Ring,
        pos: at,
        radius: s.explode,
        t: 0.0,
        dur: 0.35,
        color: s.color,
        team,
        dmg: None,
        dir: Vec3::Z,
        angle: 360.0,
    });
    g.effects.push(Effect {
        kind: EffectKind::Burst,
        pos: at,
        radius: s.explode,
        t: 0.0,
        dur: 0.25,
        color: s.color,
        team,
        dmg: None,
        dir: Vec3::Z,
        angle: 0.0,
    });
    g.shake = (g.shake + 0.12 * sim.config.difficulty.shake).min(1.5);
    events.push(SimEvent::Blast { pos: at, element: def.element as u8 });
}

/// Ground effects: rings fade, delayed hits land.
pub fn update_effects(g: &mut Game, sim: &mut Sim, dt: f32, events: &mut Vec<SimEvent>) {
    let mut effects = std::mem::take(&mut g.effects);
    for e in &mut effects {
        let before = e.t;
        e.t += dt;
        if e.kind == EffectKind::Field && (before / 0.25).floor() != (e.t / 0.25).floor() && e.t < e.dur {
            if let Some(dmg) = e.dmg.clone() {
                let hit = targets(g, sim, e.team, |f, r| flat(f - e.pos).length() <= e.radius + r);
                for (t, _) in hit {
                    g.hit(sim, t, &dmg, e.pos, events);
                }
            }
        }
        if e.kind == EffectKind::Delayed && before < e.dur && e.t >= e.dur {
            if let Some(dmg) = e.dmg.clone() {
                let hit = targets(g, sim, e.team, |f, r| flat(f - e.pos).length() <= e.radius + r);
                for (t, _) in hit {
                    g.hit(sim, t, &dmg, e.pos, events);
                }
                events.push(SimEvent::Slam { pos: e.pos, radius: e.radius });
            }
        }
    }
    effects.retain(|e| e.t < e.dur + if e.kind == EffectKind::Delayed { 0.35 } else { 0.0 });
    effects.append(&mut g.effects);
    g.effects = effects;
}

/// Where a monster's wind-up will land.
pub fn telegraph(a: &Actor, c: &Cast, feet: Vec3) -> Option<Telegraph> {
    let def = data().skill(c.skill).clone();
    if !def.telegraph || c.fired || a.team != Team::Monster {
        return None;
    }
    let progress = (c.t / c.hit_at.max(1e-3)).clamp(0.0, 1.0);
    let shape = match def.behavior {
        Behavior::Melee => {
            TeleShape::Arc { center: feet, dir: c.dir, range: def.range * (a.radius / 0.42 * 0.5 + 0.5) + 0.3, angle: def.angle }
        }
        Behavior::Slam => TeleShape::Circle { center: feet + c.dir * def.range * 0.8, radius: def.radius },
        Behavior::Nova => TeleShape::Circle { center: feet, radius: def.radius },
        Behavior::Charge | Behavior::Dash => {
            TeleShape::Line { from: feet, dir: c.dir, length: def.range, width: def.radius.max(0.6) * 2.0 }
        }
        Behavior::Projectile => TeleShape::Line { from: feet, dir: c.dir, length: def.range.min(8.0), width: 0.35 },
        Behavior::Leap => TeleShape::Circle { center: c.target, radius: def.radius },
    };
    Some(Telegraph { shape, progress, color: if def.color == "#ffffff" { [1.0, 0.25, 0.15] } else { def.rgb() } })
}
