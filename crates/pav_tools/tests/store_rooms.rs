//! The asset store's rooms (docs/AGENT_ASSET_STORE.md §10): the Library Hall is current with
//! the embedded index, the Hall, Limits and Catalogue pass `room_check`, every id a sign names
//! resolves (and the library rooms' signs name all they show), every Limits exhibit names the
//! check that catches it or says "not caught" and that is true, the Catalogue pages through every
//! store row headless (rewind and snapshots keep a page), the Store Showcase shows every core
//! material, the Clip Browser plays one clip on its whole cast and steps through the library, and
//! Kenney Mini Dungeon goes through every step of the store.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Once;

use glam::{Quat, Vec3};
use pav_core::catalogue::{self, CatalogueDef};
use pav_core::entity::Behavior;
use pav_core::params::ChoiceParam;
use pav_core::props::{self, index};
use pav_core::room::{RoomDef, load_sources, parse_all};
use pav_tools::asset_tools::check_lines;
use pav_tools::{Output, Session, call, hall};
use serde_json::{Value, json};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every room as the repository has it.
fn rooms() -> Vec<(String, RoomDef)> {
    let (defs, errors) = parse_all(&load_sources(Some(&repo().join("rooms"))));
    assert!(errors.is_empty(), "{errors:?}");
    defs
}

fn room(key: &str) -> RoomDef {
    rooms().into_iter().find(|(k, _)| k == key).unwrap_or_else(|| panic!("no room {key}")).1
}

fn json_of(s: &mut Session, tool: &str, args: Value) -> Value {
    match call(s, tool, args.as_object().unwrap()) {
        Ok(Output::Json(v)) => v,
        Ok(Output::Image { .. }) => panic!("{tool}: an image"),
        Err(e) => panic!("{tool}: {e}"),
    }
}

/// The committed packs and the catalogue read files under the repository's props root.
fn props_root() {
    static SET: Once = Once::new();
    SET.call_once(|| {
        // SAFETY: set once, before any test of this file reads it.
        unsafe { std::env::set_var("PAV_ASSETS", repo().join("assets/props")) };
    });
}

const STORE_ROOMS: [&str; 5] = [hall::KEY, "limits", "catalogue", "store_showcase", "clip_browser"];
const LIBRARY_ROOMS: [&str; 7] = ["motion_library", "mix_match", "styles", "filters", "drift", "feel_lab", "particles"];

#[test]
fn library_hall_is_generated_from_the_index() {
    let (current, fresh) = hall::expected(&repo().join("rooms")).unwrap();
    assert!(current == fresh, "rooms/{}.toml is stale: rebuild, then run `pav assets reindex=hall`", hall::KEY);
    let def = room(hall::KEY);
    // Every standard row has its plinth and its sign.
    let standard: Vec<&index::Row> = index::embedded().iter().filter(|row| row.level == "standard").collect();
    assert!(standard.len() >= 32);
    for row in &standard {
        assert!(def.objects.iter().any(|o| o.asset.as_deref() == Some(row.name.as_str())), "{} has no plinth", row.name);
        assert!(def.labels.iter().any(|l| l.text == hall::sign(row)), "{} has no sign", row.name);
    }
    // Kestrel sits through the chair's `sit` socket: his first clip's pelvis lands on the seat.
    let seated = def.npcs.iter().find(|n| !n.seat.is_empty()).expect("a seated performer");
    let chair = def.objects.iter().find(|o| o.name == seated.seat).unwrap();
    let (_, chair_def, _) = chair.placed().unwrap();
    let socket = chair_def.compiled.sockets["sit"].clone();
    let seat = chair.pos + chair.local_rot() * socket.pos;
    let (feet, yaw) = def.seat_spot(seated).unwrap().unwrap();
    let puppet = seated.puppet().unwrap();
    let clip = pav_core::clips::find(&seated.clips[0]).unwrap();
    let pelvis = feet + Quat::from_rotation_y(yaw.to_radians()) * pav_core::clips::pose_of(&puppet, clip, 0.0, 0).unwrap().pelvis;
    assert!(Vec3::new(pelvis.x - seat.x, 0.0, pelvis.z - seat.z).length() < 0.03, "pelvis {pelvis} over seat {seat}");
    assert!((pelvis.y - seat.y).abs() < 0.1, "pelvis {pelvis} at seat height {seat}");
    // Ysolde holds the staff by its grip; she and a second Kestrel play one clip.
    let ysolde = def.npcs.iter().find(|n| n.name == "ysolde").unwrap().puppet().unwrap();
    assert!(props::resolve_parts(&ysolde.weapon.asset, 0).unwrap().sockets.contains_key("grip"));
    let shared: Vec<_> = def.npcs.iter().filter(|n| n.clips.len() == 1 && n.clips == def.npcs[1].clips).collect();
    assert_eq!(shared.len(), 2);
    assert_ne!(shared[0].puppet().unwrap().leg_length, shared[1].puppet().unwrap().leg_length, "two bodies");
}

