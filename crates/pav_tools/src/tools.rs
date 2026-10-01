//! The tool registry. Add a tool: write a `fn(&mut Session, &Args) -> Result<Output>` and list
//! it in `TOOLS`. It is then available from the CLI, the REPL and MCP.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use pav_core::params::{self, ParamValue};
use serde_json::{Map, Value, json};

use crate::session::Session;

pub type Args = Map<String, Value>;

pub enum Output {
    Json(Value),
    /// A PNG image plus metadata (also saved to `path`).
    Image {
        png: Vec<u8>,
        path: Option<PathBuf>,
        meta: Value,
    },
}

pub struct Arg {
    pub name: &'static str,
    /// JSON schema type: string, number, integer, boolean.
    pub kind: &'static str,
    pub help: &'static str,
}

pub struct Tool {
    pub name: &'static str,
    pub help: &'static str,
    pub args: &'static [Arg],
    pub run: fn(&mut Session, &Args) -> Result<Output>,
}

const fn arg(name: &'static str, kind: &'static str, help: &'static str) -> Arg {
    Arg { name, kind, help }
}

pub static TOOLS: &[Tool] = &[
    Tool { name: "scenes", help: "List built-in scenes.", args: &[], run: t_scenes },
    Tool {
        name: "load",
        help: "Start a fresh simulation of a scene.",
        args: &[arg("scene", "string", "scene name (see `scenes`)"), arg("seed", "integer", "world seed (default 1)")],
        run: t_load,
    },
    Tool {
        name: "step",
        help: "Advance the simulation N ticks as fast as possible.",
        args: &[arg("ticks", "integer", "number of ticks (default 1)")],
        run: t_step,
    },
    Tool { name: "status", help: "Tick, time, entity/block counts and state hash.", args: &[], run: t_status },
    Tool {
        name: "entities",
        help: "List entities (id, name, position).",
        args: &[arg("name", "string", "only entities whose name contains this")],
        run: t_entities,
    },
    Tool {
        name: "params",
        help: "List tunable parameters (sim.*, camera.*, view.*) with values and ranges.",
        args: &[arg("prefix", "string", "only paths starting with this")],
        run: t_params,
    },
    Tool {
        name: "set",
        help: "Set a parameter: path=value.",
        args: &[arg("path", "string", "parameter path, e.g. camera.tilt"), arg("value", "string", "new value")],
        run: t_set,
    },
    Tool {
        name: "camera",
        help: "Apply a camera preset (no args lists them).",
        args: &[arg("preset", "string", "preset name")],
        run: t_camera,
    },
    Tool {
        name: "capture",
        help: "Render a screenshot (PNG) with the software/real GPU.",
        args: &[
            arg("out", "string", "output path (default out/capture-<tick>.png)"),
            arg("width", "integer", "default 960"),
            arg("height", "integer", "default 540"),
        ],
        run: t_capture,
    },
    Tool {
        name: "bench",
        help: "Measure simulation ticks per second.",
        args: &[arg("ticks", "integer", "ticks to run (default 600)")],
        run: t_bench,
    },
    Tool { name: "gpu", help: "Describe the GPU adapter used for captures.", args: &[], run: t_gpu },
    Tool {
        name: "player",
        help: "Player state: feet position, velocity, grounded, posture, climbing.",
        args: &[],
        run: t_player,
    },
    Tool {
        name: "input",
        help: "Drive the player: hold a move direction/buttons for N ticks (press = only on the first tick).",
        args: &[
            arg("move", "array", "[x, z] world direction, e.g. [1, 0] = east, [0, -1] = north"),
            arg("hold", "string", "held buttons, comma separated: jump,crouch,crawl,use,focus,interact,sprint,primary"),
            arg("press", "string", "buttons pressed on the first tick"),
            arg("aim", "array", "[x, y, z] aim point (bomb throws)"),
            arg("ticks", "integer", "ticks to run (default 1)"),
        ],
        run: t_input,
    },
    Tool {
        name: "spawn",
        help: "Spawn a prop.",
        args: &[
            arg("shape", "string", "box | sphere | capsule | cylinder | rounded_box"),
            arg("size", "array", "box: [hx,hy,hz] half extents; sphere: [r]; capsule/cylinder: [half_height, r]"),
            arg("pos", "array", "[x, y, z]"),
            arg("color", "string", "#rrggbb"),
            arg("body", "string", "dynamic (default) | fixed | kinematic | none"),
            arg("name", "string", "entity name"),
        ],
        run: t_spawn,
    },
    Tool { name: "despawn", help: "Remove an entity.", args: &[arg("id", "integer", "entity id")], run: t_despawn },
    Tool {
        name: "teleport",
        help: "Move an entity (default: the player; for characters pos = feet).",
        args: &[arg("id", "integer", "entity id (default player)"), arg("pos", "array", "[x, y, z]")],
        run: t_teleport,
    },
    Tool {
        name: "rewind",
        help: "Go back in time: ticks=N back, or to tick=T. Starts a new timeline.",
        args: &[arg("ticks", "integer", "ticks back"), arg("tick", "integer", "absolute tick")],
        run: t_rewind,
    },
    Tool {
        name: "snapshot_save",
        help: "Save the full state to a file.",
        args: &[arg("path", "string", "file path")],
        run: t_snap_save,
    },
    Tool { name: "snapshot_load", help: "Load a state file.", args: &[arg("path", "string", "file path")], run: t_snap_load },
    Tool {
        name: "record_save",
        help: "Save the inputs since the scene started as a replay file (JSON).",
        args: &[arg("path", "string", "file path")],
        run: t_record_save,
    },
    Tool { name: "rooms", help: "List rooms in the world (key, name, wing, bounds, entrance).", args: &[], run: t_rooms },
    Tool {
        name: "room",
        help: "Info card of a room (default: the one the player is in).",
        args: &[arg("key", "string", "room key")],
        run: t_room,
    },
    Tool {
        name: "goto",
        help: "Teleport the player into a room (room=key), to the plaza (room=hub) or to a position.",
        args: &[arg("room", "string", "room key or 'hub'"), arg("pos", "array", "[x, y, z] feet position")],
        run: t_goto,
    },
    Tool {
        name: "room_reset",
        help: "Rebuild a room from its definition (default: current room).",
        args: &[arg("key", "string", "room key")],
        run: t_room_reset,
    },
    Tool {
        name: "room_check",
        help: "Validate a room file without loading it. Returns errors with line/column.",
        args: &[arg("path", "string", "path to a .toml room file")],
        run: t_room_check,
    },
    Tool {
        name: "room_reload",
        help: "Re-read room files from the rooms directory and rebuild changed rooms (hot reload).",
        args: &[],
        run: t_room_reload,
    },
    Tool {
        name: "stream",
        help: "Streaming state: active/dormant regions. point=[x,y,z] adds an interest point, clear=true removes them.",
        args: &[arg("point", "array", "[x, y, z] extra interest point"), arg("clear", "boolean", "remove extra points")],
        run: t_stream,
    },
    Tool {
        name: "filmstrip",
        help: "Capture N frames, `every` ticks apart (optionally driving the player), tiled into one PNG.",
        args: &[
            arg("frames", "integer", "number of frames (default 8)"),
            arg("every", "integer", "ticks between frames (default 10)"),
            arg("columns", "integer", "tiles per row (default 4)"),
            arg("width", "integer", "frame width (default 320)"),
            arg("height", "integer", "frame height (default 180)"),
            arg("move", "array", "[x, z] move direction while recording"),
            arg("hold", "string", "held buttons while recording"),
            arg("press", "string", "buttons pressed at the start"),
            arg("out", "string", "output path"),
        ],
        run: t_filmstrip,
    },
    Tool {
        name: "camera_bench",
        help: "Render the current moment from several camera setups, tiled into one PNG.",
        args: &[
            arg("presets", "string", "comma-separated preset names or tilt angles (default: all presets)"),
            arg("columns", "integer", "tiles per row (default 4)"),
            arg("width", "integer", "tile width (default 320)"),
            arg("height", "integer", "tile height (default 180)"),
            arg("out", "string", "output path"),
        ],
        run: t_camera_bench,
    },
    Tool {
        name: "npcs",
        help: "Non-player characters and creatures: body plan, feet, velocity, brain, steps taken, flinch.",
        args: &[arg("name", "string", "only names starting with this")],
        run: t_npcs,
    },
    Tool {
        name: "signal",
        help: "Send a pad signal (spawners listening for it fire on the next tick), then step 1 tick.",
        args: &[arg("name", "string", "signal name, e.g. drop or clear")],
        run: t_signal,
    },
    Tool {
        name: "course",
        help: "Course state: current run (timer, gates, hits, falls), last result, best times, checkpoint.",
        args: &[],
        run: t_course,
    },
    Tool {
        name: "feel",
        help: "Feel metrics of the player: response ticks, time to top speed, stopping, turnaround, last jump.",
        args: &[],
        run: t_feel,
    },
    Tool {
        name: "audio_capture",
        help: "Run N ticks (optionally driving the player) and render the game's sounds to a .wav file.",
        args: &[
            arg("ticks", "integer", "ticks to run (default 180)"),
            arg("move", "array", "[x, z] move direction"),
            arg("hold", "string", "held buttons"),
            arg("press", "string", "buttons pressed on the first tick"),
            arg("aim", "array", "[x, y, z] aim point"),
            arg("out", "string", "output .wav path"),
        ],
        run: t_audio_capture,
    },
    Tool {
        name: "replay",
        help: "Rebuild the scene from a replay file, run all its inputs and verify the final state hash.",
        args: &[arg("path", "string", "file path")],
        run: t_replay,
    },
];

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

