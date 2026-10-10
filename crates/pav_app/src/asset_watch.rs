//! Validated hot reload for authored object assets. The watcher can start before the first
//! asset directory exists; accepted definitions are handed back to the simulation thread.

use std::sync::Arc;

use pav_core::Sim;
use pav_core::props::{self, PropAsset};

pub type AssetUpdate = Result<(String, Arc<PropAsset>), String>;

/// Apply an accepted file on the simulation thread only if it is still current. A bridge
/// edit can publish and refresh a newer revision before this queued callback is reached.
pub fn apply_update(sim: &mut Sim, name: &str, definition: Arc<PropAsset>) -> bool {
    let Some(current) = props::get(name) else { return false };
    if !Arc::ptr_eq(&current, &definition) && current.revision() != definition.revision() {
        return false;
    }
    sim.refresh_prop_instances(name, definition);
    true
}

#[cfg(not(target_arch = "wasm32"))]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, channel};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct AssetWatcher {
    watcher: Option<notify::RecommendedWatcher>,
    receiver: Option<Receiver<Result<PathBuf, String>>>,
    directory: Option<PathBuf>,
    attempted: bool,
    pending: BTreeMap<PathBuf, Instant>,
    failed: BTreeSet<PathBuf>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub struct AssetWatcher;

