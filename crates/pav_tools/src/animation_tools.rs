//! Persistent animation authoring. Valid edits replace one readable set and enter the live
//! library at once. The per-clip history is local editor data, outside the saved clip format.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use pav_core::animation_edit as edit;
use pav_core::clips::{self, Clip, ClipSet};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::session::Session;
use crate::tools::{Args, Output, get_bool, get_f32, get_str};

const WORKSHOP: &str = "WORKSHOP";
const LOCAL: &str = "WORKSHOP_LOCAL";
const HISTORY_LIMIT: usize = 32;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Default, Serialize, Deserialize)]
struct History {
    head: String,
    undo: Vec<Option<Clip>>,
    redo: Vec<Option<Clip>>,
}

fn root_dir() -> PathBuf {
    std::env::var("PAV_ANIM").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("anim"))
}

fn set_path(root: &Path, set: &str) -> PathBuf {
    if set == LOCAL { root.join("local/workshop_local.json") } else { root.join("workshop.json") }
}

fn history_path(root: &Path, set: &str, name: &str) -> PathBuf {
    // Clip names never become path components. Local source clips and their history stay in
    // the ignored local folder even if someone copies the other editor data elsewhere.
    let dir = if set == LOCAL { root.join("local/.editor") } else { root.join(".editor") };
    dir.join(format!("{:08x}.json", clips::clip_id(set, name)))
}

fn empty_set(name: &str) -> ClipSet {
    let mut set = ClipSet {
        set: name.into(),
        title: if name == LOCAL { "Local animation workshop".into() } else { "Animation workshop".into() },
        format: 1,
        credit: "Animation workshop. Each source entry retains its source credit and license.".into(),
        fps: 30.0,
        ..Default::default()
    };
    set.sources.insert(
        "AUTHORED".into(),
        json!({
            "label": "Original animation",
            "origin": "Created from the neutral pose in the Shardfall animation editor.",
            "license": "Author-defined",
        }),
    );
    set
}

fn read_set(root: &Path, name: &str) -> Result<ClipSet> {
    let path = set_path(root, name);
    if !path.exists() {
        return Ok(clips::library()
            .sets
            .iter()
            .find(|s| s.set == name)
            .map(|s| (**s).clone())
            .unwrap_or_else(|| empty_set(name)));
    }
    let text = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let set = ClipSet::parse(&text).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    if set.set != name {
        bail!("{} must contain set {name}, but it contains {}", path.display(), set.set);
    }
    for (clip, value) in &set.clips {
        edit::validate(value).map_err(|e| anyhow!("{name}/{clip}: {e}"))?;
    }
    Ok(set)
}

/// Load saved workshop sets after a process restart, without discarding imported libraries.
pub fn reload_authored() -> Result<()> {
    load_authored_at(&root_dir())
}

fn load_authored_at(root: &Path) -> Result<()> {
    for name in [WORKSHOP, LOCAL] {
        if set_path(root, name).is_file() {
            clips::replace_set(read_set(root, name)?).map_err(|e| anyhow!(e))?;
        }
    }
    Ok(())
}

fn target(name: &str, local: bool) -> Result<(String, String)> {
    let (set, name) = match name.split_once('/') {
        Some((set, name)) if set.eq_ignore_ascii_case(WORKSHOP) => (WORKSHOP, name),
        Some((set, name)) if set.eq_ignore_ascii_case(LOCAL) => (LOCAL, name),
        Some(_) => bail!("edits need a WORKSHOP clip; use action=copy from=SET/Clip name=NewName first"),
        None => (if local { LOCAL } else { WORKSHOP }, name),
    };
    let name = name.trim();
    if name.is_empty() || name.len() > 128 || name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\')) {
        bail!("name needs 1 to 128 bytes, with no slash, backslash, or control character");
    }
    if matches!(name, "." | "..") {
        bail!("name cannot be '.' or '..'");
    }
    Ok((set.into(), name.into()))
}

