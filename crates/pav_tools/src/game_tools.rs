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

/// A tiny software canvas for diagrams (no GPU needed).
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: u32, h: u32, bg: [u8; 3]) -> Self {
        let mut px = vec![255u8; (w * h * 4) as usize];
        for p in px.chunks_mut(4) {
            p[..3].copy_from_slice(&bg);
        }
        Self { w, h, px }
    }
    fn blend(&mut self, x: i32, y: i32, c: [u8; 3], a: f32) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        for (k, ck) in c.iter().enumerate() {
            self.px[i + k] = (self.px[i + k] as f32 * (1.0 - a) + *ck as f32 * a) as u8;
        }
    }
    pub fn disc(&mut self, cx: f32, cy: f32, r: f32, c: [u8; 3]) {
        let (x0, x1, y0, y1) = ((cx - r - 1.0) as i32, (cx + r + 1.0) as i32, (cy - r - 1.0) as i32, (cy + r + 1.0) as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let a = (r + 0.5 - d).clamp(0.0, 1.0);
                if a > 0.0 {
                    self.blend(x, y, c, a);
                }
            }
        }
    }
    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), width: f32, c: [u8; 3]) {
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let n = (len / 0.7).ceil().max(1.0) as i32;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            self.disc(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, width * 0.5, c);
        }
    }
}

pub fn t_tree_map(s: &mut Session, a: &Args) -> Result<Output> {
    use pav_core::arpg::tree::NodeKind;
    let d = data();
    let t = &d.tree;
    let size = get_u64(a, "size", 1200)?.clamp(256, 4096) as u32;
    let rings = get_u64(a, "rings", 2)? as u32;
    let alloc = s.sim.state.game.as_ref().map(|g| g.hero.tree.clone()).unwrap_or_default();
    let shown: Vec<&pav_core::arpg::tree::Node> = t.nodes.iter().filter(|n| n.ring <= rings).collect();
    let extent = shown.iter().map(|n| n.pos.length()).fold(1.0f32, f32::max) + 1.0;
    let scale = size as f32 * 0.5 / extent;
    let at = |p: glam::Vec2| (size as f32 * 0.5 + p.x * scale, size as f32 * 0.5 + p.y * scale);
    let mut c = Canvas::new(size, size, [18, 17, 22]);
    let col = |n: &pav_core::arpg::tree::Node| {
        let k = t.sectors.get(n.sector as usize).map(|s| s.color).unwrap_or([0.8; 3]);
        [(k[0] * 255.0) as u8, (k[1] * 255.0) as u8, (k[2] * 255.0) as u8]
    };
    for n in &shown {
        for l in &n.links {
            let Some(m) = t.node(*l).filter(|m| m.ring <= rings && m.id > n.id) else { continue };
            let on = alloc.contains(&n.id) && alloc.contains(&m.id);
            c.line(at(n.pos), at(m.pos), if on { 3.0 } else { 1.5 }, if on { [240, 220, 150] } else { [70, 66, 72] });
        }
    }
    for n in &shown {
        let (x, y) = at(n.pos);
        let r = match n.kind {
            NodeKind::Start => 9.0,
            NodeKind::Keystone => 8.0,
            NodeKind::Notable => 6.0,
            NodeKind::Mastery => 5.5,
            NodeKind::Skill => 4.5,
            NodeKind::Astral if n.lore == "star" => 4.5,
            _ => 3.2,
        } * (size as f32 / 1200.0).sqrt();
        let base = col(n);
        let fill = if alloc.contains(&n.id) || n.kind == NodeKind::Start { [250, 235, 170] } else { base };
        if matches!(n.kind, NodeKind::Keystone | NodeKind::Notable | NodeKind::Mastery) {
            c.disc(x, y, r + 1.5, [230, 230, 230]);
        }
        if n.kind == NodeKind::Skill {
            c.disc(x, y, r + 1.5, [140, 220, 255]);
        }
        c.disc(x, y, r, fill);
    }
    let png = pav_render::capture::encode_png(size, size, &c.px)?;
    let path = std::path::PathBuf::from(get_str(a, "out").map(String::from).unwrap_or("out/tree.png".into()));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &png)?;
    let count = |k: NodeKind| t.nodes.iter().filter(|n| n.kind == k && n.ring == 0).count();
    let meta = json!({
        "path": path,
        "nodes_main": t.nodes.iter().filter(|n| n.ring == 0).count(),
        "nodes_total": t.nodes.len(),
        "notables": count(NodeKind::Notable),
        "keystones": count(NodeKind::Keystone),
        "masteries": count(NodeKind::Mastery),
        "skill_nodes": count(NodeKind::Skill),
        "astral_rings": t.nodes.iter().map(|n| n.ring).max(),
        "allocated": alloc.len(),
    });
    Ok(Output::Image { png, path: Some(path), meta })
}

