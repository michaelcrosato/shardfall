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
    Ok(Output::Json(json!(pav_core::scenes::SCENES.iter().map(|(n, d)| json!({"name": n, "about": d})).collect::<Vec<_>>())))
}

fn t_load(s: &mut Session, a: &Args) -> Result<Output> {
    let scene = get_str(a, "scene").unwrap_or("test");
    let seed = get_u64(a, "seed", 1)?;
    let gpu = s.gpu.take();
    let (camera, view) = (s.camera.params.clone(), s.view.clone());
    *s = Session::new(scene, seed)?;
    s.gpu = gpu;
    s.camera.params = camera;
    s.view = view;
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
    }
    s.sim.drain_events();
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
    s.sim.drain_events();
    let hash = format!("{:016x}", s.sim.state_hash());
    Ok(Output::Json(json!({
        "ticks": r.ticks(),
        "hash": hash,
        "expected": r.final_hash,
        "matches": r.final_hash.as_deref().map(|h| h == hash),
        "unknown_params": unknown,
    })))
}
