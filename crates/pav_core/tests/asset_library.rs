//! The embedded asset tier (docs/ASSET_SYSTEM_PLAN.md §9.1): its budget, that everything in it
//! reads and is indexed, and that game data and rooms name only what every build has.

use pav_core::props::index::{self, Row};
use pav_core::props::{self, Tier, store};

#[test]
fn embedded_assets_fit_budget() {
    let files = store::embedded_files();
    let total: usize = files.iter().map(|(_, text)| text.len()).sum();
    assert!(total <= 3_000_000, "the embedded assets are {total} bytes (budget 3,000,000; target 2,000,000)");
    for (path, text) in files {
        let cap = match () {
            _ if path.contains("/meshes/") => 64 * 1024,
            _ if path.ends_with(".json") => 16 * 1024,
            _ => continue,
        };
        assert!(text.len() <= cap, "{path} is {} bytes (budget {cap})", text.len());
    }
}

#[test]
fn embedded_library_is_valid() {
    let rows = index::embedded();
    let mut names: Vec<String> = store::embedded_assets().map(|(name, _)| name).collect();
    names.sort();
    let mut indexed: Vec<String> = rows.iter().map(|row| row.name.clone()).collect();
    indexed.sort();
    assert_eq!(indexed, names, "every embedded asset has one index row: assets reindex=templates (or =core) writes them");
    for row in rows {
        let def = props::resolve_ref(&row.name).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(row.tier, Tier::Embedded);
        assert!(store::embedded(&row.name));
        // The columns tools compute match the asset (the index is current).
        let fresh = Row::of(&row.name, &def, Tier::Embedded);
        assert_eq!(
            (&row.size_cm, row.parts, row.tok, &row.fid, &row.src, &row.desc),
            (&fresh.size_cm, fresh.parts, fresh.tok, &fresh.fid, &fresh.src, &fresh.desc),
            "{}: assets reindex= rewrites a stale index",
            row.name
        );
        if !row.cat.is_empty() {
            assert!(props::doc::Category::parse(&row.cat).is_some(), "{}: category {}", row.name, row.cat);
        }
    }
    // Search needs something to find: every template has a category and tags.
    assert!(rows.iter().filter(|row| row.name.starts_with("BUILTIN/")).all(|row| !row.cat.is_empty() && row.tags.len() >= 2));
}

/// Every `asset`, `model` or `figure` a room or game data file names is in every build and
/// resolves: the browser has nothing else.
#[test]
fn game_asset_refs_are_embedded() {
    fn walk(value: &toml::Value, at: &str, refs: &mut Vec<(String, String)>) {
        match value {
            toml::Value::Table(table) => {
                for (key, value) in table {
                    match value {
                        toml::Value::String(name) if key == "asset" || key == "model" || key == "figure" => {
                            refs.push((format!("{at}.{key}"), name.clone()))
                        }
                        value => walk(value, &format!("{at}.{key}"), refs),
                    }
                }
            }
            toml::Value::Array(items) => items.iter().for_each(|item| walk(item, at, refs)),
            _ => {}
        }
    }
    let mut refs = Vec::new();
    let rooms = pav_core::room::load_sources(None);
    let game = pav_core::arpg::data::GAME_DATA.iter().map(|(key, text)| (format!("game/{key}"), text.to_string()));
    for (file, text) in rooms.into_iter().map(|room| (format!("rooms/{}", room.key), room.text)).chain(game) {
        let value: toml::Value = toml::from_str(&text).unwrap_or_else(|e| panic!("{file}: {e}"));
        walk(&value, &file, &mut refs);
    }
    for (at, name) in &refs {
        // `NAME:preset` names a family's preset: NAME must be in every build.
        let canonical = props::canonical(props::split_preset(name).0).unwrap_or_else(|e| panic!("{at}: {e}"));
        assert!(
            store::shipped(&canonical),
            "{at}: {name} is not in every build (BUILTIN/, LIB/core/ or a shipped pack): the browser build lacks it"
        );
        if at.ends_with(".figure") {
            pav_core::figure::get(&canonical).unwrap_or_else(|e| panic!("{at}: {e}"));
        } else {
            props::resolve_placed(name, &props::Params::new()).unwrap_or_else(|e| panic!("{at}: {e}"));
        }
    }
}
