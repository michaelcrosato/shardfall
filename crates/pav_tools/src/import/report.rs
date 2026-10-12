//! What the importers write and how they answer: a file written only once it reads back, the
//! window of notes a reply shows (`log=`), and the clip importer's set files and report.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use pav_core::clips::ClipSet;
use serde_json::{Value, json};

use crate::mocap::Done;
use crate::tools::{Args, Output, get_str, get_u64};

/// Writes `text` to `out`, making its folder. Returns the bytes written.
pub fn write_text(out: &Path, text: &str) -> Result<usize> {
    if let Some(d) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(out, text).map_err(|e| anyhow!("{}: {e}", out.display()))?;
    Ok(text.len())
}

/// The notes a reply shows (`log=`, default `shown`) and how many more there are.
pub fn log_window(a: &Args, log: &[String], shown: usize) -> (Value, usize) {
    let shown = get_u64(a, "log", shown as u64).unwrap_or(shown as u64) as usize;
    (json!(log.iter().take(shown).collect::<Vec<_>>()), log.len().saturating_sub(shown))
}

fn anim_dir() -> PathBuf {
    pav_core::anim::disk_dir().unwrap_or_else(|| PathBuf::from("anim"))
}

/// Where a set goes: `out=`, else anim/<set>.json (a library whose licence forbids sharing
/// what is made from it: anim/local, which is not committed).
pub fn set_out(a: &Args, set: &str, local: bool) -> PathBuf {
    get_str(a, "out").map(PathBuf::from).unwrap_or_else(|| {
        let dir = if local { anim_dir().join("local") } else { anim_dir() };
        dir.join(format!("{}.json", set.to_lowercase()))
    })
}

/// Writes a set to `out` once it reads back with every clip. Returns the file and its bytes.
pub fn write_set_to(set: &ClipSet, out: &Path) -> Result<(PathBuf, usize)> {
    let text = set.to_text();
    let back = ClipSet::parse(&text).map_err(|e| anyhow!("{}: the translated set does not read back: {e}", set.set))?;
    if back.clips.len() != set.clips.len() {
        bail!("{}: clips lost in translation", set.set);
    }
    let bytes = write_text(out, &text)?;
    Ok((out.to_path_buf(), bytes))
}

/// The clip importer's reply: the set, its fit, the file and the notes.
pub fn report(a: &Args, set: &ClipSet, done: &[Done], file: &Path, bytes: usize, log: Vec<String>) -> Output {
    let n = done.len().max(1) as f64;
    let mean = done.iter().map(|d| d.mean).sum::<f64>() / n;
    let worst = done.iter().max_by(|x, y| x.max.total_cmp(&y.max));
    let (shown, more) = log_window(a, &log, 60);
    Output::Json(json!({
        "set": set.set,
        "clips": done.len(),
        "key_poses": done.iter().map(|d| d.keys).sum::<usize>(),
        "bytes": bytes,
        "mean_fit_mm": (mean * 10.0).round() / 10.0,
        "worst": worst.map(|d| json!({ "clip": d.name, "mm": d.max.round() })),
        "file": file.display().to_string(),
        "log": shown,
        "log_more": more,
        "next": format!("anim_reload picks the set up; clips set={} lists it; animsheet clip={}/<clip> shows one", set.set, set.set),
    }))
}
