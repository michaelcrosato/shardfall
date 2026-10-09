//! Animation tools: the motion clip library (`clips`), translating animation libraries into it
//! (`clip_import`: set files, glTF libraries, BVH takes, a capture database's picked moments),
//! finding and cutting single moments out of open motion-capture databases (`mocap`), and live
//! reloading of /anim (`anim_reload`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use pav_core::clips::{self, ClipSet};
use serde_json::{Value, json};

use crate::mocap::{self, Catalog, Done, Entry, Ledger, Library, Options, fetch, readable::Rest, takes::Pick};
use crate::session::Session;
use crate::tools::{Args, Output, get_bool, get_f32, get_str, get_u64};

/// The JSON object of a set file: a readable set as it is, or a my-3D2dge set script
/// (`(window.MOCAP = window.MOCAP || {})["NAME"] = { ... };` under a comment header).
fn set_json(text: &str) -> Result<&str> {
    let t = text.trim_start();
    if t.starts_with('{') {
        return Ok(t);
    }
    let start = text.find("window.MOCAP").and_then(|i| text[i..].find("= {").map(|j| i + j + 2));
    let end = text.rfind('}');
    match (start, end) {
        (Some(a), Some(b)) if b > a => Ok(&text[a..=b]),
        _ => bail!("not a clip set: expected a JSON object or a my-3D2dge set script"),
    }
}

