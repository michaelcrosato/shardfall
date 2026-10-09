//! Raw motion captures translated into readable clip sets (`pav_core::clips`): the importer
//! my-3D2dge built in Node (tools/anim-import.mjs, asf-amc.mjs, cmu.mjs), ported so this engine
//! translates open animation libraries with its own tools.
//!
//! - `readable`: the encoder: a body's measurements, key poses from captured points, fitting.
//! - `glb`: libraries in glTF binary (Quaternius, Mesh2Motion: Rigify and Unreal-style rigs).
//! - `acclaim`: the CMU database's skeleton and motion files (.asf, .amc).
//! - `bvh`: BioVision files (100STYLE; the MotionBuilder names Mixamo and LaFAN1 use).
//! - `takes`: moments cut out of long captures (loops cut at their best cycle).
//! - `fetch`: downloads, including single files out of a zip archive on the web.
//!
//! This module joins them: libraries in, a set out, with the catalog that names, tags and
//! credits each clip.

pub mod acclaim;
pub mod bvh;
pub mod fetch;
pub mod glb;
pub mod readable;
pub mod takes;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use pav_core::clips::{self, ClipSet};
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

use readable::{Cap, Rest, js_round};
use takes::{Find, Pick, Take};

/// JSON with objects in the order the file writes them (serde_json's own maps sort their keys;
/// a catalog's order is the order its moments are cut in).
#[derive(Clone, Debug)]
pub enum Json {
    Obj(Vec<(String, Json)>),
    Arr(Vec<Json>),
    Val(Value),
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Json, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Json;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON")
            }
            fn visit_bool<E>(self, v: bool) -> Result<Json, E> {
                Ok(Json::Val(v.into()))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Json, E> {
                Ok(Json::Val(v.into()))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Json, E> {
                Ok(Json::Val(v.into()))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Json, E> {
                Ok(Json::Val(v.into()))
            }
            fn visit_str<E>(self, v: &str) -> Result<Json, E> {
                Ok(Json::Val(v.into()))
            }
            fn visit_unit<E>(self) -> Result<Json, E> {
                Ok(Json::Val(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Json, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = s.next_element()? {
                    v.push(x);
                }
                Ok(Json::Arr(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Json, A::Error> {
                let mut v = Vec::new();
                while let Some((k, x)) = m.next_entry::<String, Json>()? {
                    v.push((k, x));
                }
                Ok(Json::Obj(v))
            }
        }
        d.deserialize_any(V)
    }
}

impl Json {
    pub fn entries(&self) -> &[(String, Json)] {
        match self {
            Json::Obj(v) => v,
            _ => &[],
        }
    }
    pub fn value(&self) -> Value {
        match self {
            Json::Obj(v) => Value::Object(v.iter().map(|(k, x)| (k.clone(), x.value())).collect()),
            Json::Arr(a) => Value::Array(a.iter().map(Json::value).collect()),
            Json::Val(v) => v.clone(),
        }
    }
}

/// A catalog, written by hand from watching each clip: what each clip is (`clip: [tags, what
/// the body does, the clip it was made from]`; the tags `loop` and `once` say whether it loops),
/// where each library came from (`$sources`), clips left out on purpose and why (`$skip`), and
/// for a capture database which take and stretch each clip is cut from (`$pick`:
/// `clip: [take, from, to, shortest cycle]`, seconds) and which database (`$library`: `cmu`,
/// the default, or `100style`).
#[derive(Default)]
pub struct Catalog {
    pub sources: Map<String, Value>,
    pub skip: BTreeMap<String, String>,
    pub pick: Vec<(String, Value)>,
    pub library: String,
    clips: BTreeMap<String, Value>,
}

/// A catalog's line for one clip.
pub struct Entry {
    pub tags: Vec<String>,
    pub desc: String,
    pub orig: Option<String>,
}

impl Catalog {
    pub fn parse(text: &str) -> Result<Catalog> {
        let clean: String = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let j: Json = serde_json::from_str(&clean)?;
        let mut c = Catalog { library: "cmu".into(), ..Default::default() };
        for (k, v) in j.entries() {
            match k.as_str() {
                "$sources" => c.sources = v.value().as_object().cloned().unwrap_or_default(),
                "$skip" => {
                    for (n, why) in v.entries() {
                        c.skip.insert(n.clone(), why.value().as_str().unwrap_or("").to_string());
                    }
                }
                "$pick" => c.pick = v.entries().iter().map(|(n, p)| (n.clone(), p.value())).collect(),
                "$library" => c.library = v.value().as_str().unwrap_or("cmu").to_lowercase(),
                _ if k.starts_with('$') => {}
                _ => {
                    c.clips.insert(k.clone(), v.value());
                }
            }
        }
        Ok(c)
    }

    pub fn read(path: &Path) -> Result<Catalog> {
        let text = std::fs::read_to_string(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        Catalog::parse(&text).map_err(|e| anyhow!("{}: {e}", path.display()))
    }

    /// Does it say anything about clips (a catalog was given)?
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty() && self.sources.is_empty() && self.pick.is_empty()
    }

    pub fn entry(&self, name: &str) -> Option<Entry> {
        let v = self.clips.get(name)?.as_array()?;
        let s = |i: usize| v.get(i).and_then(|x| x.as_str()).unwrap_or("").to_string();
        Some(Entry {
            tags: s(0).split_whitespace().map(str::to_string).collect(),
            desc: s(1),
            orig: Some(s(2)).filter(|o| !o.is_empty()),
        })
    }
}

/// How a set is made.
pub struct Options {
    pub set: String,
    pub title: Option<String>,
    pub credit: String,
    /// The key-pose budget: in-betweening stays within this many mm of the capture.
    pub tol: f64,
    /// Frames a second the captures are sampled at.
    pub fps: f64,
    /// Only these clips.
    pub only: Option<Vec<String>>,
    /// Clips whose names hold one of these keep a blade's direction (sword clips).
    pub blade: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            set: "SET".into(),
            title: None,
            credit: String::new(),
            tol: 30.0,
            fps: 30.0,
            only: None,
            blade: vec!["Sword".into()],
        }
    }
}

/// One library: its clips as captures, and how the set records it.
pub struct Library {
    pub id: String,
    pub file: String,
    pub rig: String,
    pub clips: Vec<Cap>,
    /// The clip its body is measured on (a T-pose; `_rest` for captures).
    pub rest: Option<String>,
    /// Its label, origin, license and url.
    pub about: Option<Map<String, Value>>,
}

/// A clip fitted into a set, for the report.
pub struct Done {
    pub name: String,
    pub keys: usize,
    pub mean: f64,
    pub max: f64,
}

/// Fits every library's clips into a set. A clip whose name is already taken (by an earlier
/// library) is left out, as are `$skip`ped ones; notes go to `log`.
pub fn assemble(libs: Vec<Library>, cat: &Catalog, o: &Options, log: &mut Vec<String>) -> (ClipSet, Vec<Done>) {
    let mut set = ClipSet {
        set: o.set.clone(),
        title: o.title.clone().unwrap_or_else(|| o.set.clone()),
        format: 1,
        credit: o.credit.clone(),
        legend: clips::LEGEND.iter().map(|s| s.to_string()).collect(),
        fps: o.fps as f32,
        ..Default::default()
    };
    let mut done = Vec::new();
    for mut lib in libs {
        let names = [
            lib.rest.as_deref(),
            Some("A_TPose"),
            Some("T-Pose"),
            Some("TPose"),
            Some("Rest Pose"),
            Some("Rest_Pose"),
            Some("_rest"),
        ];
        let rest_name = names.into_iter().flatten().find(|n| lib.clips.iter().any(|c| c.name == *n));
        let rest = Rest::measure(&lib.clips, rest_name);
        add_source(&mut set, &lib, &rest, cat, log);
        let mut kept = 0;
        for cap in lib.clips.iter_mut() {
            let name = cap.name.clone();
            if set.clips.contains_key(&name) || o.only.as_ref().is_some_and(|w| !w.contains(&name)) || name.starts_with('_') {
                continue;
            }
            if let Some(why) = cat.skip.get(&name) {
                log.push(format!("left out {name}: {why}"));
                continue;
            }
            let entry = cat.entry(&name);
            if entry.is_none() && !cat.is_empty() {
                log.push(format!("no catalog entry for {name}"));
            }
            done.push(fit_into(&mut set, cap, &rest, &lib.id, entry, o));
            kept += 1;
        }
        let shares: Vec<String> = rest.spine_w.iter().map(|w| w.to_string()).collect();
        log.push(format!("{}: {} rig, {kept} clips; spine shares {}, neck {}", lib.file, lib.rig, shares.join(" "), rest.neck_w));
    }
    (set, done)
}

/// Records where a library came from (and the body it was measured on) in the set.
pub fn add_source(set: &mut ClipSet, lib: &Library, rest: &Rest, cat: &Catalog, log: &mut Vec<String>) {
    if !cat.is_empty() && lib.about.is_none() {
        log.push(format!("no \"$sources\" entry for {}: the set will not say where its clips came from", lib.id));
    }
    let mut src = Map::new();
    src.insert("file".into(), lib.file.clone().into());
    src.insert("rig".into(), lib.rig.clone().into());
    for (k, v) in lib.about.iter().flatten() {
        src.insert(k.clone(), v.clone());
    }
    src.insert("rest".into(), rest.to_value());
    set.sources.insert(lib.id.clone(), Value::Object(src));
}

/// Fits one capture and puts it in the set under its own name.
pub fn fit_into(set: &mut ClipSet, cap: &mut Cap, rest: &Rest, src: &str, entry: Option<Entry>, o: &Options) -> Done {
    let blade = o.blade.iter().any(|b| !b.is_empty() && cap.name.contains(b.as_str()));
    let (mut tags, mut desc, mut orig) = (Vec::new(), String::new(), None);
    if let Some(e) = entry {
        // The tags `loop` and `once` say whether it loops, where the clip names do not.
        if e.tags.iter().any(|t| t == "loop") {
            cap.looping = true;
        }
        if e.tags.iter().any(|t| t == "once") {
            cap.looping = false;
        }
        tags = e.tags.into_iter().filter(|t| t != "loop" && t != "once").collect();
        desc = e.desc;
        orig = e.orig;
    }
    let f = readable::fit(rest, cap, o.tol, blade);
    let mut clip = readable::clip_of(cap, &f.keys, src);
    (clip.tags, clip.desc, clip.orig) = (tags, desc, orig);
    // A loop that walks or runs in place keeps how fast it travelled (percent of standing hip
    // height a second), so a player can match it to the ground; one that hardly moves has none.
    let dur = readable::dur_of(cap);
    clip.speed = cap
        .stride
        .filter(|_| cap.looping && dur > 0.0)
        .map(|s| js_round(s / rest.h0 * 100.0 / dur))
        .filter(|v| *v >= 10.0)
        .map(|v| v as f32);
    set.fit.insert(cap.name.clone(), [js_round(f.mean) as f32, js_round(f.max) as f32]);
    set.clips.insert(cap.name.clone(), clip);
    Done { name: cap.name.clone(), keys: f.keys.len(), mean: f.mean, max: f.max }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()
}

/// Libraries in glTF binary files (`ids`: each one's source id, else its file name; `rests`:
/// each one's rest-pose clip).
pub fn glb_libraries(
    files: &[PathBuf],
    ids: &[String],
    rests: &[String],
    cat: &Catalog,
    fps: f64,
    log: &mut Vec<String>,
) -> Result<Vec<Library>> {
    let mut out = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let lib = glb::read(f, fps)?;
        log.extend(lib.notes);
        let file = file_name(f);
        let id = ids.get(i).filter(|s| !s.is_empty()).cloned().unwrap_or_else(|| {
            let lower = file.to_lowercase();
            if lower.ends_with(".glb") { file[..file.len() - 4].to_string() } else { file.clone() }
        });
        out.push(Library {
            about: cat.sources.get(&id).and_then(|v| v.as_object()).cloned(),
            id,
            file,
            rig: lib.rig.into(),
            clips: lib.clips,
            rest: rests.get(i).filter(|s| !s.is_empty()).cloned(),
        });
    }
    Ok(out)
}

/// The ledger of every CMU take (anim/cmu/takes.tsv): its frame rate, the stretch where it
/// moves, its category, fit and description.
#[derive(Default)]
pub struct Ledger {
    pub cols: Vec<String>,
    pub rows: Vec<Vec<String>>,
    by_id: HashMap<String, usize>,
}

impl Ledger {
    pub fn parse(text: &str) -> Ledger {
        let mut lines = text.lines().filter(|l| !l.is_empty() && !l.starts_with('#'));
        let cols: Vec<String> = lines.next().map(|h| h.split('\t').map(str::to_string).collect()).unwrap_or_default();
        let rows: Vec<Vec<String>> = lines.map(|l| l.split('\t').map(str::to_string).collect()).collect();
        let by_id = rows.iter().enumerate().filter_map(|(i, r)| r.first().map(|id| (id.clone(), i))).collect();
        Ledger { cols, rows, by_id }
    }

    pub fn read(path: &Path) -> Ledger {
        std::fs::read_to_string(path).map(|t| Ledger::parse(&t)).unwrap_or_default()
    }

    /// A take's value in a column.
    pub fn get(&self, id: &str, col: &str) -> Option<&str> {
        let c = self.cols.iter().position(|x| x == col)?;
        self.rows.get(*self.by_id.get(id)?)?.get(c).map(String::as_str)
    }

    pub fn row(&self, i: usize) -> Map<String, Value> {
        self.cols.iter().zip(&self.rows[i]).map(|(c, v)| (c.clone(), Value::from(v.as_str()))).collect()
    }
}

/// The databases the importer cuts moments from.
pub const CMU: &str = "cmu";
pub const STYLE100: &str = "100style";

/// Where a library came from, for the sources of a set made with `mocap cut`.
pub fn about(library: &str) -> Map<String, Value> {
    let v = match library {
        CMU => serde_json::json!({
            "label": "CMU motion capture",
            "origin": "CMU Graphics Lab Motion Capture Database (mocap.cs.cmu.edu). The database was created with funding from NSF EIA-0196217.",
            "license": "free for all uses (may be copied, modified, or redistributed without permission)",
            "url": "http://mocap.cs.cmu.edu/",
        }),
        _ => serde_json::json!({
            "label": "100STYLE",
            "origin": "The 100STYLE dataset: one performer walking and running in 100 styles, captured by Ian Mason, Sebastian Starke and Taku Komura (Real-Time Style Modelling of Human Locomotion via Feature-Wise Transformations and Local Motion Phases, 2022)",
            "license": "CC BY 4.0 (credit required)",
            "url": "https://zenodo.org/records/8127870",
        }),
    };
    v.as_object().cloned().unwrap_or_default()
}

/// The database a take belongs to: CMU takes are SUBJECT_TRIAL (`13_29`), 100STYLE's
/// STYLE_GAIT (`Neutral_FW`).
pub fn library_of(take: &str) -> &'static str {
    match take.split_once('_') {
        Some((a, b)) if a.bytes().all(|c| c.is_ascii_digit()) && b.bytes().all(|c| c.is_ascii_digit()) => CMU,
        _ => STYLE100,
    }
}

const CMU_SITE: &str = "http://mocap.cs.cmu.edu";
pub const STYLE100_ZIP: &str = "https://zenodo.org/api/records/8127870/files/100STYLE.zip/content";

/// Downloads a CMU take and its subject's skeleton into `dir` (kept when already there).
/// Returns the skeleton's and the take's paths.
pub fn cmu_get(take: &str, dir: &Path) -> Result<(PathBuf, PathBuf)> {
    let Some((s, _)) = take.split_once('_').filter(|_| library_of(take) == CMU) else {
        bail!("a CMU take is SUBJECT_TRIAL, like 02_01: {take}");
    };
    let (asf, amc) = (dir.join(format!("{s}.asf")), dir.join(format!("{take}.amc")));
    fetch::get(&format!("{CMU_SITE}/subjects/{s}/{s}.asf"), &asf)?;
    fetch::get(&format!("{CMU_SITE}/subjects/{s}/{take}.amc"), &amc)?;
    Ok((asf, amc))
}

/// Downloads 100STYLE takes (`Neutral_FW`) into `dir`, out of the dataset's archive (only
/// their own bytes). Returns their paths.
pub fn style100_get(takes: &[String], dir: &Path) -> Result<Vec<PathBuf>> {
    let want: Vec<(String, PathBuf)> = takes.iter().map(|t| (t.clone(), dir.join(format!("{t}.bvh")))).collect();
    if want.iter().all(|(_, p)| p.exists()) {
        return Ok(want.into_iter().map(|(_, p)| p).collect());
    }
    let zip = fetch::RemoteZip::open(STYLE100_ZIP)?;
    std::fs::create_dir_all(dir)?;
    for (t, p) in &want {
        if p.exists() {
            continue;
        }
        let style = t.rsplit_once('_').map_or(t.as_str(), |(s, _)| s);
        let name = format!("100STYLE/{style}/{t}.bvh");
        let e = zip
            .entries
            .iter()
            .find(|e| e.name == name)
            .ok_or_else(|| anyhow!("100STYLE has no take {t} (styles hold BR BW FR FW ID SR SW TR1 TR2 TR3 ...)"))?;
        std::fs::write(p, zip.read(e)?)?;
    }
    Ok(want.into_iter().map(|(_, p)| p).collect())
}

/// 100STYLE's own notes on its styles (Dataset_List.csv: the instruction the performer was
/// given, whether the style is stochastic, so no one cycle holds all of it, and notes) and the
/// frames each take performs its style between, the walks in and out left out (Frame_Cuts.csv).
/// Fetched once out of the archive.
#[derive(Default)]
pub struct Styles {
    /// Style, description, stochastic, notes.
    pub about: Vec<(String, String, bool, String)>,
    /// Take (`Zombie_FW`) to its first and last frame.
    cuts: HashMap<String, (f64, f64)>,
}

/// One line of a CSV file (quoted fields allowed).
fn csv_line(l: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = l.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|f| f.trim().to_string()).collect()
}

