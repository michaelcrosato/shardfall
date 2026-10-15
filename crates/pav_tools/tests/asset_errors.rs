//! Errors are part of the agent loop: each one says where (`parts.<id>` or `ops[i]`), what is
//! valid and how to fix it. The fixture holds the 23 probes from the planning dogfood
//! session (docs/ASSET_SYSTEM_PLAN.md §7.1) with the exact replies they get now.

use pav_tools::Session;
use serde_json::{Value, json};

#[test]
fn every_dogfood_probe_gets_a_located_actionable_reply() {
    let root = std::env::temp_dir().join(format!("pav-asset-errors-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    // SAFETY: this test binary has one test, and it sets the variable before any tool runs.
    unsafe { std::env::set_var("PAV_ASSETS", &root) };
    let mut s = Session::new("empty", 1).unwrap();
    let well: Value = serde_json::from_str(include_str!("../../pav_core/tests/fixtures/props/dogfood_well_v1.json")).unwrap();
    let args = json!({"action":"create","name":"well","asset":well,"preview":false});
    pav_tools::call(&mut s, "asset_edit", args.as_object().unwrap()).unwrap();
    let mut located = 0;
    for (line, text) in include_str!("fixtures/asset_errors.jsonl").lines().enumerate() {
        let probe: Value = serde_json::from_str(text).unwrap();
        let reply = pav_tools::call(&mut s, probe["tool"].as_str().unwrap(), probe["args"].as_object().unwrap());
        match (&reply, probe["error"].as_str()) {
            (Err(error), Some(expected)) => {
                assert_eq!(format!("{error:#}"), expected, "probe {}", line + 1);
                located += usize::from(expected.starts_with("parts.") || expected.starts_with("ops["));
            }
            (Ok(_), None) => {}
            (Err(error), None) => panic!("probe {} should succeed ({}): {error:#}", line + 1, probe["ok"]),
            (Ok(_), Some(expected)) => panic!("probe {} should fail with {expected}", line + 1),
        }
    }
    assert_eq!(located, 19, "every part or op problem names its part or op");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unknown_actions_kinds_args_and_tools_name_the_nearest_valid_one() {
    let mut s = Session::new("empty", 1).unwrap();
    let error = |s: &mut Session, tool: &str, args: Value| {
        format!("{:#}", pav_tools::call(s, tool, args.as_object().unwrap()).err().expect("an error"))
    };
    assert_eq!(
        error(&mut s, "asset_edit", json!({"action":"inspekt","name":"BUILTIN/bench"})),
        r#"action: unknown action "inspekt" (expected create, copy, inspect, check, patch, replace, undo, redo, schema, publish) — did you mean "inspect"?"#
    );
    assert_eq!(
        error(&mut s, "asset_edit", json!({"action":"patch","name":"x","opps":[]})),
        r#"opps: not an asset_edit argument for action=patch (expected action, name, preview, ops, if_revision, dry_run) — did you mean "ops"?"#
    );
    assert_eq!(
        error(&mut s, "asset_edit", json!({"action":"copy","name":"x","from":"BUILTIN/bench","ops":[]})),
        "ops: not an asset_edit argument for action=copy (expected action, name, preview, from, description, if_revision, dry_run) — ops belongs to another action"
    );
    assert_eq!(
        error(&mut s, "assets", json!({"kind":"props"})),
        r#"kind: unknown kind "props" (expected prop, creature, material, clip) — did you mean "prop"?"#
    );
    assert_eq!(
        error(&mut s, "asset_preview", json!({"kind":"mesh"})),
        r#"kind: unknown kind "mesh" (expected prop, creature, material)"#
    );
    assert_eq!(
        error(&mut s, "asset_preview", json!({"action":"opn"})),
        r#"action: unknown action "opn" (expected status, open, fit, close, play, pause, restart) — did you mean "open"?"#
    );
    assert_eq!(
        error(&mut s, "asset_spawn", json!({"action":"list","nmae":"x"})),
        r#"nmae: unknown argument (expected action, name, id, pos, yaw, scale, collide, room, roles, params) — did you mean "name"?"#
    );
    assert_eq!(error(&mut s, "asset_edt", json!({})), r#"unknown tool "asset_edt" — did you mean "asset_edit"?"#);
}

#[test]
fn the_field_guide_and_tool_list_stay_compact() {
    let mut s = Session::new("empty", 1).unwrap();
    let mut guide = |args: Value| match pav_tools::call(&mut s, "asset_edit", args.as_object().unwrap()).unwrap() {
        pav_tools::Output::Json(value) => value,
        _ => panic!("schema returns data"),
    };
    // Format 2's guide is the default; format 1's stays for its files.
    let v2 = guide(json!({"action":"schema"}));
    assert_eq!(v2["format"], 2);
    for key in ["legend", "document", "node", "shapes", "materials", "ops", "limits"] {
        assert!(v2.get(key).is_some(), "{key}");
    }
    assert!(v2.to_string().len() < 7_200, "format 2's field guide is {} chars", v2.to_string().len());
    let v1 = guide(json!({"action":"schema","format":1}));
    for key in ["legend", "document", "part", "shapes", "ops", "limits"] {
        assert!(v1.get(key).is_some(), "{key}");
    }
    assert!(v1.to_string().len() < 2_600, "format 1's field guide is {} chars", v1.to_string().len());
    let list = pav_tools::mcp::tool_list();
    let chars = list.to_string().len();
    // G0's 47,000 plus `asset_import` (plan §8.2 budgets its schema at ≤ 2 KB; it is ~0.96 KB).
    // Row 4.1's retrieval args (~0.4 KB) were paid for by trimming three creature/job help strings,
    // row 4.2's `review` arg by trimming the clip library's `load=` help, kind=material (row
    // 3.3) by trimming help text, and `level rooms=` (row 3.4) by trimming `level`'s help. The
    // store's `shelf=` and `kind=clip` (S0) by trimming `assets`' help; LIB-4's `params=` by
    // trimming creature_status and creature_preview help.
    assert!(chars <= 48_000, "tools/list is {chars} chars");
    let tools = list["tools"].as_array().unwrap();
    let edit = tools.iter().find(|t| t["name"] == "asset_edit").unwrap();
    for item in edit["inputSchema"]["properties"]["ops"]["items"]["oneOf"].as_array().unwrap() {
        let creature = item["description"].as_str().is_some_and(|d| d.starts_with("kind=creature"));
        assert_eq!(item.to_string().contains("path"), creature, "no path op among prop ops: {item}");
    }
    for name in ["assets", "asset_edit", "asset_preview", "asset_spawn", "studio_status"] {
        let tool = tools.iter().find(|t| t["name"] == name).unwrap();
        let words = tool["description"].as_str().unwrap().split_whitespace().count();
        assert!(words <= 36, "{name} help has {words} words");
        for (arg, schema) in tool["inputSchema"]["properties"].as_object().unwrap() {
            let words = schema["description"].as_str().unwrap_or("").split_whitespace().count();
            assert!(words <= 12, "{name}.{arg} help has {words} words");
        }
    }
}
