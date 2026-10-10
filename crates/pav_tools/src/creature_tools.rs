//! Revision-checked creature sources, asynchronous compilation and live publication.
//! Source JSON is authoritative; compiled JSON is a disposable, validated cache. A job
//! publishes on the session thread only after the latest requested source was saved.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use pav_core::creatures::{self, CreatureAsset};
use pav_core::prop_preview::StudioMode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Session;
use crate::creature_worker::{self, Worker};
use crate::live_feedback::SharedFeedback;
use crate::tools::{Args, Output, get_bool};

const HISTORY_LIMIT: usize = 32;
const JOB_LIMIT: usize = 128;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);
static OWNER_ID: AtomicU64 = AtomicU64::new(1);
static INITIALIZED: Once = Once::new();
static AUTHORED: OnceLock<Mutex<BTreeMap<String, Value>>> = OnceLock::new();
static CATALOG: OnceLock<Value> = OnceLock::new();

fn authored() -> &'static Mutex<BTreeMap<String, Value>> {
    AUTHORED.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// The last accepted source and history, including an absent source with redo history.
/// This is a memory read: a human draft can compare revisions without tool calls or I/O.
pub fn authored_snapshot(name: &str) -> Option<Value> {
    let name = creatures::canonical(name).ok()?;
    authored().lock().ok()?.get(&name).cloned()
}

/// Bundled discovery data is available before a compiler or authored directory exists.
pub fn catalog_snapshot() -> Value {
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../../tools/creature-compiler/catalog.json")).expect("bundled creature catalog")
        })
        .clone()
}

pub(crate) fn new_owner() -> u64 {
    OWNER_ID.fetch_add(1, Ordering::Relaxed)
}

/// All native front ends share this directory. Cache and history stay out of the source folder.
pub fn root() -> PathBuf {
    let path = std::env::var_os("PAV_CREATURES").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("assets/creatures"));
    if path.is_absolute() { path } else { std::env::current_dir().unwrap_or_default().join(path) }
}

fn leaf(name: &str) -> &str {
    name.split_once('/').map_or(name, |(_, leaf)| leaf)
}

fn canonical(name: &str) -> Result<String> {
    creatures::canonical(name).map_err(|e| anyhow!(e))
}

fn source_path(root: &Path, name: &str) -> PathBuf {
    root.join("workshop").join(format!("{}.json", leaf(name)))
}

fn history_path(root: &Path, name: &str) -> PathBuf {
    root.join(".editor").join(format!("{}.json", leaf(name)))
}

fn cache_path(root: &Path, source: &Source) -> PathBuf {
    root.join(".compiled").join(format!("{}--{}.json", source.name, source.revision()))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    format: u32,
    name: String,
    quality: String,
    blueprint: Value,
}

impl Source {
    fn check(&self, name: &str) -> Result<()> {
        if self.format != 1 || self.name != leaf(name) || canonical(&self.name)? != name {
            bail!("creature source must have format=1 and name='{}'", leaf(name));
        }
        quality(&self.quality)?;
        if !self.blueprint.is_object() {
            bail!("blueprint must be a SpawnForge JSON object");
        }
        Ok(())
    }

    fn revision(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("source serializes");
        let hash =
            bytes.into_iter().fold(0xcbf29ce484222325u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
        format!("{hash:016x}")
    }
}

fn quality(value: &str) -> Result<&str> {
    if !matches!(value, "low" | "medium" | "high") {
        bail!("quality must be low, medium, or high");
    }
    Ok(value)
}

fn revision(source: Option<&Source>) -> String {
    source.map(Source::revision).unwrap_or_else(|| "absent".into())
}

fn read_source(root: &Path, name: &str) -> Result<Option<Source>> {
    let path = source_path(root, name);
    if !path.try_exists().with_context(|| format!("check {}", path.display()))? {
        return Ok(None);
    }
    let repair = || {
        format!(
            "read creature source {}; repair this JSON file, then let the watcher reload it or use creature_edit action=rebuild",
            path.display()
        )
    };
    let source: Source = serde_json::from_slice(&std::fs::read(&path)?).with_context(repair)?;
    source.check(name).with_context(repair)?;
    Ok(Some(source))
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct History {
    head: String,
    undo: Vec<Option<Source>>,
    redo: Vec<Option<Source>>,
}

fn read_history(root: &Path, name: &str, current: &str) -> Result<History> {
    let path = history_path(root, name);
    if !path.is_file() {
        return Ok(History { head: current.into(), ..Default::default() });
    }
    let history: History = serde_json::from_slice(&std::fs::read(&path)?).with_context(|| format!("read {}", path.display()))?;
    if history.undo.len() > HISTORY_LIMIT || history.redo.len() > HISTORY_LIMIT {
        bail!("creature history exceeds {HISTORY_LIMIT} steps");
    }
    Ok(history)
}

fn push(stack: &mut Vec<Option<Source>>, value: Option<Source>) {
    stack.push(value);
    if stack.len() > HISTORY_LIMIT {
        stack.remove(0);
    }
}

fn lock_editor(root: &Path, wait: bool) -> Result<File> {
    let dir = root.join(".editor");
    std::fs::create_dir_all(&dir)?;
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("lock"))?;
    if wait {
        file.lock().context("lock creature sources")?;
    } else {
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Err(EditorBusy.into()),
            Err(std::fs::TryLockError::Error(error)) => return Err(error).context("lock creature sources"),
        }
    }
    Ok(file)
}

#[derive(Debug)]
struct EditorBusy;
impl std::fmt::Display for EditorBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a creature source is being saved; try the edit again")
    }
}
impl std::error::Error for EditorBusy {}

struct PreparedFile {
    temporary: PathBuf,
    target: PathBuf,
}

impl PreparedFile {
    fn new(target: &Path, bytes: &[u8]) -> Result<Self> {
        let parent = target.parent().ok_or_else(|| anyhow!("file has no parent"))?;
        std::fs::create_dir_all(parent)?;
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary =
            parent.join(format!(".{}.{}.{id}.tmp", target.file_name().unwrap().to_string_lossy(), std::process::id()));
        let prepared = Self { temporary, target: target.into() };
        let mut file = OpenOptions::new().write(true).create_new(true).open(&prepared.temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(prepared)
    }

    fn publish(&self) -> Result<()> {
        std::fs::rename(&self.temporary, &self.target).with_context(|| format!("replace {}", self.target.display()))
    }
}

impl Drop for PreparedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.temporary);
    }
}

