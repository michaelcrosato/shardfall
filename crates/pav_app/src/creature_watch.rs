//! Native source-file edits enter the same job queue as UI and MCP edits.
//! The compiler runs elsewhere; the watcher only debounces and queues readable sources.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    use serde_json::Value;

    #[derive(Default)]
    pub struct CreatureWatcher {
        watcher: Option<notify::RecommendedWatcher>,
        receiver: Option<Receiver<Result<PathBuf, String>>>,
        directory: Option<PathBuf>,
        attempted: bool,
        pending: BTreeMap<PathBuf, Instant>,
        seen: BTreeMap<PathBuf, Option<Vec<u8>>>,
    }

    fn source_file(path: &Path, directory: &Path) -> bool {
        path.parent() == Some(directory)
            && path.extension().and_then(|name| name.to_str()) == Some("json")
            && path.file_name().and_then(|name| name.to_str()).is_some_and(|name| !name.starts_with('.'))
    }

    impl CreatureWatcher {
        pub fn new() -> Self {
            Self::default()
        }

        fn attach(&mut self, directory: &Path) -> Result<(), String> {
            use notify::Watcher;
            let directory = directory.canonicalize().map_err(|error| error.to_string())?;
            let watched = directory.clone();
            let (sender, receiver) = channel();
            let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
                Ok(event) if !matches!(event.kind, notify::EventKind::Access(_)) => {
                    for path in event.paths.into_iter().filter(|path| source_file(path, &watched)) {
                        let _ = sender.send(Ok(path));
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    let _ = sender.send(Err(format!("Creature watcher: {error}")));
                }
            })
            .map_err(|error| error.to_string())?;
            watcher.watch(&directory, notify::RecursiveMode::NonRecursive).map_err(|error| error.to_string())?;
            for entry in std::fs::read_dir(&directory).map_err(|error| error.to_string())? {
                let path = entry.map_err(|error| error.to_string())?.path();
                if source_file(&path, &directory) {
                    self.pending.insert(path, Instant::now());
                }
            }
            self.directory = Some(directory);
            self.receiver = Some(receiver);
            self.watcher = Some(watcher);
            Ok(())
        }

        pub fn poll(&mut self) -> Vec<Result<Value, String>> {
            let mut reports = Vec::new();
            if self.watcher.is_some() && self.directory.as_ref().is_some_and(|path| !path.is_dir()) {
                self.watcher = None;
                self.receiver = None;
                self.directory = None;
                self.pending.clear();
                self.seen.clear();
                self.attempted = false;
                reports.push(Err("The creature source folder was removed. The last valid preview stays active.".into()));
            }
            if self.watcher.is_none() && !self.attempted {
                let directory = pav_tools::creature_tools::root().join("workshop");
                if !directory.is_dir() {
                    return reports;
                }
                self.attempted = true;
                if let Err(error) = self.attach(&directory) {
                    reports.push(Err(format!("Cannot watch creature sources at {}: {error}", directory.display())));
                    return reports;
                }
            }
            if let Some(receiver) = &self.receiver {
                for event in receiver.try_iter() {
                    match event {
                        Ok(path) => {
                            self.pending.insert(path, Instant::now());
                        }
                        Err(error) => reports.push(Err(error)),
                    }
                }
            }
            let ready: Vec<_> = self
                .pending
                .iter()
                .filter(|(_, changed)| changed.elapsed() >= Duration::from_millis(250))
                .map(|(path, _)| path.clone())
                .collect();
            for path in ready {
                self.pending.remove(&path);
                let bytes = std::fs::read(&path).ok();
                if self.seen.get(&path) == Some(&bytes) {
                    continue;
                }
                match pav_tools::creature_tools::enqueue_reload(&path) {
                    Ok(report) if report["retryable"] == true => {
                        self.pending.insert(path, Instant::now());
                    }
                    Ok(report) => {
                        self.seen.insert(path, bytes);
                        if report["changed"] != false {
                            reports.push(Ok(report));
                        }
                    }
                    Err(error) => {
                        self.seen.insert(path, bytes);
                        reports.push(Err(format!("Creature reload: {error:#}. The last valid preview stays active.")));
                    }
                }
            }
            reports
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::CreatureWatcher;

#[cfg(target_arch = "wasm32")]
pub struct CreatureWatcher;

#[cfg(target_arch = "wasm32")]
impl CreatureWatcher {
    pub fn new() -> Self {
        Self
    }
    pub fn poll(&mut self) -> Vec<Result<serde_json::Value, String>> {
        Vec::new()
    }
}
