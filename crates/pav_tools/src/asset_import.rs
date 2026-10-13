//! `asset_import`: open model libraries (glTF, OBJ) read into the engine's frame and translated
//! into readable format-2 props (`import::fit`), with their fidelity measured (`import::metrics`).
//!
//! - no arguments: the library directory (`import::libraries`);
//! - `get=lib/pack`: downloads a pack its catalog pins into `.cache/assets/<lib>/<pack>/`;
//! - `from=<model> list=true`: one model summarised (parts, triangles, size, materials, main
//!   colours, images, extensions, warnings); `from=<folder> list=true`: a row per model;
//! - `from=<model|folder|lib/pack>`: translated (`fit=auto|prims|mesh`) with the catalog's
//!   category and tags, and written where `out_dir` routes it: `out=`, else by the licence gate
//!   to `lib/<pack>/` (CC0, CC-BY: committed, with `PACK.json`) or `local/<pack>/` (other known
//!   licences, packs the catalog keeps local, files outside a pinned pack), mesh parts in
//!   `meshes/` beside, then the pack's index and the credits; `compare=true` adds the
//!   source-over-translation image;
//! - `survey=lib/pack|folder`: every model censused and translated without writing (`fit=false`:
//!   census only), into the pack's ledger (`assets/catalogs/<lib>/<pack>.tsv`, or `out=`),
//!   keeping each row's `note`;
//! - `check=<asset.json>`: an asset with a `source` measured against that source again.
//!
//! Sizes are reported in centimetres, the house unit (D1). The same input gives the same reply,
//! the same ledger bytes and the same asset files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use pav_core::issue::{self, Code, Issue};
use serde_json::{Value, json};

use pav_core::props::doc::Category;
use pav_core::props::mesh::MeshFile;

use crate::asset_library;
use crate::import::catalog::Catalog;
use crate::import::fit::{self, Mode, Translation, segment};
use crate::import::ledger::Ledger;
use crate::import::libraries::{self, lib_pack, models_in};
use crate::import::licence::{self, Route};
use crate::import::normalize::{self, Norm, Normalized, Place};
use crate::import::raster::{self, Tri};
use crate::import::report::write_text;
use crate::import::scene::{Alpha, Scene, hex};
use crate::import::{self as imp, fetch, metrics};
use crate::session::Session;
use crate::tools::{Args, Output, get_bool, get_f32, get_str, get_u64};

const ARGS: &[&str] =
    &["get", "survey", "from", "list", "fit", "check", "compare", "up", "yaw", "scale", "place", "out", "limit"];

/// The ledger's columns (plan §8.2). A census fills the first eight and `note`; a survey that
/// fits fills `parts` to `tier` (`iou_gpu` stays empty: the GPU cross-check is deferred) and
/// `used` waits for the library.
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