#[derive(Clone)]
struct PreviewRequest {
    mode: StudioMode,
    selected: Option<String>,
}

struct Job {
    id: u64,
    owner: u64,
    root: PathBuf,
    name: String,
    key: String,
    action: String,
    expected: String,
    before: Option<Source>,
    history: History,
    history_reset: bool,
    request: Option<Value>,
    quality: String,
    preview: Option<PreviewRequest>,
    feedback: Option<SharedFeedback>,
    started: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Queued,
    Building,
    Ready,
    Published,
    Failed,
    Superseded,
}

impl Phase {
    fn finished(self) -> bool {
        matches!(self, Self::Published | Self::Failed | Self::Superseded)
    }
}

struct Record {
    owner: u64,
    phase: Phase,
    info: Value,
}

struct Completion {
    job: Job,
    built: Option<Built>,
}

#[derive(Debug)]
struct Built {
    asset: Option<Arc<CreatureAsset>>,
    report: Value,
    authored: Value,
}

#[derive(Default)]
struct Book {
    next: u64,
    latest: BTreeMap<String, u64>,
    preview: BTreeMap<u64, u64>,
    jobs: BTreeMap<u64, Record>,
    ready: VecDeque<Completion>,
}

struct Service {
    book: Arc<Mutex<Book>>,
    work: Sender<Job>,
}

static SERVICE: OnceLock<Service> = OnceLock::new();

fn service() -> &'static Service {
    SERVICE.get_or_init(|| {
        let book = Arc::new(Mutex::new(Book::default()));
        let (work, jobs) = channel::<Job>();
        let state = book.clone();
        std::thread::Builder::new()
            .name("creature builds".into())
            .spawn(move || {
                let mut worker = Worker::default();
                for job in jobs {
                    {
                        let mut book = state.lock().unwrap();
                        if book.latest.get(&job.key) != Some(&job.id) {
                            continue;
                        }
                        let record = book.jobs.get_mut(&job.id).expect("queued job exists");
                        record.phase = Phase::Building;
                        record.info["state"] = json!(Phase::Building);
                    }
                    let built = build_and_save(&job, &mut worker, &state);
                    let mut book = state.lock().unwrap();
                    if book.latest.get(&job.key) != Some(&job.id) {
                        continue;
                    }
                    let record = book.jobs.get_mut(&job.id).expect("building job exists");
                    let built = match built {
                        Ok(built) => {
                            record.phase = Phase::Ready;
                            merge(&mut record.info, &built.report);
                            record.info["state"] = json!(Phase::Ready);
                            Some(built)
                        }
                        Err(error) => {
                            record.phase = Phase::Failed;
                            record.info["state"] = json!(Phase::Failed);
                            record.info["error"] = json!(format!("{error:#}"));
                            if let Some(compiler) = error.downcast_ref::<CompilerError>() {
                                record.info["issues"] = compiler.issues.clone();
                            }
                            None
                        }
                    };
                    book.ready.push_back(Completion { job, built });
                }
            })
            .expect("spawn creature build worker");
        Service { book, work }
    })
}

fn merge(target: &mut Value, fields: &Value) {
    if let (Some(target), Some(fields)) = (target.as_object_mut(), fields.as_object()) {
        target.extend(fields.clone());
    }
}

#[derive(Debug)]
struct CompilerError {
    message: String,
    issues: Value,
}

impl std::fmt::Display for CompilerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CompilerError {}

fn build_and_save(job: &Job, worker: &mut Worker, state: &Mutex<Book>) -> Result<Built> {
    build_with(job, |request| worker.request(request), state)
}

fn build_with(job: &Job, mut compile: impl FnMut(&Value) -> Result<Value>, state: &Mutex<Book>) -> Result<Built> {
    let (source, asset, details) = if let Some(request) = &job.request {
        let mut request = request.clone();
        request["id"] = json!(job.id);
        let reply = compile(&request)?;
        if reply.get("ok") != Some(&Value::Bool(true)) {
            return Err(CompilerError {
                message: reply.get("error").and_then(Value::as_str).unwrap_or("creature compilation failed").into(),
                issues: reply.get("issues").cloned().unwrap_or_else(|| json!([])),
            }
            .into());
        }
        let result = reply.get("result").ok_or_else(|| anyhow!("compiler reply has no result"))?;
        let source = Source {
            format: 1,
            name: leaf(&job.name).into(),
            quality: job.quality.clone(),
            blueprint: result.get("blueprint").cloned().ok_or_else(|| anyhow!("compiler reply has no blueprint"))?,
        };
        source.check(&job.name)?;
        let mut raw = result.get("asset").cloned().ok_or_else(|| anyhow!("compiler reply has no native asset"))?;
        if !raw.is_object() {
            bail!("compiler asset must be an object");
        }
        raw["source_revision"] = json!(source.revision());
        let asset: CreatureAsset = serde_json::from_value(raw).context("validate compiled creature")?;
        if asset.name != source.name {
            bail!("compiler returned name '{}' instead of '{}'", asset.name, source.name);
        }
        if serde_json::to_value(&asset.quality)? != json!(source.quality) {
            bail!("compiler returned a different mesh quality");
        }
        let details = json!({
            "diff": result.get("diff"), "warnings": result.get("warnings"),
            "timings": result.get("timings"), "compile_ms": result.get("compile_ms"),
            "build_kind": result.get("build_kind"), "stats": result.get("stats"),
        });
        (Some(source), Some(Arc::new(asset)), details)
    } else {
        (None, None, json!({}))
    };
    let next_revision = revision(source.as_ref());
    let changed = next_revision != job.expected;
    let mut history = job.history.clone();
    if changed && !matches!(job.action.as_str(), "undo" | "redo") {
        push(&mut history.undo, job.before.clone());
        history.redo.clear();
    }
    history.head = next_revision.clone();

    // Large cache encoding and writes run here, never on the simulation thread. The
    // source file is the atomic pointer to a cache revision; unused caches are harmless.
    if let (Some(source), Some(asset)) = (&source, &asset) {
        PreparedFile::new(&cache_path(&job.root, source), &serde_json::to_vec(asset.as_ref())?)?.publish()?;
    }
    let new_source = source
        .as_ref()
        .map(|source| {
            let mut text = serde_json::to_vec_pretty(source)?;
            text.push(b'\n');
            PreparedFile::new(&source_path(&job.root, &job.name), &text)
        })
        .transpose()?;
    let new_history = PreparedFile::new(&history_path(&job.root, &job.name), &serde_json::to_vec(&history)?)?;
    let _file_lock = lock_editor(&job.root, true)?;
    let book = state.lock().unwrap();
    if book.latest.get(&job.key) != Some(&job.id) {
        bail!("superseded by a newer creature edit");
    }
    let current = read_source(&job.root, &job.name)?;
    let current_revision = revision(current.as_ref());
    if current_revision != job.expected {
        bail!(
            "revision conflict for {}: build started from {}, current source is {}; inspect and edit again",
            job.name,
            job.expected,
            current_revision
        );
    }
    if changed || job.action == "reload" {
        let history_file = history_path(&job.root, &job.name);
        let old_history = std::fs::read(&history_file).ok();
        new_history.publish()?;
        let saved = match &new_source {
            Some(source) => source.publish(),
            None => std::fs::remove_file(source_path(&job.root, &job.name)).context("remove creature source"),
        };
        if let Err(error) = saved {
            let restored = match old_history {
                Some(bytes) => PreparedFile::new(&history_file, &bytes).and_then(|file| file.publish()),
                None => std::fs::remove_file(&history_file).map_err(anyhow::Error::from),
            };
            return Err(match restored {
                Ok(()) => error,
                Err(restore) => error.context(format!("source is unchanged; history restore failed: {restore:#}")),
            });
        }
    }
    drop(book);
    let mut report = json!({
        "name":job.name,"kind":"creature","revision":next_revision,"saved":changed || job.action == "reload", "changed":changed,
        "file":source_path(&job.root,&job.name),"undo":history.undo.len(),"redo":history.redo.len(),
        "history_reset":job.history_reset,"build_ms":job.started.elapsed().as_secs_f64()*1000.0,
        "removed":source.is_none(),
        "asset_revision":asset.as_ref().map(|asset| asset.revision()),
    });
    merge(&mut report, &details);
    let authored = source_info(&job.root, &job.name, source.as_ref(), &history, asset.as_ref());
    Ok(Built { asset, report, authored })
}

