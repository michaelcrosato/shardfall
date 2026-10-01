//! Agent tools for Shardfall: inspect and change the hero, spawn monsters, let a bot play,
//! list skills, reload the data folder. They work on any scene that runs the game (`arena`,
//! later the town and levels), headless or live (`pav live`).

use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use glam::Vec3;
use pav_core::arpg::combat::{Rarity, Team};
use pav_core::arpg::data::data;
use pav_core::arpg::items::{EquipSlot, Item, RollSpec, Slot, roll_item, unique_item};
use pav_core::arpg::{GameCmd, Place};
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
        "place": format!("{:?}", g.place).to_lowercase(),
        "bag": g.hero.inventory.len(),
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

fn rarity_arg(a: &Args) -> Result<Option<Rarity>> {
    Ok(match get_str(a, "rarity") {
        None => None,
        Some("normal") => Some(Rarity::Normal),
        Some("magic") => Some(Rarity::Magic),
        Some("rare") => Some(Rarity::Rare),
        Some("unique") => Some(Rarity::Unique),
        Some(o) => bail!("unknown rarity '{o}'"),
    })
}

fn slot_arg(a: &Args) -> Result<Option<Slot>> {
    match get_str(a, "slot") {
        None => Ok(None),
        Some(k) => Slot::ALL.iter().copied().find(|s| s.key() == k).map(Some).ok_or_else(|| anyhow!("unknown slot '{k}'")),
    }
}

/// An item as JSON: name, rarity, level, tooltip lines.
pub fn item_json(it: &Item) -> Value {
    let d = data();
    let t = it.describe(&d);
    json!({
        "id": it.id,
        "name": t.name,
        "base": it.base,
        "rarity": format!("{:?}", it.rarity).to_lowercase(),
        "level": it.level,
        "kind": t.kind,
        "header": t.header,
        "implicit": t.implicit,
        "mods": t.mods.iter().map(|m| m.0.clone()).collect::<Vec<_>>(),
        "power": (!t.power.is_empty()).then_some(t.power),
        "value": it.value(),
    })
}

pub fn t_loot_roll(s: &mut Session, a: &Args) -> Result<Output> {
    let d = data();
    let level = get_u64(a, "level", s.sim.state.game.as_ref().map(|g| g.hero.level as u64).unwrap_or(10))? as u32;
    let count = get_u64(a, "count", 5)?.clamp(1, 20000) as usize;
    let mut rng = pav_core::rng::Rng::new(get_u64(a, "seed", 1)?);
    let spec = RollSpec {
        level,
        rarity: rarity_arg(a)?,
        slot: slot_arg(a)?,
        rarity_bonus: a.get("bonus").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
    };
    let items: Vec<Item> = (0..count).filter_map(|i| roll_item(&d, &mut rng, spec, i as u32 + 1)).collect();
    if count <= 20 {
        return Ok(Output::Json(json!(items.iter().map(item_json).collect::<Vec<_>>())));
    }
    // A summary: how loot is distributed.
    let mut rarity: BTreeMap<String, usize> = BTreeMap::new();
    let mut slots: BTreeMap<String, usize> = BTreeMap::new();
    let mut affixes: BTreeMap<String, usize> = BTreeMap::new();
    let mut uniques: BTreeMap<String, usize> = BTreeMap::new();
    let mut values = Vec::new();
    for it in &items {
        *rarity.entry(format!("{:?}", it.rarity).to_lowercase()).or_default() += 1;
        *slots.entry(it.slot(&d).key().to_string()).or_default() += 1;
        for m in &it.mods {
            if !m.affix.is_empty() {
                *affixes.entry(m.affix.clone()).or_default() += 1;
            }
        }
        if !it.unique.is_empty() {
            *uniques.entry(it.unique.clone()).or_default() += 1;
        }
        values.push(it.value());
    }
    values.sort();
    let mut top: Vec<(String, usize)> = affixes.into_iter().collect();
    top.sort_by_key(|t| std::cmp::Reverse(t.1));
    let never: Vec<&str> = d
        .affixes
        .iter()
        .filter(|af| af.reached(level) > 0 && !top.iter().any(|t| t.0 == af.key))
        .map(|af| af.key.as_str())
        .collect();
    Ok(Output::Json(json!({
        "items": items.len(),
        "level": level,
        "rarity": rarity,
        "slots": slots,
        "uniques": uniques,
        "affixes_most": top.iter().take(12).collect::<Vec<_>>(),
        "affixes_least": top.iter().rev().take(8).collect::<Vec<_>>(),
        "affixes_never_rolled": never,
        "value": { "min": values.first(), "median": values.get(values.len() / 2), "max": values.last() },
    })))
}

