//! Agent tools. One registry, three ways in:
//!
//! ```text
//! pav <tool> [key=value ...]   one shot on a fresh session (game=, seed=, ticks= set it up)
//! pav repl [game=NAME]         one session; a tool per line on stdin, one JSON line out each
//! pav mcp [game=NAME]          MCP server over stdio (captures come back as images)
//! pav play <game> [seed=N]     the window, for people
//! pav help                     every tool and its arguments
//! ```
//!
//! Add a tool: write `fn t_name(s: &mut Session, a: &Args) -> R` and list it in `TOOLS`.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use glam::{Vec2, Vec3};
use serde_json::{Map, Value, json};

use crate::entity::{Body, Entity, Id, Look, Spawn};
use crate::input::{Input, buttons};
use crate::level::{self, Size, SpawnDef};
use crate::params::{self, Choice, ParamValue};
use crate::puppet::Puppet;
use crate::render::Image;
use crate::sim::{Game, GameDef, Replay, Sim};
use crate::util::Color;
use crate::view::{self, Draw};
use crate::world::{Event, World};

pub type Args = Map<String, Value>;

pub enum Output {
    Json(Value),
    /// A PNG (also saved to `meta.path`) plus what it shows.
    Image {
        png: Vec<u8>,
        meta: Value,
    },
}

type R = Result<Output, String>;

pub struct Arg {
    pub name: &'static str,
    /// JSON schema type: string, integer, number, boolean, array.
    pub kind: &'static str,
    pub help: &'static str,
}

pub struct Tool {
    pub name: &'static str,
    pub help: &'static str,
    pub args: &'static [Arg],
    /// Needs a loaded game.
    pub game: bool,
    pub run: fn(&mut Session, &Args) -> R,
}

const fn arg(name: &'static str, kind: &'static str, help: &'static str) -> Arg {
    Arg { name, kind, help }
}

const DRIVE: [Arg; 6] = [
    arg("move", "array", "[x, z] world direction: [1,0] east, [-1,0] west, [0,-1] north, [0,1] south"),
    arg("toward", "array", "[x, y, z] walk toward this point instead (stops within 0.3 m)"),
    arg("hold", "string", "buttons held, comma separated: jump,fire,alt,use,crouch,dash"),
    arg("press", "string", "buttons pressed on the first tick only (a jump needs press=jump; hold=jump makes it higher)"),
    arg("aim", "array", "[x, y, z] aim point"),
    arg("ticks", "integer", "ticks to run (60 = 1 second; default 1)"),
];

pub static TOOLS: &[Tool] = &[
    Tool { name: "help", help: "Every tool and its arguments.", args: &[], game: false, run: t_help },
    Tool { name: "games", help: "List the registered games.", args: &[], game: false, run: t_games },
    Tool {
        name: "load",
        help: "Start a game fresh.",
        args: &[arg("game", "string", "game name (see `games`)"), arg("seed", "integer", "random seed (default 1)")],
        game: false,
        run: t_load,
    },
    Tool {
        name: "status",
        help: "Tick, time, state hash, entity counts by kind, the player, the game's own status.",
        args: &[],
        game: true,
        run: t_status,
    },
    Tool {
        name: "step",
        help: "Advance N ticks with no input. Returns the player and the events that happened.",
        args: &[arg("ticks", "integer", "ticks to run (default 1)")],
        game: true,
        run: t_step,
    },
    Tool {
        name: "input",
        help: "Drive the player for N ticks (move/toward, hold, press, aim). Returns the player and the events.",
        args: &DRIVE,
        game: true,
        run: t_input,
    },
    Tool {
        name: "capture",
        help: "Screenshot (PNG) from the game camera. marks=true numbers the things that matter and returns a legend.",
        args: &[
            arg("out", "string", "file path (default out/<game>-<tick>.png)"),
            arg("width", "integer", "default 640"),
            arg("height", "integer", "default 360"),
            arg("marks", "string", "true = number the entities on the image and list them; or only these kinds: coin,enemy"),
            arg("ssaa", "integer", "supersampling 1-3 (smoother edges, slower; default 1)"),
            arg("at", "array", "[x, y, z] look at this point instead of the player"),
            arg("tilt", "number", "camera tilt for this shot (90 = top-down)"),
            arg("yaw", "number", "camera yaw for this shot"),
            arg("distance", "number", "camera distance for this shot"),
            arg("ortho", "boolean", "orthographic for this shot"),
        ],
        game: true,
        run: t_capture,
    },
    Tool {
        name: "filmstrip",
        help: "N frames, `every` ticks apart, tiled into one PNG (optionally driving the player): see motion cheaply.",
        args: &[
            arg("frames", "integer", "number of frames (default 8)"),
            arg("every", "integer", "ticks between frames (default 10)"),
            arg("columns", "integer", "tiles per row (default 4)"),
            arg("width", "integer", "tile width (default 320)"),
            arg("height", "integer", "tile height (default 180)"),
            arg("out", "string", "file path"),
            arg("move", "array", "[x, z] direction while recording"),
            arg("toward", "array", "[x, y, z] walk toward while recording"),
            arg("hold", "string", "buttons held while recording"),
            arg("press", "string", "buttons pressed at each frame"),
        ],
        game: true,
        run: t_filmstrip,
    },
    Tool {
        name: "ascii",
        help: "Text map around the player: walls #, raised +, floor ., lower , pits blank, entities as letters (legend).",
        args: &[
            arg("radius", "integer", "cells each way (default 12)"),
            arg("cell", "number", "metres per cell (default 1)"),
            arg("at", "array", "[x, y, z] centre (default the player)"),
            arg("plane", "string", "xz (top-down, default) or xy (side view)"),
        ],
        game: true,
        run: t_ascii,
    },
    Tool {
        name: "entities",
        help: "List entities (id, kind, pos, hp...). Filter by kind and/or distance.",
        args: &[
            arg("kind", "string", "only this kind"),
            arg("near", "array", "[x, y, z] sort by distance from here"),
            arg("radius", "number", "only within this distance of `near` (or the player)"),
            arg("limit", "integer", "at most this many (default 50)"),
            arg("blocks", "boolean", "include level blocks (default false)"),
        ],
        game: true,
        run: t_entities,
    },
    Tool {
        name: "entity",
        help: "Everything about one entity.",
        args: &[arg("id", "integer", "entity id (default the player)")],
        game: true,
        run: t_entity,
    },
    Tool {
        name: "params",
        help: "Tunable parameters with values and ranges (sim, movement, camera, env, game).",
        args: &[arg("prefix", "string", "only paths starting with this")],
        game: true,
        run: t_params,
    },
    Tool {
        name: "set",
        help: "Set a parameter, e.g. path=movement.speed value=8.",
        args: &[arg("path", "string", "parameter path"), arg("value", "string", "new value")],
        game: true,
        run: t_set,
    },
    Tool {
        name: "spawn",
        help: "Create an entity (a prop by default; character=true for a walker).",
        args: &[
            arg("kind", "string", "kind name (required)"),
            arg("pos", "array", "[x, y, z] (default 2 m in front of the player; feet for characters)"),
            arg("shape", "string", "box | sphere | capsule | cylinder"),
            arg("size", "array", "box: edge or [x,y,z]; sphere: radius; capsule/cylinder: [height, radius]"),
            arg("body", "string", "dynamic (default) | static | kinematic | trigger | none"),
            arg("color", "string", "#rrggbb"),
            arg("look", "string", "cel | lit | flat | glow"),
            arg("hp", "number", "health"),
            arg("team", "integer", "team number"),
            arg("character", "boolean", "a walking character with a puppet"),
        ],
        game: true,
        run: t_spawn,
    },
    Tool { name: "despawn", help: "Remove an entity.", args: &[arg("id", "integer", "entity id")], game: true, run: t_despawn },
    Tool {
        name: "teleport",
        help: "Move an entity (default the player; for characters pos is the feet).",
        args: &[arg("id", "integer", "entity id"), arg("pos", "array", "[x, y, z]")],
        game: true,
        run: t_teleport,
    },
    Tool {
        name: "rewind",
        help: "Go back N ticks (up to 60 s). What happened after is forgotten.",
        args: &[arg("ticks", "integer", "ticks back (default 60)")],
        game: true,
        run: t_rewind,
    },
    Tool {
        name: "snapshot",
        help: "Remember the whole state under a name (in memory).",
        args: &[arg("name", "string", "default \"a\"")],
        game: true,
        run: t_snapshot,
    },
    Tool {
        name: "restore",
        help: "Return to a named snapshot.",
        args: &[arg("name", "string", "default \"a\"")],
        game: true,
        run: t_restore,
    },
    Tool {
        name: "record",
        help: "Save every input since the game started, plus the state hash, as a replay file (JSON).",
        args: &[arg("path", "string", "default out/<game>.replay.json")],
        game: true,
        run: t_record,
    },
    Tool {
        name: "replay",
        help: "Re-run a replay file from the start and check it ends in the same state (bug reports).",
        args: &[arg("path", "string", "replay file")],
        game: false,
        run: t_replay,
    },
    Tool {
        name: "autoplay",
        help: "Let the game's bot play; returns event counts and the final status. seeds= sweeps many fresh games.",
        args: &[
            arg("seconds", "number", "how long (default 30)"),
            arg("until", "string", "stop early when this field of the game's status is true (e.g. won)"),
            arg("trace", "integer", "every N ticks, record the player's position, velocity and input"),
            arg("seeds", "string", "play fresh games with these seeds instead: 1-20, 5 (=1..5) or 3,7,9"),
        ],
        game: true,
        run: t_autoplay,
    },
    Tool {
        name: "bench",
        help: "Simulation ticks per second and render time, on a copy of the current state.",
        args: &[
            arg("ticks", "integer", "ticks to time (default 600)"),
            arg("frames", "integer", "frames to render (default 10)"),
            arg("width", "integer", "frame width (default 960)"),
            arg("height", "integer", "frame height (default 540)"),
        ],
        game: true,
        run: t_bench,
    },
    Tool {
        name: "level_check",
        help: "Validate a level file (TOML) without a game: errors with line/column, warnings, counts.",
        args: &[arg("path", "string", "level file"), arg("text", "string", "or the level text itself")],
        game: false,
        run: t_level_check,
    },
];

