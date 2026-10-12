//! The asset benchmark (scripts/asset_bench) measures how agents build assets. A library asset
//! with a task's name would let a run copy the answer, so no library leaf may equal a task name.

use std::collections::BTreeSet;
use std::path::Path;

fn leaves(dir: &Path, out: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "meshes") {
                leaves(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "json") {
            out.insert(path.file_stem().unwrap().to_string_lossy().into_owned());
        }
    }
}

#[test]
fn no_library_asset_answers_a_benchmark_task() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join("scripts/asset_bench/tasks.toml")).unwrap();
    let file: toml::Value = toml::from_str(&text).unwrap();
    let mut names = BTreeSet::new();
    for task in file["task"].as_array().unwrap() {
        for key in ["name", "names"] {
            match task.get(key) {
                Some(toml::Value::String(name)) => {
                    names.insert(name.clone());
                }
                Some(toml::Value::Array(list)) => names.extend(list.iter().map(|n| n.as_str().unwrap().to_owned())),
                _ => {}
            }
        }
    }
    assert!(names.len() >= 30, "the task names were read ({})", names.len());
    let mut library = BTreeSet::new();
    leaves(&root.join("assets/props/lib"), &mut library);
    let clashes: Vec<_> = names.intersection(&library).collect();
    assert!(clashes.is_empty(), "library assets named after benchmark tasks: {clashes:?}");
}
