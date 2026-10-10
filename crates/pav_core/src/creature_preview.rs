//! A native creature stage. Its immutable definition and controls are part of the snapshot;
//! compilation and publication happen outside the simulation.

use std::sync::Arc;

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::creatures::{self, Bounds, CreatureAsset};
use crate::frame::{HudFrame, RenderFrame};
use crate::prop_preview::{STAGE_ID_BASE, StudioMode, stage_objects};
use crate::{EntityId, Sim, SimConfig};

/// The view samples and skins this definition using the same clock the tools inspect.
#[derive(Clone, Debug)]
pub struct CreatureFrame {
    pub id: EntityId,
    pub definition: Arc<CreatureAsset>,
    pub clip: Option<String>,
    pub time: f32,
    pub looping: bool,
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreaturePreviewState {
    pub name: String,
    pub definition: Arc<CreatureAsset>,
    pub clip: Option<String>,
    pub time: f32,
    pub playing: bool,
    pub speed: f32,
    pub looping: bool,
    pub turntable: bool,
    pub yaw: f32,
    pub scale: f32,
    pub focus: Vec3,
    /// Continuous scaled seconds, so the turntable does not jump when an animation loops.
    turntable_time: f32,
    ticks: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreaturePreviewInfo {
    pub name: String,
    pub revision: String,
    pub source_revision: String,
    pub available: bool,
    pub title: String,
    pub quality: String,
    pub bones: usize,
    pub vertices: usize,
    pub triangles: usize,
    /// Model-space rest bounds, at the selected scale. The stage puts min.y on the floor.
    pub bounds: Bounds,
    /// "rest" selects the unanimated rest pose.
    pub clip: String,
    pub clips: Vec<String>,
    pub duration: f32,
    pub time: f32,
    pub playing: bool,
    pub speed: f32,
    pub looping: bool,
    pub turntable: bool,
    pub yaw: f32,
    pub scale: f32,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CreaturePreviewChange {
    pub clip: Option<String>,
    pub time: Option<f32>,
    pub playing: Option<bool>,
    pub speed: Option<f32>,
    pub looping: Option<bool>,
    pub turntable: Option<bool>,
    pub yaw: Option<f32>,
    pub scale: Option<f32>,
    /// Signed source frames (1/60 second for the rest pose). Scrubbing and stepping pause.
    pub step: Option<i32>,
}

impl CreaturePreviewChange {
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
            crate::prop_instance::validate_scale(scale)?;
        }
        Ok(())
    }
}

fn default_clip(asset: &CreatureAsset) -> Option<String> {
    if asset.clips.contains_key("idle") { Some("idle".into()) } else { asset.clips.keys().next().cloned() }
}

impl CreaturePreviewState {
    fn new(name: String, definition: Arc<CreatureAsset>) -> Self {
        let clip = default_clip(&definition);
        let looping = clip.as_ref().and_then(|name| definition.clips.get(name)).is_none_or(|clip| clip.looping);
        let focus = definition.bounds.center() - Vec3::Y * definition.bounds.min.y;
        Self {
            name,
            definition,
            clip,
            time: 0.0,
            playing: true,
            speed: 1.0,
            looping,
            turntable: false,
            yaw: 0.0,
            scale: 1.0,
            focus,
            turntable_time: 0.0,
            ticks: 0,
        }
    }

    pub fn duration(&self) -> f32 {
        self.clip.as_ref().and_then(|name| self.definition.clips.get(name)).map_or(0.0, |clip| clip.duration)
    }

    pub fn bounds(&self) -> Bounds {
        Bounds { min: self.definition.bounds.min * self.scale, max: self.definition.bounds.max * self.scale }
    }