pub fn call(session: &mut Session, name: &str, args: &Args) -> Result<Output> {
    let tool = find(name).ok_or_else(|| anyhow!("unknown tool '{name}' (try `help`)"))?;
    (tool.run)(session, args)
}

/// JSON schema for a tool's arguments (for MCP).
pub fn schema(t: &Tool) -> Value {
    let mut props = Map::new();
    for a in t.args {
        props.insert(a.name.into(), json!({ "type": a.kind, "description": a.help }));
    }
    json!({ "type": "object", "properties": props })
}

fn get_u64(a: &Args, k: &str, default: u64) -> Result<u64> {
    match a.get(k) {
        None => Ok(default),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_f64().map(|f| f as u64))
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .ok_or_else(|| anyhow!("argument '{k}' must be a non-negative integer")),
    }
}

fn get_str<'a>(a: &'a Args, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.as_str())
}

fn t_scenes(_: &mut Session, _: &Args) -> Result<Output> {
    Ok(Output::Json(json!({
        "scenes": pav_core::scenes::SCENES.iter().map(|(n, d)| json!({"name": n, "about": d})).collect::<Vec<_>>(),
        "standalone_rooms": pav_core::scenes::names().into_iter().filter(|n| !pav_core::scenes::SCENES.iter().any(|s| s.0 == n)).collect::<Vec<_>>(),
    })))
}

