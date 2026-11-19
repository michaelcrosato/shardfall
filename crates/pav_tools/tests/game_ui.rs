//! The game UI through the tools: `ui` reads the HUD, the open windows and the touch controls
//! as text and clicks them; captures draw the UI over the game.

use pav_tools::{Output, Session};
use serde_json::{Value, json};

/// The device whose glyphs the UI draws is the process's (`pav_ui::controls::current`), and these
/// tests read glyphs on one device or switch to another: one at a time.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

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
    let _one = serial();
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
    let _one = serial();
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
    let _one = serial();
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
    let _one = serial();
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
    let _one = serial();
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
    let _one = serial();
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
    let _one = serial();
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

/// The hero's panels as tabs: on a phone the bag's button opens the bag, and its tabs reach the
/// tree (its points to spend on its tab), whose tabs reach the hero's sheet: without them a touch
/// screen had no way to the sheet, the skills or the tree.
#[test]
fn the_panels_tabs_reach_every_panel_on_a_phone() {
    let _one = serial();
    let mut s = Session::new("town", 1).unwrap();
    s.step(30);
    call(&mut s, "hero", json!({"level": 5})).unwrap();
    s.step(2);
    let phone = |extra: Value| {
        let mut a = json!({"device": "touch", "width": 390, "height": 844});
        for (k, v) in extra.as_object().unwrap() {
            a[k] = v.clone();
        }
        a
    };
    call(&mut s, "ui", phone(json!({"open": "inventory"}))).unwrap();
    let r = call(&mut s, "ui", phone(json!({}))).unwrap();
    let tabs = texts(&window(&r, "Inventory")["text"]);
    for tab in ["Bag", "Hero", "Skills", "Passives · 4"] {
        assert!(tabs.iter().any(|t| t == tab), "the bag's tabs: {tabs:?}");
    }
    let r = call(&mut s, "ui", phone(json!({"click": at(&r, "Inventory", "Passives · 4")}))).unwrap();
    assert_eq!(r["open"], json!(["tree"]), "{r}");
    let r = call(&mut s, "ui", phone(json!({}))).unwrap();
    let r = call(&mut s, "ui", phone(json!({"click": at(&r, "Passive Tree", "Hero")}))).unwrap();
    assert_eq!(r["open"], json!(["character"]), "{r}");
}

/// The game UI driven by a pad as the window drives it: `PadNav` turns the pad into egui's input
/// between passes, then the device is set and the UI drawn (`pav_app`'s order).
struct OnPad {
    ctx: egui::Context,
    nav: pav_ui::padnav::PadNav,
    ui: pav_ui::arpg_items::GameUi,
    time: f64,
    /// The screen in points: 1280x720 is a 16:9 screen at the menu's Medium size, 1067x600 at
    /// Large.
    size: egui::Vec2,
}

impl OnPad {
    fn new() -> Self {
        let ctx = egui::Context::default();
        pav_ui::fonts::install_embedded(&ctx, &pav_core::lines::language(), &[]);
        pav_ui::game_style(&ctx, false, false);
        pav_ui::controls::set_current(pav_ui::Device::Gamepad, pav_ui::controls::current().1);
        Self { ctx, nav: Default::default(), ui: Default::default(), time: 0.0, size: egui::vec2(1280.0, 720.0) }
    }

    /// One frame at 1280x720: the pad when it is in hand (`pad`, with the mouse where it last was),
    /// else the keyboard and mouse, then one pass of the UI. What the UI did.
    fn frame(
        &mut self,
        g: &pav_core::arpg::GameFrame,
        pad: Option<pav_ui::padnav::PadIn>,
        mouse: Option<egui::Pos2>,
    ) -> Vec<pav_core::arpg::GameCmd> {
        let events = match &pad {
            Some(input) => self.nav.frame(&self.ctx, &pav_ui::padnav::PadIn { dt: 1.0 / 60.0, ..input.clone() }, mouse),
            None => {
                let out = self.nav.release();
                self.nav.reset();
                out
            }
        };
        self.ui.pad = pad.is_some();
        self.ui.tree.pad = pad.is_some();
        let size = self.size;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            focused: true,
            time: Some(self.time),
            predicted_dt: 1.0 / 60.0,
            events,
            ..Default::default()
        };
        self.time += 1.0 / 60.0;
        let proj = pav_ui::arpg_ui::Projector { vp: glam::Mat4::IDENTITY, size };
        let ui = &mut self.ui;
        let mut did = Vec::new();
        let mut out = self.ctx.run_ui(raw, |root| {
            let ctx = root.ctx().clone();
            did = ui.ui(&ctx, g, &proj);
        });
        out.textures_delta.clear();
        did
    }

    fn rect(&self, id: egui::Id) -> Option<egui::Rect> {
        self.ctx.read_response(id).map(|r| r.rect)
    }

    /// Where the screens drawn last pass ask the pad to land.
    fn asked(&self) -> Option<egui::Id> {
        self.ctx.data(|d| d.get_temp(egui::Id::new(pav_ui::padnav::FOCUS)))
    }
}