/// Runs one game command as an input frame; returns complaints that floated up.
fn run_cmd(s: &mut Session, cmd: GameCmd) -> Vec<String> {
    let before = s.sim.state.game.as_ref().map(|g| g.floaters.len()).unwrap_or(0);
    s.sim.step(&pav_core::InputFrame { cmd: Some(cmd), ..Default::default() });
    s.keep_events();
    s.sim
        .state
        .game
        .as_ref()
        .map(|g| g.floaters.iter().skip(before).filter(|f| !f.text.is_empty()).map(|f| f.text.clone()).collect())
        .unwrap_or_default()
}

pub fn t_tree(s: &mut Session, a: &Args) -> Result<Output> {
    use pav_core::arpg::tree::NodeKind;
    game(s)?;
    let d = data();
    let t = &d.tree;
    let find_node = |q: &str| -> Option<u32> {
        if let Ok(id) = q.parse::<u32>() {
            return t.node(id).map(|n| n.id);
        }
        let q = q.to_lowercase();
        t.nodes
            .iter()
            .filter(|n| n.ring == 0)
            .find(|n| n.name.to_lowercase() == q)
            .or_else(|| t.nodes.iter().find(|n| n.name.to_lowercase().contains(&q) || n.key == q))
            .map(|n| n.id)
    };
    let mut said = Vec::new();
    if let Some(q) = get_str(a, "take") {
        let id = find_node(q).ok_or_else(|| anyhow!("no node matches '{q}'"))?;
        let alloc = game(s)?.hero.tree.clone();
        let path = t.path_to(&alloc, id).ok_or_else(|| anyhow!("no path"))?;
        for p in path {
            said.extend(run_cmd(s, GameCmd::Allocate(p)));
        }
    }
    if let Some(q) = get_str(a, "refund") {
        let id = find_node(q).ok_or_else(|| anyhow!("no node matches '{q}'"))?;
        said.extend(run_cmd(s, GameCmd::Refund(id)));
    }
    if a.get("respec").and_then(|v| v.as_bool()).unwrap_or(false) {
        said.extend(run_cmd(s, GameCmd::Respec));
    }
    if let Some(q) = get_str(a, "mastery") {
        let id = find_node(q).ok_or_else(|| anyhow!("no node matches '{q}'"))?;
        said.extend(run_cmd(s, GameCmd::Mastery(id, get_u64(a, "option", 0)? as u8)));
    }
    let g = game(s)?;
    let alloc = &g.hero.tree;
    let describe = |n: &pav_core::arpg::tree::Node| {
        json!({
            "id": n.id,
            "name": n.name,
            "kind": format!("{:?}", n.kind).to_lowercase(),
            "does": t.describe(n),
            "allocated": alloc.contains(&n.id),
            "points_away": if alloc.contains(&n.id) { 0 } else { t.path_to(alloc, n.id).map(|p| p.len()).unwrap_or(0) },
        })
    };
    let found: Vec<Value> = match get_str(a, "find") {
        Some(q) => {
            let q = q.to_lowercase();
            t.nodes
                .iter()
                .filter(|n| n.ring == 0 || a.get("astral").and_then(|v| v.as_bool()).unwrap_or(false))
                .filter(|n| {
                    q == "keystone" && n.kind == NodeKind::Keystone
                        || n.name.to_lowercase().contains(&q)
                        || t.describe(n).iter().any(|l| l.to_lowercase().contains(&q))
                })
                .take(40)
                .map(describe)
                .collect()
        }
        None => Vec::new(),
    };
    let bonus = t.bonus(alloc, &g.hero.masteries);
    let mut lines: Vec<String> = bonus.mods.0.iter().map(|(st, v)| pav_core::arpg::stats::describe(*st, *v)).collect();
    lines.extend(bonus.powers.iter().map(|p| p.describe()));
    for tw in &bonus.tweaks {
        let name = d.skill_id(&tw.skill).map(|i| d.skill(i).name.clone()).unwrap_or_default();
        lines.push(tw.describe(&name));
    }
    Ok(Output::Json(json!({
        "points": g.hero.points(),
        "allocated": alloc.len(),
        "tree_gives": lines,
        "found": found,
        "said": said,
        "refund_cost": g.hero.refund_cost(),
        "respec_cost": g.hero.respec_cost(),
        "nodes": t.nodes.iter().filter(|n| n.ring == 0).count(),
    })))
}
