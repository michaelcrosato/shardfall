//! Prop inspection and placement through the same session used by the native studio.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use glam::{Quat, Vec3};
use pav_core::issue::{self, Code, Issue};
use pav_core::prop_instance::{PropInstance, transform_bounds, validate_pose, validate_roles, validate_scale};
use pav_core::prop_preview::{PropPreviewChange, StudioMode, figure_bounds};
use pav_core::props;
use pav_core::statics::RegionKey;
use pav_core::{Entity, EntityId};
use serde_json::{Value, json};

use crate::preview_tools::{StudioCheckpoint, opt_bool, opt_float, opt_text, step_arg, switch_camera};
use crate::session::Session;
use crate::tools::{Args, Output};

/// `open` says whether the object tab is showing. Playback fields appear only when they
/// differ from a fresh stage (playing, speed 1, time 0, no turntable, yaw 0, scale 1).
fn status(s: &Session) -> Value {
    let mut out = json!({"open": s.sim.studio_mode() == StudioMode::Prop});
    let Some(preview) = s.sim.state.prop_preview.as_ref() else { return out };
    let info = preview.info();
    out["name"] = json!(info.name);
    if info.available {
        out["revision"] = json!(info.revision);
        out["parts"] = json!(info.parts);
        out["size_cm"] = json!(info.bounds.size_cm());
    } else {
        out["available"] = json!(false);
    }
    let fresh = [
        ("playing", json!(info.playing), json!(true)),
        ("speed", json!(info.speed), json!(1.0)),
        ("time", json!(info.time), json!(0.0)),
        ("turntable", json!(info.turntable), json!(false)),
        ("yaw", json!(info.yaw), json!(0.0)),
        ("scale", json!(info.scale), json!(1.0)),
        ("seed", json!(info.seed), json!(0)),
    ];
    for (key, value, default) in fresh {
        if value != default {
            out[key] = value;
        }
    }
    out
}

/// Session startup loads saved definitions. A preview lookup never publishes a disk change:
/// only explicit edits and the watcher also refresh every affected placed collider.
fn ensure_asset(name: &str) -> Result<String> {
    let name = props::canonical(name).map_err(|e| anyhow!(e))?;
    if props::get(&name).is_none() {
        bail!("no prop '{name}' (assets lists available props)");
    }
    Ok(name)
}

/// Fit the visible bounds once; accepted edits to the same asset preserve the user's view.
pub fn fit_camera(s: &mut Session) -> Result<()> {
    let preview = s.sim.state.prop_preview.as_ref().ok_or_else(|| anyhow!("no prop preview is selected"))?;
    let definition = props::get(&preview.name).ok_or_else(|| anyhow!("the selected prop is unavailable"))?;
    let local = definition.bounds(preview.scale);
    let lift = -Vec3::Y * local.min.y;
    let mut bounds = transform_bounds(local, lift, preview.rotation());
    if preview.turntable {
        for i in 0..48 {
            bounds = bounds.union(transform_bounds(local, lift, Quat::from_rotation_y(i as f32 * std::f32::consts::TAU / 48.0)));
        }
    }
    s.camera.params = pav_view::CameraParams {
        tilt: 22.0,
        yaw: 45.0,
        fov: 38.0,
        height_offset: 0.0,
        follow_lag: 0.0,
        distance: 5.0,
        ortho: false,
    };
    let center = bounds.center();
    let reach = (s.camera.params.fov.to_radians() * 0.5).tan() / 1.25;
    let (forward, up, right) = (s.camera.forward(), s.camera.up(), s.camera.ground_axes().1);
    let mut distance = 2.0_f32;
    for corner in 0..8 {
        let q = Vec3::new(
            if corner & 1 == 0 { bounds.min.x } else { bounds.max.x },
            if corner & 2 == 0 { bounds.min.y } else { bounds.max.y },
            if corner & 4 == 0 { bounds.min.z } else { bounds.max.z },
        ) - center;
        distance = distance.max(q.dot(up).abs().max(q.dot(right).abs()) / reach - q.dot(forward));
    }
    s.camera.params.distance = distance;
    s.camera.snap(center);
    s.sim.state.prop_preview.as_mut().unwrap().focus = center;
    Ok(())
}