fn enqueue(mut job: Job) -> Result<Value> {
    if cfg!(target_arch = "wasm32") {
        bail!("creature compilation runs in the native studio; the browser can view compiled creatures");
    }
    let service = service();
    let mut book = service.book.lock().unwrap();
    if book.jobs.values().filter(|record| !record.phase.finished()).count() >= 32 {
        bail!("32 creature builds are pending; wait for creature_status before queuing more");
    }
    book.next += 1;
    job.id = book.next;
    let id = job.id;
    if let Some(previous) = book.latest.insert(job.key.clone(), id) {
        if let Some(record) = book.jobs.get_mut(&previous) {
            if !record.phase.finished() {
                record.phase = Phase::Superseded;
                record.info["state"] = json!(Phase::Superseded);
                record.info["superseded_by"] = json!(id);
            }
        }
    }
    if job.preview.is_some() {
        book.preview.insert(job.owner, id);
    }
    let info = json!({
        "job":id,"name":job.name,"kind":"creature","action":job.action,"state":Phase::Queued,
        "base_revision":job.expected,"status_tool":"creature_status","preview_requested":job.preview.is_some(),
    });
    book.jobs.insert(id, Record { owner: job.owner, phase: Phase::Queued, info: info.clone() });
    while book.jobs.len() > JOB_LIMIT {
        let old = book.jobs.iter().find(|(_, record)| record.phase.finished()).map(|(id, _)| *id);
        if let Some(old) = old {
            book.jobs.remove(&old);
        } else {
            break;
        }
    }
    drop(book);
    service.work.send(job).map_err(|_| anyhow!("creature build worker stopped"))?;
    Ok(info)
}

/// True only when the live app has a completion to adopt (including a failed job to report).
pub fn has_updates() -> bool {
    SERVICE.get().is_some_and(|service| service.book.lock().unwrap().ready.iter().any(|done| done.job.owner == 0))
}

pub(crate) fn snapshot(owner: u64) -> Value {
    let assets = authored()
        .lock()
        .unwrap()
        .iter()
        .map(|(name, info)| {
            let mut info = info.clone();
            if let Some(fields) = info.as_object_mut() {
                fields.remove("source");
                fields.remove("blueprint");
            }
            (name.clone(), info)
        })
        .collect::<BTreeMap<_, _>>();
    let Some(service) = SERVICE.get() else { return json!({"jobs":[],"pending":0,"assets":assets}) };
    let book = service.book.lock().unwrap();
    let jobs = book.jobs.values().rev().filter(|job| job.owner == owner).take(32).map(|job| job.info.clone()).collect::<Vec<_>>();
    let pending = book.jobs.values().filter(|job| job.owner == owner && !job.phase.finished()).count();
    json!({"jobs":jobs,"pending":pending,"assets":assets})
}

/// Read-only live UI status; never starts the compiler or waits for a build.
pub fn status_snapshot() -> Value {
    snapshot(0)
}

/// A live panel can reconcile its own terminal job even after it leaves the recent list.
pub fn job_snapshot(id: u64) -> Option<Value> {
    SERVICE.get()?.book.lock().ok()?.jobs.get(&id).filter(|record| record.owner == 0).map(|record| record.info.clone())
}

/// Adopt completed builds on their owning session. It does not wait for the compiler.
/// The live app calls this through Session::from_live and adopts its camera and frame ticket.
pub fn poll(session: &mut Session) -> Vec<Value> {
    let Some(service) = SERVICE.get() else { return Vec::new() };
    poll_ready(session, service)
}

