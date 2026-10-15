//! Imported figures (docs/AGENT_ASSET_STORE.md S3b, `pav_core::figure`): the KayKit
//! adventurers committed in `assets/props/lib/kaykit_adventurers` check clean as props and as
//! figures, every embedded clip poses them finitely with their bones whole and their bodies on
//! or above the floor, they follow the puppet's sides, and the store finds them.

use std::path::{Path, PathBuf};
use std::sync::Once;

use pav_core::figure::{self, Seg};
use pav_core::puppet::PuppetDef;
use pav_tools::{Output, Session, call};
use serde_json::{Value, json};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Figures and their meshes are read from the repository's props root.
fn props_root() {
    static SET: Once = Once::new();
    SET.call_once(|| {
        // SAFETY: set once, before any test of this file reads it.
        unsafe { std::env::set_var("PAV_ASSETS", repo().join("assets/props")) };
    });
}

const FIGURES: [&str; 5] = ["knight", "barbarian", "mage", "rogue", "rogue_hooded"];

fn name(leaf: &str) -> String {
    format!("LIB/kaykit_adventurers/{leaf}")
}

fn json_of(s: &mut Session, tool: &str, args: Value) -> Value {
    match call(s, tool, args.as_object().unwrap()) {
        Ok(Output::Json(v)) => v,
        Ok(Output::Image { .. }) => panic!("{tool}: an image"),
        Err(e) => panic!("{tool}: {e}"),
    }
}

#[test]
fn the_adventurers_check_clean_as_props_and_as_figures() {
    props_root();
    let mut s = Session::new("empty", 1).unwrap();
    for leaf in FIGURES {
        let reply = json_of(&mut s, "asset_edit", json!({"action": "check", "name": name(leaf)}));
        assert!(reply.get("check").is_none(), "{leaf}: {reply}");
        assert_eq!(reply["figure"]["segments"], 16, "{leaf}: {reply}");
        let figure = figure::get(&name(leaf)).unwrap();
        assert_eq!(figure.segments().len(), 16, "{leaf}");
        assert_eq!(figure.rig.table, "kaykit");
        // The held props stay out; the body's own attachments stay in.
        assert!(
            figure
                .rig
                .left_out
                .iter()
                .any(|m| m.contains("Sword") || m.contains("Wand") || m.contains("Knife") || m.contains("Axe")),
            "{leaf}"
        );
    }
    let knight = figure::get(&name("knight")).unwrap();
    assert!(knight.rig.folded.iter().any(|f| f == "Knight_Helmet > head"), "{:?}", knight.rig.folded);
}

#[test]
fn every_embedded_clip_poses_three_figures_whole_and_finite() {
    props_root();
    let lib = pav_core::clips::library();
    let clips: Vec<(u32, f32, String)> = lib
        .sets
        .iter()
        .flat_map(|set| {
            set.clips
                .iter()
                .map(move |(clip, c)| (pav_core::clips::clip_id(&set.set, clip), c.dur, format!("{}/{clip}", set.set)))
        })
        .collect();
    assert!(clips.len() > 400, "{} embedded clips", clips.len());
    for leaf in ["knight", "barbarian", "mage"] {
        let figure = figure::get(&name(leaf)).unwrap();
        let def = figure.fit(&PuppetDef::default());
        let rig = &figure.rig;
        let length = |a: Seg, b: Seg| {
            let (ja, jb) = (rig.joint(a).unwrap().0, rig.joint(b).unwrap().0);
            (jb - ja).length()
        };
        let bones = [
            (Seg::UpperArmL, Seg::LowerArmL),
            (Seg::LowerArmL, Seg::HandL),
            (Seg::UpperArmR, Seg::LowerArmR),
            (Seg::LowerArmR, Seg::HandR),
            (Seg::UpperLegL, Seg::LowerLegL),
            (Seg::LowerLegL, Seg::FootL),
            (Seg::UpperLegR, Seg::LowerLegR),
            (Seg::LowerLegR, Seg::FootR),
        ];
        let mut poses = 0;
        for (id, dur, clip) in &clips {
            for flags in [0, pav_core::clips::MIRROR] {
                for k in 0..=6 {
                    let t = dur * k as f32 / 6.0;
                    let sk = pav_core::clips::pose_of(&def, *id, t, flags).unwrap();
                    let posed = figure.pose(&def, &sk);
                    assert_eq!(posed.len(), 16, "{leaf} {clip}");
                    let at = |s: Seg| posed.iter().find(|p| p.seg == s).unwrap();
                    for p in &posed {
                        assert!(p.pos.is_finite() && p.rot.is_finite(), "{leaf} {clip} @{t}: {:?}", p.seg);
                        assert!((p.rot.length() - 1.0).abs() < 1e-3, "{leaf} {clip}: {:?} is not a rotation", p.seg);
                        assert!((p.pos - at(Seg::Hips).pos).length() < 3.0, "{leaf} {clip}: {:?} flew off", p.seg);
                    }
                    // Bones keep their lengths: a limb is whole in every pose.
                    for (a, b) in bones {
                        let (have, want) = ((at(b).pos - at(a).pos).length(), length(a, b));
                        assert!((have - want).abs() < 0.002, "{leaf} {clip} @{t}: {a:?}-{b:?} {have} vs {want}");
                    }
                    // The body stands with its lowest point where the puppet's is: never below
                    // the floor.
                    let low = posed
                        .iter()
                        .flat_map(|p| figure.extremes(p.seg).iter().map(move |v| (p.pos + p.rot * *v).y))
                        .fold(f32::INFINITY, f32::min);
                    assert!(low > -0.005, "{leaf} {clip} @{t}: {low}");
                    poses += 1;
                }
            }
        }
        assert!(poses > 5_000, "{leaf}: {poses} poses");
    }
}

