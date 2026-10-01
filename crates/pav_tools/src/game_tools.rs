//! Agent tools for Shardfall: inspect and change the hero, spawn monsters, let a bot play,
//! list skills, reload the data folder. They work on any scene that runs the game (`arena`,
//! later the town and levels), headless or live (`pav live`).

use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use glam::Vec3;
use pav_core::arpg::combat::{Rarity, Team};
use pav_core::arpg::data::data;
use serde_json::{Value, json};

use crate::session::Session;
use crate::tools::{Args, Output, get_str, get_u64, round3, vec_arg};

fn game(s: &Session) -> Result<&pav_core::arpg::Game> {
    s.sim.state.game.as_deref().ok_or_else(|| anyhow!("this scene is not running Shardfall (try `load scene=arena`)"))
}

fn hero_json(s: &Session) -> Result<Value> {
    let g = game(s)?;
    let h = g.hero_actor();
    Ok(json!({
        "name": g.hero.name,
        "level": g.hero.level,
        "xp": g.hero.xp.round(),
        "xp_next": pav_core::arpg::hero::xp_to_next(g.hero.level).round(),
        "gold": g.hero.gold,
        "kills": g.hero.kills,
        "deaths": g.hero.deaths,
        "potions": g.hero.potions,
        "life": h.map(|a| round3(a.life)),
        "life_max": h.map(|a| round3(a.sheet.life_max)),
        "mana": h.map(|a| round3(a.mana)),
        "mana_max": h.map(|a| round3(a.sheet.mana_max)),
        "dead": h.map(|a| a.dead),
        "weapon": g.hero.weapon.name,
        "bar": g.hero.bar,
    }))
}

pub fn t_game(s: &mut Session, _: &Args) -> Result<Output> {
    let g = game(s)?;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for a in g.actors.values().filter(|a| a.team == Team::Monster && !a.dead) {
        *counts.entry(format!("{} ({:?}, level {})", a.family, a.rarity, a.level).to_lowercase()).or_default() += 1;
    }
    Ok(Output::Json(json!({
        "hero": hero_json(s)?,
        "monsters_alive": g.monsters_alive(),
        "monsters": counts,
        "projectiles": g.shots.len(),
        "effects": g.effects.len(),
        "wave": g.arena.as_ref().map(|a| a.wave),
        "message": g.message.as_ref().map(|m| m.0.clone()),
        "tick": s.sim.state.tick,
    })))
}

pub fn t_hero(s: &mut Session, a: &Args) -> Result<Output> {
    game(s)?;
    let mut g = s.sim.state.game.take().unwrap();
    let r = (|| -> Result<()> {
        if let Some(l) = a.get("level") {
            g.hero.level = l.as_u64().ok_or_else(|| anyhow!("level must be a number"))?.max(1) as u32;
            g.hero.xp = 0.0;
        }
        if a.contains_key("gold") {
            g.hero.gold = get_u64(a, "gold", 0)?;
        }
        if let Some(x) = a.get("xp").and_then(|v| v.as_f64()) {
            g.hero.gain_xp(x);
        }
        if let Some(k) = get_str(a, "skill") {
            let slot = get_u64(a, "slot", 0)? as usize;
            if slot > 5 {
                bail!("slot is 0-5");
            }
            if data().skill_id(k).is_none() {
                bail!("unknown skill '{k}'");
            }
            g.hero.bar[slot] = k.to_string();
        }
        Ok(())
    })();
    let heal = a.get("heal").and_then(|v| v.as_bool()).unwrap_or(false) || a.contains_key("level");
    if heal {
        g.hero.potions = g.hero.potion_max;
    }
    pav_core::arpg::refresh_hero(&mut s.sim, &mut g, heal);
    s.sim.state.game = Some(g);
    r?;
    Ok(Output::Json(hero_json(s)?))
}