fn poll_ready(session: &mut Session, service: &Service) -> Vec<Value> {
    let completed = {
        let mut book = service.book.lock().unwrap();
        let (mine, rest): (VecDeque<_>, VecDeque<_>) =
            book.ready.drain(..).partition(|done| done.job.owner == session.creature_owner);
        book.ready = rest;
        mine
    };
    let mut notes = Vec::new();
    for completion in completed {
        // Use the same lock order as saving. A source save in flight is retried next frame;
        // compilation never makes this path wait on a file or a compiler worker.
        let file_lock = match lock_editor(&completion.job.root, false) {
            Ok(lock) => lock,
            Err(error) if error.is::<EditorBusy>() => {
                service.book.lock().unwrap().ready.push_back(completion);
                continue;
            }
            Err(error) => {
                let mut book = service.book.lock().unwrap();
                if let Some(record) = book.jobs.get_mut(&completion.job.id) {
                    record.phase = Phase::Failed;
                    record.info["state"] = json!(Phase::Failed);
                    record.info["error"] = json!(format!("publish creature: {error:#}"));
                    notes.push(record.info.clone());
                }
                continue;
            }
        };
        let job = completion.job;
        // A new acceptance cannot overtake the final source check and live installation.
        let mut book = service.book.lock().unwrap();
        if book.latest.get(&job.key) != Some(&job.id) {
            continue;
        }
        let desired_preview = book.preview.get(&job.owner) == Some(&job.id);
        let Some(built) = completion.built else {
            if let Some(record) = book.jobs.get(&job.id) {
                notes.push(record.info.clone());
            }
            continue;
        };
        let mut report = built.report;
        let current = read_source(&job.root, &job.name).map(|source| revision(source.as_ref()));
        let published = current.and_then(|current| {
            if report["revision"].as_str() != Some(current.as_str()) {
                bail!("source changed before publication; queued geometry was discarded (current revision {current})");
            }
            match &built.asset {
                Some(asset) => creatures::install(&job.name, asset.clone()).map(|_| {
                    session.sim.refresh_creature_preview(&job.name, asset);
                }),
                None => creatures::remove(&job.name).map(|_| ()),
            }
            .map_err(|error| anyhow!(error))
        });
        if let Err(error) = published {
            report["state"] = json!(Phase::Failed);
            report["error"] = json!(format!("{error:#}"));
        } else {
            let selected = session.sim.state.creature_preview.as_ref().map(|state| state.name.clone());
            if built.asset.is_none() && selected.as_deref() == Some(&job.name) {
                // Undoing creation keeps the workspace open on its empty stage, with redo
                // history still available to both the human panel and the agent.
                session.sim.state.creature_preview = None;
            } else if desired_preview
                && built.asset.is_some()
                && job
                    .preview
                    .as_ref()
                    .is_some_and(|request| request.mode == session.sim.studio_mode() && request.selected == selected)
            {
                if let Err(error) = crate::creature_preview_tools::t_creature_preview(
                    session,
                    &json!({"name":job.name}).as_object().unwrap().clone(),
                ) {
                    report["preview_error"] = json!(format!("{error:#}"));
                }
            }
            report["state"] = json!(Phase::Published);
            authored().lock().unwrap().insert(job.name.clone(), built.authored);
            if let Some(feedback) = job.feedback.as_ref().or(session.feedback.as_ref()) {
                if let Ok(mut feedback) = feedback.lock() {
                    let note = feedback.accepted("creature_publish", job.started);
                    session.sim.live_edit_ticket = note["ticket"].as_u64().unwrap_or_default();
                    report["feedback"] = note;
                }
            }
        }
        session.prev_frame = session.sim.frame();
        report["job"] = json!(job.id);
        if let Some(record) = book.jobs.get_mut(&job.id) {
            record.phase = if report["state"] == "published" { Phase::Published } else { Phase::Failed };
            merge(&mut record.info, &report);
            notes.push(record.info.clone());
        }
        drop(book);
        drop(file_lock);
    }
    notes
}

fn fields(a: &Args, allowed: &[&str]) -> Result<()> {
    for key in a.keys() {
        if !["scene", "seed", "ticks"].contains(&key.as_str()) && !allowed.contains(&key.as_str()) {
            bail!("unknown or unused creature argument '{key}'");
        }
    }
    Ok(())
}

fn text<'a>(a: &'a Args, key: &str) -> Result<Option<&'a str>> {
    a.get(key).map(|value| value.as_str().ok_or_else(|| anyhow!("{key} must be text"))).transpose()
}

fn value(a: &Args, key: &str) -> Result<Value> {
    match a.get(key).ok_or_else(|| anyhow!("{key}= is required"))? {
        Value::String(text) => serde_json::from_str(text).with_context(|| format!("invalid JSON in {key}")),
        value => Ok(value.clone()),
    }
}

fn preview_request(s: &Session, wanted: bool) -> Option<PreviewRequest> {
    wanted.then(|| PreviewRequest {
        mode: s.sim.studio_mode(),
        selected: s.sim.state.creature_preview.as_ref().map(|state| state.name.clone()),
    })
}

fn prepare(root: PathBuf, name: String, action: &str, owner: u64, before: Option<Source>, mut history: History) -> Result<Job> {
    let expected = revision(before.as_ref());
    let history_reset = history.head != expected;
    if history_reset {
        if matches!(action, "undo" | "redo") {
            bail!("the creature source changed outside the editor; inspect it before undoing");
        }
        history = History { head: expected.clone(), ..Default::default() };
    }
    let quality = before.as_ref().map(|source| source.quality.clone()).unwrap_or_else(|| "medium".into());
    let key = source_path(&root, &name).to_string_lossy().into_owned();
    Ok(Job {
        id: 0,
        owner,
        root,
        name,
        key,
        action: action.into(),
        expected,
        before,
        history,
        history_reset,
        request: None,
        quality,
        preview: None,
        feedback: None,
        started: Instant::now(),
    })
}