// ---------------------------------------------------------------------------- session

pub struct Session {
    pub games: &'static [GameDef],
    pub sim: Option<Sim>,
    snapshots: BTreeMap<String, (World, Box<dyn Game>, Replay)>,
    /// The world was changed by tools (set, spawn, teleport...), so a replay of the inputs
    /// alone won't end in the same state.
    edited: bool,
}

impl Session {
    pub fn new(games: &'static [GameDef]) -> Self {
        Session { games, sim: None, snapshots: BTreeMap::new(), edited: false }
    }

    pub fn load(&mut self, name: &str, seed: u64) -> Result<&mut Sim, String> {
        let def =
            self.games.iter().find(|g| g.name == name).ok_or_else(|| format!("no game '{name}'; games: {}", self.names()))?;
        self.sim = Some(Sim::new(def, seed));
        self.snapshots.clear();
        self.edited = false;
        Ok(self.sim.as_mut().unwrap())
    }

    fn names(&self) -> String {
        self.games.iter().map(|g| g.name).collect::<Vec<_>>().join(", ")
    }

    fn sim(&mut self) -> Result<&mut Sim, String> {
        let names = self.names();
        self.sim.as_mut().ok_or_else(|| format!("no game loaded: call `load game=NAME` first (games: {names})"))
    }

    /// Runs a tool by name.
    pub fn call(&mut self, name: &str, a: &Args) -> R {
        let tool = TOOLS.iter().find(|t| t.name == name).ok_or_else(|| format!("unknown tool '{name}' (try `help`)"))?;
        if let Some(bad) = a.keys().find(|k| !tool.args.iter().any(|x| x.name == k.as_str())) {
            let known: Vec<&str> = tool.args.iter().map(|x| x.name).collect();
            let known = if known.is_empty() { "no arguments".to_string() } else { known.join(", ") };
            return Err(format!("unknown argument '{bad}' for `{name}` (it takes: {known})"));
        }
        if tool.game {
            self.sim()?;
        }
        if ["set", "spawn", "despawn", "teleport"].contains(&name) {
            self.edited = true;
        }
        (tool.run)(self, a)
    }
}

// ---------------------------------------------------------------------------- arguments

fn arg_u64(a: &Args, k: &str, default: u64) -> Result<u64, String> {
    match a.get(k) {
        None => Ok(default),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .ok_or_else(|| format!("{k} must be a whole number, got {v}")),
    }
}

fn arg_f32(a: &Args, k: &str) -> Result<Option<f32>, String> {
    match a.get(k) {
        None => Ok(None),
        Some(v) => v
            .as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .map(|f| Some(f as f32))
            .ok_or_else(|| format!("{k} must be a number, got {v}")),
    }
}

fn arg_bool(a: &Args, k: &str) -> bool {
    match a.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => matches!(s.as_str(), "true" | "1" | "yes" | "on"),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
        _ => false,
    }
}

fn arg_str<'a>(a: &'a Args, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.as_str())
}

fn floats(a: &Args, k: &str, n: usize) -> Result<Option<Vec<f32>>, String> {
    let Some(v) = a.get(k) else { return Ok(None) };
    let parsed;
    let v = match v {
        Value::String(s) => {
            parsed = serde_json::from_str::<Value>(s).map_err(|_| format!("{k} must be a list like [1, 2]"))?;
            &parsed
        }
        v => v,
    };
    let list: Option<Vec<f32>> = v.as_array().map(|l| l.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect());
    match list {
        Some(l) if l.len() == n => Ok(Some(l)),
        _ => Err(format!("{k} must be a list of {n} numbers, got {v}")),
    }
}

fn arg_vec3(a: &Args, k: &str) -> Result<Option<Vec3>, String> {
    Ok(floats(a, k, 3)?.map(|v| Vec3::new(v[0], v[1], v[2])))
}

