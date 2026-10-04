//! `mission`: the numbers of a room run, for authoring and comparing missions without playing
//! them: its cast, enemies, danger, loot, story and the length of the critical path. `vs=` puts
//! another room's numbers (and the ratios) next to them; without `room=` it reports the live
//! mission state of the current session (uplinks, things waiting for signals).

use std::collections::{BTreeMap, VecDeque};

use anyhow::{Context, Result};
use pav_core::ai::AiDef;
use pav_core::entity::{Behavior, EmitterDef};
use pav_core::room::RoomDef;
use pav_core::zones::ZoneKind;
use serde_json::{Map, Value, json};

use crate::session::Session;
use crate::tools::{Args, Output, get_str};

fn words(s: &str) -> usize {
    s.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).count()
}

fn rate(e: &EmitterDef) -> f32 {
    e.count.max(1) as f32 / e.interval.max(0.05)
}

fn behavior_rate(b: &Behavior) -> f32 {
    match b {
        Behavior::Emitter(e) => rate(e),
        _ => 0.0,
    }
}

fn load(key: &str) -> Result<RoomDef> {
    let path = if key.ends_with(".toml") { key.to_string() } else { format!("rooms/{key}.toml") };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => pav_core::room::load_sources(pav_core::room::rooms_dir().as_deref())
            .into_iter()
            .find(|s| s.key == key)
            .map(|s| s.text)
            .with_context(|| format!("no room '{key}'"))?,
    };
    RoomDef::parse(&text).map_err(|e| anyhow::anyhow!("{key}: {e}"))
}

/// Shortest walk on the first layer: START -> gates in order -> FINISH (tiles). Floors and
/// anything with headroom from 1 m (crouch ducts) are walkable; doors and other objects are not
/// considered.
fn critical_path(def: &RoomDef) -> Option<usize> {
    let layer = def.layout.layers.first()?;
    let mut lines: Vec<&str> = layer.map.lines().collect();
    if lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    let grid: Vec<Vec<char>> = lines.iter().map(|l| l.chars().collect()).collect();
    let tile = |c: char| def.layout.legend.get(&c.to_string());
    let walkable = |c: char| match tile(c) {
        Some(t) => t.blocks.iter().any(|b| b.y1 <= 0.01) && !t.blocks.iter().any(|b| !b.ghost && b.y1 > 0.05 && b.y0 < 1.0),
        None => c == '.',
    };
    let cells_where = |f: &dyn Fn(&pav_core::level::ZoneDef) -> bool| -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (r, row) in grid.iter().enumerate() {
            for (c, ch) in row.iter().enumerate() {
                if tile(*ch).and_then(|t| t.zone.as_ref()).is_some_and(f) {
                    out.push((r, c));
                }
            }
        }
        out
    };
    let start = cells_where(&|z| z.kind == ZoneKind::Start);
    let finish = cells_where(&|z| z.kind == ZoneKind::Finish);
    if start.is_empty() || finish.is_empty() {
        return None;
    }
    let mut gates: Vec<i32> = cells_where(&|z| z.kind == ZoneKind::Gate)
        .iter()
        .filter_map(|(r, c)| tile(grid[*r][*c]).and_then(|t| t.zone.as_ref()).map(|z| z.index))
        .collect();
    gates.sort();
    gates.dedup();
    let mut stages: Vec<Vec<(usize, usize)>> = vec![start];
    for g in gates {
        stages.push(cells_where(&|z| z.kind == ZoneKind::Gate && z.index == g));
    }
    stages.push(finish);
    // Multi-source BFS stage to stage; the next stage starts from the cell it arrived at.
    let mut total = 0;
    let mut from = stages[0].clone();
    for goal in &stages[1..] {
        let mut dist: BTreeMap<(usize, usize), usize> = from.iter().map(|p| (*p, 0)).collect();
        let mut q: VecDeque<(usize, usize)> = from.iter().copied().collect();
        let mut hit = None;
        while let Some((r, c)) = q.pop_front() {
            let d = dist[&(r, c)];
            if goal.contains(&(r, c)) {
                hit = Some(((r, c), d));
                break;
            }
            let n = [(r.wrapping_sub(1), c), (r + 1, c), (r, c.wrapping_sub(1)), (r, c + 1)];
            for (nr, nc) in n {
                let Some(ch) = grid.get(nr).and_then(|row| row.get(nc)) else { continue };
                if walkable(*ch) && !dist.contains_key(&(nr, nc)) {
                    dist.insert((nr, nc), d + 1);
                    q.push_back((nr, nc));
                }
            }
        }
        let (cell, d) = hit?;
        total += d;
        from = vec![cell];
    }
    Some(total)
}

