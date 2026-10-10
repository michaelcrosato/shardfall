//! Prop inspection and placement through the same session used by the native studio.

use anyhow::{Result, anyhow, bail};
use glam::{Quat, Vec3};
use pav_core::prop_instance::{transform_bounds, validate_pose, validate_scale};
use pav_core::prop_preview::{PropPreviewChange, StudioMode};
use pav_core::props;
use pav_core::statics::RegionKey;
use pav_core::{Entity, EntityId};
use serde_json::{Value, json};

use crate::preview_tools::{StudioCheckpoint, opt_bool, opt_float, opt_text, step_arg, switch_camera};
use crate::session::Session;
use crate::tools::{Args, Output};

fn status(s: &Session) -> Value {
    let mut out = s.sim.state.prop_preview.as_ref().map(|p| serde_json::to_value(p.info()).unwrap()).unwrap_or_else(|| json!({}));
    let active = s.sim.studio_mode() == StudioMode::Prop;
    out["open"] = json!(active);
    out["active"] = json!(active);
    out["studio_mode"] = json!(s.sim.studio_mode());
    out["scene_preserved"] = json!(s.sim.state.scene);
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
    allowed_keys(a, &["action", "name", "close", "playing", "time", "speed", "turntable", "yaw", "scale", "step"])?;
    let action = opt_text(a, "action")?.unwrap_or("status");
    if !matches!(action, "status" | "open" | "fit" | "close" | "play" | "pause" | "restart") {
        bail!("action must be status, open, fit, close, play, pause or restart");
    }
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
    let controls = a.keys().any(|k| matches!(k.as_str(), "time" | "playing" | "speed" | "turntable" | "yaw" | "scale" | "step"));
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

fn allowed_keys(a: &Args, allowed: &[&str]) -> Result<()> {
    // The one-shot CLI applies these before dispatch and keeps them in the argument map.
    if let Some(key) =
        a.keys().find(|key| !allowed.contains(&key.as_str()) && !["scene", "seed", "ticks"].contains(&key.as_str()))
    {
        bail!("unknown argument '{key}'; accepted arguments: {}", allowed.join(", "));
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
    json!({
        "id": entity.id.0, "name": entity.name, "asset": prop.asset,
        "revision": prop.definition.revision(), "pos": entity.pos,
        "yaw": yaw.to_degrees(), "pitch": pitch.to_degrees(), "roll": roll.to_degrees(),
        "scale": prop.scale, "collide": prop.collide, "room": room, "active": active,
        "bounds": prop.bounds_at(entity.pos, entity.rot),
    })
}

/// Explicit world placement. Adding a prop does not leave the current studio tab.
pub fn t_asset_spawn(s: &mut Session, a: &Args) -> Result<Output> {
    allowed_keys(a, &["action", "name", "id", "pos", "yaw", "scale", "collide", "room"])?;
    let action = opt_text(a, "action")?.unwrap_or("list");
    if !matches!(action, "add" | "list" | "update" | "remove") {
        bail!("action must be add, list, update or remove");
    }
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
    let transforms = pos.is_some() || yaw.is_some() || scale.is_some() || collide.is_some() || region.is_some();
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
            let id = s
                .sim
                .spawn_prop(
                    &name,
                    pos,
                    Quat::from_rotation_y(yaw.unwrap_or(0.0).to_radians()),
                    scale.unwrap_or(1.0),
                    collide.unwrap_or(true),
                    region,
                )
                .map_err(|e| anyhow!(e))?;
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
