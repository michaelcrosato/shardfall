//! Tools for agents that build and judge content rather than play it:
//! - `see`: a screenshot with numbered marks on everything that matters (monsters, the hero,
//!   townsfolk, loot, spots, level pieces) and a legend saying what each number is: visual
//!   grounding, so an agent can talk about "mark 7" and know it is a rare Frost Ghoul at 40%.
//! - `campaign`: the bot plays down through the levels and reports each one (time, kills,
//!   deaths, damage taken, hero level): the balance pass in one call.
//! - `theme_swatch`: every theme's palette, and the blends the endless depths make.
//! - `anatomy` numbers for any creature (also in `turntable`'s report), and `def=` overrides
//!   for authoring creatures as JSON on top of a family, boss, genome or the hero.

use std::collections::BTreeMap;

use pav_core::params::ChoiceParam;

use anyhow::{Result, anyhow};
use glam::{Mat4, Vec3};
use pav_core::arpg::combat::{Rarity, Team};
use pav_core::arpg::data::data;
use pav_core::arpg::{Place, SpotKind};
use serde_json::{Value, json};

use crate::game_tools::Canvas;
use crate::session::Session;
use crate::tools::{Args, Output, get_str, get_u64, round3};

/// Something to mark on a screenshot.
struct Mark {
    kind: &'static str,
    name: String,
    pos: Vec3,
    color: [u8; 3],
    extra: Value,
}

fn rarity_rgb(r: Rarity) -> [u8; 3] {
    match r {
        Rarity::Normal => [230, 230, 230],
        Rarity::Magic => [120, 150, 255],
        Rarity::Rare => [255, 220, 90],
        Rarity::Unique => [255, 140, 50],
    }
}

fn marks(s: &Session) -> Vec<Mark> {
    let mut out = Vec::new();
    let sim = &s.sim;
    let feet = |id| {
        let e = sim.state.entities.get(id)?;
        let h = e.character.as_ref().map(|c| c.height()).or_else(|| e.visual.as_ref().map(|v| v.shape.half_extents().y * 2.0))?;
        Some((e.pos - Vec3::Y * h * 0.5, h))
    };
    if let Some(g) = sim.state.game.as_deref() {
        for (id, a) in &g.actors {
            if a.dead {
                continue;
            }
            let Some((f, h)) = feet(*id) else { continue };
            let (kind, color) = match a.team {
                Team::Hero => ("hero", [255, 255, 255]),
                Team::Monster if a.family == "totem" => ("totem", [150, 230, 90]),
                Team::Monster if a.boss.is_some() => ("boss", [255, 60, 40]),
                Team::Monster => ("monster", rarity_rgb(a.rarity)),
                Team::Neutral => ("keg", [210, 120, 40]),
            };
            let mut extra = json!({
                "id": id.0,
                "level": a.level,
                "life": round3(a.life / a.sheet.life_max.max(1.0)),
            });
            if a.team == Team::Monster {
                extra["rarity"] = json!(format!("{:?}", a.rarity).to_lowercase());
                extra["family"] = json!(a.family);
                extra["aggro"] = json!(a.brain.as_ref().is_some_and(|b| b.aggro));
                if !a.affixes.is_empty() {
                    extra["affixes"] = json!(a.affixes);
                }
            }
            out.push(Mark { kind, name: a.name.clone(), pos: f + Vec3::Y * h, color, extra });
        }
        for n in &g.npcs {
            if let Some((f, h)) = feet(n.id) {
                out.push(Mark {
                    kind: "townsfolk",
                    name: n.name.clone(),
                    pos: f + Vec3::Y * h,
                    color: [140, 255, 160],
                    extra: json!({ "id": n.id.0, "role": format!("{:?}", n.role).to_lowercase() }),
                });
            }
        }
        for (i, sp) in g.spots.iter().enumerate() {
            if sp.name.is_empty() || (sp.reach <= 0.0 && sp.kind != SpotKind::Exit) {
                continue;
            }
            out.push(Mark {
                kind: "spot",
                name: sp.name.clone(),
                pos: sp.pos + Vec3::Y * 0.3,
                color: [255, 230, 120],
                extra: json!({ "spot": i, "use": format!("{:?}", sp.kind).to_lowercase() }),
            });
        }
        for l in &g.loot {
            out.push(Mark {
                kind: "loot",
                name: l.item.name.clone(),
                pos: l.pos,
                color: rarity_rgb(l.item.rarity),
                extra: json!({ "item": l.item.id, "rarity": format!("{:?}", l.item.rarity).to_lowercase() }),
            });
        }
        if let Some(lv) = &g.level {
            use pav_core::arpg::mechanics::FeatureKind as F;
            for (i, f) in lv.features.iter().enumerate() {
                let (name, state) = match &f.kind {
                    F::Shrine { boon, used } => (format!("Shrine of {}", boon.name()), json!({ "used": used })),
                    F::Gate { to, .. } => ("Rift gate".to_string(), json!({ "to_feature": to })),
                    F::Well { lit } => ("Well".to_string(), json!({ "lit": lit })),
                    F::Chest { state, .. } => {
                        ("Cursed chest".to_string(), json!({ "state": format!("{state:?}").to_lowercase() }))
                    }
                    F::Lava { radius, .. } => ("Lava".to_string(), json!({ "radius": round3(*radius) })),
                    F::Bubble { radius, .. } => ("Time bubble".to_string(), json!({ "radius": round3(*radius) })),
                    F::Spikes { .. } => ("Spike plate".to_string(), json!({})),
                    _ => continue,
                };
                out.push(Mark {
                    kind: "feature",
                    name,
                    pos: f.pos + Vec3::Y * 0.2,
                    color: [120, 230, 255],
                    extra: json!({ "feature": i, "state": state }),
                });
            }
        }
    } else {
        // Not a game: characters and named things.
        for e in sim.state.entities.iter() {
            if e.name.is_empty() || e.name.starts_with('~') || (e.character.is_none() && e.visual.is_none()) {
                continue;
            }
            let kind = if Some(e.id) == sim.state.player {
                "player"
            } else if e.character.is_some() {
                "character"
            } else {
                "object"
            };
            out.push(Mark { kind, name: e.name.clone(), pos: e.pos, color: [255, 230, 120], extra: json!({ "id": e.id.0 }) });
        }
    }
    out
}

