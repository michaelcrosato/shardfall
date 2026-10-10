//! Creature playback shares the live camera, frame clock and renderer with the studio UI.

use anyhow::{Result, anyhow, bail};
use glam::{Quat, Vec3};
use pav_core::creature_preview::CreaturePreviewChange;
use pav_core::creatures;
use pav_core::prop_instance::transform_bounds;
use pav_core::prop_preview::StudioMode;
use serde_json::{Value, json};

use crate::Session;
use crate::preview_tools::{StudioCheckpoint, opt_bool, opt_float, opt_text, step_arg, switch_camera};
use crate::tools::{Args, Output};

fn status(s: &Session) -> Value {
    let mut out = s
        .sim
        .state
        .creature_preview
        .as_ref()
        .map(|p| serde_json::to_value(p.info()).unwrap())
        .unwrap_or_else(|| json!({"available":false}));
    out["open"] = json!(s.sim.studio_mode() == StudioMode::Creature);
    out["active"] = out["open"].clone();
    out["studio_mode"] = json!(s.sim.studio_mode());
    out["scene_preserved"] = json!(s.sim.state.scene);
    out
}

/// Fit the model's rest bounds once. Rebuilding the same selection keeps the human camera;
/// an explicit fit also accounts for every turntable angle.
pub fn fit_camera(s: &mut Session) -> Result<()> {
    let Some(preview) = s.sim.state.creature_preview.as_ref() else {
        s.camera = pav_view::CameraRig::default();
        s.camera.snap(Vec3::Y);
        return Ok(());
    };
    let local = preview.bounds();
    let lift = -Vec3::Y * local.min.y;
    let mut bounds = transform_bounds(local, lift, preview.rotation());
    if preview.turntable {
        for i in 0..48 {
            bounds = bounds.union(transform_bounds(local, lift, Quat::from_rotation_y(i as f32 * std::f32::consts::TAU / 48.0)));
        }
    }
    s.camera = pav_view::CameraRig::new(pav_view::CameraParams {
        tilt: 22.0,
        yaw: 45.0,
        fov: 38.0,
        height_offset: 0.0,
        follow_lag: 0.0,
        distance: 5.0,
        ortho: false,
    });
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
    s.sim.state.creature_preview.as_mut().unwrap().focus = center;
    Ok(())
}

pub fn t_creature_preview(s: &mut Session, a: &Args) -> Result<Output> {
    let allowed = [
        "action",
        "name",
        "close",
        "clip",
        "playing",
        "time",
        "speed",
        "looping",
        "turntable",
        "yaw",
        "scale",
        "step",
        "scene",
        "seed",
        "ticks",
    ];
    if let Some(key) = a.keys().find(|key| !allowed.contains(&key.as_str())) {
        bail!("unknown creature_preview argument '{key}'")
    }
    let action = opt_text(a, "action")?.unwrap_or("status");
    if !matches!(action, "status" | "open" | "fit" | "close" | "play" | "pause" | "restart" | "pose") {
        bail!("action must be status, open, fit, close, play, pause, restart, or pose")
    }
    let name = opt_text(a, "name")?.map(creatures::canonical).transpose().map_err(|e| anyhow!(e))?;
    let close = opt_bool(a, "close")?.unwrap_or(false) || action == "close";
    let mut change = CreaturePreviewChange {
        clip: opt_text(a, "clip")?.map(str::to_string),
        time: opt_float(a, "time")?,
        playing: opt_bool(a, "playing")?,
        speed: opt_float(a, "speed")?,
        looping: opt_bool(a, "looping")?,
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
                change.playing = Some(true)
            }
        }
        _ => {}
    }
    change.validate().map_err(|e| anyhow!(e))?;
    let controls = a.keys().any(|key| {
        matches!(key.as_str(), "clip" | "playing" | "time" | "speed" | "looping" | "turntable" | "yaw" | "scale" | "step")
    });
    if close && (name.is_some() || controls || !matches!(action, "close" | "status")) {
        bail!("close cannot select or alter a creature")
    }
    let activate = name.is_some() || controls || matches!(action, "open" | "fit" | "play" | "pause" | "restart");
    if close || activate {
        let checkpoint = StudioCheckpoint::new(s);
        let before_mode = s.sim.studio_mode();
        let before_name = s.sim.state.creature_preview.as_ref().map(|p| p.name.clone());
        let result = (|| -> Result<()> {
            if close {
                s.sim.creature_preview_close();
                switch_camera(s, before_mode)?;
                return Ok(());
            }
            if name.is_none() && before_name.is_none() && creatures::library().assets.is_empty() {
                if controls || matches!(action, "play" | "pause" | "restart") {
                    bail!("create a creature before changing playback")
                }
                s.sim.set_studio_mode(StudioMode::Creature);
                let restored = switch_camera(s, before_mode)?;
                if !restored || action == "fit" {
                    fit_camera(s)?;
                }
                return Ok(());
            }
            s.sim.creature_preview_open(name.as_deref()).map_err(|e| anyhow!(e))?;
            s.sim.creature_preview_change(&change).map_err(|e| anyhow!(e))?;
            switch_camera(s, before_mode)?;
            let selected = &s.sim.state.creature_preview.as_ref().unwrap().name;
            if before_name.as_ref() != Some(selected) || action == "fit" {
                fit_camera(s)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            checkpoint.restore(s);
            return Err(error);
        }
        s.prev_frame = s.sim.frame();
    }
    let mut out = status(s);
    if action == "pose" {
        let p = s.sim.state.creature_preview.as_ref().ok_or_else(|| anyhow!("no creature is selected"))?;
        let pose = p.definition.sample_pose(p.clip.as_deref(), p.time, p.looping).map_err(|e| anyhow!(e))?;
        let rotation = p.rotation();
        let lift = -Vec3::Y * p.bounds().min.y;
        out["joints"] = json!(
            p.definition
                .bones
                .names
                .iter()
                .enumerate()
                .map(|(i, name)| json!({
                    "name":name,"parent":p.definition.bones.parents[i],"position":lift+rotation*(pose.positions[i]*p.scale),
                    "rotation":rotation*pose.rotations[i],
                }))
                .collect::<Vec<_>>()
        );
        out["coordinates"] = json!("stage metres; quaternions are xyzw");
    }
    Ok(Output::Json(out))
}
