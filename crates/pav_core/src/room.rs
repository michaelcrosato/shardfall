//! Rooms: showcase spaces described by data files in `rooms/` (TOML). A room is a tile layout
//! plus metadata (info card, primary device, movement model, camera defaults, parameter
//! overrides) and free-placed objects. Room files hot-reload; new ones need no rebuild.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::character::MovementModel;
use crate::choice_enum;
use crate::entity::{Behavior, BodyKind, Hazard};
use crate::joints::JointKind;
use crate::level::{LabelDef, Layout};
use crate::params::ParamValue;
use crate::shape::{Look, Shape};
use crate::softbody::SoftDef;
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
    #[serde(default = "box_shape")]
    pub shape: Shape,
    /// Reusable procedural prop. Its named parts replace shape/color/look for rendering.
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default = "one")]
    pub scale: f32,
    pub pos: Vec3,
    /// Rotation about the vertical axis (degrees).
    #[serde(default)]
    pub yaw: f32,
    /// Tilt about the object's X axis and Z axis (degrees): ramps, seesaws.
    #[serde(default)]
    pub pitch: f32,
    #[serde(default)]
    pub roll: f32,
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
    #[serde(default = "one")]
    pub density: f32,
    #[serde(default = "half")]
    pub friction: f32,
    #[serde(default)]
    pub restitution: f32,
    /// Air drag (linear and angular damping, 0 = none).
    #[serde(default)]
    pub damping: f32,
    /// A deformable body instead of a rigid one (jelly, ball, cloth, rope).
    #[serde(default)]
    pub soft: Option<SoftDef>,
    /// Visual-only effects: a point light, a particle emitter, a screen distortion.
    #[serde(default)]
    pub light: Option<crate::fxdef::LightDef>,
    #[serde(default)]
    pub particles: Option<crate::fxdef::EmitterDef>,
    #[serde(default)]
    pub distortion: Option<crate::fxdef::DistortDef>,
    /// A drivable vehicle (`shape`, `body` and colour come from the vehicle instead); `pos` is
    /// where its wheels / skids stand.
    #[serde(default)]
    pub vehicle: Option<crate::vehicle::VehicleDef>,
    /// Shootable (enemies, bosses): hp, score, signal, finish, bar, sway, phases.
    #[serde(default)]
    pub health: Option<crate::entity::HealthDef>,
    /// Moves in the wind (drawing only): "leaves" for canopies and banners, "grass" for tufts.
    #[serde(default)]
    pub sway: crate::shape::Sway,
}

impl ObjectDef {
    /// Rotation in the room's own frame.
    pub fn local_rot(&self) -> glam::Quat {
        glam::Quat::from_euler(glam::EulerRot::YXZ, self.yaw.to_radians(), self.pitch.to_radians(), self.roll.to_radians())
    }
}

fn box_shape() -> Shape {
    Shape::Box { half: Vec3::splat(0.5) }
}
fn one() -> f32 {
    1.0
}
fn half() -> f32 {
    0.5
}

/// A joint between two named objects (or an object and the world), anchored at `at`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointDef {
    pub a: String,
    /// The other object; empty = fixed to the world.
    #[serde(default)]
    pub b: String,
    /// Anchor point in layout space (x = column, y = height, z = row).
    pub at: Vec3,
    /// Anchor on `b` (or the world) when it differs from `at`: a spring that starts stretched.
    #[serde(default)]
    pub at_b: Option<Vec3>,
    pub joint: JointKind,
}

choice_enum! {
    #[derive(Default)]
    pub enum ChainStyle {
        /// Capsule links joined end to end.
        #[default]
        Chain => "chain",
        /// Planks hinged edge to edge: a walkable rope bridge.
        Bridge => "bridge",
    }
}

/// A generated chain or rope bridge between two points (layout space).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChainDef {
    pub from: Vec3,
    pub to: Vec3,
    pub style: ChainStyle,
    pub links: u32,
    /// Chain link radius / plank thickness (m).
    pub radius: f32,
    /// Bridge width (m).
    pub width: f32,
    /// Extra slack: the chain hangs this fraction of its span lower in the middle.
    pub sag: f32,
    pub color: String,
    pub density: f32,
    /// Air drag on the links (calms bouncing bridges).
    pub damping: f32,
    /// Ends fixed to the world (default: the start always, the end for bridges) ...
    pub fix_from: bool,
    pub fix_to: Option<bool>,
    /// ... or tied to named objects.
    pub attach_from: String,
    pub attach_to: String,
}