/// The keys open the tree, then the Skills window over it (P, then K); a pad picked up on a row
/// opens the chooser on the skill the button has, and choosing comes back to that row: the
/// tree's canvas, asked for while it is the only panel open, never takes the pad from them.
#[test]
fn a_pad_beside_the_tree_keeps_the_skill_choosers_place() {
    let _one = serial();
    let mut s = Session::new("town", 1).unwrap();
    s.step(30);
    let frame = s.sim.frame();
    let g = frame.game.as_deref().expect("a game");
    let mut p = OnPad::new();
    p.ui.tree.open = true;
    for _ in 0..3 {
        p.frame(g, None, None);
    }
    p.ui.skills = true;
    for _ in 0..3 {
        p.frame(g, None, None);
    }
    let row = egui::Id::new(("skill_slot", 2usize));
    let at = p.rect(row).expect("the third row is drawn");
    // Picked up with the mouse on the third row: on it.
    let pad = pav_ui::padnav::PadIn::default;
    p.frame(g, Some(pad()), Some(at.center()));
    p.frame(g, Some(pad()), None);

    assert_eq!(p.nav.focused(), Some(at), "picked up on the row under the mouse");
    // Cross: the chooser opens on the card of the skill the button has.
    p.frame(g, Some(pav_ui::padnav::PadIn { cross: true, ..pad() }), None);
    for _ in 0..4 {
        p.frame(g, Some(pad()), None);
    }
    let chooser = p.ctx.memory(|m| m.area_rect(egui::Id::new("skill_chooser"))).expect("the chooser is open");
    let card = p.asked().and_then(|id| p.rect(id)).expect("the chooser asks for a card");
    assert!(chooser.contains_rect(card), "the chooser asks for one of its cards, not the tree's canvas: {card:?} in {chooser:?}");
    assert_eq!(p.nav.focused(), Some(card), "the pad is on the card the chooser asks for");
    // Cross on it (the skill it has): it sets the button, the chooser closes, and the pad is back
    // on the third row.
    let did = p.frame(g, Some(pav_ui::padnav::PadIn { cross: true, ..pad() }), None);
    assert!(did.iter().any(|c| matches!(c, pav_core::arpg::GameCmd::Bar(2, _))), "chose for the third button: {did:?}");
    for _ in 0..4 {
        p.frame(g, Some(pad()), None);
    }
    assert_eq!(p.nav.focused(), p.rect(row), "back on the row the chooser was opened from");
}

/// P opens the passive tree, then the pad is picked up: it lands on the tree's canvas (the stick
/// zooms, the D-pad walks the nodes), not on the Bag tab above it.
#[test]
fn a_pad_picked_up_on_a_tree_the_keys_opened_lands_on_its_canvas() {
    let _one = serial();
    let mut s = Session::new("town", 1).unwrap();
    s.step(30);
    let frame = s.sim.frame();
    let g = frame.game.as_deref().expect("a game");
    let mut p = OnPad::new();
    p.ui.tree.open = true;
    for _ in 0..3 {
        p.frame(g, None, None);
    }
    for _ in 0..3 {
        p.frame(g, Some(pav_ui::padnav::PadIn::default()), None);
    }
    assert!(p.nav.free() && p.nav.focused().is_none(), "on the canvas, not a tab: {:?}", p.nav.focused());
}

/// The keys open the bag, then the tree (I, then P): a pad picked up lands on the tree's canvas,
/// and there opens a taken mastery's options: the pad stays in them (Up doesn't step out to the
/// tree's Reset all behind them), and Circle brings it back to the node, not to the bag.
/// A hero at level 20 in town with the mastery nearest the start taken (and the way to it).
fn mastery_hero() -> (Session, u32) {
    let mut s = Session::new("town", 1).unwrap();
    s.step(30);
    call(&mut s, "hero", json!({"level": 20})).unwrap();
    s.step(1);
    let d = pav_core::arpg::data::data();
    let none = std::collections::BTreeSet::new();
    let (mastery, _) = d
        .tree
        .nodes
        .iter()
        .filter(|n| n.kind == pav_core::arpg::tree::NodeKind::Mastery && n.ring == 0)
        .filter_map(|n| d.tree.path_to(&none, n.id).map(|p| (n.id, p.len())))
        .min_by_key(|(_, len)| *len)
        .expect("a mastery");
    call(&mut s, "tree", json!({"take": mastery.to_string()})).unwrap();
    s.step(1);
    (s, mastery)
}

