//! The whole CMU motion capture database: every take downloaded (`get_all`), measured into the
//! ledger (`survey`: where each take moves, how far it travels, how high and low the hips go,
//! whether it turns upside down or keeps off the floor, how well it converts) and translated into
//! one set per subject (`survey` with a library tolerance: whole takes, a long one in 10 s parts,
//! fitted a little looser, for browsing). A port of `cmuSurvey` in my-3D2dge's tools/cmu.mjs.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use pav_core::clips::{self, ClipSet};
use serde_json::{Map, Value};

use super::readable::{self, ANKLE, BALL, Cap, HEAD_TOP, P, PELVIS, Rest, TOE, js_round, side};
use super::takes::{self, Pick, Take, to_fixed};
use super::{Ledger, acclaim, fetch};

/// The site's archive of every take but the latest subject's.
pub const ARCHIVE: &str = "http://mocap.cs.cmu.edu/allasfamc.zip";
const SITE: &str = "http://mocap.cs.cmu.edu";

fn is_take_file(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    (ext == "asf" || ext == "amc")
        && match stem.split_once('_') {
            Some((a, b)) => digits(a) && digits(b),
            None => digits(stem),
        }
}

/// Every take into `dir`: the site's archive (1.08 GB, 2,514 takes) unpacked flat, then what the
/// ledger lists that the archive lacks (the subject added after it), one by one. What is already
/// there is kept, so it resumes. Returns how many takes are on disk.
pub fn get_all(dir: &Path, ledger: &Ledger, log: &mut Vec<String>) -> Result<usize> {
    std::fs::create_dir_all(dir)?;
    let have = |id: &str| dir.join(format!("{id}.amc")).exists();
    let ids: Vec<&str> = ledger.rows.iter().filter_map(|r| r.first().map(String::as_str)).collect();
    if ids.iter().filter(|id| !have(id)).count() > 30 {
        let zip = dir.join("allasfamc.zip");
        fetch::get(ARCHIVE, &zip)?;
        let z = fetch::RemoteZip::open_file(&zip)?;
        let mut got = 0;
        for e in &z.entries {
            let base = e.name.rsplit('/').next().unwrap_or("");
            if is_take_file(base) && !dir.join(base).exists() {
                std::fs::write(dir.join(base), z.read(e)?)?;
                got += 1;
            }
        }
        std::fs::remove_file(&zip)?;
        log.push(format!("unpacked {got} files from the archive"));
    }
    let missing: Vec<&str> = ids.iter().copied().filter(|id| !have(id)).collect();
    for id in &missing {
        super::cmu_get(id, dir)?;
    }
    if !missing.is_empty() {
        log.push(format!("fetched {} takes the archive lacks", missing.len()));
    }
    Ok(std::fs::read_dir(dir)?.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".amc")).count())
}

/// Take ids in the order the reference sorts them (numbers compared as numbers).
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |s: &str| s.split('_').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    key(a).cmp(&key(b))
}

/// The ledger's columns, and the head that explains them.
const COLS: [&str; 14] =
    ["id", "subject", "fps", "sec", "category", "active", "fit", "travel", "hips", "flags", "used", "note", "desc", "about"];
const HEAD: &str = "# Every take of the CMU motion capture database (mocap.cs.cmu.edu; free for all uses), one a line. Written by
# the mocap tool (mocap survey=ledger), which converts each take into the readable format and measures it; edit only \"note\".
# sec: length in seconds. category: what a game would use it for (from the words of its description). active: the
# seconds where it moves (still stretches at either end left out). fit: the readable format's average and worst
# body-point error against the capture (mm). travel: metres the hips cover. hips: lowest and highest hip height, in
# percent of standing (under 60: crouching or lying; over 115: in the air or up on something). flags: inverted (upside
# down at some point), floats (feet off the floor most of the time: stairs, a ladder, a bench), loose (fit over 30 mm),
# short (under a second), fps? (not on the site's list: 120 assumed), error. used: the CMU set's clips cut from it
# (anim/catalogs/cmu.json). note: yours, kept when the survey runs again: \"pick\" (next to import), \"skip: why\".";