impl AssetWatcher {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(target_arch = "wasm32")]
impl AssetWatcher {
    pub fn poll(&mut self) -> Vec<AssetUpdate> {
        Vec::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn asset_file(path: &Path, directory: &Path) -> bool {
    // Workshop names are flat leaves. Watching only that directory also keeps history,
    // templates, temporary files, and unrelated JSON out of the reload queue.
    path.parent() == Some(directory)
        && path.file_name().and_then(|n| n.to_str()).is_some_and(|n| !n.starts_with('.'))
        && path.extension().and_then(|e| e.to_str()) == Some("json")
}

#[cfg(not(target_arch = "wasm32"))]
impl AssetWatcher {
    fn attach(&mut self, directory: &Path) -> Result<(), String> {
        use notify::Watcher;

        let directory = directory.canonicalize().map_err(|e| e.to_string())?;
        let watched = directory.clone();
        let (sender, receiver) = channel();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
            Ok(event) => {
                if matches!(event.kind, notify::EventKind::Access(_)) {
                    return;
                }
                for path in event.paths.into_iter().filter(|p| asset_file(p, &watched)) {
                    let _ = sender.send(Ok(path));
                }
            }
            Err(error) => {
                let _ = sender.send(Err(format!("Asset watcher: {error}")));
            }
        })
        .map_err(|e| e.to_string())?;
        watcher.watch(&directory, notify::RecursiveMode::NonRecursive).map_err(|e| e.to_string())?;

        // The first file may have created the directory before we could watch it. Queue an
        // initial scan after attaching so edits during the scan are observed as well.
        let changed = Instant::now();
        for entry in std::fs::read_dir(&directory).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if asset_file(&path, &directory) {
                self.pending.insert(path, changed);
            }
        }
        log::info!("object asset hot reload: watching {}", directory.display());
        self.directory = Some(directory);
        self.receiver = Some(receiver);
        self.watcher = Some(watcher);
        Ok(())
    }

    /// Called once per native frame. A missing workshop stays dormant until a file editor or
    /// agent creates it. An unavailable watcher is reported once, without a retry loop.
    /// Each valid definition can be applied with `Sim::refresh_prop_instances`.
    pub fn poll(&mut self) -> Vec<AssetUpdate> {
        let mut updates = Vec::new();
        if self.watcher.is_some() && self.directory.as_ref().is_some_and(|p| !p.is_dir()) {
            self.watcher = None;
            self.receiver = None;
            self.directory = None;
            self.pending.clear();
            self.attempted = false;
            updates.push(Err("The object asset directory was removed; existing objects stay active.".into()));
        }
        if self.watcher.is_none() && !self.attempted {
            let directory = pav_tools::asset_tools::root().join("workshop");
            if !directory.is_dir() {
                return updates;
            }
            self.attempted = true;
            if let Err(error) = self.attach(&directory) {
                self.pending.clear();
                updates.push(Err(format!("Cannot watch object assets at {}: {error}", directory.display())));
                return updates;
            }
        }

        if let Some(receiver) = &self.receiver {
            for event in receiver.try_iter() {
                match event {
                    Ok(path) => {
                        self.pending.insert(path, Instant::now());
                    }
                    Err(error) => updates.push(Err(error)),
                }
            }
        }
        let ready: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, changed)| changed.elapsed() >= Duration::from_millis(250))
            .map(|(path, _)| path.clone())
            .collect();
        for path in ready {
            self.pending.remove(&path);
            if let Some(update) = self.reload(&path) {
                updates.push(update);
            }
        }
        updates
    }

    fn reload(&mut self, path: &Path) -> Option<AssetUpdate> {
        match pav_tools::asset_tools::reload_file(path) {
            Ok(Some(asset)) => {
                self.failed.remove(path);
                Some(Ok(asset))
            }
            Ok(None) => {
                // Restoring the exact last-good bytes is a recovery even though the
                // library is already correct. Report it once so the panel clears its error.
                if self.failed.remove(path) && path.is_file() {
                    let leaf = path.file_stem()?.to_str()?;
                    let name = format!("WORKSHOP/{leaf}");
                    return pav_core::props::get(&name).map(|asset| Ok((name, asset)));
                }
                None
            }
            Err(error) => {
                self.failed.insert(path.to_owned());
                Some(Err(format!("{}: {error}; the last valid object stays active", path.display())))
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn authored_assets_are_watched_without_history_or_temporary_files() {
        let root = Path::new("assets/props/workshop");
        assert!(asset_file(Path::new("assets/props/workshop/bench.json"), root));
        assert!(!asset_file(Path::new("assets/props/.history/bench.json"), root));
        assert!(!asset_file(Path::new("assets/props/builtin/bench.json"), root));
        assert!(!asset_file(Path::new("assets/props/workshop/.bench.json"), root));
        assert!(!asset_file(Path::new("assets/props/workshop/bench.json.tmp"), root));
        assert!(!asset_file(Path::new("assets/props/bench.json"), root));
    }

    #[test]
    fn restoring_the_last_good_file_reports_recovery_once() {
        let root = std::env::temp_dir().join(format!("pav-asset-watch-recovery-{}", std::process::id()));
        let path = root.join("workshop/test_watcher_recovery.json");
        let name = "WORKSHOP/test_watcher_recovery";
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut asset = (*pav_core::props::get("BUILTIN/box").unwrap()).clone();
        asset.name = "test_watcher_recovery".into();
        let text = asset.text();
        std::fs::write(&path, &text).unwrap();
        let mut watcher = AssetWatcher::new();
        let (_, installed) = watcher.reload(&path).unwrap().unwrap();
        assert!(watcher.reload(&path).is_none(), "an ordinary duplicate save stays quiet");

        std::fs::write(&path, "{unfinished").unwrap();
        assert!(watcher.reload(&path).unwrap().is_err());
        assert!(Arc::ptr_eq(&installed, &pav_core::props::get(name).unwrap()));
        std::fs::write(&path, text).unwrap();
        let (recovered_name, recovered) = watcher.reload(&path).unwrap().unwrap();
        assert_eq!(recovered_name, name);
        assert!(Arc::ptr_eq(&installed, &recovered), "recovery must retain the installed definition");
        assert!(watcher.reload(&path).is_none(), "one recovery clears the error; further saves stay quiet");

        pav_core::props::remove(name).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_stale_queued_file_cannot_replace_a_newer_live_instance() {
        let name = "WORKSHOP/test_watcher_stale";
        let mut first = (*props::get("BUILTIN/bench").unwrap()).clone();
        first.name = "test_watcher_stale".into();
        let first = Arc::new(first);
        props::install(name, first.clone()).unwrap();
        let mut sim = Sim::empty(3);
        let id = sim.spawn_prop(name, glam::Vec3::ZERO, glam::Quat::IDENTITY, 1.0, true, None).unwrap();
        let body = sim.state.entities.get(id).unwrap().body;

        let mut latest = (*first).clone();
        latest.parts.remove("crossbar");
        let latest = Arc::new(latest);
        props::install(name, latest.clone()).unwrap();
        assert!(apply_update(&mut sim, name, latest.clone()));
        assert!(!apply_update(&mut sim, name, first), "an older queued file must be skipped");
        let entity = sim.state.entities.get(id).unwrap();
        assert!(Arc::ptr_eq(&entity.prop.as_ref().unwrap().definition, &latest));
        assert_eq!(entity.body, body);
        let collider = sim.state.physics.bodies.get(body.unwrap()).unwrap().colliders()[0];
        let shape = sim.state.physics.colliders.get(collider).unwrap().shape();
        assert_eq!(shape.as_compound().unwrap().shapes().len(), latest.parts.len());
        assert!(apply_update(&mut sim, name, latest.clone()), "the current revision still applies");

        props::remove(name).unwrap();
        assert!(!apply_update(&mut sim, name, latest), "a removed asset must not be restored by a queued file");
    }
}