/// A screenshot with numbered marks and a legend: `see` (any scene; game scenes mark
/// monsters, the hero, townsfolk, loot, spots and level pieces).
pub fn t_see(s: &mut Session, a: &Args) -> Result<Output> {
    let w = get_u64(a, "width", 1280)?.clamp(160, 4096) as u32;
    let h = get_u64(a, "height", 720)?.clamp(120, 4096) as u32;
    let max = get_u64(a, "max", 40)? as usize;
    let only = get_str(a, "only");
    let (rgba, vp) = s.render_with_camera(w, h)?;
    let mut c = Canvas { w, h, px: rgba };
    let eye = s.camera_eye();
    let to_px = |p: Vec3, vp: &Mat4| -> Option<(f32, f32, f32)> {
        let q = *vp * p.extend(1.0);
        if q.w <= 0.05 {
            return None;
        }
        let n = q.truncate() / q.w;
        if n.x.abs() > 1.0 || n.y.abs() > 1.0 {
            return None;
        }
        Some(((n.x * 0.5 + 0.5) * w as f32, (0.5 - n.y * 0.5) * h as f32, (p - eye).length()))
    };
    let mut seen: Vec<(Mark, (f32, f32, f32))> = marks(s)
        .into_iter()
        .filter(|m| only.is_none_or(|o| o.split(',').any(|k| k.trim() == m.kind)))
        .filter_map(|m| to_px(m.pos, &vp).map(|p| (m, p)))
        .collect();
    seen.sort_by(|x, y| x.1.2.total_cmp(&y.1.2));
    seen.truncate(max);
    let mut legend = Vec::new();
    for (i, (m, (x, y, dist))) in seen.iter().enumerate() {
        let n = i + 1;
        c.ring(*x, *y, 7.0, 2.0, m.color);
        // The number in a dark box just above the mark.
        let scale = 3.0;
        let tw = c.number_width(n, scale);
        let (bx, by) = (x - tw * 0.5, y - 14.0 - 5.0 * scale);
        c.rect((bx - 3.0, by - 3.0), (bx + tw + 3.0, by + 5.0 * scale + 3.0), [10, 10, 14], 0.8);
        c.number(bx, by, n, scale, m.color);
        let mut e = json!({
            "n": n,
            "kind": m.kind,
            "name": m.name,
            "px": [x.round(), y.round()],
            "dist": round3(*dist),
            "pos": [round3(m.pos.x), round3(m.pos.y), round3(m.pos.z)],
        });
        if let (Value::Object(dst), Value::Object(src)) = (&mut e, &m.extra) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
        legend.push(e);
    }
    let png = pav_render::capture::encode_png(w, h, &c.px)?;
    let path =
        std::path::PathBuf::from(get_str(a, "out").map(String::from).unwrap_or(format!("out/see-{}.png", s.sim.state.tick)));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &png)?;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (m, _) in &seen {
        *counts.entry(m.kind).or_default() += 1;
    }
    Ok(Output::Image {
        png,
        path: Some(path.clone()),
        meta: json!({ "path": path, "tick": s.sim.state.tick, "marks": legend, "counts": counts }),
    })
}