/// A normalisation as a catalog's placement fields, for `norm` to read arguments over.
fn norm_map(n: &Norm) -> serde_json::Map<String, Value> {
    let place = match n.place {
        Place::Keep => "keep",
        Place::Floor => "floor",
        Place::Center => "center",
    };
    let up = if n.z_up { "z" } else { "y" };
    serde_json::Map::from_iter([
        ("up".to_string(), json!(up)),
        ("yaw".to_string(), json!(n.yaw)),
        ("scale".to_string(), json!(n.scale)),
        ("place".to_string(), json!(place)),
    ])
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
    if let Some(file) = get_str(a, "check") {
        return check(a, file);
    }
    if let Some(what) = get_str(a, "survey") {
        return survey(a, what);
    }
    if let Some(from) = get_str(a, "from") {
        if !get_bool(a, "list", false)? {
            return translate_from(a, from);
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
    out["next"] = json!("from=<this file> translates it into a prop");
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
        let stem = id.rsplit_once('.').filter(|(stem, _)| !stem.is_empty() && !stem.ends_with('/')).map_or(id, |(stem, _)| stem);
        // KayKit names its binaries `chair.gltf.glb`.
        stem.strip_suffix(".gltf").filter(|s| !s.is_empty() && !s.ends_with('/')).unwrap_or(stem).to_string()
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
    let mut out = String::from(LEDGER_HEAD);
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

// --- Sources: a library's pack, a folder or one file ------------------------------------------

/// What a pack's catalog says about one model (plan §8.7): `"<pack>/<id>": ["<category> <tag>
/// ...", "<description>"]`, or a `$skip` line with the reason.
#[derive(Clone, Debug, Default)]
struct Meta {
    category: Option<String>,
    tags: Vec<String>,
    description: String,
    skip: Option<String>,
    /// The catalog has a line for it (a `listed` source translates only those).
    listed: bool,
}

/// A pinned pack's credits, for its `PACK.json`, and whether its catalog keeps it local.
#[derive(Clone, Debug)]
struct PackInfo {
    title: String,
    credit: String,
    source: String,
    catalog: String,
    /// Why a pack whose licence could be shared stays in the local tier (`"local": "..."`).
    local: Option<String>,
}

/// Where models come from.
struct Src {
    /// The folder `files` are relative to.
    dir: PathBuf,
    files: Vec<PathBuf>,
    ids: Vec<String>,
    /// Each model's prop name (`names`: distinct).
    names: Vec<String>,
    /// Each model's catalog line.
    meta: Vec<Meta>,
    norm: Norm,
    /// What `check` reads the source with when a translation records no `norm`: the pack's
    /// pinned placement, else the default.
    base_norm: Norm,
    /// The catalog's `lib/pack`, when they come from a downloaded pack.
    lib_pack: Option<String>,
    /// Only some of the pack's models (a file or folder inside it).
    partial: bool,
    /// The library pack translations belong to (the catalog's `pack`; `import` for others).
    pack: String,
    license: Option<String>,
    pack_info: Option<PackInfo>,
    label: String,
}

/// A library pack (`lib/pack`), a folder, or one model file; a file or folder inside a
/// downloaded pack counts as that pack's (only its models, with the pack's catalog data).
fn source_of(a: &Args, what: &str) -> Result<Src> {
    let path = PathBuf::from(what);
    if !path.exists() {
        let (lib, pack) = lib_pack(what).map_err(|e| anyhow!("{e}, a folder or a model file"))?;
        return pack_source(a, lib, pack, None);
    }
    // Inside a downloaded pack: .cache/assets/<lib>/<pack>/...
    let cache = fetch::cache_dir_for("assets");
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if let Ok(rel) = canon(&path).strip_prefix(canon(&cache)) {
        let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
        if parts.len() >= 2 && libraries::source(&parts[0], &parts[1]).is_ok() {
            let inside: PathBuf = rel.iter().skip(2).collect();
            let only = (!inside.as_os_str().is_empty()).then_some(inside);
            return pack_source(a, &parts[0], &parts[1], only);
        }
    }
    let (dir, files) = if path.is_dir() {
        (path.clone(), models_in(&path, None))
    } else {
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        (dir, vec![PathBuf::from(path.file_name().ok_or_else(|| anyhow!("{what}: not a model file"))?)])
    };
    if files.is_empty() {
        bail!("{what}: no .glb, .gltf or .obj files");
    }
    let ids = ids(&files, None);
    Ok(Src {
        dir,
        meta: vec![Meta::default(); files.len()],
        files,
        names: names(&ids),
        ids,
        norm: norm(a, None)?,
        base_norm: Norm::default(),
        lib_pack: None,
        // Loose imports share local/import: one never removes another's files.
        partial: true,
        pack: "import".into(),
        license: None,
        pack_info: None,
        label: what.to_string(),
    })
}

/// A model's catalog line, checked: a category first, then 2-6 tags.
fn catalog_meta(cat: &Catalog, lib: &str, key: &str) -> Result<Meta> {
    let mut m = Meta { skip: cat.skip.get(key).cloned(), listed: cat.skip.contains_key(key), ..Default::default() };
    if cat.has(key) && cat.entry(key).is_none() {
        bail!("assets/catalogs/{lib}.json \"{key}\": a model line is [\"<category> <tags>\", \"<description>\"]");
    }
    if let Some(e) = cat.entry(key) {
        m.listed = true;
        let mut words = e.tags.into_iter();
        let category = words.next().unwrap_or_default();
        if Category::parse(&category).is_none() {
            bail!(
                "assets/catalogs/{lib}.json \"{key}\": the first word is a category ({}), not \"{category}\"",
                Category::NAMES.join(", ")
            );
        }
        m.tags = words.collect();
        if !(2..=6).contains(&m.tags.len()) {
            bail!("assets/catalogs/{lib}.json \"{key}\": 2-6 tags after the category (has {})", m.tags.len());
        }
        m.category = Some(category);
        m.description = e.desc;
    }
    Ok(m)
}

/// A pack's models; `only`: the one file, or the folder's models, at that path inside it. A
/// `listed` source keeps only the models its catalog has a line for.
fn pack_source(a: &Args, lib: &str, pack: &str, only: Option<PathBuf>) -> Result<Src> {
    let src = libraries::source(lib, pack)?;
    let dir = libraries::pack_dir(lib, pack);
    if !dir.is_dir() {
        bail!("{lib}/{pack} is not downloaded: asset_import get={lib}/{pack}");
    }
    let text = |k: &str| src.get(k).and_then(Value::as_str);
    let glob = text("models").map(str::to_string);
    let all = models_in(&dir, glob.as_deref());
    let all_ids = ids(&all, glob.as_deref());
    // Names over the whole pack, so a part of it gets the names the whole would.
    let all_names = names(&all_ids);
    let cat = libraries::catalog(lib).unwrap_or_default();
    let all_meta: Vec<Meta> = all_ids.iter().map(|id| catalog_meta(&cat, lib, &format!("{pack}/{id}"))).collect::<Result<_>>()?;
    // A catalog line that names no model is a typo: the model it meant would go unlisted.
    let prefix = format!("{pack}/");
    let stray: Vec<&str> = cat
        .names()
        .chain(cat.skip.keys())
        .filter_map(|k| k.strip_prefix(prefix.as_str()))
        .filter(|id| !all_ids.iter().any(|x| x == id))
        .collect();
    if !stray.is_empty() {
        bail!("assets/catalogs/{lib}.json names models {lib}/{pack} does not have: {}", stray.join(", "));
    }
    let listed = src.get("listed").and_then(Value::as_bool).unwrap_or(false);
    let pool: Vec<usize> = (0..all.len()).filter(|&i| !listed || all_meta[i].listed).collect();
    let keep: Vec<usize> = pool.iter().copied().filter(|&i| only.as_ref().is_none_or(|f| all[i].starts_with(f))).collect();
    if keep.is_empty() {
        match &only {
            Some(f) => bail!(
                "{} holds none of {lib}/{pack}'s models{}",
                f.display(),
                glob.as_ref().map(|g| format!(" ({g}{})", if listed { ", listed" } else { "" })).unwrap_or_default()
            ),
            None => bail!("{}: no models{}", dir.display(), glob.map(|g| format!(" matching {g}")).unwrap_or_default()),
        }
    }
    let out_pack = text("pack").map(str::to_string).unwrap_or_else(|| format!("{lib}_{pack}"));
    if out_pack == "import" {
        bail!("{lib}/{pack}: the pack import holds loose files: give the source another \"pack\"");
    }
    if let Some(other) = libraries::pack_name_owner(lib, pack, &out_pack) {
        bail!("{lib}/{pack} and {other} both translate into the pack {out_pack}: give one of them another \"pack\"");
    }
    let pick = |v: &[String]| keep.iter().map(|&i| v[i].clone()).collect::<Vec<_>>();
    let source = match (text("url"), text("repo"), text("version")) {
        (Some(url), _, _) => url.to_string(),
        (None, Some(repo), Some(commit)) => format!("https://github.com/{repo} at {commit}"),
        _ => format!("{lib}/{pack}"),
    };
    Ok(Src {
        dir,
        files: keep.iter().map(|&i| all[i].clone()).collect(),
        ids: pick(&all_ids),
        names: pick(&all_names),
        meta: keep.iter().map(|&i| all_meta[i].clone()).collect(),
        norm: norm(a, Some(&src))?,
        base_norm: norm(&Args::new(), Some(&src))?,
        lib_pack: Some(format!("{lib}/{pack}")),
        partial: keep.len() < pool.len(),
        pack: out_pack,
        license: text("license").map(str::to_string),
        pack_info: Some(PackInfo {
            title: text("label").unwrap_or(pack).to_string(),
            credit: text("credit").or(text("origin")).unwrap_or_default().to_string(),
            source,
            catalog: format!("assets/catalogs/{lib}.json"),
            local: text("local").map(str::to_string),
        }),
        label: match &only {
            Some(f) => format!("{lib}/{pack}/{}", f.to_string_lossy().replace('\\', "/")),
            None => format!("{lib}/{pack}"),
        },
    })
}

/// A model id as a prop name: lowercase letters, digits, `_` and `-`, starting with a letter.
fn leaf(id: &str) -> String {
    let mut s: String = id.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    if !s.starts_with(|c: char| c.is_ascii_lowercase()) {
        s = format!("m_{s}");
    }
    s.chars().take(64).collect()
}

/// Each id's prop name, all distinct: ids that come out the same (`Foo`/`foo`, `a/b`/`a_b`)
/// would write over each other's asset and mesh. An id that already is its name keeps it; the
/// others take `_2`, `_3`... in id order, skipping names in use. The same files give the same
/// names.
fn names(ids: &[String]) -> Vec<String> {
    let base: Vec<String> = ids.iter().map(|id| leaf(id)).collect();
    let mut order: Vec<usize> = (0..ids.len()).collect();
    order.sort_by(|&a, &b| (base[a] != ids[a]).cmp(&(base[b] != ids[b])).then(ids[a].cmp(&ids[b])));
    let mut taken = std::collections::BTreeSet::new();
    let mut out = vec![String::new(); ids.len()];
    for i in order {
        let mut name = base[i].clone();
        let mut k = 2;
        while taken.contains(&name) {
            let tail = format!("_{k}");
            name = format!("{}{tail}", base[i].chars().take(64 - tail.len()).collect::<String>());
            k += 1;
        }
        taken.insert(name.clone());
        out[i] = name;
    }
    out
}

/// Where a source's translations go.
#[derive(Clone, Debug)]
pub struct Dest {
    pub dir: PathBuf,
    /// `lib` or `local` under the props root; None for `out=`.
    pub tier: Option<&'static str>,
    /// Why a pack goes to the local tier.
    pub why: Option<String>,
}

/// Where a pack's translations are written, their mesh files in `meshes/` beside them (plan
/// §8.7, §9.1): `out=` (a folder), else the licence gate's tier under the props root. A pinned
/// pack whose licence may be shared (CC0-1.0, CC-BY) goes to `lib/<pack>/` unless its catalog
/// keeps it local (`keep_local`, the reason); other known licences, and models outside a pinned
/// pack (no catalog licence), go to the git-ignored `local/<pack>/`; an unknown licence is
/// refused.
pub fn out_dir(pack: &str, license: Option<&str>, keep_local: Option<&str>, out: Option<&str>) -> Result<Dest> {
    if let Some(o) = out {
        // The library's folders are the licence gate's to fill.
        let inside = |dir: PathBuf| match (std::path::absolute(o), std::path::absolute(dir)) {
            (Ok(o), Ok(d)) => o.starts_with(d),
            _ => false,
        };
        let root = crate::asset_tools::root();
        if inside(root.join("lib")) || inside(root.join("templates")) {
            bail!("out={o} is inside the library: leave out= off and the pack's licence decides its folder");
        }
        return Ok(Dest { dir: PathBuf::from(o), tier: None, why: None });
    }
    pav_core::props::store::validate_pack(pack).map_err(|e| anyhow!("{e}"))?;
    let (tier, why) = match license {
        None => ("local", Some("no catalog licence (not a pinned pack)".to_string())),
        Some(l) => match licence::route(pack, l)? {
            Route::Local => ("local", Some(format!("licence {l} stays on this machine"))),
            Route::Shared => match keep_local {
                Some(reason) => ("local", Some(reason.to_string())),
                None => ("lib", None),
            },
        },
    };
    Ok(Dest { dir: crate::asset_tools::root().join(tier).join(pack), tier: Some(tier), why })
}

/// `fit=`: the translation mode, or None for a census only (`fit=false`).
fn fit_arg(a: &Args) -> Result<Option<Mode>> {
    match a.get("fit") {
        None | Some(Value::Bool(true)) => Ok(Some(Mode::Auto)),
        Some(Value::Bool(false)) => Ok(None),
        Some(v) => match v.as_str() {
            Some("false") => Ok(None),
            Some(s) => Ok(Some(Mode::parse(s)?)),
            None => bail!("fit=: auto, prims, mesh or false"),
        },
    }
}

/// One model read, censused and (with a mode) translated.
struct Done {
    cols: BTreeMap<&'static str, String>,
    error: Option<String>,
    translation: Option<Translation>,
    /// Why it is not translated: its catalog's `$skip` line, or a rigged model (the creature
    /// domain, plan §8.4).
    skip: Option<String>,
    /// Reading or translating it panicked (the pilot counts these; plan §8.8).
    panicked: bool,
}

impl Done {
    fn error(cols: BTreeMap<&'static str, String>, e: String) -> Done {
        Done { cols, error: Some(e), translation: None, skip: None, panicked: false }
    }
}

/// One model processed; a panic is caught and reported as that model's error.
fn process(src: &Src, i: usize, mode: Option<Mode>) -> Done {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process_one(src, i, mode))) {
        Ok(d) => d,
        Err(p) => {
            let what = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned());
            let mut d = Done::error(
                BTreeMap::from([("flags", "error".to_string())]),
                format!("panic: {}", what.unwrap_or_else(|| "unknown".into())),
            );
            d.panicked = true;
            d
        }
    }
}