fn t_load(s: &mut Session, a: &Args) -> Result<Output> {
    let scene = get_str(a, "scene").unwrap_or("test");
    let seed = get_u64(a, "seed", 1)?;
    let gpu = s.gpu.take();
    // Keep the caller's camera and view settings, minus any room's own view table.
    let (camera, view) = (s.camera_base_or_current(), s.view_base_or_current());
    *s = Session::new(scene, seed)?;
    s.gpu = gpu;
    s.reset_view(view);
    s.reset_camera(camera);
    t_status(s, a)
}

fn t_step(s: &mut Session, a: &Args) -> Result<Output> {
    let n = get_u64(a, "ticks", 1)?;
    let t = Instant::now();
    s.step(n);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let mut st = status(s);
    st["elapsed_ms"] = json!((ms * 100.0).round() / 100.0);
    Ok(Output::Json(st))
}

fn status(s: &Session) -> Value {
    json!({
        "scene": s.sim.state.scene,
        "tick": s.sim.state.tick,
        "time": (s.sim.time() * 1000.0).round() / 1000.0,
        "tick_rate": s.sim.config.tick_rate.hz(),
        "entities": s.sim.state.entities.len(),
        "blocks": s.sim.state.statics.block_count(),
        "projectiles": s.sim.state.projectiles.list.len(),
        "hash": format!("{:016x}", s.sim.state_hash()),
    })
}

fn t_status(s: &mut Session, _: &Args) -> Result<Output> {
    Ok(Output::Json(status(s)))
}

fn t_entities(s: &mut Session, a: &Args) -> Result<Output> {
    let filter = get_str(a, "name").unwrap_or("");
    let list: Vec<Value> = s
        .sim
        .state
        .entities
        .iter()
        .filter(|e| e.name.contains(filter))
        .map(|e| {
            let p = e.pos;
            json!({"id": e.id.0, "name": e.name, "pos": [round3(p.x), round3(p.y), round3(p.z)], "body": e.body_kind})
        })
        .collect();
    Ok(Output::Json(json!(list)))
}

fn round3(x: f32) -> f64 {
    (x as f64 * 1000.0).round() / 1000.0
}

fn t_params(s: &mut Session, a: &Args) -> Result<Output> {
    let prefix = get_str(a, "prefix").unwrap_or("").to_string();
    let list: Vec<_> = params::list(&mut s.params()).into_iter().filter(|p| p.path.starts_with(&prefix)).collect();
    Ok(Output::Json(serde_json::to_value(list)?))
}

/// Parses CLI text into the most natural JSON value.
pub fn parse_value(v: &Value) -> ParamValue {
    match v {
        Value::Bool(b) => ParamValue::Bool(*b),
        Value::Number(n) => ParamValue::Float(n.as_f64().unwrap_or(0.0)),
        Value::String(t) => match serde_json::from_str::<Value>(t) {
            Ok(Value::Bool(b)) => ParamValue::Bool(b),
            Ok(Value::Number(n)) => ParamValue::Float(n.as_f64().unwrap_or(0.0)),
            _ => ParamValue::Text(t.clone()),
        },
        other => ParamValue::Text(other.to_string()),
    }
}

fn t_set(s: &mut Session, a: &Args) -> Result<Output> {
    let path = get_str(a, "path").context("missing 'path'")?.to_string();
    let value = a.get("value").context("missing 'value'")?;
    params::set(&mut s.params(), &path, parse_value(value)).map_err(|e| anyhow!(e))?;
    let now = params::get(&mut s.params(), &path);
    Ok(Output::Json(json!({ "path": path, "value": now })))
}

fn t_camera(s: &mut Session, a: &Args) -> Result<Output> {
    let presets = pav_view::CameraParams::PRESETS;
    match get_str(a, "preset") {
        None => Ok(Output::Json(json!(presets.iter().map(|p| p.0).collect::<Vec<_>>()))),
        Some(name) => {
            let p = presets.iter().find(|p| p.0.starts_with(name)).ok_or_else(|| anyhow!("unknown preset '{name}'"))?;
            s.camera.params = (p.1)();
            Ok(Output::Json(json!({ "preset": p.0 })))
        }
    }
}