impl Styles {
    pub fn load(dir: &Path) -> Result<Styles> {
        let files = ["Dataset_List.csv", "Frame_Cuts.csv"];
        if files.iter().any(|f| !dir.join(f).exists()) {
            let zip = fetch::RemoteZip::open(STYLE100_ZIP)?;
            std::fs::create_dir_all(dir)?;
            for f in files {
                let e = zip
                    .entries
                    .iter()
                    .find(|e| e.name == format!("100STYLE/{f}"))
                    .ok_or_else(|| anyhow!("100STYLE has no {f}"))?;
                std::fs::write(dir.join(f), zip.read(e)?)?;
            }
        }
        let read = |f: &str| -> Result<Vec<Vec<String>>> {
            let b = std::fs::read(dir.join(f))?;
            Ok(String::from_utf8_lossy(&b).lines().filter(|l| !l.trim().is_empty()).map(csv_line).collect())
        };
        let mut st = Styles::default();
        for r in read("Dataset_List.csv")?.into_iter().skip(1) {
            let g = |i: usize| r.get(i).cloned().unwrap_or_default();
            // Style Name, Description, Stochastic, Symmetric, Notes.
            st.about.push((g(0), g(1), g(2).eq_ignore_ascii_case("yes"), g(4)));
        }
        let rows = read("Frame_Cuts.csv")?;
        let head = rows.first().cloned().unwrap_or_default();
        for r in rows.iter().skip(1) {
            for (i, col) in head.iter().enumerate() {
                let Some(gait) = col.strip_suffix("_START") else { continue };
                let stop = head.iter().position(|c| *c == format!("{gait}_STOP"));
                let (a, b) = (
                    r.get(i).and_then(|v| v.parse::<f64>().ok()),
                    stop.and_then(|j| r.get(j)).and_then(|v| v.parse::<f64>().ok()),
                );
                if let (Some(a), Some(b), Some(style)) = (a, b, r.first()) {
                    st.cuts.insert(format!("{style}_{gait}"), (a, b));
                }
            }
        }
        Ok(st)
    }

