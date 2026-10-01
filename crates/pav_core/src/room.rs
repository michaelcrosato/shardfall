//! Rooms: showcase spaces described by data files in `rooms/` (TOML). A room is a tile layout
//! plus metadata (info card, primary device, movement model, camera defaults, parameter
//! overrides) and free-placed objects. Room files hot-reload; new ones need no rebuild.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::character::MovementModel;
use crate::entity::{Behavior, BodyKind, Hazard};
use crate::level::{LabelDef, Layout};
use crate::params::ParamValue;
use crate::shape::{Look, Shape};
use crate::statics::Facing;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/rooms.rs"));
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    #[default]
    KeyboardMouse,
    Gamepad,
}

/// The door cell (inside the room, on its edge) and the direction out of the room.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entrance {
    pub at: [i32; 2],
    pub facing: Facing,
}

/// An object placed freely in the room (position in layout space: x = column, z = row).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectDef {
    #[serde(default)]
    pub name: String,
    pub shape: Shape,
    pub pos: Vec3,
    /// Rotation about the vertical axis (degrees).
    #[serde(default)]
    pub yaw: f32,
    #[serde(default = "gray")]
    pub color: String,
    #[serde(default = "dynamic")]
    pub body: BodyKind,
    #[serde(default)]
    pub look: Look,
    #[serde(default)]
    pub emissive: f32,
    #[serde(default)]
    pub behavior: Behavior,
    #[serde(default)]
    pub hazard: Option<Hazard>,
}

fn gray() -> String {
    "#b0b0b0".into()
}
fn dynamic() -> BodyKind {
    BodyKind::Dynamic
}
fn misc() -> String {
    "misc".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoomDef {
    #[serde(default)]
    pub name: String,
    /// Info card: what the room demonstrates.
    #[serde(default)]
    pub about: String,
    /// Info card: things to try.
    #[serde(default, rename = "try")]
    pub try_list: Vec<String>,
    /// Pavilion wing (movement, physics, animation, vfx, aesthetic, genre, misc).
    #[serde(default = "misc")]
    pub wing: String,
    /// Order inside the wing.
    #[serde(default)]
    pub order: i32,
    /// The device the room is designed for.
    #[serde(default)]
    pub primary_device: Device,
    /// The movement model the room is designed for (applied while inside).
    #[serde(default)]
    pub movement_model: Option<MovementModel>,
    /// Camera defaults while inside (camera.* parameter names without the prefix).
    #[serde(default)]
    pub camera: BTreeMap<String, ParamValue>,
    /// Simulation parameter overrides while inside (full paths, e.g. "movement.jump_height").
    #[serde(default)]
    pub params: BTreeMap<String, ParamValue>,
    /// Room-specific control guide lines: [action, keys].
    #[serde(default)]
    pub controls: Vec<[String; 2]>,
    /// Keyboard remapping while inside: action -> physical key names, e.g.
    /// `jump = ["KeyW"]`. Actions: move_up, move_down, move_left, move_right, jump, crouch,
    /// crawl, use, focus, interact. Key names follow the physical layout (KeyA, Digit1,
    /// Space, ShiftLeft, ArrowUp, ...).
    #[serde(default)]
    pub keys: BTreeMap<String, Vec<String>>,
    /// HUD overlays opened while inside: "feel" (feel metrics).
    #[serde(default)]
    pub overlays: Vec<String>,
    pub entrance: Entrance,
    pub layout: Layout,
    /// Free-placed text (positions in layout space).
    #[serde(default, rename = "label")]
    pub labels: Vec<LabelDef>,
    #[serde(default, rename = "object")]
    pub objects: Vec<ObjectDef>,
}

impl RoomDef {
    pub fn parse(text: &str) -> Result<RoomDef, String> {
        let def: RoomDef = toml::from_str(text).map_err(|e| e.to_string())?;
        def.validate()?;
        Ok(def)
    }

    /// Checks things the parser cannot (unknown legend characters, entrance on the map...).
    pub fn validate(&self) -> Result<(), String> {
        let (w, h) = self.layout.extent();
        if w == 0 || h == 0 {
            return Err("layout has no map rows".into());
        }
        let [c, r] = self.entrance.at;
        if c < 0 || r < 0 || c >= w || r >= h {
            return Err(format!("entrance {:?} is outside the {w}x{h} map", self.entrance.at));
        }
        let mut missing = Vec::new();
        for l in &self.layout.layers {
            for ch in l.map.chars() {
                if ch != ' ' && ch != '\n' && ch != '\r' && ch != '.' && !self.layout.legend.contains_key(&ch.to_string()) {
                    if !missing.contains(&ch) {
                        missing.push(ch);
                    }
                }
            }
        }
        if !missing.is_empty() {
            return Err(format!("map characters without a legend entry: {missing:?}"));
        }
        for l in &self.labels {
            if l.pos.is_none() {
                return Err(format!("label '{}' needs pos = [x, y, z]", l.text));
            }
        }
        for (k, t) in &self.layout.legend {
            if let Some(z) = &t.zone {
                if z.y1 <= z.y0 {
                    return Err(format!("legend '{k}': zone y1 must be above y0"));
                }
            }
        }
        Ok(())
    }

    /// Player start for a standalone room: the "player" marker, else just inside the entrance.
    pub fn local_start(&self) -> Vec3 {
        let [c, r] = self.entrance.at;
        Vec3::new(c as f32 + 0.5, 0.0, r as f32 + 0.5) - self.entrance.facing.dir() * 1.5
    }
}

/// A room file's text and where it came from.
#[derive(Clone, Debug)]
pub struct RoomSource {
    pub key: String,
    pub text: String,
    pub path: Option<PathBuf>,
}

/// The directory room files are read (and hot-reloaded) from, if any: `PAV_ROOMS`, then
/// `./rooms`, then `rooms/` next to the executable.
pub fn rooms_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("PAV_ROOMS") {
        return Some(PathBuf::from(d));
    }
    let cwd = PathBuf::from("rooms");
    if cwd.is_dir() {
        return Some(cwd);
    }
    let exe = std::env::current_exe().ok()?.parent()?.join("rooms");
    exe.is_dir().then_some(exe)
}