pub fn t_give(s: &mut Session, a: &Args) -> Result<Output> {
    game(s)?;
    let d = data();
    let level = get_u64(a, "level", game(s)?.hero.level as u64)? as u32;
    let mut g = s.sim.state.game.take().unwrap();
    let id = g.hero.new_id();
    let item = if let Some(u) = get_str(a, "unique") {
        match d.unique(u) {
            Some(def) => Ok(unique_item(&d, &mut s.sim.state.rng, def, level, id)),
            None => Err(anyhow!(
                "unknown unique '{u}' (known: {})",
                d.uniques.iter().map(|u| u.key.as_str()).collect::<Vec<_>>().join(", ")
            )),
        }
    } else if let Some(b) = get_str(a, "base") {
        d.base(b).map(|b| Item::plain(id, b, level)).ok_or_else(|| anyhow!("unknown base '{b}'"))
    } else {
        let spec = RollSpec { level, rarity: rarity_arg(a)?, slot: slot_arg(a)?, rarity_bonus: 0.0 };
        roll_item(&d, &mut s.sim.state.rng, spec, id).ok_or_else(|| anyhow!("nothing fits"))
    };
    let item = match item {
        Ok(i) => i,
        Err(e) => {
            s.sim.state.game = Some(g);
            return Err(e);
        }
    };
    let json = item_json(&item);
    g.hero.inventory.push(item);
    g.inv_changed();
    s.sim.state.game = Some(g);
    if a.get("equip").and_then(|v| v.as_bool()).unwrap_or(false) {
        s.sim.step(&pav_core::InputFrame { cmd: Some(GameCmd::Equip(id)), ..Default::default() });
        s.keep_events();
    }
    Ok(Output::Json(json!({ "item": json, "hero": hero_json(s)? })))
}

pub fn t_inventory(s: &mut Session, a: &Args) -> Result<Output> {
    let g = game(s)?;
    let brief = a.get("brief").and_then(|v| v.as_bool()).unwrap_or(false);
    let show = |it: &Item| {
        if brief { json!(format!("#{} {} ({:?}, level {})", it.id, it.name, it.rarity, it.level)) } else { item_json(it) }
    };
    let worn: BTreeMap<&str, Value> =
        EquipSlot::ALL.iter().filter_map(|e| g.hero.worn(*e).map(|it| (e.key(), show(it)))).collect();
    Ok(Output::Json(json!({
        "place": format!("{:?}", g.place).to_lowercase(),
        "gold": g.hero.gold,
        "worn": worn,
        "bag": g.hero.inventory.iter().map(show).collect::<Vec<_>>(),
        "stash": g.hero.stash.iter().map(show).collect::<Vec<_>>(),
        "vendor": g.vendor.iter().map(|it| { let mut v = show(it); if let Some(o) = v.as_object_mut() { o.insert("price".into(), json!(pav_core::arpg::cmd::buy_price(it))); } v }).collect::<Vec<_>>(),
        "on_ground": g.loot.iter().map(|l| json!({ "id": l.item.id, "name": l.item.name, "rarity": format!("{:?}", l.item.rarity).to_lowercase(), "pos": [round3(l.pos.x), round3(l.pos.y), round3(l.pos.z)] })).collect::<Vec<_>>(),
        "powers": g.hero.powers.iter().map(|p| p.describe()).collect::<Vec<_>>(),
        "weapon": g.hero.weapon,
    })))
}

pub fn t_game_cmd(s: &mut Session, a: &Args) -> Result<Output> {
    game(s)?;
    let d = data();
    let what = get_str(a, "do").ok_or_else(|| anyhow!("'do' is required"))?;
    let id = || -> Result<u32> { Ok(get_u64(a, "id", 0)? as u32) };
    let eslot = || -> Result<u8> {
        let k = get_str(a, "slot").ok_or_else(|| anyhow!("slot is required"))?;
        Ok(EquipSlot::from_key(k).ok_or_else(|| anyhow!("unknown slot '{k}'"))?.index() as u8)
    };
    let cmd = match what {
        "pickup" => GameCmd::Pickup(id()?),
        "equip" if a.contains_key("slot") => GameCmd::EquipTo(id()?, eslot()?),
        "equip" => GameCmd::Equip(id()?),
        "unequip" => GameCmd::Unequip(eslot()?),
        "drop" => GameCmd::Drop(id()?),
        "sell" => GameCmd::Sell(id()?),
        "buy" => GameCmd::Buy(id()?),
        "stash" => GameCmd::Stash(id()?),
        "take" => GameCmd::Take(id()?),
        "sell_all" => GameCmd::SellAll(get_u64(a, "rarity", 0)? as u8),
        "auto_loot" => GameCmd::AutoLoot(get_u64(a, "rarity", 1)? as u8),
        "sort" => GameCmd::Sort,
        "bar" => {
            let slot = get_str(a, "slot").and_then(|x| x.parse::<u8>().ok()).ok_or_else(|| anyhow!("slot 0-5 is required"))?;
            let k = get_str(a, "skill").ok_or_else(|| anyhow!("skill is required"))?;
            GameCmd::Bar(slot, d.skill_id(k).ok_or_else(|| anyhow!("unknown skill '{k}'"))?)
        }
        "travel" => {
            let p = match get_str(a, "place").unwrap_or("town") {
                "town" => Place::Town,
                "arena" => Place::Arena,
                o => bail!("unknown place '{o}' (town, arena)"),
            };
            GameCmd::Travel(p.code())
        }
        o => bail!("unknown action '{o}'"),
    };
    let floaters_before = game(s)?.floaters.len();
    s.sim.step(&pav_core::InputFrame { cmd: Some(cmd), ..Default::default() });
    s.keep_events();
    s.sync_camera();
    // Complaints float over the hero; report them.
    let g = game(s)?;
    let said: Vec<String> =
        g.floaters.iter().skip(floaters_before).filter(|f| !f.text.is_empty()).map(|f| f.text.clone()).collect();
    Ok(Output::Json(json!({ "did": format!("{cmd:?}"), "said": said, "scene": s.sim.state.scene, "hero": hero_json(s)? })))
}