fn arg_vec2(a: &Args, k: &str) -> Result<Option<Vec2>, String> {
    Ok(floats(a, k, 2)?.map(|v| Vec2::new(v[0], v[1])))
}

/// The player input described by `move`, `hold`, `press`, `aim` (`toward` is per tick).
fn drive(a: &Args) -> Result<(Input, Option<Vec3>), String> {
    let mut input = Input::default();
    if let Some(m) = arg_vec2(a, "move")? {
        input.move_dir = if m.length() > 1.0 { m.normalize() } else { m };
    }
    if let Some(h) = arg_str(a, "hold") {
        input.held = buttons::parse(h)?;
    }
    if let Some(p) = arg_str(a, "press") {
        input.pressed = buttons::parse(p)?;
        input.held |= input.pressed;
    }
    input.aim = arg_vec3(a, "aim")?;
    Ok((input, arg_vec3(a, "toward")?))
}

// ---------------------------------------------------------------------------- formatting

fn r2(x: f32) -> f64 {
    (x as f64 * 100.0).round() / 100.0
}

fn v3(v: Vec3) -> Value {
    json!([r2(v.x), r2(v.y), r2(v.z)])
}

fn tag(w: &World, id: Id) -> String {
    match w.get(id) {
        Some(e) => format!("{}#{id}", e.kind),
        None => format!("#{id}"),
    }
}

/// Events as short readable lines: "t130 enter coin#5 by player#1".
fn event_line(w: &World, tick: u64, e: &Event) -> String {
    let body = match e {
        Event::Enter { trigger, other } => format!("enter {} by {}", tag(w, *trigger), tag(w, *other)),
        Event::Exit { trigger, other } => format!("exit {} by {}", tag(w, *trigger), tag(w, *other)),
        Event::Hit { target, owner, damage, .. } => {
            let by = owner.map(|o| format!(" by {}", tag(w, o))).unwrap_or_default();
            format!("hit {}{by} dmg {}", tag(w, *target), r2(*damage))
        }
        Event::Killed { id, by } => {
            let by = by.map(|o| format!(" by {}", tag(w, o))).unwrap_or_default();
            format!("killed {}{by}", tag(w, *id))
        }
        Event::Jump { id } => format!("jump {}", tag(w, *id)),
        Event::Land { id, speed } => format!("land {} {} m/s", tag(w, *id), r2(*speed)),
        Event::Fell { id } => format!("fell {}", tag(w, *id)),
    };
    format!("t{tick} {body}")
}

fn ent_json(e: &Entity, full: bool) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(e.id));
    m.insert("kind".into(), json!(e.kind));
    if e.name != e.kind {
        m.insert("name".into(), json!(e.name));
    }
    m.insert("pos".into(), v3(e.pos));
    if e.body != Body::Static {
        m.insert("body".into(), json!(e.body.name()));
    }
    if e.max_hp > 0.0 || e.hp != 0.0 {
        m.insert("hp".into(), json!(r2(e.hp)));
    }
    if e.vel.length() > 0.01 {
        m.insert("vel".into(), v3(e.vel));
    }
    if let Some(c) = &e.character {
        m.insert("grounded".into(), json!(c.grounded));
        m.insert("facing_deg".into(), json!(r2(c.facing.to_degrees())));
        if c.stun > 0.0 {
            m.insert("stun".into(), json!(r2(c.stun)));
        }
    }
    if e.team != 0 {
        m.insert("team".into(), json!(e.team));
    }
    if !e.visible {
        m.insert("visible".into(), json!(false));
    }
    if full {
        m.insert("shape".into(), serde_json::to_value(e.shape).unwrap_or(Value::Null));
        m.insert("color".into(), json!(e.color.to_hex()));
        m.insert("look".into(), json!(e.look.name()));
        let (ax, ang) = e.rot.to_axis_angle();
        if ang.abs() > 1e-3 {
            m.insert("rot_axis_deg".into(), json!([r2(ax.x), r2(ax.y), r2(ax.z), r2(ang.to_degrees())]));
        }
        if let Some(l) = e.life {
            m.insert("life".into(), json!(r2(l)));
        }
        if let Some(mv) = &e.mover {
            m.insert(
                "mover".into(),
                json!({ "from": v3(mv.from), "offset": v3(mv.offset), "period": mv.period, "hold": mv.hold }),
            );
        }
        if let Some(c) = &e.character {
            m.insert("height".into(), json!(c.height));
            m.insert("air_time".into(), json!(r2(c.air_time)));
            m.insert("input".into(), serde_json::to_value(c.input).unwrap_or(Value::Null));
        }
        if let Some(p) = &e.puppet {
            m.insert("puppet".into(), json!(p.plan.name()));
        }
    }
    Value::Object(m)
}

fn player_json(w: &World) -> Value {
    w.player().map(|e| ent_json(e, false)).unwrap_or(Value::Null)
}

fn status_json(sim: &Sim) -> Value {
    let w = &sim.world;
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for e in w.entities.values() {
        *kinds.entry(e.kind.as_str()).or_default() += 1;
    }
    json!({
        "game": sim.name,
        "tick": w.tick,
        "time": r2(w.time()),
        "hash": sim.hash(),
        "kinds": kinds,
        "player": player_json(w),
        "shots": w.shots.len(),
        "status": sim.game.status(w),
    })
}

/// Steps with `input` (re-aimed toward `toward` each tick), collecting event lines.
fn run_ticks(sim: &mut Sim, ticks: u64, input: Input, toward: Option<Vec3>) -> (Vec<String>, usize) {
    let mut lines = Vec::new();
    let mut total = 0;
    for i in 0..ticks {
        let mut inp = input;
        if i > 0 {
            inp.pressed = 0;
        }
        if let Some(t) = toward {
            let Some(p) = sim.world.player() else { break };
            let d = Vec2::new(t.x - p.pos.x, t.z - p.pos.z);
            if d.length() < 0.3 {
                break;
            }
            inp.move_dir = d.normalize();
        }
        let tick = sim.world.tick;
        sim.step(&inp);
        total += sim.world.events.len();
        for e in &sim.world.events {
            if lines.len() < 40 {
                lines.push(event_line(&sim.world, tick, e));
            }
        }
    }
    (lines, total)
}

fn stepped(sim: &Sim, lines: Vec<String>, total: usize) -> Value {
    let mut v = json!({ "tick": sim.world.tick, "player": player_json(&sim.world), "events": lines });
    if total > 40 {
        v["events_total"] = json!(total);
    }
    let st = sim.game.status(&sim.world);
    if !st.is_null() {
        v["status"] = st;
    }
    v
}

fn out_path(a: &Args, default: String) -> Result<PathBuf, String> {
    let p = PathBuf::from(arg_str(a, "out").or(arg_str(a, "path")).map(String::from).unwrap_or(default));
    if let Some(d) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| format!("cannot create {}: {e}", d.display()))?;
    }
    Ok(p)
}

fn save_png(img: &Image, path: &PathBuf) -> Result<Vec<u8>, String> {
    let png = img.png();
    std::fs::write(path, &png).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(png)
}

// ---------------------------------------------------------------------------- tools

