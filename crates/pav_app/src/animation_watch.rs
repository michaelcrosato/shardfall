//! Reload individual animation files after a short pause in filesystem changes.
//! A bad file keeps the last valid asset. Reloading one file preserves other loaded libraries.

#[cfg(not(target_arch = "wasm32"))]
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, channel};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
pub struct AnimationWatcher {
    _watcher: notify::RecommendedWatcher,
    receiver: Receiver<PathBuf>,
    pending: BTreeMap<PathBuf, Instant>,
}

#[cfg(target_arch = "wasm32")]
pub struct AnimationWatcher;

#[cfg(target_arch = "wasm32")]
impl AnimationWatcher {
    pub fn directory_exists() -> bool {
        false
    }
    pub fn start() -> Option<Self> {
        None
    }
    pub fn poll(&mut self) -> Vec<Result<String, String>> {
        Vec::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn animation_dir() -> Option<PathBuf> {
    let dir = std::env::var("PAV_ANIM").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("anim"));
    dir.is_dir().then_some(dir)
}

#[cfg(not(target_arch = "wasm32"))]
fn animation_file(path: &Path) -> bool {
    // Editor history and import catalogs contain JSON, but they are not animation sets.
    if path.components().any(|c| c.as_os_str() == ".editor" || c.as_os_str() == "catalogs") {
        return false;
    }
    path.file_name().is_some_and(|n| n == "moves.toml")
        || path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

#[cfg(not(target_arch = "wasm32"))]
impl AnimationWatcher {
    /// A client can defer its one watcher attempt until the first authored file creates the
    /// directory. Workshop-only folders are valid even when they contain no moves.toml.
    pub fn directory_exists() -> bool {
        animation_dir().is_some()
    }

    pub fn start() -> Option<Self> {
        use notify::Watcher;
        let dir = animation_dir()?;
        let (sender, receiver) = channel();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                if matches!(event.kind, notify::EventKind::Access(_)) {
                    return;
                }
                for path in event.paths.into_iter().filter(|p| animation_file(p)) {
                    let _ = sender.send(path);
                }
            }
        })
        .map_err(|e| log::warn!("animation file watcher unavailable: {e}"))
        .ok()?;
        watcher
            .watch(&dir, notify::RecursiveMode::Recursive)
            .map_err(|e| log::warn!("cannot watch animation files at {}: {e}", dir.display()))
            .ok()?;
        log::info!("animation hot reload: watching {}", dir.display());
        Some(Self { _watcher: watcher, receiver, pending: BTreeMap::new() })
    }

    /// Called by the native frame loop. Each result can be shown in the studio status area.
    pub fn poll(&mut self) -> Vec<Result<String, String>> {
        for path in self.receiver.try_iter() {
            self.pending.insert(path, Instant::now());
        }
        let ready: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, changed)| changed.elapsed() >= Duration::from_millis(250))
            .map(|(path, _)| path.clone())
            .collect();
        ready
            .into_iter()
            .map(|path| {
                self.pending.remove(&path);
                reload_file(&path).map_err(|e| format!("{}: {e}; the last valid animation stays active", path.display()))
            })
            .collect()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn reload_file(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if path.file_name().is_some_and(|n| n == "moves.toml") {
        let count = pav_core::moves::replace_text(&text)?;
        return Ok(format!("Reloaded {count} procedural moves"));
    }
    let set = pav_core::clips::ClipSet::parse(&text)?;
    let authored = set.set == "WORKSHOP" || set.set == "WORKSHOP_LOCAL";
    if !set.fps.is_finite() || set.fps < 0.0 {
        return Err("the set frame rate must be a finite number of zero or greater".into());
    }
    for (name, clip) in &set.clips {
        let check = if authored { pav_core::animation_edit::validate(clip) } else { finite_values(clip) };
        check.map_err(|e| format!("{}/{name}: {e}", set.set))?;
    }
    let message = format!("Reloaded {} ({} clips)", set.set, set.clips.len());
    pav_core::clips::replace_set(set)?;
    Ok(message)
}

/// Imported files can contain a final interpolation key beyond their duration. Keep that
/// format support while rejecting numbers that cannot form a finite pose.
#[cfg(not(target_arch = "wasm32"))]
fn finite_values(clip: &pav_core::clips::Clip) -> Result<(), String> {
    if !clip.dur.is_finite() || clip.dur < 0.0 || clip.speed.is_some_and(|s| !s.is_finite()) {
        return Err("duration and speed must be finite numbers, with no negative duration".into());
    }
    for key in &clip.keys {
        let mut values = std::iter::once(key.t)
            .chain(key.hips)
            .chain(key.body)
            .chain(key.chest)
            .chain(key.head)
            .chain(key.arm_l)
            .chain(key.arm_r)
            .chain(key.leg_l)
            .chain(key.leg_r)
            .chain(key.sh_l.into_iter().flatten())
            .chain(key.sh_r.into_iter().flatten())
            .chain(key.foot_l.into_iter().flatten())
            .chain(key.foot_r.into_iter().flatten())
            .chain(key.blade.into_iter().flatten())
            .chain(key.root.into_iter().flatten());
        if values.any(|v| !v.is_finite() || v.abs() > 1_000_000.0) {
            return Err("pose values must be finite numbers from -1000000 to 1000000".into());
        }
    }
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn watcher_ignores_editor_history_and_import_catalogs() {
        assert!(animation_file(Path::new("anim/workshop.json")));
        assert!(animation_file(Path::new("anim/local/custom.json")));
        assert!(animation_file(Path::new("anim/moves.toml")));
        assert!(!animation_file(Path::new("anim/.editor/workshop.json")));
        assert!(!animation_file(Path::new("anim/catalogs/quaternius.json")));
        assert!(!animation_file(Path::new("anim/workshop.json.tmp")));
    }
}
