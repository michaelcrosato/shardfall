//! `asset_import`: open model libraries (glTF, OBJ) read into the engine's frame. This stage
//! reads and measures; translating into readable props (the fitter) comes next (plan row 5.2).
//!
//! - no arguments: the library directory (`import::libraries`);
//! - `get=lib/pack`: downloads a pack its catalog pins into `.cache/assets/<lib>/<pack>/`;
//! - `from=<model> list=true`: one model summarised (parts, triangles, size, materials, main
//!   colours, images, extensions, warnings); `from=<folder> list=true`: a row per model;
//! - `survey=lib/pack|folder`: a census of every model, written to the pack's ledger
//!   (`assets/catalogs/<lib>/<pack>.tsv`, or `out=`), keeping each row's `note`.
//!
//! Sizes are reported in centimetres, the house unit (D1). The same input gives the same reply
//! and the same ledger bytes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use pav_core::issue::{self, Code, Issue};
use serde_json::{Value, json};

use crate::import::ledger::Ledger;
use crate::import::libraries::{self, lib_pack, models_in};
use crate::import::normalize::{self, Norm, Normalized};
use crate::import::report::write_text;
use crate::import::scene::{Alpha, Scene, hex};
use crate::import::{self as imp};
use crate::session::Session;
use crate::tools::{Args, Output, get_bool, get_f32, get_str, get_u64};

const ARGS: &[&str] = &["get", "survey", "from", "list", "up", "yaw", "scale", "place", "out", "limit"];

/// The ledger's columns (plan §8.2). A census fills the first eight and `note`; the fit columns
/// stay empty until the fitter writes them.
pub const LEDGER_COLS: &[&str] = &[
    "id", "file", "tris", "mats", "tex", "comps", "size_cm", "flags", "parts", "mesh_pct", "iou", "iou_mean", "iou_gpu", "de",
    "chars", "tier", "used", "note",
];

fn allowed(a: &Args) -> Result<()> {
    if let Some(k) = a.keys().find(|k| !ARGS.contains(&k.as_str()) && !["scene", "seed", "ticks"].contains(&k.as_str())) {
        bail!(
            "{}",
            Issue::error(k.as_str(), Code::UnknownKey, "unknown argument")
                .expected(ARGS.join(", "))
                .maybe_fix(issue::suggestion(k, ARGS.iter().copied()))
        );
    }
    Ok(())
}

fn num(a: &Args, k: &str) -> Result<Option<f64>> {
    Ok(if a.contains_key(k) { Some(get_f32(a, k, 0.0)? as f64) } else { None })
}

/// Normalisation from the arguments over a pack's pinned placement.
fn norm(a: &Args, pinned: Option<&serde_json::Map<String, Value>>) -> Result<Norm> {
    let p = |k: &str| pinned.and_then(|s| s.get(k));
    let text = |k: &str| get_str(a, k).or_else(|| p(k).and_then(Value::as_str));
    let number = |k: &str| -> Result<Option<f64>> { Ok(num(a, k)?.or_else(|| p(k).and_then(Value::as_f64))) };
    Norm::parse(text("up"), number("yaw")?, number("scale")?, text("place"))
}

pub fn t_asset_import(_: &mut Session, a: &Args) -> Result<Output> {
    allowed(a)?;
    if let Some(packs) = get_str(a, "get") {
        let mut got = Vec::new();
        for p in packs.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (lib, pack) = lib_pack(p)?;
            got.push(libraries::get(lib, pack)?);
        }
        return Ok(Output::Json(json!({
            "got": got,
            "next": "asset_import survey=<lib/pack> censuses it; from=<model> list=true summarises one",
        })));
    }
    if let Some(what) = get_str(a, "survey") {
        return survey(a, what);
    }
    if let Some(from) = get_str(a, "from") {
        if !get_bool(a, "list", false)? {
            bail!("from= is read with list=true for now: translating models into props arrives with the fitter (plan row 5.2)");
        }
        let path = PathBuf::from(from);
        let n = norm(a, None)?;
        if path.is_dir() {
            return list_folder(a, &path, &n);
        }
        let (s, done) = read(&path, &n)?;
        return Ok(Output::Json(summary(&s, &done)));
    }
    if get_bool(a, "list", false)? {
        bail!("list=true summarises from= (a model file or a folder)");
    }
    Ok(Output::Json(libraries::directory()))
}