#[test]
fn store_rooms_pass_room_check() {
    let mut s = Session::new("empty", 1).unwrap();
    for key in STORE_ROOMS {
        let path = repo().join("rooms").join(format!("{key}.toml"));
        let out = json_of(&mut s, "room_check", json!({"path": path.to_str().unwrap()}));
        assert_eq!(out["ok"], true, "{key}: {out}");
    }
}

/// The ids a label names: (kind, id). `ENTRIES · <kind>` starts a block whose lines list ids
/// split by " · " (a line in brackets is a remark); elsewhere a line or " · " item that starts
/// like an asset, material or clip id is one.
fn named(text: &str) -> Vec<(String, String)> {
    let mut kind: Option<String> = None;
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(k) = line.strip_prefix("ENTRIES · ") {
            kind = Some(k.to_owned());
            continue;
        }
        if line.starts_with('(') {
            continue;
        }
        for item in line.split(" · ").map(str::trim).filter(|item| !item.is_empty()) {
            match &kind {
                Some(kind) => out.push((kind.clone(), item.to_owned())),
                None => {
                    let id_kind = match item.split('/').next().unwrap_or_default() {
                        "BUILTIN" | "LIB" => "props",
                        "MAT" => "materials",
                        "QUATERNIUS" | "MESH2MOTION" | "CMU" | "STYLE100" => "clips",
                        _ => continue,
                    };
                    out.push((id_kind.to_owned(), item.to_owned()));
                }
            }
        }
    }
    out
}

/// Whether `id` of `kind` resolves in this build (in `def`, for a room's own vehicles).
fn resolves(kind: &str, id: &str, def: &RoomDef) -> bool {
    use pav_view::look::{Section, look_preset, preset};
    let section = |name: &str| Section::ALL.into_iter().find(|s| s.key() == name);
    match kind {
        "props" => props::resolve_ref(id).is_ok() && props::store::embedded(id),
        "materials" => pav_core::material::Library::core().get(id).is_some(),
        "clips" => pav_core::clips::find(id).is_some(),
        "looks" => look_preset(id).is_some(),
        "look presets" => id.split_once('/').is_some_and(|(s, name)| section(s).is_some_and(|s| preset(s, name).is_some())),
        "look sections" => section(id).is_some(),
        "styles" => pav_core::shape::Look::NAMES.contains(&id),
        "movement models" => pav_core::character::MovementModel::NAMES.contains(&id),
        "particle presets" => props::doc::PRESETS.contains(&id),
        "vehicles" => def.objects.iter().any(|o| o.name == id && o.vehicle.is_some()),
        _ => false,
    }
}