impl Default for ChainDef {
    fn default() -> Self {
        Self {
            from: Vec3::ZERO,
            to: Vec3::new(0.0, -3.0, 0.0),
            style: ChainStyle::Chain,
            links: 10,
            radius: 0.07,
            width: 1.4,
            sag: 0.0,
            color: "#8a8f99".into(),
            // Wood/steel-ish: light links make joints unstable under a character's weight.
            density: 400.0,
            damping: 0.3,
            fix_from: true,
            fix_to: None,
            attach_from: String::new(),
            attach_to: String::new(),
        }
    }
}

fn gray() -> String {
    "#b0b0b0".into()
}
fn dynamic() -> BodyKind {
    BodyKind::Dynamic
}
fn room_height() -> f32 {
    8.0
}

fn misc() -> String {
    "misc".into()
}

/// A station guide (`[learn]` in a room file). Plain words; explain jargon the first time
/// (or list it in `terms`, the field guide explains it).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Learn {
    /// What you are looking at, in a few sentences.
    pub what: String,
    /// How it works, step by step.
    pub how: Vec<String>,
    /// Where games use it: "Title: text" (name real games where it is well known).
    pub uses: Vec<String>,
    /// Phrases to ask for it ("normal-mapped 2D lights with soft shadows").
    pub ask: Vec<String>,
    /// What it costs and what makes it cost more.
    pub cost: String,
    /// Settings that drive it (paths as in `pav params`, e.g. "view.bloom" or
    /// "movement.jump_height"): live controls on the guide.
    pub knobs: Vec<String>,
    /// The key code, quoted from the engine.
    pub code: Vec<CodeSnippet>,
    /// Field guide words this station uses (`pav guide` / the in-game field guide).
    pub terms: Vec<String>,
}

impl Learn {
    pub fn is_empty(&self) -> bool {
        *self == Learn::default()
    }
}

impl RoomDef {
    /// The room's pads in legend order: label and note.
    pub fn pads(&self) -> Vec<(&str, &str)> {
        self.layout
            .legend
            .values()
            .filter_map(|t| t.zone.as_ref())
            .filter(|z| z.kind == crate::zones::ZoneKind::Pad && !z.label.is_empty())
            .map(|z| (z.label.as_str(), z.note.as_str()))
            .collect()
    }
}

/// A piece of engine code on a station guide.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CodeSnippet {
    pub title: String,
    /// Where it lives (repository path), e.g. "crates/pav_render/src/shaders/post.wgsl".
    pub file: String,
    /// "wgsl" or "rust".
    pub lang: String,
    pub src: String,
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
    /// View setting overrides while inside (`pav params prefix=view`, without the `view.`):
    /// e.g. "light.sun_intensity" = 0.1, bloom = 0.8, sky = "#101830".
    #[serde(default)]
    pub view: BTreeMap<String, ParamValue>,
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
    /// The station guide: what you are looking at, how it works, where games use it and how
    /// to ask for it (H in the game; the `room` tool prints it).
    #[serde(default)]
    pub learn: Learn,
    /// Height of the room's volume (m): above it (plus a margin) you count as outside, which
    /// cancels its courses. Raise it for rooms you fly in.
    #[serde(default = "room_height")]
    pub height: f32,
    pub entrance: Entrance,
    pub layout: Layout,
    /// Free-placed text (positions in layout space).
    #[serde(default, rename = "label")]
    pub labels: Vec<LabelDef>,
    #[serde(default, rename = "joint")]
    pub joints: Vec<JointDef>,
    #[serde(default, rename = "chain")]
    pub chains: Vec<ChainDef>,
    #[serde(default, rename = "npc")]
    pub npcs: Vec<NpcDef>,
    #[serde(default, rename = "object")]
    pub objects: Vec<ObjectDef>,
}

