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
        "travel" => GameCmd::Travel(place_arg(a)?.code()),
        "use" => GameCmd::Use(get_u64(a, "spot", 0)? as u32),
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
    /// A filled axis-aligned rectangle (pixel coordinates, any corner order).
    pub fn rect(&mut self, a: (f32, f32), b: (f32, f32), c: [u8; 3], alpha: f32) {
        let (x0, x1) = (a.0.min(b.0).round() as i32, a.0.max(b.0).round() as i32);
        let (y0, y1) = (a.1.min(b.1).round() as i32, a.1.max(b.1).round() as i32);
        for y in y0..y1 {
            for x in x0..x1 {
                self.blend(x, y, c, alpha);
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

// ------------------------------------------------------------------------------ creatures

fn genome_opts(a: &Args) -> Result<pav_core::arpg::genome::GenomeOpts> {
    use pav_core::params::ChoiceParam;
    let body = match get_str(a, "body") {
        None => None,
        Some(b) => Some(
            pav_core::puppet::BodyPlan::NAMES
                .iter()
                .position(|n| *n == b)
                .map(pav_core::puppet::BodyPlan::from_index)
                .ok_or_else(|| anyhow!("unknown body '{b}' (biped spider lizard beetle blob)"))?,
        ),
    };
    let element = match get_str(a, "element") {
        None => None,
        Some(e) => Some(
            pav_core::arpg::data::Element::ALL
                .iter()
                .copied()
                .find(|x| x.name().eq_ignore_ascii_case(e))
                .ok_or_else(|| anyhow!("unknown element '{e}'"))?,
        ),
    };
    let parts = match get_str(a, "parts") {
        None => None,
        Some(list) => Some(
            list.split(',')
                .map(|p| {
                    pav_core::parts::AttachKind::NAMES
                        .iter()
                        .position(|n| *n == p.trim())
                        .map(pav_core::parts::AttachKind::from_index)
                        .ok_or_else(|| anyhow!("unknown part '{p}' ({})", pav_core::parts::AttachKind::NAMES.join(" ")))
                })
                .collect::<Result<Vec<_>>>()?,
        ),
    };
    Ok(pav_core::arpg::genome::GenomeOpts { body, archetype: get_str(a, "archetype").map(String::from), element, parts })
}

fn genome_json(g: &pav_core::arpg::genome::Genome) -> Value {
    json!({
        "seed": g.seed,
        "name": g.name,
        "body": format!("{:?}", g.body).to_lowercase(),
        "archetype": g.archetype,
        "brain": format!("{:?}", g.brain).to_lowercase(),
        "element": g.element.name(),
        "size": round3(g.puppet.scale),
        "parts": g.puppet.parts.iter().map(|p| format!("{:?} x{:.1}{}", p.kind, p.size, if p.count > 0 { format!(" ({})", p.count) } else { String::new() }).to_lowercase()).collect::<Vec<_>>(),
        "skills": g.skills,
        "life": round3(g.life),
        "damage": round3(g.damage),
        "speed": round3(g.speed),
        "powers": g.powers.iter().map(|p| p.describe()).collect::<Vec<_>>(),
        "colors": [g.puppet.skin.clone(), g.puppet.shirt.clone(), g.puppet.accent.clone()],
    })
}

pub fn t_genome(s: &mut Session, a: &Args) -> Result<Output> {
    let d = data();
    let seed = get_u64(a, "seed", 1)?;
    let level = get_u64(a, "level", 10)? as u32;
    let g = pav_core::arpg::genome::Genome::generate(&d, seed, level, &genome_opts(a)?).map_err(|e| anyhow!(e))?;
    let mut out = genome_json(&g);
    if a.get("spawn").and_then(|v| v.as_bool()).unwrap_or(false) {
        game(s)?;
        let feet = s.sim.player().map(|p| p.pos - Vec3::Y * p.character.as_ref().unwrap().height() * 0.5).unwrap_or_default();
        let spec = g.spec(&d);
        let mut gm = s.sim.state.game.take().unwrap();
        let pack = gm.next_pack;
        gm.next_pack += 1;
        let id = pav_core::arpg::spawn_spec_into(
            &mut s.sim,
            &mut gm,
            &spec,
            level,
            Rarity::Normal,
            feet + Vec3::new(0.0, 0.0, -6.0),
            pack,
        );
        s.sim.state.game = Some(gm);
        out["spawned"] = json!(id.map(|i| i.0));
    }
    Ok(Output::Json(out))
}

pub fn t_bestiary(_: &mut Session, a: &Args) -> Result<Output> {
    let d = data();
    let count = get_u64(a, "count", 20)?.clamp(1, 5000);
    let level = get_u64(a, "level", 10)? as u32;
    let start = get_u64(a, "seed", 1)?;
    let opts = genome_opts(a)?;
    let mut list = Vec::new();
    let mut by: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut names = std::collections::BTreeSet::new();
    for i in 0..count {
        let g = pav_core::arpg::genome::Genome::generate(&d, start + i, level, &opts).map_err(|e| anyhow!(e))?;
        for (k, v) in [
            ("body", format!("{:?}", g.body).to_lowercase()),
            ("archetype", g.archetype.clone()),
            ("element", g.element.name().to_string()),
            ("parts", g.puppet.parts.len().to_string()),
        ] {
            *by.entry(k.into()).or_default().entry(v).or_default() += 1;
        }
        for p in &g.puppet.parts {
            *by.entry("part kinds".into()).or_default().entry(format!("{:?}", p.kind).to_lowercase()).or_default() += 1;
        }
        names.insert(g.name.clone());
        if list.len() < 30 {
            list.push(json!(format!(
                "#{} {} — {} {} ({}), {}",
                g.seed,
                g.name,
                g.element.name(),
                format!("{:?}", g.body).to_lowercase(),
                g.archetype,
                g.skills.join("/")
            )));
        }
    }
    Ok(Output::Json(json!({ "count": count, "distinct_names": names.len(), "spread": by, "examples": list })))
}

pub fn t_boss(s: &mut Session, a: &Args) -> Result<Output> {
    let d = data();
    let Some(key) =
        get_str(a, "key").map(String::from).or_else(|| a.get("seed").and_then(|v| v.as_u64()).map(|x| format!("gen:{x}")))
    else {
        let list: Vec<Value> = d
            .bosses
            .iter()
            .map(|b| json!({ "key": b.key, "name": b.name, "title": b.title, "phases": b.phases.iter().map(|p| json!({ "at": p.at, "say": p.say })).collect::<Vec<_>>() }))
            .collect();
        return Ok(Output::Json(json!({ "bosses": list, "generated": "seed=N spawns a generated boss" })));
    };
    game(s)?;
    let level = get_u64(a, "level", game(s)?.hero.level as u64)? as u32;
    let feet = s.sim.player().map(|p| p.pos - Vec3::Y * p.character.as_ref().unwrap().height() * 0.5).unwrap_or_default();
    let mut gm = s.sim.state.game.take().unwrap();
    let id = gm.spawn_boss(&mut s.sim, &key, level, feet + Vec3::new(0.0, 0.0, -9.0));
    s.sim.state.game = Some(gm);
    let id = id.ok_or_else(|| anyhow!("unknown boss '{key}'"))?;
    let g = game(s)?;
    let b = &g.actors[&id];
    Ok(Output::Json(
        json!({ "spawned": id.0, "name": b.name, "life": round3(b.sheet.life_max), "level": level, "skills": b.skills.iter().map(|k| d.skill(*k).key.clone()).collect::<Vec<_>>() }),
    ))
}

/// The creature a tool should show: seed=, family=, boss=, or the hero.
fn creature_look(s: &Session, a: &Args) -> Result<(String, pav_core::puppet::PuppetDef, Option<pav_core::arpg::data::SkillDef>)> {
    let d = data();
    let level = get_u64(a, "level", 20)? as u32;
    let first_skill = |skills: &[u16]| skills.first().map(|k| d.skill(*k).clone());
    if let Some(k) = get_str(a, "boss") {
        let def = pav_core::arpg::boss::boss_def(&d, k, level).ok_or_else(|| anyhow!("unknown boss '{k}'"))?;
        let spec = pav_core::arpg::boss::boss_spec(&d, &def, level).map_err(|e| anyhow!(e))?;
        return Ok((spec.name.clone(), spec.puppet.clone(), first_skill(&spec.skills)));
    }
    if let Some(k) = get_str(a, "family") {
        let f = d.family(k).ok_or_else(|| anyhow!("unknown family '{k}'"))?;
        return Ok((f.name.clone(), f.puppet.clone(), first_skill(&f.skill_ids)));
    }
    if a.contains_key("seed") || a.contains_key("body") || a.contains_key("archetype") || a.contains_key("parts") {
        let g = pav_core::arpg::genome::Genome::generate(&d, get_u64(a, "seed", 1)?, level, &genome_opts(a)?)
            .map_err(|e| anyhow!(e))?;
        let spec = g.spec(&d);
        return Ok((g.name.clone(), g.puppet.clone(), first_skill(&spec.skills)));
    }
    let p = s.sim.player().and_then(|p| p.character.as_ref()?.puppet.clone()).map(|p| (*p).clone()).unwrap_or_default();
    Ok(("the hero".into(), p, d.skill_id("slash").map(|i| d.skill(i).clone())))
}

/// Renders a creature alone on a small floor: from several angles, or through an action.
fn creature_frames(
    s: &mut Session,
    look: pav_core::puppet::PuppetDef,
    frames: usize,
    size: u32,
    act: Option<&pav_core::arpg::data::SkillDef>,
) -> Result<Vec<Vec<u8>>> {
    s.gpu()?;
    let mut tmp = Session::new("empty", 1)?;
    tmp.gpu = s.gpu.take();
    if let Some(p) = tmp.sim.state.player.take() {
        tmp.sim.despawn(p);
    }
    let scale = look.scale;
    let id = tmp.sim.spawn_npc("subject", Vec3::new(0.5, 0.0, 0.5), 0.0, look, None, None);
    tmp.sim.run(20, &pav_core::InputFrame::default());
    let feet = tmp.sim.state.entities.get(id).map(|e| e.pos).unwrap_or_default();
    tmp.sim.state.focus = feet;
    tmp.camera.params.tilt = 22.0;
    tmp.camera.params.distance = 2.2 + scale * 2.6;
    tmp.camera.params.fov = 38.0;
    tmp.camera.params.height_offset = 0.0;
    tmp.camera.params.follow_lag = 0.0;
    let mut shots = Vec::with_capacity(frames);
    let r = (|| -> Result<()> {
        for i in 0..frames {
            match act {
                None => tmp.camera.params.yaw = 30.0 + i as f32 * 360.0 / frames as f32,
                Some(def) => {
                    tmp.camera.params.yaw = 60.0;
                    if let Some(ch) = tmp.sim.state.entities.get_mut(id).and_then(|e| e.character.as_mut()) {
                        ch.anim.act_kind = def.anim.index();
                        ch.anim.act = i as f32 / (frames - 1).max(1) as f32;
                        ch.anim.act_hit = def.hit;
                        ch.anim.act_side = 1.0;
                        ch.anim.time += 0.05;
                    }
                }
            }
            shots.push(tmp.render(size, size)?);
        }
        Ok(())
    })();
    s.gpu = tmp.gpu.take();
    r?;
    Ok(shots)
}

fn save_png(a: &Args, default: &str, png: &[u8]) -> Result<std::path::PathBuf> {
    let path = std::path::PathBuf::from(get_str(a, "out").map(String::from).unwrap_or(default.into()));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, png)?;
    Ok(path)
}

pub fn t_turntable(s: &mut Session, a: &Args) -> Result<Output> {
    let (name, look, _) = creature_look(s, a)?;
    let angles = get_u64(a, "angles", 8)?.clamp(1, 16) as usize;
    let size = get_u64(a, "size", 256)?.clamp(64, 1024) as u32;
    let shots = creature_frames(s, look, angles, size, None)?;
    let cols = get_u64(a, "columns", 4)? as u32;
    let (tw, th, px) = pav_render::capture::tile_frames(&shots, size, size, cols);
    let png = pav_render::capture::encode_png(tw, th, &px)?;
    let path = save_png(a, "out/turntable.png", &png)?;
    Ok(Output::Image { png, path: Some(path.clone()), meta: json!({ "creature": name, "angles": angles, "path": path }) })
}

pub fn t_animsheet(s: &mut Session, a: &Args) -> Result<Output> {
    let d = data();
    let (name, look, first) = creature_look(s, a)?;
    let def = match get_str(a, "skill") {
        Some(k) => d.skill_id(k).map(|i| d.skill(i).clone()).ok_or_else(|| anyhow!("unknown skill '{k}'"))?,
        None => first.ok_or_else(|| anyhow!("no skill to show"))?,
    };
    let frames = get_u64(a, "frames", 8)?.clamp(2, 24) as usize;
    let size = get_u64(a, "size", 220)?.clamp(64, 1024) as u32;
    let shots = creature_frames(s, look, frames, size, Some(&def))?;
    let cols = get_u64(a, "columns", 8)? as u32;
    let (tw, th, px) = pav_render::capture::tile_frames(&shots, size, size, cols);
    let png = pav_render::capture::encode_png(tw, th, &px)?;
    let path = save_png(a, "out/animsheet.png", &png)?;
    Ok(Output::Image {
        png,
        path: Some(path.clone()),
        meta: json!({ "creature": name, "skill": def.key, "anim": format!("{:?}", def.anim).to_lowercase(), "frames": frames, "path": path }),
    })
}

/// `place=town|arena|lab|level` (with `depth=` for levels).
fn place_arg(a: &Args) -> Result<Place> {
    Ok(match get_str(a, "place").unwrap_or("town") {
        "town" => Place::Town,
        "arena" => Place::Arena,
        "lab" => Place::Lab,
        "level" => Place::Level(get_u64(a, "depth", 1)?.max(1) as u32),
        o => bail!("unknown place '{o}' (town, arena, lab, level)"),
    })
}

fn plan_json(p: &pav_core::arpg::world::LevelPlan) -> Value {
    json!({
        "depth": p.depth,
        "label": p.label(),
        "name": p.name,
        "about": p.about,
        "endless": p.endless,
        "theme": p.theme.key,
        "palette": { "floor": p.theme.floor, "wall": p.theme.wall, "accent": p.theme.accent, "light": p.theme.light, "sky": p.theme.sky },
        "mechanics": p.mechanics.iter().map(|m| json!({ "key": m.key(), "name": m.name(), "hint": m.hint(), "weight": p.weight(*m) })).collect::<Vec<_>>(),
        "rooms": p.rooms,
        "boss": p.boss,
        "monster_level": p.monster_level,
        "families": p.theme.families,
        "favoured_archetypes": p.archetypes,
        "genome_share": round3(p.genome_share),
    })
}

fn feature_name(k: &pav_core::arpg::mechanics::FeatureKind) -> &'static str {
    use pav_core::arpg::mechanics::FeatureKind as F;
    match k {
        F::Shrine { .. } => "shrine",
        F::Keg { .. } => "keg",
        F::Spikes { .. } => "spikes",
        F::Gate { .. } => "gate",
        F::Wind { .. } => "wind",
        F::Totem => "totem",
        F::Lava { .. } => "lava",
        F::Ice { .. } => "ice",
        F::Crumble { .. } => "crumble",
        F::Well { .. } => "well",
        F::Chest { .. } => "chest",
        F::Bubble { .. } => "bubble",
    }
}

/// What a depth is (no args: the level being played, with its live state).
pub fn t_level(s: &mut Session, a: &Args) -> Result<Output> {
    let d = data();
    if let Some(depth) = a.get("depth").map(|_| get_u64(a, "depth", 1)) {
        let depth = depth?.max(1) as u32;
        let to = get_u64(a, "to", depth as u64)?.max(depth as u64) as u32;
        if to > depth {
            let list: Vec<Value> = (depth..=to.min(depth + 60)).map(|n| plan_json(&pav_core::arpg::world::plan(&d, n))).collect();
            return Ok(Output::Json(json!({ "designed": pav_core::arpg::world::designed(&d), "levels": list })));
        }
        return Ok(Output::Json(plan_json(&pav_core::arpg::world::plan(&d, depth))));
    }
    let g = game(s)?;
    let lv = g.level.as_ref().ok_or_else(|| anyhow!("not in a level (try `level depth=3`, or `go place=level depth=3`)"))?;
    let mut by_kind: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    for (i, f) in lv.features.iter().enumerate() {
        let state = match &f.kind {
            pav_core::arpg::mechanics::FeatureKind::Shrine { boon, used } => json!({ "boon": boon.name(), "used": used }),
            pav_core::arpg::mechanics::FeatureKind::Gate { to, .. } => json!({ "to": to }),
            pav_core::arpg::mechanics::FeatureKind::Well { lit } => json!({ "lit": lit }),
            pav_core::arpg::mechanics::FeatureKind::Chest { state, wave, .. } => {
                json!({ "state": format!("{state:?}").to_lowercase(), "wave": wave })
            }
            pav_core::arpg::mechanics::FeatureKind::Keg { fuse } => json!({ "exploded": *fuse < 0.0 }),
            pav_core::arpg::mechanics::FeatureKind::Totem => json!({ "alive": f.entity.is_some() }),
            _ => json!({}),
        };
        by_kind
            .entry(feature_name(&f.kind))
            .or_default()
            .push(json!({ "i": i, "room": f.room, "pos": [round3(f.pos.x), round3(f.pos.z)], "state": state }));
    }
    let hero = g.hero_id.and_then(|h| s.sim.state.entities.get(h)).map(|e| e.pos);
    Ok(Output::Json(json!({
        "depth": lv.depth,
        "label": lv.label,
        "name": lv.name,
        "mechanics": lv.mechanics.iter().map(|m| m.key()).collect::<Vec<_>>(),
        "rooms": lv.layout.rooms.len(),
        "rooms_seen": lv.seen.iter().filter(|x| **x).count(),
        "hero_room": hero.and_then(|p| lv.room_at(p)),
        "exit_open": lv.exit_open,
        "boss": if lv.boss_name.is_empty() { None } else { Some(&lv.boss_name) },
        "monsters_alive": g.monsters_alive(),
        "features": by_kind,
        "spots": g.spots.iter().enumerate().map(|(i, s)| json!({ "i": i, "kind": format!("{:?}", s.kind).to_lowercase(), "name": s.name, "pos": [round3(s.pos.x), round3(s.pos.z)] })).collect::<Vec<_>>(),
        "time": round3(lv.time),
    })))
}

/// Travel anywhere at once (waypoints unlocked as needed): the place is built around the hero.
pub fn t_go(s: &mut Session, a: &Args) -> Result<Output> {
    let place = place_arg(a)?;
    let g = s.sim.state.game.as_mut().ok_or_else(|| anyhow!("this scene is not running Shardfall"))?;
    if let Place::Level(n) = place {
        g.hero.max_depth = g.hero.max_depth.max(n);
    }
    let said = run_cmd(s, GameCmd::Travel(place.code()));
    s.sync_camera();
    let g = game(s)?;
    Ok(Output::Json(
        json!({ "place": g.place.name(), "said": said, "scene": s.sim.state.scene, "monsters_alive": g.monsters_alive() }),
    ))
}

/// Puts the hero next to a level feature (shrine, keg, gate, totem, lava, well, chest, bubble,
/// spikes, wind, ice, crumble), the exit or the portal: the quickest way to look at one.
pub fn t_goto_feature(s: &mut Session, a: &Args) -> Result<Output> {
    let kind = get_str(a, "kind").ok_or_else(|| anyhow!("kind is required (shrine, keg, gate, exit, ...)"))?;
    let nth = get_u64(a, "n", 0)? as usize;
    let g = game(s)?;
    let lv = g.level.as_ref().ok_or_else(|| anyhow!("not in a level"))?;
    let pos = match kind {
        "exit" => g.spots.iter().find(|s| s.kind == pav_core::arpg::SpotKind::Exit).map(|s| s.pos),
        "portal" => g.spots.iter().find(|s| s.kind == pav_core::arpg::SpotKind::Portal).map(|s| s.pos),
        k => lv.features.iter().filter(|f| feature_name(&f.kind) == k).nth(nth).map(|f| f.pos),
    }
    .ok_or_else(|| anyhow!("no '{kind}' #{nth} in this level"))?;
    let off = match vec_arg(a, "offset")? {
        Some(v) if v.len() == 3 => Vec3::new(v[0], v[1], v[2]),
        Some(_) => bail!("offset is [x, y, z]"),
        None => Vec3::new(0.0, 0.0, 3.0),
    };
    let hid = g.hero_id.ok_or_else(|| anyhow!("no hero"))?;
    s.sim.set_position(hid, pos + off + Vec3::Y * 0.05);
    s.sim.step(&pav_core::InputFrame::default());
    s.keep_events();
    s.sync_camera();
    Ok(Output::Json(json!({ "kind": kind, "at": [round3(pos.x), round3(pos.y), round3(pos.z)], "hero": hero_json(s)? })))
}

/// A top-down map of a level (the one being played, or a fresh one built for `depth=`):
/// rooms by role, corridors, walls, every mechanic's pieces, packs and the boss.
pub fn t_levelmap(s: &mut Session, a: &Args) -> Result<Output> {
    use pav_core::arpg::mechanics::FeatureKind as F;
    let fresh;
    let sim = if a.contains_key("depth") {
        let depth = get_u64(a, "depth", 1)?.max(1);
        let seed = get_u64(a, "seed", 1)?;
        fresh = pav_core::Sim::new(&format!("level/{depth}"), seed)?;
        &fresh
    } else {
        &s.sim
    };
    let g = sim.state.game.as_deref().ok_or_else(|| anyhow!("not running Shardfall"))?;
    let lv = g.level.as_ref().ok_or_else(|| anyhow!("not in a level (pass depth=N)"))?;
    let size = get_u64(a, "size", 900)?.clamp(256, 4096) as u32;
    let b = lv.layout.bounds();
    let span = (b.max - b.min).max_element() + 8.0;
    let k = size as f32 / span;
    let o = (b.min + b.max) * 0.5;
    let at = |x: f32, z: f32| (size as f32 * 0.5 + (x - o.x) * k, size as f32 * 0.5 + (z - o.y) * k);
    let mut c = Canvas::new(size, size, [16, 15, 20]);
    let hexc = |h: &str| {
        let v = u32::from_str_radix(h.trim_start_matches('#'), 16).unwrap_or(0x808080);
        [(v >> 16) as u8, (v >> 8) as u8, v as u8]
    };
    let theme = pav_core::arpg::world::plan(&data(), lv.depth).theme;
    for cor in &lv.layout.corridors {
        c.rect(at(cor.rect.min.x, cor.rect.min.y), at(cor.rect.max.x, cor.rect.max.y), hexc(&theme.floor[1]), 1.0);
    }
    for r in &lv.layout.rooms {
        let col = match r.role {
            pav_core::arpg::levelgen::RoomRole::Start => [70, 120, 170],
            pav_core::arpg::levelgen::RoomRole::Exit => [180, 150, 70],
            _ => hexc(&theme.floor[0]),
        };
        c.rect(at(r.rect.min.x - 1.0, r.rect.min.y - 1.0), at(r.rect.max.x + 1.0, r.rect.max.y + 1.0), hexc(&theme.wall), 1.0);
        c.rect(at(r.rect.min.x, r.rect.min.y), at(r.rect.max.x, r.rect.max.y), col, 1.0);
    }
    for cor in &lv.layout.corridors {
        c.rect(at(cor.rect.min.x, cor.rect.min.y), at(cor.rect.max.x, cor.rect.max.y), hexc(&theme.floor[1]), 1.0);
    }
    for f in &lv.features {
        let (x, y) = at(f.pos.x, f.pos.z);
        match &f.kind {
            F::Ice { min, max } => c.rect(at(min.x, min.y), at(max.x, max.y), [170, 215, 240], 0.45),
            F::Crumble { min, max } => c.rect(at(min.x, min.y), at(max.x, max.y), [60, 40, 30], 0.45),
            F::Wind { min, max, .. } => c.rect(at(min.x, min.y), at(max.x, max.y), [200, 235, 255], 0.3),
            F::Lava { radius, .. } => c.disc(x, y, radius * k, [255, 100, 30]),
            F::Bubble { radius, .. } => c.disc(x, y, radius * k, [90, 170, 255]),
            F::Spikes { half, .. } => {
                c.rect(at(f.pos.x - half, f.pos.z - half), at(f.pos.x + half, f.pos.z + half), [150, 60, 50], 0.8)
            }
            _ => {}
        }
    }
    for f in &lv.features {
        let (x, y) = at(f.pos.x, f.pos.z);
        let r = (k * 0.8).max(3.0);
        match &f.kind {
            F::Shrine { boon, .. } => c.disc(x, y, r * 1.2, hexc(boon.color())),
            F::Keg { fuse } if *fuse >= 0.0 => c.disc(x, y, r * 0.7, [210, 120, 40]),
            F::Gate { to, .. } => {
                c.disc(x, y, r * 1.3, [180, 110, 255]);
                if let Some(t) = lv.features.get(*to) {
                    c.line((x, y), at(t.pos.x, t.pos.z), 1.0, [140, 90, 200]);
                }
            }
            F::Totem if f.entity.is_some() => c.disc(x, y, r, [150, 230, 90]),
            F::Well { lit } => c.disc(x, y, r, if *lit { [255, 190, 90] } else { [130, 60, 30] }),
            F::Chest { .. } => c.disc(x, y, r * 1.2, [160, 80, 255]),
            _ => {}
        }
    }
    for (id, act) in &g.actors {
        if act.team != Team::Monster || act.dead || act.family == "totem" {
            continue;
        }
        let Some(e) = sim.state.entities.get(*id) else { continue };
        let (x, y) = at(e.pos.x, e.pos.z);
        let (r, col) = match act.rarity {
            Rarity::Unique => (k * 1.6, [255, 60, 40]),
            Rarity::Rare => (k * 0.9, [255, 220, 90]),
            Rarity::Magic => (k * 0.6, [120, 150, 255]),
            Rarity::Normal => (k * 0.45, [220, 70, 60]),
        };
        c.disc(x, y, r.max(1.5), col);
    }
    for sp in &g.spots {
        let (x, y) = at(sp.pos.x, sp.pos.z);
        match sp.kind {
            pav_core::arpg::SpotKind::Exit => c.disc(x, y, k * 1.8, [255, 215, 110]),
            pav_core::arpg::SpotKind::Portal => c.disc(x, y, k * 1.5, [110, 200, 255]),
            _ => {}
        }
    }
    if let Some(h) = g.hero_id.and_then(|h| sim.state.entities.get(h)) {
        let (x, y) = at(h.pos.x, h.pos.z);
        c.disc(x, y, (k * 0.9).max(3.0), [255, 255, 255]);
    }
    let png = pav_render::capture::encode_png(size, size, &c.px)?;
    let path = std::path::PathBuf::from(get_str(a, "out").map(String::from).unwrap_or(format!("out/level{}.png", lv.depth)));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &png)?;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &lv.features {
        *counts.entry(feature_name(&f.kind)).or_default() += 1;
    }
    let meta = json!({
        "path": path,
        "depth": lv.depth,
        "name": lv.name,
        "rooms": lv.layout.rooms.len(),
        "corridors": lv.layout.corridors.len(),
        "features": counts,
        "monsters": g.monsters_alive(),
        "boss": if lv.boss_name.is_empty() { None } else { Some(&lv.boss_name) },
        "legend": "blue room = start, gold room = exit; red/blue/yellow dots = normal/magic/rare monsters; big red = boss; orange discs = lava; blue discs = time bubbles; pale = ice/wind; dark = crumbling; violet = rift gates (linked); coloured = shrines; brown = kegs; green = totems; purple = cursed chests",
    });
    Ok(Output::Image { png, path: Some(path), meta })
}