fn t_help(_: &mut Session, _: &Args) -> R {
    Ok(Output::Json(Value::Array(
        TOOLS
            .iter()
            .map(|t| {
                let args: Vec<String> = t.args.iter().map(|a| format!("{}=<{}> {}", a.name, a.kind, a.help)).collect();
                json!({ "tool": t.name, "help": t.help, "args": args })
            })
            .collect(),
    )))
}

fn t_games(s: &mut Session, _: &Args) -> R {
    Ok(Output::Json(json!(s.games.iter().map(|g| json!({ "name": g.name, "about": g.about })).collect::<Vec<_>>())))
}

fn t_load(s: &mut Session, a: &Args) -> R {
    let name = arg_str(a, "game").map(String::from).or_else(|| s.games.first().map(|g| g.name.to_string())).unwrap_or_default();
    let seed = arg_u64(a, "seed", 1)?;
    let sim = s.load(&name, seed)?;
    Ok(Output::Json(status_json(sim)))
}

fn t_status(s: &mut Session, _: &Args) -> R {
    Ok(Output::Json(status_json(s.sim()?)))
}

fn t_step(s: &mut Session, a: &Args) -> R {
    let ticks = arg_u64(a, "ticks", 1)?;
    let sim = s.sim()?;
    let (lines, total) = run_ticks(sim, ticks, Input::default(), None);
    Ok(Output::Json(stepped(sim, lines, total)))
}

fn t_input(s: &mut Session, a: &Args) -> R {
    let ticks = arg_u64(a, "ticks", 1)?;
    let (input, toward) = drive(a)?;
    let sim = s.sim()?;
    let (lines, total) = run_ticks(sim, ticks, input, toward);
    Ok(Output::Json(stepped(sim, lines, total)))
}

/// Entities worth pointing at: everything except plain level geometry.
fn interesting(w: &World) -> Vec<&Entity> {
    w.entities.values().filter(|e| !(e.kind == "block" && e.body == Body::Static && e.hp <= 0.0)).collect()
}

fn t_capture(s: &mut Session, a: &Args) -> R {
    let width = arg_u64(a, "width", 640)?.clamp(16, 3840) as usize;
    let height = arg_u64(a, "height", 360)?.clamp(16, 2160) as usize;
    // Supersampling multiplies the pixels; keep it within ~16 million.
    let ssaa = (arg_u64(a, "ssaa", 1)? as usize).clamp(1, 3);
    let ssaa = (1..=ssaa).rev().find(|k| width * height * k * k <= 16_000_000).unwrap_or(1);
    // marks=true (everything but level blocks and `body: none` decorations) or a kind list.
    let mark_kinds: Option<Vec<String>> = match a.get("marks") {
        None | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(Vec::new()),
        Some(Value::String(s)) if matches!(s.as_str(), "false" | "0" | "no" | "") => None,
        Some(Value::String(s)) if matches!(s.as_str(), "true" | "1" | "yes") => Some(Vec::new()),
        Some(Value::String(s)) => Some(s.split(',').map(|k| k.trim().to_string()).collect()),
        Some(v) => return Err(format!("marks must be true or a list of kinds, got {v}")),
    };
    let marks = mark_kinds.is_some();
    let at = arg_vec3(a, "at")?;
    let (tilt, yaw, dist) = (arg_f32(a, "tilt")?, arg_f32(a, "yaw")?, arg_f32(a, "distance")?);
    let ortho = a.get("ortho").map(|_| arg_bool(a, "ortho"));
    let sim = s.sim()?;
    let path = out_path(a, format!("out/{}-{}.png", sim.name, sim.world.tick))?;
    let saved = sim.world.camera.clone();
    {
        let c = &mut sim.world.camera;
        if let Some(t) = tilt {
            c.tilt = t.clamp(0.0, 90.0);
        }
        if let Some(y) = yaw {
            c.yaw = y;
        }
        if let Some(d) = dist {
            c.distance = d.max(1.0);
        }
        if let Some(o) = ortho {
            c.ortho = o;
        }
    }
    let w = &sim.world;
    let target = at.unwrap_or_else(|| view::focus(w));
    let mut legend = Vec::new();
    let extra = marks.then(|| {
        let mut d = Draw::new(width as f32 / height as f32);
        let kinds = mark_kinds.unwrap_or_default();
        let mut list: Vec<&Entity> = if kinds.is_empty() {
            interesting(w).into_iter().filter(|e| e.body != Body::None).collect()
        } else {
            w.entities.values().filter(|e| kinds.contains(&e.kind)).collect()
        };
        list.sort_by(|x, y| x.pos.distance(target).total_cmp(&y.pos.distance(target)));
        for (n, e) in list.into_iter().take(40).enumerate() {
            let top = match &e.character {
                Some(c) => e.pos + Vec3::Y * (c.height + 0.35),
                None => e.pos + Vec3::Y * (e.shape.half_extents().y + 0.35),
            };
            d.label(top, &(n + 1).to_string(), "#ffe14d");
            let mut row = ent_json(e, false);
            row["n"] = json!(n + 1);
            legend.push(row);
        }
        d
    });
    let scene = view::scene(w, sim.game.as_ref(), target, width, height, extra);
    let img = crate::render::render(&scene, width, height, ssaa);
    sim.world.camera = saved;
    let png = save_png(&img, &path)?;
    let mut meta = json!({ "path": path.display().to_string(), "width": width, "height": height, "tick": sim.world.tick });
    if marks {
        meta["marks"] = Value::Array(legend);
    }
    Ok(Output::Image { png, meta })
}

fn t_filmstrip(s: &mut Session, a: &Args) -> R {
    let frames = arg_u64(a, "frames", 8)?.clamp(1, 64) as usize;
    let every = arg_u64(a, "every", 10)?.max(1);
    let cols = arg_u64(a, "columns", 4)?.clamp(1, 16) as usize;
    let (tw, th) = (arg_u64(a, "width", 320)?.clamp(32, 1280) as usize, arg_u64(a, "height", 180)?.clamp(32, 720) as usize);
    let (input, toward) = drive(a)?;
    let sim = s.sim()?;
    let path = out_path(a, format!("out/{}-strip-{}.png", sim.name, sim.world.tick))?;
    let rows = frames.div_ceil(cols);
    let mut sheet = Image::new(tw * cols.min(frames), th * rows, 0x101216);
    let mut ticks = Vec::new();
    let mut lines = Vec::new();
    for f in 0..frames {
        let mut tile = view::snapshot(&sim.world, sim.game.as_ref(), tw, th, 1, None);
        tile.text(4, th as i32 - 12, 1.0, Color::hex("#ffe14d"), &format!("t{}", sim.world.tick));
        sheet.blit(&tile, (f % cols) * tw, (f / cols) * th);
        ticks.push(sim.world.tick);
        if f + 1 < frames {
            let (l, _) = run_ticks(sim, every, input, toward);
            lines.extend(l);
        }
    }
    lines.truncate(40);
    let png = save_png(&sheet, &path)?;
    let meta = json!({ "path": path.display().to_string(), "ticks": ticks, "player": player_json(&sim.world), "events": lines });
    Ok(Output::Image { png, meta })
}