fn source(name: &str) -> Result<(ClipSet, Clip, String)> {
    let lib = clips::library();
    let id = lib.find(name).ok_or_else(|| anyhow!("no clip '{name}'; clips find=WORDS searches the library"))?;
    let full = lib.name_of(id).ok_or_else(|| anyhow!("clip '{name}' has no library name"))?;
    let clip = lib.get(id).ok_or_else(|| anyhow!("no clip '{name}'"))?.clone();
    let set_name = full.split_once('/').map(|(s, _)| s).unwrap_or_default();
    let set = lib.sets.iter().find(|s| s.set == set_name).ok_or_else(|| anyhow!("no source set for '{name}'"))?;
    Ok(((**set).clone(), clip, full))
}

fn local_source(root: &Path, set: &ClipSet, clip: &Clip) -> Result<bool> {
    if set.set == LOCAL {
        return Ok(true);
    }
    let text = format!("{} {} {}", set.set, set.credit, set.sources.get(&clip.src).unwrap_or(&Value::Null)).to_lowercase();
    if [
        "mixamo",
        "bandai",
        "lafan",
        "non-commercial",
        "noncommercial",
        "cc-by-nc",
        "cc by-nc",
        "research only",
        "no redistribution",
        "not redistribute",
    ]
    .iter()
    .any(|word| text.contains(word))
    {
        return Ok(true);
    }
    let local = root.join("local");
    if !local.exists() {
        return Ok(false);
    }
    for entry in std::fs::read_dir(&local).with_context(|| format!("read {}", local.display()))? {
        let path = entry?.path();
        if !path.is_file() || path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let local_set = ClipSet::parse(&text).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        if local_set.set == set.set {
            return Ok(true);
        }
    }
    Ok(false)
}

fn preserve_source(target: &mut ClipSet, source: &ClipSet, clip: &mut Clip, full: &str) {
    // Different clips from the same library have distinct provenance. The revision also
    // keeps a later copy of a changed source from rewriting an earlier copy's source entry.
    let key = format!("{full}@{}", edit::revision(clip));
    let original = source.sources.get(&clip.src).cloned().unwrap_or(Value::Null);
    let mut meta = if original.is_object() { original } else { json!({"original": original}) };
    if meta.get("source_set").is_none() {
        meta["source_set"] = json!(source.set);
    }
    if meta.get("source_clip").is_none() {
        meta["source_clip"] = json!(full);
    }
    meta["copied_from"] = json!(full);
    // Keep an earlier source's credit when copying a clip that was itself a workshop copy.
    if meta.get("credit").is_none() {
        meta["credit"] = json!(source.credit);
    }
    target.sources.insert(key.clone(), meta);
    clip.src = key;
}

fn clip_revision(clip: Option<&Clip>) -> String {
    clip.map(edit::revision).unwrap_or_else(|| "absent".into())
}

fn read_history(path: &Path, current: &str) -> Result<History> {
    if !path.exists() {
        return Ok(History { head: current.into(), ..Default::default() });
    }
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("read animation history {}", path.display()))
}

fn push_history(stack: &mut Vec<Option<Clip>>, clip: Option<Clip>) {
    stack.push(clip);
    if stack.len() > HISTORY_LIMIT {
        stack.remove(0);
    }
}

fn json_arg(a: &Args, name: &str) -> Result<Value> {
    let value = a.get(name).ok_or_else(|| anyhow!("{name}= needs a JSON object"))?;
    match value {
        Value::String(text) => serde_json::from_str(text).with_context(|| format!("invalid JSON in {name}")),
        value => Ok(value.clone()),
    }
}

fn check_args(a: &Args) -> Result<()> {
    for (name, value) in a {
        if ![
            "action",
            "name",
            "from",
            "duration",
            "loop",
            "time",
            "pose",
            "clip",
            "factor",
            "if_revision",
            "preview",
            "scene",
            "seed",
            "ticks",
        ]
        .contains(&name.as_str())
        {
            bail!("unknown anim_edit argument '{name}'");
        }
        if ["action", "name", "from", "if_revision"].contains(&name.as_str()) && !value.is_string() {
            bail!("{name} must be text");
        }
    }
    Ok(())
}

