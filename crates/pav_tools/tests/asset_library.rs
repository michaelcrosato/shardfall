//! Committed asset folders (docs/ASSET_SYSTEM_PLAN.md §9.1-9.5): every index is current, the
//! packs kept on disk fit their budget, their licences may be shared and are credited, and every
//! core asset was reviewed at its current revision and passes the library gate.

use std::path::{Path, PathBuf};

use pav_core::props::index;
use pav_core::props::{self, Tier};
use pav_tools::asset_library;

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// Installs the mesh files of a pack folder (`meshes/`): imported assets draw their mesh parts
/// from them, and the tests run outside the repository root the engine reads them from.
fn install_meshes(pack: &str, folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder.join("meshes")) else { return };
    for path in entries.filter_map(Result::ok).map(|e| e.path()) {
        let leaf = path.file_stem().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&path).unwrap();
        let file = pav_core::props::mesh::MeshFile::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        pav_core::props::mesh::install(&format!("{pack}/{leaf}"), file).unwrap();
    }
}

/// The committed index folders: the templates and each pack under lib/ (local/ is not committed),
/// with the packs' mesh files installed.
fn indexed_folders() -> Vec<(PathBuf, String)> {
    let props = assets().join("props");
    let mut folders = vec![(props.join("templates"), "BUILTIN".to_owned())];
    folders.extend(index::disk_packs(&props).into_iter().filter(|(_, _, tier)| *tier == Tier::Disk).map(|(pack, folder, _)| {
        install_meshes(&pack, &folder);
        (folder, format!("LIB/{pack}"))
    }));
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
        let rows = pav_tools::asset_search::build_index(&root, &folder, &set, &old).unwrap();
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

/// Every committed pack has a `PACK.json` whose licence may be shared, with its credit lines, and
/// each asset in it is own work or names the pack's licence.
#[test]
fn committed_packs_are_redistributable() {
    let root = assets().join("props");
    for (pack, folder) in asset_library::committed_packs(&root) {
        let info = asset_library::read_pack(&folder).unwrap();
        if pack == "core" {
            assert_eq!(info.credit, pav_core::store::OWN_CREDIT, "lineage credits own work with core's line");
        }
        for (name, def) in pack_assets(&pack, &folder) {
            asset_library::source_allowed(def.doc.source.as_ref(), &info.license).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}

#[test]
fn credits_file_is_current() {
    let root = assets().join("props");
    let text = std::fs::read_to_string(root.join("CREDITS.md")).unwrap_or_default();
    assert_eq!(asset_library::credits(&root).unwrap(), text, "assets/props/CREDITS.md is stale: run `pav assets reindex=core`");
}

/// Each core asset passes `check` and the library gate, and `review.tsv` holds a review of its
/// current revision: an edit needs a new review (`asset_edit action=publish`).
#[test]
fn core_assets_are_reviewed() {
    let root = assets().join("props");
    let folder = root.join("lib/core");
    let reviews = asset_library::reviews(&folder).unwrap();
    let core = asset_library::read_pack(&folder).unwrap();
    for (name, def) in pack_assets("core", &folder) {
        let mut found = pav_tools::asset_tools::check_lines(&def);
        found.extend(asset_library::gate(&def, &core));
        assert!(found.is_empty(), "{name}: {found:?}");
        assert!(
            reviews.iter().any(|(row, revision, _)| *row == name && *revision == def.revision),
            "{name} has no review of revision {}: publish it again after a sheet review",
            def.revision
        );
    }
}

/// The assets of a pack folder, read from their files.
fn pack_assets(pack: &str, folder: &Path) -> Vec<(String, std::sync::Arc<props::PropDef>)> {
    install_meshes(pack, folder);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(folder)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json") && !path.ends_with("PACK.json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let name = format!("LIB/{pack}/{}", path.file_stem().unwrap().to_string_lossy());
            let text = std::fs::read_to_string(&path).unwrap();
            let doc = props::Doc::read(&text).unwrap_or_else(|e| panic!("{name}: {}", pav_core::issue::render(&e)));
            let def = props::PropDef::new(doc).unwrap_or_else(|e| panic!("{name}: {}", pav_core::issue::render(&e)));
            (name, std::sync::Arc::new(def))
        })
        .collect()
}