fn read_set(path: &Path) -> Result<ClipSet> {
    let text = std::fs::read_to_string(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    ClipSet::parse(set_json(&text)?).map_err(|e| anyhow!("{}: {e}", path.display()))
}

fn anim_dir() -> PathBuf {
    pav_core::anim::disk_dir().unwrap_or_else(|| PathBuf::from("anim"))
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn list_arg(a: &Args, k: &str) -> Vec<String> {
    get_str(a, k).map(|c| c.split(',').map(|s| s.trim().to_string()).collect()).unwrap_or_default()
}

/// `clip_import`: translates an animation library into this engine's clip sets.
pub fn t_clip_import(_: &mut Session, a: &Args) -> Result<Output> {
    let from: Vec<PathBuf> = list_arg(a, "from").into_iter().filter(|s| !s.is_empty()).map(PathBuf::from).collect();
    let cat = match get_str(a, "catalog") {
        Some(c) => Catalog::read(Path::new(c))?,
        None => Catalog::default(),
    };
    if from.is_empty() {
        if !cat.pick.is_empty() {
            return import_picks(a, &cat);
        }
        bail!("from= a .glb, .fbx, .bvh or set file (or a folder of sets), or catalog= with \"$pick\"");
    }
    if from.iter().all(|p| ext(p) == "glb") {
        if get_bool(a, "list", false)? {
            return Ok(Output::Json(Value::Array(from.iter().map(|f| mocap::glb::list(f)).collect::<Result<_>>()?)));
        }
        return import_glb(a, &from, &cat);
    }
    if from.iter().all(|p| ext(p) == "bvh") {
        return import_bvh(a, &from, &cat);
    }
    if from.iter().all(|p| ext(p) == "fbx") {
        return import_fbx(a, &from, &cat);
    }
    import_sets(a, &from)
}

/// How a set is made, from the arguments (and the catalog's credits).
fn options(a: &Args, cat: &Catalog) -> Result<Options> {
    let set = get_str(a, "set").ok_or_else(|| anyhow!("set= names the set (MESH2MOTION, CMU ...)"))?.to_string();
    let credit = get_str(a, "credit").map(str::to_string).unwrap_or_else(|| {
        let lines: Vec<String> = cat
            .sources
            .values()
            .filter_map(|s| {
                let g = |k: &str| s.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
                (!g("label").is_empty()).then(|| format!("{} ({})", g("label"), g("license")))
            })
            .collect();
        lines.join("; ")
    });
    let only = get_str(a, "clips").map(|c| c.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
    Ok(Options {
        set,
        title: get_str(a, "title").map(str::to_string),
        credit,
        tol: get_f32(a, "tol", 30.0)? as f64,
        fps: get_f32(a, "fps", 30.0)? as f64,
        only,
        blade: get_str(a, "blade").map_or(vec!["Sword".into()], |b| b.split(',').map(|s| s.trim().to_string()).collect()),
    })
}

/// Writes a set where `out=` says (else anim/<set>.json) once it reads back.
fn write_set(set: &ClipSet, a: &Args) -> Result<(PathBuf, usize)> {
    write_set_to(set, &set_out(a, &set.set, false))
}

fn write_set_to(set: &ClipSet, out: &Path) -> Result<(PathBuf, usize)> {
    let text = set.to_text();
    let back = ClipSet::parse(&text).map_err(|e| anyhow!("{}: the translated set does not read back: {e}", set.set))?;
    if back.clips.len() != set.clips.len() {
        bail!("{}: clips lost in translation", set.set);
    }
    let out = out.to_path_buf();
    if let Some(d) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&out, &text)?;
    Ok((out, text.len()))
}

fn report(a: &Args, set: &ClipSet, done: &[Done], file: &Path, bytes: usize, log: Vec<String>) -> Output {
    let n = done.len().max(1) as f64;
    let mean = done.iter().map(|d| d.mean).sum::<f64>() / n;
    let worst = done.iter().max_by(|x, y| x.max.total_cmp(&y.max));
    let shown = get_u64(a, "log", 60).unwrap_or(60) as usize;
    Output::Json(json!({
        "set": set.set,
        "clips": done.len(),
        "key_poses": done.iter().map(|d| d.keys).sum::<usize>(),
        "bytes": bytes,
        "mean_fit_mm": (mean * 10.0).round() / 10.0,
        "worst": worst.map(|d| json!({ "clip": d.name, "mm": d.max.round() })),
        "file": file.display().to_string(),
        "log": log.iter().take(shown).collect::<Vec<_>>(),
        "log_more": log.len().saturating_sub(shown),
        "next": format!("anim_reload picks the set up; clips set={} lists it; animsheet clip={}/<clip> shows one", set.set, set.set),
    }))
}

/// glTF libraries (`sources=` each one's id, `rest=` each one's rest-pose clip).
fn import_glb(a: &Args, files: &[PathBuf], cat: &Catalog) -> Result<Output> {
    let o = options(a, cat)?;
    let mut log = Vec::new();
    let rm: Vec<PathBuf> = list_arg(a, "rm").into_iter().map(PathBuf::from).collect();
    let libs = mocap::glb_libraries(files, &list_arg(a, "sources"), &list_arg(a, "rest"), &rm, cat, o.fps, &mut log)?;
    let (set, done) = mocap::assemble(libs, cat, &o, &mut log);
    let (file, bytes) = write_set(&set, a)?;
    Ok(report(a, &set, &done, &file, bytes, log))
}

/// BVH files, each a take: the whole take (one file: `at=FROM-TO` seconds), looped when `loop=`
/// or its catalog entry says so.
fn import_bvh(a: &Args, files: &[PathBuf], cat: &Catalog) -> Result<Output> {
    if get_bool(a, "list", false)? {
        let mut out = Vec::new();
        for f in files {
            let b = mocap::bvh::Bvh::parse(&std::fs::read_to_string(f)?)?;
            let (frames, ft, joints) = (b.frames(), b.frame_time, b.joints().to_vec());
            let map = mocap::bvh::Rigged::new(b, None)
                .map(|r| json!({ "map": r.map, "metres_per_unit": r.scale }))
                .unwrap_or_else(|e| json!({ "map": e.to_string() }));
            out.push(json!({ "file": f.display().to_string(), "frames": frames, "fps": 1.0 / ft, "joints": joints, "rig": map }));
        }
        return Ok(Output::Json(Value::Array(out)));
    }
    let o = options(a, cat)?;
    let (from, to) = span(a)?;
    let mut picks = Vec::new();
    let mut paths = HashMap::new();
    for f in files {
        let take = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let name = match get_str(a, "name") {
            Some(n) if files.len() == 1 => n.to_string(),
            _ => take.clone(),
        };
        let looping = get_bool(a, "loop", cat.entry(&name).is_some_and(|e| e.tags.iter().any(|t| t == "loop")))?;
        let find = looping
            .then_some(way(a)?.map_or(mocap::takes::Find::Straight, mocap::takes::Find::Going))
            .filter(|_| from.is_none() && to.is_none());
        picks.push(Pick { name, take: take.clone(), from, to, min_cycle: min_cycle(a)?, fps: 0.0, looping, find });
        paths.insert(take, f.clone());
    }
    let id = list_arg(a, "sources").into_iter().next().filter(|s| !s.is_empty()).unwrap_or_else(|| "BVH".into());
    let about = cat.sources.get(&id).and_then(|v| v.as_object()).cloned();
    let units = get_str(a, "units").and_then(|u| u.parse().ok());
    let mut log = Vec::new();
    let lib = mocap::bvh_library(&id, &picks, &paths, about, units, o.fps, &mut log)?;
    let (set, done) = mocap::assemble(vec![lib], cat, &o, &mut log);
    let (file, bytes) = write_set(&set, a)?;
    Ok(report(a, &set, &done, &file, bytes, log))
}

/// FBX files: every animation stack a clip (`list=true`: their stacks and bones).
fn import_fbx(a: &Args, files: &[PathBuf], cat: &Catalog) -> Result<Output> {
    let fps = get_f32(a, "fps", 30.0)? as f64;
    if get_bool(a, "list", false)? {
        let mut out = Vec::new();
        for f in files {
            let stacks = mocap::fbx::read(f, fps)?;
            let names: Vec<String> = stacks.iter().map(|(n, b)| format!("{n} ({} frames)", b.frames())).collect();
            let joints = stacks.first().map(|(_, b)| b.joints().to_vec()).unwrap_or_default();
            let rig = match stacks.into_iter().next().map(|(_, b)| mocap::bvh::Rigged::new(b, None)) {
                Some(Ok(r)) => json!({ "map": r.map, "metres_per_unit": r.scale }),
                Some(Err(e)) => json!({ "map": e.to_string() }),
                None => Value::Null,
            };
            out.push(json!({ "file": f.display().to_string(), "stacks": names, "joints": joints, "rig": rig }));
        }
        return Ok(Output::Json(Value::Array(out)));
    }
    let o = options(a, cat)?;
    let ids = list_arg(a, "sources");
    let units = get_str(a, "units").and_then(|u| u.parse().ok());
    let mut log = Vec::new();
    let libs = mocap::fbx_libraries(files, &ids, cat, units, o.fps, &mut log)?;
    let (set, done) = mocap::assemble(libs, cat, &o, &mut log);
    let (file, bytes) = write_set(&set, a)?;
    Ok(report(a, &set, &done, &file, bytes, log))
}

/// `at=FROM-TO` (seconds).
fn span(a: &Args) -> Result<(Option<f64>, Option<f64>)> {
    let Some(at) = get_str(a, "at") else {
        return Ok((None, None));
    };
    let (x, y) = at.split_once('-').ok_or_else(|| anyhow!("at= is FROM-TO in seconds, like 1.5-2.6"))?;
    let num = |s: &str| -> Result<Option<f64>> {
        let s = s.trim();
        if s.is_empty() { Ok(None) } else { s.parse().map(Some).map_err(|_| anyhow!("at={at}: {s} is not a number")) }
    };
    Ok((num(x)?, num(y)?))
}

fn min_cycle(a: &Args) -> Result<Option<f64>> {
    Ok(a.get("min_cycle").map(|_| get_f32(a, "min_cycle", 0.5)).transpose()?.map(|v| v as f64))
}

/// `way=`: which way a loop's stretch travels, seen from the hips.
fn way(a: &Args) -> Result<Option<mocap::takes::Way>> {
    get_str(a, "way")
        .map(|w| mocap::takes::Way::parse(w).ok_or_else(|| anyhow!("way= is forward, back, left or right, not {w}")))
        .transpose()
}

/// A catalog's picked moments, cut from its database (`$library`: cmu or 100style), the takes
/// downloaded when missing.
fn import_picks(a: &Args, cat: &Catalog) -> Result<Output> {
    let o = options(a, cat)?;
    let ledger = Ledger::read(&anim_dir().join("cmu").join("takes.tsv"));
    let picks: Vec<Pick> = cat.pick.iter().map(|(n, v)| mocap::pick_of(n, v, cat, &ledger)).collect::<Result<_>>()?;
    let picks: Vec<Pick> = match &o.only {
        Some(w) => picks.into_iter().filter(|p| w.contains(&p.name)).collect(),
        None => picks,
    };
    let cache = fetch::cache_dir();
    let mut log = Vec::new();
    let picks = if cat.library == mocap::STYLE100 { styled(picks, &cache)? } else { picks };
    let libs = match cat.library.as_str() {
        mocap::CMU => mocap::cmu_libraries(&picks, cat, &ledger, &cache.join(mocap::CMU), o.fps, &mut log)?,
        lib @ (mocap::STYLE100 | mocap::BANDAI | mocap::LAFAN1) => {
            let about = cat.sources.values().next().and_then(|v| v.as_object()).cloned();
            vec![bvh_db_library(lib, &picks, about, o.fps, &mut log)?]
        }
        other => bail!("$library {other}: the databases are cmu, 100style, bandai and lafan1"),
    };
    let (set, done) = mocap::assemble(libs, cat, &o, &mut log);
    let (file, bytes) = write_set_to(&set, &set_out(a, &set.set, mocap::local_only(&cat.library)))?;
    Ok(report(a, &set, &done, &file, bytes, log))
}

/// 100STYLE picks with no stretch given play the frames the dataset marks as the style.
fn styled(picks: Vec<Pick>, cache: &Path) -> Result<Vec<Pick>> {
    let styles = mocap::Styles::load(&cache.join(mocap::STYLE100))?;
    Ok(picks
        .into_iter()
        .map(|p| match (p.from, p.to, styles.window(&p.take, 60.0)) {
            (None, None, Some((a, b))) => Pick { from: Some(a), to: Some(b), ..p },
            _ => p,
        })
        .collect())
}

/// A BVH database's picks as one library (100STYLE, Bandai Namco, LaFAN1), the takes downloaded
/// into the cache when missing.
fn bvh_db_library(
    library: &str,
    picks: &[Pick],
    about: Option<serde_json::Map<String, Value>>,
    fps: f64,
    log: &mut Vec<String>,
) -> Result<Library> {
    let dir = fetch::cache_dir().join(library);
    let mut takes: Vec<String> = picks.iter().map(|p| p.take.clone()).collect();
    takes.sort();
    takes.dedup();
    let files: HashMap<String, PathBuf> = takes.iter().cloned().zip(mocap::bvh_get(library, &takes, &dir)?).collect();
    let id = match library {
        mocap::BANDAI => "BANDAI_NAMCO",
        mocap::LAFAN1 => "LAFAN1",
        _ => "100STYLE",
    };
    mocap::bvh_library(id, picks, &files, Some(about.unwrap_or_else(|| mocap::about(library))), None, fps, log)
}

/// Where a set goes: `out=`, else anim/<set>.json (a library whose licence forbids sharing
/// what is made from it: anim/local, which is not committed).
fn set_out(a: &Args, set: &str, local: bool) -> PathBuf {
    get_str(a, "out").map(PathBuf::from).unwrap_or_else(|| {
        let dir = if local { anim_dir().join("local") } else { anim_dir() };
        dir.join(format!("{}.json", set.to_lowercase()))
    })
}

/// Set files (my-3D2dge set scripts or readable sets), or a folder of them.
fn import_sets(a: &Args, from: &[PathBuf]) -> Result<Output> {
    let from = from[0].clone();
    let folder = from.is_dir();
    let mut files: Vec<PathBuf> = if folder {
        std::fs::read_dir(&from)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "js" || x == "json"))
            .collect()
    } else {
        vec![from.clone()]
    };
    files.sort();
    if files.is_empty() {
        bail!("no set files in {}", from.display());
    }
    let anim = anim_dir();
    let out_dir = match get_str(a, "out") {
        Some(o) if folder => PathBuf::from(o),
        _ if folder => {
            anim.join(from.file_name().map(|n| n.to_string_lossy().to_lowercase().replace("-lib", "")).unwrap_or_default())
        }
        _ => anim.clone(),
    };
    std::fs::create_dir_all(&out_dir)?;
    let only: Option<Vec<String>> =
        get_str(a, "clips").map(|c| c.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
    let mut written = Vec::new();
    let mut skipped = Vec::new();
    let (mut n_clips, mut n_keys, mut bytes, mut fit_sum, mut fit_n) = (0usize, 0usize, 0usize, 0.0f32, 0usize);
    for f in &files {
        // A folder may hold other files beside its sets (an index): those are skipped, named.
        if folder {
            let text = std::fs::read_to_string(f)?;
            if set_json(&text).is_err() {
                skipped.push(f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                continue;
            }
        }
        let mut set = read_set(f)?;
        if !folder {
            if let Some(n) = get_str(a, "set") {
                set.set = n.to_string();
            }
        }
        if let Some(t) = get_str(a, "title") {
            set.title = t.to_string();
        } else if set.title.is_empty() {
            set.title = set.set.clone();
        }
        if let Some(c) = get_str(a, "credit") {
            set.credit = c.to_string();
        }
        if let Some(keep) = &only {
            set.clips.retain(|k, _| keep.iter().any(|w| w.eq_ignore_ascii_case(k)));
            set.fit.retain(|k, _| set.clips.contains_key(k));
            if set.clips.is_empty() {
                continue;
            }
        }
        set.legend = clips::LEGEND.iter().map(|s| s.to_string()).collect();
        set.format = 1;
        let text = set.to_text();
        // The translation must read back exactly.
        let back = ClipSet::parse(&text).map_err(|e| anyhow!("{}: the translated set does not read back: {e}", set.set))?;
        if back.clips.len() != set.clips.len() {
            bail!("{}: clips lost in translation", set.set);
        }
        let out = match (folder, get_str(a, "out")) {
            (false, Some(o)) => PathBuf::from(o),
            _ => out_dir.join(format!("{}.json", set.set.to_lowercase())),
        };
        std::fs::write(&out, &text)?;
        n_clips += set.clips.len();
        n_keys += set.clips.values().map(|c| c.keys.len()).sum::<usize>();
        bytes += text.len();
        for v in set.fit.values() {
            fit_sum += v[0];
            fit_n += 1;
        }
        written.push(json!({ "set": set.set, "clips": set.clips.len(), "file": out.display().to_string(), "bytes": text.len() }));
    }
    if let Some(l) = get_str(a, "ledger") {
        let to = out_dir.join("takes.tsv");
        std::fs::copy(l, &to).map_err(|e| anyhow!("ledger {l}: {e}"))?;
    }
    let shown: Vec<Value> = written.iter().take(12).cloned().collect();
    Ok(Output::Json(json!({
        "sets": written.len(),
        "clips": n_clips,
        "key_poses": n_keys,
        "bytes": bytes,
        "mean_fit_mm": if fit_n > 0 { (fit_sum as f64 / fit_n as f64 * 10.0).round() / 10.0 } else { 0.0 },
        "written": shown,
        "skipped": skipped,
        "next": if folder { "clips load=<folder name> adds them to this session's library" } else { "anim_reload picks the new set up (the build embeds it)" },
    })))
}

/// 100STYLE's gaits, by the code that ends a take's name.
const GAITS: [(&str, &str); 8] = [
    ("FW", "walking forward"),
    ("BW", "walking backward"),
    ("SW", "sidestepping"),
    ("FR", "running forward"),
    ("BR", "running backward"),
    ("SR", "running sideways"),
    ("ID", "standing idle"),
    ("TR", "transitions between gaits"),
];

/// `mocap`: open motion-capture databases: what they hold (`find=`), downloads (`get=`), and
/// one moment cut into a set (`cut=`).
pub fn t_mocap(_: &mut Session, a: &Args) -> Result<Output> {
    let cache = fetch::cache_dir();
    let ledger = Ledger::read(&anim_dir().join("cmu").join("takes.tsv"));
    let limit = get_u64(a, "limit", 30)? as usize;
    if let Some(words) = get_str(a, "find") {
        let words: Vec<String> = words.split_whitespace().map(|w| w.to_lowercase()).collect();
        let lib = get_str(a, "lib").unwrap_or("all").to_lowercase();
        let mut out = serde_json::Map::new();
        if lib == "all" || lib == mocap::CMU {
            let mut hits = Vec::new();
            let mut cats: HashMap<String, usize> = HashMap::new();
            for i in 0..ledger.rows.len() {
                let r = ledger.row(i);
                let g = |k: &str| r.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let hay = ["id", "desc", "about", "flags", "used", "note"]
                    .iter()
                    .map(|k| g(k))
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase();
                if words.iter().all(|w| g("category") == *w || hay.contains(w.as_str())) {
                    *cats.entry(g("category")).or_default() += 1;
                    hits.push(r);
                }
            }
            out.insert(
                "cmu".into(),
                json!({
                    "found": hits.len(),
                    "categories": cats,
                    "takes": hits.into_iter().take(limit).collect::<Vec<_>>(),
                    "columns": "sec: length; active: seconds where it moves; fit: average/worst mm in the readable format; travel: metres; hips: lowest-highest hip height, % of standing; used: clips of the CMU set cut from it",
                }),
            );
        }
        if lib == "all" || lib == mocap::STYLE100 {
            let dir = cache.join(mocap::STYLE100);
            match mocap::style100_takes(&dir).and_then(|t| Ok((t, mocap::Styles::load(&dir)?))) {
                Ok((takes, info)) => {
                    let mut styles: Vec<(String, Vec<String>)> = Vec::new();
                    for t in takes {
                        let Some((style, gait)) = t.rsplit_once('_') else { continue };
                        let about = info.about.iter().find(|a| a.0 == style).map(|a| a.1.to_lowercase()).unwrap_or_default();
                        if !words.iter().all(|w| {
                            style.to_lowercase().contains(w.as_str())
                                || about.contains(w.as_str())
                                || gait.eq_ignore_ascii_case(w)
                        }) {
                            continue;
                        }
                        match styles.iter_mut().find(|s| s.0 == style) {
                            Some(s) => s.1.push(gait.into()),
                            None => styles.push((style.into(), vec![gait.into()])),
                        }
                    }
                    let row = |s: &str, g: &[String]| {
                        let a = info.about.iter().find(|a| a.0 == s);
                        json!({
                            "style": s,
                            "desc": a.map(|a| a.1.clone()),
                            "varies": a.map(|a| a.2),
                            "notes": a.map(|a| a.3.clone()).filter(|n| !n.is_empty()),
                            "takes": g.iter().map(|g| format!("{s}_{g}")).collect::<Vec<_>>(),
                        })
                    };
                    out.insert(
                        "100style".into(),
                        json!({
                            "found": styles.len(),
                            "styles": styles.iter().take(limit).map(|(s, g)| row(s, g)).collect::<Vec<_>>(),
                            "gaits": GAITS.iter().map(|(c, w)| format!("{c}: {w}")).collect::<Vec<_>>(),
                            "varies": "the style varies at random (no one cycle holds all of it)",
                        }),
                    );
                }
                Err(e) => {
                    out.insert("100style".into(), json!({ "error": format!("{e:#}") }));
                }
            }
        }
        return Ok(Output::Json(Value::Object(out)));
    }
    if let Some(what) = get_str(a, "survey") {
        return survey(a, what, &ledger, &cache);
    }
    if let Some(ids) = get_str(a, "get") {
        let mut got = Vec::new();
        for id in ids.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if id.eq_ignore_ascii_case(mocap::CMU) {
                // The whole database.
                let mut log = Vec::new();
                let n = mocap::cmu::get_all(&cache.join(mocap::CMU), &ledger, &mut log)?;
                got.push(format!("{n} CMU takes in {} ({})", cache.join(mocap::CMU).display(), log.join("; ")));
            } else if id.eq_ignore_ascii_case("mesh2motion") {
                for f in ["human-base-animations.glb", "human-addon-animations.glb", "human-mocap-animations.glb"] {
                    let p = cache.join("mesh2motion").join(f);
                    fetch::get(
                        &format!("https://raw.githubusercontent.com/Mesh2Motion/mesh2motion-app/main/static/animations/{f}"),
                        &p,
                    )?;
                    got.push(p.display().to_string());
                }
            } else if mocap::library_of(id) == mocap::CMU {
                let (asf, amc) = mocap::cmu_get(id, &cache.join(mocap::CMU))?;
                got.extend([asf.display().to_string(), amc.display().to_string()]);
            } else {
                let lib = mocap::library_of(id);
                for p in mocap::bvh_get(lib, &[id.to_string()], &cache.join(lib))? {
                    got.push(p.display().to_string());
                }
            }
        }
        return Ok(Output::Json(json!({ "downloaded": got, "cache": cache.display().to_string() })));
    }
    if let Some(take) = get_str(a, "cut") {
        return cut(a, take, &ledger, &cache);
    }
    let cached = |sub: &str| std::fs::read_dir(cache.join(sub)).map(|d| d.count()).unwrap_or(0);
    Ok(Output::Json(json!({
        "databases": [
            {
                "name": "CMU",
                "what": "the CMU Graphics Lab Motion Capture Database: 2,548 takes by 113 people, about ten hours: walks, runs, sports, dances, martial arts, acrobatics, everyday actions, animals and characters",
                "license": "free for all uses (may be copied, modified, or redistributed without permission)",
                "ledger": "anim/cmu/takes.tsv: every take with its category, length, the stretch where it moves and how well it converts",
                "files_cached": cached(mocap::CMU),
                "use": "mocap find=kick lib=cmu, then mocap cut=13_17 at=1.5-2.6 name=Boxing_Jab set=MINE (a loop: loop=true finds its best cycle in the stretch)",
                "whole": "mocap get=cmu downloads every take (the site's 1.08 GB archive, unpacked); mocap survey=ledger measures them all into the ledger; mocap survey=library translates them into a set a subject (anim/cmu, 50 mm, for browsing; subjects=5,13 for some)",
            },
            {
                "name": "100STYLE",
                "what": "one performer walking, running, sidestepping and idling in 100 styles (old, zombie, proud, drunk, robot ...): 810 takes",
                "license": "CC BY 4.0 (credit required: every set records it)",
                "files_cached": cached(mocap::STYLE100),
                "use": "mocap find=zombie lib=100style, then mocap cut=Zombie_FW loop=true name=Zombie_Walk set=MINE (a take is fetched alone out of the 1.5 GB archive)",
                "sets": "STYLE100 (anim/style100.json: the forward walks, embedded); anim/100style: runs, backward walks and runs, sidesteps and sideways runs each way, idles (clips load=100style; catalogs anim/catalogs/100style_*.json)",
            },
            {
                "name": "Bandai Namco Research motion dataset",
                "what": "three professional actors walking, running, dashing, gesturing (bow, wave, guide, call), fighting (kick, punch, slash) and dancing in 15 styles: dataset 1, 175 takes (dataset 2: 2,902 more, mostly walks and hand actions)",
                "license": "CC BY-NC 4.0: non-commercial only, so translated into anim/local (git-ignored), never committed",
                "files_cached": cached(mocap::BANDAI),
                "use": "clip_import catalog=anim/catalogs/bandai.json set=BANDAI_NAMCO_1 (every take of dataset 1, into anim/local), or mocap cut=dataset-1_walk_happy_001 loop=true",
            },
            {
                "name": "LaFAN1",
                "what": "Ubisoft La Forge's 77 sequences by 5 subjects, 4.6 hours: walks, runs, sprints, jumps, crawls, falls and getting up, fights, dances, obstacle courses",
                "license": "CC BY-NC-ND 4.0: non-commercial, nothing made from it shared, so translated into anim/local (git-ignored), never committed",
                "files_cached": cached(mocap::LAFAN1),
                "use": "clip_import catalog=anim/catalogs/lafan1.json set=LAFAN1 (a loop out of every walk, run and sprint), or mocap cut=fallAndGetUp1_subject1 at=12-16 (a take alone out of the 144 MB archive)",
            },
            {
                "name": "Mixamo",
                "what": "Adobe's character animation library: thousands of moves on a MotionBuilder-named rig (mixamorig:Hips ...), downloaded by hand as FBX (or glTF)",
                "license": "free to use in projects, not to redistribute: translate your downloads into anim/local",
                "use": "clip_import from=Walking.fbx,Jab.fbx set=MIXAMO out=anim/local/mixamo.json (every animation stack a clip; .glb works too)",
            },
            {
                "name": "Mesh2Motion",
                "what": "177 human animations (Quaternius' library re-exported, hand-animated climbs, dodges, dances, emotes, mocopi captures), CC0",
                "files_cached": cached("mesh2motion"),
                "use": "mocap get=mesh2motion, then clip_import from=<the three .glb> sources=M2M_QUATERNIUS,M2M_ANIMATED,M2M_MOCOPI catalog=anim/catalogs/mesh2motion.json set=MESH2MOTION",
            },
            {
                "name": "Quaternius",
                "what": "the Universal Animation Libraries 1 and 2 (Standard editions free, Pro and Source paid; all CC0)",
                "use": "download from quaternius.com (itch.io), then clip_import from=UAL1.glb,UAL2.glb sources=UAL1,UAL2 catalog=anim/catalogs/quaternius.json set=QUATERNIUS",
            },
        ],
        "formats": "clip_import reads .glb (Rigify, Unreal-style and Mixamo rigs), .fbx (binary, version 7: Mixamo, Blender, Unreal, Maya exports), .bvh (100STYLE, MotionBuilder names: Mixamo, LaFAN1; Bandai Namco; Unreal and Rigify names), CMU's .asf/.amc and readable set files",
        "cache": cache.display().to_string(),
    })))
}

/// `mocap survey=ledger|library`: every downloaded CMU take (`mocap get=cmu`) measured into the
/// ledger, or translated into a set a subject for browsing (anim/cmu/cmu_NN.json).
fn survey(a: &Args, what: &str, ledger: &Ledger, cache: &Path) -> Result<Output> {
    let lib = match what {
        "ledger" => false,
        "library" => true,
        _ => bail!(
            "survey=ledger measures every take into anim/cmu/takes.tsv; survey=library translates them, a set a subject, into anim/cmu"
        ),
    };
    // `subjects=5` arrives as a number, `subjects=5,13` as text.
    let subjects: Option<Vec<u32>> = a
        .get("subjects")
        .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string))
        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect());
    let tol = get_f32(a, "tol", if lib { 50.0 } else { 30.0 })? as f64;
    let cat = Catalog::read(&anim_dir().join("catalogs").join("cmu.json")).unwrap_or_default();
    let used = mocap::cmu::used_by(&cat);
    let dir = cache.join(mocap::CMU);
    let mut log = Vec::new();
    let s = mocap::cmu::survey(&dir, ledger, &used, subjects.as_deref(), !lib, tol, lib.then_some(tol), &mut log)?;
    if s.takes == 0 {
        bail!("no CMU takes in {} (mocap get=cmu downloads them all)", dir.display());
    }
    let hours = (s.seconds / 360.0).round() / 10.0;
    if !lib {
        let path = get_str(a, "out").map(PathBuf::from).unwrap_or_else(|| anim_dir().join("cmu").join("takes.tsv"));
        mocap::cmu::write_ledger(&s.rows, &path)?;
        return Ok(Output::Json(json!({
            "measured": s.takes,
            "hours": hours,
            "rows": s.rows.len(),
            "file": path.display().to_string(),
            "log": log,
        })));
    }
    let out = get_str(a, "out").map(PathBuf::from).unwrap_or_else(|| anim_dir().join("cmu"));
    std::fs::create_dir_all(&out)?;
    let (mut bytes, mut clips) = (0, 0);
    for set in &s.sets {
        let text = set.to_text();
        ClipSet::parse(&text).map_err(|e| anyhow!("{}: the translated set does not read back: {e}", set.set))?;
        std::fs::write(mocap::cmu::set_path(&out, set), &text)?;
        bytes += text.len();
        clips += set.clips.len();
    }
    Ok(Output::Json(json!({
        "takes": s.takes,
        "hours": hours,
        "sets": s.sets.len(),
        "clips": clips,
        "bytes": bytes,
        "tol_mm": tol,
        "dir": out.display().to_string(),
        "log": log,
        "next": "anim_reload picks the sets up; clips load=cmu lists them",
    })))
}

