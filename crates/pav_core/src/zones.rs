//! Trigger zones and in-world labels: static content of a region (room, hub, terrain chunk).
//!
//! Zones are boxes that react to the player: course start/finish/gates, checkpoints, kill
//! volumes (pits), water, parameter pads (step on it to switch e.g. the movement model) and
//! camera cues. Labels are text drawn in the world by the renderer.

use std::collections::BTreeMap;

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::choice_enum;
use crate::color::Color;
use crate::params::ParamValue;
use crate::statics::Facing;

choice_enum! {
    /// What a zone does when the player is inside it.
    #[derive(Default)]
    pub enum ZoneKind {
        /// Leaving it starts the course timer.
        #[default]
        Start => "start",
        /// Entering it stops the timer (if every gate was passed in order).
        Finish => "finish",
        /// Course gates, passed in `index` order; missed gates add a time penalty.
        Gate => "gate",
        /// Sets the respawn point.
        Checkpoint => "checkpoint",
        /// Respawns the player at the last checkpoint (pits, lava).
        Kill => "kill",
        /// Swimming below the zone's top.
        Water => "water",
        /// Applies `params` (and `camera`) when stepped on; they last until you leave the room.
        Pad => "pad",
        /// Applies `camera` while inside.
        Camera => "camera",
    }
}

/// A camera parameter animated back and forth (camera bench sweeps).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sweep {
    /// Camera parameter name (tilt, yaw, distance, fov).
    pub param: String,
    pub from: f32,
    pub to: f32,
    /// Seconds for a full there-and-back cycle.
    pub period: f32,
    /// 0..1 offset into the cycle.
    pub phase: f32,
}

/// Camera changes requested by game data (pads, camera zones).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraCue {
    /// Camera parameters to set (tilt, yaw, distance, fov, ortho, ...). `yaw` is relative to
    /// the room's orientation.
    pub set: BTreeMap<String, ParamValue>,
    /// Parameters animated continuously while the cue is active.
    pub sweep: Vec<Sweep>,
    /// Toggle orthographic/perspective every this many seconds (0 = never).
    pub flip_ortho: f32,
}

impl CameraCue {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.sweep.is_empty() && self.flip_ortho <= 0.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub min: Vec3,
    pub max: Vec3,
    pub kind: ZoneKind,
    /// Course name (start/finish/gate/checkpoint of the same course share it).
    #[serde(default)]
    pub course: String,
    /// Gate order.
    #[serde(default)]
    pub index: i32,
    /// Pad: parameter overrides (full paths, e.g. "movement.model").
    #[serde(default)]
    pub params: BTreeMap<String, ParamValue>,
    #[serde(default)]
    pub camera: Option<CameraCue>,
    /// Text drawn on the floor of the zone.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub label_size: Option<f32>,
    /// Floor marking colour (None = the kind's default; transparent kinds draw nothing).
    #[serde(default)]
    pub color: Option<Color>,
    /// Direction the player faces after respawning here (checkpoints, start).
    #[serde(default)]
    pub facing: Option<Facing>,
}

impl Zone {
    /// Half-open on the ground plane, so zones that touch never both contain a point.
    pub fn contains(&self, p: Vec3) -> bool {
        p.x >= self.min.x && p.x < self.max.x && p.y >= self.min.y && p.y <= self.max.y && p.z >= self.min.z && p.z < self.max.z
    }
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    /// Floor point at the zone's centre.
    pub fn floor_center(&self) -> Vec3 {
        Vec3::new((self.min.x + self.max.x) * 0.5, self.min.y, (self.min.z + self.max.z) * 0.5)
    }
}

choice_enum! {
    /// How a label is oriented.
    #[derive(Default)]
    pub enum LabelMode {
        /// Lying on the floor, turned to read upright from the current camera.
        #[default]
        Floor => "floor",
        /// Upright on a wall, facing `facing`.
        Wall => "wall",
        /// Always facing the camera.
        Billboard => "billboard",
    }
}

/// Text in the world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub text: String,
    /// Centre of the text (bottom of the text box for billboards).
    pub pos: Vec3,
    /// Letter height (m).
    pub size: f32,
    pub color: Color,
    pub mode: LabelMode,
    /// Wall labels: the direction the text faces.
    pub facing: Facing,
}

/// Default floor-marking colours per zone kind (None = invisible).
pub fn default_zone_color(kind: ZoneKind) -> Option<Color> {
    match kind {
        ZoneKind::Start => Some(Color::hex("#4caf6a")),
        ZoneKind::Finish => Some(Color::hex("#f2f2f2")),
        ZoneKind::Gate => None,
        ZoneKind::Checkpoint => Some(Color::hex("#5b9bd5")),
        ZoneKind::Kill => None,
        ZoneKind::Water => Some(Color::hex("#3f88c5")),
        ZoneKind::Pad => Some(Color::hex("#9b6fd6")),
        ZoneKind::Camera => None,
    }
}