#[test]
fn a_masterys_options_keep_the_pad_and_give_it_back_to_the_node() {
    let _one = serial();
    let (mut s, mastery) = mastery_hero();
    let frame = s.sim.frame();
    let g = frame.game.as_deref().expect("a game");
    assert!(g.inv.as_ref().is_some_and(|i| i.tree.contains(&mastery)), "the mastery is taken");
    let mut p = OnPad::new();
    p.ui.inventory = true;
    for _ in 0..3 {
        p.frame(g, None, None);
    }
    p.ui.tree.open = true;
    for _ in 0..3 {
        p.frame(g, None, None);
    }
    // Picked up with the mouse elsewhere: on the tree's canvas (it is on top), not on the bag's
    // first slot hidden under it. Then off to a node.
    let pad = pav_ui::padnav::PadIn::default;
    p.frame(g, Some(pad()), None);
    p.frame(g, Some(pad()), None);
    assert!(p.nav.free(), "on the canvas, not the bag: {:?}", p.nav.focused());
    for _ in 0..8 {
        p.frame(g, Some(pav_ui::padnav::PadIn { stick: glam::Vec2::new(0.5, 0.4), ..pad() }), None);
    }
    p.frame(g, Some(pad()), None);
    assert!(p.nav.free(), "still on the canvas");
    let node = p.nav.pointer.expect("a pointer");
    // The mastery's options: the pad is on one of them, and Up keeps it there.
    p.ui.tree.show_mastery(mastery);
    for _ in 0..3 {
        p.frame(g, Some(pad()), None);
    }
    let options = p.ctx.memory(|m| m.area_rect(egui::Id::new("mastery"))).expect("the options are open");
    let inside = |p: &OnPad| p.nav.focused().is_some_and(|r| options.contains_rect(r));
    assert!(inside(&p), "on an option: {:?} in {options:?}", p.nav.focused());
    for _ in 0..4 {
        p.frame(g, Some(pav_ui::padnav::PadIn { taps: vec![pav_ui::padnav::Dir::Up], ..pad() }), None);
        assert!(inside(&p), "Up stays in the options: {:?} in {options:?}", p.nav.focused());
    }
    // Circle: back on the node, beside the bag all the same.
    p.ui.back();
    for _ in 0..4 {
        p.frame(g, Some(pad()), None);
    }
    assert!(p.nav.free(), "back on the tree's canvas, not the bag: {:?}", p.nav.focused());
    assert!(p.nav.pointer.is_some_and(|q| q.distance(node) < 1.0), "back at {node:?}: {:?}", p.nav.pointer);
}

/// Once the skill chooser has been used, an idle Skills window doesn't keep the pad from the tree:
/// K, then P (the tree comes up over the Skills window), a pad picked up on the tree's canvas,
/// a mastery's options closed: the pad is back on the node, not on a Skills row hidden under it.
#[test]
fn after_the_chooser_the_skills_window_leaves_the_tree_the_pad() {
    let _one = serial();
    let (mut s, mastery) = mastery_hero();
    let frame = s.sim.frame();
    let g = frame.game.as_deref().expect("a game");
    let mut p = OnPad::new();
    let pad = pav_ui::padnav::PadIn::default;
    let cross = || pav_ui::padnav::PadIn { cross: true, ..pad() };
    p.ui.skills = true;
    for _ in 0..3 {
        p.frame(g, None, None);
    }
    // The chooser, on the pad: the first row, then the card it starts on.
    let row = egui::Id::new(("skill_slot", 0usize));
    let at = p.rect(row).expect("the first row is drawn");
    p.frame(g, Some(pad()), Some(at.center()));
    p.frame(g, Some(pad()), None);
    p.frame(g, Some(cross()), None);
    for _ in 0..4 {
        p.frame(g, Some(pad()), None);
    }
    let did = p.frame(g, Some(cross()), None);
    assert!(did.iter().any(|c| matches!(c, pav_core::arpg::GameCmd::Bar(0, _))), "chose: {did:?}");
    for _ in 0..4 {
        p.frame(g, Some(pad()), None);
    }
    assert_eq!(p.nav.focused(), p.rect(row), "back on the row");
    // The keys open the tree over it; the pad picked up on the tree's canvas, clear of the Skills
    // window, goes to a node.
    for _ in 0..5 {
        p.frame(g, None, None);
    }
    p.ui.tree.open = true;
    for _ in 0..3 {
        p.frame(g, None, None);
    }
    let tree = p.ctx.memory(|m| m.area_rect(egui::Id::new(pav_ui::arpg_tree::TREE_WINDOW))).expect("the tree is open");
    p.frame(g, Some(pad()), Some(egui::pos2(tree.left() + 150.0, tree.center().y)));
    p.frame(g, Some(pad()), None);
    assert!(p.nav.free(), "on the canvas: {:?}", p.nav.focused());
    let node = p.nav.pointer.expect("a pointer");
    // Its options, then Circle: back on the node.
    p.ui.tree.show_mastery(mastery);
    for _ in 0..3 {
        p.frame(g, Some(pad()), None);
    }
    p.ui.back();
    for _ in 0..4 {
        p.frame(g, Some(pad()), None);
    }
    assert!(p.nav.free(), "back on the tree's canvas, not a Skills row: {:?}", p.nav.focused());
    assert!(p.nav.pointer.is_some_and(|q| q.distance(node) < 1.0), "back at {node:?}: {:?}", p.nav.pointer);
}