/// The numbers of one room.
fn report(def: &RoomDef) -> Value {
    let npcs = &def.npcs;
    let crew: Vec<&str> = npcs.iter().filter(|n| matches!(n.ai, AiDef::Follow { .. })).map(|n| n.name.as_str()).collect();
    let guards: Vec<_> = npcs.iter().filter(|n| matches!(n.ai, AiDef::Guard { .. })).collect();
    let townsfolk = npcs.len() - crew.len() - guards.len();
    let vision: f32 = guards
        .iter()
        .map(|n| match &n.ai {
            AiDef::Guard { range, angle, .. } => 0.5 * range * range * angle.to_radians(),
            _ => 0.0,
        })
        .sum();
    let objs = &def.objects;
    let hostile = |o: &&pav_core::room::ObjectDef| {
        matches!(o.behavior, Behavior::Emitter(_)) || o.health.as_ref().is_some_and(|h| !h.phases.is_empty())
    };
    let shooters: Vec<_> = objs.iter().filter(hostile).collect();
    let killable: Vec<_> = shooters.iter().filter(|o| o.health.is_some()).collect();
    let bosses: Vec<&str> = objs.iter().filter(|o| o.health.as_ref().is_some_and(|h| h.bar)).map(|o| o.name.as_str()).collect();
    let caches: Vec<_> = objs.iter().filter(|o| o.health.is_some() && !hostile(o)).collect();
    let pickups: Vec<_> = objs.iter().filter(|o| o.pickup.is_some()).collect();
    let hazards = objs.iter().filter(|o| o.hazard.is_some()).count();
    let enemy_hp: f32 = killable.iter().filter_map(|o| o.health.as_ref()).map(|h| h.hp).sum();
    let fire: f32 = shooters.iter().map(|o| behavior_rate(&o.behavior)).sum();
    let peak: f32 = shooters
        .iter()
        .map(|o| {
            let phases = o.health.as_ref().map(|h| h.phases.iter().map(|p| behavior_rate(&p.behavior)).fold(0.0, f32::max));
            behavior_rate(&o.behavior).max(phases.unwrap_or(0.0))
        })
        .sum();
    let reinforcements =
        guards.iter().filter(|n| !n.on.is_empty()).count() + shooters.iter().filter(|o| !o.on.is_empty()).count();
    let mut zones: BTreeMap<String, usize> = BTreeMap::new();
    let mut uplinks = Vec::new();
    let mut zone_words = 0;
    for (k, t) in &def.layout.legend {
        let Some(z) = &t.zone else { continue };
        let kind = pav_core::params::ChoiceParam::name(z.kind).to_string();
        let n = def.layout.layers.iter().map(|l| l.map.matches(k.as_str()).count()).sum::<usize>();
        if n == 0 {
            continue;
        }
        *zones.entry(kind).or_default() += 1;
        zone_words += words(&z.label);
        if z.kind == ZoneKind::Hack {
            uplinks.push(json!({ "label": z.label, "time": z.time, "score": z.score, "signal": z.signal }));
        }
    }
    let uplink_pay: u32 =
        def.layout.legend.values().filter_map(|t| t.zone.as_ref()).filter(|z| z.kind == ZoneKind::Hack).map(|z| z.score).sum();
    let kill_pay: u32 = killable.iter().filter_map(|o| o.health.as_ref()).map(|h| h.score).sum();
    let cache_pay: u32 = caches.iter().filter_map(|o| o.health.as_ref()).map(|h| h.score).sum();
    let pickup_pay: u32 = pickups.iter().filter_map(|o| o.pickup.as_ref()).map(|p| p.score).sum();
    let label_words: usize = def.labels.iter().map(|l| words(&l.text)).sum();
    let try_words: usize = def.try_list.iter().map(|t| words(t)).sum();
    let story_words = label_words + zone_words + words(&def.about) + try_words;
    let switched = objs.iter().filter(|o| !o.off.is_empty()).count() + npcs.iter().filter(|n| !n.off.is_empty()).count();
    let held = objs.iter().filter(|o| !o.on.is_empty()).count() + npcs.iter().filter(|n| !n.on.is_empty()).count();
    let reveals = def.labels.iter().filter(|l| !l.on.is_empty()).count();
    let mut behaviors: BTreeMap<&str, usize> = BTreeMap::new();
    for o in objs {
        let k = match o.behavior {
            Behavior::None => continue,
            Behavior::Rain { .. } => "rain",
            Behavior::Spin { .. } => "spin",
            Behavior::Move(_) => "move",
            Behavior::Rotate(_) => "rotate",
            Behavior::Emitter(_) => "emitter",
            Behavior::Spawner(_) => "spawner",
        };
        *behaviors.entry(k).or_default() += 1;
    }
    let danger = enemy_hp + 10.0 * peak + vision + 20.0 * hazards as f32 + 30.0 * reinforcements as f32;
    let (w, h) = def.layout.extent();
    json!({
        "name": def.name,
        "size": [w, h],
        "characters": {
            "total": npcs.len(),
            "crew": crew,
            "guards": guards.len(),
            "townsfolk": townsfolk,
            "names": npcs.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        },
        "enemies": {
            "total": guards.len() + shooters.len(),
            "guards": guards.len(),
            "turrets_and_bosses": shooters.len(),
            "killable": killable.len(),
            "bosses": bosses,
            "reinforcements": reinforcements,
        },
        "danger": {
            "index": danger.round(),
            "enemy_hp": enemy_hp,
            "fire_rate": ((fire as f64) * 100.0).round() / 100.0,
            "peak_fire_rate": ((peak as f64) * 100.0).round() / 100.0,
            "vision_m2": vision.round(),
            "hazards": hazards,
        },
        "loot": {
            "total": kill_pay + cache_pay + pickup_pay + uplink_pay,
            "caches": caches.len(),
            "cache_pay": cache_pay,
            "pickups": pickups.len(),
            "pickup_pay": pickup_pay,
            "uplink_pay": uplink_pay,
            "kill_pay": kill_pay,
        },
        "story": {
            "words": story_words,
            "labels": def.labels.len(),
            "label_words": label_words,
            "zone_words": zone_words,
            "try": def.try_list.len(),
        },
        "path": { "critical_tiles": critical_path(def) },
        "mechanics": {
            "zones": zones,
            "behaviors": behaviors,
            "uplinks": uplinks,
            "switched_off": switched,
            "held_for_signals": held,
            "story_reveals": reveals,
        },
    })
}

