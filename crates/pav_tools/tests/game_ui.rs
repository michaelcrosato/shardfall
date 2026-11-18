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
    // The skill bar's and potion's keys are glyphs: a keycap's letter, a mouse button by name.
    for line in ["70/70", "50/50", "LMB", "RMB", "Q", "1", "potion", "Hilda the Smith"] {
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

/// Where to click a text in a window (a little inside its top-left corner).
fn at(r: &Value, title: &str, text: &str) -> [f64; 2] {
    let found = window(r, title)["text"].as_array().unwrap().iter().find(|t| t["text"] == text);
    let t = found.unwrap_or_else(|| panic!("no \"{text}\" in {title}: {r}"));
    [t["at"][0].as_f64().unwrap() + 6.0, t["at"][1].as_f64().unwrap() + 6.0]
}

/// The screens at ten feet: the Skills window's rows open a chooser of cards whose click sets
/// the button (Back changes nothing); the bag's auto-pickup is one button that steps; the tree
/// keeps the search field and the legend for a mouse. (On the mouse only: the device whose
/// glyphs are drawn is the process's, and the other tests read the keyboard's. The pad's tree:
/// `pav_ui::arpg_tree`'s tests.)
#[test]
fn skill_rows_open_a_chooser_and_the_bag_steps_auto_pickup() {
    let mut s = Session::new("town", 1).unwrap();
    s.step(30);
    let r = call(&mut s, "ui", json!({"open": "skills", "width": 1920, "height": 1080})).unwrap();
    let rows = texts(&window(&r, "Skills")["text"]);
    // (Not the buttons' glyphs: another test switches the process's device to touch.)
    for line in ["Fireball", "9 mana", "No mana cost"] {
        assert!(rows.iter().any(|t| t == line), "the Skills window shows {line}: {rows:?}");
    }
    // A row opens the chooser (no command yet); Back closes it, changing nothing.
    let r = call(&mut s, "ui", json!({"click": at(&r, "Skills", "Fireball")})).unwrap();
    assert_eq!(r["did"], json!([]), "{r}");
    let r = call(&mut s, "ui", json!({})).unwrap();
    let cards = texts(&window(&r, "Choose a skill")["text"]);
    assert!(cards.iter().any(|t| t.starts_with("Blizzard (level")), "a skill not learned yet shows its level: {cards:?}");
    let r = call(&mut s, "ui", json!({"click": at(&r, "Choose a skill", "Back")})).unwrap();
    assert_eq!(r["did"], json!([]), "{r}");
    let r = call(&mut s, "ui", json!({})).unwrap();
    assert!(!r["windows"].as_array().unwrap().iter().any(|w| w["title"] == "Choose a skill"), "{r}");
    // Again, and a card sets the button: Cleave onto the second (it swaps with the third).
    call(&mut s, "ui", json!({"click": at(&r, "Skills", "Fireball")})).unwrap();
    let r = call(&mut s, "ui", json!({})).unwrap();
    let r = call(&mut s, "ui", json!({"click": at(&r, "Choose a skill", "Cleave")})).unwrap();
    let cleave = pav_core::arpg::data::data().skill_id("cleave").unwrap();
    assert_eq!(r["did"], json!([format!("Bar(1, {cleave})")]), "{r}");
    let bar = &s.sim.state.game.as_ref().unwrap().hero.bar;
    assert_eq!((bar[1].as_str(), bar[2].as_str()), ("cleave", "fireball"));
    // Circle (or Esc) backs out of the chooser before it closes the window.
    s.ui.game.skills = true;
    let r = call(&mut s, "ui", json!({})).unwrap();
    call(&mut s, "ui", json!({"click": at(&r, "Skills", "Slash")})).unwrap();
    s.ui.game.back();
    let r = call(&mut s, "ui", json!({})).unwrap();
    assert_eq!(r["open"], json!(["skills"]), "{r}");
    assert!(!r["windows"].as_array().unwrap().iter().any(|w| w["title"] == "Choose a skill"), "{r}");

    // Auto-pickup: one button naming the level; a click steps to the next.
    let r = call(&mut s, "ui", json!({"close": "all", "open": "inventory"})).unwrap();
    let r = call(&mut s, "ui", json!({"click": at(&r, "Inventory", "Auto-pickup: magic +")})).unwrap();
    assert_eq!(r["did"], json!(["AutoLoot(2)"]), "{r}");
    let r = call(&mut s, "ui", json!({})).unwrap();
    assert!(texts(&window(&r, "Inventory")["text"]).iter().any(|t| t == "Auto-pickup: rare +"), "{r}");

    // The tree: a mouse has the search field and the shapes' legend.
    let r = call(&mut s, "ui", json!({"close": "all", "open": "tree"})).unwrap();
    let mouse = texts(&window(&r, "Passive Tree")["text"]);
    assert!(mouse.iter().any(|t| t == "Search:") && mouse.iter().any(|t| t.starts_with("diamonds:")), "{mouse:?}");
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

/// The pavilion's plaza, where the game opens: every station's name shows, standing on a spot
/// brings up its card, and the card's button names the station for the window to open.
#[test]
fn the_plaza_names_its_stations_and_its_cards_open_them() {
    let mut s = Session::new("world", 1).unwrap();
    let r = call(&mut s, "ui", json!({})).unwrap();
    let hud: Vec<&str> = r["hud"].as_array().unwrap().iter().filter_map(|t| t["text"].as_str()).collect();
    assert!(hud.iter().any(|t| t.contains("Welcome to the Pavilion")), "{r}");
    assert!(hud.contains(&"Object Studio") && hud.contains(&"Field guide"), "{r}");
    let rift = pav_core::plaza::station("shardfall").unwrap();
    let id = s.sim.state.player.unwrap();
    s.sim.set_position(id, rift.stand());
    s.step(1);
    let r = call(&mut s, "ui", json!({})).unwrap();
    let card = window(&r, "Shardfall");
    let texts: Vec<&str> = card["text"].as_array().unwrap().iter().filter_map(|t| t["text"].as_str()).collect();
    // The button names what it does; its key (E) is a glyph on it.
    assert!(texts.contains(&"E"), "{texts:?}");
    let button = card["text"].as_array().unwrap().iter().find(|t| t["text"] == "Begin the descent");
    let at = button.unwrap_or_else(|| panic!("no rift button: {texts:?}"))["at"].clone();
    let r = call(&mut s, "ui", json!({"click": at})).unwrap();
    assert_eq!(r["station"], "shardfall", "{r}");
}

#[test]
fn the_mouse_clicks_things_in_the_world() {
    // An item on the ground by the hero: its name over it is a button, and a click picks it up.
    let mut s = Session::new("town", 1).unwrap();
    s.step(30);
    // (Auto-pickup off, or the hero would take it back where it lands.)
    call(&mut s, "game_cmd", json!({"do": "auto_loot", "rarity": 4})).unwrap();
    let given = call(&mut s, "give", json!({"rarity": "rare", "level": 5, "slot": "weapon"})).unwrap();
    let id = given["item"]["id"].as_u64().unwrap();
    let name = given["item"]["name"].as_str().unwrap().to_string();
    call(&mut s, "game_cmd", json!({"do": "drop", "id": id})).unwrap();
    s.step(120);
    let r = call(&mut s, "ui", json!({})).unwrap();
    // (Each label is an area of its own: the reading lists it with the windows.)
    let all: Vec<Value> = r["hud"]
        .as_array()
        .unwrap()
        .iter()
        .cloned()
        .chain(r["windows"].as_array().unwrap().iter().flat_map(|w| w["text"].as_array().unwrap().clone()))
        .collect();
    let label = all
        .iter()
        .find(|t| t["text"].as_str().is_some_and(|t| t.contains(name.as_str())))
        .unwrap_or_else(|| panic!("no label for {name}: {r}"));
    let at = [label["at"][0].as_f64().unwrap() + 6.0, label["at"][1].as_f64().unwrap() + 6.0];
    let r = call(&mut s, "ui", json!({"click": at})).unwrap();
    assert!(r["did"].as_array().unwrap().iter().any(|d| d.as_str().unwrap().contains("Pickup")), "{r}");
}

#[test]
fn the_mouse_clicks_the_place_the_hero_stands_by() {
    // By Hilda's forge, the "use" prompt under her name is a button: a click opens her shop.
    let mut s = Session::new("town", 1).unwrap();
    s.step(10);
    let smith = s.sim.state.game.as_ref().unwrap().spots.iter().find(|p| p.kind == pav_core::arpg::SpotKind::Vendor).unwrap().pos;
    let id = s.sim.state.player.unwrap();
    s.sim.set_position(id, smith + glam::Vec3::new(0.0, 0.0, 1.2));
    s.step(5);
    let r = call(&mut s, "ui", json!({})).unwrap();
    let all: Vec<Value> = r["hud"]
        .as_array()
        .unwrap()
        .iter()
        .cloned()
        .chain(r["windows"].as_array().unwrap().iter().flat_map(|w| w["text"].as_array().unwrap().clone()))
        .collect();
    let prompt = all
        .iter()
        .find(|t| t["text"].as_str().is_some_and(|t| t.to_lowercase().contains("trade")))
        .unwrap_or_else(|| panic!("no prompt: {r}"));
    let at = [prompt["at"][0].as_f64().unwrap() + 4.0, prompt["at"][1].as_f64().unwrap() + 6.0];
    call(&mut s, "ui", json!({"click": at})).unwrap();
    let r = call(&mut s, "ui", json!({})).unwrap();
    assert!(r["open"].as_array().unwrap().contains(&json!("inventory")), "the shop opens with the bag: {r}");
}