fn t_ascii(s: &mut Session, a: &Args) -> R {
    let radius = arg_u64(a, "radius", 12)?.clamp(2, 60) as i32;
    let cell = arg_f32(a, "cell")?.unwrap_or(1.0).max(0.1);
    let side = arg_str(a, "plane") == Some("xy");
    let sim = s.sim()?;
    let w = &sim.world;
    let at = match arg_vec3(a, "at")? {
        Some(p) => p,
        None => w.player().map(|p| p.pos).unwrap_or(w.camera.target),
    };
    // Snap to the cell grid so map cells line up with level cells.
    let snap = |v: f32| ((v / cell).floor() + 0.5) * cell;
    let center =
        if side { Vec3::new(snap(at.x), (at.y / cell).floor() * cell, at.z) } else { Vec3::new(snap(at.x), at.y, snap(at.z)) };
    let mut grid = vec![vec![' '; (radius * 2 + 1) as usize]; (radius * 2 + 1) as usize];
    let mut legend: BTreeMap<char, String> = BTreeMap::new();
    let mut letters: BTreeMap<String, char> = BTreeMap::new();
    legend.insert('@', "player".into());
    for (r, row) in grid.iter_mut().enumerate() {
        for (c, ch) in row.iter_mut().enumerate() {
            let (dx, dr) = ((c as i32 - radius) as f32 * cell, (r as i32 - radius) as f32 * cell);
            *ch = if side {
                let p = Vec3::new(center.x + dx, center.y + 0.5 * cell - dr, center.z);
                match w.solid_at(p, cell * 0.45) {
                    Some(id) if w.get(id).is_some_and(|e| e.mover.is_some()) => '=',
                    Some(_) => '#',
                    None => ' ',
                }
            } else {
                let (x, z) = (center.x + dx, center.z + dr);
                let from = center.y + 30.0;
                // Only the level counts as ground (not characters or props).
                let mut hit = None;
                let mut y = from;
                for _ in 0..6 {
                    match w.raycast(Vec3::new(x, y, z), Vec3::NEG_Y, y - center.y + 60.0, None) {
                        Some(h) if w.get(h.id).is_some_and(|e| e.character.is_some() || e.body == Body::Dynamic) => {
                            y = h.point.y - 0.05
                        }
                        other => {
                            hit = other;
                            break;
                        }
                    }
                }
                match hit {
                    None => ' ',
                    Some(h) => {
                        let dh = h.point.y - center.y;
                        let mover = w.get(h.id).is_some_and(|e| e.mover.is_some());
                        if mover {
                            '='
                        } else if dh > 1.0 {
                            '#'
                        } else if dh > 0.25 {
                            '+'
                        } else if dh > -0.25 {
                            '.'
                        } else if dh > -2.5 {
                            ','
                        } else {
                            ' '
                        }
                    }
                }
            };
        }
    }
    for e in interesting(w) {
        if e.body == Body::Static && e.kind == "block" {
            continue;
        }
        let (dx, dr) = if side {
            (e.pos.x - center.x, (center.y + 0.5 * cell) - e.center().y)
        } else {
            (e.pos.x - center.x, e.pos.z - center.z)
        };
        let (c, r) = ((dx / cell).round() as i32 + radius, (dr / cell).round() as i32 + radius);
        if c < 0 || r < 0 || c > radius * 2 || r > radius * 2 {
            continue;
        }
        let ch = if Some(e.id) == w.player {
            '@'
        } else {
            // One letter per kind: its first free letter (upper case for characters).
            let upper = e.character.is_some();
            let key = format!("{}{}", e.kind, upper as u8);
            match letters.get(&key) {
                Some(ch) => *ch,
                None => {
                    let pick = e
                        .kind
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric())
                        .map(|c| if upper { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() })
                        .chain("0123456789&$%*!?~".chars())
                        .find(|c| !legend.contains_key(c))
                        .unwrap_or('?');
                    letters.insert(key, pick);
                    legend.insert(pick, e.kind.clone());
                    pick
                }
            }
        };
        let slot = &mut grid[r as usize][c as usize];
        if *slot != '@' {
            *slot = ch;
        }
    }
    let map: Vec<String> = grid.into_iter().map(|r| r.into_iter().collect::<String>().trim_end().to_string()).collect();
    let key = if side {
        "# solid, = moving platform, blank = air; rows top = high"
    } else {
        "# wall (>1 m above you), + raised, . floor, , lower, blank = pit/void, = moving platform; top = north (-z)"
    };
    Ok(Output::Json(json!({
        "center": v3(center),
        "cell": cell,
        "map": map,
        "key": key,
        "letters": legend.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<_, _>>(),
    })))
}

fn t_entities(s: &mut Session, a: &Args) -> R {
    let kind = arg_str(a, "kind").map(String::from);
    let near = arg_vec3(a, "near")?;
    let radius = arg_f32(a, "radius")?;
    let limit = arg_u64(a, "limit", 50)? as usize;
    let blocks = arg_bool(a, "blocks") || kind.as_deref() == Some("block");
    let sim = s.sim()?;
    let w = &sim.world;
    let origin = near.or_else(|| radius.and(w.player().map(|p| p.pos)));
    let mut list: Vec<&Entity> = w
        .entities
        .values()
        .filter(|e| kind.as_ref().is_none_or(|k| &e.kind == k))
        .filter(|e| blocks || !(e.kind == "block" && e.body == Body::Static && e.hp <= 0.0))
        .filter(|e| match (origin, radius) {
            (Some(o), Some(r)) => e.pos.distance(o) <= r,
            _ => true,
        })
        .collect();
    if let Some(o) = origin {
        list.sort_by(|x, y| x.pos.distance(o).total_cmp(&y.pos.distance(o)));
    }
    let total = list.len();
    let rows: Vec<Value> = list.into_iter().take(limit).map(|e| ent_json(e, false)).collect();
    Ok(Output::Json(json!({ "count": total, "entities": rows })))
}

fn t_entity(s: &mut Session, a: &Args) -> R {
    let sim = s.sim()?;
    let id = match a.get("id") {
        Some(_) => arg_u64(a, "id", 0)? as Id,
        None => sim.world.player.ok_or("no player; pass id=")?,
    };
    let e = sim.world.get(id).ok_or_else(|| format!("no entity {id}"))?;
    Ok(Output::Json(ent_json(e, true)))
}

fn t_params(s: &mut Session, a: &Args) -> R {
    let prefix = arg_str(a, "prefix").unwrap_or("").to_string();
    let sim = s.sim()?;
    let lines: Vec<String> = params::list(sim)
        .into_iter()
        .filter(|p| p.path.starts_with(&prefix))
        .map(|p| {
            let v = match &p.value {
                ParamValue::Float(f) => format!("{f}"),
                ParamValue::Bool(b) => b.to_string(),
                ParamValue::Text(t) => t.clone(),
            };
            format!("{} = {v}  ({}) {}", p.path, p.kind, p.help)
        })
        .collect();
    Ok(Output::Json(json!(lines)))
}

