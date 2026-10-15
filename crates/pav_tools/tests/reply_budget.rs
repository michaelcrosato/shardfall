//! Reply sizes are part of the agent contract: every character is read on every call. These
//! caps hold the asset tools to the compact shapes (docs/ASSET_SYSTEM_PLAN.md §7.1). Calls go
//! through `pav_tools::call`, as the CLI, MCP and bridges do, so the number tidy pass applies.

use pav_tools::{Output, Session};
use serde_json::{Value, json};

fn call(s: &mut Session, tool: &str, args: Value) -> String {
    match pav_tools::call(s, tool, args.as_object().unwrap()).unwrap() {
        Output::Json(value) => value.to_string(),
        Output::Image { meta, .. } => meta.to_string(),
    }
}

/// f32 values widened to f64 print as `0.10000000149011612`; tidied replies never do.
fn assert_no_float_noise(reply: &str) {
    let digits = reply.split(|c: char| !c.is_ascii_digit() && c != '.').filter(|n| n.contains('.'));
    for number in digits {
        let decimals = number.split('.').nth(1).unwrap_or("");
        assert!(decimals.len() <= 6, "f32 noise {number} in {reply}");
    }
}

#[test]
fn asset_replies_fit_their_budgets() {
    let root = std::env::temp_dir().join(format!("pav-reply-budget-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    // SAFETY: this test binary has one test, and it sets the variable before any tool runs.
    unsafe { std::env::set_var("PAV_ASSETS", &root) };
    let mut s = Session::new("empty", 1).unwrap();
    let budgets: [(&str, Value, usize); 6] = [
        ("asset_edit", json!({"action":"inspect","name":"BUILTIN/bench"}), 1_100),
        ("asset_edit", json!({"action":"copy","from":"BUILTIN/bench","name":"budget_bench","preview":false}), 400),
        (
            "asset_edit",
            json!({"action":"patch","name":"budget_bench","preview":false,"ops":[{"op":"set","part":"seat","fields":{"color":"#3e9a9e"}}]}),
            220,
        ),
        ("asset_preview", json!({"name":"WORKSHOP/budget_bench"}), 160),
        ("asset_preview", json!({}), 160),
        ("studio_status", json!({}), 320),
    ];
    for (tool, args, budget) in budgets {
        let reply = call(&mut s, tool, args.clone());
        assert!(reply.len() <= budget, "{tool} {args}: {} > {budget} chars: {reply}", reply.len());
        assert_no_float_noise(&reply);
    }
    let listing: Value = serde_json::from_str(&call(&mut s, "assets", json!({}))).unwrap();
    assert_eq!(listing["cols"], "name cat size_cm tier mesh tok tags desc");
    let rows = listing["rows"].as_array().unwrap();
    let matched = listing["matched"].as_u64().unwrap() as usize;
    assert!(matched >= 5 && rows.len() == matched.min(20) && listing.get("next").is_some() == (matched > 20));
    // ≤ 46 tokens a row: plan §9.2's 40, plus the store tier and mesh share (store §8, S0). o200k
    // reads these rows at 3.0-3.8 bytes per token (tiktoken on the template rows), so at most 3
    // bytes a token is a safe count without a tokenizer.
    let found: Value = serde_json::from_str(&call(&mut s, "assets", json!({"find":"wood"}))).unwrap();
    assert_eq!(found["rows"][0][0], "BUILTIN/crate", "{found}");
    for row in rows.iter().chain(found["rows"].as_array().unwrap()) {
        let tokens = row.to_string().len().div_ceil(3);
        assert!(tokens <= 46, "listing row of ~{tokens} tokens: {row}");
        assert_no_float_noise(&row.to_string());
    }
    let paged: Value = serde_json::from_str(&call(&mut s, "assets", json!({"limit":2}))).unwrap();
    assert_eq!((paged["rows"].as_array().unwrap().len(), paged["next"].as_u64()), (2, Some(2)));
    let pages = matched.div_ceil(2);
    let last: Value = serde_json::from_str(&call(&mut s, "assets", json!({"limit":2,"page":pages}))).unwrap();
    assert!(last["rows"].as_array().unwrap().len() == 2 - matched % 2 && last.get("next").is_none());
    let _ = std::fs::remove_dir_all(&root);
}