/// A non-player character or creature (layout space).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NpcDef {
    #[serde(default)]
    pub name: String,
    /// Body plan preset: biped | spider | lizard | beetle | blob.
    #[serde(default)]
    pub kind: crate::puppet::BodyPlan,
    /// Feet position (layout space).
    pub pos: Vec3,
    /// Facing (degrees about Y, 0 = +Z / south).
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub ai: crate::ai::AiDef,
    /// Fraction of full speed (0..1).
    #[serde(default = "half_speed")]
    pub speed: f32,
    /// Seconds between hops (0 = never).
    #[serde(default)]
    pub hop: f32,
    /// Puppet settings over the preset (any puppet field: colours, proportions, look...).
    #[serde(default)]
    pub look: toml::Table,
    /// A performance: motion clips (`SET/Clip`, see the `clips` tool) and then moves
    /// (anim/moves.toml), played one after another for ever.
    #[serde(default)]
    pub clips: Vec<String>,
    #[serde(default)]
    pub moves: Vec<String>,
    /// Seconds each looping clip plays before the next (default 5).
    #[serde(default)]
    pub hold: f32,
    /// Clips mirrored left to right, or played on the upper body only (the legs keep walking).
    #[serde(default)]
    pub mirror: bool,
    #[serde(default)]
    pub upper: bool,
    /// Speed of the performance (1 = as captured).
    #[serde(default)]
    pub tempo: f32,
}

fn half_speed() -> f32 {
    0.5
}

impl NpcDef {
    /// The performance (`clips`, `moves`), resolved; None when there is none.
    pub fn perform(&self) -> Result<Option<crate::ai::Perform>, String> {
        if self.clips.is_empty() && self.moves.is_empty() {
            return Ok(None);
        }
        let lib = crate::clips::library();
        let clips = self
            .clips
            .iter()
            .map(|c| lib.find(c).ok_or_else(|| format!("npc '{}': no clip '{c}' (the clips tool lists them)", self.name)))
            .collect::<Result<Vec<_>, _>>()?;
        let moves = self
            .moves
            .iter()
            .map(|m| {
                crate::moves::MoveId::named(m)
                    .filter(|m| m.index() != 0)
                    .map(|m| m.index())
                    .ok_or_else(|| format!("npc '{}': no move '{m}' (anim/moves.toml)", self.name))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut flags = 0;
        if self.mirror {
            flags |= crate::clips::MIRROR;
        }
        if self.upper {
            flags |= crate::clips::UPPER;
        }
        Ok(Some(crate::ai::Perform::new(clips, moves, flags, if self.hold > 0.0 { self.hold } else { 5.0 }, self.tempo)))
    }

    /// The preset for `kind` with `look` applied.
    pub fn puppet(&self) -> Result<crate::puppet::PuppetDef, String> {
        let preset = crate::puppet::PuppetDef::preset(self.kind);
        if self.look.is_empty() {
            return Ok(preset);
        }
        let mut table = toml::Table::try_from(&preset).map_err(|e| e.to_string())?;
        for (k, v) in &self.look {
            if !table.contains_key(k) {
                return Err(format!("npc '{}': unknown look setting '{k}'", self.name));
            }
            table.insert(k.clone(), v.clone());
        }
        toml::Value::Table(table).try_into().map_err(|e: toml::de::Error| format!("npc '{}': {e}", self.name))
    }
}

impl RoomDef {
    pub fn parse(text: &str) -> Result<RoomDef, String> {
        let def: RoomDef = toml::from_str(text).map_err(|e| e.to_string())?;
        def.validate()?;
        Ok(def)
    }

    /// Checks things the parser cannot (unknown legend characters, entrance on the map...).
    pub fn validate(&self) -> Result<(), String> {
        for object in &self.objects {
            if let Some(asset) = &object.asset {
                crate::props::canonical(asset).map_err(|e| format!("object '{}': {e}", object.name))?;
                crate::prop_instance::validate_scale(object.scale)?;
                crate::prop_instance::validate_pose(object.pos, object.local_rot())?;
                if !matches!(object.body, BodyKind::Fixed | BodyKind::None) {
                    return Err(format!("prop '{}': body must be fixed or none", object.name));
                }
                if object.soft.is_some() || object.vehicle.is_some() || object.behavior != Behavior::None {
                    return Err(format!("prop '{}': soft bodies, vehicles and behaviors are not supported", object.name));
                }
            }
        }
        for n in &self.npcs {
            let p = n.puppet()?;
            for (field, clip) in [("idle_clip", &p.idle_clip), ("walk_clip", &p.walk_clip)] {
                if !clip.is_empty() && crate::clips::find(clip).is_none() {
                    return Err(format!("npc '{}': no clip '{clip}' for {field} (the clips tool lists them)", n.name));
                }
            }
            n.perform()?;
        }
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