fn t_set(s: &mut Session, a: &Args) -> R {
    let path = arg_str(a, "path").ok_or("path= is required")?.to_string();
    let value = a.get("value").ok_or("value= is required")?;
    let sim = s.sim()?;
    params::set(sim, &path, &ParamValue::from_json(value))?;
    Ok(Output::Json(json!({ "path": path, "value": params::get(sim, &path) })))
}

fn t_spawn(s: &mut Session, a: &Args) -> R {
    let kind = arg_str(a, "kind").ok_or("kind= is required")?.to_string();
    let body: Body = match arg_str(a, "body") {
        Some(b) => serde_json::from_value(json!(b)).map_err(|_| format!("body must be one of {}", Body::NAMES.join("|")))?,
        None => Body::Dynamic,
    };
    let look: Look = match arg_str(a, "look") {
        Some(l) => serde_json::from_value(json!(l)).map_err(|_| format!("look must be one of {}", Look::NAMES.join("|")))?,
        None => Look::Cel,
    };
    let size = match a.get("size") {
        None => None,
        Some(Value::Array(v)) => Some(Size::Many(v.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect())),
        Some(v) => Some(Size::One(
            v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).ok_or("size must be a number or a list")? as f32,
        )),
    };
    let def = SpawnDef {
        kind: kind.clone(),
        shape: arg_str(a, "shape").unwrap_or("box").to_string(),
        size,
        body,
        color: Color::hex(arg_str(a, "color").unwrap_or("#b0b0b0")),
        look,
        y: None,
        hidden: false,
        rot: None,
        hp: arg_f32(a, "hp")?.unwrap_or(0.0),
        team: arg_u64(a, "team", 0)? as u8,
        spin: 0.0,
        move_by: None,
        period: 3.0,
    };
    let shape = def.shape()?;
    let character = arg_bool(a, "character");
    let sim = s.sim()?;
    let w = &mut sim.world;
    let pos = match arg_vec3(a, "pos")? {
        Some(p) => p,
        None => {
            let p = w.player().ok_or("pos= is required when there is no player")?;
            let fwd = p.character.as_ref().map(|c| c.forward()).unwrap_or(Vec3::Z);
            p.pos + fwd * 2.0 + Vec3::Y * if character { 0.0 } else { shape.half_extents().y + 0.5 }
        }
    };
    let sp = if character {
        let mut sp = Spawn::character(&kind, pos).team(def.team).hp(def.hp);
        if let Some(c) = arg_str(a, "color") {
            sp = sp.puppet(Puppet::biped(c));
        }
        sp
    } else {
        let mut sp = Spawn::new(&kind, pos).shape(shape).body(body).look(look).hp(def.hp).team(def.team);
        sp.color = def.color;
        sp
    };
    let id = w.spawn(sp);
    Ok(Output::Json(ent_json(w.get(id).unwrap(), false)))
}

fn t_despawn(s: &mut Session, a: &Args) -> R {
    let id = arg_u64(a, "id", 0)? as Id;
    let ok = s.sim()?.world.despawn(id);
    Ok(Output::Json(json!({ "despawned": ok })))
}

fn t_teleport(s: &mut Session, a: &Args) -> R {
    let pos = arg_vec3(a, "pos")?.ok_or("pos=[x,y,z] is required")?;
    let sim = s.sim()?;
    let id = match a.get("id") {
        Some(_) => arg_u64(a, "id", 0)? as Id,
        None => sim.world.player.ok_or("no player; pass id=")?,
    };
    if sim.world.get(id).is_none() {
        return Err(format!("no entity {id}"));
    }
    sim.world.set_pos(id, pos);
    Ok(Output::Json(ent_json(sim.world.get(id).unwrap(), false)))
}

fn t_rewind(s: &mut Session, a: &Args) -> R {
    let ticks = arg_u64(a, "ticks", 60)?;
    let sim = s.sim()?;
    let before = sim.world.tick;
    let tick = sim.rewind(ticks);
    let mut v = json!({ "tick": tick, "rewound": before - tick, "player": player_json(&sim.world) });
    if before - tick < ticks {
        v["note"] = json!(format!("history reaches back to tick {} only (it restarts at load and restore)", sim.oldest_tick()));
    }
    Ok(Output::Json(v))
}

fn t_snapshot(s: &mut Session, a: &Args) -> R {
    let name = arg_str(a, "name").unwrap_or("a").to_string();
    let sim = s.sim()?;
    let snap = (sim.world.clone(), sim.game.clone(), sim.recording.clone());
    let tick = sim.world.tick;
    s.snapshots.insert(name.clone(), snap);
    Ok(Output::Json(json!({ "saved": name, "tick": tick })))
}

fn t_restore(s: &mut Session, a: &Args) -> R {
    let name = arg_str(a, "name").unwrap_or("a").to_string();
    let (w, g, r) = s.snapshots.get(&name).cloned().ok_or_else(|| format!("no snapshot '{name}'"))?;
    let sim = s.sim()?;
    sim.restore(w, g, r);
    Ok(Output::Json(json!({ "restored": name, "tick": sim.world.tick, "player": player_json(&sim.world) })))
}

fn t_record(s: &mut Session, a: &Args) -> R {
    let sim = s.sim()?;
    let path = out_path(a, format!("out/{}.replay.json", sim.name))?;
    let r = sim.replay();
    let text = serde_json::to_string(&r).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let mut v = json!({ "path": path.display().to_string(), "ticks": r.ticks(), "hash": r.hash });
    if s.edited {
        v["warning"] = json!(
            "tools changed the world in this session (set/spawn/despawn/teleport); a replay holds only inputs, so it won't match"
        );
    }
    Ok(Output::Json(v))
}

fn t_replay(s: &mut Session, a: &Args) -> R {
    let path = arg_str(a, "path").ok_or("path= is required")?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let r: Replay = serde_json::from_str(&text).map_err(|e| format!("not a replay file: {e}"))?;
    let sim = s.load(&r.game, r.seed)?;
    for input in r.iter() {
        sim.step(input);
    }
    let hash = sim.hash();
    Ok(Output::Json(json!({
        "game": r.game,
        "ticks": r.ticks(),
        "hash": hash,
        "expected": r.hash,
        "match": r.hash.as_deref().is_none_or(|h| h == hash),
        "status": status_json(sim),
    })))
}