    pub fn info(&self) -> CreaturePreviewInfo {
        let mut clips = vec!["rest".into()];
        clips.extend(self.definition.clips.keys().cloned());
        CreaturePreviewInfo {
            name: self.name.clone(),
            revision: self.definition.revision(),
            source_revision: self.definition.source_revision.clone(),
            available: true,
            title: self.definition.title.clone(),
            quality: self.definition.quality.clone(),
            bones: self.definition.bones.names.len(),
            vertices: self.definition.vertex_count(),
            triangles: self.definition.triangle_count(),
            bounds: self.bounds(),
            clip: self.clip.clone().unwrap_or_else(|| "rest".into()),
            clips,
            duration: self.duration(),
            time: self.time,
            playing: self.playing,
            speed: self.speed,
            looping: self.looping,
            turntable: self.turntable,
            yaw: self.yaw,
            scale: self.scale,
            warnings: self.definition.warnings.clone(),
        }
    }

    pub fn rotation(&self) -> Quat {
        let angle = self.yaw + if self.turntable { (self.turntable_time % 12.0) * 30.0 } else { 0.0 };
        Quat::from_rotation_y(angle.to_radians())
    }

    /// A change is checked as a whole before any controls are written.
    pub fn change(&mut self, change: &CreaturePreviewChange) -> Result<(), String> {
        change.validate()?;
        let mut next = self.clone();
        if let Some(name) = &change.clip {
            let clip = if name == "rest" || name.is_empty() { None } else { Some(name.clone()) };
            if let Some(name) = &clip {
                if !next.definition.clips.contains_key(name) {
                    return Err(format!("no creature clip '{name}'"));
                }
            }
            if next.clip != clip {
                next.clip = clip;
                next.time = 0.0;
                next.looping =
                    next.clip.as_ref().and_then(|name| next.definition.clips.get(name)).is_none_or(|clip| clip.looping);
            }
        }
        if let Some(time) = change.time {
            next.time = time;
        }
        if let Some(step) = change.step {
            let seconds = next
                .clip
                .as_ref()
                .and_then(|name| next.definition.clips.get(name))
                .map_or(1.0 / 60.0, |clip| clip.duration / (clip.frames - 1) as f32);
            next.time += step as f32 * seconds;
        }
        next.time = if next.clip.is_some() { next.time.clamp(0.0, next.duration()) } else { 0.0 };
        if change.time.is_some() || change.step.is_some() {
            next.playing = false;
        }
        if let Some(playing) = change.playing {
            next.playing = playing;
        }
        if let Some(speed) = change.speed {
            next.speed = speed;
        }
        if let Some(looping) = change.looping {
            next.looping = looping;
        }
        if let Some(turntable) = change.turntable {
            next.turntable = turntable;
        }
        if let Some(yaw) = change.yaw {
            next.yaw = yaw;
        }
        if let Some(scale) = change.scale {
            next.scale = scale;
        }
        *self = next;
        Ok(())
    }

    fn refresh(&mut self, definition: &Arc<CreatureAsset>) -> bool {
        if self.definition.revision() == definition.revision() {
            return false;
        }
        let compatible =
            self.definition.bones.names == definition.bones.names && self.definition.bones.parents == definition.bones.parents;
        let lost_clip = self.clip.as_ref().is_some_and(|name| !definition.clips.contains_key(name));
        self.definition = definition.clone();
        if lost_clip {
            self.clip = default_clip(definition);
            self.looping = self.clip.as_ref().and_then(|name| definition.clips.get(name)).is_none_or(|clip| clip.looping);
        }
        if !compatible || lost_clip {
            self.time = 0.0;
        }
        self.time = self.time.min(self.duration());
        // Camera focus, stage rotation, speed and playing stay as the user set them.
        true
    }

    pub(crate) fn advance(&mut self, dt: f32) {
        self.ticks = self.ticks.wrapping_add(1);
        if !self.playing {
            return;
        }
        let elapsed = dt * self.speed;
        self.turntable_time = (self.turntable_time + elapsed).rem_euclid(12.0);
        if self.clip.is_none() {
            return;
        }
        self.time += elapsed;
        let duration = self.duration();
        if self.looping {
            self.time = self.time.rem_euclid(duration);
        } else if self.time >= duration {
            self.time = duration;
            self.playing = false;
        }
    }