fn t_capture(s: &mut Session, a: &Args) -> Result<Output> {
    let w = get_u64(a, "width", 960)? as u32;
    let h = get_u64(a, "height", 540)? as u32;
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        bail!("width/height must be 1..8192");
    }
    let path = PathBuf::from(get_str(a, "out").map(String::from).unwrap_or(format!("out/capture-{}.png", s.sim.state.tick)));
    let t = Instant::now();
    let rgba = s.render(w, h)?;
    let png = pav_render::capture::encode_png(w, h, &rgba)?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &png)?;
    let meta = json!({ "path": path, "width": w, "height": h, "tick": s.sim.state.tick, "render_ms": t.elapsed().as_millis() });
    Ok(Output::Image { png, path: Some(path), meta })
}

fn t_bench(s: &mut Session, a: &Args) -> Result<Output> {
    let n = get_u64(a, "ticks", 600)?.max(1);
    let t = Instant::now();
    s.step(n);
    let secs = t.elapsed().as_secs_f64();
    Ok(Output::Json(json!({
        "ticks": n,
        "seconds": (secs * 1000.0).round() / 1000.0,
        "ticks_per_second": (n as f64 / secs).round(),
        "realtime_factor": ((n as f64 / s.sim.config.tick_rate.hz() as f64) / secs * 10.0).round() / 10.0,
        "entities": s.sim.state.entities.len(),
    })))
}

fn t_gpu(s: &mut Session, _: &Args) -> Result<Output> {
    let g = s.gpu()?;
    Ok(Output::Json(json!({ "adapter": pav_render::gpu::describe(&g.headless.adapter.get_info()) })))
}

fn vec_arg(a: &Args, k: &str) -> Result<Option<Vec<f32>>> {
    match a.get(k) {
        None => Ok(None),
        Some(Value::Array(v)) => Ok(Some(v.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect())),
        Some(Value::String(t)) => Ok(Some(
            t.trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|x| x.trim().parse::<f32>())
                .collect::<Result<_, _>>()
                .map_err(|_| anyhow!("argument '{k}' must be numbers like [1, 2]"))?,
        )),
        Some(Value::Number(n)) => Ok(Some(vec![n.as_f64().unwrap_or(0.0) as f32])),
        _ => bail!("argument '{k}' must be an array of numbers"),
    }
}

fn buttons_arg(a: &Args, k: &str) -> Result<u32> {
    let Some(t) = get_str(a, k) else { return Ok(0) };
    let mut b = 0;
    for name in t.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        b |= pav_core::input::buttons::from_name(name).ok_or_else(|| anyhow!("unknown button '{name}'"))?;
    }
    Ok(b)
}

pub fn player_json(s: &Session) -> Value {
    let Some(p) = s.sim.player() else { return json!(null) };
    let ch = p.character.as_ref().unwrap();
    let feet = p.pos - glam::Vec3::Y * ch.height() * 0.5;
    json!({
        "id": p.id.0,
        "feet": [round3(feet.x), round3(feet.y), round3(feet.z)],
        "vel": [round3(ch.vel.x), round3(ch.vel.y), round3(ch.vel.z)],
        "grounded": ch.grounded,
        "posture": pav_core::params::ChoiceParam::name(ch.posture),
        "climbing": ch.climbing.is_some(),
        "hanging": ch.hang.map(|h| h.climb >= 0.0).map(|c| if c { "climbing up" } else { "hanging" }),
        "swimming": ch.swimming,
        "water_depth": round3(ch.water_depth),
        "rolling": ch.roll > 0.0,
        "stunned": ch.stun > 0.0,
        "model": pav_core::params::ChoiceParam::name(s.sim.config.movement.model),
        "facing_deg": round3(ch.facing.to_degrees()),
        "tick": s.sim.state.tick,
    })
}

fn t_player(s: &mut Session, _: &Args) -> Result<Output> {
    Ok(Output::Json(player_json(s)))
}

fn t_input(s: &mut Session, a: &Args) -> Result<Output> {
    let mv = vec_arg(a, "move")?.unwrap_or_default();
    let dir = glam::Vec2::new(mv.first().copied().unwrap_or(0.0), mv.get(1).copied().unwrap_or(0.0));
    let held = buttons_arg(a, "hold")?;
    let press = buttons_arg(a, "press")?;
    let aim = vec_arg(a, "aim")?.filter(|v| v.len() == 3).map(|v| glam::Vec3::new(v[0], v[1], v[2]));
    let n = get_u64(a, "ticks", 1)?.max(1);
    for i in 0..n {
        let f = pav_core::InputFrame {
            move_dir: dir.clamp_length_max(1.0),
            vertical: 0.0,
            aim,
            held: held | if i == 0 { press } else { 0 },
            pressed: if i == 0 { press } else { 0 },
        };
        s.sim.step(&f);
        s.sync_camera();
    }
    s.keep_events();
    Ok(Output::Json(player_json(s)))
}

