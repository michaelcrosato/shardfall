//! Animation data files (/anim): the moves table (`moves.toml`) and the motion clip sets
//! (`*.json`). They are embedded in the binary at build time; `reload(true)` reads them from
//! ./anim (or $PAV_ANIM) instead, for live editing. Sub-folders of /anim (the full motion-capture
//! libraries) are never embedded: `crate::clips` loads them from disk when a clip is asked for.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

include!(concat!(env!("OUT_DIR"), "/anim_data.rs"));

static USE_DISK: AtomicBool = AtomicBool::new(false);

/// The anim folder on disk: $PAV_ANIM, else ./anim (when it holds moves.toml).
pub fn disk_dir() -> Option<PathBuf> {
    let d = std::env::var("PAV_ANIM").map(PathBuf::from).unwrap_or_else(|_| "anim".into());
    d.join("moves.toml").exists().then_some(d)
}

/// The text of an animation file (`"moves.toml"`, `"quaternius.json"`): from disk while live
/// editing, else the embedded copy.
pub fn source(name: &str) -> Option<String> {
    if USE_DISK.load(Ordering::Relaxed) {
        if let Some(t) = disk_dir().and_then(|d| std::fs::read_to_string(d.join(name)).ok()) {
            return Some(t);
        }
    }
    ANIM_DATA.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
}

/// Names of the animation files with extension `ext` (embedded, plus new ones on disk while live
/// editing), sorted.
pub fn files(ext: &str) -> Vec<String> {
    let dot = format!(".{ext}");
    let mut v: Vec<String> = ANIM_DATA.iter().filter(|(k, _)| k.ends_with(&dot)).map(|(k, _)| k.to_string()).collect();
    if USE_DISK.load(Ordering::Relaxed) {
        if let Some(rd) = disk_dir().and_then(|d| std::fs::read_dir(d).ok()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if e.path().is_file() && name.ends_with(&dot) && !v.contains(&name) {
                    v.push(name);
                }
            }
        }
    }
    v.sort();
    v
}

/// Re-reads the moves table and the clip sets (from disk when `from_disk`). An invalid file
/// keeps the old data and returns the error. Returns a one-line summary.
pub fn reload(from_disk: bool) -> Result<String, String> {
    USE_DISK.store(from_disk, Ordering::Relaxed);
    let moves = crate::moves::reload()?;
    let (sets, clips) = crate::clips::reload()?;
    Ok(format!("{moves} moves, {clips} clips in {sets} sets"))
}