/// Compile a changed workshop source. The watcher queues work; it never installs geometry.
pub fn enqueue_reload(path: &Path) -> Result<Value> {
    if path.extension().is_none_or(|ext| ext != "json")
        || path.parent().and_then(Path::file_name).is_none_or(|name| name != "workshop")
    {
        bail!("creature reload expects workshop/<name>.json");
    }
    let name = canonical(path.file_stem().and_then(|name| name.to_str()).ok_or_else(|| anyhow!("invalid creature file name"))?)?;
    let root = path.parent().and_then(Path::parent).ok_or_else(|| anyhow!("creature source has no root"))?.to_owned();
    let _lock = match lock_editor(&root, false) {
        Ok(lock) => lock,
        Err(error) if error.is::<EditorBusy>() => return Ok(json!({"name":name,"state":"busy","retryable":true})),
        Err(error) => return Err(error),
    };
    let source = read_source(&root, &name)?
        .ok_or_else(|| anyhow!("{} was removed; last compiled creature remains available", path.display()))?;
    let history = read_history(&root, &name, &source.revision())?;
    // The external file is already saved, even if its blueprint will fail compilation.
    // Expose its revision so the human can repair it while the old mesh stays live.
    // Different history heads report zero undo/redo until a valid reload resets history.
    let info = source_info(&root, &name, Some(&source), &history, creatures::get(&name).as_ref());
    authored().lock().unwrap().insert(name.clone(), info);
    if creatures::get(&name).is_some_and(|asset| asset.source_revision == source.revision()) {
        return Ok(json!({"name":name,"changed":false,"state":"unchanged"}));
    }
    if let Some(service) = SERVICE.get() {
        let book = service.book.lock().unwrap();
        let key = source_path(&root, &name).to_string_lossy().into_owned();
        if let Some(record) = book.latest.get(&key).and_then(|id| book.jobs.get(id)) {
            let same_saved =
                record.info["revision"] == source.revision() && matches!(record.phase, Phase::Ready | Phase::Published);
            let same_reload = record.info["base_revision"] == source.revision()
                && record.info["action"] == "reload"
                && !record.phase.finished();
            if same_saved || same_reload {
                return Ok(json!({"name":name,"changed":false,"state":"unchanged","job":record.info["job"]}));
            }
        }
    }
    let mut job = prepare(root, name, "reload", 0, Some(source.clone()), history)?;
    job.request = Some(json!({"op":"build","name":source.name,"blueprint":source.blueprint,"quality":source.quality}));
    enqueue(job)
}

/// Load valid compiled caches once, before creating the first scene. Missing caches can be
/// rebuilt with creature_edit action=rebuild; startup never waits for Node.
pub fn initialize_authored() -> Result<()> {
    let mut result = Ok(());
    INITIALIZED.call_once(|| result = load_cached(&root()));
    result
}

fn load_cached(root: &Path) -> Result<()> {
    let folder = root.join("workshop");
    let mut names = std::collections::BTreeSet::new();
    for folder in [folder, root.join(".editor")] {
        if !folder.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&folder)? {
            let path = entry?.path();
            if path.extension().is_some_and(|extension| extension == "json")
                && !path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.'))
            {
                if let Some(name) = path.file_stem().and_then(|name| name.to_str()).and_then(|name| canonical(name).ok()) {
                    names.insert(name);
                }
            }
        }
    }
    let mut errors = Vec::new();
    for name in names {
        let loaded = (|| -> Result<()> {
            // A missing or damaged cache must still leave an editable, rebuildable source.
            inspect(root, &name)?;
            if let Some(source) = read_source(root, &name)? {
                let cache = cache_path(root, &source);
                if cache.is_file() {
                    let asset = CreatureAsset::parse(&std::fs::read_to_string(&cache)?).map_err(|e| anyhow!(e))?;
                    if asset.source_revision != source.revision() || asset.name != source.name {
                        bail!("compiled cache does not match source {name}")
                    }
                    creatures::install(&name, Arc::new(asset)).map_err(|e| anyhow!(e))?;
                }
            }
            inspect(root, &name).map(|_| ())
        })();
        if let Err(error) = loaded {
            errors.push(format!("{name}: {error:#}"));
        }
    }
    if !errors.is_empty() {
        bail!("some creatures did not load: {}", errors.join("; "))
    }
    Ok(())
}

pub fn t_creature_edit(s: &mut Session, a: &Args) -> Result<Output> {
    let action = text(a, "action")?.unwrap_or("inspect");
    let extra: &[&str] = match action {
        "inspect" => &[],
        "create" => &["template", "blueprint", "theme", "seed", "constraints", "quality", "if_revision"],
        "copy" => &["from", "quality", "if_revision"],
        "patch" => &["ops", "quality", "if_revision"],
        "surface" => &["skin", "quality", "if_revision"],
        "replace" => &["blueprint", "quality", "if_revision"],
        "undo" | "redo" => &["if_revision"],
        "rebuild" => &["if_revision", "quality"],
        _ => bail!("action must be create, copy, inspect, patch, surface, replace, undo, redo, or rebuild"),
    };
    let mut allowed = vec!["action", "name", "preview"];
    allowed.extend(extra);
    fields(a, &allowed)?;
    let name = canonical(text(a, "name")?.ok_or_else(|| anyhow!("name= names the creature"))?)?;
    let root = root();
    if action == "inspect" {
        return inspect(&root, &name).map(Output::Json);
    }
    if cfg!(target_arch = "wasm32") {
        bail!("creature authoring needs the native studio and its local compiler")
    }
    let preview = get_bool(a, "preview", true)?;
    let _lock = lock_editor(&root, false)?;
    let before = read_source(&root, &name)?;
    let current = revision(before.as_ref());
    if let Some(expected) = text(a, "if_revision")? {
        if expected != current {
            bail!("revision conflict for {name}: expected {expected}, current {current}; inspect and edit again")
        }
    }
    let history = read_history(&root, &name, &current)?;
    let mut job = prepare(root, name.clone(), action, s.creature_owner, before, history)?;
    if let Some(q) = text(a, "quality")? {
        job.quality = quality(q)?.into();
    }
    let mut request = json!({"op":"build","name":leaf(&name),"quality":job.quality});
    match action {
        "create" => {
            if job.before.is_some() {
                bail!("{name} already exists; use patch or replace")
            }
            let count = ["template", "blueprint", "theme"].iter().filter(|key| a.contains_key(**key)).count();
            if count > 1 {
                bail!("create accepts one of template, blueprint, or theme")
            }
            if let Some(theme) = text(a, "theme")? {
                request["theme"] = json!(theme);
                request["seed"] = json!(
                    a.get("seed")
                        .map(|seed| seed
                            .as_u64()
                            .filter(|seed| *seed <= 9_007_199_254_740_991)
                            .ok_or_else(|| anyhow!("seed must be an integer in 0..9007199254740991")))
                        .transpose()?
                        .unwrap_or(1)
                );
                if a.contains_key("constraints") {
                    let constraints = value(a, "constraints")?;
                    if !constraints.is_object() {
                        bail!("constraints must be a JSON object")
                    }
                    request["constraints"] = constraints;
                }
            } else {
                if a.contains_key("constraints") {
                    bail!("constraints requires theme")
                }
                request["blueprint"] = if a.contains_key("blueprint") {
                    value(a, "blueprint")?
                } else {
                    let template = text(a, "template")?.unwrap_or("ridgeback_stalker");
                    canonical(template)?;
                    if template.contains('/') {
                        bail!("template is a leaf id from creature_catalog")
                    }
                    let path = creature_worker::package_root().join("templates").join(format!("{template}.json"));
                    serde_json::from_slice(
                        &std::fs::read(&path)
                            .with_context(|| format!("read template {}; use creature_catalog", path.display()))?,
                    )?
                };
            }
        }
        "copy" => {
            if job.before.is_some() {
                bail!("{name} already exists; choose a new name")
            }
            let from = canonical(text(a, "from")?.ok_or_else(|| anyhow!("from= names the source creature"))?)?;
            let source = read_source(&job.root, &from)?.ok_or_else(|| anyhow!("no source creature {from}"))?;
            if !a.contains_key("quality") {
                job.quality = source.quality;
                request["quality"] = json!(job.quality);
            }
            request["blueprint"] = source.blueprint;
        }
        "undo" | "redo" => {
            let (from, to) = if action == "undo" {
                (&mut job.history.undo, &mut job.history.redo)
            } else {
                (&mut job.history.redo, &mut job.history.undo)
            };
            let next = from.pop().ok_or_else(|| anyhow!("no edit to {action} for {name}"))?;
            push(to, job.before.clone());
            if let Some(source) = next {
                source.check(&name)?;
                job.quality = source.quality;
                request["quality"] = json!(job.quality);
                request["blueprint"] = source.blueprint;
            } else {
                request = Value::Null;
            }
        }
        _ => {
            let source = job.before.as_ref().ok_or_else(|| anyhow!("no creature {name}; create one first"))?;
            request["blueprint"] = if action == "replace" { value(a, "blueprint")? } else { source.blueprint.clone() };
            if action == "patch" {
                let ops = value(a, "ops")?;
                check_ops(&ops)?;
                request["ops"] = ops;
            } else if action == "surface" {
                let skin = value(a, "skin")?;
                if !skin.is_object() {
                    bail!("skin must be a SpawnForge skin object")
                }
                request["ops"] = json!([{"op":"set","path":"skin","value":skin}]);
            }
        }
    }
    if request.get("blueprint").is_some_and(|blueprint| !blueprint.is_object()) {
        bail!("blueprint must be a JSON object")
    }
    job.request = (!request.is_null()).then_some(request);
    job.preview = preview_request(s, preview);
    job.feedback = s.feedback.clone();
    enqueue(job).map(Output::Json)
}

