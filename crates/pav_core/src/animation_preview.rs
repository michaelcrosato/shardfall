//! A shared animation stage for agents and the native application.
//!
//! The stage has its own clock. While it is open, the world does not step, and its entities,
//! physics, history and recording stay intact. The stage publishes ordinary `RenderFrame`s,
//! so the game window and headless captures use the same puppet and renderer.

use std::sync::Arc;

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::clips;
use crate::frame::{HudFrame, PuppetFrame, RenderFrame, RenderObject};
use crate::prop_preview::{STAGE_ID_BASE, StudioMode, stage_objects};
use crate::puppet::{BodyPlan, PuppetDef, PuppetState, Skel};
use crate::shape::{Shape, Visual};
use crate::{Color, EntityId, Sim, SimConfig};

/// A clip keeps its stable name hash. Moves resolve their names again after a data reload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Selection {
    Clip { id: u32, name: String },
    Move { name: String },
}

/// Everything that changes in the stage. It is part of `SimState` and survives snapshots and
/// the live bridge's short-lived tool sessions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewState {
    pub selection: Selection,
    pub time: f32,
    pub playing: bool,
    pub speed: f32,
    pub repeat: bool,
    pub mirror: bool,
    pub upper: bool,
    pub travel: bool,
    pub side: f32,
    pub hit: Option<f32>,
    pub puppet: Arc<PuppetDef>,
    /// The centre of the whole animation's bounds; a camera can orbit this fixed point.
    pub focus: Vec3,
    /// Camera parameters saved by the tool client. The simulation does not interpret them.
    #[serde(default)]
    pub saved_camera: Option<serde_json::Value>,
    ticks: u64,
}

/// The same status is sent to agents and to the application's animation panel.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewInfo {
    pub name: String,
    pub kind: String,
    pub clip: Option<String>,
    #[serde(rename = "move")]
    pub move_name: Option<String>,
    pub time: f32,
    pub duration: f32,
    pub playing: bool,
    pub speed: f32,
    pub repeat: bool,
    pub mirror: bool,
    pub upper: bool,
    pub travel: bool,
    pub side: f32,
    pub hit: Option<f32>,
    pub fps: f32,
    pub key_times: Vec<f32>,
    pub available: bool,
}

/// A partial control update. Time is in seconds; `step` is a signed number of source frames.
/// Scrubbing or stepping pauses playback unless `playing` is also supplied.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewChange {
    pub playing: Option<bool>,
    pub time: Option<f32>,
    pub speed: Option<f32>,
    pub repeat: Option<bool>,
    pub mirror: Option<bool>,
    pub upper: Option<bool>,
    pub travel: Option<bool>,
    pub side: Option<f32>,
    pub hit: Option<f32>,
    pub step: Option<i32>,
}

impl PreviewChange {
    /// Check options before loading assets or changing a running preview.
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [("time", self.time), ("speed", self.speed), ("side", self.side), ("hit", self.hit)] {
            if value.is_some_and(|v| !v.is_finite()) {
                return Err(format!("{name} must be a finite number"));
            }
        }
        if self.speed.is_some_and(|v| !(0.05..=8.0).contains(&v)) {
            return Err("speed must be between 0.05 and 8".into());
        }
        if self.time.is_some_and(|v| v < 0.0) {
            return Err("time must be zero or greater".into());
        }
        if self.hit.is_some_and(|v| !(0.0..=1.0).contains(&v)) {
            return Err("hit must be between 0 and 1".into());
        }
        Ok(())
    }
}

impl PreviewState {
    fn new(selection: Selection, puppet: PuppetDef) -> Self {
        let focus = Vec3::Y * (clips::hip_height(&puppet) + puppet.torso_length * puppet.scale * 0.25);
        Self {
            selection,
            time: 0.0,
            playing: true,
            speed: 1.0,
            repeat: true,
            mirror: false,
            upper: false,
            travel: false,
            side: 1.0,
            hit: None,
            puppet: Arc::new(puppet),
            focus,
            saved_camera: None,
            ticks: 0,
        }
    }