    /// The seconds a take performs its style between.
    pub fn window(&self, take: &str, fps: f64) -> Option<(f64, f64)> {
        self.cuts.get(take).map(|(a, b)| (a / fps, b / fps))
    }
}

/// Every 100STYLE take's name, from the archive's list of files.
pub fn style100_takes(dir: &Path) -> Result<Vec<String>> {
    let list = dir.join("takes.txt");
    if let Ok(t) = std::fs::read_to_string(&list) {
        return Ok(t.lines().map(str::to_string).collect());
    }
    let zip = fetch::RemoteZip::open(STYLE100_ZIP)?;
    let mut names: Vec<String> = zip
        .entries
        .iter()
        .filter(|e| e.name.starts_with("100STYLE/") && e.name.ends_with(".bvh"))
        .filter_map(|e| e.name.rsplit('/').next().map(|n| n.trim_end_matches(".bvh").to_string()))
        .collect();
    names.sort();
    std::fs::create_dir_all(dir)?;
    std::fs::write(&list, names.join("\n"))?;
    Ok(names)
}

/// A pick from a catalog line `[take, from, to, shortest cycle]`; a take alone plays the
/// stretch where it moves (the ledger's `active`).
pub fn pick_of(name: &str, v: &Value, cat: &Catalog, ledger: &Ledger) -> Result<Pick> {
    let a = v.as_array().ok_or_else(|| anyhow!("$pick {name}: [take, from, to] expected"))?;
    let take = a.first().and_then(|t| t.as_str()).ok_or_else(|| anyhow!("$pick {name}: no take"))?.to_string();
    let num = |i: usize| a.get(i).and_then(|x| x.as_f64());
    let (mut from, mut to) = (num(1), num(2));
    let active = ledger.get(&take, "active").and_then(|s| {
        let (x, y) = s.split_once('-')?;
        Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))
    });
    if from.is_none() {
        if let Some((x, y)) = active {
            (from, to) = (Some(x), Some(y));
        }
    }
    let fps =
        ledger.get(&take, "fps").and_then(|f| f.parse().ok()).unwrap_or(if library_of(&take) == CMU { 120.0 } else { 60.0 });
    let looping = cat.entry(name).is_some_and(|e| e.tags.iter().any(|t| t == "loop"));
    let find = style100_find(&take, looping);
    let min_cycle = num(3).or(style100_min_cycle(&take, looping));
    Ok(Pick { name: name.to_string(), take, from, to, min_cycle, fps, looping, find })
}