/// `mocap cut=TAKE`: one moment of a database take, fitted into a set (created, or added to).
fn cut(a: &Args, take: &str, ledger: &Ledger, cache: &Path) -> Result<Output> {
    let lib = mocap::library_of(take);
    let (from, to) = span(a)?;
    let looping = get_bool(a, "loop", false)?;
    let name =
        get_str(a, "name").map(str::to_string).unwrap_or_else(|| if looping { format!("{take}_Loop") } else { take.to_string() });
    let set_name = get_str(a, "set").unwrap_or("MOCAP").to_string();
    let fps = get_f32(a, "fps", 30.0)? as f64;
    let find = match way(a)? {
        Some(w) => Some(mocap::takes::Find::Going(w)),
        None => mocap::style100_find(take, looping),
    };
    let min_cycle = min_cycle(a)?.or(mocap::style100_min_cycle(take, looping));
    let mut pick = Pick { name: name.clone(), take: take.to_string(), from, to, min_cycle, fps: 120.0, looping, find };
    let mut log = Vec::new();
    let mut library = if lib == mocap::CMU {
        pick.fps = ledger.get(take, "fps").and_then(|f| f.parse().ok()).unwrap_or(120.0);
        if from.is_none() && to.is_none() {
            // A take alone: the stretch where it moves.
            if let Some((x, y)) = ledger.get(take, "active").and_then(|s| s.split_once('-')) {
                (pick.from, pick.to) = (x.parse().ok(), y.parse().ok());
            }
        }
        let mut cat = Catalog::default();
        cat.sources.insert("CMU".into(), Value::Object(mocap::about(mocap::CMU)));
        mocap::cmu_libraries(&[pick], &cat, ledger, &cache.join(mocap::CMU), fps, &mut log)?.remove(0)
    } else {
        let pick = if lib == mocap::STYLE100 { styled(vec![pick], cache)?.remove(0) } else { pick };
        bvh_db_library(lib, &[pick], None, fps, &mut log)?
    };
    let path = set_out(a, &set_name, mocap::local_only(lib));
    let mut set = if path.exists() {
        read_set(&path)?
    } else {
        ClipSet {
            set: set_name.clone(),
            title: get_str(a, "title").unwrap_or(&set_name).to_string(),
            format: 1,
            credit: "Moments cut from open motion-capture databases; each clip's source names its origin and license.".into(),
            legend: clips::LEGEND.iter().map(|s| s.to_string()).collect(),
            fps: fps as f32,
            ..Default::default()
        }
    };
    // Every clip of a source is measured on one body: an existing source keeps its own.
    let existing: Option<Rest> =
        set.sources.get(&library.id).and_then(|s| s.get("rest")).and_then(|r| serde_json::from_value(r.clone()).ok());
    let rest = match existing {
        Some(r) => r,
        None => {
            let r = Rest::measure(&library.clips, Some("_rest"));
            mocap::add_source(&mut set, &library, &r, &Catalog::default(), &mut log);
            r
        }
    };
    let o = Options { set: set.set.clone(), tol: get_f32(a, "tol", 30.0)? as f64, fps, ..Options::default() };
    let entry = Entry {
        tags: get_str(a, "tags").map(|t| t.split_whitespace().map(str::to_string).collect()).unwrap_or_default(),
        desc: get_str(a, "desc").unwrap_or("").to_string(),
        orig: None,
    };
    let id = library.id.clone();
    let cap = library.clips.iter_mut().find(|c| c.name == name).ok_or_else(|| anyhow!("nothing was cut"))?;
    let done = mocap::fit_into(&mut set, cap, &rest, &id, Some(entry), &o);
    let take_span = set.clips.get(&name).and_then(|c| c.take.clone());
    let text = set.to_text();
    ClipSet::parse(&text).map_err(|e| anyhow!("the set does not read back: {e}"))?;
    std::fs::write(&path, &text)?;
    Ok(Output::Json(json!({
        "clip": format!("{}/{}", set.set, name),
        "take": take_span,
        "seconds": set.clips.get(&name).map(|c| c.dur),
        "loop": looping,
        "key_poses": done.keys,
        "fit_mm": { "mean": (done.mean * 10.0).round() / 10.0, "worst": done.max.round() },
        "file": path.display().to_string(),
        "log": log,
        "next": format!("anim_reload, then animsheet clip={}/{} to see it", set.set, name),
    })))
}