    /// Read the current asset on every call. An accepted edit is visible even while paused.
    pub fn info(&self) -> PreviewInfo {
        let (name, kind, clip, move_name, duration, fps, key_times, available) = match &self.selection {
            Selection::Clip { id, name } => {
                let lib = clips::library();
                let c = lib.get(*id);
                let fps = name
                    .split_once('/')
                    .and_then(|(set, _)| lib.sets.iter().find(|s| s.set == set))
                    .map(|s| s.fps)
                    .filter(|f| f.is_finite() && *f > 0.0)
                    .unwrap_or(30.0);
                (
                    name.clone(),
                    "clip".to_string(),
                    Some(name.clone()),
                    None,
                    c.map(|c| c.dur).unwrap_or(0.0),
                    fps,
                    c.map(|c| c.keys.iter().map(|k| k.t).collect()).unwrap_or_default(),
                    c.is_some(),
                )
            }
            Selection::Move { name } => {
                let table = crate::moves::table();
                let m = table.id(name).and_then(|id| crate::moves::played(&table, id, self.move_side()));
                let duration = m.map(|m| m.wind + m.active + m.recover).unwrap_or(0.0);
                let keys = m.map(|m| vec![0.0, m.wind, m.wind + m.active, duration]).unwrap_or_default();
                (name.clone(), "move".into(), None, Some(name.clone()), duration, 60.0, keys, m.is_some())
            }
        };
        let duration = if duration.is_finite() { duration.max(0.0) } else { 0.0 };
        PreviewInfo {
            name,
            kind,
            clip,
            move_name,
            time: self.time.clamp(0.0, duration),
            duration,
            playing: self.playing && available,
            speed: self.speed,
            repeat: self.repeat,
            mirror: self.mirror,
            upper: self.upper,
            travel: self.travel,
            side: self.side,
            hit: self.hit,
            fps,
            key_times,
            available,
        }
    }

    fn move_side(&self) -> f32 {
        if self.mirror { -self.side } else { self.side }
    }

    /// Advances only the stage. Paused previews still publish after an asset edit or a scrub.
    pub(crate) fn advance(&mut self, dt: f32) {
        self.ticks = self.ticks.wrapping_add(1);
        let info = self.info();
        if !self.playing || !info.available || info.duration <= 0.0 {
            return;
        }
        let next = self.time + dt * self.speed;
        if self.repeat {
            self.time = next.rem_euclid(info.duration);
        } else {
            self.time = next.min(info.duration);
            if next >= info.duration {
                self.playing = false;
            }
        }
    }

    pub fn change(&mut self, c: &PreviewChange) -> Result<(), String> {
        c.validate()?;
        if let Some(v) = c.speed {
            self.speed = v;
        }
        if let Some(v) = c.repeat {
            self.repeat = v;
        }
        if let Some(v) = c.mirror {
            self.mirror = v;
        }
        if let Some(v) = c.upper {
            self.upper = v;
        }
        if let Some(v) = c.travel {
            self.travel = v;
        }
        if let Some(v) = c.side {
            self.side = if v < 0.0 { -1.0 } else { 1.0 };
        }
        if let Some(v) = c.hit {
            self.hit = Some(v);
        }
        let info = self.info();
        if let Some(t) = c.time {
            self.time = t.min(info.duration);
            self.playing = false;
        }
        if let Some(n) = c.step {
            self.time = (self.time + n as f32 / info.fps).clamp(0.0, info.duration);
            self.playing = false;
        }
        if let Some(v) = c.playing {
            // Play from a held endpoint starts again. An explicit time selects that moment.
            if v && !self.playing && self.time >= info.duration && c.time.is_none() && c.step.is_none() {
                self.time = 0.0;
            }
            self.playing = v;
        }
        Ok(())
    }

    /// An ordinary engine animation state for a chosen moment. No special renderer is used.
    pub fn state_at(&self, time: f32) -> PuppetState {
        let mut st = PuppetState { time, ..Default::default() };
        match &self.selection {
            Selection::Clip { id, .. } => {
                st.clip = *id;
                st.clip_t = time;
                st.clip_w = 1.0;
                // The stage owns looping, so scrubbing reaches a loop's last key as well.
                st.clip_flags = clips::CLAMP
                    | if self.mirror { clips::MIRROR } else { 0 }
                    | if self.upper { clips::UPPER } else { 0 }
                    | if self.travel { clips::TRAVEL } else { 0 };
                st.clip_speed = self.speed;
            }
            Selection::Move { name } => {
                let table = crate::moves::table();
                if let Some(id) = table.id(name) {
                    if let Some(m) = crate::moves::played(&table, id, self.move_side()) {
                        let dur = (m.wind + m.active + m.recover).max(1e-6);
                        let hit = self.hit.unwrap_or((m.wind + m.hit * m.active) / dur);
                        st.set_action(crate::moves::MoveId(id), (time / dur).clamp(0.0, 1.0), hit, self.move_side());
                    }
                }
            }
        }
        st
    }

    /// Joints before camera and clothing adjustments, in metres: x right, y up, z forward.
    pub fn skeleton(&self) -> Option<Skel> {
        if self.puppet.body != BodyPlan::Biped || !self.info().available {
            return None;
        }
        let st = self.state_at(self.info().time);
        let base = crate::puppet::procedural(&self.puppet, &st, &crate::moves::table());
        Some(clips::over(&self.puppet, &st, base))
    }