fn t_spawn(s: &mut Session, a: &Args) -> Result<Output> {
    use pav_core::{BodyKind, Color, Shape, Spawn, Visual};
    let size = vec_arg(a, "size")?.unwrap_or_default();
    let g = |i: usize, d: f32| size.get(i).copied().unwrap_or(d);
    let shape = match get_str(a, "shape").unwrap_or("box") {
        "box" => Shape::Box { half: glam::Vec3::new(g(0, 0.4), g(1, g(0, 0.4)), g(2, g(0, 0.4))) },
        "rounded_box" => Shape::RoundedBox { half: glam::Vec3::new(g(0, 0.4), g(1, g(0, 0.4)), g(2, g(0, 0.4))), radius: 0.1 },
        "sphere" => Shape::Sphere { radius: g(0, 0.4) },
        "capsule" => Shape::Capsule { half_height: g(0, 0.3), radius: g(1, 0.25) },
        "cylinder" => Shape::Cylinder { half_height: g(0, 0.4), radius: g(1, 0.3) },
        other => bail!("unknown shape '{other}'"),
    };
    let pos = vec_arg(a, "pos")?.filter(|v| v.len() == 3).map(|v| glam::Vec3::new(v[0], v[1], v[2]));
    let pos = pos.unwrap_or_else(|| s.sim.state.focus + glam::Vec3::new(0.0, 3.0, 0.0));
    let body = match get_str(a, "body").unwrap_or("dynamic") {
        "dynamic" => BodyKind::Dynamic,
        "fixed" => BodyKind::Fixed,
        "kinematic" => BodyKind::Kinematic,
        "none" => BodyKind::None,
        other => bail!("unknown body '{other}'"),
    };
    let color = Color::try_hex(get_str(a, "color").unwrap_or("#e8704a")).context("color must be #rrggbb")?;
    let id = s.sim.spawn(Spawn::new(get_str(a, "name").unwrap_or("prop"), pos).visual(Visual::new(shape, color)).body(body));
    Ok(Output::Json(json!({ "id": id.0 })))
}

fn t_despawn(s: &mut Session, a: &Args) -> Result<Output> {
    let id = pav_core::EntityId(get_u64(a, "id", 0)? as u32);
    Ok(Output::Json(json!({ "removed": s.sim.despawn(id) })))
}

fn t_teleport(s: &mut Session, a: &Args) -> Result<Output> {
    let id = match a.get("id") {
        Some(_) => pav_core::EntityId(get_u64(a, "id", 0)? as u32),
        None => s.sim.state.player.context("no player")?,
    };
    let p = vec_arg(a, "pos")?.filter(|v| v.len() == 3).context("pos must be [x, y, z]")?;
    let ok = s.sim.set_position(id, glam::Vec3::new(p[0], p[1], p[2]));
    Ok(Output::Json(json!({ "moved": ok })))
}

fn t_rewind(s: &mut Session, a: &Args) -> Result<Output> {
    let now = s.sim.state.tick;
    let target = match (a.get("tick"), a.get("ticks")) {
        (Some(_), _) => get_u64(a, "tick", now)?,
        (None, Some(_)) => now.saturating_sub(get_u64(a, "ticks", 0)?),
        _ => bail!("give ticks=N (back) or tick=T"),
    };
    let oldest = s.sim.history.oldest_tick().unwrap_or(now);
    if !s.sim.rewind_to(target.max(oldest)) {
        bail!("nothing to rewind to");
    }
    s.sim.commit_rewind();
    let mut st = status(s);
    st["oldest_available"] = json!(oldest);
    Ok(Output::Json(st))
}

fn t_snap_save(s: &mut Session, a: &Args) -> Result<Output> {
    let path = get_str(a, "path").context("missing path")?;
    s.sim.save_state(std::path::Path::new(path))?;
    Ok(Output::Json(json!({ "saved": path, "tick": s.sim.state.tick })))
}

fn t_snap_load(s: &mut Session, a: &Args) -> Result<Output> {
    let path = get_str(a, "path").context("missing path")?;
    s.sim.load_state(std::path::Path::new(path))?;
    Ok(Output::Json(status(s)))
}

fn t_record_save(s: &mut Session, a: &Args) -> Result<Output> {
    let path = get_str(a, "path").context("missing path")?;
    let mut r = s.sim.recording.clone();
    r.params = pav_core::params::to_map(&mut s.params());
    r.final_hash = Some(format!("{:016x}", s.sim.state_hash()));
    if let Some(d) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, serde_json::to_string(&r)?)?;
    Ok(Output::Json(json!({ "saved": path, "ticks": r.ticks() })))
}

fn t_replay(s: &mut Session, a: &Args) -> Result<Output> {
    let path = get_str(a, "path").context("missing path")?;
    let r: pav_core::history::Replay = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let gpu = s.gpu.take();
    *s = Session::new(&r.scene, r.seed)?;
    s.gpu = gpu;
    let unknown = pav_core::params::apply_map(&mut s.params(), &r.params);
    for f in r.iter() {
        s.sim.step(f);
    }
    s.keep_events();
    let hash = format!("{:016x}", s.sim.state_hash());
    Ok(Output::Json(json!({
        "ticks": r.ticks(),
        "hash": hash,
        "expected": r.final_hash,
        "matches": r.final_hash.as_deref().map(|h| h == hash),
        "unknown_params": unknown,
    })))
}