pub fn t_asset_preview(s: &mut Session, a: &Args) -> Result<Output> {
    if a.get("views").is_some_and(|v| v != false && v != "false") {
        return sheet(s, a);
    }
    if let Some(key) = SHEET_ARGS.iter().find(|key| a.contains_key(**key)) {
        bail!("{}", Issue::error(*key, Code::Missing, "needs views=").fix("add views=true for a contact sheet"));
    }
    allowed_keys(a, &["action", "name", "close", "playing", "time", "speed", "turntable", "yaw", "scale", "step", "views"])?;
    let action = opt_text(a, "action")?.unwrap_or("status");
    one_of("action", action, &["status", "open", "fit", "close", "play", "pause", "restart"])?;
    let name = opt_text(a, "name")?;
    let close = opt_bool(a, "close")?.unwrap_or(false) || action == "close";
    let mut change = PropPreviewChange {
        time: opt_float(a, "time")?,
        playing: opt_bool(a, "playing")?,
        speed: opt_float(a, "speed")?,
        turntable: opt_bool(a, "turntable")?,
        yaw: opt_float(a, "yaw")?,
        scale: opt_float(a, "scale")?,
        step: step_arg(a)?,
        seed: seed_arg(a)?,
    };
    match action {
        "play" => change.playing = Some(true),
        "pause" => change.playing = Some(false),
        "restart" => {
            change.time = Some(0.0);
            if change.playing.is_none() {
                change.playing = Some(true);
            }
        }
        _ => {}
    }
    change.validate().map_err(|e| anyhow!(e))?;
    let controls =
        a.keys().any(|k| matches!(k.as_str(), "time" | "playing" | "speed" | "turntable" | "yaw" | "scale" | "step" | "seed"));
    if close && (name.is_some() || controls || matches!(action, "open" | "fit" | "play" | "pause" | "restart")) {
        bail!("close cannot select or alter a prop preview");
    }
    if !close && name.is_none() && !controls && action == "status" {
        return Ok(Output::Json(status(s)));
    }
    let checkpoint = StudioCheckpoint::new(s);
    let before_mode = s.sim.studio_mode();
    let before_name = s.sim.state.prop_preview.as_ref().map(|p| p.name.clone());
    let result = (|| -> Result<()> {
        if close {
            s.sim.prop_preview_close();
            switch_camera(s, before_mode)?;
            return Ok(());
        }
        let requested = name.map(str::to_string).or_else(|| before_name.clone()).unwrap_or_else(|| "BUILTIN/bench".into());
        let requested = ensure_asset(&requested)?;
        s.sim.prop_preview_open(Some(&requested)).map_err(|e| anyhow!(e))?;
        s.sim.prop_preview_change(&change).map_err(|e| anyhow!(e))?;
        switch_camera(s, before_mode)?;
        if before_name.as_deref() != Some(&requested) {
            s.clear_particles();
        }
        if before_name.as_deref() != Some(&requested) || action == "fit" {
            fit_camera(s)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        checkpoint.restore(s);
        return Err(error);
    }
    s.prev_frame = s.sim.frame();
    Ok(Output::Json(status(s)))
}

const SHEET_ARGS: [&str; 3] = ["elevation", "tile", "figure"];
const VIEWS: [&str; 6] = ["front", "right", "back", "left", "top", "iso"];

/// One tile of a contact sheet: its label and the camera's yaw and tilt (degrees).
struct View {
    label: Value,
    yaw: f32,
    tilt: f32,
}

/// `views=true` is front, right, back and iso; a list may also name left and top, or give
/// degrees around from the front toward the right side.
fn views(value: &Value, tilt: f32) -> Result<Vec<View>> {
    let items: Vec<Value> = match value {
        Value::Bool(true) => ["front", "right", "back", "iso"].map(Value::from).to_vec(),
        Value::String(text) if text == "true" => ["front", "right", "back", "iso"].map(Value::from).to_vec(),
        Value::String(text) => {
            text.trim_matches(['[', ']']).split(',').map(|t| Value::from(t.trim().trim_matches('"'))).collect()
        }
        Value::Array(items) => items.clone(),
        _ => bail!("{}", Issue::error("views", Code::InvalidType, "views is true or a list of sides").expected(VIEWS.join(", "))),
    };
    if !(1..=8).contains(&items.len()) {
        bail!("{}", Issue::error("views", Code::Limit, format!("{} views", items.len())).expected("1 to 8"));
    }
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let text = item.as_str().unwrap_or("");
        if let Some(degrees) = item.as_f64().or_else(|| text.trim_end_matches('°').parse().ok()) {
            if !(-360.0..=360.0).contains(&degrees) {
                bail!(
                    "{}",
                    Issue::error(format!("views[{i}]"), Code::OutOfRange, format!("{degrees}")).expected("-360 to 360 degrees")
                );
            }
            out.push(View { label: json!(degrees), yaw: -degrees as f32, tilt });
            continue;
        }
        let (yaw, tilt) = match text {
            "front" => (0.0, tilt),
            "right" => (-90.0, tilt),
            "back" => (180.0, tilt),
            "left" => (90.0, tilt),
            "top" => (0.0, 90.0),
            "iso" => (-45.0, tilt.max(30.0)),
            _ => bail!(
                "{}",
                Issue::error(format!("views[{i}]"), Code::UnknownValue, format!("unknown view \"{text}\""))
                    .expected("front, right, back, left, top, iso or degrees")
                    .maybe_fix(issue::suggestion(text, VIEWS))
            ),
        };
        out.push(View { label: json!(text), yaw, tilt });
    }
    Ok(out)
}

/// A camera that fits `points` in a square tile from one side, the larger extent at about 80%.
fn sheet_camera(points: &[Vec3], yaw: f32, tilt: f32, fov: f32) -> pav_view::CameraRig {
    let (lo, hi) = points.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let center = (lo + hi) * 0.5;
    let params = pav_view::CameraParams { tilt, yaw, distance: 1.0, fov, ortho: false, follow_lag: 0.0, height_offset: 0.0 };
    let mut rig = pav_view::CameraRig::new(params);
    let (forward, up, right) = (rig.forward(), rig.up(), rig.ground_axes().1);
    let side = |q: Vec3| q.dot(up).abs().max(q.dot(right).abs());
    // Every point stays 0.15 m past the eye (the near plane is at 0.1 m).
    let closest = 0.15 - points.iter().map(|p| (*p - center).dot(forward)).fold(0.0, f32::min);
    let reach = (fov.to_radians() * 0.5).tan() * 0.8;
    let fit = points.iter().map(|p| side(*p - center) / reach - (*p - center).dot(forward)).fold(0.0, f32::max);
    rig.params.distance = fit.max(closest);
    if fit < closest {
        // Small things: the camera stays that close and the lens narrows instead.
        let needed = points.iter().map(|p| side(*p - center) / (closest + (*p - center).dot(forward))).fold(0.0, f32::max);
        rig.params.fov = (2.0 * (needed / 0.8).atan()).to_degrees().clamp(1.0, fov);
    }
    rig.snap(center);
    rig
}

/// `views=`: one PNG of the asset from several sides, each tile framed to fit. The stage, its
/// controls and the human's camera are as they were afterwards.
fn sheet(s: &mut Session, a: &Args) -> Result<Output> {
    let allowed = ["name", "views", "elevation", "tile", "figure", "scene", "seed", "ticks"];
    if let Some(key) = a.keys().find(|key| !allowed.contains(&key.as_str())) {
        bail!(
            "{}",
            Issue::error(key.as_str(), Code::UnknownKey, "not an asset_preview argument with views")
                .expected(allowed[..5].join(", "))
        );
    }
    let (tilt, fov) = match a.get("elevation") {
        None => (20.0, 38.0),
        Some(value) if value == "game" => (56.0, 40.0),
        Some(value) => {
            let degrees = value.as_f64().or_else(|| value.as_str().and_then(|t| t.parse().ok()));
            match degrees.filter(|d| (0.0..=90.0).contains(d)) {
                Some(degrees) => (degrees as f32, 38.0),
                None => bail!(
                    "{}",
                    Issue::error("elevation", Code::OutOfRange, format!("elevation {value}"))
                        .expected("0 to 90 degrees, or game")
                ),
            }
        }
    };
    let views = views(&a["views"], tilt)?;
    let tile = crate::tools::get_u64(a, "tile", 240)?;
    if !(64..=1024).contains(&tile) {
        bail!("{}", Issue::error("tile", Code::OutOfRange, format!("tile {tile}")).expected("64 to 1024 pixels"));
    }
    let tile = tile as u32;
    let name = match opt_text(a, "name")? {
        Some(name) => ensure_asset(name)?,
        None => match s.sim.state.prop_preview.as_ref() {
            Some(preview) => ensure_asset(&preview.name)?,
            None => bail!(
                "{}",
                Issue::error("name", Code::Missing, "views needs an asset").fix("add name=BUILTIN/bench or a WORKSHOP prop")
            ),
        },
    };
    let asset = props::get(&name).ok_or_else(|| anyhow!("no prop '{name}'"))?;
    let seed = seed_arg(a)?.unwrap_or(0);
    let parts = asset.variant(seed);
    let bounds = parts.bounds_at(1.0);
    let lift = -Vec3::Y * bounds.min.y;
    let drawn = parts.parts.iter().filter(|part| !part.hidden);
    let mut points: Vec<Vec3> = drawn.flat_map(|part| part.hull(1.0)).map(|p| p + lift).collect();
    if points.is_empty() {
        bail!(
            "{}",
            Issue::error("name", Code::Missing, format!("{name} draws nothing: every part is hidden or left out at seed {seed}"))
                .fix("asset_edit action=inspect bounds=true measures the hidden parts")
        );
    }
    let wanted = opt_bool(a, "figure")?.unwrap_or(false);
    let figure = wanted && (0.5..=20.0).contains(&bounds.size().max_element());
    if figure {
        let b = figure_bounds(bounds);
        points.extend((0..8).map(|i| Vec3::select(glam::BVec3::new(i & 1 > 0, i & 2 > 0, i & 4 > 0), b.max, b.min)));
    }
    let checkpoint = StudioCheckpoint::new(s);
    let events = std::mem::take(&mut s.events);
    // The sheet's subject starts with its own particles, and leaves none on the stage.
    s.clear_particles();
    let shots = (|| -> Result<Vec<Vec<u8>>> {
        s.sim.prop_preview_open(Some(&name)).map_err(|e| anyhow!(e))?;
        let still = PropPreviewChange {
            time: Some(0.0),
            playing: Some(false),
            turntable: Some(false),
            yaw: Some(0.0),
            scale: Some(1.0),
            seed: Some(seed),
            ..Default::default()
        };
        s.sim.prop_preview_change(&still).map_err(|e| anyhow!(e))?;
        let mut shots = Vec::with_capacity(views.len());
        for view in &views {
            s.camera = sheet_camera(&points, view.yaw, view.tilt, fov);
            let preview = s.sim.state.prop_preview.as_mut().unwrap();
            preview.figure = figure;
            preview.focus = s.camera.target;
            shots.push(s.render(tile, tile)?);
        }
        Ok(shots)
    })();
    checkpoint.restore(s);
    s.events = events;
    s.clear_particles();
    let shots = shots?;
    let (width, height, rgba) = pav_render::capture::tile_frames(&shots, tile, tile, views.len().min(4) as u32);
    let png = pav_render::capture::encode_png(width, height, &rgba)?;
    let leaf = name.rsplit('/').next().unwrap_or(&name);
    let path = std::path::PathBuf::from(format!("out/sheet-{leaf}.png"));
    std::fs::create_dir_all("out")?;
    std::fs::write(&path, &png)?;
    let mut meta = json!({"path": path, "width": width, "height": height, "views": views.iter().map(|v| v.label.clone()).collect::<Vec<_>>()});
    if wanted {
        meta["figure"] = if figure { json!(true) } else { json!("off: the largest side is outside 0.5-20 m") };
    }
    Ok(Output::Image { png, path: Some(path), meta })
}

fn allowed_keys(a: &Args, allowed: &[&str]) -> Result<()> {
    // The one-shot CLI applies these before dispatch and keeps them in the argument map.
    if let Some(key) =
        a.keys().find(|key| !allowed.contains(&key.as_str()) && !["scene", "seed", "ticks"].contains(&key.as_str()))
    {
        bail!(
            "{}",
            Issue::error(key.as_str(), Code::UnknownKey, "unknown argument")
                .expected(allowed.join(", "))
                .maybe_fix(issue::suggestion(key, allowed.iter().copied()))
        );
    }
    Ok(())
}

fn one_of(key: &str, value: &str, valid: &[&str]) -> Result<()> {
    if !valid.contains(&value) {
        bail!(
            "{}",
            Issue::error(key, Code::UnknownValue, format!("unknown {key} \"{value}\""))
                .expected(valid.join(", "))
                .maybe_fix(issue::suggestion(value, valid.iter().copied()))
        );
    }
    Ok(())
}

fn position(a: &Args) -> Result<Option<Vec3>> {
    let Some(value) = a.get("pos") else { return Ok(None) };
    let value = if let Some(text) = value.as_str() { serde_json::from_str(text)? } else { value.clone() };
    let array: [f32; 3] =
        serde_json::from_value(value).map_err(|_| anyhow!("pos must be three numbers [x,y,z] in world metres"))?;
    let pos = Vec3::from_array(array);
    validate_pose(pos, Quat::IDENTITY).map_err(|e| anyhow!(e))?;
    Ok(Some(pos))
}

fn entity_id(a: &Args) -> Result<Option<EntityId>> {
    a.get("id")
        .map(|v| {
            let id = v
                .as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                .ok_or_else(|| anyhow!("id must be an integer entity ID"))?;
            Ok(EntityId(u32::try_from(id).map_err(|_| anyhow!("id is too large"))?))
        })
        .transpose()
}

fn room(a: &Args, s: &Session) -> Result<Option<Option<RegionKey>>> {
    let Some(value) = a.get("room") else { return Ok(None) };
    if value.is_null() {
        return Ok(Some(None));
    }
    let id = if let Some(id) = value.as_u64() {
        u16::try_from(id).map_err(|_| anyhow!("room ID is too large"))?
    } else if let Some(name) = value.as_str() {
        if name == "none" {
            return Ok(Some(None));
        }
        s.sim
            .state
            .world
            .room(name)
            .map(|r| r.id)
            .or_else(|| name.parse::<u16>().ok())
            .ok_or_else(|| anyhow!("no room '{name}'"))?
    } else {
        bail!("room must be a room key, integer ID, or null");
    };
    if s.sim.state.world.rooms.get(id as usize).is_none() {
        bail!("no room {id}");
    }
    Ok(Some(Some(RegionKey::Room(id))))
}

fn instance(s: &Session, entity: &Entity, active: bool) -> Value {
    let prop = entity.prop.as_ref().unwrap();
    let (yaw, pitch, roll) = entity.rot.to_euler(glam::EulerRot::YXZ);
    let room = match entity.region {
        Some(RegionKey::Room(id)) => s.sim.state.world.rooms.get(id as usize).map(|r| r.key.clone()),
        _ => None,
    };
    let mut out = json!({
        "id": entity.id.0, "name": entity.name, "asset": prop.asset,
        "revision": prop.definition.revision, "pos": entity.pos,
        "yaw": yaw.to_degrees(), "pitch": pitch.to_degrees(), "roll": roll.to_degrees(),
        "scale": prop.scale, "collide": prop.collide, "room": room, "active": active,
        "bounds": prop.bounds_at(entity.pos, entity.rot),
    });
    if prop.seed != 0 {
        out["seed"] = json!(prop.seed);
    }
    if !prop.roles.is_empty() {
        out["roles"] = json!(prop.roles);
    }
    out
}

/// `seed=`: which variant of an asset (chance, jitter and pick), 0 as authored.
fn seed_arg(a: &Args) -> Result<Option<u32>> {
    a.contains_key("seed")
        .then(|| {
            let seed = crate::tools::get_u64(a, "seed", 0)?;
            u32::try_from(seed).map_err(|_| anyhow!("seed must be from 0 to 4294967295"))
        })
        .transpose()
}

/// `roles=`: colours for the theme roles an asset's parts name, `{"$wall":"#5a6b7c"}`.
fn roles_arg(a: &Args) -> Result<Option<BTreeMap<String, String>>> {
    let Some(value) = a.get("roles") else { return Ok(None) };
    let value = match value {
        Value::String(text) => props::strict_value(text).map_err(|issue| anyhow!("roles: {}", issue.message))?,
        value => value.clone(),
    };
    let object = value.as_object().ok_or_else(|| anyhow!("roles must be an object such as {{\"$wall\":\"#5a6b7c\"}}"))?;
    let roles = object
        .iter()
        .map(|(role, color)| match color.as_str() {
            Some(color) => Ok((role.clone(), color.to_string())),
            None => Err(anyhow!("roles.{role} must be a #rrggbb colour")),
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    validate_roles(&roles).map_err(|e| anyhow!(e))?;
    Ok(Some(roles))
}

/// Explicit world placement. Adding a prop does not leave the current studio tab.
pub fn t_asset_spawn(s: &mut Session, a: &Args) -> Result<Output> {
    allowed_keys(a, &["action", "name", "id", "pos", "yaw", "scale", "collide", "room", "roles"])?;
    let action = opt_text(a, "action")?.unwrap_or("list");
    one_of("action", action, &["add", "list", "update", "remove"])?;
    let name = opt_text(a, "name")?.map(props::canonical).transpose().map_err(|e| anyhow!(e))?;
    let id = entity_id(a)?;
    let pos = position(a)?;
    let yaw = opt_float(a, "yaw")?;
    if yaw.is_some_and(|v| !v.is_finite() || v.abs() > 36_000.0) {
        bail!("yaw must be a finite angle from -36000 to 36000 degrees");
    }
    let scale = opt_float(a, "scale")?;
    if let Some(scale) = scale {
        validate_scale(scale).map_err(|e| anyhow!(e))?;
    }
    let collide = opt_bool(a, "collide")?;
    let region = room(a, s)?;
    let (seed, roles) = (seed_arg(a)?, roles_arg(a)?);
    let transforms =
        pos.is_some() || yaw.is_some() || scale.is_some() || collide.is_some() || region.is_some() || roles.is_some();
    match action {
        "list" => {
            if id.is_some() || transforms {
                bail!("list accepts only an optional asset name filter");
            }
            let active = s.sim.state.entities.iter().filter(|e| e.prop.is_some()).map(|e| (e, true));
            let dormant = s
                .sim
                .state
                .world
                .dormant_entities
                .values()
                .flatten()
                .map(|d| (&d.entity, false))
                .filter(|(e, _)| e.prop.is_some());
            let instances: Vec<_> = active
                .chain(dormant)
                .filter(|(e, _)| name.as_ref().is_none_or(|n| e.prop.as_ref().is_some_and(|p| &p.asset == n)))
                .map(|(e, active)| instance(s, e, active))
                .collect();
            Ok(Output::Json(json!({"instances": instances})))
        }
        "add" => {
            if id.is_some() {
                bail!("add allocates an entity ID; omit id");
            }
            let pos = pos.ok_or_else(|| anyhow!("add needs explicit world pos=[x,y,z]"))?;
            let name = name.ok_or_else(|| anyhow!("add needs name=BUILTIN/bench or a WORKSHOP prop"))?;
            let name = ensure_asset(&name)?;
            let region = region.unwrap_or_else(|| s.sim.state.world.current_room.map(RegionKey::Room));
            let definition = props::get(&name).ok_or_else(|| anyhow!("no prop '{name}'"))?;
            let copy = PropInstance::new(&name, definition, scale.unwrap_or(1.0), collide.unwrap_or(true))
                .and_then(|copy| copy.with_seed(seed.unwrap_or(0)).with_roles(roles.unwrap_or_default()))
                .map_err(|e| anyhow!(e))?;
            let rot = Quat::from_rotation_y(yaw.unwrap_or(0.0).to_radians());
            let id = s.sim.place_prop(copy, pos, rot, region).map_err(|e| anyhow!(e))?;
            Ok(Output::Json(json!({"success": true, "instance": instance(s, s.sim.state.entities.get(id).unwrap(), true)})))
        }
        "update" => {
            if name.is_some() {
                bail!("update keeps the embedded asset; use asset_edit to change its named parts");
            }
            let id = id.ok_or_else(|| anyhow!("update needs a prop root id"))?;
            let entity = s
                .sim
                .state
                .entities
                .get(id)
                .ok_or_else(|| anyhow!("no active entity {}; wake its room before updating a sleeping prop", id.0))?;
            let prop = entity.prop.as_ref().ok_or_else(|| anyhow!("entity {} is not a prop instance", id.0))?;
            let rot = if let Some(yaw) = yaw {
                let (_, pitch, roll) = entity.rot.to_euler(glam::EulerRot::YXZ);
                Quat::from_euler(glam::EulerRot::YXZ, yaw.to_radians(), pitch, roll)
            } else {
                entity.rot
            };
            s.sim
                .update_prop_instance(
                    id,
                    pos.unwrap_or(entity.pos),
                    rot,
                    scale.unwrap_or(prop.scale),
                    collide.unwrap_or(prop.collide),
                )
                .map_err(|e| anyhow!(e))?;
            if seed.is_some() || roles.is_some() {
                s.sim.restyle_prop(id, seed, roles).map_err(|e| anyhow!(e))?;
            }
            if let Some(region) = region {
                s.sim.state.entities.get_mut(id).unwrap().region = region;
            }
            Ok(Output::Json(json!({"success": true, "instance": instance(s, s.sim.state.entities.get(id).unwrap(), true)})))
        }
        "remove" => {
            if name.is_some() || transforms {
                bail!("remove accepts only a prop root id");
            }
            let id = id.ok_or_else(|| anyhow!("remove needs a prop root id"))?;
            if let Some(entity) = s.sim.state.entities.get(id) {
                if entity.prop.is_none() {
                    bail!("entity {} is not a prop instance", id.0);
                }
                s.sim.despawn(id);
            } else {
                let list = s
                    .sim
                    .state
                    .world
                    .dormant_entities
                    .values_mut()
                    .find(|list| list.iter().any(|d| d.entity.id == id && d.entity.prop.is_some()))
                    .ok_or_else(|| anyhow!("no prop instance {}", id.0))?;
                list.retain(|d| d.entity.id != id);
                s.sim.invalidate_prop_navigation();
            }
            Ok(Output::Json(json!({"removed": true, "id": id.0})))
        }
        _ => unreachable!(),
    }
}