/// K, the chooser opened on the pad, then P: the tree comes up over the Skills window. Choosing a
/// card closes the chooser, and the row it was opened from is hidden under the tree: the pad lands
/// on the tree's canvas, never on a button that lies over the row (Reset all, at the Large size).
/// Every row of a mid-game hero (skill upgrades lengthen the rows), at Medium and Large.
#[test]
fn a_chooser_closed_under_the_tree_gives_the_pad_to_the_tree() {
    let _one = serial();
    let mut s = Session::new("town", 1).unwrap();
    s.step(30);
    call(&mut s, "hero", json!({"level": 30, "gold": 5000})).unwrap();
    s.step(1);
    let frame = s.sim.frame();
    let g = frame.game.as_deref().expect("a game");
    let pad = pav_ui::padnav::PadIn::default;
    let cross = || pav_ui::padnav::PadIn { cross: true, ..pad() };
    for size in [egui::vec2(1280.0, 720.0), egui::vec2(1067.0, 600.0)] {
        for slot in 0..3usize {
            let mut p = OnPad::new();
            p.size = size;
            p.ui.skills = true;
            for _ in 0..3 {
                p.frame(g, None, None);
            }
            let row = egui::Id::new(("skill_slot", slot));
            let at = p.rect(row).expect("the row is drawn");
            p.frame(g, Some(pad()), Some(at.center()));
            p.frame(g, Some(pad()), None);
            p.frame(g, Some(cross()), None);
            for _ in 0..4 {
                p.frame(g, Some(pad()), None);
            }
            assert!(p.ctx.memory(|m| m.area_rect(egui::Id::new("skill_chooser"))).is_some(), "the chooser is open");
            // P on the keyboard: the tree comes up over the Skills window (the chooser on top).
            p.ui.tree.open = true;
            for _ in 0..4 {
                p.frame(g, None, None);
            }
            // The pad again: on the chooser's card; Cross takes it.
            p.frame(g, Some(pad()), None);
            p.frame(g, Some(pad()), None);
            let did = p.frame(g, Some(cross()), None);
            assert!(did.iter().any(|c| matches!(c, pav_core::arpg::GameCmd::Bar(b, _) if *b as usize == slot)), "chose: {did:?}");
            for _ in 0..4 {
                p.frame(g, Some(pad()), None);
            }
            let at = format!("row {slot} at {size:?}");
            assert!(p.nav.free(), "{at}: on the tree's canvas, not {:?}", p.nav.focused());
            // And Cross there takes or shows a node: it refunds nothing.
            let did = p.frame(g, Some(cross()), None);
            assert!(!did.iter().any(|c| matches!(c, pav_core::arpg::GameCmd::Respec)), "{at}: {did:?}");
        }
    }
}

/// Reset all refunds every node for gold: its first press asks (the button says so), the second
/// does it; one press left alone for a few seconds asks again next time.
#[test]
fn reset_all_asks_before_it_refunds_the_tree() {
    let _one = serial();
    let mut s = Session::new("town", 1).unwrap();
    s.step(30);
    call(&mut s, "hero", json!({"level": 10, "gold": 5000})).unwrap();
    s.step(1);
    let r = call(&mut s, "ui", json!({"open": "tree", "width": 1280, "height": 720})).unwrap();
    let reset =
        |r: &Value| texts(&window(r, "Passive Tree")["text"]).into_iter().find(|t| t.contains("eset all")).expect("Reset all");
    let first = reset(&r);
    assert!(first.starts_with("Reset all"), "{first}");
    let r = call(&mut s, "ui", json!({"click": at(&r, "Passive Tree", &first)})).unwrap();
    assert_eq!(r["did"], json!([]), "the first press only asks: {r}");
    let r = call(&mut s, "ui", json!({})).unwrap();
    let again = reset(&r);
    assert!(again.starts_with("Again to reset all"), "{again}");
    let r = call(&mut s, "ui", json!({"click": at(&r, "Passive Tree", &again)})).unwrap();
    assert_eq!(r["did"], json!(["Respec"]), "the second does it: {r}");
}