fn process_one(src: &Src, i: usize, mode: Option<Mode>) -> Done {
    let (s, done) = match read(&src.dir.join(&src.files[i]), &src.norm) {
        Ok(r) => r,
        Err(e) => return Done::error(BTreeMap::from([("flags", "error".to_string())]), format!("{e:#}")),
    };
    let mut cols = census(&s, &done);
    let Some(mode) = mode else { return Done { cols, error: None, translation: None, skip: None, panicked: false } };
    let meta = &src.meta[i];
    let skip = meta.skip.clone().or_else(|| s.info.skinned.then(|| "rigged (skinned): rigs belong to the creature tools".into()));
    if skip.is_some() {
        cols.insert("tier", "skip".into());
        return Done { cols, error: None, translation: None, skip, panicked: false };
    }
    let id = &src.ids[i];
    let o = fit::Options {
        mode,
        pack: src.pack.clone(),
        name: src.names[i].clone(),
        category: meta.category.clone(),
        tags: meta.tags.clone(),
        description: meta.description.clone(),
        source: fit::Source {
            library: src.lib_pack.clone(),
            title: Some(id.clone()),
            license: src.license.clone(),
            url: src.lib_pack.is_none().then(|| src.dir.join(&src.files[i]).to_string_lossy().replace('\\', "/")),
            // Recorded when `check` would otherwise read the source another way (`up=y`: the
            // defaults, over a pack's pinned placement).
            norm: (src.norm != src.base_norm).then(|| Some(src.norm.words()).filter(|w| !w.is_empty()).unwrap_or("up=y".into())),
        },
    };
    match fit::translate(&s, &o) {
        Ok(t) => {
            let r2 = |x: f64| format!("{:.3}", x);
            cols.insert("parts", t.parts.to_string());
            cols.insert("mesh_pct", format!("{}", t.mesh_pct.round() as i64));
            cols.insert("iou", r2(t.fidelity.iou));
            cols.insert("iou_mean", r2(t.fidelity.iou_mean));
            cols.insert("de", format!("{:.1}", t.fidelity.de));
            cols.insert("chars", t.chars.to_string());
            cols.insert("tier", t.tier.to_string());
            Done { cols, error: None, translation: Some(t), skip: None, panicked: false }
        }
        Err(e) => {
            cols.insert("tier", "fail".into());
            Done { cols, error: Some(format!("{e:#}")), translation: None, skip: None, panicked: false }
        }
    }
}

