//! Persistent authoring for procedural props. One validated batch is one file replacement,
//! history step and library publication. Preview control is separate from that transaction.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Once};

use anyhow::{Context, Result, anyhow, bail};
use pav_core::props::{self, PropAsset};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::session::Session;
use crate::tools::{Args, Output, get_bool, get_str};

const HISTORY_LIMIT: usize = 32;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);
static INITIALIZED: Once = Once::new();

#[derive(Default, Serialize, Deserialize)]
struct History {
    head: String,
    undo: Vec<Option<PropAsset>>,
    redo: Vec<Option<PropAsset>>,
}

struct Edited {
    report: Value,
    asset: Option<Arc<PropAsset>>,
    mutation: bool,
    preview: bool,
}

/// Asset files use this root in the app, CLI, live bridge and file watcher.
pub fn root() -> PathBuf {
    std::env::var_os("PAV_ASSETS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("assets/props"))
}

fn asset_path(root: &Path, name: &str) -> PathBuf {
    root.join("workshop").join(format!("{}.json", name.split_once('/').expect("canonical name").1))
}

fn history_path(root: &Path, name: &str) -> PathBuf {
    root.join(".editor").join(format!("{}.json", name.split_once('/').expect("canonical name").1))
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
    file.lock().context("lock the asset editor")?;
    Ok(file)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("{} has no parent", path.display()))?;
    std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let name = path.file_name().ok_or_else(|| anyhow!("{} has no file name", path.display()))?.to_string_lossy();
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(".{name}.{}.{id}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp)?;
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

fn file_definition(path: &Path) -> Result<(String, Arc<PropAsset>)> {
    let leaf = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| anyhow!("invalid asset file name"))?;
    props::validate_name(leaf).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let asset = PropAsset::parse(&text).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    if asset.name != leaf {
        bail!("{} must contain name '{leaf}', but it contains '{}'", path.display(), asset.name);
    }
    Ok((format!("WORKSHOP/{leaf}"), Arc::new(asset)))
}

fn publish_if_changed(name: &str, asset: Arc<PropAsset>) -> Result<Option<(String, Arc<PropAsset>)>> {
    if props::get(name).is_some_and(|current| current.revision() == asset.revision()) {
        return Ok(None);
    }
    props::install(name, asset.clone()).map_err(|e| anyhow!(e))?;
    Ok(Some((name.into(), asset)))
}

/// Read one workshop JSON file. Invalid content keeps the last good library definition.
/// Ignored paths and definitions already in the library return None. The lock also prevents
/// a watcher from publishing an older read after an accepted tool edit.
pub fn reload_file(path: &Path) -> Result<Option<(String, Arc<PropAsset>)>> {
    let Some(folder) = path.parent() else { return Ok(None) };
    if folder.file_name().is_none_or(|name| name != "workshop")
        || path.extension().is_none_or(|ext| ext != "json")
        || path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.'))
    {
        return Ok(None);
    }
    let root = folder.parent().ok_or_else(|| anyhow!("{} has no asset root", path.display()))?;
    let _lock = lock_editor(root)?;
    if !path.try_exists().with_context(|| format!("check {}", path.display()))? {
        let leaf = path.file_stem().and_then(|name| name.to_str()).ok_or_else(|| anyhow!("invalid asset file name"))?;
        props::validate_name(leaf).map_err(|e| anyhow!(e))?;
        if props::get(&format!("WORKSHOP/{leaf}")).is_none() {
            // A tool undo already removed this definition. Its watcher event is redundant.
            return Ok(None);
        }
    }
    let (name, asset) = file_definition(path)?;
    publish_if_changed(&name, asset)
}

/// Initialize disk assets once per process, before the first scene is constructed. Later
/// temporary sessions must not consume a file change that the live watcher will apply.
pub fn initialize_authored() -> Result<()> {
    let mut result = Ok(());
    INITIALIZED.call_once(|| result = reload_authored().map(|_| ()));
    result
}

/// Load saved definitions after a restart. Each file is independent: valid files load, and
/// invalid files retain their prior definitions. The error lists files that need a fix.
pub fn reload_authored() -> Result<Vec<(String, Arc<PropAsset>)>> {
    reload_authored_at(&root())
}

fn reload_authored_at(root: &Path) -> Result<Vec<(String, Arc<PropAsset>)>> {
    let folder = root.join("workshop");
    if !folder.exists() {
        return Ok(Vec::new());
    }
    let _lock = lock_editor(root)?;
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(&folder).with_context(|| format!("read {}", folder.display()))? {
        let path = entry?.path();
        if path.is_file()
            && path.extension().is_some_and(|ext| ext == "json")
            && path.file_name().is_some_and(|name| !name.to_string_lossy().starts_with('.'))
        {
            paths.push(path);
        }
    }
    paths.sort();
    let mut changed = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        match file_definition(&path).and_then(|(name, asset)| publish_if_changed(&name, asset)) {
            Ok(Some(asset)) => changed.push(asset),
            Ok(None) => {}
            Err(error) => errors.push(format!("{error:#}")),
        }
    }
    if !errors.is_empty() {
        bail!("some asset files did not load; their last good definitions remain: {}", errors.join("; "));
    }
    Ok(changed)
}