fn slot_json(r: &pav_core::world::RoomSlot) -> Value {
    json!({
        "key": r.key,
        "name": r.def.name,
        "wing": r.def.wing,
        "built": r.built,
        "min": [round3(r.min.x), round3(r.min.z)],
        "max": [round3(r.max.x), round3(r.max.z)],
        "inside": [round3(r.inside.x), round3(r.inside.y), round3(r.inside.z)],
        "primary_device": r.def.primary_device,
        "movement_model": r.def.movement_model,
    })
}

fn t_rooms(s: &mut Session, _: &Args) -> Result<Output> {
    let w = &s.sim.state.world;
    Ok(Output::Json(json!({
        "rooms": w.rooms.iter().map(slot_json).collect::<Vec<_>>(),
        "current": w.current_room.and_then(|i| w.rooms.get(i as usize)).map(|r| r.key.clone()),
        "errors": w.errors,
    })))
}

fn t_room(s: &mut Session, a: &Args) -> Result<Output> {
    let w = &s.sim.state.world;
    let slot = match get_str(a, "key") {
        Some(k) => w.room(k).with_context(|| format!("no room '{k}'"))?,
        None => w.current_room.and_then(|i| w.rooms.get(i as usize)).context("the player is not in a room")?,
    };
    let mut v = slot_json(slot);
    v["about"] = json!(slot.def.about);
    v["try"] = json!(slot.def.try_list);
    v["params"] = serde_json::to_value(&slot.def.params)?;
    v["camera"] = serde_json::to_value(&slot.def.camera)?;
    Ok(Output::Json(v))
}

fn t_goto(s: &mut Session, a: &Args) -> Result<Output> {
    if let Some(r) = get_str(a, "room") {
        if r == "hub" {
            let pid = s.sim.state.player.context("no player")?;
            s.sim.set_position(pid, glam::Vec3::new(0.0, 0.0, 6.0));
        } else if !s.sim.teleport_to_room(r) {
            bail!("unknown room '{r}' (or not a world scene; use `load scene=world`)");
        }
    } else if let Some(p) = vec_arg(a, "pos")?.filter(|v| v.len() == 3) {
        let pid = s.sim.state.player.context("no player")?;
        s.sim.state.world.interest.push(glam::Vec3::new(p[0], p[1], p[2]));
        s.sim.update_streaming(usize::MAX);
        s.sim.state.world.interest.pop();
        s.sim.set_position(pid, glam::Vec3::new(p[0], p[1], p[2]));
    } else {
        bail!("give room=<key> or pos=[x,y,z]");
    }
    s.step(2);
    Ok(Output::Json(player_json(s)))
}

fn t_room_reset(s: &mut Session, a: &Args) -> Result<Output> {
    let w = &s.sim.state.world;
    let id = match get_str(a, "key") {
        Some(k) => w.room(k).with_context(|| format!("no room '{k}'"))?.id,
        None => w.current_room.context("the player is not in a room")?,
    };
    s.sim.reset_room(id);
    Ok(Output::Json(json!({ "reset": id })))
}

fn t_room_check(_: &mut Session, a: &Args) -> Result<Output> {
    let path = get_str(a, "path").context("missing path")?;
    let text = std::fs::read_to_string(path)?;
    Ok(Output::Json(match pav_core::room::RoomDef::parse(&text) {
        Ok(d) => json!({ "ok": true, "name": d.name, "size": d.layout.extent(), "objects": d.objects.len() }),
        Err(e) => json!({ "ok": false, "error": e }),
    }))
}

fn t_room_reload(s: &mut Session, _: &Args) -> Result<Output> {
    let dir = pav_core::room::rooms_dir();
    let (defs, errors) = pav_core::room::parse_all(&pav_core::room::load_sources(dir.as_deref()));
    let n = reload_rooms(&mut s.sim, defs);
    Ok(Output::Json(json!({ "dir": dir, "reloaded": n, "errors": errors })))
}

/// Applies freshly parsed room definitions: changed rooms are rebuilt, new/removed rooms
/// trigger a pavilion re-layout. Returns how many rooms changed.
pub fn reload_rooms(sim: &mut pav_core::Sim, defs: Vec<(String, pav_core::room::RoomDef)>) -> usize {
    if !sim.state.world.enabled {
        // Standalone room scene: rebuild it if it changed.
        let Some(slot) = sim.state.world.rooms.first().cloned() else { return 0 };
        if let Some((_, d)) = defs.into_iter().find(|(k, _)| *k == slot.key) {
            if serde_json::to_string(&d).ok() != serde_json::to_string(&*slot.def).ok() {
                let mut fresh = pav_core::Sim::empty(sim.state.seed);
                fresh.config = sim.config.clone();
                pav_core::scenes::build_standalone_room(&mut fresh, &slot.key, d);
                fresh.state.scene = sim.state.scene.clone();
                *sim = fresh;
                return 1;
            }
        }
        return 0;
    }
    let same_set = defs.len() == sim.state.world.rooms.len() && defs.iter().all(|(k, _)| sim.state.world.room(k).is_some());
    if !same_set {
        sim.rebuild_pavilion(defs);
        return 1;
    }
    let mut n = 0;
    for (k, d) in defs {
        let old = sim.state.world.room(&k).map(|r| r.def.clone());
        if let Some(old) = old {
            if serde_json::to_string(&d).ok() != serde_json::to_string(&*old).ok() {
                sim.replace_room(&k, d);
                n += 1;
            }
        }
    }
    n
}

