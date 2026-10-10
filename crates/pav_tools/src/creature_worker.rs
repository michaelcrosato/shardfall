//! One persistent SpawnForge compiler process. Only the creature job thread calls it;
//! game and tool requests enqueue work without waiting for JavaScript or mesh generation.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

const RESPONSE_LIMIT: u64 = 128 * 1024 * 1024;
const BUILD_TIMEOUT: Duration = Duration::from_secs(120);

/// Locate the packaged compiler beside the executable, then in the source checkout.
pub fn entry_path() -> PathBuf {
    if let Some(path) = std::env::var_os("PAV_CREATURE_WORKER") {
        return PathBuf::from(path);
    }
    if let Some(exe) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_owned)) {
        for candidate in [exe.join("tools/creature-compiler/worker.mjs"), exe.join("creature-compiler/worker.mjs")] {
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/creature-compiler/worker.mjs")
}

pub fn package_root() -> PathBuf {
    entry_path().parent().expect("compiler has a parent directory").to_owned()
}

fn node_path(package: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("PAV_NODE") {
        return path.into();
    }
    let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_owned));
    for parent in [Some(package.to_owned()), exe].into_iter().flatten() {
        for name in ["node.exe", "node"] {
            let path = parent.join("runtime").join(name);
            if path.is_file() {
                return path;
            }
        }
    }
    "node".into()
}

struct Process {
    child: Child,
    input: ChildStdin,
    replies: Receiver<Result<String, String>>,
    errors: Arc<Mutex<VecDeque<String>>>,
}

impl Process {
    fn start() -> Result<Self> {
        let entry = entry_path();
        let entry = std::fs::canonicalize(&entry).with_context(|| {
            format!(
                "creature compiler is missing at {}; install tools/creature-compiler or set PAV_CREATURE_WORKER",
                entry.display()
            )
        })?;
        let package = entry.parent().expect("compiler has a parent");
        let node = node_path(package);
        let mut command = Command::new(&node);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW: sidecar stdio is piped.
        }
        let mut child = command
            .arg(&entry)
            .current_dir(package)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("start {}: install Node 22.18+ or set PAV_NODE to its executable", node.display()))?;
        let input = child.stdin.take().expect("piped compiler stdin");
        let output = child.stdout.take().expect("piped compiler stdout");
        let stderr = child.stderr.take().expect("piped compiler stderr");
        let (tx, replies) = sync_channel(1);
        std::thread::Builder::new().name("creature compiler output".into()).spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                let read = (&mut reader).take(RESPONSE_LIMIT + 1).read_until(b'\n', &mut bytes);
                let result = match read {
                    Ok(0) => break,
                    Ok(_) if bytes.len() as u64 > RESPONSE_LIMIT => {
                        let _ = tx.send(Err("creature compiler response exceeds 128 MiB".into()));
                        break;
                    }
                    Ok(_) => String::from_utf8(bytes).map_err(|e| format!("compiler output is not UTF-8: {e}")),
                    Err(error) => Err(format!("read creature compiler output: {error}")),
                };
                if tx.send(result).is_err() {
                    break;
                }
            }
        })?;
        let errors = Arc::new(Mutex::new(VecDeque::new()));
        let recent = errors.clone();
        std::thread::Builder::new().name("creature compiler diagnostics".into()).spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Ok(mut recent) = recent.lock() {
                    recent.push_back(line.chars().take(2000).collect());
                    while recent.len() > 8 {
                        recent.pop_front();
                    }
                }
            }
        })?;
        Ok(Self { child, input, replies, errors })
    }

    fn exchange(&mut self, request: &Value) -> Result<Value> {
        serde_json::to_writer(&mut self.input, request).context("send creature build")?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        let line = self
            .replies
            .recv_timeout(BUILD_TIMEOUT)
            .map_err(|error| {
                let recent =
                    self.errors.lock().map(|lines| lines.iter().cloned().collect::<Vec<_>>().join("; ")).unwrap_or_default();
                anyhow!("creature compiler did not finish within 120 seconds or exited ({error}); {recent}")
            })?
            .map_err(|error| anyhow!(error))?;
        let reply: Value = serde_json::from_str(&line).context("creature compiler returned invalid JSON")?;
        if reply.get("id") != request.get("id") {
            bail!("creature compiler returned a different request id; its process will restart");
        }
        Ok(reply)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
pub(crate) struct Worker {
    process: Option<Process>,
}

impl Worker {
    pub(crate) fn request(&mut self, request: &Value) -> Result<Value> {
        if self.process.is_none() {
            self.process = Some(Process::start()?);
        }
        match self.process.as_mut().expect("compiler started").exchange(request) {
            Ok(reply) => Ok(reply),
            Err(error) => {
                self.process = None;
                Err(error)
            }
        }
    }
}