fn read_asset(root: &Path, name: &str) -> Result<Option<Arc<PropAsset>>> {
    if name.starts_with("BUILTIN/") {
        return Ok(props::get(name));
    }
    let path = asset_path(root, name);
    if !path.exists() {
        return Ok(None);
    }
    file_definition(&path).map(|(_, asset)| Some(asset))
}

fn revision(asset: Option<&PropAsset>) -> String {
    asset.map(PropAsset::revision).unwrap_or_else(|| "absent".into())
}

fn read_history(path: &Path, current: &str) -> Result<History> {
    if !path.exists() {
        return Ok(History { head: current.into(), ..Default::default() });
    }
    let history: History =
        serde_json::from_slice(&std::fs::read(path)?).with_context(|| format!("read asset history {}", path.display()))?;
    if history.undo.len() > HISTORY_LIMIT || history.redo.len() > HISTORY_LIMIT {
        bail!("asset history {} exceeds {HISTORY_LIMIT} steps", path.display());
    }
    Ok(history)
}

fn push_history(stack: &mut Vec<Option<PropAsset>>, asset: Option<PropAsset>) {
    stack.push(asset);
    if stack.len() > HISTORY_LIMIT {
        stack.remove(0);
    }
}

fn report(root: &Path, name: &str, asset: Option<&PropAsset>, history: &History, saved: bool) -> Value {
    let revision = revision(asset);
    let current = history.head == revision;
    let editable = name.starts_with("WORKSHOP/");
    json!({
        "name": name,
        "file": editable.then(|| asset_path(root, name)),
        "editable": editable,
        "saved": saved,
        "changed": saved,
        "revision": revision,
        "undo": if current { history.undo.len() } else { 0 },
        "redo": if current { history.redo.len() } else { 0 },
        "part_count": asset.map(|asset| asset.parts.len()).unwrap_or(0),
        "bounds": asset.map(|asset| asset.bounds(1.0)),
    })
}

fn inspect(root: &Path, name: &str, preview: bool) -> Result<Edited> {
    let asset = read_asset(root, name)?;
    let history_file = history_path(root, name);
    if asset.is_none() && (!name.starts_with("WORKSHOP/") || !history_file.is_file()) {
        bail!("no asset '{name}'; assets find=WORDS searches the library");
    }
    let history = if name.starts_with("WORKSHOP/") {
        read_history(&history_file, &revision(asset.as_deref()))?
    } else {
        History::default()
    };
    let mut result = report(root, name, asset.as_deref(), &history, false);
    result["asset"] = json!(asset.as_deref());
    result["parts"] = asset
        .as_ref()
        .map(|asset| {
            let bounds = asset.parts.iter().map(|(name, part)| (name.clone(), json!(part.bounds(1.0)))).collect::<Args>();
            Value::Object(bounds)
        })
        .unwrap_or(Value::Null);
    result["legend"] = json!(props::LEGEND);
    Ok(Edited { report: result, asset, mutation: false, preview })
}

fn json_arg(a: &Args, key: &str) -> Result<Value> {
    match a.get(key).ok_or_else(|| anyhow!("{key}= is required"))? {
        Value::String(text) => serde_json::from_str(text).with_context(|| format!("invalid JSON in {key}")),
        value => Ok(value.clone()),
    }
}

