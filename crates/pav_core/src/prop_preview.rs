//! A prop stage sharing the animation studio's world-preserving frame path.

use std::sync::Arc;

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::frame::{HudFrame, RenderFrame, RenderObject};
use crate::prop_instance::{PropFrame, validate_scale};
use crate::props::{self, Bounds};
use crate::shape::{Shape, Visual};
use crate::{Color, EntityId, Sim, SimConfig};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioMode {
    #[default]
    World,
    Animation,
    Prop,
    Creature,
}

impl StudioMode {
    pub fn active(self) -> bool {
        self != Self::World
    }
}

/// Opaque camera parameters are interpreted by tools/view, never by the simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StudioCamera {
    pub params: serde_json::Value,
    pub focus: Vec3,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StudioCameras {
    pub world: Option<StudioCamera>,
    pub animation: Option<StudioCamera>,
    pub prop: Option<StudioCamera>,
    #[serde(default)]
    pub creature: Option<StudioCamera>,
}

impl StudioCameras {
    pub fn get(&self, mode: StudioMode) -> Option<&StudioCamera> {
        match mode {
            StudioMode::World => self.world.as_ref(),
            StudioMode::Animation => self.animation.as_ref(),
            StudioMode::Prop => self.prop.as_ref(),
            StudioMode::Creature => self.creature.as_ref(),
        }
    }