/// What a survey makes: the ledger's rows (when measured) and a set a subject (when asked for).
pub struct Survey {
    pub rows: BTreeMap<String, Map<String, Value>>,
    pub sets: Vec<ClipSet>,
    pub takes: usize,
    pub seconds: f64,
}

/// Converts every downloaded take of the `subjects` (all when `None`), a subject at a time, the
/// way the importer would (whole takes, 30 frames a second), and measures each (`ledger`, fitting
/// within `tol` mm) and/or makes a set a subject (`lib_tol`: fitted within that, for browsing).
/// Descriptions, categories, frame rates and notes come from the old ledger, which the survey
/// brings up to date; `used` lists the clips of the CMU catalog cut from each take.
pub fn survey(
    dir: &Path,
    old: &Ledger,
    used: &HashMap<String, Vec<String>>,
    subjects: Option<&[u32]>,
    ledger: bool,
    tol: f64,
    lib_tol: Option<f64>,
    log: &mut Vec<String>,
) -> Result<Survey> {
    let mut by_subject: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for e in std::fs::read_dir(dir)?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(id) = name.strip_suffix(".amc").filter(|s| is_take_file(&format!("{s}.amc")) && s.contains('_')) {
            let s: u32 = id.split('_').next().unwrap_or("0").parse().unwrap_or(0);
            by_subject.entry(s).or_default().push(id.to_string());
        }
    }
    let g = |id: &str, col: &str| old.get(id, col).unwrap_or("").to_string();
    let about_of = |s: u32| {
        (0..old.rows.len())
            .map(|i| old.row(i))
            .find(|r| r.get("subject").and_then(|v| v.as_str()).and_then(|v| v.parse::<u32>().ok()) == Some(s))
            .and_then(|r| r.get("about").and_then(|v| v.as_str()).map(str::to_string))
            .unwrap_or_default()
    };
    let mut out = Survey { rows: BTreeMap::new(), sets: Vec::new(), takes: 0, seconds: 0.0 };
    // Every row of the old ledger, brought up to date below for each take that is on disk.
    for i in 0..old.rows.len() {
        let r = old.row(i);
        if let Some(id) = r.get("id").and_then(|v| v.as_str()) {
            out.rows.insert(id.to_string(), r.clone());
        }
    }
    let (pel, top) = (PELVIS, HEAD_TOP);
    let feet = [side(0, TOE), side(1, TOE), side(0, BALL), side(1, BALL), side(0, ANKLE), side(1, ANKLE)];
    for (s, mut ids) in by_subject {
        if subjects.is_some_and(|w| !w.contains(&s)) {
            continue;
        }
        ids.sort_by(|a, b| natural(a, b));
        let asf_path = dir.join(format!("{s:02}.asf"));
        let read = |p: &Path| -> Result<String> {
            std::fs::read(p).map(|b| b.iter().map(|&c| c as char).collect()).map_err(|e| anyhow!("{}: {e}", p.display()))
        };
        let fps_of = |id: &str| g(id, "fps").parse::<f64>().unwrap_or(120.0);
        let picks: Vec<Pick> = ids
            .iter()
            .map(|id| Pick {
                name: id.clone(),
                take: id.clone(),
                from: None,
                to: None,
                min_cycle: None,
                fps: fps_of(id),
                looping: false,
                find: None,
            })
            .collect();
        let mut failed: HashMap<String, String> = HashMap::new();
        let mut caps: Vec<Cap> = Vec::new();
        let subject = read(&asf_path).and_then(|t| acclaim::Subject::parse(&t));
        match &subject {
            Err(e) => {
                for id in &ids {
                    failed.insert(id.clone(), e.to_string());
                }
            }
            Ok(subject) => {
                let mut texts: HashMap<String, String> = HashMap::new();
                for id in &ids {
                    match read(&dir.join(format!("{id}.amc"))) {
                        Ok(t) => {
                            texts.insert(id.clone(), t);
                        }
                        Err(e) => {
                            failed.insert(id.clone(), e.to_string());
                        }
                    }
                }
                let mut all: HashMap<String, Box<dyn Take + '_>> = HashMap::new();
                for (id, t) in &texts {
                    match subject.take(t) {
                        Ok(tk) => {
                            all.insert(id.clone(), Box::new(tk));
                        }
                        Err(e) => {
                            failed.insert(id.clone(), e.to_string());
                        }
                    }
                }
                let ok: Vec<Pick> = picks.iter().filter(|p| all.contains_key(&p.take)).cloned().collect();
                let mut cut_log = Vec::new();
                match takes::cut(&subject.body, &all, &ok, 30.0, &mut cut_log) {
                    Ok(c) => caps = c,
                    Err(_) => {
                        // One bad take: read the subject's takes one at a time to find it.
                        for pk in &ok {
                            match takes::cut(&subject.body, &all, std::slice::from_ref(pk), 30.0, &mut cut_log) {
                                Ok(c) => {
                                    for cap in c {
                                        match caps.iter_mut().find(|x| x.name == cap.name) {
                                            Some(x) => *x = cap,
                                            None => caps.push(cap),
                                        }
                                    }
                                }
                                Err(e) => {
                                    failed.insert(pk.take.clone(), e.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        let rest_cap = caps.iter().find(|c| c.name == "_rest");
        let rest = rest_cap.map(|_| Rest::measure(&caps, Some("_rest")));
        let floor = rest_cap.map_or(0.0, |r| feet[..4].iter().map(|&i| r.data[i * 3 + 2] as f64).fold(f64::INFINITY, f64::min));
        let standing = rest.as_ref().map_or(1.0, |r| r.h0);
        let set_name = format!("CMU_{s:02}");
        let about = about_of(s);
        let mut set = lib_tol.map(|_| ClipSet {
            set: set_name.clone(),
            title: format!("CMU subject {s}"),
            format: 1,
            credit: "CMU Graphics Lab Motion Capture Database (mocap.cs.cmu.edu), free for all uses; created with funding from NSF EIA-0196217.".into(),
            legend: clips::LEGEND.iter().map(|l| l.to_string()).collect(),
            fps: 30.0,
            ..Default::default()
        });
        if let (Some(set), Some(rest)) = (set.as_mut(), rest.as_ref()) {
            let mut src = Map::new();
            src.insert("file".into(), format!("{s:02}.asf").into());
            src.insert("rig".into(), "cmu".into());
            let label = format!(
                "CMU motion capture, subject {s}{}",
                if about.is_empty() { String::new() } else { format!(" ({about})") }
            );
            src.insert("label".into(), label.into());
            src.insert("origin".into(), "CMU Graphics Lab Motion Capture Database (mocap.cs.cmu.edu)".into());
            src.insert(
                "license".into(),
                "free for all uses (may be copied, modified, or redistributed without permission)".into(),
            );
            src.insert("url".into(), format!("{SITE}/search.php?subjectnumber={s}").into());
            src.insert("rest".into(), rest.to_value());
            set.sources.insert(set_name.clone(), Value::Object(src));
        }
        for id in &ids {
            let mut r: Map<String, Value> = out.rows.get(id).cloned().unwrap_or_default();
            for (k, v) in [("id", id.clone()), ("subject", s.to_string()), ("fps", fps_of(id).to_string())] {
                r.insert(k.into(), v.into());
            }
            let mut flags: Vec<String> = Vec::new();
            // A take the site does not list keeps its mark: its frame rate is assumed.
            if g(id, "flags").split_whitespace().any(|f| f == "fps?") {
                flags.push("fps?".into());
            }
            let cap = caps.iter().find(|c| &c.name == id);
            let (Some(cap), Some(rest)) = (cap, rest.as_ref()) else {
                let why = failed.get(id).cloned().unwrap_or_else(|| "not read".into());
                for k in ["sec", "category", "active", "fit", "travel", "hips"] {
                    r.insert(k.into(), "".into());
                }
                r.insert(
                    "flags".into(),
                    std::iter::once(format!("error: {why}")).chain(flags).collect::<Vec<_>>().join(" ").into(),
                );
                out.rows.insert(id.clone(), r);
                continue;
            };
            let (n, f3) = (cap.n, P * 3);
            let d = &cap.data;
            r.insert("sec".into(), to_fixed(cap.dur, 1).into());
            // Where it moves: each frame's change (every point, plus the root's travel); the
            // still ends are trimmed.
            let mut e = vec![0f32; n];
            for f in 1..n {
                let mut a = 0.0f64;
                for k in 0..f3 {
                    let v = d[f * f3 + k] as f64 - d[(f - 1) * f3 + k] as f64;
                    a += v * v;
                }
                if let Some(m) = &cap.travel {
                    let (x, y) = (m[f * 2] as f64 - m[f * 2 - 2] as f64, m[f * 2 + 1] as f64 - m[f * 2 - 1] as f64);
                    a += P as f64 * (x * x + y * y);
                }
                e[f] = (a / P as f64).sqrt() as f32;
            }
            let mut sorted: Vec<f64> = e.iter().map(|v| *v as f64).collect();
            sorted.sort_by(|a, b| a.total_cmp(b));
            let cut = 2.0f64.max(sorted[((n as f64 * 0.9).floor() as usize).min(n - 1)] * 0.2);
            let mut a0 = 1;
            while a0 + 1 < n && (e[a0] as f64) < cut {
                a0 += 1;
            }
            let mut a1 = n - 1;
            while a1 > a0 && (e[a1] as f64) < cut {
                a1 -= 1;
            }
            let active = format!(
                "{}-{}",
                to_fixed(0f64.max((a0 as f64 - 6.0) / 30.0), 1),
                to_fixed(cap.dur.min((a1 as f64 + 6.0) / 30.0), 1)
            );
            r.insert("active".into(), active.into());
            let mut travel = 0.0;
            if let Some(m) = &cap.travel {
                for f in 1..n {
                    travel +=
                        readable::hypot(&[m[f * 2] as f64 - m[f * 2 - 2] as f64, m[f * 2 + 1] as f64 - m[f * 2 - 1] as f64]);
                }
            }
            r.insert("travel".into(), to_fixed(travel / 1000.0, 1).into());
            let (mut lo, mut hi, mut inverted, mut off) = (f64::INFINITY, f64::NEG_INFINITY, 0, 0);
            for f in 0..n {
                let hz = d[f * f3 + pel * 3 + 2] as f64;
                lo = lo.min(hz);
                hi = hi.max(hz);
                if (d[f * f3 + top * 3 + 2] as f64) < hz - 50.0 {
                    inverted += 1;
                }
                if feet.iter().map(|&i| d[f * f3 + i * 3 + 2] as f64).fold(f64::INFINITY, f64::min) > floor + 150.0 {
                    off += 1;
                }
            }
            r.insert("hips".into(), format!("{}-{}", js_round(lo / standing * 100.0), js_round(hi / standing * 100.0)).into());
            if inverted > 2 {
                flags.push("inverted".into());
            }
            if off as f64 > n as f64 * 0.6 {
                flags.push("floats".into());
            }
            if cap.dur < 1.0 {
                flags.push("short".into());
            }
            // Fitted in pieces of at most 10 s (fitting costs the square of a clip's length; a
            // long take is a series of moments anyway): the ledger's fit is over all of them; the
            // library keeps a long take as its numbered parts.
            let w = if n > 600 { 300 } else { n - 1 };
            let (mut sum, mut worst, mut part, mut a) = (0.0, 0.0f64, 0, 0usize);
            loop {
                if !(a + 1 < n || a == 0) {
                    break;
                }
                let b = (n - 1).min(a + w);
                let piece = Cap {
                    name: id.clone(),
                    n: b - a + 1,
                    dur: (b - a) as f64 / 30.0,
                    looping: false,
                    fps: 30.0,
                    data: d[a * f3..(b + 1) * f3].to_vec(),
                    travel: cap.travel.as_ref().map(|m| m[a * 2..(b + 1) * 2].to_vec()),
                    take: None,
                    stride: None,
                };
                let fitted = ledger.then(|| readable::fit(rest, &piece, tol, false));
                if let Some(f) = &fitted {
                    sum += f.mean * piece.n as f64;
                    worst = worst.max(f.max);
                }
                if let (Some(set), Some(lt)) = (set.as_mut(), lib_tol) {
                    let fl = match fitted {
                        Some(f) if lt == tol => f,
                        _ => readable::fit(rest, &piece, lt, false),
                    };
                    let name = if w < n - 1 {
                        part += 1;
                        format!("{id}_part{part}")
                    } else {
                        id.clone()
                    };
                    let mut c = readable::clip_of(&piece, &fl.keys, &set_name);
                    c.clip = name.clone();
                    c.take = Some(format!("{id} {}-{}", to_fixed(a as f64 / 30.0, 2), to_fixed(b as f64 / 30.0, 2)));
                    c.tags = vec![g(id, "category")].into_iter().filter(|t| !t.is_empty()).collect();
                    c.desc = g(id, "desc");
                    set.fit.insert(name.clone(), [js_round(fl.mean) as f32, js_round(fl.max) as f32]);
                    set.clips.insert(name, c);
                }
                if b >= n - 1 || w == 0 {
                    break;
                }
                a += w;
            }
            if ledger {
                let mean = sum / n as f64;
                r.insert("fit".into(), format!("{}/{}", js_round(mean), js_round(worst)).into());
                if mean > 30.0 {
                    flags.push("loose".into());
                }
            }
            r.insert("flags".into(), flags.join(" ").into());
            out.rows.insert(id.clone(), r);
            out.takes += 1;
            out.seconds += cap.dur;
        }
        if let Some(set) = set.filter(|s| !s.clips.is_empty()) {
            out.sets.push(set);
        }
        log.push(format!("subject {s}: {} takes", ids.len()));
    }
    for (id, r) in out.rows.iter_mut() {
        r.insert("used".into(), used.get(id).map(|c| c.join(" ")).unwrap_or_default().into());
    }
    Ok(out)
}

/// Which clips of a catalog each take feeds: `13_17` → `Boxing_Guard_Loop Boxing_Jab`.
pub fn used_by(cat: &super::Catalog) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for (clip, p) in &cat.pick {
        if let Some(take) = p.get(0).and_then(|t| t.as_str()) {
            out.entry(take.to_string()).or_default().push(clip.clone());
        }
    }
    out
}

/// Writes the ledger (rows by subject, then take).
pub fn write_ledger(rows: &BTreeMap<String, Map<String, Value>>, path: &Path) -> Result<()> {
    let mut list: Vec<&Map<String, Value>> = rows.values().collect();
    let num = |r: &Map<String, Value>| r.get("subject").and_then(|v| v.as_str()).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let id = |r: &Map<String, Value>| r.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    list.sort_by(|a, b| num(a).cmp(&num(b)).then_with(|| natural(&id(a), &id(b))));
    let mut out = String::from(HEAD);
    out.push('\n');
    out.push_str(&COLS.join("\t"));
    out.push('\n');
    for r in list {
        let cells: Vec<String> =
            COLS.iter().map(|c| r.get(*c).and_then(|v| v.as_str()).unwrap_or("").replace(['\t', '\n'], " ")).collect();
        out.push_str(&cells.join("\t"));
        out.push('\n');
    }
    std::fs::write(path, out)?;
    Ok(())
}

/// Where a subject's library set goes: anim/cmu/cmu_NN.json.
pub fn set_path(dir: &Path, set: &ClipSet) -> PathBuf {
    dir.join(format!("{}.json", set.set.to_lowercase()))
}