fn check_args(a: &Args) -> Result<(&str, bool)> {
    let preview = get_bool(a, "preview", true)?;
    if a.get("action").is_some_and(|value| !value.is_string()) {
        bail!("action must be text");
    }
    let action = get_str(a, "action").unwrap_or("inspect");
    let extra: &[&str] = match action {
        "create" => &["template", "asset", "description", "if_revision"],
        "copy" => &["from", "description", "if_revision"],
        "inspect" => &[],
        "patch" => &["ops", "if_revision"],
        "replace" => &["asset", "if_revision"],
        "undo" | "redo" => &["if_revision"],
        _ => bail!("unknown action '{action}'; use create, copy, inspect, patch, replace, undo, or redo"),
    };
    for (key, value) in a {
        if !["action", "name", "preview", "scene", "seed", "ticks"].contains(&key.as_str()) && !extra.contains(&key.as_str()) {
            bail!("unknown or unused asset_edit argument '{key}' for action={action}");
        }
        if ["action", "name", "from", "template", "description", "if_revision"].contains(&key.as_str()) && !value.is_string() {
            bail!("{key} must be text");
        }
    }
    if a.contains_key("template") && a.contains_key("asset") {
        bail!("create accepts template or asset, not both");
    }
    Ok((action, preview))
}

fn definition(value: Value, name: &str, creating: bool) -> Result<PropAsset> {
    let mut object = value.as_object().ok_or_else(|| anyhow!("asset must be a JSON object"))?.clone();
    let leaf = name.split_once('/').expect("canonical name").1;
    if creating {
        object.entry("format").or_insert(json!(1));
        object.insert("name".into(), json!(leaf));
    }
    let asset: PropAsset = serde_json::from_value(Value::Object(object)).context("invalid asset definition")?;
    if asset.name != leaf {
        bail!("asset.name must stay '{leaf}'; use action=copy to make a new asset");
    }
    asset.validate().map_err(|e| anyhow!(e))?;
    Ok(asset)
}

fn changed_parts(before: Option<&PropAsset>, after: Option<&PropAsset>) -> Vec<String> {
    let names = before
        .into_iter()
        .flat_map(|asset| asset.parts.keys())
        .chain(after.into_iter().flat_map(|asset| asset.parts.keys()))
        .collect::<BTreeSet<_>>();
    names
        .into_iter()
        .filter(|name| before.and_then(|asset| asset.parts.get(*name)) != after.and_then(|asset| asset.parts.get(*name)))
        .cloned()
        .collect()
}

