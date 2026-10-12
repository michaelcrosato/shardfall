// Embeds every room file in /rooms into the binary (rooms.rs in OUT_DIR), so adding a room
// file is enough for it to ship; likewise game data, animation data and the embedded assets.
use std::io::Write;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rooms");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect())
        .unwrap_or_default();
    entries.sort();
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("rooms.rs");
    let mut f = std::fs::File::create(out).unwrap();
    writeln!(f, "pub const EMBEDDED: &[(&str, &str)] = &[").unwrap();
    for p in entries {
        println!("cargo:rerun-if-changed={}", p.display());
        let stem = p.file_stem().unwrap().to_string_lossy().to_string();
        let abs = p.canonicalize().unwrap();
        writeln!(f, "    ({stem:?}, include_str!({:?})),", abs.display().to_string()).unwrap();
    }
    writeln!(f, "];").unwrap();

    // Game data (Shardfall): every .toml under /game, keyed by its path relative to /game
    // without the extension ("skills", "levels/01_shrines").
    let game = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../game");
    println!("cargo:rerun-if-changed={}", game.display());
    let mut files = Vec::new();
    collect(&game, &mut files);
    files.sort();
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("game_data.rs");
    let mut f = std::fs::File::create(out).unwrap();
    writeln!(f, "pub const GAME_DATA: &[(&str, &str)] = &[").unwrap();
    for p in files {
        println!("cargo:rerun-if-changed={}", p.display());
        let rel = p.strip_prefix(&game).unwrap().with_extension("");
        let key = rel.to_string_lossy().replace('\\', "/");
        let abs = p.canonicalize().unwrap();
        writeln!(f, "    ({key:?}, include_str!({:?})),", abs.display().to_string()).unwrap();
    }
    writeln!(f, "];").unwrap();

    // Animation data: the files directly in /anim (moves.toml and the clip sets, *.json), keyed by
    // file name. Sub-folders (the full motion-capture libraries) stay on disk and load on demand.
    let anim = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../anim");
    println!("cargo:rerun-if-changed={}", anim.display());
    let mut files: Vec<_> = std::fs::read_dir(&anim)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "toml" || x == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("anim_data.rs");
    let mut f = std::fs::File::create(out).unwrap();
    writeln!(f, "pub const ANIM_DATA: &[(&str, &str)] = &[").unwrap();
    for p in files {
        println!("cargo:rerun-if-changed={}", p.display());
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let abs = p.canonicalize().unwrap();
        writeln!(f, "    ({name:?}, include_str!({:?})),", abs.display().to_string()).unwrap();
    }
    writeln!(f, "];").unwrap();

    // Core materials: assets/materials/*.json and pb/*.json, keyed by their name ("oak_planks",
    // "pb/gold"), which is `MAT/<name>`. The other tiers (workshop, fit, local) stay on disk.
    let materials = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/materials");
    let mut files = Vec::new();
    for sub in ["", "pb"] {
        let dir = materials.join(sub);
        println!("cargo:rerun-if-changed={}", dir.display());
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for p in rd.filter_map(|e| e.ok()).map(|e| e.path()) {
            if p.is_file() && p.extension().is_some_and(|x| x == "json") {
                let stem = p.file_stem().unwrap().to_string_lossy().to_string();
                files.push((if sub.is_empty() { stem } else { format!("{sub}/{stem}") }, p));
            }
        }
    }
    files.sort();
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("materials.rs");
    let mut f = std::fs::File::create(out).unwrap();
    writeln!(f, "pub const MATERIALS: &[(&str, &str)] = &[").unwrap();
    for (name, p) in files {
        println!("cargo:rerun-if-changed={}", p.display());
        let abs = p.canonicalize().unwrap();
        writeln!(f, "    ({name:?}, include_str!({:?})),", abs.display().to_string()).unwrap();
    }
    writeln!(f, "];").unwrap();

    // Object assets, the embedded tier (docs/ASSET_SYSTEM_PLAN.md §9.1): the templates and the
    // core library (assets, meshes, index.tsv), keyed by their path under assets/props. Other
    // packs stay on disk, and local ones on this machine.
    let props = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/props");
    let mut files = Vec::new();
    for folder in ["templates", "lib/core"] {
        let dir = props.join(folder);
        if dir.is_dir() {
            println!("cargo:rerun-if-changed={}", dir.display());
            collect_ext(&dir, &["json", "tsv"], &mut files);
        }
    }
    files.sort();
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("asset_data.rs");
    let mut f = std::fs::File::create(out).unwrap();
    writeln!(f, "pub const ASSET_DATA: &[(&str, &str)] = &[").unwrap();
    for p in files {
        let key = p.strip_prefix(&props).unwrap().to_string_lossy().replace('\\', "/");
        let abs = p.canonicalize().unwrap();
        writeln!(f, "    ({key:?}, include_str!({:?})),", abs.display().to_string()).unwrap();
    }
    writeln!(f, "];").unwrap();
}

/// Files under `dir` with one of `extensions`, skipping hidden files and folders.
fn collect_ext(dir: &std::path::Path, extensions: &[&str], out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            continue;
        }
        if p.is_dir() {
            collect_ext(&p, extensions, out);
        } else if p.extension().is_some_and(|x| extensions.iter().any(|e| x == *e)) {
            out.push(p);
        }
    }
}

fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.is_dir() {
            println!("cargo:rerun-if-changed={}", p.display());
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "toml") {
            out.push(p);
        }
    }
}
