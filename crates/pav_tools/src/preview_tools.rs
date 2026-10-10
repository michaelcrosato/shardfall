//! Live animation controls. The same handler serves CLI, MCP and the native studio panel.

use anyhow::{Result, anyhow, bail};
use glam::Vec3;
use pav_core::animation_preview::{PreviewChange, Selection};
use pav_core::prop_preview::{StudioCamera, StudioMode};
use serde_json::{Value, json};

use crate::session::Session;
use crate::tools::{Args, Output, get_bool, get_f32, get_str};

pub(crate) fn opt_float(a: &Args, key: &str) -> Result<Option<f32>> {
    a.contains_key(key).then(|| get_f32(a, key, 0.0)).transpose()
}

pub(crate) fn opt_bool(a: &Args, key: &str) -> Result<Option<bool>> {
    a.contains_key(key).then(|| get_bool(a, key, false)).transpose()
}

pub(crate) fn opt_text<'a>(a: &'a Args, key: &str) -> Result<Option<&'a str>> {
    a.get(key).map(|v| v.as_str().ok_or_else(|| anyhow!("{key} must be text"))).transpose()
}

pub(crate) fn step_arg(a: &Args) -> Result<Option<i32>> {
    a.get("step")
        .map(|v| {
            let n = v
                .as_i64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                .ok_or_else(|| anyhow!("step must be a signed integer number of frames"))?;
            i32::try_from(n).map_err(|_| anyhow!("step is too large"))
        })
        .transpose()
}

fn status(s: &Session) -> Value {
    match s.sim.state.animation_preview.as_ref().map(|p| p.info()) {
        Some(info) => {
            let mut out = serde_json::to_value(info).unwrap_or_default();
            out["open"] = json!(s.sim.studio_mode() == StudioMode::Animation);
            out["active"] = out["open"].clone();
            out["studio_mode"] = json!(s.sim.studio_mode());
            out["scene_preserved"] = json!(s.sim.state.scene);
            out
        }
        None => json!({
            "open": false,
            "active": false,
            "studio_mode": s.sim.studio_mode(),
            "use": "clip=SET/Clip or move=NAME opens the stage; playing/time/speed/repeat control it; action=pose reads joints; close=true returns to the scene",
        }),
    }
}

/// Keep every workspace camera in snapshot state. Call after the core changes mode; the
/// caller supplies its previous mode and restores the checkpoint if fitting/decoding fails.
pub(crate) fn switch_camera(s: &mut Session, from: StudioMode) -> Result<bool> {
    let to = s.sim.studio_mode();
    if from == to {
        return Ok(false);
    }
    let old = StudioCamera { params: serde_json::to_value(&s.camera.params)?, focus: s.camera.target };
    let target = s.sim.state.studio_cameras.get(to).cloned().or_else(|| {
        // Animation snapshots made before shared workspaces kept just the world camera.
        (to == StudioMode::World)
            .then(|| {
                s.sim
                    .state
                    .animation_preview
                    .as_ref()?
                    .saved_camera
                    .clone()
                    .map(|params| StudioCamera { params, focus: s.sim.state.focus })
            })
            .flatten()
    });
    let decoded = target
        .as_ref()
        .map(|c| serde_json::from_value(c.params.clone()))
        .transpose()
        .map_err(|e| anyhow!("saved studio camera: {e}"))?;
    s.sim.state.studio_cameras.set(from, old);
    if let (Some(params), Some(target)) = (decoded, target) {
        s.camera.params = params;
        s.camera.snap(target.focus);
    } else {
        let focus = match to {
            StudioMode::Animation => s.sim.state.animation_preview.as_ref().map(|p| p.focus),
            StudioMode::Prop => s.sim.state.prop_preview.as_ref().map(|p| p.focus),
            StudioMode::Creature => Some(s.sim.state.creature_preview.as_ref().map_or(Vec3::Y, |p| p.focus)),
            StudioMode::World => Some(s.sim.state.focus),
        };
        if let Some(focus) = focus {
            s.camera.snap(focus);
        }
    }
    s.clear_view_effects();
    Ok(s.sim.state.studio_cameras.get(to).is_some())
}