fn t_stream(s: &mut Session, a: &Args) -> Result<Output> {
    if a.get("clear").and_then(|v| v.as_bool()).unwrap_or(false) {
        s.sim.state.world.interest.clear();
    }
    if let Some(p) = vec_arg(a, "point")?.filter(|v| v.len() == 3) {
        s.sim.state.world.interest.push(glam::Vec3::new(p[0], p[1], p[2]));
        s.sim.update_streaming(usize::MAX);
    }
    let st = &s.sim.state.statics;
    let fmt = |k: &pav_core::statics::RegionKey| format!("{k:?}");
    Ok(Output::Json(json!({
        "active": st.chunks.keys().map(fmt).collect::<Vec<_>>(),
        "dormant": st.dormant.keys().map(fmt).collect::<Vec<_>>(),
        "dormant_entities": s.sim.state.world.dormant_entities.values().map(|v| v.len()).sum::<usize>(),
        "interest": s.sim.state.world.interest.iter().map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>(),
    })))
}

fn t_filmstrip(s: &mut Session, a: &Args) -> Result<Output> {
    let frames = get_u64(a, "frames", 8)?.clamp(1, 64) as usize;
    let every = get_u64(a, "every", 10)?.max(1);
    let cols = get_u64(a, "columns", 4)? as u32;
    let w = get_u64(a, "width", 320)? as u32;
    let h = get_u64(a, "height", 180)? as u32;
    let mv = vec_arg(a, "move")?.unwrap_or_default();
    let dir = glam::Vec2::new(mv.first().copied().unwrap_or(0.0), mv.get(1).copied().unwrap_or(0.0));
    let held = buttons_arg(a, "hold")?;
    let press = buttons_arg(a, "press")?;
    let mut shots = Vec::with_capacity(frames);
    for i in 0..frames {
        shots.push(s.render(w, h)?);
        if i + 1 < frames {
            for t in 0..every {
                let first = i == 0 && t == 0;
                let f = pav_core::InputFrame {
                    move_dir: dir.clamp_length_max(1.0),
                    held: held | if first { press } else { 0 },
                    pressed: if first { press } else { 0 },
                    ..Default::default()
                };
                s.sim.step(&f);
                s.sync_camera();
            }
            s.keep_events();
        }
    }
    let (tw, th, px) = pav_render::capture::tile_frames(&shots, w, h, cols);
    let png = pav_render::capture::encode_png(tw, th, &px)?;
    let path = PathBuf::from(get_str(a, "out").map(String::from).unwrap_or(format!("out/filmstrip-{}.png", s.sim.state.tick)));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &png)?;
    let meta = json!({ "path": path, "frames": frames, "every_ticks": every, "size": [tw, th], "tick": s.sim.state.tick });
    Ok(Output::Image { png, path: Some(path), meta })
}

fn t_audio_capture(s: &mut Session, a: &Args) -> Result<Output> {
    let n = get_u64(a, "ticks", 180)?.max(1);
    let mv = vec_arg(a, "move")?.unwrap_or_default();
    let dir = glam::Vec2::new(mv.first().copied().unwrap_or(0.0), mv.get(1).copied().unwrap_or(0.0));
    let held = buttons_arg(a, "hold")?;
    let press = buttons_arg(a, "press")?;
    let aim = vec_arg(a, "aim")?.filter(|v| v.len() == 3).map(|v| glam::Vec3::new(v[0], v[1], v[2]));
    s.keep_events();
    let dt = s.sim.dt();
    let mut events = Vec::new();
    for i in 0..n {
        let f = pav_core::InputFrame {
            move_dir: dir.clamp_length_max(1.0),
            aim,
            held: held | if i == 0 { press } else { 0 },
            pressed: if i == 0 { press } else { 0 },
            ..Default::default()
        };
        s.sim.step(&f);
        s.sync_camera();
        for e in s.sim.drain_events() {
            events.push((i as f32 * dt, e));
        }
    }
    let listener = pav_audio::Listener { pos: s.sim.state.focus, right: glam::Vec3::X };
    let duration = n as f32 * dt + 1.5;
    let samples = pav_audio::render_events(&events, duration, 44100.0, &listener);
    let path = PathBuf::from(get_str(a, "out").map(String::from).unwrap_or(format!("out/audio-{}.wav", s.sim.state.tick)));
    pav_audio::write_wav(&path, &samples, 44100)?;
    let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let kinds: std::collections::BTreeMap<String, usize> = events.iter().fold(Default::default(), |mut m, (_, e)| {
        let k = serde_json::to_value(e)
            .ok()
            .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(String::from))
            .unwrap_or_default();
        *m.entry(k).or_default() += 1;
        m
    });
    Ok(Output::Json(json!({ "path": path, "seconds": duration, "events": kinds, "peak": (peak * 1000.0).round() / 1000.0 })))
}