fn check_ops(value: &Value) -> Result<()> {
    let ops = value.as_array().ok_or_else(|| anyhow!("ops must be an array"))?;
    if ops.is_empty() || ops.len() > 100 {
        bail!("ops must contain 1..100 operations")
    }
    for (i, op) in ops.iter().enumerate() {
        let fields = op.as_object().ok_or_else(|| anyhow!("ops[{i}] must be an object"))?;
        let kind = fields.get("op").and_then(Value::as_str).ok_or_else(|| anyhow!("ops[{i}].op is required"))?;
        let allowed: &[&str] = match kind {
            "set" | "add" => &["op", "path", "value"],
            "remove" => &["op", "path"],
            "mirror" => &["op", "path", "side"],
            "scale" => &["op", "path", "by"],
            _ => bail!("ops[{i}].op must be set, add, remove, mirror, or scale"),
        };
        for key in fields.keys() {
            if !allowed.contains(&key.as_str()) {
                bail!("ops[{i}]: unused field '{key}'")
            }
        }
        let path = fields.get("path").and_then(Value::as_str).ok_or_else(|| anyhow!("ops[{i}].path must be text"))?;
        if path
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|key| matches!(key, "__proto__" | "constructor" | "prototype"))
        {
            bail!("ops[{i}].path names a reserved key");
        }
        if matches!(kind, "set" | "add") && !fields.contains_key("value") {
            bail!("ops[{i}].value is required")
        }
        if kind == "scale" && fields.get("by").and_then(Value::as_f64).is_none_or(|n| !n.is_finite() || n <= 0.0) {
            bail!("ops[{i}].by must be positive")
        }
        if kind == "mirror"
            && fields.get("side").is_some_and(|side| !matches!(side.as_str(), Some("left" | "right" | "center" | "both")))
        {
            bail!("ops[{i}].side must be left, right, center, or both")
        }
    }
    Ok(())
}

fn inspect(root: &Path, name: &str) -> Result<Value> {
    let source = read_source(root, name)?;
    let current = revision(source.as_ref());
    let history = read_history(root, name, &current)?;
    if source.is_none() && history.undo.is_empty() && history.redo.is_empty() {
        bail!("no creature {name}; use creature_edit action=create")
    }
    let asset = creatures::get(name);
    let info = source_info(root, name, source.as_ref(), &history, asset.as_ref());
    authored().lock().unwrap().insert(name.into(), info.clone());
    Ok(info)
}

fn source_info(root: &Path, name: &str, source: Option<&Source>, history: &History, asset: Option<&Arc<CreatureAsset>>) -> Value {
    let current = revision(source);
    let matching = history.head == current;
    json!({
        "name":name,"kind":"creature","revision":current,"source":source,"blueprint":source.as_ref().map(|source|&source.blueprint),
        "file":source_path(root,name),"undo":if matching {history.undo.len()} else {0},"redo":if matching {history.redo.len()} else {0},
        "compiled":asset.is_some(),"asset_revision":asset.as_ref().map(|asset|asset.revision()),
        "source_revision":asset.as_ref().map(|asset|&asset.source_revision),
        "clips":asset.as_ref().map(|asset|asset.clips.keys().collect::<Vec<_>>()),
        "bounds":asset.as_ref().map(|asset|asset.bounds),
    })
}

pub fn t_creature_status(s: &mut Session, a: &Args) -> Result<Output> {
    fields(a, &["job", "name"])?;
    let job = a.get("job").map(|id| id.as_u64().ok_or_else(|| anyhow!("job must be a non-negative integer"))).transpose()?;
    let name = text(a, "name")?.map(canonical).transpose()?;
    if let Some(id) = job {
        let mut info = SERVICE
            .get()
            .and_then(|service| {
                service
                    .book
                    .lock()
                    .unwrap()
                    .jobs
                    .get(&id)
                    .filter(|record| record.owner == s.creature_owner)
                    .map(|record| record.info.clone())
            })
            .ok_or_else(|| anyhow!("no creature job {id} in this session"))?;
        if let (Some(feedback), Some(ticket)) = (&s.feedback, info["feedback"]["ticket"].as_u64()) {
            if let Ok(feedback) = feedback.lock() {
                info["feedback"] = feedback.status(Some(ticket));
            }
        }
        return Ok(Output::Json(info));
    }
    let mut status = snapshot(s.creature_owner);
    if let Some(name) = name {
        if let Some(jobs) = status["jobs"].as_array_mut() {
            jobs.retain(|job| job["name"] == name);
        }
    }
    status["preview"] = json!(s.sim.creature_preview_info());
    Ok(Output::Json(status))
}

