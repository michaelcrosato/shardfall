//! Committed asset folders (docs/ASSET_SYSTEM_PLAN.md §9.1-9.2): every index is current, and the
//! packs kept on disk fit their budget.

use std::path::{Path, PathBuf};

use pav_core::props::index;
use pav_core::props::{self, Tier};

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// The committed index folders: the templates and each pack under lib/ (local/ is not committed).
fn indexed_folders() -> Vec<(PathBuf, String)> {
    let props = assets().join("props");
    let mut folders = vec![(props.join("templates"), "BUILTIN".to_owned())];
    folders.extend(
        index::disk_packs(&props)
            .into_iter()
            .filter(|(_, _, tier)| *tier == Tier::Disk)
            .map(|(pack, folder, _)| (folder, format!("LIB/{pack}"))),
    );
    folders.push((props.join("lib").join("core"), "LIB/core".to_owned()));
    folders
}

#[test]
fn index_files_are_current() {
    let root = assets().join("props");
    for (folder, set) in indexed_folders() {
        let path = folder.join("index.tsv");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let old = index::parse(&text, Tier::Disk).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let rows = index::build(&folder, &set, &old, &|name| props::resolve_in(&root, name).ok().flatten()).unwrap();
        let pack = set.rsplit('/').next().unwrap();
        let fix = if set == "BUILTIN" { "templates" } else { pack };
        assert_eq!(index::text(&rows), text, "{} is stale: run `pav assets reindex={fix}`", path.display());
    }
}

/// Disk-tier budget: packs other than core, and fitted materials, at most 40 MB and 1 MB a file.
#[test]
fn committed_packs_fit_budget() {
    fn files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            if path.is_dir() { files(&path, out) } else { out.push(path) }
        }
    }
    let mut found = Vec::new();
    for (_, folder, tier) in index::disk_packs(&assets().join("props")) {
        if tier == Tier::Disk {
            files(&folder, &mut found);
        }
    }
    files(&assets().join("materials/fit"), &mut found);
    let mut total = 0;
    for path in &found {
        let bytes = std::fs::metadata(path).unwrap().len();
        assert!(bytes <= 1_000_000, "{} is {bytes} bytes (budget 1,000,000 a file)", path.display());
        total += bytes;
    }
    assert!(total <= 40_000_000, "the packs on disk are {total} bytes (budget 40,000,000)");
}