/// Embedded room files, overridden/extended by files in `dir`. Names starting with '_' are
/// templates and are skipped.
pub fn load_sources(dir: Option<&Path>) -> Vec<RoomSource> {
    let mut out: Vec<RoomSource> =
        embedded::EMBEDDED.iter().map(|(k, t)| RoomSource { key: k.to_string(), text: t.to_string(), path: None }).collect();
    if let Some(dir) = dir {
        if let Ok(rd) = std::fs::read_dir(dir) {
            let mut files: Vec<PathBuf> =
                rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
            files.sort();
            for p in files {
                let key = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let Ok(text) = std::fs::read_to_string(&p) else { continue };
                match out.iter_mut().find(|r| r.key == key) {
                    Some(r) => {
                        r.text = text;
                        r.path = Some(p);
                    }
                    None => out.push(RoomSource { key, text, path: Some(p) }),
                }
            }
        }
    }
    out.retain(|r| !r.key.starts_with('_'));
    out
}

/// Parses sources; returns the good rooms and (key, error) for the bad ones.
pub fn parse_all(sources: &[RoomSource]) -> (Vec<(String, RoomDef)>, Vec<(String, String)>) {
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    for s in sources {
        match RoomDef::parse(&s.text) {
            Ok(d) => ok.push((s.key.clone(), d)),
            Err(e) => bad.push((s.key.clone(), e)),
        }
    }
    (ok, bad)
}

pub fn embedded_keys() -> Vec<&'static str> {
    embedded::EMBEDDED.iter().map(|(k, _)| *k).collect()
}