    pub fn set(&mut self, mode: StudioMode, camera: StudioCamera) {
        *match mode {
            StudioMode::World => &mut self.world,
            StudioMode::Animation => &mut self.animation,
            StudioMode::Prop => &mut self.prop,
            StudioMode::Creature => &mut self.creature,
        } = Some(camera);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropPreviewState {
    pub name: String,
    pub time: f32,
    pub playing: bool,
    pub speed: f32,
    pub turntable: bool,
    /// Base rotation in degrees. A turntable adds 30 degrees per scaled second.
    pub yaw: f32,
    pub scale: f32,
    pub focus: Vec3,
    ticks: u64,
    /// A 1.8 m figure beside the asset, for scale (contact sheets).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub figure: bool,
    /// The variant shown (0: as authored).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub seed: u32,
    /// A family's knob values shown (defaults left out).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub params: props::Params,
}

fn is_zero(seed: &u32) -> bool {
    *seed == 0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropPreviewInfo {
    pub name: String,
    pub revision: String,
    pub available: bool,
    pub parts: usize,
    /// Asset-local bounds at the selected scale. The stage lifts min.y to the floor.
    pub bounds: Bounds,
    pub time: f32,
    pub playing: bool,
    pub speed: f32,
    pub turntable: bool,
    pub yaw: f32,
    pub scale: f32,
    pub seed: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PropPreviewChange {
    pub time: Option<f32>,
    pub playing: Option<bool>,
    pub speed: Option<f32>,
    pub turntable: Option<bool>,
    pub yaw: Option<f32>,
    pub scale: Option<f32>,
    /// Signed 1/60-second frames. Stepping and scrubbing pause unless playing is explicit.
    pub step: Option<i32>,
    /// The variant to show.
    pub seed: Option<u32>,
    /// A family's knob values to show, checked (`PropDef::param_values`).
    pub params: Option<props::Params>,
}

impl PropPreviewChange {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [("time", self.time), ("speed", self.speed), ("yaw", self.yaw)] {
            if value.is_some_and(|v| !v.is_finite()) {
                return Err(format!("{name} must be finite"));
            }
        }
        if self.time.is_some_and(|v| !(0.0..=1_000_000.0).contains(&v)) {
            return Err("time must be between 0 and 1000000 seconds".into());
        }
        if self.speed.is_some_and(|v| !(0.05..=8.0).contains(&v)) {
            return Err("speed must be between 0.05 and 8".into());
        }
        if self.yaw.is_some_and(|v| v.abs() > 36_000.0) {
            return Err("yaw must be between -36000 and 36000 degrees".into());
        }
        if let Some(scale) = self.scale {
            validate_scale(scale)?;
        }
        Ok(())
    }
}

impl PropPreviewState {
    fn new(name: String, bounds: Bounds) -> Self {
        Self {
            name,
            time: 0.0,
            playing: true,
            speed: 1.0,
            turntable: false,
            yaw: 0.0,
            scale: 1.0,
            focus: bounds.center() - Vec3::Y * bounds.min.y,
            ticks: 0,
            figure: false,
            seed: 0,
            params: props::Params::new(),
        }
    }

    pub fn info(&self) -> PropPreviewInfo {
        let definition = props::get(&self.name);
        self.info_with(definition.as_deref())
    }

    fn info_with(&self, definition: Option<&props::PropDef>) -> PropPreviewInfo {
        PropPreviewInfo {
            name: self.name.clone(),
            revision: definition.map(|d| d.revision.clone()).unwrap_or_default(),
            available: definition.is_some(),
            parts: definition.map(|d| d.variant_with(&self.params, self.seed).parts.len()).unwrap_or(0),
            bounds: definition
                .map(|d| d.variant_with(&self.params, self.seed).bounds_at(self.scale))
                .unwrap_or(Bounds { min: Vec3::ZERO, max: Vec3::ZERO }),
            time: self.time,
            playing: self.playing,
            speed: self.speed,
            turntable: self.turntable,
            yaw: self.yaw,
            scale: self.scale,
            seed: self.seed,
        }
    }

    pub fn rotation(&self) -> Quat {
        let angle = self.yaw + if self.turntable { (self.time % 12.0) * 30.0 } else { 0.0 };
        Quat::from_rotation_y(angle.to_radians())
    }

    pub fn change(&mut self, change: &PropPreviewChange) -> Result<(), String> {
        change.validate()?;
        if let Some(time) = change.time {
            self.time = time;
        }
        if let Some(step) = change.step {
            self.time = (self.time + step as f32 / 60.0).clamp(0.0, 1_000_000.0);
        }
        if change.time.is_some() || change.step.is_some() {
            self.playing = false;
        }
        if let Some(playing) = change.playing {
            self.playing = playing;
        }
        if let Some(speed) = change.speed {
            self.speed = speed;
        }
        if let Some(turntable) = change.turntable {
            self.turntable = turntable;
        }
        if let Some(yaw) = change.yaw {
            self.yaw = yaw;
        }
        if let Some(scale) = change.scale {
            self.scale = scale;
        }
        if let Some(seed) = change.seed {
            self.seed = seed;
        }
        if let Some(params) = &change.params {
            self.params = params.clone();
        }
        Ok(())
    }

    pub(crate) fn advance(&mut self, dt: f32) {
        self.ticks = self.ticks.wrapping_add(1);
        if self.playing {
            self.time = (self.time + dt * self.speed).min(1_000_000.0);
        }
    }

    pub(crate) fn frame(&self, config: Arc<SimConfig>, dt: f32, ticket: u64) -> RenderFrame {
        self.frame_of(props::get(&self.name), Default::default(), config, dt, ticket)
    }

    fn frame_of(
        &self,
        definition: Option<Arc<props::PropDef>>,
        roles: std::collections::BTreeMap<String, String>,
        config: Arc<SimConfig>,
        dt: f32,
        ticket: u64,
    ) -> RenderFrame {
        let info = self.info_with(definition.as_deref());
        let mut objects = stage_objects();
        let subject = EntityId(STAGE_ID_BASE);
        if let Some(definition) = definition {
            objects.push(RenderObject {
                id: subject,
                pos: -Vec3::Y * info.bounds.min.y,
                rot: self.rotation(),
                visual: Visual::new(Shape::Sphere { radius: 0.0 }, Color::WHITE),
                prop: Some(PropFrame {
                    asset: self.name.clone(),
                    parts: definition.variant_with(&self.params, self.seed),
                    definition,
                    scale: self.scale,
                    roles,
                }),
                puppet: None,
                pulse: -1.0,
                soft: None,
                vehicle: None,
                cone: None,
                scenery: false,
            });
        }
        if self.figure && info.available {
            objects.extend(figure_at(info.bounds));
        }
        objects.sort_by_key(|o| o.id);
        RenderFrame {
            tick: self.ticks,
            time: self.time as f64,
            dt,
            live_edit_ticket: ticket,
            objects,
            statics: Default::default(),
            focus: self.focus,
            focus_is_player: false,
            player: Some(subject),
            puppet_def: config.puppet.clone(),
            room: None,
            config,
            events: Vec::new(),
            projectiles: Vec::new(),
            hud: HudFrame::default(),
            game: None,
            studio_mode: StudioMode::Prop,
            animation_preview: None,
            prop_preview: Some(info),
            creature_preview: None,
            creature: None,
        }
    }
}

/// The prop stage showing `definition`, an asset the library does not hold (a material swatch),
/// as `name`, at rest on the floor in the theme colours `roles`.
pub fn stage_frame(
    config: Arc<SimConfig>,
    name: &str,
    definition: Arc<props::PropDef>,
    roles: std::collections::BTreeMap<String, String>,
) -> RenderFrame {
    let mut state = PropPreviewState::new(name.to_owned(), definition.compiled.bounds);
    state.playing = false;
    state.frame_of(Some(definition), roles, config, 0.0, 0)
}

// Safely below the view's u32 object-group offset. Stages are isolated from world entities.
pub(crate) const STAGE_ID_BASE: u32 = 1_000_000;

/// Height of the scale figure on contact sheets.
const FIGURE_HEIGHT: f32 = 1.8;
const FIGURE_RADIUS: f32 = 0.22;

/// Where the figure stands on the floor beside an asset with these bounds: past its back-right
/// corner, so no front, side, back or 3/4 view hides the asset behind it.
fn figure_spot(bounds: Bounds) -> Vec3 {
    let gap = 0.35 + FIGURE_RADIUS;
    Vec3::new(bounds.max.x + gap, 0.0, bounds.min.z - gap)
}

/// The figure's bounds at `figure_spot`.
pub fn figure_bounds(bounds: Bounds) -> Bounds {
    let at = figure_spot(bounds);
    let r = Vec3::new(FIGURE_RADIUS, 0.0, FIGURE_RADIUS);
    Bounds { min: at - r, max: at + r + Vec3::Y * FIGURE_HEIGHT }
}

/// A grey capsule body and a head, 1.8 m tall in all (stage objects 42 and 43).
fn figure_at(bounds: Bounds) -> [RenderObject; 2] {
    let at = figure_spot(bounds);
    let head = 0.125;
    let body_top = FIGURE_HEIGHT - 2.0 * head;
    let part = |id: u32, pos: Vec3, shape: Shape, color: &str| RenderObject {
        id: EntityId(id),
        pos,
        rot: Quat::IDENTITY,
        visual: Visual::new(shape, Color::hex(color)),
        prop: None,
        puppet: None,
        pulse: -1.0,
        soft: None,
        vehicle: None,
        cone: None,
        scenery: true,
    };
    [
        part(
            STAGE_ID_BASE + 42,
            at + Vec3::Y * body_top * 0.5,
            Shape::Capsule { half_height: body_top * 0.5 - FIGURE_RADIUS, radius: FIGURE_RADIUS },
            "#8d949b",
        ),
        part(STAGE_ID_BASE + 43, at + Vec3::Y * (FIGURE_HEIGHT - head), Shape::Sphere { radius: head }, "#a7aeb5"),
    ]
}

/// Common metre grid for both studio subjects.
pub(crate) fn stage_objects() -> Vec<RenderObject> {
    let mut objects = Vec::with_capacity(39);
    let mut object = |id: u32, pos: Vec3, half: Vec3, color: &str| {
        objects.push(RenderObject {
            id: EntityId(id),
            pos,
            rot: Quat::IDENTITY,
            visual: Visual::new(Shape::Box { half }, Color::hex(color)),
            prop: None,
            puppet: None,
            pulse: -1.0,
            soft: None,
            vehicle: None,
            cone: None,
            scenery: true,
        });
    };
    object(STAGE_ID_BASE + 1, Vec3::new(0.0, -0.035, 0.0), Vec3::new(8.0, 0.025, 8.0), "#273338");
    for i in -8..=8 {
        let at = i as f32 * 0.5;
        object(STAGE_ID_BASE + 2 + (i + 8) as u32 * 2, Vec3::new(at, -0.008, 0.0), Vec3::new(0.005, 0.002, 4.0), "#3a494d");
        object(STAGE_ID_BASE + 3 + (i + 8) as u32 * 2, Vec3::new(0.0, -0.007, at), Vec3::new(4.0, 0.002, 0.005), "#3a494d");
    }
    object(STAGE_ID_BASE + 40, Vec3::new(0.0, -0.002, 0.0), Vec3::new(4.0, 0.002, 0.012), "#a97169");
    object(STAGE_ID_BASE + 41, Vec3::ZERO, Vec3::new(0.012, 0.002, 4.0), "#6598af");
    objects
}

impl Sim {
    pub fn studio_mode(&self) -> StudioMode {
        self.state.studio_mode.unwrap_or_else(|| {
            // Snapshots saved before the prop studio have no mode field.
            if self.state.animation_preview.is_some() { StudioMode::Animation } else { StudioMode::World }
        })
    }

    pub fn studio_active(&self) -> bool {
        self.studio_mode().active()
    }

    pub fn set_studio_mode(&mut self, mode: StudioMode) {
        self.state.studio_mode = Some(mode);
    }

    pub fn prop_preview_open(&mut self, name: Option<&str>) -> Result<PropPreviewInfo, String> {
        // `NAME:preset` shows a family's preset.
        let (name, preset) = match name {
            Some(name) => {
                let (base, preset) = props::split_preset(name);
                (props::canonical(base)?, preset)
            }
            None => (self.state.prop_preview.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "BUILTIN/bench".into()), None),
        };
        let definition = props::resolve_ref(&name)?;
        let params = preset.map(|preset| definition.param_values(Some(preset), &props::Params::new())).transpose()?;
        if self.state.prop_preview.as_ref().is_none_or(|p| p.name != name) {
            self.state.prop_preview = Some(PropPreviewState::new(name, definition.bounds(1.0)));
        }
        if let Some(params) = params {
            self.state.prop_preview.as_mut().unwrap().params = params;
        }
        self.set_studio_mode(StudioMode::Prop);
        Ok(self.state.prop_preview.as_ref().unwrap().info())
    }

    pub fn prop_preview_change(&mut self, change: &PropPreviewChange) -> Result<PropPreviewInfo, String> {
        let preview = self.state.prop_preview.as_mut().ok_or("no prop preview is open (asset_preview name=BUILTIN/bench)")?;
        preview.change(change)?;
        Ok(preview.info())
    }

    pub fn prop_preview_info(&self) -> Option<PropPreviewInfo> {
        (self.studio_mode() == StudioMode::Prop).then(|| self.state.prop_preview.as_ref().map(PropPreviewState::info)).flatten()
    }

    /// Returning to the world keeps this stage's selection, controls and camera for next time.
    pub fn prop_preview_close(&mut self) {
        if self.studio_mode() == StudioMode::Prop {
            self.set_studio_mode(StudioMode::World);
        }
    }
}

impl RenderFrame {
    pub fn studio_mode(&self) -> StudioMode {
        self.studio_mode
    }
    pub fn studio_active(&self) -> bool {
        self.studio_mode.active()
    }
}