/// Runs the game's bot for up to `ticks`, stopping early when `until` (a field of the game's
/// status) becomes truthy. Returns ticks run, event counts and an optional trace.
fn play(
    sim: &mut Sim,
    ticks: u64,
    until: Option<&str>,
    trace_every: u64,
) -> Result<(u64, BTreeMap<String, usize>, Vec<Value>), String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut trace = Vec::new();
    let truthy = |v: &Value| match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Null => false,
        _ => true,
    };
    let mut ran = 0;
    for _ in 0..ticks {
        // None on the first tick means there is no bot; later it means "do nothing".
        let input = match sim.game.bot(&sim.world) {
            Some(i) => i,
            None if ran == 0 => return Err("this game has no bot (implement Game::bot)".into()),
            None => Input::default(),
        };
        if trace_every > 0 && sim.world.tick.is_multiple_of(trace_every) && trace.len() < 120 {
            let p = sim.world.player();
            trace.push(json!({
                "t": sim.world.tick,
                "pos": p.map(|p| v3(p.pos)),
                "vel": p.map(|p| v3(p.vel)),
                "move": [r2(input.move_dir.x), r2(input.move_dir.y)],
                "buttons": buttons::names(input.held | input.pressed),
            }));
        }
        sim.step(&input);
        ran += 1;
        for e in &sim.world.events {
            let w = &sim.world;
            let kind = |id: Id| w.get(id).map(|e| e.kind.clone()).unwrap_or_default();
            let key = match e {
                Event::Enter { trigger, other } => format!("enter {} by {}", kind(*trigger), kind(*other)),
                Event::Exit { .. } | Event::Jump { .. } | Event::Land { .. } => continue,
                Event::Hit { target, .. } => format!("hit {}", kind(*target)),
                Event::Killed { id, .. } => format!("killed {}", kind(*id)),
                Event::Fell { id } => format!("fell {}", kind(*id)),
            };
            *counts.entry(key).or_default() += 1;
        }
        if let Some(field) = until {
            if sim.game.status(&sim.world).get(field).is_some_and(truthy) {
                break;
            }
        }
    }
    Ok((ran, counts, trace))
}

/// "1-20", "5" (seeds 1..=5), "3,7,9" or a JSON list.
fn parse_seeds(v: &Value) -> Result<Vec<u64>, String> {
    let bad = || format!("seeds must look like 1-20, 5 or 3,7,9; got {v}");
    let list: Vec<u64> = match v {
        Value::Number(n) => (1..=n.as_u64().ok_or_else(bad)?).collect(),
        Value::Array(a) => a.iter().map(|x| x.as_u64().ok_or_else(bad)).collect::<Result<_, _>>()?,
        Value::String(s) => match s.split_once('-') {
            Some((a, b)) => (a.trim().parse::<u64>().map_err(|_| bad())?..=b.trim().parse::<u64>().map_err(|_| bad())?).collect(),
            None if s.contains(',') => {
                s.split(',').map(|x| x.trim().parse::<u64>().map_err(|_| bad())).collect::<Result<_, _>>()?
            }
            None => (1..=s.trim().parse::<u64>().map_err(|_| bad())?).collect(),
        },
        _ => return Err(bad()),
    };
    if list.is_empty() || list.len() > 500 {
        return Err("give between 1 and 500 seeds".into());
    }
    Ok(list)
}

fn t_autoplay(s: &mut Session, a: &Args) -> R {
    let seconds = arg_f32(a, "seconds")?.unwrap_or(30.0).clamp(0.0, 3600.0);
    let ticks = (seconds * 60.0) as u64;
    let until = arg_str(a, "until").map(String::from);
    let trace_every = arg_u64(a, "trace", 0)?;
    if let Some(spec) = a.get("seeds") {
        // A sweep: every seed from a fresh start; the session's own game is left alone.
        let seeds = parse_seeds(spec)?;
        let name = s.sim()?.name.clone();
        let def = s.games.iter().find(|g| g.name == name).ok_or("game not registered")?;
        let start = Instant::now();
        let mut runs = Vec::new();
        for seed in seeds {
            let mut sim = Sim::new(def, seed);
            let (ran, _, _) = play(&mut sim, ticks, until.as_deref(), 0)?;
            runs.push(json!({ "seed": seed, "ticks": ran, "status": sim.game.status(&sim.world) }));
        }
        return Ok(Output::Json(json!({ "game": name, "wall_ms": start.elapsed().as_millis() as u64, "runs": runs })));
    }
    let sim = s.sim()?;
    let start = Instant::now();
    let (ran, counts, trace) = play(sim, ticks, until.as_deref(), trace_every)?;
    let mut v = json!({
        "ticks": ran,
        "wall_ms": start.elapsed().as_millis() as u64,
        "events": counts,
        "final": status_json(sim),
    });
    if trace_every > 0 {
        v["trace"] = Value::Array(trace);
    }
    Ok(Output::Json(v))
}

fn t_bench(s: &mut Session, a: &Args) -> R {
    let ticks = arg_u64(a, "ticks", 600)?;
    let frames = arg_u64(a, "frames", 10)?;
    let (w, h) = (arg_u64(a, "width", 960)? as usize, arg_u64(a, "height", 540)? as usize);
    let sim = s.sim()?;
    let mut fork = sim.fork();
    let t = Instant::now();
    for _ in 0..ticks {
        let input = fork.game.bot(&fork.world).unwrap_or_default();
        fork.step(&input);
    }
    let sim_s = t.elapsed().as_secs_f64();
    let t = Instant::now();
    for _ in 0..frames {
        let _ = view::snapshot(&fork.world, fork.game.as_ref(), w.max(16), h.max(16), 1, None);
    }
    let render_ms = if frames > 0 { t.elapsed().as_secs_f64() * 1000.0 / frames as f64 } else { 0.0 };
    Ok(Output::Json(json!({
        "ticks": ticks,
        "ticks_per_sec": (ticks as f64 / sim_s.max(1e-9)).round(),
        "realtime_x": r2((ticks as f64 / 60.0 / sim_s.max(1e-9)) as f32),
        "render_ms": r2(render_ms as f32),
        "render_size": [w, h],
        "entities": fork.world.entities.len(),
    })))
}

fn t_level_check(_: &mut Session, a: &Args) -> R {
    let text = match (arg_str(a, "text"), arg_str(a, "path")) {
        (Some(t), _) => t.to_string(),
        (None, Some(p)) => std::fs::read_to_string(p).map_err(|e| format!("cannot read {p}: {e}"))?,
        _ => return Err("path= or text= is required".into()),
    };
    let lf = level::parse(&text)?;
    let mut w = World::new(1);
    let info = w.build_level(&lf)?;
    Ok(Output::Json(serde_json::to_value(info).map_err(|e| e.to_string())?))
}

// ---------------------------------------------------------------------------- front ends

/// Parses `key=value` words; values are JSON when they parse as JSON, else strings.
pub fn parse_args(words: &[String]) -> Args {
    let mut a = Args::new();
    for w in words {
        if let Some((k, v)) = w.split_once('=') {
            let val = serde_json::from_str::<Value>(v).unwrap_or_else(|_| Value::String(v.to_string()));
            a.insert(k.trim_start_matches("--").to_string(), val);
        }
    }
    a
}

/// Splits a REPL line into words, keeping [ ... ] and quoted values together.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut depth, mut quote) = (0i32, false);
    for c in line.chars() {
        match c {
            '"' => {
                quote = !quote;
                cur.push(c);
            }
            '[' | '{' if !quote => {
                depth += 1;
                cur.push(c);
            }
            ']' | '}' if !quote => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && depth <= 0 && !quote => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter()
        .map(|w| {
            let quoted = w.len() >= 2 && w.starts_with('"') && w.ends_with('"') && !w[1..w.len() - 1].contains('"');
            if quoted { w[1..w.len() - 1].to_string() } else { w }
        })
        .map(|w| match w.split_once('=') {
            // key="quoted text" -> key=quoted text
            Some((k, v)) if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') && !v[1..v.len() - 1].contains('"') => {
                format!("{k}={}", &v[1..v.len() - 1])
            }
            _ => w,
        })
        .collect()
}