/// Ratios a/b for every number both reports have (recursively).
fn ratios(a: &Value, b: &Value) -> Option<Value> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut out = Map::new();
            for (k, va) in x {
                if let Some(r) = y.get(k).and_then(|vb| ratios(va, vb)) {
                    out.insert(k.clone(), r);
                }
            }
            (!out.is_empty()).then_some(Value::Object(out))
        }
        (Value::Array(x), Value::Array(y)) if !y.is_empty() && x.iter().all(|v| !v.is_number()) => {
            Some(json!((x.len() as f64 / y.len() as f64 * 100.0).round() / 100.0))
        }
        _ => {
            let (x, y) = (a.as_f64()?, b.as_f64()?);
            (y != 0.0).then(|| json!((x / y * 100.0).round() / 100.0))
        }
    }
}

pub fn t_mission(s: &mut Session, a: &Args) -> Result<Output> {
    let Some(key) = get_str(a, "room") else {
        // Live state of the session's room mission.
        let sim = &s.sim;
        let hacks: Vec<Value> = sim
            .state
            .switchboard
            .hacks
            .iter()
            .map(|h| {
                let z = sim.state.statics.zone(h.zone);
                json!({ "label": z.map(|z| z.label.clone()), "signal": z.map(|z| z.signal.clone()), "progress": (h.progress * 100.0).round() / 100.0, "done": h.done })
            })
            .collect();
        let armed: Vec<Value> = sim
            .state
            .switchboard
            .armed
            .iter()
            .map(|x| {
                let name = match &x.thing {
                    pav_core::switches::ArmedThing::Object(o) => o.name.clone(),
                    pav_core::switches::ArmedThing::Npc(n) => n.name.clone(),
                    pav_core::switches::ArmedThing::Label(l) => format!("label: {}", l.text),
                };
                json!({ "name": name, "on": x.on, "heard": x.heard })
            })
            .collect();
        let switches: Vec<Value> = sim
            .state
            .entities
            .iter()
            .filter_map(|e| Some((e, e.switch.as_ref().filter(|s| !s.off.is_empty())?)))
            .map(|(e, sw)| json!({ "name": e.name, "off": sw.off, "heard": sw.heard }))
            .collect();
        return Ok(Output::Json(json!({
            "uplinks": hacks,
            "waiting": armed,
            "switches": switches,
            "uplink": sim.uplink_hud().map(|u| json!({ "label": u.label, "progress": u.progress, "done": u.done })),
            "course": sim.state.courses.run.as_ref().map(|r| json!({ "score": r.score, "alarms": r.alarms, "hits": r.hits, "falls": r.falls })),
        })));
    };
    let mine = report(&load(key)?);
    let Some(other) = get_str(a, "vs") else { return Ok(Output::Json(mine)) };
    let theirs = report(&load(other)?);
    let r = ratios(&mine, &theirs);
    Ok(Output::Json(json!({ "room": mine, "vs": theirs, "ratio": r })))
}