    pub(crate) fn frame(&self, config: Arc<SimConfig>, dt: f32, ticket: u64) -> RenderFrame {
        let info = self.info();
        let mut objects = stage_objects();
        let subject = EntityId(STAGE_ID_BASE);
        objects.push(RenderObject {
            id: subject,
            pos: Vec3::ZERO,
            rot: Quat::IDENTITY,
            visual: Visual::new(Shape::Sphere { radius: 0.0 }, Color::WHITE),
            prop: None,
            puppet: Some(PuppetFrame {
                state: self.state_at(info.time),
                feet_offset: 0.0,
                def: Some(self.puppet.clone()),
                rig: None,
                tint: None,
            }),
            pulse: -1.0,
            soft: None,
            vehicle: None,
            cone: None,
            scenery: false,
        });
        objects.sort_by_key(|o| o.id);
        RenderFrame {
            tick: self.ticks,
            time: info.time as f64,
            dt,
            live_edit_ticket: ticket,
            objects,
            statics: Default::default(),
            focus: self.focus,
            focus_is_player: false,
            player: Some(subject),
            puppet_def: (*self.puppet).clone(),
            room: None,
            config,
            events: Vec::new(),
            projectiles: Vec::new(),
            hud: HudFrame::default(),
            game: None,
            animation_preview: Some(info),
            studio_mode: StudioMode::Animation,
            prop_preview: None,
        }
    }
}

impl Sim {
    /// Opens a stage without replacing the current scene. Selecting the same asset preserves
    /// the clock and controls, which is essential while an agent makes several small edits.
    pub fn preview_open(&mut self, clip: Option<&str>, move_name: Option<&str>) -> Result<PreviewInfo, String> {
        if clip.is_some() && move_name.is_some() {
            return Err("choose clip or move, not both".into());
        }
        let selection = if let Some(name) = move_name {
            let id = crate::moves::MoveId::named(name)
                .filter(|id| id.index() != 0)
                .ok_or_else(|| format!("unknown move '{name}' (anim/moves.toml lists the moves)"))?;
            Selection::Move { name: id.name() }
        } else {
            let lib = clips::library();
            let id = match clip {
                Some(name) => lib.find(name).ok_or_else(|| format!("no clip '{name}' (clips find=WORDS searches)"))?,
                None => {
                    if let Some(p) = &self.state.animation_preview {
                        let info = p.info();
                        self.set_studio_mode(StudioMode::Animation);
                        return Ok(info);
                    }
                    let first = lib.sets.iter().flat_map(|s| s.clips.keys().map(move |n| clips::clip_id(&s.set, n))).next();
                    lib.find("QUATERNIUS/Idle_Loop").or(first).ok_or("the animation library has no clips")?
                }
            };
            Selection::Clip { id, name: lib.name_of(id).ok_or("the clip has no name")? }
        };
        if let Some(p) = &self.state.animation_preview {
            if p.selection == selection {
                let info = p.info();
                self.set_studio_mode(StudioMode::Animation);
                return Ok(info);
            }
        }
        let puppet = self
            .state
            .animation_preview
            .as_ref()
            .map(|p| (*p.puppet).clone())
            .or_else(|| self.player().and_then(|p| p.character.as_ref()?.puppet.as_ref()).map(|p| (**p).clone()))
            .unwrap_or_else(|| self.config.puppet.clone());
        if matches!(selection, Selection::Clip { .. }) && puppet.body != BodyPlan::Biped {
            return Err("motion clips require a biped puppet".into());
        }
        let mut p = PreviewState::new(selection, puppet);
        if let Some(old) = &self.state.animation_preview {
            p.saved_camera = old.saved_camera.clone();
            p.speed = old.speed;
            p.repeat = old.repeat;
            p.mirror = old.mirror;
            p.upper = old.upper;
            p.travel = old.travel;
        }
        let info = p.info();
        self.state.animation_preview = Some(p);
        self.set_studio_mode(StudioMode::Animation);
        Ok(info)
    }

    /// Applies a validated control update atomically.
    pub fn preview_change(&mut self, change: &PreviewChange) -> Result<PreviewInfo, String> {
        let p = self.state.animation_preview.as_mut().ok_or("no preview is open (anim_preview clip=SET/Clip)")?;
        p.change(change)?;
        Ok(p.info())
    }

    pub fn preview_info(&self) -> Option<PreviewInfo> {
        (self.studio_mode() == StudioMode::Animation)
            .then(|| self.state.animation_preview.as_ref().map(PreviewState::info))
            .flatten()
    }

    /// The world resumes at the exact point where the preview was opened.
    pub fn preview_close(&mut self) {
        if self.studio_mode() == StudioMode::Animation {
            self.set_studio_mode(StudioMode::World);
        }
    }
}

/// Selects an edited clip, keeping the clock when the same asset is already shown.
pub fn select(sim: &mut Sim, name: &str) -> Result<(), String> {
    sim.preview_open(Some(name), None).map(|_| ())
}