fn clip_row(lib: &clips::ClipLib, id: u32) -> Value {
    let c = lib.get(id).unwrap();
    json!({ "name": lib.name_of(id), "seconds": c.dur, "loop": c.looping, "tags": c.tags, "desc": c.desc })
}

/// `clips`: the motion clip library.
pub fn t_clips(_: &mut Session, a: &Args) -> Result<Output> {
    if let Some(sub) = get_str(a, "load") {
        let (sets, added) = clips::load_folder(sub).map_err(|e| anyhow!(e))?;
        return Ok(Output::Json(json!({ "loaded": sub, "sets": sets, "clips": added, "library": clips::library().len() })));
    }
    let lib = clips::library();
    if let Some(n) = get_str(a, "name") {
        let id = lib.find(n).ok_or_else(|| anyhow!("no clip '{n}' (clips find=WORDS searches)"))?;
        let c = lib.get(id).unwrap();
        let full = lib.name_of(id).unwrap_or_default();
        let set = lib.sets.iter().find(|s| full.starts_with(&format!("{}/", s.set))).unwrap();
        let src = set.sources.get(&c.src).cloned().unwrap_or(Value::Null);
        return Ok(Output::Json(json!({
            "name": full,
            "seconds": c.dur,
            "loop": c.looping,
            "travels": c.travels(),
            "tags": c.tags,
            "desc": c.desc,
            "keys": c.keys.len(),
            "speed": c.speed,
            // When a one-off's strike lands: what a captured attack is timed by.
            "strike": (!c.looping).then(|| clips::strike_time(id)).flatten().map(|t| (t * 1000.0).round() / 1000.0),
            "fit_mm": set.fit.get(&c.clip),
            "source": { "label": src.get("label"), "license": src.get("license"), "url": src.get("url"), "origin": src.get("origin") },
            "credit": set.credit,
            "legend": clips::LEGEND,
            "text": c.text(),
        })));
    }
    let limit = get_u64(a, "limit", 40)? as usize;
    if let Some(words) = get_str(a, "find") {
        let words: Vec<String> = words.split_whitespace().map(|w| w.to_lowercase()).collect();
        let mut hits = Vec::new();
        for s in &lib.sets {
            for (k, c) in &s.clips {
                let hay = format!("{} {} {} {}", s.set, k, c.tags.join(" "), c.desc).to_lowercase();
                if words.iter().all(|w| hay.contains(w)) {
                    hits.push(clips::clip_id(&s.set, k));
                }
            }
        }
        let total = hits.len();
        return Ok(Output::Json(json!({
            "found": total,
            "clips": hits.iter().take(limit).map(|id| clip_row(&lib, *id)).collect::<Vec<_>>(),
        })));
    }
    if let Some(name) = get_str(a, "set") {
        let s = lib.sets.iter().find(|s| s.set.eq_ignore_ascii_case(name)).ok_or_else(|| anyhow!("no set '{name}'"))?;
        return Ok(Output::Json(json!({
            "set": s.set,
            "title": s.title,
            "credit": s.credit,
            "clips": s.clips.keys().map(|k| clip_row(&lib, clips::clip_id(&s.set, k))).collect::<Vec<_>>(),
        })));
    }
    let on_disk = pav_core::anim::disk_dir()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir() && e.file_name() != "catalogs")
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(Output::Json(json!({
        "clips": lib.len(),
        "sets": lib.sets.iter().map(|s| json!({ "set": s.set, "title": s.title, "clips": s.clips.len(), "credit": s.credit })).collect::<Vec<_>>(),
        "folders_on_disk": on_disk,
        "use": "find=WORDS searches names, tags and descriptions; name=SET/Clip shows one as readable key poses; animsheet clip=NAME renders it; load=FOLDER adds an on-disk library; mocap finds and cuts more from open motion-capture databases",
    })))
}

/// `anim_reload`: re-reads anim/ (moves and clip sets) from disk.
pub fn t_anim_reload(_: &mut Session, _: &Args) -> Result<Output> {
    let summary = pav_core::anim::reload(true).map_err(|e| anyhow!(e))?;
    Ok(Output::Json(json!({ "reloaded": summary })))
}