    pub(crate) fn frame(&self, config: Arc<SimConfig>, dt: f32, ticket: u64) -> RenderFrame {
        let subject = EntityId(STAGE_ID_BASE);
        let mut frame = empty_frame(config, dt, ticket);
        frame.tick = self.ticks;
        frame.time = self.time as f64;
        frame.focus = self.focus;
        frame.player = Some(subject);
        frame.creature_preview = Some(self.info());
        frame.creature = Some(CreatureFrame {
            id: subject,
            definition: self.definition.clone(),
            clip: self.clip.clone(),
            time: self.time,
            looping: self.looping,
            pos: -Vec3::Y * self.bounds().min.y,
            rot: self.rotation(),
            scale: self.scale,
        });
        frame
    }
}

/// The workspace can open before the first compiler result. It still freezes the world and
/// draws the isolated stage rather than exposing the room behind it.
pub(crate) fn empty_frame(config: Arc<SimConfig>, dt: f32, ticket: u64) -> RenderFrame {
    RenderFrame {
        tick: 0,
        time: 0.0,
        dt,
        live_edit_ticket: ticket,
        objects: stage_objects(),
        statics: Default::default(),
        focus: Vec3::Y,
        focus_is_player: false,
        player: None,
        puppet_def: config.puppet.clone(),
        room: None,
        config,
        events: Vec::new(),
        projectiles: Vec::new(),
        hud: HudFrame::default(),
        game: None,
        studio_mode: StudioMode::Creature,
        animation_preview: None,
        prop_preview: None,
        creature_preview: None,
        creature: None,
    }
}

impl Sim {
    pub fn creature_preview_open(&mut self, name: Option<&str>) -> Result<CreaturePreviewInfo, String> {
        if name.is_none() && self.state.creature_preview.is_some() {
            // Returning to a saved workspace must not consume a newer registry definition.
            self.set_studio_mode(StudioMode::Creature);
            return Ok(self.state.creature_preview.as_ref().unwrap().info());
        }
        let name = match name {
            Some(name) => creatures::canonical(name)?,
            None => creatures::library()
                .assets
                .keys()
                .next()
                .cloned()
                .ok_or("no compiled creature is available; create one with creature_edit")?,
        };
        let definition = creatures::get(&name).ok_or_else(|| format!("no compiled creature '{name}'"))?;
        if let Some(preview) = self.state.creature_preview.as_mut().filter(|p| p.name == name) {
            preview.refresh(&definition);
        } else {
            self.state.creature_preview = Some(CreaturePreviewState::new(name, definition));
        }
        self.set_studio_mode(StudioMode::Creature);
        Ok(self.state.creature_preview.as_ref().unwrap().info())
    }

    pub fn creature_preview_change(&mut self, change: &CreaturePreviewChange) -> Result<CreaturePreviewInfo, String> {
        let preview = self.state.creature_preview.as_mut().ok_or("no creature preview is open")?;
        preview.change(change)?;
        Ok(preview.info())
    }

    pub fn creature_preview_info(&self) -> Option<CreaturePreviewInfo> {
        (self.studio_mode() == StudioMode::Creature)
            .then(|| self.state.creature_preview.as_ref().map(CreaturePreviewState::info))
            .flatten()
    }

    /// Adopt an accepted compile for the retained selection without changing workspace/camera.
    pub fn refresh_creature_preview(&mut self, name: &str, definition: &Arc<CreatureAsset>) -> bool {
        let Ok(name) = creatures::canonical(name) else {
            return false;
        };
        if name.split_once('/').map(|(_, leaf)| leaf) != Some(definition.name.as_str()) {
            return false;
        }
        self.state.creature_preview.as_mut().filter(|p| p.name == name).is_some_and(|p| p.refresh(definition))
    }