fn replace_clip(current: &Clip, value: Value) -> Result<Clip> {
    let object = value.as_object().ok_or_else(|| anyhow!("clip must be a JSON object"))?;
    for key in object.keys() {
        if !["clip", "src", "orig", "take", "dur", "loop", "speed", "tags", "desc", "keys"].contains(&key.as_str()) {
            bail!("unknown clip field '{key}'");
        }
    }
    if let Some(keys) = object.get("keys").and_then(Value::as_array) {
        for (index, key) in keys.iter().enumerate() {
            let key = key.as_object().ok_or_else(|| anyhow!("key {index} must be a JSON object"))?;
            for channel in key.keys() {
                if ![
                    "t", "hips", "body", "chest", "head", "shL", "shR", "armL", "armR", "legL", "legR", "footL", "footR",
                    "blade", "root",
                ]
                .contains(&channel.as_str())
                {
                    bail!("key {index}: unknown channel '{channel}'");
                }
            }
        }
    }
    let mut next: Clip = serde_json::from_value(value).context("invalid clip")?;
    next.clip = current.clip.clone();
    next.src = current.src.clone();
    next.orig = current.orig.clone();
    next.take = current.take.clone();
    edit::checked(next).map_err(|e| anyhow!(e))
}

/// Write a complete sibling before replacing a file. An interrupted write cannot leave half
/// a clip set for the next frame to read.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("{} has no parent", path.display()))?;
    std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().ok_or_else(|| anyhow!("{} has no file name", path.display()))?.to_string_lossy();
    let temp = parent.join(format!(".{name}.{}.{id}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file =
            OpenOptions::new().write(true).create_new(true).open(&temp).with_context(|| format!("create {}", temp.display()))?;
        file.write_all(bytes).with_context(|| format!("write {}", temp.display()))?;
        file.sync_all().with_context(|| format!("flush {}", temp.display()))?;
        drop(file);
        std::fs::rename(&temp, path).with_context(|| format!("replace {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn lock_editor(root: &Path) -> Result<File> {
    let dir = root.join(".editor");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join("lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    file.lock().context("lock the animation editor")?;
    Ok(file)
}

fn report(root: &Path, set: &ClipSet, name: &str, history: &History, saved: bool, changed: bool) -> Value {
    let clip = set.clips.get(name);
    let revision = clip_revision(clip);
    let current_history = history.head == revision;
    let editable = matches!(set.set.as_str(), WORKSHOP | LOCAL);
    json!({
        "name": format!("{}/{name}", set.set),
        "file": editable.then(|| set_path(root, &set.set)),
        "editable": editable,
        "local_only": set.set == LOCAL,
        "saved": saved,
        "changed": changed,
        "revision": revision,
        "undo": if current_history { history.undo.len() } else { 0 },
        "redo": if current_history { history.redo.len() } else { 0 },
        "credit": set.credit,
        "source": clip.and_then(|c| set.sources.get(&c.src)),
        "legend": clips::LEGEND,
        "clip": clip,
        "text": clip.map(Clip::text),
    })
}

fn inspect(root: &Path, name: &str) -> Result<Value> {
    load_authored_at(root)?;
    let resolved = if name.contains('/') {
        source(name)
    } else {
        source(&format!("{WORKSHOP}/{name}")).or_else(|_| source(&format!("{LOCAL}/{name}"))).or_else(|_| source(name))
    };
    if let Ok((set, clip, _)) = resolved {
        let history = if matches!(set.set.as_str(), WORKSHOP | LOCAL) {
            read_history(&history_path(root, &set.set, &clip.clip), &edit::revision(&clip))?
        } else {
            History::default()
        };
        return Ok(report(root, &set, &clip.clip, &history, false, false));
    }
    // Undo of a new clip removes it. Its absent revision and redo remain inspectable.
    if let Ok((set, clip)) = target(name, false) {
        let path = history_path(root, &set, &clip);
        if path.is_file() {
            let set = read_set(root, &set)?;
            let history = read_history(&path, "absent")?;
            return Ok(report(root, &set, &clip, &history, false, false));
        }
    }
    bail!("no clip '{name}'; clips find=WORDS searches the library")
}

/// The disk and library operation, kept separate from preview control for headless checks.
fn edit_at(root: &Path, a: &Args) -> Result<Value> {
    check_args(a)?;
    let action = get_str(a, "action").unwrap_or("inspect");
    let requested = get_str(a, "name").ok_or_else(|| anyhow!("name= names the clip"))?;
    if action == "inspect" {
        return inspect(root, requested);
    }
    if !["create", "copy", "key", "delete_key", "replace", "retime", "mirror", "undo", "redo"].contains(&action) {
        bail!("unknown action '{action}'; use create, copy, inspect, key, delete_key, replace, retime, mirror, undo, or redo");
    }
    let _lock = lock_editor(root)?;
    load_authored_at(root)?;
    let copied = if action == "copy" {
        Some(source(get_str(a, "from").ok_or_else(|| anyhow!("from= names the source clip"))?)?)
    } else {
        None
    };
    let local = copied.as_ref().map(|(set, clip, _)| local_source(root, set, clip)).transpose()?.unwrap_or(false);
    let (set_name, mut name) = target(requested, local)?;
    if local && set_name != LOCAL {
        bail!("this source stays in anim/local; use a bare name or WORKSHOP_LOCAL/Name");
    }
    let mut set = read_set(root, &set_name)?;
    if let Some(existing) = set.clips.keys().find(|n| n.eq_ignore_ascii_case(&name)) {
        name = existing.clone();
    }
    let before = set.clips.get(&name).cloned();
    let revision = clip_revision(before.as_ref());
    if let Some(expected) = get_str(a, "if_revision") {
        if expected != revision {
            bail!(
                "revision conflict for {set_name}/{name}: expected {expected}, current {revision}; inspect the clip and apply the edit again"
            );
        }
    }
    if matches!(action, "create" | "copy") && before.is_some() {
        bail!("{set_name}/{name} already exists; choose a new name or edit the existing clip");
    }
    let history_file = history_path(root, &set_name, &name);
    let old_history_bytes = if history_file.is_file() { Some(std::fs::read(&history_file)?) } else { None };
    let mut history = read_history(&history_file, &revision)?;
    let history_reset = history.head != revision;
    if history_reset {
        if matches!(action, "undo" | "redo") {
            bail!("the clip changed outside this editor; its old history does not match revision {revision}");
        }
        history = History { head: revision.clone(), ..Default::default() };
    }
    let next = match action {
        "create" => Some(edit::create(&name, get_f32(a, "duration", 2.0)?, get_bool(a, "loop", true)?).map_err(|e| anyhow!(e))?),
        "copy" => {
            let (source_set, mut clip, full) = copied.ok_or_else(|| anyhow!("no source clip"))?;
            preserve_source(&mut set, &source_set, &mut clip, &full);
            Some(edit::copy_clip(&clip, &name).map_err(|e| anyhow!(e))?)
        }
        "undo" => {
            let next = history.undo.pop().ok_or_else(|| anyhow!("no edit to undo for {set_name}/{name}"))?;
            push_history(&mut history.redo, before.clone());
            next
        }
        "redo" => {
            let next = history.redo.pop().ok_or_else(|| anyhow!("no edit to redo for {set_name}/{name}"))?;
            push_history(&mut history.undo, before.clone());
            next
        }
        action => {
            let current =
                before.as_ref().ok_or_else(|| anyhow!("no clip {set_name}/{name}; use action=create or action=copy first"))?;
            Some(match action {
                "key" => edit::apply_key(current, required_time(a)?, &json_arg(a, "pose")?).map_err(|e| anyhow!(e))?,
                "delete_key" => edit::delete_key(current, required_time(a)?).map_err(|e| anyhow!(e))?,
                "replace" => replace_clip(current, json_arg(a, "clip")?)?,
                "retime" => {
                    if !a.contains_key("factor") {
                        bail!("factor= scales all key times and duration; 0.5 is twice as fast");
                    }
                    edit::retime(current, get_f32(a, "factor", 1.0)?).map_err(|e| anyhow!(e))?
                }
                "mirror" => edit::mirrored(current).map_err(|e| anyhow!(e))?,
                _ => unreachable!("the action was checked above"),
            })
        }
    };
    if let Some(clip) = &next {
        edit::validate(clip).map_err(|e| anyhow!(e))?;
    }
    let next_revision = clip_revision(next.as_ref());
    if next_revision == revision {
        return Ok(report(root, &set, &name, &history, false, false));
    }
    if !matches!(action, "undo" | "redo") {
        push_history(&mut history.undo, before);
        history.redo.clear();
    }
    history.head = next_revision;
    match next {
        Some(clip) => {
            set.clips.insert(name.clone(), clip);
        }
        None => {
            set.clips.remove(&name);
        }
    }
    // An edited pose no longer has the importer's measured fitting error.
    set.fit.remove(&name);
    let text = set.to_text();
    let set = ClipSet::parse(&text).map_err(|e| anyhow!("the saved set does not read back: {e}"))?;
    clips::library().replacing_set(set.clone()).map_err(|e| anyhow!(e))?;

    // A standalone build may only have embedded moves. This marker lets the normal anim
    // loader find the new disk folder on the next launch.
    if !root.join("moves.toml").exists() {
        let moves = pav_core::anim::source("moves.toml").ok_or_else(|| anyhow!("embedded moves.toml is missing"))?;
        atomic_write(&root.join("moves.toml"), moves.as_bytes())?;
    }
    atomic_write(&history_file, &serde_json::to_vec(&history)?)?;
    let path = set_path(root, &set_name);
    if let Err(error) = atomic_write(&path, text.as_bytes()) {
        // The animation file and live library still hold the previous version. Restore its
        // history as well when saving the new animation fails.
        let restored = match old_history_bytes {
            Some(bytes) => atomic_write(&history_file, &bytes),
            None => std::fs::remove_file(&history_file).map_err(anyhow::Error::from),
        };
        return match restored {
            Ok(()) => Err(error),
            Err(history_error) => {
                Err(error.context(format!("the animation is unchanged; history restore failed: {history_error}")))
            }
        };
    }
    clips::replace_set(set.clone()).map_err(|e| anyhow!(e))?;
    let mut result = report(root, &set, &name, &history, true, true);
    result["history_reset"] = json!(history_reset);
    Ok(result)
}

fn required_time(a: &Args) -> Result<f32> {
    if !a.contains_key("time") {
        bail!("time= selects the key time in seconds");
    }
    get_f32(a, "time", 0.0)
}

/// `anim_edit`: create, copy, inspect, and edit animation clips. Writes are automatic; every
/// response returns the new revision for a later guarded edit.
pub fn t_anim_edit(s: &mut Session, a: &Args) -> Result<Output> {
    // Parse all options that could fail before the disk operation begins.
    let preview = get_bool(a, "preview", true)?;
    let mut result = edit_at(&root_dir(), a)?;
    if get_str(a, "action").unwrap_or("inspect") != "inspect" && preview {
        let mut args = Args::new();
        if result["clip"].is_null() {
            args.insert("action".into(), json!("close"));
        } else {
            args.insert("clip".into(), result["name"].clone());
        }
        match crate::preview_tools::t_anim_preview(s, &args) {
            Ok(Output::Json(preview)) => {
                result["preview"] = preview;
            }
            Ok(_) => {}
            Err(error) => {
                result["preview_error"] = json!(error.to_string());
            }
        }
    }
    Ok(Output::Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(value: Value) -> Args {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn file_edits_are_guarded_reversible_and_keep_source_credits() {
        let root = std::env::temp_dir().join(format!("pav-animation-editor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let name = "Editor_Test_Wave";
        let first = edit_at(&root, &args(json!({"action": "create", "name": name, "duration": 2, "loop": true}))).unwrap();
        assert_eq!(first["undo"], 1);
        let path = root.join("workshop.json");
        let saved = std::fs::read(&path).unwrap();
        let failed = edit_at(&root, &args(json!({"action": "key", "name": name, "time": 1, "pose": {"armRigth": [1, 0, 0]}})));
        assert!(failed.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), saved, "a rejected pose does not write the file");
        let changed = edit_at(&root, &args(json!({"action": "key", "name": name, "time": 1, "pose": {"armR": [100, 0, 0, 30, 0]}, "if_revision": first["revision"]}))).unwrap();
        assert_ne!(changed["revision"], first["revision"]);
        let stale = edit_at(&root, &args(json!({"action": "mirror", "name": name, "if_revision": first["revision"]})));
        assert!(stale.unwrap_err().to_string().contains("revision conflict"));
        assert_eq!(inspect(&root, name).unwrap()["revision"], changed["revision"]);
        let undone = edit_at(&root, &args(json!({"action": "undo", "name": name, "if_revision": changed["revision"]}))).unwrap();
        assert_eq!(undone["revision"], first["revision"]);
        let redone = edit_at(&root, &args(json!({"action": "redo", "name": name, "if_revision": undone["revision"]}))).unwrap();
        assert_eq!(redone["revision"], changed["revision"]);

        let copied =
            edit_at(&root, &args(json!({"action": "copy", "name": "Editor_Test_Idle", "from": "QUATERNIUS/Idle_Loop"}))).unwrap();
        assert!(copied["source"]["credit"].as_str().unwrap().contains("Quaternius"));
        assert_eq!(copied["source"]["license"], "CC0 1.0");
        edit_at(&root, &args(json!({"action": "copy", "name": "Editor_Test_Jump", "from": "QUATERNIUS/Jump_Start"}))).unwrap();
        assert_eq!(inspect(&root, "Editor_Test_Idle").unwrap()["source"]["source_clip"], "QUATERNIUS/Idle_Loop");
        assert_eq!(inspect(&root, "Editor_Test_Jump").unwrap()["source"]["source_clip"], "QUATERNIUS/Jump_Start");
        let tail =
            edit_at(&root, &args(json!({"action": "copy", "name": "Editor_Test_Bow", "from": "MESH2MOTION/Bow"}))).unwrap();
        let last = tail["clip"]["keys"].as_array().unwrap().last().unwrap();
        assert_eq!(last["t"], tail["clip"]["dur"]);
        assert!(clips::find("CMU/Cartwheel").is_some(), "unrelated library clips remain installed");
        let removed = edit_at(&root, &args(json!({"action": "undo", "name": "Editor_Test_Idle"}))).unwrap();
        assert_eq!(removed["revision"], "absent");
        assert!(removed["clip"].is_null());
        assert_eq!(inspect(&root, "Editor_Test_Idle").unwrap()["redo"], 1);
        let restored =
            edit_at(&root, &args(json!({"action": "redo", "name": "Editor_Test_Idle", "if_revision": "absent"}))).unwrap();
        assert_eq!(restored["revision"], copied["revision"]);

        // A new edit starts a new branch and keeps the most recent 32 revisions.
        for turn in 0..35 {
            edit_at(&root, &args(json!({"action": "key", "name": name, "time": 1, "pose": {"head": [turn, 0, 0]}}))).unwrap();
        }
        assert_eq!(inspect(&root, name).unwrap()["undo"], HISTORY_LIMIT);
        edit_at(&root, &args(json!({"action": "undo", "name": name}))).unwrap();
        edit_at(&root, &args(json!({"action": "key", "name": name, "time": 1, "pose": {"head": [-10, 0, 0]}}))).unwrap();
        assert_eq!(inspect(&root, name).unwrap()["redo"], 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_restrictions_and_names_cannot_change_the_destination() {
        assert!(target("../../other", false).is_err());
        assert!(target("WORKSHOP/../other", false).is_err());
        assert!(target("QUATERNIUS/Idle_Loop", false).is_err());
        assert_eq!(target("Local copy", true).unwrap().0, LOCAL);
        let root = std::env::temp_dir().join("pav-editor-no-local-folder");
        let mut set = empty_set("Example");
        set.sources.insert("X".into(), json!({"license": "CC BY-NC 4.0"}));
        let clip = Clip { src: "X".into(), ..edit::create("Example", 1.0, false).unwrap() };
        assert!(local_source(&root, &set, &clip).unwrap());
        assert!(check_args(&args(json!({"name": "A", "if_revision": 42}))).is_err());
        assert!(check_args(&args(json!({"name": "A", "if_revison": "absent"}))).is_err());
        let mut session = Session::new("empty", 1).unwrap();
        let invalid_preview =
            t_anim_edit(&mut session, &args(json!({"action": "create", "name": "Do_Not_Create", "preview": "perhaps"})));
        assert!(invalid_preview.err().unwrap().to_string().contains("preview"));
    }
}