#[test]
fn a_figure_follows_the_puppet_side_its_limbs_sit_on() {
    props_root();
    let figure = figure::get(&name("knight")).unwrap();
    let def = figure.fit(&PuppetDef::default());
    let id = pav_core::clips::find("CMU/Wave_Hello").unwrap();
    let dur = pav_core::clips::with(id, |c| c.dur).unwrap();
    // The moment the waving hand is highest.
    let (t, sk) = (0..40)
        .map(|k| dur * k as f32 / 39.0)
        .map(|t| (t, pav_core::clips::pose_of(&def, id, t, 0).unwrap()))
        .max_by(|a, b| a.1.hand[0].y.max(a.1.hand[1].y).total_cmp(&b.1.hand[0].y.max(b.1.hand[1].y)))
        .unwrap();
    let up = if sk.hand[1].y > sk.hand[0].y { 1 } else { 0 };
    let posed = figure.pose(&def, &sk);
    let hand = |s: Seg| posed.iter().find(|p| p.seg == s).unwrap().pos;
    // The Knight's left arm sits at +x (a glTF character faces +z), the puppet's index 1.
    assert!(figure.rig.joint(Seg::UpperArmL).unwrap().0.x > 0.0);
    let (raised, lowered) = if up == 1 { (Seg::HandL, Seg::HandR) } else { (Seg::HandR, Seg::HandL) };
    assert!(hand(raised).y > hand(lowered).y + 0.1, "@{t}: {} vs {}", hand(raised), hand(lowered));
    assert_eq!(hand(raised).x > 0.0, sk.hand[up].x > 0.0, "the raised hand is on the puppet's side");
    // Rest pose and display pose agree on the body's height (soles on the floor).
    let shown = figure.pose(&def, &pav_core::clips::skel(&def, &figure::display_key(), false));
    let low = shown
        .iter()
        .flat_map(|p| figure.extremes(p.seg).iter().map(move |v| (p.pos + p.rot * *v).y))
        .fold(f32::INFINITY, f32::min);
    assert!(low.abs() < 0.01, "{low}");
}

#[test]
fn a_planted_defect_is_caught() {
    props_root();
    let figure = figure::get(&name("mage")).unwrap();
    let mut rig = figure.rig.clone();
    // A segment moved half a metre off its bone, a required bone unmapped.
    rig.segments.get_mut("lower_arm_l").unwrap().joint[1] += 50.0;
    rig.segments.remove("foot_r");
    let broken = figure::Figure::new(&figure.name, rig, figure.body.clone());
    // The body still names foot_r, whose segment is gone: refused outright.
    assert!(broken.is_err());
    let mut rig = figure.rig.clone();
    rig.segments.get_mut("lower_arm_l").unwrap().joint[1] += 50.0;
    rig.look.leg_length = 3.0;
    let shifted = figure::Figure::new(&figure.name, rig, figure.body.clone()).unwrap();
    let issues = figure::check(&shifted);
    let paths: Vec<&str> = issues.iter().map(|i| i.path.as_str()).collect();
    assert!(paths.contains(&"segments.lower_arm_l.joint"), "{issues:?}");
    assert!(paths.contains(&"look.leg_length"), "{issues:?}");
}

#[test]
fn the_store_finds_figures_by_their_kind() {
    props_root();
    let mut s = Session::new("empty", 1).unwrap();
    let found = json_of(&mut s, "assets", json!({"find": "knight"}));
    assert_eq!(found["rows"][0][0], name("knight"), "{found}");
    assert_eq!(found["rows"][0][1], "character", "{found}");
    let all = json_of(&mut s, "assets", json!({"find": "character"}));
    assert!(all["matched"].as_u64().unwrap() >= 5, "{all}");
    let index = std::fs::read_to_string(repo().join("assets/props/lib/kaykit_adventurers/index.tsv")).unwrap();
    let rows = pav_core::props::index::parse(&index, pav_core::props::Tier::Disk).unwrap();
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|r| r.kind == "character" && r.licence == "CC0-1.0" && r.cat == "character"), "{rows:?}");
    let store = std::fs::read_to_string(repo().join("store/STORE.tsv")).unwrap();
    assert!(
        store.contains("\nLIB/kaykit_adventurers/knight\tcharacter\tdisk\tCC0-1.0\timported\timport\t"),
        "STORE.tsv lists the figures"
    );
}