/// Reads a model and brings it into the engine's frame.
pub fn read(path: &Path, n: &Norm) -> Result<(Scene, Normalized)> {
    let mut s = imp::read_model(path)?;
    let done = normalize::normalize(&mut s, n);
    Ok((s, done))
}

fn cm(v: f64) -> f64 {
    (v * 1000.0).round() / 10.0
}

/// The words that sort a model (fixed vocabulary).
pub fn flags(s: &Scene, done: &Normalized) -> Vec<&'static str> {
    let mats = || s.meshes.iter().filter_map(|m| m.material.and_then(|i| s.materials.get(i)));
    let tex_images = || mats().filter_map(|m| m.base_tex).filter_map(|t| s.images.get(t.image));
    let mut f = Vec::new();
    let mut add = |on: bool, w: &'static str| {
        if on {
            f.push(w);
        }
    };
    add(s.info.skinned, "skinned");
    add(s.info.animated, "animated");
    add(tex_images().any(|i| !i.rgba.is_empty()), "textured");
    add(tex_images().any(|i| i.rgba.is_empty()), "undecoded");
    add(s.meshes.iter().any(|m| !m.col.is_empty()), "vcolor");
    add(s.meshes.iter().any(|m| !m.uv1.is_empty()), "uv1");
    add(mats().any(|m| m.alpha != Alpha::Opaque), "alpha");
    add(mats().any(|m| m.unlit), "unlit");
    add(mats().any(|m| m.emissive.iter().any(|&e| e > 0.0)), "emissive");
    add(s.meshes.iter().any(|m| m.material.is_none()), "nomat");
    add(!done.flipped.is_empty(), "flipped");
    add(s.info.warnings.iter().any(|w| !w.starts_with("skinned")), "warn");
    f
}

