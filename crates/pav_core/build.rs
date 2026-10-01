// Embeds every room file in /rooms into the binary (rooms.rs in OUT_DIR), so adding a room
// file is enough for it to ship.
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
}