fn print(out: Result<Output, String>) -> bool {
    match out {
        Ok(Output::Json(v)) => {
            println!("{v}");
            true
        }
        Ok(Output::Image { meta, .. }) => {
            println!("{meta}");
            true
        }
        Err(e) => {
            println!("{}", json!({ "error": e }));
            false
        }
    }
}

fn help_text(games: &[GameDef]) -> String {
    let mut s = String::from(
        "pav: Pavilion Lite engine tools\n\n\
         usage: pav <tool> [key=value ...]   one shot (game=NAME seed=N ticks=N set it up first)\n       \
         pav repl [game=NAME]          a tool per line on stdin, JSON out\n       \
         pav mcp [game=NAME]           MCP server on stdio\n       \
         pav play <game> [seed=N] [scale=N] [size=WxH]   play in a window\n\ntools:\n",
    );
    for t in TOOLS {
        let args: Vec<String> = t.args.iter().map(|a| format!("{}=", a.name)).collect();
        s.push_str(&format!("  {:<12} {}  {}\n", t.name, t.help, args.join(" ")));
    }
    s.push_str("\ngames:\n");
    for g in games {
        s.push_str(&format!("  {:<12} {}\n", g.name, g.about));
    }
    s
}

/// The `pav` binary.
pub fn main(games: &'static [GameDef]) {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first().cloned() else {
        print!("{}", help_text(games));
        return;
    };
    let mut session = Session::new(games);
    let rest = parse_args(&argv[1..]);
    let preload = |s: &mut Session, a: &Args| -> Result<(), String> {
        if let Some(g) = arg_str(a, "game") {
            s.load(g, arg_u64(a, "seed", 1)?)?;
        }
        Ok(())
    };
    match cmd.as_str() {
        "help" | "--help" | "-h" => print!("{}", help_text(games)),
        "play" => {
            #[cfg(feature = "window")]
            {
                let name = argv.get(1).filter(|a| !a.contains('=')).cloned().unwrap_or_else(|| games[0].name.into());
                let Some(def) = games.iter().find(|g| g.name == name) else {
                    eprintln!("no game '{name}'");
                    std::process::exit(1);
                };
                if let Err(e) = crate::window::play(def, &rest) {
                    eprintln!("window error: {e}");
                    std::process::exit(1);
                }
            }
            #[cfg(not(feature = "window"))]
            {
                eprintln!("built without the window feature (rebuild with default features)");
                std::process::exit(1);
            }
        }
        "repl" => {
            if let Err(e) = preload(&mut session, &rest) {
                print(Err(e));
            }
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                let w = words(&line);
                let Some(name) = w.first() else { continue };
                if name.starts_with('#') {
                    continue;
                }
                if name == "quit" || name == "exit" {
                    break;
                }
                print(session.call(name, &parse_args(&w[1..])));
                let _ = std::io::stdout().flush();
            }
        }
        "mcp" => {
            if let Err(e) = preload(&mut session, &rest) {
                eprintln!("{e}");
            }
            if let Err(e) = serve_mcp(&mut session) {
                eprintln!("mcp: {e}");
            }
        }
        name => {
            let tool = TOOLS.iter().find(|t| t.name == name);
            let mut ok = true;
            if tool.is_some_and(|t| t.game) {
                let g = arg_str(&rest, "game").map(String::from).unwrap_or_else(|| games[0].name.to_string());
                match session.load(&g, arg_u64(&rest, "seed", 1).unwrap_or(1)) {
                    Ok(sim) => {
                        let own_ticks = ["step", "input", "bench", "rewind", "filmstrip"].contains(&name);
                        if !own_ticks {
                            let n = arg_u64(&rest, "ticks", 0).unwrap_or(0);
                            sim.run(n, &Input::default());
                        }
                    }
                    Err(e) => ok = print(Err(e)),
                }
            }
            if ok {
                // game=, seed= and ticks= set the session up; the tool only sees its own arguments.
                let mut args = rest.clone();
                for k in ["game", "seed", "ticks"] {
                    if !tool.is_some_and(|t| t.args.iter().any(|x| x.name == k)) {
                        args.remove(k);
                    }
                }
                ok = print(session.call(name, &args));
            }
            if !ok {
                std::process::exit(1);
            }
        }
    }
}

// ---------------------------------------------------------------------------- MCP

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn schema(t: &Tool) -> Value {
    let props: Map<String, Value> = t
        .args
        .iter()
        .map(|a| {
            let mut p = json!({ "description": a.help });
            match a.kind {
                "array" => p["type"] = json!("array"),
                "string" => p["type"] = json!("string"),
                k => p["type"] = json!([k, "string"]),
            }
            if a.kind == "array" {
                p["items"] = json!({ "type": "number" });
            }
            (a.name.to_string(), p)
        })
        .collect();
    let required: Vec<&str> = match t.name {
        "set" => vec!["path", "value"],
        "spawn" => vec!["kind"],
        "replay" => vec!["path"],
        "despawn" => vec!["id"],
        _ => vec![],
    };
    json!({ "type": "object", "properties": props, "required": required })
}

/// MCP (JSON-RPC 2.0, newline-delimited on stdio) until stdin closes.
pub fn serve_mcp(session: &mut Session) -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}}))?;
                out.flush()?;
                continue;
            }
        };
        let Some(id) = msg.get("id").cloned() else { continue };
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("2025-06-18"),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "pavilion-lite", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Pavilion Lite game engine. `games` lists games, `load game=NAME` starts one, `input` drives the player, `capture` (marks=true) shows it, `status`/`entities`/`ascii` describe it, `params`/`set` tune it.",
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": TOOLS.iter().map(|t| json!({
                "name": t.name, "description": t.help, "inputSchema": schema(t),
            })).collect::<Vec<_>>() })),
            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").and_then(|v| v.as_object()).cloned().unwrap_or_default();
                Ok(match session.call(name, &args) {
                    Ok(Output::Json(v)) => json!({ "content": [{ "type": "text", "text": v.to_string() }] }),
                    Ok(Output::Image { png, meta }) => json!({ "content": [
                        { "type": "image", "data": base64(&png), "mimeType": "image/png" },
                        { "type": "text", "text": meta.to_string() },
                    ] }),
                    Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                })
            }
            _ => Err(json!({ "code": -32601, "message": format!("unknown method {method}") })),
        };
        let reply = match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
        };
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn repl_words_keep_lists_and_quotes() {
        let w = words(r#"input move=[1, 0] hold=jump text="a b" ticks=3"#);
        assert_eq!(w, vec!["input", "move=[1, 0]", "hold=jump", "text=a b", "ticks=3"]);
        let a = parse_args(&w[1..]);
        assert_eq!(a["move"], json!([1, 0]));
        assert_eq!(a["ticks"], json!(3));
    }
}