/// One model summarised.
pub fn summary(s: &Scene, done: &Normalized) -> Value {
    let parts = normalize::parts(s);
    let (lo, hi) = s.bounds().map_or(([0.0; 3], [0.0; 3]), |(l, h)| (l.to_array(), h.to_array()));
    let tris_of = |node: usize| s.meshes.iter().filter(|m| m.node == node).map(|m| m.tris.len()).sum::<usize>();
    let nodes: Vec<Value> = s
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, _)| tris_of(*i) > 0)
        .map(|(i, n)| json!({"node": n.name, "tris": tris_of(i), "parts": parts.iter().filter(|p| p.node == i).count()}))
        .collect();
    let used: Vec<usize> = {
        let mut u: Vec<usize> = s.meshes.iter().filter_map(|m| m.material).collect();
        u.sort();
        u.dedup();
        u
    };
    let materials: Vec<Value> = used
        .iter()
        .map(|&i| {
            let m = &s.materials[i];
            let mut v = json!({"name": m.name, "color": hex(m.base.map(f64::from)[..3].try_into().unwrap())});
            if let Some(t) = m.base_tex {
                let img = &s.images[t.image];
                v["tex"] =
                    json!(format!("{} {}x{}{}", img.name, img.w, img.h, if img.rgba.is_empty() { " (not decoded)" } else { "" }));
            }
            match m.alpha {
                Alpha::Opaque => {}
                Alpha::Mask(c) => v["alpha"] = json!(format!("mask {c}")),
                Alpha::Blend => v["alpha"] = json!(format!("blend {}", (m.base[3] * 100.0).round() / 100.0)),
            }
            if m.unlit {
                v["unlit"] = json!(true);
            }
            if m.emissive.iter().any(|&e| e > 0.0) {
                v["emissive"] = json!(hex(m.emissive.map(|e| e.min(1.0) as f64)));
            }
            v
        })
        .collect();
    let shown = 16;
    let mut out = json!({
        "file": s.info.file,
        "format": s.info.format,
        "tris": s.tris(),
        "size_cm": [cm(hi[0] - lo[0]), cm(hi[1] - lo[1]), cm(hi[2] - lo[2])],
        "min_cm": lo.map(cm),
        "max_cm": hi.map(cm),
        "parts": parts.len(),
        "nodes": nodes.iter().take(shown).collect::<Vec<_>>(),
        "materials": materials.iter().take(shown).collect::<Vec<_>>(),
        "colours": normalize::palette(s, 6).into_iter().map(|(h, pct)| json!([h, pct])).collect::<Vec<_>>(),
    });
    if nodes.len() > shown {
        out["nodes_more"] = json!(nodes.len() - shown);
    }
    if materials.len() > shown {
        out["materials_more"] = json!(materials.len() - shown);
    }
    if !s.images.is_empty() {
        out["images"] = json!(s.images.iter().map(|i| format!("{} {} {}x{}", i.name, i.kind, i.w, i.h)).collect::<Vec<_>>());
    }
    if !s.info.extensions.is_empty() {
        out["extensions"] = json!(s.info.extensions);
    }
    let f = flags(s, done);
    if !f.is_empty() {
        out["flags"] = json!(f);
    }
    if !s.info.warnings.is_empty() {
        out["warnings"] = json!(s.info.warnings.iter().take(8).collect::<Vec<_>>());
    }
    let moved = done.moved.map(cm);
    if moved != [0.0; 3] {
        out["moved_cm"] = json!(moved);
    }
    out["next"] = json!("translation into a readable prop arrives with the fitter (plan row 5.2)");
    out
}

/// A model's census: the ledger's first columns (id and note are the caller's).
fn census(s: &Scene, done: &Normalized) -> BTreeMap<&'static str, String> {
    let (lo, hi) = s.bounds().map_or(([0.0; 3], [0.0; 3]), |(l, h)| (l.to_array(), h.to_array()));
    let used: std::collections::BTreeSet<usize> = s.meshes.iter().filter_map(|m| m.material).collect();
    let tex: std::collections::BTreeSet<usize> = used.iter().filter_map(|&i| s.materials[i].base_tex).map(|t| t.image).collect();
    let size = (0..3).map(|k| format!("{}", ((hi[k] - lo[k]) * 100.0).round() as i64)).collect::<Vec<_>>().join("x");
    BTreeMap::from([
        ("tris", s.tris().to_string()),
        ("mats", used.len().to_string()),
        ("tex", tex.len().to_string()),
        ("comps", normalize::parts(s).len().to_string()),
        ("size_cm", size),
        ("flags", flags(s, done).join(" ")),
    ])
}

