//! The game UI through the tools: `ui` reads the HUD, the open windows and the touch controls
//! as text and clicks them; captures draw the UI over the game.

use pav_tools::{Output, Session};
use serde_json::{Value, json};

fn call(s: &mut Session, tool: &str, args: Value) -> anyhow::Result<Value> {
    match pav_tools::call(s, tool, args.as_object().unwrap())? {
        Output::Json(value) => Ok(value),
        Output::Image { meta, .. } => Ok(meta),
    }
}

fn texts(list: &Value) -> Vec<String> {
    list.as_array().unwrap().iter().map(|t| t["text"].as_str().unwrap().to_owned()).collect()
}

fn window<'a>(r: &'a Value, title: &str) -> &'a Value {
    let found = r["windows"].as_array().unwrap().iter().find(|w| w["title"] == title);
    found.unwrap_or_else(|| panic!("no window \"{title}\": {r}"))
}

#[test]
fn ui_reads_the_hud_windows_and_touch_controls() {
    let mut s = Session::new("town", 1).unwrap();
    s.step(30);
    let r = call(&mut s, "ui", json!({})).unwrap();
    let hud = texts(&r["hud"]);
    for line in ["70/70", "50/50", "LMB", "1 potion", "Hilda the Smith"] {
        assert!(hud.iter().any(|t| t == line), "the HUD shows {line}: {hud:?}");
    }
    assert_eq!(r["windows"], json!([]));

    let r = call(&mut s, "ui", json!({"open": "inventory,tree"})).unwrap();
    assert_eq!(r["open"], json!(["inventory", "tree"]));
    let inventory = texts(&window(&r, "Inventory")["text"]);
    assert!(inventory.iter().any(|t| t == "helmet") && inventory.iter().any(|t| t == "Sort"), "{inventory:?}");
    assert!(texts(&window(&r, "Passive Tree")["text"]).iter().any(|t| t.contains("points to spend")));
    // Text on a window's layer is the window's, even where the HUD shows under it.
    assert!(!texts(&r["hud"]).iter().any(|t| t == "Sort"));

    // A click presses what is there: the inventory's Sort button.
    let sort = &window(&r, "Inventory")["text"].as_array().unwrap().iter().find(|t| t["text"] == "Sort").unwrap()["at"];
    let at = [sort[0].as_f64().unwrap() + 8.0, sort[1].as_f64().unwrap() + 7.0];
    let r = call(&mut s, "ui", json!({"close": "tree", "click": at})).unwrap();
    assert_eq!(r["did"], json!(["Sort"]), "{r}");

    let r = call(&mut s, "ui", json!({"close": "all", "device": "touch", "width": 844, "height": 390})).unwrap();
    let controls: Vec<&str> = r["controls"].as_array().unwrap().iter().map(|c| c["control"].as_str().unwrap()).collect();
    for c in ["Dodge", "Potion", "Bag", "Menu", "Skill(1)"] {
        assert!(controls.contains(&c), "touch shows {c}: {controls:?}");
    }
    let err = call(&mut s, "ui", json!({"open": "inventroy"})).unwrap_err().to_string();
    assert!(err.contains("inventory"), "{err}");
}

#[test]
fn captures_draw_the_game_ui() {
    let mut s = Session::new("town", 1).unwrap();
    s.step(30);
    let (w, h) = (640u32, 360u32);
    let without = s.render(w, h).unwrap();
    let (with, drawn) = s.render_ui(w, h, Default::default()).unwrap();
    assert!(drawn.is_some());
    // Pixels the UI changed, as a share of a band of rows: the orbs and skill bar fill the bottom.
    let changed = |y0: u32, y1: u32| {
        let px = (y0 * w * 4) as usize..(y1 * w * 4) as usize;
        let n = without[px.clone()].chunks(4).zip(with[px].chunks(4)).filter(|(a, b)| (0..3).any(|c| a[c].abs_diff(b[c]) > 24));
        n.count() as f32 / ((y1 - y0) * w) as f32
    };
    assert!(changed(h - 60, h) > 0.15, "the bottom HUD is drawn: {}", changed(h - 60, h));
    assert!(changed(h / 3, h / 2) < 0.1, "the middle of the view stays the game's: {}", changed(h / 3, h / 2));
    let out = std::env::temp_dir().join("pav-test-ui-off.png");
    let r = call(&mut s, "capture", json!({"width": w, "height": h, "ui": false, "out": out})).unwrap();
    assert_eq!(r["width"], w);
}

#[test]
fn ui_reads_a_hook_games_hud() {
    let mut s = Session::new("starter", 1).unwrap();
    let r = call(&mut s, "ui", json!({})).unwrap();
    assert_eq!(texts(&window(&r, "Shards 0/5")["text"]), ["Shards 0/5", "Lives 3"], "{r}");
    let mut s = Session::new("empty", 1).unwrap();
    assert!(call(&mut s, "ui", json!({})).unwrap_err().to_string().contains("no game UI"));
}