/// A 100STYLE idle loops over its breathing and weight shifts, not a twitch: 2.5 s at least.
pub fn style100_min_cycle(take: &str, looping: bool) -> Option<f64> {
    (looping && library_of(take) == STYLE100 && take.ends_with("_ID")).then_some(2.5)
}

/// How a 100STYLE loop with no stretch given finds one: an idle (`_ID`) its still stretch, a
/// gait its longest straight run (the performer walks back and forth, turning at each end).
pub fn style100_find(take: &str, looping: bool) -> Option<Find> {
    if !looping || library_of(take) != STYLE100 {
        return None;
    }
    match take.rsplit_once('_').map(|(_, g)| g) {
        Some("ID") => Some(Find::Still),
        Some(g) if g.starts_with("TR") => None,
        _ => Some(Find::Straight),
    }
}

/// CMU subjects as libraries: the catalog's picks grouped by subject in the catalog's order,
/// each subject's takes downloaded into `dir` when missing.
pub fn cmu_libraries(
    picks: &[Pick],
    cat: &Catalog,
    ledger: &Ledger,
    dir: &Path,
    fps: f64,
    log: &mut Vec<String>,
) -> Result<Vec<Library>> {
    let mut groups: Vec<(String, Vec<Pick>)> = Vec::new();
    for p in picks {
        let s = p.take.split('_').next().unwrap_or("").to_string();
        match groups.iter_mut().find(|g| g.0 == s) {
            Some(g) => g.1.push(p.clone()),
            None => groups.push((s, vec![p.clone()])),
        }
    }
    let mut out = Vec::new();
    for (s, picks) in groups {
        let mut paths = HashMap::new();
        for p in &picks {
            if !paths.contains_key(&p.take) {
                paths.insert(p.take.clone(), cmu_get(&p.take, dir)?);
            }
        }
        let asf = dir.join(format!("{s}.asf"));
        let read = |p: &Path| {
            std::fs::read(p)
                .map(|b| b.iter().map(|&c| c as char).collect::<String>())
                .map_err(|e| anyhow!("{}: {e}", p.display()))
        };
        let subject = acclaim::Subject::parse(&read(&asf)?).map_err(|e| anyhow!("{s}.asf: {e}"))?;
        let mut takes: HashMap<String, Box<dyn Take + '_>> = HashMap::new();
        for (t, (_, amc)) in &paths {
            takes.insert(t.clone(), Box::new(subject.take(&read(amc)?).map_err(|e| anyhow!("{t}.amc: {e}"))?));
        }
        let clips = takes::cut(&subject.body, &takes, &picks, fps, log)?;
        let n: u64 = s.parse().unwrap_or(0);
        let id = format!("CMU_{n:02}");
        let about = cat.sources.get("CMU").and_then(|v| v.as_object()).map(|base| {
            let mut a = base.clone();
            let who = (0..ledger.rows.len())
                .map(|i| ledger.row(i))
                .find(|r| r.get("subject").and_then(|v| v.as_str()).and_then(|v| v.parse::<u64>().ok()) == Some(n))
                .and_then(|r| r.get("about").and_then(|v| v.as_str()).map(str::to_string))
                .filter(|w| !w.is_empty());
            let label = a.get("label").and_then(|v| v.as_str()).unwrap_or("CMU motion capture").to_string();
            let label = format!("{label}, subject {n}{}", who.map(|w| format!(" ({w})")).unwrap_or_default());
            a.insert("label".into(), label.into());
            a.insert("url".into(), format!("{CMU_SITE}/search.php?subjectnumber={n}").into());
            a
        });
        out.push(Library { id, file: format!("{s}.asf"), rig: "cmu".into(), clips, rest: Some("_rest".into()), about });
    }
    Ok(out)
}