/// Each model's id, from its own path alone so it never changes as other files come and go:
/// the path below the pack's model folder (the longest folder part of its `models` globs; for a
/// plain folder, the path inside it) without the extension. Two files that differ only in
/// their extension (`chair.glb`, `chair.obj`) keep it.
fn ids(files: &[PathBuf], glob: Option<&str>) -> Vec<String> {
    let roots: Vec<&str> = glob.map(|g| g.split(',').map(|g| libraries::glob_root(g.trim())).collect()).unwrap_or_default();
    let rel = |f: &PathBuf| f.to_string_lossy().replace('\\', "/");
    let bare = |f: &PathBuf| {
        let r = rel(f);
        let root = roots.iter().filter(|root| r.starts_with(*root)).max_by_key(|root| root.len()).copied().unwrap_or("");
        let id = &r[root.len()..];
        id.rsplit_once('.').filter(|(stem, _)| !stem.is_empty() && !stem.ends_with('/')).map_or(id, |(stem, _)| stem).to_string()
    };
    files
        .iter()
        .map(|f| {
            let id = bare(f);
            if files.iter().filter(|g| bare(g) == id).count() > 1 { rel(f) } else { id }
        })
        .collect()
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

struct Row {
    cols: BTreeMap<&'static str, String>,
    error: Option<String>,
}

/// Reads every model of a folder (sorted), one row each.
fn rows(dir: &Path, files: &[PathBuf], n: &Norm) -> Vec<Row> {
    files
        .iter()
        .map(|f| match read(&dir.join(f), n) {
            Ok((s, done)) => Row { cols: census(&s, &done), error: None },
            Err(e) => Row { cols: BTreeMap::from([("flags", "error".to_string())]), error: Some(format!("{e:#}")) },
        })
        .collect()
}

fn short(id: &str, r: &Row) -> String {
    let g = |k: &str| r.cols.get(k).map(String::as_str).unwrap_or("");
    match &r.error {
        Some(e) => format!("{id}: {e}"),
        None => format!("{id} {}t {}p {}cm {}", g("tris"), g("comps"), g("size_cm"), g("flags")).trim_end().to_string(),
    }
}

fn list_folder(a: &Args, dir: &Path, n: &Norm) -> Result<Output> {
    let files = models_in(dir, None);
    if files.is_empty() {
        bail!("{}: no .glb, .gltf or .obj files", dir.display());
    }
    let ids = ids(&files, None);
    let rows = rows(dir, &files, n);
    let limit = get_u64(a, "limit", 20)? as usize;
    let lines: Vec<String> = ids.iter().zip(&rows).map(|(id, r)| short(id, r)).collect();
    Ok(Output::Json(json!({
        "folder": dir.display().to_string(),
        "models": files.len(),
        "rows": lines.iter().take(limit).collect::<Vec<_>>(),
        "rows_more": lines.len().saturating_sub(limit),
        "columns": "id tris(t) parts(p) size_cm flags",
        "next": "from=<folder>/<file> list=true shows one; survey= writes a ledger",
    })))
}

fn median(mut v: Vec<usize>) -> usize {
    v.sort();
    v.get(v.len() / 2).copied().unwrap_or(0)
}

/// The ledger text: a comment, the header, then one row per model (sorted by file).
pub fn ledger_text(ids: &[String], files: &[PathBuf], rows: &[(BTreeMap<&'static str, String>, String)]) -> String {
    let mut out = String::from(
        "# asset census (asset_import survey): tris, materials used, textures, connected parts, size WxHxD cm, flags; fit columns are empty until the fitter\n",
    );
    out.push_str(&LEDGER_COLS.join("\t"));
    out.push('\n');
    for ((id, f), (cols, note)) in ids.iter().zip(files).zip(rows) {
        let line: Vec<String> = LEDGER_COLS
            .iter()
            .map(|&c| match c {
                "id" => clean(id),
                "file" => clean(&f.to_string_lossy().replace('\\', "/")),
                "note" => clean(note),
                _ => clean(cols.get(c).map(String::as_str).unwrap_or("")),
            })
            .collect();
        out.push_str(&line.join("\t"));
        out.push('\n');
    }
    out
}

fn survey(a: &Args, what: &str) -> Result<Output> {
    let as_dir = PathBuf::from(what);
    let (dir, glob, n, ledger_path, label) = if as_dir.is_dir() {
        // A plain folder is not a library's pack, so it has no place among the catalogs'
        // ledgers (plan §8.7): the caller names the file.
        let out = get_str(a, "out").map(PathBuf::from).ok_or_else(|| {
            anyhow!(
                "survey={what}: a folder's census needs out=<file>.tsv (a library's packs go to assets/catalogs/<lib>/<pack>.tsv); from={what} list=true shows the rows without writing"
            )
        })?;
        (as_dir.clone(), None, norm(a, None)?, out, as_dir.display().to_string())
    } else {
        let (lib, pack) = lib_pack(what).map_err(|e| anyhow!("{e} (or a folder of models)"))?;
        let src = libraries::source(lib, pack).ok();
        let dir = libraries::pack_dir(lib, pack);
        if !dir.is_dir() {
            bail!("{what} is not downloaded: asset_import get={what}");
        }
        let glob = src.as_ref().and_then(|s| s.get("models")).and_then(Value::as_str).map(str::to_string);
        let out = get_str(a, "out")
            .map(PathBuf::from)
            .unwrap_or_else(|| libraries::catalogs_dir().join(lib).join(format!("{pack}.tsv")));
        (dir, glob, norm(a, src.as_ref())?, out, what.to_string())
    };
    let files = models_in(&dir, glob.as_deref());
    if files.is_empty() {
        bail!("{}: no models{}", dir.display(), glob.map(|g| format!(" matching {g}")).unwrap_or_default());
    }
    let ids = ids(&files, glob.as_deref());
    let read = rows(&dir, &files, &n);
    let old = Ledger::read(&ledger_path);
    // An agent's note, by the model's id, else by its file (a ledger written under another id).
    let col = |c: &str| old.cols.iter().position(|x| x == c);
    let note_of = |id: &str, file: &str| -> String {
        old.get(id, "note")
            .filter(|n| !n.is_empty())
            .or_else(|| {
                let (fc, nc) = (col("file")?, col("note")?);
                old.rows.iter().find(|r| r.get(fc).map(String::as_str) == Some(file)).and_then(|r| r.get(nc)).map(String::as_str)
            })
            .unwrap_or("")
            .to_string()
    };
    let failed: Vec<String> = ids.iter().zip(&read).filter(|(_, r)| r.error.is_some()).map(|(id, r)| short(id, r)).collect();
    let ok: Vec<&Row> = read.iter().filter(|r| r.error.is_none()).collect();
    let num_of = |r: &Row, k: &str| r.cols.get(k).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    let tris: Vec<usize> = ok.iter().map(|r| num_of(r, "tris")).collect();
    let mut flag_count: BTreeMap<String, usize> = BTreeMap::new();
    for r in &ok {
        for f in r.cols.get("flags").map(String::as_str).unwrap_or("").split_whitespace() {
            *flag_count.entry(f.to_string()).or_default() += 1;
        }
    }
    let with_notes: Vec<(BTreeMap<&'static str, String>, String)> = ids
        .iter()
        .zip(&files)
        .zip(&read)
        .map(|((id, f), r)| (r.cols.clone(), note_of(id, &f.to_string_lossy().replace('\\', "/"))))
        .collect();
    write_text(&ledger_path, &ledger_text(&ids, &files, &with_notes))?;
    let written = ledger_path.display().to_string();
    let limit = get_u64(a, "limit", 20)? as usize;
    let lines: Vec<String> = ids.iter().zip(&read).filter(|(_, r)| r.error.is_none()).map(|(id, r)| short(id, r)).collect();
    let (failed_shown, failed_more) = (json!(failed.iter().take(limit).collect::<Vec<_>>()), failed.len().saturating_sub(limit));
    Ok(Output::Json(json!({
        "survey": label,
        "models": files.len(),
        "read": ok.len(),
        "failed": failed_shown,
        "failed_more": failed_more,
        "tris": {"total": tris.iter().sum::<usize>(), "median": median(tris.clone()), "max": tris.iter().max().copied().unwrap_or(0)},
        "parts_median": median(ok.iter().map(|r| num_of(r, "comps")).collect()),
        "flags": flag_count,
        "ledger": written,
        "rows": lines.iter().take(limit).collect::<Vec<_>>(),
        "rows_more": lines.len().saturating_sub(limit),
        "next": "from=<model> list=true shows one; fitting into props arrives with plan row 5.2",
    })))
}