fn t_course(s: &mut Session, _: &Args) -> Result<Output> {
    let c = &s.sim.state.courses;
    Ok(Output::Json(json!({
        "run": c.hud(s.sim.state.tick, s.sim.dt()),
        "last": c.last,
        "best": c.best,
        "checkpoint": c.checkpoint.map(|(p, _)| [round3(p.x), round3(p.y), round3(p.z)]),
        "message": c.message.as_ref().map(|m| &m.0),
    })))
}

fn t_feel(s: &mut Session, _: &Args) -> Result<Output> {
    let f = s.sim.state.feel.report;
    let ms = s.sim.dt() * 1000.0;
    Ok(Output::Json(json!({
        "model": pav_core::params::ChoiceParam::name(s.sim.config.movement.model),
        "response_ticks": f.response_ticks,
        "response_ms": (f.response_ticks as f32 * ms).round(),
        "speed": round3(f.speed),
        "top_speed": round3(f.top_speed),
        "accel_ms": f.accel_ms.round(),
        "stop_ms": f.stop_ms.round(),
        "stop_dist": round3(f.stop_dist),
        "turn_ms": f.turn_ms.round(),
        "jump_height": round3(f.jump_height),
        "air_ms": f.air_ms.round(),
        "jump_dist": round3(f.jump_dist),
    })))
}

fn t_camera_bench(s: &mut Session, a: &Args) -> Result<Output> {
    use pav_view::CameraParams;
    let w = get_u64(a, "width", 320)? as u32;
    let h = get_u64(a, "height", 180)? as u32;
    let cols = get_u64(a, "columns", 4)? as u32;
    let base = s.camera.params.clone();
    let list: Vec<(String, CameraParams)> = match get_str(a, "presets") {
        None => CameraParams::PRESETS.iter().map(|(n, f)| (n.to_string(), f())).collect(),
        Some(spec) => spec
            .split(',')
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .map(|t| match t.parse::<f32>() {
                Ok(tilt) => Ok((format!("{tilt}°"), CameraParams { tilt, ..base.clone() })),
                Err(_) => CameraParams::PRESETS
                    .iter()
                    .find(|(n, _)| n.starts_with(t))
                    .map(|(n, f)| (n.to_string(), f()))
                    .ok_or_else(|| anyhow!("unknown preset '{t}'")),
            })
            .collect::<Result<_>>()?,
    };
    let mut shots = Vec::with_capacity(list.len());
    for (name, mut p) in list.iter().cloned() {
        if !name.starts_with("isometric") {
            p.yaw = base.yaw;
        }
        s.camera.params = p;
        shots.push(s.render(w, h)?);
    }
    s.camera.params = base;
    let (tw, th, px) = pav_render::capture::tile_frames(&shots, w, h, cols);
    let png = pav_render::capture::encode_png(tw, th, &px)?;
    let path = PathBuf::from(get_str(a, "out").map(String::from).unwrap_or(format!("out/camera-bench-{}.png", s.sim.state.tick)));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &png)?;
    let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
    let meta = json!({ "path": path, "tiles": names, "size": [tw, th] });
    Ok(Output::Image { png, path: Some(path), meta })
}

fn t_npcs(s: &mut Session, a: &Args) -> Result<Output> {
    let prefix = get_str(a, "name").unwrap_or("");
    let player = s.sim.state.player;
    let r = |v: f32| (v * 1000.0).round() / 1000.0;
    let list: Vec<Value> = s
        .sim
        .state
        .entities
        .iter()
        .filter(|e| Some(e.id) != player && e.name.starts_with(prefix))
        .filter_map(|e| {
            let ch = e.character.as_ref()?;
            let feet = e.pos - glam::Vec3::Y * ch.height() * 0.5;
            let body = ch.puppet.as_ref().map(|d| d.body).unwrap_or(s.sim.config.puppet.body);
            Some(json!({
                "id": e.id.0,
                "name": e.name,
                "body": pav_core::params::ChoiceParam::name(body),
                "feet": [r(feet.x), r(feet.y), r(feet.z)],
                "vel": [r(ch.vel.x), r(ch.vel.y), r(ch.vel.z)],
                "facing_deg": r(ch.facing.to_degrees()),
                "grounded": ch.grounded,
                "ai": e.ai.as_ref().map(|a| serde_json::to_value(&a.def).unwrap_or_default()),
                "steps": ch.rig.as_ref().map(|r| r.steps),
                "flinch": r(ch.anim.hit_side.abs() + ch.anim.hit_fwd.abs()),
            }))
        })
        .collect();
    Ok(Output::Json(json!(list)))
}

fn t_signal(s: &mut Session, a: &Args) -> Result<Output> {
    let name = get_str(a, "name").context("give name=<signal>")?.to_string();
    s.sim.state.signals.push(name.clone());
    s.step(1);
    Ok(Output::Json(json!({ "sent": name, "entities": s.sim.state.entities.len() })))
}