pub fn t_monster(s: &mut Session, a: &Args) -> Result<Output> {
    let d = data();
    let Some(family) = get_str(a, "family") else {
        let list: Vec<Value> = d
            .families
            .iter()
            .map(|f| json!({"key": f.key, "name": f.name, "body": format!("{:?}", f.body).to_lowercase(), "archetype": format!("{:?}", f.archetype).to_lowercase(), "skills": f.skills}))
            .collect();
        return Ok(Output::Json(json!(list)));
    };
    if d.family(family).is_none() {
        bail!("unknown family '{family}' (known: {})", d.families.iter().map(|f| f.key.as_str()).collect::<Vec<_>>().join(", "));
    }
    let g = game(s)?;
    let level = get_u64(a, "level", g.hero.level as u64)? as u32;
    let rarity = match get_str(a, "rarity").unwrap_or("normal") {
        "normal" => Rarity::Normal,
        "magic" => Rarity::Magic,
        "rare" => Rarity::Rare,
        "unique" | "boss" => Rarity::Unique,
        o => bail!("unknown rarity '{o}'"),
    };
    let count = get_u64(a, "count", 1)?.clamp(1, 60) as usize;
    let (feet, facing) = match s.sim.player() {
        Some(p) => {
            let ch = p.character.as_ref().unwrap();
            (p.pos - Vec3::Y * ch.height() * 0.5, ch.facing)
        }
        None => (Vec3::ZERO, 0.0),
    };
    let at = match vec_arg(a, "pos")?.filter(|v| v.len() == 3) {
        Some(v) => Vec3::new(v[0], v[1], v[2]),
        None => feet + Vec3::new(facing.sin(), 0.0, facing.cos()) * 6.0,
    };
    let aggro = a.get("aggro").and_then(|v| v.as_bool()).unwrap_or(true);
    let pack = g.next_pack;
    let mut ids = Vec::new();
    for i in 0..count {
        let ang = i as f32 * 2.4;
        let p = at + Vec3::new(ang.cos(), 0.0, ang.sin()) * (i as f32).sqrt() * 0.9;
        if let Some(id) = s.sim.spawn_monster(family, level, rarity, p, pack) {
            ids.push(id.0);
        }
    }
    if let Some(g) = s.sim.state.game.as_mut() {
        g.next_pack += 1;
        for id in &ids {
            if let Some(b) = g.actors.get_mut(&pav_core::EntityId(*id)).and_then(|a| a.brain.as_mut()) {
                b.aggro = aggro;
            }
        }
    }
    Ok(Output::Json(json!({ "spawned": ids, "family": family, "level": level, "rarity": format!("{rarity:?}").to_lowercase() })))
}

pub fn t_autoplay(s: &mut Session, a: &Args) -> Result<Output> {
    game(s)?;
    let secs = a.get("seconds").and_then(|v| v.as_f64()).unwrap_or(30.0).clamp(0.1, 3600.0);
    let ticks = (secs * s.sim.config.tick_rate.hz() as f64) as u64;
    let mut bot = pav_core::arpg::bot::Bot::default();
    bot.goal = vec_arg(a, "goal")?.filter(|v| v.len() == 3).map(|v| Vec3::new(v[0], v[1], v[2]));
    let t = std::time::Instant::now();
    let stats = bot.run(&mut s.sim, ticks);
    s.keep_events();
    s.sync_camera();
    Ok(Output::Json(json!({
        "stats": stats,
        "hero": hero_json(s)?,
        "wall_ms": t.elapsed().as_millis(),
    })))
}

pub fn t_skills(_: &mut Session, a: &Args) -> Result<Output> {
    let d = data();
    let only = get_str(a, "key");
    let list: Vec<Value> = d
        .skills
        .iter()
        .filter(|k| only.is_none_or(|o| o == k.key))
        .map(|k| serde_json::to_value(k).unwrap_or_default())
        .collect();
    if list.is_empty() {
        bail!("unknown skill '{}'", only.unwrap_or(""));
    }
    Ok(Output::Json(json!(list)))
}

pub fn t_game_reload(_: &mut Session, _: &Args) -> Result<Output> {
    pav_core::arpg::data::reload(true).map_err(|e| anyhow!(e))?;
    let d = data();
    Ok(Output::Json(json!({ "skills": d.skills.len(), "families": d.families.len() })))
}
