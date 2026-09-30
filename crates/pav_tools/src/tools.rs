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