/// A one-shot CLI must not exit while its newly queued job still owns unsaved work.
pub fn finish_one_shot(s: &mut Session, output: Output) -> Result<Output> {
    let id = match &output {
        Output::Json(value) => value.get("job").and_then(Value::as_u64),
        _ => None,
    };
    let Some(id) = id else { return Ok(output) };
    if s.is_live() {
        return Ok(output);
    }
    let deadline = Instant::now() + Duration::from_secs(125);
    loop {
        poll(s);
        let Output::Json(status) = t_creature_status(s, &json!({"job":id}).as_object().unwrap().clone())? else { unreachable!() };
        match status["state"].as_str() {
            Some("published") => return Ok(Output::Json(status)),
            Some("failed") => bail!("creature job {id} failed: {}", status["error"]),
            Some("superseded") => return Ok(Output::Json(status)),
            _ => {}
        }
        if Instant::now() >= deadline {
            bail!("creature job {id} did not finish within 125 seconds")
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Generator discovery is static, so listing shapes and parameters never launches Node.
pub fn t_creature_catalog(_: &mut Session, a: &Args) -> Result<Output> {
    fields(a, &["kind", "module", "find", "schema"])?;
    let schema = get_bool(a, "schema", false)?;
    let mut catalog: Value = if schema {
        serde_json::from_str(include_str!("../../../tools/creature-compiler/blueprint.schema.json"))?
    } else {
        catalog_snapshot()
    };
    if schema {
        return Ok(Output::Json(catalog));
    }
    let module = text(a, "module")?;
    let kind = text(a, "kind")?;
    let find = text(a, "find")?.map(str::to_lowercase);
    if let Some(modules) = catalog.get_mut("modules").and_then(Value::as_array_mut) {
        modules.retain(|entry| {
            module.is_none_or(|id| entry["id"] == id)
                && kind.is_none_or(|kind| entry["kind"] == kind)
                && find.as_ref().is_none_or(|find| entry.to_string().to_lowercase().contains(find))
        });
        if let Some(id) = module {
            if modules.is_empty() {
                bail!("no generator module {id}; use creature_catalog")
            }
        }
    }
    catalog["worker"] = json!(creature_worker::entry_path());
    catalog["authoring"] = json!(if cfg!(target_arch = "wasm32") { "native_required" } else { "local_node" });
    Ok(Output::Json(catalog))
}

pub fn t_creatures(_: &mut Session, a: &Args) -> Result<Output> {
    fields(a, &["find", "limit"])?;
    let find = text(a, "find")?.unwrap_or("").to_lowercase();
    let limit = a
        .get("limit")
        .map(|value| value.as_u64().ok_or_else(|| anyhow!("limit must be an integer")))
        .transpose()?
        .unwrap_or(100)
        .min(500) as usize;
    let library = creatures::library();
    let mut assets=library.assets.iter().filter(|(name,_)|name.to_lowercase().contains(&find)).take(limit).map(|(name,asset)|json!({
        "name":name,"kind":"creature","revision":asset.source_revision,"asset_revision":asset.revision(),
        "title":asset.title,"quality":asset.quality,"bounds":asset.bounds,"clips":asset.clips.keys().collect::<Vec<_>>(),"compiled":true,
    })).collect::<Vec<_>>();
    let authored = authored().lock().unwrap();
    let missing = authored
        .iter()
        .filter(|(name, info)| info["revision"] != "absent" && !library.assets.contains_key(*name))
        .collect::<Vec<_>>();
    for (name, info) in &missing {
        if assets.len() >= limit {
            break;
        }
        if !name.to_lowercase().contains(&find) {
            continue;
        }
        assets.push(json!({"name":name,"kind":"creature","revision":info["revision"],"compiled":false,"quality":info["source"]["quality"],"rebuild_tool":"creature_edit action=rebuild"}));
    }
    Ok(Output::Json(
        json!({"kind":"creature","assets":assets,"total":library.assets.len()+missing.len(),"catalog_tool":"creature_catalog"}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Folder(PathBuf);
    impl Folder {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pav-creature-jobs-{}-{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn reply(request: &Value) -> Value {
        json!({"id":request["id"],"ok":true,"result":{
            "blueprint":request["blueprint"],"warnings":[],"build_kind":"test",
            "asset":{
                "format":1,"generator_revision":creatures::GENERATOR_REVISION,"name":request["name"],"source_revision":"pending","quality":"low",
                "bones":{"names":["root"],"parents":[-1],"positions":[0,0,0],"rotations":[0,0,0,1],"lengths":[1]},
                "meshes":{"skin":{"positions":[0,0,0,1,0,0,0,1,0],"normals":[0,0,1,0,0,1,0,0,1],"indices":[0,1,2],
                    "colors":[1,0,0,1,0,0,1,0,0],"skin_indices":[0,0,0,0,0,0,0,0,0,0,0,0],"skin_weights":[1,0,0,0,1,0,0,0,1,0,0,0]}},
                "bounds":{"min":[0,0,0],"max":[1,1,0]},
                "clips":{"bend":{"duration":1,"looping":true,"frames":2,"positions":[0,0,0,0,0,0],"rotations":[0,0,0,1,0,0,1,0]}}
            }
        }})
    }

    fn prepared(root: &Path, name: &str, action: &str, blueprint: Value, state: &Mutex<Book>) -> Job {
        let name = canonical(name).unwrap();
        let before = read_source(root, &name).unwrap();
        let history = read_history(root, &name, &revision(before.as_ref())).unwrap();
        let mut job = prepare(root.into(), name, action, 99, before, history).unwrap();
        job.quality = "low".into();
        job.request = Some(json!({"op":"build","name":leaf(&job.name),"quality":"low","blueprint":blueprint}));
        let mut book = state.lock().unwrap();
        book.next += 1;
        job.id = book.next;
        book.latest.insert(job.key.clone(), job.id);
        book.jobs
            .insert(job.id, Record { owner: job.owner, phase: Phase::Building, info: json!({"job":job.id,"state":"building"}) });
        job
    }

    #[test]
    fn failed_superseded_and_conflicting_builds_never_replace_the_saved_source() {
        let folder = Folder::new();
        let state = Mutex::new(Book::default());
        let first = prepared(&folder.0, "job-save-guard", "create", json!({"size":1}), &state);
        let built = build_with(&first, |request| Ok(reply(request)), &state).unwrap();
        let saved = std::fs::read(source_path(&folder.0, &first.name)).unwrap();
        let history = std::fs::read(history_path(&folder.0, &first.name)).unwrap();
        assert_eq!(built.authored["undo"], 1);
        assert_eq!(built.authored["source"]["blueprint"]["size"], 1);
        let bad = prepared(&folder.0, &first.name, "patch", json!({"size":2}), &state);
        assert!(
            build_with(
                &bad,
                |_| Ok(json!({"ok":false,"error":"bad named path","issues":[{"path":"parts[id=missing]"}]})),
                &state
            )
            .is_err()
        );
        assert_eq!(std::fs::read(source_path(&folder.0, &first.name)).unwrap(), saved);
        assert_eq!(std::fs::read(history_path(&folder.0, &first.name)).unwrap(), history);
        let stale = prepared(&folder.0, &first.name, "patch", json!({"size":3}), &state);
        assert!(
            build_with(
                &stale,
                |request| {
                    state.lock().unwrap().latest.insert(stale.key.clone(), stale.id + 1);
                    Ok(reply(request))
                },
                &state
            )
            .unwrap_err()
            .to_string()
            .contains("superseded")
        );
        assert_eq!(std::fs::read(source_path(&folder.0, &first.name)).unwrap(), saved);
        let conflict = prepared(&folder.0, &first.name, "patch", json!({"size":4}), &state);
        let mut external = read_source(&folder.0, &first.name).unwrap().unwrap();
        external.blueprint = json!({"size":99});
        assert!(
            build_with(
                &conflict,
                |request| {
                    PreparedFile::new(&source_path(&folder.0, &first.name), &serde_json::to_vec(&external).unwrap())
                        .unwrap()
                        .publish()
                        .unwrap();
                    Ok(reply(request))
                },
                &state
            )
            .unwrap_err()
            .to_string()
            .contains("revision conflict")
        );
        assert_eq!(read_source(&folder.0, &first.name).unwrap().unwrap().revision(), external.revision());
        assert_eq!(std::fs::read(history_path(&folder.0, &first.name)).unwrap(), history);
    }

    #[test]
    fn publication_is_owned_and_feedback_follows_the_adopted_paused_frame() {
        let folder = Folder::new();
        let (work, _receiver) = channel();
        let service = Service { book: Arc::new(Mutex::new(Book::default())), work };
        let mut owner = Session::new("empty", 7).unwrap();
        let mut other = Session::new("empty", 8).unwrap();
        let feedback = crate::live_feedback::LiveFeedback::shared();
        owner.feedback = Some(feedback.clone());
        let mut first = prepared(&folder.0, "job-publication", "create", json!({"size":1}), &service.book);
        first.owner = owner.creature_owner;
        first.preview = preview_request(&owner, true);
        let built = build_with(&first, |request| Ok(reply(request)), &service.book).unwrap();
        let first_revision = built.report["revision"].clone();
        {
            let mut book = service.book.lock().unwrap();
            book.preview.insert(first.owner, first.id);
            book.ready.push_back(Completion { job: first, built: Some(built) });
        }
        assert!(poll_ready(&mut other, &service).is_empty());
        assert!(creatures::get("job-publication").is_none());
        assert_eq!(feedback.lock().unwrap().status(None)["state"], "idle");
        let notes = poll_ready(&mut owner, &service);
        assert_eq!(notes[0]["state"], "published");
        assert_eq!(owner.sim.frame().creature_preview.unwrap().source_revision, first_revision.as_str().unwrap());
        assert_eq!(feedback.lock().unwrap().status(None)["state"], "pending");
        let ticket = owner.sim.live_edit_ticket;
        feedback.lock().unwrap().submitted(ticket);
        assert_eq!(feedback.lock().unwrap().status(Some(ticket))["state"], "submitted");
        crate::creature_preview_tools::t_creature_preview(&mut owner, json!({"time":0.5,"playing":false}).as_object().unwrap())
            .unwrap();
        owner.camera.params.yaw = 117.0;
        let mut next = prepared(&folder.0, "job-publication", "patch", json!({"size":2}), &service.book);
        next.owner = owner.creature_owner;
        next.preview = preview_request(&owner, true);
        let built = build_with(&next, |request| Ok(reply(request)), &service.book).unwrap();
        {
            let mut book = service.book.lock().unwrap();
            book.preview.insert(next.owner, next.id);
            book.ready.push_back(Completion { job: next, built: Some(built) });
        }
        assert_eq!(poll_ready(&mut owner, &service)[0]["state"], "published");
        let info = owner.sim.creature_preview_info().unwrap();
        assert_eq!(info.time, 0.5);
        assert!(!info.playing);
        assert_eq!(owner.camera.params.yaw, 117.0);
        assert_ne!(info.source_revision, first_revision.as_str().unwrap());
        assert_eq!(authored_snapshot("job-publication").unwrap()["undo"], 2);
        assert!(owner.gpu.is_none());
        creatures::remove("job-publication").unwrap();
    }

    #[test]
    fn reload_lock_contention_is_retryable_and_bad_paths_are_rejected_before_work() {
        let folder = Folder::new();
        let path = source_path(&folder.0, "job-lock");
        let lock = lock_editor(&folder.0, false).unwrap();
        let busy = enqueue_reload(&path).unwrap();
        assert_eq!(busy["state"], "busy");
        assert_eq!(busy["retryable"], true);
        drop(lock);
        assert!(enqueue_reload(&path).unwrap_err().to_string().contains("removed"));
        assert!(check_ops(&json!([{"op":"set","path":"parts[id=horn].params.length","value":0.4}])).is_ok());
        assert!(check_ops(&json!([{"op":"set","path":"__proto__.polluted","value":true}])).is_err());
        assert!(check_ops(&json!([{"op":"remove","path":"skin","value":0}])).is_err());
    }
}