#[test]
fn signs_name_entries_that_resolve() {
    let defs = rooms();
    let mut checked = 0;
    for (key, def) in &defs {
        for label in &def.labels {
            for (kind, id) in named(&label.text) {
                assert!(resolves(&kind, &id, def), "{key}: the sign names {kind} \"{id}\", which does not resolve");
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "ids checked: {checked}");
    // The library rooms each name what they show.
    for key in LIBRARY_ROOMS {
        let def = &defs.iter().find(|(k, _)| k == key).unwrap().1;
        let shown: BTreeSet<(String, String)> = def.labels.iter().flat_map(|l| named(&l.text)).collect();
        assert!(shown.iter().any(|(kind, _)| kind != "props"), "{key}: no ENTRIES sign");
        let has = |kind: &str, id: &str| shown.contains(&(kind.to_owned(), id.to_owned()));
        for clip in def.npcs.iter().flat_map(|n| &n.clips) {
            assert!(has("clips", clip), "{key}: performs {clip} but its sign does not name it");
        }
        for preset in def.objects.iter().filter_map(|o| o.particles.as_ref()).map(|p| p.preset.as_str()) {
            assert!(has("particle presets", preset), "{key}: shows the {preset} preset but its sign does not name it");
        }
        for car in def.objects.iter().filter(|o| o.vehicle.is_some()) {
            assert!(has("vehicles", &car.name), "{key}: has the vehicle {} but its sign does not name it", car.name);
        }
    }
}

/// The two-hand clip's closest hand gap (m) on a performer.
fn hand_gap(npc: &pav_core::room::NpcDef) -> f32 {
    let puppet = npc.puppet().unwrap();
    let clip = pav_core::clips::find(&npc.clips[0]).unwrap();
    let dur = pav_core::clips::with(clip, |c| c.keys.last().map_or(0.0, |k| k.t)).unwrap();
    (0..40)
        .map(|i| pav_core::clips::pose_of(&puppet, clip, dur * i as f32 / 40.0, 0).unwrap())
        .map(|sk| sk.hand[0].distance(sk.hand[1]))
        .fold(f32::MAX, f32::min)
}

#[test]
fn limits_signs_tell_the_truth() {
    let def = room("limits");
    let numbered: Vec<(u32, &str)> = def
        .labels
        .iter()
        .filter_map(|l| l.text.split_once(" · ").and_then(|(n, _)| n.parse().ok()).map(|n| (n, l.text.as_str())))
        .collect();
    assert_eq!(numbered.iter().map(|(n, _)| *n).collect::<Vec<_>>(), (1..=7).collect::<Vec<_>>(), "exhibits 1-7, in order");
    let mut s = Session::new("empty", 1).unwrap();
    for (n, text) in &numbered {
        let caught = text.split("caught by ").nth(1).map(|rest| rest.split([' ', '\n']).next().unwrap_or_default());
        assert!(caught.is_some() || text.contains("not caught"), "exhibit {n} names no check and does not say 'not caught'");
        if let Some(object) = def.objects.iter().find(|o| o.name == format!("exhibit_{n}")) {
            // What `check` says about the object as placed (its patch applied).
            let (_, placed, _) = object.placed().unwrap();
            let found = check_lines(&placed);
            match caught {
                Some(code) => assert!(
                    found.iter().any(|f| f.starts_with(&format!("{code}: "))),
                    "exhibit {n}: {code} expected, check says {found:?}"
                ),
                None => assert!(found.is_empty(), "exhibit {n} says not caught, but check says {found:?}"),
            }
        }
    }
    // 1 and 2 are both 48 cm chairs: one placed at half scale, one a copy scaled to half.
    let height = |o: &pav_core::room::ObjectDef| o.placed().unwrap().1.compiled.bounds.size().y * o.scale;
    for n in [1, 2] {
        let o = def.objects.iter().find(|o| o.name == format!("exhibit_{n}")).unwrap();
        assert!((height(o) - 0.48).abs() < 0.01, "exhibit {n} is {} m tall", height(o));
    }
    // 6: the walk's worst point, as `clips name=` reports it.
    let walk = def.npcs.iter().find(|n| n.clips == ["STYLE100/WildLegs_Walk"]).expect("a WildLegs_Walk performer");
    let fit = json_of(&mut s, "clips", json!({"name": walk.clips[0]}))["fit_mm"].clone();
    let six = numbered[5].1;
    assert!(six.contains(&format!("{} mm off (mean {} mm)", fit[1], fit[0])), "exhibit 6 says {six:?}; fit_mm is {fit}");
    // 7: the same clip brings the hands together on one body and leaves them apart on the other.
    let two: Vec<_> = def.npcs.iter().filter(|n| n.clips == ["QUATERNIUS/Pistol_Idle_Loop"]).collect();
    let (standard, short) = (hand_gap(two[0]), hand_gap(two[1]));
    let seven = numbered[6].1;
    let says = format!(
        "hands {} cm apart on the standard body, {} cm on the short one",
        (standard * 100.0).round(),
        (short * 100.0).round()
    );
    assert!(seven.contains(&says), "exhibit 7 should say {says:?}");
    assert!(short > standard * 2.0, "{standard} vs {short}");
    // The imports sign counts the committed imports over 30% mesh afresh.
    let mut imports = Vec::new();
    for pack in std::fs::read_dir(repo().join("assets/props/lib")).unwrap().filter_map(Result::ok) {
        if pack.file_name() == props::store::CORE {
            continue;
        }
        let text = std::fs::read_to_string(pack.path().join("index.tsv")).unwrap();
        imports.extend(index::parse(&text, props::Tier::Disk).unwrap());
    }
    let over = imports.iter().filter(|row| catalogue::mesh_share(row) > 30).count();
    let sign = &def.labels.iter().find(|l| l.text.contains("imports are over 30% mesh")).expect("the imports sign").text;
    assert!(
        sign.contains(&format!("{over} of {} imports are over 30% mesh", imports.len())),
        "{over} of {}: {sign}",
        imports.len()
    );
    assert!(sign.contains("not caught"));
}

/// The rows on show: the store row names of the catalogue's placed props, in placing order.
fn page(s: &Session) -> (Vec<String>, CatalogueDef) {
    let state = &s.sim.state;
    let cat = state.entities.iter().find_map(|e| match &e.behavior {
        Behavior::Catalogue(cat) => Some(cat.clone()),
        _ => None,
    });
    let cat = cat.expect("the catalogue object");
    let names = cat
        .spawned
        .iter()
        .filter_map(|id| state.entities.get(*id))
        .filter(|e| e.prop.is_some())
        .map(|e| e.prop.as_ref().unwrap().asset.clone())
        .collect();
    (names, cat)
}

#[test]
fn catalogue_pages_through_every_store_row() {
    props_root();
    let rows = catalogue::rows();
    let names: Vec<String> = rows.iter().map(|row| row.name.clone()).collect();
    assert!(names.iter().any(|n| n.starts_with("LIB/kenney_mini_dungeon/")), "the committed packs are read");
    assert!(names.iter().all(|n| !pav_core::store::local_path(n)), "never local/");
    let mut s = Session::new("catalogue", 1).unwrap();
    assert!(page(&s).0.is_empty(), "nothing until a pad");
    let slot = s.sim.state.world.rooms[0].clone();
    let pages = names.len().div_ceil(20);
    let mut seen = Vec::new();
    for p in 0..pages {
        json_of(&mut s, "signal", json!({"name": "next"}));
        let (shown, cat) = page(&s);
        assert_eq!(cat.page, p as i32);
        assert_eq!(shown, names[p * 20..(p * 20 + 20).min(names.len())], "page {}", p + 1);
        // Each prop has its figure and its sign, and the page has a header.
        assert_eq!(cat.spawned.len(), shown.len() * 3, "page {}: a prop and a figure's two parts each", p + 1);
        assert_eq!(cat.signs.len(), shown.len() + 1);
        assert!(cat.signs[0].text.starts_with(&format!("PAGE {} / {pages}", p + 1)));
        let chunk = &s.sim.state.statics.chunks[&pav_core::statics::RegionKey::Room(slot.id)];
        assert!(cat.signs.iter().all(|sign| chunk.labels.contains(sign)), "the signs are up");
        // All of it stands inside the room.
        for e in cat.spawned.iter().filter_map(|id| s.sim.state.entities.get(*id)) {
            assert!(slot.contains(e.pos), "page {}: {} at {} is outside the room", p + 1, e.name, e.pos);
        }
        seen.extend(shown);
    }
    assert_eq!(seen, names, "every row once, in order");
    // The imported figures (S3b) stand on a page in their display pose, as props do.
    let figures = seen.iter().filter(|n| n.starts_with("LIB/kaykit_adventurers/")).count();
    assert!(figures >= 3, "{figures} figures in the Catalogue");
    // Paging wraps round; BACK from the first page shows the last.
    json_of(&mut s, "signal", json!({"name": "next"}));
    assert_eq!(page(&s).1.page, 0);
    json_of(&mut s, "signal", json!({"name": "back"}));
    assert_eq!(page(&s).1.page, pages as i32 - 1);
    // Rewind and snapshots keep a page: its props travel with the state.
    json_of(&mut s, "signal", json!({"name": "back"}));
    let before = page(&s).0;
    s.step(30);
    let tick = s.sim.state.tick;
    let path = std::env::temp_dir().join(format!("pav_catalogue_{}.json", std::process::id()));
    json_of(&mut s, "snapshot_save", json!({"path": path.to_str().unwrap()}));
    json_of(&mut s, "signal", json!({"name": "next"}));
    s.step(30);
    assert_ne!(page(&s).0, before);
    json_of(&mut s, "rewind", json!({"tick": tick}));
    assert_eq!(page(&s).0, before, "rewind restores the page");
    json_of(&mut s, "signal", json!({"name": "next"}));
    json_of(&mut s, "snapshot_load", json!({"path": path.to_str().unwrap()}));
    assert_eq!(page(&s).0, before, "a snapshot restores the page");
    let _ = std::fs::remove_file(&path);
}

/// Kenney Mini Dungeon through every step of the store (§13 S2): its ledger, the licence gate,
/// its rows in the index and STORE.tsv, `assets find=`, the Catalogue's pages and (once one is
/// published) a fork at standard in the Hall. Prints what it finds.
#[test]
fn kenney_mini_dungeon_through_every_step() {
    props_root();
    let root = repo();
    let ledger = std::fs::read_to_string(root.join("assets/catalogs/kenney/mini_dungeon.tsv")).expect("the ledger");
    let ledger_rows = ledger.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).count() - 1;
    let pack: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("assets/props/lib/kenney_mini_dungeon/PACK.json")).unwrap())
            .unwrap();
    let licence = pav_core::store::admitted(pack["license"].as_str().unwrap(), "prop").expect("the licence gate admits it");
    let index_text = std::fs::read_to_string(root.join("assets/props/lib/kenney_mini_dungeon/index.tsv")).unwrap();
    let rows = index::parse(&index_text, props::Tier::Disk).unwrap();
    let checked = rows.iter().filter(|r| r.level == "checked").count();
    let store = std::fs::read_to_string(root.join("store/STORE.tsv")).unwrap();
    let in_store = store.lines().filter(|l| l.starts_with("LIB/kenney_mini_dungeon/")).count();
    assert_eq!(in_store, rows.len(), "every index row is a STORE.tsv row");
    let mut s = Session::new("empty", 1).unwrap();
    let found = json_of(&mut s, "assets", json!({"find": "dungeon chest"}));
    let hit = found.to_string().contains("LIB/kenney_mini_dungeon/chest");
    assert!(hit, "assets find=\"dungeon chest\" finds the Kenney chest: {found}");
    let names: Vec<String> = catalogue::rows().into_iter().map(|r| r.name).collect();
    let pages: BTreeSet<usize> =
        names.iter().enumerate().filter(|(_, n)| n.starts_with("LIB/kenney_mini_dungeon/")).map(|(i, _)| i / 20 + 1).collect();
    assert_eq!(names.iter().filter(|n| n.starts_with("LIB/kenney_mini_dungeon/")).count(), rows.len());
    // A fork of a Kenney row published to LIB/core shows in the Hall.
    let forks: Vec<&str> = index::embedded()
        .iter()
        .filter(|row| {
            props::resolve_ref(&row.name)
                .ok()
                .and_then(|def| def.doc.lineage.clone())
                .is_some_and(|lineage| lineage.parents.iter().any(|p| p.starts_with("LIB/kenney_mini_dungeon/")))
        })
        .map(|row| row.name.as_str())
        .collect();
    let hall = room(hall::KEY);
    for fork in &forks {
        assert!(hall.objects.iter().any(|o| o.asset.as_deref() == Some(fork)), "{fork} is at standard but not in the Hall");
    }
    println!(
        "Kenney Mini Dungeon: ledger {ledger_rows} rows; licence {} admitted; index {} rows ({checked} checked, {} disk); \
         STORE.tsv {in_store} rows; find \"dungeon chest\" hits LIB/kenney_mini_dungeon/chest; catalogue pages {pages:?}; \
         forks at standard in the Hall: {forks:?}",
        licence.id,
        rows.len(),
        rows.len() - checked
    );
}