/// Every model processed, on up to 8 threads (each model on one; the results in file order).
fn process_all(src: &Src, mode: Option<Mode>) -> Vec<Done> {
    let n = src.files.len();
    let workers = std::thread::available_parallelism().map_or(1, |p| p.get()).clamp(1, 8).min(n.max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<Done>>> = (0..n).map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let d = process(src, i, mode);
                    *slots[i].lock().unwrap() = Some(d);
                }
            });
        }
    });
    slots.into_iter().map(|s| s.into_inner().unwrap().expect("every model processed")).collect()
}

fn row_line(id: &str, d: &Done) -> String {
    let g = |k: &str| d.cols.get(k).map(String::as_str).unwrap_or("");
    match (&d.error, &d.translation) {
        (Some(e), _) => format!("{id}: {e}"),
        (None, Some(_)) => format!(
            "{id} {} iou {} mean {} de {} mesh {}% {} chars",
            g("tier"),
            g("iou"),
            g("iou_mean"),
            g("de"),
            g("mesh_pct"),
            g("chars")
        ),
        (None, None) if d.skip.is_some() => format!("{id} skipped: {}", d.skip.as_deref().unwrap_or_default()),
        (None, None) => format!("{id} {}t {}p {}cm {}", g("tris"), g("comps"), g("size_cm"), g("flags")).trim_end().to_string(),
    }
}