/// Disk editing stays independent of a GPU or active preview.
fn edit_at(root: &Path, a: &Args) -> Result<Edited> {
    let (action, preview) = check_args(a)?;
    let name = props::canonical(get_str(a, "name").ok_or_else(|| anyhow!("name= names the asset"))?).map_err(|e| anyhow!(e))?;
    if action == "inspect" {
        return inspect(root, &name, preview);
    }
    if !name.starts_with("WORKSHOP/") {
        bail!("built-in templates are read-only; use action=copy from={name} name=your_name first");
    }
    let _lock = lock_editor(root)?;
    let before = read_asset(root, &name)?;
    let current_revision = revision(before.as_deref());
    if let Some(expected) = get_str(a, "if_revision") {
        if expected != current_revision {
            bail!(
                "revision conflict for {name}: expected {expected}, current {current_revision}; inspect the asset and apply the edit again"
            );
        }
    }
    if matches!(action, "create" | "copy") && before.is_some() {
        bail!("{name} already exists; choose a new name or edit the existing asset");
    }
    let history_file = history_path(root, &name);
    let old_history = if history_file.is_file() { Some(std::fs::read(&history_file)?) } else { None };
    let mut history = read_history(&history_file, &current_revision)?;
    let history_reset = history.head != current_revision;
    if history_reset {
        if matches!(action, "undo" | "redo") {
            bail!("the asset changed outside this editor; its old history does not match revision {current_revision}");
        }
        history = History { head: current_revision.clone(), ..Default::default() };
    }
    let before_owned = before.as_deref().cloned();
    let next = match action {
        "create" | "copy" => {
            let mut asset = if action == "create" && a.contains_key("asset") {
                definition(json_arg(a, "asset")?, &name, true)?
            } else {
                let source = if action == "copy" {
                    props::canonical(get_str(a, "from").ok_or_else(|| anyhow!("from= names the source asset"))?)
                        .map_err(|e| anyhow!(e))?
                } else {
                    let template = get_str(a, "template").unwrap_or("box");
                    let template = if template.contains('/') { template.into() } else { format!("BUILTIN/{template}") };
                    let template = props::canonical(&template).map_err(|e| anyhow!(e))?;
                    if !template.starts_with("BUILTIN/") {
                        bail!("template names a BUILTIN asset; use action=copy for workshop assets");
                    }
                    template
                };
                let source = read_asset(root, &source)?.ok_or_else(|| anyhow!("no source asset '{source}'"))?;
                (*source).clone()
            };
            asset.name = name.split_once('/').expect("canonical name").1.into();
            if let Some(description) = get_str(a, "description") {
                asset.description = description.into();
            }
            Some(asset)
        }
        "undo" => {
            let next = history.undo.pop().ok_or_else(|| anyhow!("no edit to undo for {name}"))?;
            push_history(&mut history.redo, before_owned.clone());
            next
        }
        "redo" => {
            let next = history.redo.pop().ok_or_else(|| anyhow!("no edit to redo for {name}"))?;
            push_history(&mut history.undo, before_owned.clone());
            next
        }
        action => {
            let current = before.as_ref().ok_or_else(|| anyhow!("no asset {name}; use action=create or action=copy first"))?;
            Some(match action {
                "patch" => current.patch(&json_arg(a, "ops")?).map_err(|e| anyhow!(e))?,
                "replace" => definition(json_arg(a, "asset")?, &name, false)?,
                _ => unreachable!("action was checked"),
            })
        }
    };
    if let Some(asset) = &next {
        asset.validate().map_err(|e| anyhow!(e))?;
        if name.split_once('/').map(|(_, leaf)| leaf) != Some(asset.name.as_str()) {
            bail!("saved history has a different asset name; cannot restore it as {name}");
        }
    }
    let next_revision = revision(next.as_ref());
    let changed_parts = changed_parts(before.as_deref(), next.as_ref());
    if next_revision == current_revision {
        let mut result = report(root, &name, before.as_deref(), &history, false);
        result["changed_parts"] = json!(changed_parts);
        return Ok(Edited { report: result, asset: before, mutation: true, preview });
    }
    if !matches!(action, "undo" | "redo") {
        push_history(&mut history.undo, before_owned);
        history.redo.clear();
    }
    history.head = next_revision;
    atomic_write(&history_file, &serde_json::to_vec(&history)?)?;
    let path = asset_path(root, &name);
    let save = match &next {
        Some(asset) => atomic_write(&path, asset.text().as_bytes()),
        None => std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display())),
    };
    if let Err(error) = save {
        let restored = match old_history {
            Some(bytes) => atomic_write(&history_file, &bytes),
            None => std::fs::remove_file(&history_file).map_err(anyhow::Error::from),
        };
        return match restored {
            Ok(()) => Err(error),
            Err(history_error) => Err(error.context(format!("the asset is unchanged; history restore failed: {history_error}"))),
        };
    }
    let asset = next.map(Arc::new);
    match &asset {
        Some(asset) => props::install(&name, asset.clone()).map_err(|e| anyhow!(e))?,
        None => {
            props::remove(&name).map_err(|e| anyhow!(e))?;
        }
    }
    let mut result = report(root, &name, asset.as_deref(), &history, true);
    result["changed_parts"] = json!(changed_parts);
    result["history_reset"] = json!(history_reset);
    Ok(Edited { report: result, asset, mutation: true, preview })
}

/// Search the registry without returning complete geometry for every result.
pub fn t_assets(_: &mut Session, a: &Args) -> Result<Output> {
    for key in a.keys() {
        if !["find", "limit", "scene", "seed", "ticks"].contains(&key.as_str()) {
            bail!("unknown assets argument '{key}'");
        }
    }
    if a.get("find").is_some_and(|value| !value.is_string()) {
        bail!("find must be text");
    }
    let limit = match a.get("limit") {
        None => 30,
        Some(value) => value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            .filter(|value| (1..=1000).contains(value))
            .ok_or_else(|| anyhow!("limit must be an integer from 1 to 1000"))?,
    } as usize;
    let root = root();
    let find = get_str(a, "find").unwrap_or("").to_ascii_lowercase();
    let library = props::library();
    let matched = library
        .assets
        .iter()
        .filter(|(name, asset)| {
            name.to_ascii_lowercase().contains(&find) || asset.description.to_ascii_lowercase().contains(&find)
        })
        .collect::<Vec<_>>();
    let result = json!({
        "total": library.assets.len(),
        "matched": matched.len(),
        "returned": matched.len().min(limit),
        "assets": matched.into_iter().take(limit).map(|(name, asset)| json!({
            "name": name,
            "description": asset.description,
            "editable": name.starts_with("WORKSHOP/"),
            "file": name.starts_with("WORKSHOP/").then(|| asset_path(&root, name)),
            "revision": asset.revision(),
            "part_count": asset.parts.len(),
            "bounds": asset.bounds(1.0),
        })).collect::<Vec<_>>(),
    });
    Ok(Output::Json(result))
}