    pub fn creature_preview_close(&mut self) {
        if self.studio_mode() == StudioMode::Creature {
            self.set_studio_mode(StudioMode::World);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InputFrame;

    fn install_fixture(name: &str) -> Arc<CreatureAsset> {
        let definition: Arc<CreatureAsset> = Arc::new(serde_json::from_value(crate::creatures::tests::fixture(name)).unwrap());
        creatures::install(name, definition.clone()).unwrap();
        definition
    }

    #[test]
    fn stage_preserves_world_and_other_workspaces_while_stepping_its_own_clip() {
        install_fixture("stage-fixture");
        let mut sim = Sim::new("empty", 31).unwrap();
        sim.run(10, &InputFrame::default());
        let world = sim.state_hash();
        let tick = sim.state.tick;
        let recorded = sim.recording.ticks();
        sim.set_studio_mode(StudioMode::Creature);
        let empty = sim.frame();
        assert_eq!(empty.studio_mode, StudioMode::Creature);
        assert!(empty.creature.is_none() && empty.game.is_none());
        assert!(empty.objects.iter().all(|object| object.id.0 >= STAGE_ID_BASE));
        sim.run(2, &InputFrame::default());
        assert_eq!(sim.state.tick, tick);
        sim.preview_open(Some("QUATERNIUS/Sword_Regular_Combo"), None).unwrap();
        sim.preview_change(&crate::animation_preview::PreviewChange { time: Some(0.4), ..Default::default() }).unwrap();
        sim.creature_preview_open(Some("stage-fixture")).unwrap();
        sim.creature_preview_change(&CreaturePreviewChange { time: Some(0.5), ..Default::default() }).unwrap();
        let before = serde_json::to_value(sim.creature_preview_info()).unwrap();
        assert!(
            sim.creature_preview_change(&CreaturePreviewChange {
                clip: Some("missing".into()),
                time: Some(0.1),
                ..Default::default()
            })
            .is_err()
        );
        assert_eq!(serde_json::to_value(sim.creature_preview_info()).unwrap(), before);
        sim.creature_preview_change(&CreaturePreviewChange { playing: Some(true), ..Default::default() }).unwrap();
        sim.run(60, &InputFrame::default());
        let info = sim.creature_preview_info().unwrap();
        assert_eq!(info.time, 1.0);
        assert!(!info.playing);
        assert_eq!(sim.state.tick, tick);
        assert_eq!(sim.recording.ticks(), recorded);
        assert_eq!(sim.state_hash(), world);
        assert_eq!(sim.state.animation_preview.as_ref().unwrap().info().time, 0.4);
        assert_eq!(sim.frame().creature.as_ref().unwrap().clip.as_deref(), Some("bend"));
        sim.creature_preview_close();
        sim.step(&InputFrame::default());
        assert_eq!(sim.state.tick, tick + 1);
        creatures::remove("stage-fixture").unwrap();
    }

    #[test]
    fn accepted_revisions_keep_compatible_controls_and_snapshots_keep_their_definition() {
        let original = install_fixture("refresh-fixture");
        let mut sim = Sim::empty(1);
        sim.creature_preview_open(Some("refresh-fixture")).unwrap();
        sim.creature_preview_change(&CreaturePreviewChange {
            time: Some(0.4),
            yaw: Some(37.0),
            speed: Some(2.0),
            looping: Some(true),
            ..Default::default()
        })
        .unwrap();
        let snapshot: crate::SimState = serde_json::from_value(serde_json::to_value(&sim.state).unwrap()).unwrap();
        let mut changed = crate::creatures::tests::fixture("refresh-fixture");
        changed["source_revision"] = serde_json::json!("next-source");
        changed["meshes"]["skin"]["colors"][0] = serde_json::json!(0.5);
        let changed = Arc::new(serde_json::from_value::<CreatureAsset>(changed).unwrap());
        creatures::install("refresh-fixture", changed.clone()).unwrap();
        assert!(sim.refresh_creature_preview("refresh-fixture", &changed));
        let info = sim.creature_preview_info().unwrap();
        assert_eq!((info.time, info.yaw, info.speed), (0.4, 37.0, 2.0));
        assert_eq!(info.revision, changed.revision());
        assert!(!info.playing);
        sim.restore(snapshot);
        assert_eq!(sim.creature_preview_info().unwrap().revision, original.revision());
        sim.creature_preview_close();
        sim.creature_preview_open(None).unwrap();
        assert_eq!(sim.creature_preview_info().unwrap().revision, original.revision());
        creatures::remove("refresh-fixture").unwrap();
    }
}