/// `x` to the nearest multiple of `q` (0.1, 0.001...), without binary noise in the reply.
fn rnd(x: f64, q: f64) -> f64 {
    let k = (1.0 / q).round();
    (x * k).round() / k
}

fn medianf(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    if v.is_empty() { 0.0 } else { v[v.len() / 2] }
}

/// What a set of translations came to: tiers, medians, the worst.
fn fit_summary(done: &[Done], seconds: f64) -> Value {
    let ts: Vec<&Translation> = done.iter().filter_map(|d| d.translation.as_ref()).collect();
    let mut tiers: BTreeMap<&str, usize> = BTreeMap::new();
    for t in &ts {
        *tiers.entry(t.tier).or_default() += 1;
    }
    let fits: Vec<&&Translation> = ts.iter().filter(|t| t.tier == "fit").collect();
    let r = rnd;
    json!({
        "translated": ts.len(),
        "skipped": done.iter().filter(|d| d.skip.is_some()).count(),
        "panics": done.iter().filter(|d| d.panicked).count(),
        "tiers": tiers,
        "editable": fits.iter().filter(|t| metrics::editable(t.mesh_pct)).count(),
        "iou": {"median": r(medianf(ts.iter().map(|t| t.fidelity.iou).collect()), 0.001), "min": r(ts.iter().map(|t| t.fidelity.iou).fold(1.0, f64::min), 0.001)},
        "iou_mean_median": r(medianf(ts.iter().map(|t| t.fidelity.iou_mean).collect()), 0.001),
        "de_median": r(medianf(ts.iter().map(|t| t.fidelity.de).collect()), 0.1),
        "mesh_pct_median": r(medianf(ts.iter().map(|t| t.mesh_pct).collect()), 1.0),
        "chars_median": medianf(ts.iter().map(|t| t.chars as f64).collect()),
        "tokens_median": medianf(ts.iter().map(|t| t.tokens as f64).collect()),
        "seconds": r(seconds, 0.01),
    })
}

/// The ledger's comment line.
const LEDGER_HEAD: &str = "# asset census and fit (asset_import survey): tris, materials used, textures, connected parts, size WxHxD cm, flags; parts (authored nodes), mesh_pct (area in the mesh part), iou (worst of 6 views) and iou_mean, iou_gpu (deferred), de (Oklab x100), chars, tier\n";