/// Create, inspect and alter an asset. Options are parsed before disk mutation; a later
/// preview failure is reported separately from an accepted, saved definition.
pub fn t_asset_edit(s: &mut Session, a: &Args) -> Result<Output> {
    let Edited { mut report, asset, mutation, preview } = edit_at(&root(), a)?;
    if mutation {
        let name = report["name"].as_str().expect("report has canonical name").to_owned();
        let saved = report["saved"] == true;
        report["refreshed_instances"] = json!(if saved {
            asset.as_ref().map(|asset| s.sim.refresh_prop_instances(&name, asset.clone())).unwrap_or(0)
        } else {
            0
        });
        let mut args = Args::new();
        if asset.is_some() && preview {
            args.insert("name".into(), json!(name));
        } else if asset.is_none() && saved {
            // Undo of creation leaves placed snapshot copies intact and closes only this
            // missing asset's active stage. A different preview remains selected.
            if let Ok(Output::Json(status)) = crate::asset_preview_tools::t_asset_preview(s, &Args::new()) {
                if status["open"] == true && status["name"] == name {
                    args.insert("close".into(), json!(true));
                }
            }
        }
        if !args.is_empty() {
            match crate::asset_preview_tools::t_asset_preview(s, &args) {
                Ok(Output::Json(status)) => report["preview"] = status,
                Ok(_) => {}
                Err(error) => report["preview_error"] = json!(format!("{error:#}")),
            }
        }
    }
    Ok(Output::Json(report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(value: Value) -> Args {
        value.as_object().unwrap().clone()
    }

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("pav-asset-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn edit(root: &Path, value: Value) -> Value {
        edit_at(root, &args(value)).unwrap().report
    }

    #[test]
    fn batches_are_atomic_guarded_and_reversible_across_restart() {
        let root = temp_root("transactions");
        let name = "test_asset_transactions";
        let canonical = format!("WORKSHOP/{name}");
        let first = edit(&root, json!({"action":"create","name":name,"template":"bench"}));
        let path = asset_path(&root, &canonical);
        let original_bytes = std::fs::read(&path).unwrap();
        let history_bytes = std::fs::read(history_path(&root, &canonical)).unwrap();
        let original = props::get(&canonical).unwrap();
        let failed = edit_at(
            &root,
            &args(json!({"action":"patch","name":name,"ops":[
                {"op":"set","part":"seat","fields":{"color":"#ffffff"}},
                {"op":"remove","part":"missing"}
            ]})),
        );
        assert!(failed.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
        assert_eq!(std::fs::read(history_path(&root, &canonical)).unwrap(), history_bytes);
        assert!(Arc::ptr_eq(&original, &props::get(&canonical).unwrap()));
        let changed = edit(
            &root,
            json!({"action":"patch","name":name,"if_revision":first["revision"],"ops":[
                {"op":"set","part":"seat","fields":{"color":"#4060a0"}},
                {"op":"set","part":"backrest","fields":{"pitch":-20}},
                {"op":"add","part":"brace","value":{"shape":{"type":"box","half":[0.6,0.05,0.05]}}}
            ]}),
        );
        assert_eq!(changed["undo"], 2, "one batch is one history entry");
        assert_eq!(changed["changed_parts"], json!(["backrest", "brace", "seat"]));
        assert!(changed.get("asset").is_none(), "edit replies omit complete definitions");
        let stale = edit_at(&root, &args(json!({"action":"patch","name":name,"if_revision":first["revision"],"ops":[]})));
        assert!(stale.err().unwrap().to_string().contains("revision conflict"));
        props::remove(&canonical).unwrap();
        assert!(props::get(&canonical).is_none());
        let loaded = reload_authored_at(&root).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(props::get(&canonical).unwrap().revision(), changed["revision"]);
        assert!(reload_file(&path).unwrap().is_none(), "a watcher does not publish the tool edit twice");
        let undone = edit(&root, json!({"action":"undo","name":name,"if_revision":changed["revision"]}));
        assert_eq!(undone["revision"], first["revision"]);
        let removed = edit(&root, json!({"action":"undo","name":name,"if_revision":undone["revision"]}));
        assert_eq!(removed["revision"], "absent");
        assert!(!path.exists() && props::get(&canonical).is_none());
        assert!(reload_file(&path).unwrap().is_none(), "a tool removal does not become a watcher error");
        let absent = edit(&root, json!({"action":"inspect","name":name}));
        assert!(absent["asset"].is_null());
        assert_eq!(absent["redo"], 2);
        let restored = edit(&root, json!({"action":"redo","name":name,"if_revision":"absent"}));
        assert_eq!(restored["revision"], first["revision"]);
        let final_state = edit(&root, json!({"action":"redo","name":name,"if_revision":restored["revision"]}));
        assert_eq!(final_state["revision"], changed["revision"]);
        props::remove(&canonical).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn external_edits_keep_last_good_data_and_reset_only_matching_history() {
        let root = temp_root("external");
        let name = "test_asset_external";
        let canonical = format!("WORKSHOP/{name}");
        let created = edit(&root, json!({"action":"copy","name":name,"from":"BUILTIN/lantern"}));
        let builtin = props::get("BUILTIN/lantern").unwrap();
        let path = asset_path(&root, &canonical);
        std::fs::write(&path, "{ unfinished").unwrap();
        assert!(reload_file(&path).is_err());
        assert_eq!(props::get(&canonical).unwrap().revision(), created["revision"]);
        let mut external = (*props::get(&canonical).unwrap()).clone();
        external.parts.get_mut("post").unwrap().color = "#112233".into();
        std::fs::write(&path, external.text()).unwrap();
        assert!(reload_file(&path).unwrap().is_some());
        let undo = edit_at(&root, &args(json!({"action":"undo","name":name,"if_revision":external.revision()})));
        assert!(undo.err().unwrap().to_string().contains("outside this editor"));
        let next = edit(
            &root,
            json!({"action":"patch","name":name,"if_revision":external.revision(),"ops":[
                {"op":"set","part":"post","fields":{"color":"#446688"}}
            ]}),
        );
        assert_eq!(next["history_reset"], true);
        assert_eq!(next["undo"], 1);
        let undo = edit(&root, json!({"action":"undo","name":name}));
        assert_eq!(undo["revision"], external.revision());
        assert!(Arc::ptr_eq(&builtin, &props::get("BUILTIN/lantern").unwrap()));
        assert!(reload_file(&root.join(".editor/unrelated.json")).unwrap().is_none());
        assert!(reload_file(&root.join("workshop/unfinished.tmp")).unwrap().is_none());
        assert!(reload_file(&root.join("workshop/.hidden.json")).unwrap().is_none());
        props::remove(&canonical).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn history_is_bounded_and_bad_arguments_cannot_write() {
        let root = temp_root("history");
        let name = "test_asset_history";
        let canonical = format!("WORKSHOP/{name}");
        for bad in [
            json!({"action":"create","name":name,"preview":"perhaps"}),
            json!({"action":"create","name":name,"if_revision":42}),
            json!({"action":"create","name":name,"if_revison":"absent"}),
            json!({"action":"create","name":name,"template":"box","asset":{}}),
            json!({"action":"create","name":"WORKSHOP/../../outside"}),
            json!({"action":"patch","name":"BUILTIN/box","ops":[]}),
        ] {
            assert!(edit_at(&root, &args(bad)).is_err());
        }
        assert!(!root.join("workshop").exists());
        // The history write happens first. A failed asset write must restore that history
        // and leave the library unchanged, including for the first save into a new root.
        std::fs::write(root.join("workshop"), "this path is a file").unwrap();
        assert!(edit_at(&root, &args(json!({"action":"create","name":name}))).is_err());
        assert!(!history_path(&root, &canonical).exists());
        assert!(props::get(&canonical).is_none());
        std::fs::remove_file(root.join("workshop")).unwrap();
        edit(&root, json!({"action":"create","name":name,"scene":"empty","seed":1,"ticks":0}));
        for angle in 1..=35 {
            edit(&root, json!({"action":"patch","name":name,"ops":[{"op":"set","part":"body","fields":{"yaw":angle}}]}));
        }
        let current = edit(&root, json!({"action":"inspect","name":name}));
        assert_eq!(current["undo"], HISTORY_LIMIT);
        edit(&root, json!({"action":"undo","name":name}));
        let branch = edit(&root, json!({"action":"patch","name":name,"ops":[{"op":"set","part":"body","fields":{"yaw":-10}}]}));
        assert_eq!(branch["redo"], 0);
        let no_op = edit(&root, json!({"action":"patch","name":name,"ops":[]}));
        assert_eq!(no_op["revision"], branch["revision"]);
        assert_eq!(no_op["saved"], false);
        assert_eq!(no_op["undo"], branch["undo"]);
        props::remove(&canonical).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