#[test]
fn store_showcase_shows_every_core_material() {
    let def = room("store_showcase");
    let shown: BTreeSet<String> = def
        .objects
        .iter()
        .flat_map(|o| &o.patch)
        .filter(|op| op["op"] == "add")
        .filter_map(|op| op["value"]["base"].as_str().map(str::to_owned))
        .collect();
    let library = pav_core::material::Library::core();
    for id in library.names() {
        assert!(shown.contains(id), "store_showcase shows no cube of {id}: regenerate the room");
    }
    assert!(library.names().count() >= 18);
}

/// The cast's clips: (performer, its one clip) for every character named cast_*.
fn cast(s: &Session) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = s
        .sim
        .state
        .entities
        .iter()
        .filter(|e| e.name.starts_with("cast"))
        .map(|e| {
            let clips = e.ai.as_ref().and_then(|ai| ai.perform.as_ref()).map(|p| p.clips.clone()).unwrap_or_default();
            assert_eq!(clips.len(), 1, "{} performs {clips:?}", e.name);
            (e.name.clone(), clips[0])
        })
        .collect();
    out.sort();
    out
}

#[test]
fn clip_browser_plays_one_clip_on_its_whole_cast() {
    let def = room("clip_browser");
    let figures: Vec<&str> = def.npcs.iter().filter_map(|n| n.look.get("figure").and_then(|f| f.as_str())).collect();
    assert_eq!(figures.len(), 5, "the five KayKit figures perform");
    for figure in &figures {
        assert!(props::store::shipped(figure), "{figure} ships in every build");
        pav_core::figure::get(figure).unwrap_or_else(|e| panic!("{e}"));
    }
    let order = pav_core::clip_browser::ids();
    let mut s = Session::new("clip_browser", 1).unwrap();
    s.step(2);
    let first = cast(&s);
    assert_eq!(first.len(), 6);
    let start = pav_core::clips::find("QUATERNIUS/Idle_Loop").unwrap();
    assert!(first.iter().all(|(_, id)| *id == start), "the cast starts on the authored clip: {first:?}");
    // NEXT: the whole cast moves on to the next clip in the library's order, together.
    json_of(&mut s, "signal", json!({"name": "clip_next"}));
    s.step(2);
    let at = order.iter().position(|&id| id == start).unwrap();
    assert!(cast(&s).iter().all(|(_, id)| *id == order[(at + 1) % order.len()]));
    // A set pad jumps to that set's first clip.
    json_of(&mut s, "signal", json!({"name": "clip_set_cmu"}));
    s.step(2);
    let lib = pav_core::clips::library();
    assert!(cast(&s).iter().all(|(_, id)| lib.name_of(*id).unwrap() == "CMU/Backflip"), "{:?}", cast(&s));
    // BACK from the first clip wraps to the last.
    json_of(&mut s, "signal", json!({"name": "clip_back"}));
    s.step(2);
    assert!(cast(&s).iter().all(|(_, id)| *id == order[order.len() - 1]));
}