fn survey(a: &Args, what: &str) -> Result<Output> {
    let as_dir = PathBuf::from(what);
    if as_dir.is_dir() && get_str(a, "out").is_none() {
        // A plain folder is not a library's pack, so it has no place among the catalogs'
        // ledgers (plan §8.7): the caller names the file.
        bail!(
            "survey={what}: a folder's census needs out=<file>.tsv (a library's packs go to assets/catalogs/<lib>/<pack>.tsv); from={what} list=true shows the rows without writing"
        );
    }
    let src = source_of(a, what)?;
    let ledger_path = match (get_str(a, "out"), &src.lib_pack) {
        (Some(o), _) => PathBuf::from(o),
        // The pack's ledger has a row for every model: a part of it would drop the others'.
        (None, Some(lp)) if src.partial => {
            bail!("survey={what}: part of {lp}, whose ledger holds the whole pack: survey={lp}, or out=<file>.tsv for this part")
        }
        (None, Some(lp)) => {
            let (lib, pack) = lib_pack(lp)?;
            libraries::catalogs_dir().join(lib).join(format!("{pack}.tsv"))
        }
        (None, None) => bail!("survey={what}: needs out=<file>.tsv"),
    };
    let mode = fit_arg(a)?;
    let started = std::time::Instant::now();
    let read = process_all(&src, mode);
    let seconds = started.elapsed().as_secs_f64();
    let (ids, files) = (&src.ids, &src.files);
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
    let failed: Vec<String> = ids.iter().zip(&read).filter(|(_, r)| r.error.is_some()).map(|(id, r)| row_line(id, r)).collect();
    let ok: Vec<&Done> = read.iter().filter(|r| r.error.is_none()).collect();
    let num_of = |r: &Done, k: &str| r.cols.get(k).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    let tris: Vec<usize> = ok.iter().map(|r| num_of(r, "tris")).collect();
    let mut flag_count: BTreeMap<String, usize> = BTreeMap::new();
    for r in &ok {
        for f in r.cols.get("flags").map(String::as_str).unwrap_or("").split_whitespace() {
            *flag_count.entry(f.to_string()).or_default() += 1;
        }
    }
    let with_notes: Vec<(BTreeMap<&'static str, String>, String)> = ids
        .iter()
        .zip(files)
        .zip(&read)
        .map(|((id, f), r)| (r.cols.clone(), note_of(id, &f.to_string_lossy().replace('\\', "/"))))
        .collect();
    write_text(&ledger_path, &ledger_text(ids, files, &with_notes))?;
    let limit = get_u64(a, "limit", 20)? as usize;
    let lines: Vec<String> = ids.iter().zip(&read).filter(|(_, r)| r.error.is_none()).map(|(id, r)| row_line(id, r)).collect();
    let (failed_shown, failed_more) = (json!(failed.iter().take(limit).collect::<Vec<_>>()), failed.len().saturating_sub(limit));
    let mut out = json!({
        "survey": src.label,
        "models": files.len(),
        "read": ok.len(),
        "failed": failed_shown,
        "failed_more": failed_more,
        "tris": {"total": tris.iter().sum::<usize>(), "median": median(tris.clone()), "max": tris.iter().max().copied().unwrap_or(0)},
        "parts_median": median(ok.iter().map(|r| num_of(r, "comps")).collect()),
        "flags": flag_count,
        "ledger": ledger_path.display().to_string(),
        "rows": lines.iter().take(limit).collect::<Vec<_>>(),
        "rows_more": lines.len().saturating_sub(limit),
        "next": "from=<lib/pack|model> translates (writes props); check=<asset.json> compare=true shows one",
    });
    if mode.is_some() {
        out["fit"] = fit_summary(&read, seconds);
    }
    Ok(Output::Json(out))
}

// --- Translation -------------------------------------------------------------------------------

/// Writes a translation: the asset as `<dir>/<name>.json`, its mesh as `<dir>/meshes/<name>.json`
/// (a stale mesh file of the same name is removed). Returns the asset's path.
fn write_translation(dir: &Path, t: &Translation) -> Result<PathBuf> {
    let path = dir.join(format!("{}.json", t.doc.name));
    write_text(&path, &t.text)?;
    let mesh = dir.join("meshes").join(format!("{}.json", t.doc.name));
    match t.meshes.first() {
        Some((_, file)) => {
            write_text(&mesh, &file.text())?;
        }
        None => {
            let _ = std::fs::remove_file(&mesh);
        }
    }
    Ok(path)
}

fn translation_reply(t: &Translation, path: &Path) -> Value {
    let f = &t.fidelity;
    let r = rnd;
    let mut v = json!({
        "asset": t.doc.name,
        "file": path.display().to_string(),
        "tier": t.tier,
        "editable": t.tier == "fit" && metrics::editable(t.mesh_pct),
        "iou": r(f.iou, 0.001),
        "iou_mean": r(f.iou_mean, 0.001),
        "views": f.views.iter().map(|(n, x)| format!("{n} {:.3}", x)).collect::<Vec<_>>(),
        "de": r(f.de, 0.1),
        "mean_mm": r(f.mean_mm, 0.1),
        "worst_mm": r(f.worst_mm, 0.1),
        "mesh_pct": r(t.mesh_pct, 0.1),
        "parts": t.parts,
        "chars": t.chars,
        "tokens": t.tokens,
        "size_cm": t.compiled.bounds.size_cm(),
    });
    if !t.meshes.is_empty() {
        v["mesh"] = json!(
            t.meshes.iter().map(|(n, m)| format!("{n} {} tris {} colours", m.f.len(), m.palette.len())).collect::<Vec<_>>()
        );
    }
    if !t.findings.is_empty() {
        v["findings"] = json!(t.findings.iter().take(6).collect::<Vec<_>>());
    }
    v["next"] = json!(format!("asset_import check={} compare=true", path.display()));
    v
}

/// `compare=`: the source over its translation, written under .cache/asset_import/compare/.
fn compare_image(name: &str, source: &[Tri], fit_tris: &[Tri], meta: Value) -> Result<Output> {
    let png = metrics::compare_png(source, fit_tris);
    let path = fetch::cache_dir_for("assets").parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".cache"));
    let path = path.join("asset_import").join("compare").join(format!("{name}.png"));
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&path, &png).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    let mut meta = meta;
    meta["image"] =
        json!(format!("{}: top the source, bottom the translation; front, back, left, right, top, iso", path.display()));
    Ok(Output::Image { png, path: Some(path), meta })
}

/// The pack folder's `PACK.json` (plan §9.1), fields in its documented order.
fn pack_json(pack: &str, license: &str, p: &PackInfo) -> String {
    let fields = [("pack", pack), ("title", &p.title), ("license", license), ("credit", &p.credit), ("source", &p.source)];
    let mut lines: Vec<String> = fields.iter().map(|(k, v)| format!("  \"{k}\": {}", json!(v))).collect();
    lines.push(format!("  \"catalog\": {}", json!(p.catalog)));
    format!("{{\n{}\n}}\n", lines.join(",\n"))
}

/// After translations are written into a tier folder: a `lib/` pack's `PACK.json` (checked by
/// `asset_library::read_pack`); for a whole pack, the files of models it no longer writes and
/// its copy in the other tier removed (that copy's index notes kept); then the pack's index and
/// the credits, as `assets reindex=<pack>` writes them.
fn finish_tier(src: &Src, dest: &Dest, written: &[String]) -> Result<Value> {
    let root = crate::asset_tools::root();
    if dest.tier == Some("lib") {
        let (Some(info), Some(license)) = (&src.pack_info, &src.license) else { bail!("lib/{} needs a pinned pack", src.pack) };
        write_text(&dest.dir.join("PACK.json"), &pack_json(&src.pack, license, info))?;
        asset_library::read_pack(&dest.dir)?;
    }
    if !src.partial {
        let keep: std::collections::BTreeSet<String> = written.iter().map(|n| format!("{n}.json")).collect();
        for folder in [dest.dir.clone(), dest.dir.join("meshes")] {
            let Ok(entries) = std::fs::read_dir(&folder) else { continue };
            for path in entries.flatten().map(|e| e.path()) {
                let file = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                if path.is_file() && file.ends_with(".json") && file != "PACK.json" && !keep.contains(&file) {
                    std::fs::remove_file(&path)?;
                }
            }
        }
        let other = root.join(if dest.tier == Some("lib") { "local" } else { "lib" }).join(&src.pack);
        if other.is_dir() {
            let (from, to) = (other.join("index.tsv"), dest.dir.join("index.tsv"));
            if from.exists() && !to.exists() {
                std::fs::rename(&from, &to)?;
            }
            std::fs::remove_dir_all(&other)?;
            if dest.tier == Some("local") {
                // It left lib/: its credit goes too.
                asset_library::write_credits(&root)?;
            }
        }
    }
    crate::asset_search::reindex(&root, &src.pack)
}

