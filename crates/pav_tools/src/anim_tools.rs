//! Animation tools: the motion clip library (`clips`), translating animation libraries into it
//! (`clip_import`) and live reloading of /anim (`anim_reload`).

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use pav_core::clips::{self, ClipSet};
use serde_json::{Value, json};

use crate::session::Session;
use crate::tools::{Args, Output, get_str, get_u64};

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

/// `clip_import from=PATH`: translates an animation library into this engine's clip sets.
pub fn t_clip_import(_: &mut Session, a: &Args) -> Result<Output> {
    let from = PathBuf::from(get_str(a, "from").ok_or_else(|| anyhow!("from= a set file (.js or .json) or a folder of them"))?);
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
    let anim = pav_core::anim::disk_dir().unwrap_or_else(|| PathBuf::from("anim"));
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
        "mean_fit_mm": if fit_n > 0 { (fit_sum / fit_n as f32 * 10.0).round() / 10.0 } else { 0.0 },
        "written": shown,
        "skipped": skipped,
        "next": if folder { "clips load=<folder name> adds them to this session's library" } else { "anim_reload picks the new set up (the build embeds it)" },
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
            rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(Output::Json(json!({
        "clips": lib.len(),
        "sets": lib.sets.iter().map(|s| json!({ "set": s.set, "title": s.title, "clips": s.clips.len(), "credit": s.credit })).collect::<Vec<_>>(),
        "folders_on_disk": on_disk,
        "use": "find=WORDS searches names, tags and descriptions; name=SET/Clip shows one as readable key poses; animsheet clip=NAME renders it; load=FOLDER adds an on-disk library",
    })))
}

/// `anim_reload`: re-reads anim/ (moves and clip sets) from disk.
pub fn t_anim_reload(_: &mut Session, _: &Args) -> Result<Output> {
    let summary = pav_core::anim::reload(true).map_err(|e| anyhow!(e))?;
    Ok(Output::Json(json!({ "reloaded": summary })))
}
