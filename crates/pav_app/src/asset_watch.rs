//! Validated hot reload for authored object assets. The watcher can start before the first
//! asset directory exists; accepted definitions are handed back to the simulation thread.

use std::sync::Arc;

use pav_core::Sim;
use pav_core::props::{self, PropDef};

pub type AssetUpdate = Result<(String, Arc<PropDef>), String>;

/// Apply an accepted file on the simulation thread only if it is still current. A bridge
/// edit can publish and refresh a newer revision before this queued callback is reached.
pub fn apply_update(sim: &mut Sim, name: &str, definition: Arc<PropDef>) -> bool {
    let Some(current) = props::get(name) else { return false };
    if !Arc::ptr_eq(&current, &definition) && current.key != definition.key {
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
        for path in &ready {
            self.pending.remove(path);
        }
        updates.extend(self.reload_batch(ready));
        updates
    }

    /// Files that became ready together. One can reference another later in the batch, so the
    /// failures are retried while each round loads something; the last round's errors remain.
    fn reload_batch(&mut self, mut queue: Vec<PathBuf>) -> Vec<AssetUpdate> {
        let mut updates = Vec::new();
        let mut errors = Vec::new();
        while !queue.is_empty() {
            let attempted = queue.len();
            errors.clear();
            for path in std::mem::take(&mut queue) {
                let results = self.reload(&path);
                if results.iter().any(Result::is_err) {
                    errors.extend(results);
                    queue.push(path);
                } else {
                    updates.extend(results);
                }
            }
            if queue.len() == attempted {
                break;
            }
        }
        updates.extend(errors);
        updates
    }

    /// The asset a changed file holds, then the assets that reference it (recompiled with it).
    fn reload(&mut self, path: &Path) -> Vec<AssetUpdate> {
        match pav_tools::asset_tools::reload_file(path) {
            Ok(published) if !published.is_empty() => {
                self.failed.remove(path);
                published.into_iter().map(Ok).collect()
            }
            Ok(_) => {
                // Restoring the exact last-good bytes is a recovery even though the
                // library is already correct. Report it once so the panel clears its error.
                if self.failed.remove(path) && path.is_file() {
                    let Some(leaf) = path.file_stem().and_then(|leaf| leaf.to_str()) else { return Vec::new() };
                    let name = format!("WORKSHOP/{leaf}");
                    return pav_core::props::get(&name).map(|def| Ok((name, def))).into_iter().collect();
                }
                Vec::new()
            }
            Err(error) => {
                self.failed.insert(path.to_owned());
                vec![Err(format!("{}: {error}; the last valid object stays active", path.display()))]
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
        let mut doc = pav_core::props::get("BUILTIN/box").unwrap().doc.clone();
        doc.name = "test_watcher_recovery".into();
        let text = doc.text();
        std::fs::write(&path, &text).unwrap();
        let mut watcher = AssetWatcher::new();
        let (_, installed) = watcher.reload(&path).remove(0).unwrap();
        assert!(watcher.reload(&path).is_empty(), "an ordinary duplicate save stays quiet");

        std::fs::write(&path, "{unfinished").unwrap();
        assert!(watcher.reload(&path).remove(0).is_err());
        assert!(Arc::ptr_eq(&installed, &pav_core::props::get(name).unwrap()));
        std::fs::write(&path, text).unwrap();
        let (recovered_name, recovered) = watcher.reload(&path).remove(0).unwrap();
        assert_eq!(recovered_name, name);
        assert!(Arc::ptr_eq(&installed, &recovered), "recovery must retain the installed definition");
        assert!(watcher.reload(&path).is_empty(), "one recovery clears the error; further saves stay quiet");

        pav_core::props::remove(name).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_file_that_references_a_later_one_in_the_batch_loads() {
        let root = std::env::temp_dir().join(format!("pav-asset-watch-batch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("workshop")).unwrap();
        let file = |leaf: &str, parts: &str| {
            let path = root.join(format!("workshop/{leaf}.json"));
            std::fs::write(&path, format!(r#"{{"format":2,"name":"{leaf}","unit":"cm","parts":{parts}}}"#)).unwrap();
            path
        };
        let pair = file("test_watch_batch_pair", r#"{"a":{"ref":"WORKSHOP/test_watch_batch_stool"}}"#);
        let stool = file("test_watch_batch_stool", r##"{"seat":{"type":"cylinder","radius":18,"height":4,"color":"#8a5a35"}}"##);
        let loaded: Vec<String> =
            AssetWatcher::new().reload_batch(vec![pair, stool]).into_iter().map(|update| update.unwrap().0).collect();
        assert_eq!(loaded, ["WORKSHOP/test_watch_batch_stool", "WORKSHOP/test_watch_batch_pair"]);
        assert_eq!(props::get("WORKSHOP/test_watch_batch_pair").unwrap().compiled.parts.len(), 1);

        props::remove("WORKSHOP/test_watch_batch_pair").unwrap();
        props::remove("WORKSHOP/test_watch_batch_stool").unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_stale_queued_file_cannot_replace_a_newer_live_instance() {
        let name = "WORKSHOP/test_watcher_stale";
        let mut asset = (**props::get("BUILTIN/bench").unwrap().v1.as_ref().unwrap()).clone();
        asset.name = "test_watcher_stale".into();
        props::install(name, PropDef::from_v1(Arc::new(asset.clone())).unwrap()).unwrap();
        let first = props::get(name).unwrap();
        let mut sim = Sim::empty(3);
        let id = sim.spawn_prop(name, glam::Vec3::ZERO, glam::Quat::IDENTITY, 1.0, true, None).unwrap();
        let body = sim.state.entities.get(id).unwrap().body;

        asset.parts.remove("crossbar");
        props::install(name, PropDef::from_v1(Arc::new(asset)).unwrap()).unwrap();
        let latest = props::get(name).unwrap();
        assert!(apply_update(&mut sim, name, latest.clone()));
        assert!(!apply_update(&mut sim, name, first), "an older queued file must be skipped");
        let entity = sim.state.entities.get(id).unwrap();
        assert!(Arc::ptr_eq(&entity.prop.as_ref().unwrap().definition, &latest));
        assert_eq!(entity.body, body);
        let collider = sim.state.physics.bodies.get(body.unwrap()).unwrap().colliders()[0];
        let shape = sim.state.physics.colliders.get(collider).unwrap().shape();
        assert_eq!(shape.as_compound().unwrap().shapes().len(), latest.compiled.parts.len());
        assert!(apply_update(&mut sim, name, latest.clone()), "the current revision still applies");

        props::remove(name).unwrap();
        assert!(!apply_update(&mut sim, name, latest), "a removed asset must not be restored by a queued file");
    }
}
