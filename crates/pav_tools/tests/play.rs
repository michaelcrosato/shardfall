//! The `play` tool: without a game it names the ones there are; with one it reports the HUD,
//! outcome and state, and lets the game's bot play on or over fresh seeds.

use pav_tools::{Output, Session};
use serde_json::{Value, json};

fn call(s: &mut Session, tool: &str, args: Value) -> anyhow::Result<Value> {
    match pav_tools::call(s, tool, args.as_object().unwrap())? {
        Output::Json(value) => Ok(value),
        Output::Image { meta, .. } => Ok(meta),
    }
}

#[test]
fn play_reports_and_drives_a_game() {
    let mut s = Session::new("empty", 1).unwrap();
    let err = call(&mut s, "play", json!({})).unwrap_err().to_string();
    assert!(err.contains("starter"), "{err}");

    call(&mut s, "load", json!({"scene": "starter"})).unwrap();
    let r = call(&mut s, "play", json!({})).unwrap();
    assert_eq!(r["outcome"], "playing");
    assert_eq!(r["hud"], json!(["Shards 0/5", "Lives 3"]));

    let sweep = call(&mut s, "play", json!({"bot": true, "seeds": 3})).unwrap();
    assert_eq!(sweep["wins"], "3/3", "{sweep}");
    assert!(sweep.to_string().len() < 400, "{sweep}");
    assert_eq!(call(&mut s, "play", json!({})).unwrap()["status"]["time"], 0.0, "a sweep leaves the session alone");

    let r = call(&mut s, "play", json!({"bot": true})).unwrap();
    assert_eq!(r["outcome"], "won", "{r}");
    assert!(call(&mut s, "play", json!({"seeds": 2})).is_err(), "seeds= needs bot=true");
}
