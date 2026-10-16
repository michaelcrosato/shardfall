//! The files every session loads stay short: `docs/PROGRESS.md` under 60 lines (one page of
//! current state; detail goes to HISTORY) and `AGENTS.md` under 32 KiB (Codex reads no further).

use std::path::Path;

fn read(path: &str) -> String {
    let full = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(path);
    std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{}: {e}", full.display()))
}

#[test]
fn session_files_stay_short() {
    let progress = read("docs/PROGRESS.md").lines().count();
    assert!(progress <= 60, "docs/PROGRESS.md has {progress} lines: keep it under 60, move detail to docs/HISTORY.md");
    let agents = read("AGENTS.md").len();
    assert!(agents < 32 * 1024, "AGENTS.md is {agents} bytes: keep it under 32 KiB, move detail to docs/REFERENCE.md");
}