/// The bot plays down through the levels: each finished level is a row.
pub fn t_campaign(s: &mut Session, a: &Args) -> Result<Output> {
    let from = get_u64(a, "from", 0)? as u32;
    let to = get_u64(a, "to", 12)? as u32;
    let secs = a.get("seconds").and_then(|v| v.as_f64()).unwrap_or(1800.0).clamp(1.0, 36_000.0);
    let hz = s.sim.config.tick_rate.hz() as f64;
    {
        let g = s.sim.state.game.as_mut().ok_or_else(|| anyhow!("not running Shardfall (load scene=town)"))?;
        if let Some(l) = a.get("hero_level").and_then(|v| v.as_u64()) {
            g.hero.level = l as u32;
        }
        if from > 0 {
            g.hero.max_depth = g.hero.max_depth.max(from);
        }
    }
    if a.contains_key("hero_level") {
        let mut g = s.sim.state.game.take().unwrap();
        pav_core::arpg::refresh_hero(&mut s.sim, &mut g, true);
        s.sim.state.game = Some(g);
    }
    if from > 0 && s.sim.state.game.as_ref().map(|g| g.place) != Some(Place::Level(from)) {
        s.sim.step(&pav_core::InputFrame {
            cmd: Some(pav_core::arpg::GameCmd::Travel(Place::Level(from).code())),
            ..Default::default()
        });
    }
    let mut bot = pav_core::arpg::bot::Bot::default();
    let snapshot = |s: &Session| {
        let g = s.sim.state.game.as_ref().unwrap();
        (g.place, s.sim.state.tick, g.hero.kills, g.hero.deaths, g.hero.level, g.hero.gold)
    };
    let mut start = snapshot(s);
    let mut dmg0 = 0.0f32;
    let mut rows = Vec::new();
    let t0 = std::time::Instant::now();
    let total = (secs * hz) as u64;
    for _ in 0..total {
        let f = bot.input(&s.sim);
        s.sim.step(&f);
        let now = snapshot(s);
        if now.0 != start.0 {
            let ticks = now.1 - start.1;
            rows.push(json!({
                "place": start.0.name(),
                "minutes": round3((ticks as f64 / hz / 60.0) as f32),
                "kills": now.2 - start.2,
                "deaths": now.3 - start.3,
                "damage_taken": bot.stats.damage_taken - dmg0,
                "hero_level": [start.4, now.4],
                "gold": now.5.saturating_sub(start.5),
            }));
            dmg0 = bot.stats.damage_taken;
            start = now;
            if let Place::Level(n) = now.0 {
                if n > to {
                    break;
                }
            }
        }
    }
    s.keep_events();
    s.sync_camera();
    let g = s.sim.state.game.as_ref().unwrap();
    let lv = g.level.as_ref();
    Ok(Output::Json(json!({
        "levels": rows,
        "now": {
            "place": g.place.name(),
            "rooms_seen": lv.map(|l| l.seen.iter().filter(|x| **x).count()),
            "exit_open": lv.map(|l| l.exit_open),
            "hero_level": g.hero.level,
            "deaths": g.hero.deaths,
            "kills": g.hero.kills,
        },
        "game_minutes": round3((bot.stats.ticks as f64 / hz / 60.0) as f32),
        "wall_seconds": round3(t0.elapsed().as_secs_f32()),
    })))
}

