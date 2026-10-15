//! The agent asset store's S0 checks (docs/AGENT_ASSET_STORE.md §8, §9, §13): common words find
//! something or say `none` with a route, every store row carries an admitted licence and none
//! comes from a local tier, the credits name every CC-BY source, `kind=clip` reaches the clips
//! ranker, and each shelf stays within its token budget.

use std::path::{Path, PathBuf};

use pav_core::props::index;
use pav_tools::{Output, Session, asset_library, asset_search};
use serde_json::{Value, json};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn call(s: &mut Session, tool: &str, args: Value) -> Value {
    match pav_tools::call(s, tool, args.as_object().unwrap()).unwrap() {
        Output::Json(value) => value,
        Output::Image { meta, .. } => meta,
    }
}

fn names(rows: &Value) -> Vec<String> {
    rows.as_array().unwrap().iter().map(|row| row[0].as_str().unwrap().to_owned()).collect()
}

#[test]
fn the_store_finds_shelves_credits_and_refuses() {
    let scratch = std::env::temp_dir().join(format!("pav-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    std::fs::write(scratch.join("misses.tsv"), "date\tkind\tquery\tnearest\n").unwrap();
    // SAFETY: this test binary has one test, and it sets the variables before any tool runs.
    unsafe {
        std::env::set_var("PAV_ASSETS", repo().join("assets/props"));
        std::env::set_var("PAV_ANIM", repo().join("anim"));
        std::env::set_var("PAV_STORE", &scratch);
    }
    let mut s = Session::new("empty", 1).unwrap();

    // Common words: a hit, or `none` with the nearest rows and a route.
    let wooden = call(&mut s, "assets", json!({"find": "wooden table"}));
    assert!(names(&wooden["rows"]).contains(&"LIB/core/table".to_owned()), "{wooden}");
    let tables = call(&mut s, "assets", json!({"find": "tables"}));
    assert_eq!(names(&tables["rows"])[0], "LIB/core/table", "{tables}");
    for word in ["tree", "character", "dragon statue"] {
        let reply = call(&mut s, "assets", json!({"find": word}));
        let hit = reply["rows"].as_array().is_some_and(|rows| !rows.is_empty());
        let none = reply["none"].is_string() && reply["route"].as_str().is_some_and(|route| !route.is_empty());
        assert!(hit || none, "{word}: {reply}");
        assert!(reply.get("matched") != Some(&json!(0)), "never a bare 0: {reply}");
    }
    let tree = call(&mut s, "assets", json!({"find": "tree"}));
    assert!(tree["route"].as_str().unwrap().starts_with("make it:"), "{tree}");
    assert!(names(&tree["nearest"]).iter().any(|name| name.contains("mushroom")), "the vegetation hint: {tree}");
    let character = call(&mut s, "assets", json!({"find": "character"}));
    assert!(character["route"].as_str().unwrap().contains("heroes.toml"), "{character}");
    let misses = std::fs::read_to_string(scratch.join("misses.tsv")).unwrap();
    assert!(misses.contains("\tprop\ttree\t") && misses.contains("\tprop\tcharacter\t"), "{misses}");
    // Rows carry their store tier and mesh share; the local tier is not searched by default.
    let barrel = call(&mut s, "assets", json!({"find": "barrel"}));
    assert_eq!(barrel["cols"], "name cat size_cm tier mesh tok tags desc");
    let first = &barrel["rows"][0];
    assert_eq!((first[0].as_str().unwrap(), first[3].as_str().unwrap()), ("LIB/core/barrel", "standard"), "{barrel}");
    let table = call(&mut s, "assets", json!({"find": "table"}));
    assert!(table["rows"].as_array().unwrap().iter().any(|row| row[3] == "checked"), "{table}");

    // kind=clip hands the words to the clips ranker; store rows only.
    let walk = call(&mut s, "assets", json!({"find": "heavy walk", "kind": "clip", "limit": 5}));
    assert!(walk["found"].as_u64().unwrap() >= 1, "{walk}");
    assert!(walk["clips"][0]["name"].as_str().unwrap().contains("Walk"), "{walk}");
    let taunt = call(&mut s, "assets", json!({"find": "taunt", "kind": "clip"}));
    assert!(taunt["found"].as_u64().unwrap() >= 1, "a synonym finds the insult gesture: {taunt}");
    let nothing = call(&mut s, "assets", json!({"find": "juggle flaming torches", "kind": "clip"}));
    assert!(nothing["none"].is_string() && nothing["route"].is_string(), "{nothing}");

    // Each shelf is within about 1,500 tokens, and the prop shelf holds only standard and
    // checked rows.
    for kind in ["prop", "clip"] {
        let shelf = call(&mut s, "assets", json!({"shelf": kind}));
        let tokens = index::tokens(shelf.to_string().len());
        assert!(tokens <= 1_500, "shelf={kind}: {tokens} tokens");
    }
    let props = call(&mut s, "assets", json!({"shelf": "prop"})).to_string();
    assert!(props.contains("LIB/core/chair") && props.contains("LIB/kenney_mini_dungeon/table"), "{props}");
    assert!(!props.contains("LIB/kenney_mini_dungeon/wall\""), "a 94% mesh import is not on the shelf: {props}");

    // Every row the store reads has an admitted licence and none comes from a local tier.
    let root = repo().join("assets/props");
    let mut rows = index::embedded().to_vec();
    let (disk, errors) = index::disk(&root);
    assert!(errors.is_empty(), "{errors:?}");
    rows.extend(disk.into_iter().filter(|row| row.tier != pav_core::props::Tier::Local));
    for row in &rows {
        assert!(pav_core::store::admitted(&row.src, "prop").is_ok(), "{}: {}", row.name, row.src);
        assert!(!pav_core::store::local_path(&row.name), "{}", row.name);
    }
    // A row with a licence the store does not admit is refused.
    let pack = scratch.join("props/lib/refusedpack");
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(
        pack.join("PACK.json"),
        r#"{"pack":"refusedpack","title":"T","license":"CC0-1.0","credit":"T","source":"own"}"#,
    )
    .unwrap();
    let asset = r##"{"format":2,"name":"post","unit":"cm","category":"architecture","description":"A post.","tags":["wood","post"],"source":{"license":"CC-BY-NC-4.0","library":"elsewhere"},"parts":{"post":{"type":"box","size":[10,100,10],"pos":[0,50,0],"color":"#806040"}}}"##;
    std::fs::write(pack.join("post.json"), asset).unwrap();
    let refused = asset_search::build_index(&scratch.join("props"), &pack, "LIB/refusedpack", &[]).unwrap_err().to_string();
    assert!(refused.contains("refused") && refused.contains("CC-BY-NC-4.0"), "{refused}");

    // The credits text is current and names every CC-BY source in the build.
    let sources = asset_library::credit_sources(&root).unwrap();
    let text = pav_core::store::credits_text(&sources);
    let committed = std::fs::read_to_string(repo().join("store/CREDITS.md")).unwrap();
    assert_eq!(text, committed, "store/CREDITS.md is stale: run `pav assets reindex=store`");
    let cc_by: Vec<_> = sources
        .iter()
        .filter(|source| pav_core::store::spdx(&source.licence).is_some_and(|id| id.starts_with("CC-BY")))
        .collect();
    assert!(cc_by.iter().any(|source| source.what == "STYLE100"), "STYLE100 is CC BY 4.0 and embedded");
    for source in cc_by {
        assert!(text.contains(source.credit.trim_end_matches('.')), "{} is not credited", source.what);
        // Sets on disk are listed as a range; every embedded one by name.
        assert!(source.where_ != "embedded" || text.contains(&format!("- {} (embedded)", source.what)), "{}", source.what);
    }
    std::fs::remove_dir_all(&scratch).unwrap();
}