/// Preview mutations never need to clone or roll back the frozen game/physics world.
pub(crate) struct StudioCheckpoint {
    animation: Option<pav_core::animation_preview::PreviewState>,
    prop: Option<pav_core::prop_preview::PropPreviewState>,
    creature: Option<pav_core::creature_preview::CreaturePreviewState>,
    mode: Option<StudioMode>,
    cameras: pav_core::prop_preview::StudioCameras,
    camera: pav_view::CameraRig,
}

impl StudioCheckpoint {
    pub(crate) fn new(s: &Session) -> Self {
        Self {
            animation: s.sim.state.animation_preview.clone(),
            prop: s.sim.state.prop_preview.clone(),
            creature: s.sim.state.creature_preview.clone(),
            mode: s.sim.state.studio_mode,
            cameras: s.sim.state.studio_cameras.clone(),
            camera: s.camera.clone(),
        }
    }
    pub(crate) fn restore(self, s: &mut Session) {
        s.sim.state.animation_preview = self.animation;
        s.sim.state.prop_preview = self.prop;
        s.sim.state.creature_preview = self.creature;
        s.sim.state.studio_mode = self.mode;
        s.sim.state.studio_cameras = self.cameras;
        s.camera = self.camera;
    }
}

/// Fits the full motion once. Subsequent edits to the same clip keep the user's camera.
/// `anim_preview action=fit` repeats this calculation after a large change in reach or travel.
pub fn fit_camera(s: &mut Session) -> Result<()> {
    let p = s.sim.state.animation_preview.as_ref().ok_or_else(|| anyhow!("no preview is open"))?;
    let info = p.info();
    s.camera.params = pav_view::CameraParams {
        tilt: 22.0,
        yaw: 60.0,
        fov: 38.0,
        height_offset: 0.0,
        follow_lag: 0.0,
        distance: 5.0,
        ortho: false,
    };
    let (forward, up, right) = (s.camera.forward(), s.camera.up(), s.camera.ground_axes().1);
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut times = info.key_times;
    times.extend((0..=48).map(|i| info.duration * i as f32 / 48.0));
    for time in times {
        let parts = pav_core::puppet::pose(&p.puppet, &p.state_at(time), None, Vec3::ZERO, forward);
        let (a, b) = crate::agent_tools::pose_bounds(&parts);
        lo = lo.min(a);
        hi = hi.max(b);
    }
    if !lo.is_finite() || !hi.is_finite() {
        bail!("the preview has no finite body bounds");
    }
    let centre = (lo + hi) * 0.5;
    let reach = (s.camera.params.fov.to_radians() * 0.5).tan() / 1.25;
    let mut distance = 3.0_f32;
    for corner in 0..8 {
        let q = Vec3::new(
            if corner & 1 == 0 { lo.x } else { hi.x },
            if corner & 2 == 0 { lo.y } else { hi.y },
            if corner & 4 == 0 { lo.z } else { hi.z },
        ) - centre;
        distance = distance.max(q.dot(up).abs().max(q.dot(right).abs()) / reach - q.dot(forward));
    }
    s.camera.params.distance = distance;
    s.camera.snap(centre);
    if let Some(p) = &mut s.sim.state.animation_preview {
        p.focus = centre;
    }
    Ok(())
}