/// BVH takes as one library (their skeleton is the first take's), each downloaded into `dir`
/// when missing (100STYLE) or read from `files`.
pub fn bvh_library(
    id: &str,
    picks: &[Pick],
    files: &HashMap<String, PathBuf>,
    about: Option<Map<String, Value>>,
    units: Option<f64>,
    fps: f64,
    log: &mut Vec<String>,
) -> Result<Library> {
    let mut rigged: Vec<(String, bvh::Rigged)> = Vec::new();
    for p in picks {
        if rigged.iter().any(|(t, _)| *t == p.take) {
            continue;
        }
        let path = files.get(&p.take).ok_or_else(|| anyhow!("{}: no file for take {}", p.name, p.take))?;
        let text = std::fs::read_to_string(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        let r = bvh::Rigged::new(bvh::Bvh::parse(&text).map_err(|e| anyhow!("{}: {e}", path.display()))?, units)?;
        rigged.push((p.take.clone(), r));
    }
    let Some(first) = rigged.first().map(|(_, r)| r) else { bail!("no takes") };
    let body = takes::Body { rest: first.body.rest.clone(), fwd: first.body.fwd, right: first.body.right };
    let (map, scale) = (first.map, first.scale);
    let picks: Vec<Pick> = picks
        .iter()
        .map(|p| {
            // A frame time written to six places (0.016667) is a whole frame rate.
            let fr = rigged.iter().find(|(t, _)| *t == p.take).map_or(p.fps, |(_, r)| 1.0 / r.bvh.frame_time);
            Pick { fps: if (fr - fr.round()).abs() < 0.01 { fr.round() } else { fr }, ..p.clone() }
        })
        .collect();
    let takes: HashMap<String, Box<dyn Take + '_>> =
        rigged.iter().map(|(t, r)| (t.clone(), Box::new(bvh::BvhTake(r)) as Box<dyn Take>)).collect();
    let clips = takes::cut(&body, &takes, &picks, fps, log)?;
    log.push(format!("{id}: the {map} map, {scale} m a unit"));
    Ok(Library { id: id.into(), file: format!("{id}.bvh"), rig: map.into(), clips, rest: Some("_rest".into()), about })
}
