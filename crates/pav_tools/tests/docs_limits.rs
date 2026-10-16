//! The files every session loads stay short: `docs/PROGRESS.md` under 60 lines (one page of
//! current state; detail goes to HISTORY) and `AGENTS.md` under 32 KiB (Codex reads no further).
//! `docs/MAKE_A_GAME.md`, read whole by every agent that makes a game, stays under 16 KiB and
//! names only paths that exist.

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

#[test]
fn the_game_guide_stays_short_and_true() {
    let guide = read("docs/MAKE_A_GAME.md");
    assert!(guide.len() < 16 * 1024, "docs/MAKE_A_GAME.md is {} bytes: keep it under 16 KiB", guide.len());
    // Inline `code` outside fenced blocks: every repository path there must exist.
    let mut prose = String::new();
    let mut fenced = false;
    for line in guide.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
        } else if !fenced {
            prose += line;
            prose.push('\n');
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let roots = ["crates/", "docs/", "rooms/", "scripts/", "assets/", "anim/"];
    for span in prose.split('`').skip(1).step_by(2) {
        let path = roots.iter().any(|r| span.starts_with(r)) && !span.contains(['<', '*', ' ']);
        assert!(!path || root.join(span).exists(), "docs/MAKE_A_GAME.md names `{span}`, which doesn't exist");
    }
}