/// Every theme's colours as swatch rows (and `depths=a-b`: the palettes those endless depths
/// blend), with a legend.
pub fn t_theme_swatch(_: &mut Session, a: &Args) -> Result<Output> {
    let d = data();
    let mut rows: Vec<(String, pav_core::arpg::world::ThemeDef)> = d.themes.iter().map(|(k, t)| (k.clone(), t.clone())).collect();
    if let Some(r) = get_str(a, "depths") {
        let (lo, hi) = r.split_once('-').ok_or_else(|| anyhow!("depths=13-20"))?;
        let (lo, hi): (u32, u32) = (lo.trim().parse()?, hi.trim().parse()?);
        for n in lo..=hi.min(lo + 40) {
            let p = pav_core::arpg::world::plan(&d, n);
            rows.push((format!("depth {n}: {}", p.name), p.theme));
        }
    }
    let cell = 48u32;
    let cols = 7u32;
    let (w, h) = (cols * cell + 8, rows.len() as u32 * (cell + 6) + 8);
    let mut c = Canvas::new(w, h, [20, 19, 24]);
    let hexc = |h: &str| {
        let v = u32::from_str_radix(h.trim_start_matches('#'), 16).unwrap_or(0x808080);
        [(v >> 16) as u8, (v >> 8) as u8, v as u8]
    };
    let mut legend = Vec::new();
    for (i, (name, t)) in rows.iter().enumerate() {
        let y = 4.0 + i as f32 * (cell + 6) as f32;
        let swatches = [&t.floor[0], &t.floor[1], &t.wall, &t.pillar, &t.accent, &t.light, &t.sky];
        for (k, col) in swatches.iter().enumerate() {
            let x = 4.0 + k as f32 * cell as f32;
            c.rect((x, y), (x + cell as f32 - 4.0, y + cell as f32), hexc(col), 1.0);
        }
        legend.push(json!({ "row": i + 1, "theme": name, "particles": t.particles, "families": t.families }));
    }
    let png = pav_render::capture::encode_png(w, h, &c.px)?;
    let path = std::path::PathBuf::from(get_str(a, "out").unwrap_or("out/themes.png"));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &png)?;
    Ok(Output::Image {
        png,
        path: Some(path.clone()),
        meta: json!({ "path": path, "columns": ["floor", "floor 2", "wall", "pillar", "accent", "light", "sky"], "rows": legend }),
    })
}

/// Body measurements of a creature as posed at rest: height, length, width, how many parts,
/// its colours.
pub fn anatomy(def: &pav_core::puppet::PuppetDef) -> Value {
    let st = pav_core::puppet::PuppetState::default();
    let (parts, _) = pav_core::puppet::pose_ex(def, &st, None, Vec3::ZERO, Vec3::new(0.0, -0.6, -0.8).normalize());
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &parts {
        for (c, r) in [(p.a, p.ra), (p.b, p.rb)] {
            lo = lo.min(c - Vec3::splat(r));
            hi = hi.max(c + Vec3::splat(r));
        }
    }
    let size = hi - lo;
    json!({
        "body": def.body.name(),
        "height": round3(size.y),
        "width": round3(size.x),
        "length": round3(size.z),
        "parts": parts.len(),
        "attachments": def.parts.iter().map(|p| p.kind.name()).collect::<Vec<_>>(),
        "colors": { "skin": def.skin, "body": def.shirt, "accent": def.accent, "eyes": def.eyes },
        "legs": def.leg_pairs() * 2,
        "tail": round3(def.tail_length * def.scale),
    })
}