fn translate_from(a: &Args, what: &str) -> Result<Output> {
    let src = source_of(a, what)?;
    let mode = fit_arg(a)?.ok_or_else(|| anyhow!("fit=false translates nothing: use list=true or survey="))?;
    let keep_local = src.pack_info.as_ref().and_then(|p| p.local.as_deref());
    let dest = out_dir(&src.pack, src.license.as_deref(), keep_local, get_str(a, "out"))?;
    // A lib/ pack credits the catalog's pinned source, so it is translated only from that.
    if let (Some("lib"), Some(lp)) = (dest.tier, &src.lib_pack) {
        let (lib, pack) = lib_pack(lp)?;
        let missing = libraries::unpinned(lib, pack)?;
        if !missing.is_empty() {
            bail!("{lp}: a lib/ pack pins its bytes and licence text: add {} to its source", missing.join(", "));
        }
        if !libraries::is_current(lib, pack)? {
            bail!("{lp}: the cached pack is not the one the catalog pins: asset_import get={lp} first");
        }
    }
    // A pack that moved tiers moves whole: a part of it would sit behind the old copy.
    if let (Some(tier), true) = (dest.tier, src.partial) {
        let other = crate::asset_tools::root().join(if tier == "lib" { "local" } else { "lib" }).join(&src.pack);
        if other.is_dir() && src.lib_pack.is_some() {
            bail!(
                "{}: the pack is in {} now and goes to {tier}/: translate the whole pack to move it",
                src.label,
                other.display()
            );
        }
    }
    let dir = dest.dir.clone();
    let started = std::time::Instant::now();
    let done = process_all(&src, Some(mode));
    let seconds = started.elapsed().as_secs_f64();
    // A model that errors keeps its last files (below), but only where they stay true: a pack
    // that moves tiers or changes licence is rebuilt only without errors.
    let errored: Vec<String> = src.names.iter().zip(&done).filter(|(_, d)| d.error.is_some()).map(|(n, _)| n.clone()).collect();
    if let (Some(tier), false, false) = (dest.tier, errored.is_empty(), src.partial) {
        let root = crate::asset_tools::root();
        let moves = root.join(if tier == "lib" { "local" } else { "lib" }).join(&src.pack).is_dir();
        let old = asset_library::read_pack(&dest.dir).ok().map(|p| p.license);
        let relicensed = old.is_some_and(|l| Some(l.as_str()) != src.license.as_deref());
        if moves || relicensed {
            bail!(
                "{}: {} did not translate, and the pack {}: fix them first (the rows say why)",
                src.label,
                errored.join(", "),
                if moves { "changes folder" } else { "changes licence" }
            );
        }
    }
    // Every asset in a lib/ pack carries the pack's licence (plan §9.5).
    if let (Some("lib"), Some(license)) = (dest.tier, src.license.as_deref()) {
        for t in done.iter().filter_map(|d| d.translation.as_ref()) {
            asset_library::source_allowed(t.doc.source.as_ref(), license).map_err(|e| anyhow!("{}: {e}", t.doc.name))?;
        }
    }
    // One model file; a whole pack of one model takes the folder path, which also prunes.
    if src.files.len() == 1 && (src.partial || src.lib_pack.is_none()) {
        let d = &done[0];
        if let Some(why) = &d.skip {
            bail!("{}: skipped: {why}", src.ids[0]);
        }
        let t = d.translation.as_ref().ok_or_else(|| anyhow!("{}", d.error.clone().unwrap_or_default()))?;
        // Like a folder's: a failed translation is not written (plan §8.6).
        if t.tier == "fail" {
            let f = &t.fidelity;
            let hint = if mode == Mode::Mesh { "" } else { "; fit=mesh keeps the whole model as a mesh part" };
            bail!(
                "{}: the translation fails (iou {:.3}, iou_mean {:.3}, de {:.1}, {} chars), so it is not written{hint}",
                src.ids[0],
                f.iou,
                f.iou_mean,
                f.de,
                t.chars
            );
        }
        let path = write_translation(&dir, t)?;
        let mut reply = translation_reply(t, &path);
        if dest.tier.is_some() {
            finish_tier(&src, &dest, std::slice::from_ref(&t.doc.name))?;
            reply["to"] = json!(dest.tier);
            if let Some(why) = &dest.why {
                reply["why"] = json!(why);
            }
        }
        if get_bool(a, "compare", false)? {
            let (s, _) = read(&src.dir.join(&src.files[0]), &src.norm)?;
            return compare_image(&t.doc.name, &raster::scene_tris(&s), &raster::prop_tris(&t.compiled), reply);
        }
        return Ok(Output::Json(reply));
    }
    let mut written = Vec::new();
    for d in &done {
        if let Some(t) = d.translation.as_ref().filter(|t| t.tier != "fail") {
            write_translation(&dir, t)?;
            written.push(t.doc.name.clone());
        }
    }
    let limit = get_u64(a, "limit", 20)? as usize;
    let lines: Vec<String> = src.ids.iter().zip(&done).map(|(id, d)| row_line(id, d)).collect();
    let mut reply = json!({
        "from": src.label,
        "dir": dir.display().to_string(),
        "written": written.len(),
        "fit": fit_summary(&done, seconds),
        "rows": lines.iter().take(limit).collect::<Vec<_>>(),
        "rows_more": lines.len().saturating_sub(limit),
        "next": format!("asset_import check={}/<name>.json compare=true; assets find=<words> tier=disk|local", dir.display()),
    });
    if dest.tier.is_some() {
        // A model that could not be read or translated this time keeps its last files; only a
        // model the pack no longer has, or one that now fails, loses them.
        let keep: Vec<String> = written.iter().chain(&errored).cloned().collect();
        reply["index"] = finish_tier(&src, &dest, &keep)?;
        reply["to"] = json!(dest.tier);
        if let Some(why) = &dest.why {
            reply["why"] = json!(why);
        }
    }
    Ok(Output::Json(reply))
}