/// `anim_preview`: inspect, open or control a clip or procedural move on the engine's stage.
pub fn t_anim_preview(s: &mut Session, a: &Args) -> Result<Output> {
    let action = get_str(a, "action").unwrap_or("status");
    if !matches!(action, "status" | "open" | "play" | "pause" | "restart" | "pose" | "close" | "fit") {
        bail!("action must be status, open, play, pause, restart, pose, close or fit");
    }
    let clip = opt_text(a, "clip")?;
    let move_name = opt_text(a, "move")?;
    let close = get_bool(a, "close", false)? || action == "close";
    // Parse before opening the stage, so a bad command never leaves a partial change.
    let mut change = PreviewChange {
        playing: opt_bool(a, "playing")?,
        time: opt_float(a, "time")?,
        speed: opt_float(a, "speed")?,
        repeat: opt_bool(a, "repeat")?,
        mirror: opt_bool(a, "mirror")?,
        upper: opt_bool(a, "upper")?,
        travel: opt_bool(a, "travel")?,
        side: opt_float(a, "side")?,
        hit: opt_float(a, "hit")?,
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
    if clip.is_some() && move_name.is_some() {
        bail!("choose clip or move, not both");
    }
    let controls = a.keys().any(|k| {
        matches!(k.as_str(), "playing" | "time" | "speed" | "repeat" | "mirror" | "upper" | "travel" | "side" | "hit" | "step")
    });
    let open = clip.is_some() || move_name.is_some() || action == "open" || get_bool(a, "stage", false)?;
    if close {
        if open || controls {
            bail!("close cannot select or alter an animation");
        }
        let checkpoint = StudioCheckpoint::new(s);
        let before = s.sim.studio_mode();
        s.sim.preview_close();
        if let Err(error) = switch_camera(s, before) {
            checkpoint.restore(s);
            return Err(error);
        }
        s.prev_frame = s.sim.frame();
        return Ok(Output::Json(status(s)));
    }
    if !open && s.sim.state.animation_preview.is_none() {
        if controls || action != "status" {
            bail!("no preview is open (anim_preview clip=SET/Clip)");
        }
        return Ok(Output::Json(status(s)));
    }

    let activate = open || controls || matches!(action, "play" | "pause" | "restart" | "fit");
    if !activate && action == "status" {
        return Ok(Output::Json(status(s)));
    }
    let checkpoint = StudioCheckpoint::new(s);
    let previous = s.sim.state.animation_preview.as_ref().map(|p| p.selection.clone());
    let mode_before = s.sim.studio_mode();
    let result = (|| -> Result<()> {
        if activate {
            if let Some(name) = clip {
                let authored = name
                    .split_once('/')
                    .is_some_and(|(set, _)| set.eq_ignore_ascii_case("WORKSHOP") || set.eq_ignore_ascii_case("WORKSHOP_LOCAL"));
                if authored && pav_core::clips::library().find(name).is_none() {
                    crate::animation_tools::reload_authored()?;
                }
            }
            s.sim.preview_open(clip, move_name).map_err(|e| anyhow!(e))?;
        }
        if activate {
            s.sim.preview_change(&change).map_err(|e| anyhow!(e))?;
            switch_camera(s, mode_before)?;
            let selected = s.sim.state.animation_preview.as_ref().map(|p| &p.selection);
            if previous.as_ref() != selected || action == "fit" {
                fit_camera(s)?;
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        checkpoint.restore(s);
        return Err(e);
    }
    if activate {
        s.prev_frame = s.sim.frame();
    }
    let mut out = status(s);
    if action == "pose" {
        let p = s.sim.state.animation_preview.as_ref().ok_or_else(|| anyhow!("no preview is open"))?;
        if let Selection::Clip { id, .. } = &p.selection {
            let key = pav_core::clips::with(*id, |c| c.key_at_clamped(p.info().time))
                .map(|k| if p.mirror { pav_core::clips::mirror(&k) } else { k });
            out["key"] = serde_json::to_value(key)?;
            out["legend"] = json!(pav_core::clips::LEGEND);
        }
        if let Some(sk) = p.skeleton() {
            out["joint_space"] = json!(
                "metres before camera and clothing adjustments; x right, y up, z forward; left and right are the subject's sides"
            );
            out["joints"] = json!({
                "pelvis": sk.pelvis, "chest": sk.chest, "neck": sk.neck, "head": sk.head,
                "shoulderL": sk.shoulder[0], "elbowL": sk.elbow[0], "handL": sk.hand[0],
                "shoulderR": sk.shoulder[1], "elbowR": sk.elbow[1], "handR": sk.hand[1],
                "hipL": sk.hip[0], "kneeL": sk.knee[0], "ankleL": sk.ankle[0],
                "hipR": sk.hip[1], "kneeR": sk.knee[1], "ankleR": sk.ankle[1],
            });
        }
        let parts = pav_core::puppet::pose(&p.puppet, &p.state_at(p.info().time), None, Vec3::ZERO, s.camera.forward());
        let (lo, hi) = crate::agent_tools::pose_bounds(&parts);
        out["bounds_metres"] = json!({ "min": lo, "max": hi });
    }
    Ok(Output::Json(out))
}