/// The mesh names a document's parts use.
fn mesh_names(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            if let Some(Value::String(m)) = o.get("mesh") {
                out.push(m.clone());
            }
            for x in o.values() {
                mesh_names(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| mesh_names(x, out)),
        _ => {}
    }
}

/// `check=<asset.json>`: an asset that carries `source`, measured against that source again.
fn check(a: &Args, file: &str) -> Result<Output> {
    let path = PathBuf::from(file);
    let text = std::fs::read_to_string(&path).map_err(|e| anyhow!("check={file}: {e}"))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| anyhow!("check={file}: not JSON: {e}"))?;
    // Its mesh files sit in meshes/ beside it.
    let mut names = Vec::new();
    mesh_names(value.get("parts").unwrap_or(&Value::Null), &mut names);
    for name in names {
        let Some((_, leaf)) = name.split_once('/') else { continue };
        let f = path.parent().unwrap_or(Path::new(".")).join("meshes").join(format!("{leaf}.json"));
        if let Ok(t) = std::fs::read_to_string(&f) {
            let m = MeshFile::parse(&t).map_err(|e| anyhow!("{}: {e}", f.display()))?;
            pav_core::props::mesh::install(&name, m).map_err(|e| anyhow!(e))?;
        }
    }
    let (doc, compiled) = fit::read_back(&text)?;
    let source = doc.source.clone().ok_or_else(|| anyhow!("check={file}: the asset has no source to compare with"))?;
    let mut model = match (&source.library, &source.url) {
        (Some(lp), _) => source_of(a, lp)?,
        (None, Some(url)) => source_of(a, url)?,
        _ => bail!("check={file}: its source names no library or file"),
    };
    // Read the source as the import did: its recorded normalisation, under any up=, yaw=,
    // scale= or place= given here.
    if let Some(words) = &source.norm {
        let recorded = Norm::from_words(words).map_err(|e| anyhow!("check={file}: source.norm \"{words}\": {e}"))?;
        model.norm = norm(a, Some(&norm_map(&recorded)))?;
    }
    let i = match &source.title {
        Some(t) if model.files.len() > 1 => {
            model.ids.iter().position(|id| id == t).ok_or_else(|| anyhow!("{} has no model {t}", model.label))?
        }
        _ => 0,
    };
    let (s, _) = read(&model.dir.join(&model.files[i]), &model.norm)?;
    let src_tris = raster::scene_tris(&s);
    let fit_tris = raster::prop_tris(&compiled);
    let fidelity = metrics::compare(&src_tris, &fit_tris);
    let text = doc.text();
    let chars = text.chars().count();
    // The mesh part's share of the drawn area stands for its share of the source's.
    let area = |t: &[Tri]| t.iter().map(segment::tri_area).sum::<f64>();
    let mesh_area: f64 = compiled
        .parts
        .iter()
        .filter(|p| matches!(p.prim, pav_core::props::geom::Primitive::Mesh { .. }))
        .map(|p| {
            let one = pav_core::props::CompiledProp { parts: vec![p.clone()], ..Default::default() };
            area(&raster::prop_tris(&one))
        })
        .sum();
    let mesh_pct = if area(&src_tris) > 0.0 { (mesh_area / area(&src_tris) * 100.0).min(100.0) } else { 0.0 };
    let whole_mesh = compiled.parts.iter().all(|p| matches!(p.prim, pav_core::props::geom::Primitive::Mesh { .. }));
    let tier = metrics::tier(&fidelity, chars, whole_mesh);
    let r = rnd;
    let mut reply = json!({
        "asset": doc.name,
        "source": format!("{} {}", model.label, model.ids[i]),
        "tier": tier,
        "iou": r(fidelity.iou, 0.001),
        "iou_mean": r(fidelity.iou_mean, 0.001),
        "views": fidelity.views.iter().map(|(n, x)| format!("{n} {:.3}", x)).collect::<Vec<_>>(),
        "de": r(fidelity.de, 0.1),
        "mean_mm": r(fidelity.mean_mm, 0.1),
        "worst_mm": r(fidelity.worst_mm, 0.1),
        "mesh_pct": r(mesh_pct, 0.1),
        "chars": chars,
        "tokens": metrics::tokens(&text),
    });
    if let Some(stored) = &doc.fit {
        reply["stored"] = json!({"iou": stored.iou, "de": stored.de, "mesh_pct": stored.mesh_pct});
    }
    if get_bool(a, "compare", false)? {
        return compare_image(&doc.name, &src_tris, &fit_tris, reply);
    }
    Ok(Output::Json(reply))
}
