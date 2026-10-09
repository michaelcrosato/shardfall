//! The motion importer against my-3D2dge's own (tools/anim-import.mjs): a small glTF rig and a
//! CMU take, translated by both, agree number for number (the expected sets in
//! tests/fixtures/mocap were written by that tool). A BVH take cuts into a loop that plays.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use glam::DVec3;
use pav_core::clips::ClipSet;
use pav_tools::mocap::bvh::{Bvh, BvhTake, Rigged};
use pav_tools::mocap::takes::{self, Find, Pick, Take, Way};
use pav_tools::mocap::{self, Catalog, Ledger, Options};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mocap")
}

/// A set as its file reads back.
fn written(set: &ClipSet) -> ClipSet {
    ClipSet::parse(&set.to_text()).expect("the set reads back")
}

fn same(ours: &ClipSet, want: &ClipSet) {
    assert_eq!(ours.clips.keys().collect::<Vec<_>>(), want.clips.keys().collect::<Vec<_>>());
    for (name, w) in &want.clips {
        let o = &ours.clips[name];
        assert_eq!((o.dur, o.looping, &o.take, &o.tags, &o.desc), (w.dur, w.looping, &w.take, &w.tags, &w.desc), "{name}");
        assert_eq!(o.keys.len(), w.keys.len(), "{name}: key poses");
        for (i, (a, b)) in o.keys.iter().zip(&w.keys).enumerate() {
            assert_eq!(a, b, "{name}: key {i}");
        }
    }
    assert_eq!(ours.fit, want.fit, "each clip's fit");
    for (id, w) in &want.sources {
        assert_eq!(ours.sources[id]["rest"], w["rest"], "{id}: the body it was measured on");
    }
}

fn expected(file: &str) -> ClipSet {
    ClipSet::parse(&std::fs::read_to_string(fixtures().join(file)).unwrap()).unwrap()
}

/// A tiny glTF binary: an Unreal-style humanoid rig (metres, y up, facing +z) and three clips: a
/// T-pose, a wave (the right arm swings up and back, the elbow bends, the chest turns, the hips
/// bob, the left leg steps) and a step forward (the root travels). Built here rather than kept as
/// a binary file; my-3D2dge's importer read the same rig for rig_expected.json.
fn rig_glb() -> Vec<u8> {
    use serde_json::{Value, json};
    let mut names: Vec<String> = Vec::new();
    let mut trans: Vec<[f64; 3]> = Vec::new();
    let mut kids: Vec<Vec<usize>> = Vec::new();
    let mut node = |name: &str, t: [f64; 3], parent: Option<usize>| -> usize {
        names.push(name.into());
        trans.push(t);
        kids.push(Vec::new());
        let i = names.len() - 1;
        if let Some(p) = parent {
            kids[p].push(i);
        }
        i
    };
    let root = node("root", [0.0, 0.0, 0.0], None);
    let pelvis = node("pelvis", [0.0, 0.95, 0.0], Some(root));
    let s1 = node("spine_01", [0.0, 0.1, 0.0], Some(pelvis));
    let s2 = node("spine_02", [0.0, 0.12, 0.0], Some(s1));
    let s3 = node("spine_03", [0.0, 0.12, 0.0], Some(s2));
    let neck = node("neck_01", [0.0, 0.16, 0.0], Some(s3));
    let head = node("Head", [0.0, 0.1, 0.0], Some(neck));
    node("head_leaf", [0.0, 0.18, 0.0], Some(head));
    for (s, x) in [("l", 1.0), ("r", -1.0)] {
        let clav = node(&format!("clavicle_{s}"), [0.03 * x, 0.12, 0.0], Some(s3));
        let up = node(&format!("upperarm_{s}"), [0.15 * x, 0.0, 0.0], Some(clav));
        let lo = node(&format!("lowerarm_{s}"), [0.28 * x, 0.0, 0.0], Some(up));
        let hand = node(&format!("hand_{s}"), [0.25 * x, 0.0, 0.0], Some(lo));
        node(&format!("index_01_{s}"), [0.09 * x, 0.0, 0.03], Some(hand));
        let m1 = node(&format!("middle_01_{s}"), [0.09 * x, 0.0, 0.0], Some(hand));
        node(&format!("middle_02_{s}"), [0.045 * x, 0.0, 0.0], Some(m1));
        node(&format!("pinky_01_{s}"), [0.08 * x, 0.0, -0.03], Some(hand));
        let th = node(&format!("thigh_{s}"), [0.1 * x, -0.02, 0.0], Some(pelvis));
        let calf = node(&format!("calf_{s}"), [0.0, -0.44, 0.0], Some(th));
        let foot = node(&format!("foot_{s}"), [0.0, -0.43, 0.0], Some(calf));
        let ball = node(&format!("ball_{s}"), [0.0, -0.06, 0.13], Some(foot));
        node(&format!("ball_leaf_{s}"), [0.0, 0.0, 0.06], Some(ball));
    }
    let index = |n: &str| names.iter().position(|x| x == n).unwrap();
    let quat = |axis: [f64; 3], deg: f64| -> Vec<f64> {
        let a = deg.to_radians() / 2.0;
        vec![axis[0] * a.sin(), axis[1] * a.sin(), axis[2] * a.sin(), a.cos()]
    };
    let mut bin: Vec<u8> = Vec::new();
    let (mut accessors, mut views) = (Vec::<Value>::new(), Vec::<Value>::new());
    let mut add = |values: &[f64], kind: &str, count: usize| -> usize {
        let off = bin.len();
        for v in values {
            bin.extend_from_slice(&(*v as f32).to_le_bytes());
        }
        views.push(json!({ "buffer": 0, "byteOffset": off, "byteLength": bin.len() - off }));
        accessors.push(json!({ "bufferView": views.len() - 1, "componentType": 5126, "count": count, "type": kind }));
        accessors.len() - 1
    };
    let mut anims = Vec::new();
    let mut anim = |name: &str, tracks: Vec<(&str, &str, Vec<f64>, Vec<Vec<f64>>)>| {
        let (mut ch, mut sa) = (Vec::new(), Vec::new());
        for (target, path, times, values) in tracks {
            let t = add(&times, "SCALAR", times.len());
            let kind = if path == "rotation" { "VEC4" } else { "VEC3" };
            let v = add(&values.concat(), kind, values.len());
            sa.push(json!({ "input": t, "output": v, "interpolation": "LINEAR" }));
            ch.push(json!({ "sampler": sa.len() - 1, "target": { "node": index(target), "path": path } }));
        }
        anims.push(json!({ "name": name, "channels": ch, "samplers": sa }));
    };
    anim("A_TPose", vec![("pelvis", "rotation", vec![0.0], vec![vec![0.0, 0.0, 0.0, 1.0]])]);
    let times = vec![0.0, 0.25, 0.5, 0.75, 1.0];
    let turn = |axis: [f64; 3], d: [f64; 5]| d.iter().map(|d| quat(axis, *d)).collect::<Vec<_>>();
    anim(
        "Wave_Loop",
        vec![
            ("upperarm_r", "rotation", times.clone(), turn([0.0, 0.0, 1.0], [0.0, -60.0, -100.0, -60.0, 0.0])),
            ("lowerarm_r", "rotation", times.clone(), turn([0.0, 1.0, 0.0], [0.0, 40.0, 80.0, 40.0, 0.0])),
            ("spine_02", "rotation", times.clone(), turn([0.0, 1.0, 0.0], [0.0, 10.0, 20.0, 10.0, 0.0])),
            (
                "pelvis",
                "translation",
                times.clone(),
                [0.0, -0.03, 0.0, -0.03, 0.0].iter().map(|d| vec![0.0, 0.95 + d, 0.0]).collect(),
            ),
            ("thigh_l", "rotation", times.clone(), turn([1.0, 0.0, 0.0], [0.0, -20.0, -35.0, -20.0, 0.0])),
            ("calf_l", "rotation", times.clone(), turn([1.0, 0.0, 0.0], [0.0, 30.0, 60.0, 30.0, 0.0])),
        ],
    );
    anim("Step_RM", vec![("root", "translation", vec![0.0, 1.0], vec![vec![0.0, 0.0, 0.0], vec![0.0, 0.0, 0.6]])]);
    let nodes: Vec<Value> = (0..names.len())
        .map(|i| {
            let mut n = json!({ "name": names[i], "translation": trans[i] });
            if !kids[i].is_empty() {
                n["children"] = json!(kids[i]);
            }
            n
        })
        .collect();
    let gltf = json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": [root] }],
        "nodes": nodes,
        "animations": anims,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{ "byteLength": bin.len() }],
    });
    let mut js = serde_json::to_vec(&gltf).unwrap();
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let mut out = Vec::new();
    for v in [0x4654_6c67u32, 2, (12 + 8 + js.len() + 8 + bin.len()) as u32, js.len() as u32, 0x4e4f_534a] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&js);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004e_4942u32.to_le_bytes());
    out.extend_from_slice(&bin);
    out
}

#[test]
fn a_gltf_rig_translates_like_the_reference() {
    let dir = fixtures();
    let cat = Catalog::read(&dir.join("rig_catalog.json")).unwrap();
    let glb = std::env::temp_dir().join(format!("pav_rig_{}.glb", std::process::id()));
    std::fs::write(&glb, rig_glb()).unwrap();
    let mut log = Vec::new();
    let libs = mocap::glb_libraries(std::slice::from_ref(&glb), &["RIG".into()], &[], &[], &cat, 30.0, &mut log);
    let _ = std::fs::remove_file(&glb);
    let libs = libs.unwrap();
    assert_eq!(libs[0].rig, "unreal");
    let (set, done) = mocap::assemble(libs, &cat, &Options { set: "RIG".into(), ..Options::default() }, &mut log);
    assert_eq!(done.len(), 3, "{log:?}");
    same(&written(&set), &expected("rig_expected.json"));
    // The step travels; the wave stays put.
    let set = written(&set);
    assert!(set.clips["Step_RM"].travels() && !set.clips["Wave_Loop"].travels());
}

#[test]
fn a_cmu_take_cuts_like_the_reference() {
    let dir = fixtures();
    let cat = Catalog::read(&dir.join("cmu_catalog.json")).unwrap();
    let ledger = Ledger::default();
    let picks: Vec<Pick> = cat.pick.iter().map(|(n, v)| mocap::pick_of(n, v, &cat, &ledger).unwrap()).collect();
    let mut log = Vec::new();
    // The take and its skeleton are in the fixtures: nothing is downloaded.
    let libs = mocap::cmu_libraries(&picks, &cat, &ledger, &dir.join("cmu"), 30.0, &mut log).unwrap();
    assert!(log.iter().any(|l| l.contains("a 0.75 s cycle at 0.20-0.95 s of 09_02 (its seam is off by 33 mm)")), "{log:?}");
    let (set, _) = mocap::assemble(libs, &cat, &Options { set: "CMUTEST".into(), ..Options::default() }, &mut log);
    same(&written(&set), &expected("cmu_expected.json"));
}

/// The whole-database survey on one take: its ledger row and its clip in the subject's set
/// against what my-3D2dge's survey wrote (the committed anim/cmu ledger and CMU_09 set). The
/// fixtures hold one of the subject's twelve takes, so the floor (from all of them) and the body
/// measured standing on it differ a little: what depends on them is close, the rest the same.
/// (Run over the whole database, `mocap survey`'s ledger and sets equal the committed ones.)
#[test]
fn the_cmu_survey_measures_and_translates_like_the_reference() {
    let anim = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../anim/cmu");
    let ledger = Ledger::read(&anim.join("takes.tsv"));
    let mut log = Vec::new();
    let s = mocap::cmu::survey(&fixtures().join("cmu"), &ledger, &HashMap::new(), Some(&[9]), true, 30.0, Some(50.0), &mut log)
        .unwrap();
    assert_eq!((s.takes, s.sets.len()), (1, 1), "{log:?}");
    let row = &s.rows["09_02"];
    for col in ["sec", "category", "active", "travel", "flags"] {
        assert_eq!(row[col].as_str(), ledger.get("09_02", col), "{col}");
    }
    let nums = |v: Option<&str>| -> Vec<f64> { v.unwrap().split(['/', '-']).map(|x| x.parse().unwrap()).collect() };
    for (col, within) in [("fit", 3.0), ("hips", 2.0)] {
        let (ours, theirs) = (nums(row[col].as_str()), nums(ledger.get("09_02", col)));
        assert!(ours.iter().zip(&theirs).all(|(a, b)| (a - b).abs() <= within), "{col}: {ours:?} against {theirs:?}");
    }
    let want = ClipSet::parse(&std::fs::read_to_string(anim.join("cmu_09.json")).unwrap()).unwrap();
    let set = written(&s.sets[0]);
    let (ours, theirs) = (&set.clips["09_02"], &want.clips["09_02"]);
    assert_eq!((&ours.take, &ours.tags, &ours.desc, ours.dur), (&theirs.take, &theirs.tags, &theirs.desc, theirs.dur));
    assert!(ours.keys.len().abs_diff(theirs.keys.len()) <= 2, "{} key poses against {}", ours.keys.len(), theirs.keys.len());
    assert!(set.fit["09_02"][0] <= want.fit["09_02"][0] + 3.0, "{:?} against {:?}", set.fit["09_02"], want.fit["09_02"]);
}

/// A walker in the MotionBuilder skeleton: legs swinging once a second, the hips bobbing
/// twice, walking forward 1.2 m a second for 3 s, then turning on the spot.
fn walker() -> String {
    mover(5, |t| if t < 3.0 { ([0.0, 120.0 * t], 0.0, true) } else { ([0.0, 360.0], 90.0 * (t - 3.0), false) })
}

/// A sidestepper: facing forward throughout, stepping 0.8 m a second to its left (+x) for 3 s,
/// then back to its right.
fn sidestepper() -> String {
    mover(6, |t| ([80.0 * if t < 3.0 { t } else { 6.0 - t }, 0.0], 0.0, true))
}

/// The skeleton and a motion: at each second `t`, where the root is on the floor (x, z cm), its
/// yaw (degrees) and whether the legs swing.
fn mover(secs: usize, at: impl Fn(f64) -> ([f64; 2], f64, bool)) -> String {
    let joints = [
        ("Hips", None, [0.0, 0.0, 0.0]),
        ("Spine", Some(0), [0.0, 10.0, 0.0]),
        ("Spine1", Some(1), [0.0, 10.0, 0.0]),
        ("Spine2", Some(2), [0.0, 10.0, 0.0]),
        ("Neck", Some(3), [0.0, 15.0, 0.0]),
        ("Head", Some(4), [0.0, 10.0, 0.0]),
        ("LeftShoulder", Some(3), [3.0, 12.0, 0.0]),
        ("LeftArm", Some(6), [15.0, 0.0, 0.0]),
        ("LeftForeArm", Some(7), [28.0, 0.0, 0.0]),
        ("LeftHand", Some(8), [25.0, 0.0, 0.0]),
        ("RightShoulder", Some(3), [-3.0, 12.0, 0.0]),
        ("RightArm", Some(10), [-15.0, 0.0, 0.0]),
        ("RightForeArm", Some(11), [-28.0, 0.0, 0.0]),
        ("RightHand", Some(12), [-25.0, 0.0, 0.0]),
        ("LeftUpLeg", Some(0), [10.0, 0.0, 0.0]),
        ("LeftLeg", Some(14), [0.0, -44.0, 0.0]),
        ("LeftFoot", Some(15), [0.0, -43.0, 0.0]),
        ("LeftToeBase", Some(16), [0.0, -8.0, 14.0]),
        ("RightUpLeg", Some(0), [-10.0, 0.0, 0.0]),
        ("RightLeg", Some(18), [0.0, -44.0, 0.0]),
        ("RightFoot", Some(19), [0.0, -43.0, 0.0]),
        ("RightToeBase", Some(20), [0.0, -8.0, 14.0]),
    ];
    let ends =
        [(5, [0.0, 18.0, 0.0]), (9, [18.0, 0.0, 0.0]), (13, [-18.0, 0.0, 0.0]), (17, [0.0, 0.0, 6.0]), (21, [0.0, 0.0, 6.0])];
    fn write(out: &mut String, j: usize, depth: usize, joints: &[(&str, Option<usize>, [f64; 3])], ends: &[(usize, [f64; 3])]) {
        let pad = " ".repeat(depth);
        let (name, parent, o) = joints[j];
        let chans = if parent.is_none() {
            "6 Xposition Yposition Zposition Zrotation Xrotation Yrotation"
        } else {
            "3 Zrotation Xrotation Yrotation"
        };
        out.push_str(&format!(
            "{pad}{} {name}\n{pad}{{\n{pad} OFFSET {} {} {}\n{pad} CHANNELS {chans}\n",
            if parent.is_none() { "ROOT" } else { "JOINT" },
            o[0],
            o[1],
            o[2]
        ));
        for c in (0..joints.len()).filter(|&c| joints[c].1 == Some(j)) {
            write(out, c, depth + 1, joints, ends);
        }
        if let Some((_, e)) = ends.iter().find(|(k, _)| *k == j) {
            out.push_str(&format!("{pad} End Site\n{pad} {{\n{pad}  OFFSET {} {} {}\n{pad} }}\n", e[0], e[1], e[2]));
        }
        out.push_str(&format!("{pad}}}\n"));
    }
    let mut text = String::from("HIERARCHY\n");
    write(&mut text, 0, 0, &joints, &ends);
    let fps = 60;
    text += &format!("MOTION\nFrames: {}\nFrame Time: {}\n", fps * secs, 1.0 / fps as f64);
    for f in 0..fps * secs {
        let t = f as f64 / fps as f64;
        let ph = std::f64::consts::TAU * t;
        let ([x, z], yaw, walking) = at(t);
        let mut v = vec![0.0; 6 + 3 * (joints.len() - 1)];
        v[0] = x;
        v[1] = 92.0 + if walking { 2.0 * (2.0 * ph).cos() } else { 0.0 };
        v[2] = z;
        // The yaw: the root's third rotation channel, Y.
        v[5] = yaw;
        if walking {
            // Thighs (X rotation: forward swing) and knees.
            let swing = 25.0 * ph.sin();
            v[6 + 3 * 13 + 1] = -swing; // LeftUpLeg
            v[6 + 3 * 17 + 1] = swing; // RightUpLeg
            v[6 + 3 * 14 + 1] = 30.0 * (0.5 - 0.5 * ph.cos()); // LeftLeg
            v[6 + 3 * 18 + 1] = 30.0 * (0.5 + 0.5 * ph.cos()); // RightLeg
            // Arms hang (Z rotation) and swing against the legs.
            v[6 + 3 * 6] = -75.0;
            v[6 + 3 * 10] = 75.0;
            v[6 + 3 * 6 + 1] = swing;
            v[6 + 3 * 10 + 1] = -swing;
        }
        text += &v.iter().map(|x| format!("{x:.4}")).collect::<Vec<_>>().join(" ");
        text.push('\n');
    }
    text
}

#[test]
fn a_bvh_walk_cuts_into_a_clean_loop() {
    let r = Rigged::new(Bvh::parse(&walker()).unwrap(), None).unwrap();
    assert_eq!((r.map, r.scale), ("motionbuilder", 0.01));
    let take = BvhTake(&r);
    let path: Vec<DVec3> = (0..take.frames()).step_by(6).map(|f| DVec3::from(take.points(f)[0])).collect();
    assert!(path.last().unwrap().z > 3.5, "walked forward: {}", path.last().unwrap());
    let body = takes::Body { rest: r.body.rest.clone(), fwd: r.body.fwd, right: r.body.right };
    let (a, b) = takes::find(&take, 60.0, Find::Still, None, &body).expect("a still stretch");
    assert!((a - 3.1).abs() < 0.15 && (b - 4.7).abs() < 0.15, "stands turning on the spot after three seconds: {a}-{b}");
    let pick = Pick {
        name: "Walk_Loop".into(),
        take: "walk".into(),
        from: Some(0.2),
        to: Some(2.9),
        min_cycle: None,
        fps: 60.0,
        looping: true,
        find: None,
    };
    let files = std::collections::HashMap::from([("walk".to_string(), {
        let p = std::env::temp_dir().join(format!("pav_walker_{}.bvh", std::process::id()));
        std::fs::write(&p, walker()).unwrap();
        p
    })]);
    let mut log = Vec::new();
    let lib = mocap::bvh_library("WALKER", &[pick], &files, None, None, 30.0, &mut log).unwrap();
    let _ = std::fs::remove_file(&files["walk"]);
    let cycle = log.iter().find(|l| l.contains("cycle")).expect("a cycle");
    assert!(cycle.contains("a 1.00 s cycle"), "{cycle}");
    let (set, done) =
        mocap::assemble(vec![lib], &Catalog::default(), &Options { set: "WALKER".into(), ..Options::default() }, &mut log);
    assert!(done[0].mean < 10.0, "fits within 1 cm on average: {}", done[0].mean);
    let set = written(&set);
    let clip = &set.clips["Walk_Loop"];
    assert!(clip.looping && !clip.travels(), "plays in place");
    // It plays on the puppet: limbs where they belong, feet on the floor, first and last pose
    // the same (the loop closes).
    let def = pav_core::puppet::PuppetDef::default();
    let a = pav_core::clips::skel(&def, &clip.key_at(0.0), false);
    let b = pav_core::clips::skel(&def, &clip.key_at(clip.dur - 1e-4), false);
    assert!((a.ankle[0] - b.ankle[0]).length() < 0.03 && (a.hand[1] - b.hand[1]).length() < 0.03, "the loop closes");
    let mid = pav_core::clips::skel(&def, &clip.key_at(clip.dur * 0.25), false);
    assert!(mid.ankle[0].y > -1e-3 && mid.ankle[1].y > -1e-3, "feet on or above the floor");
    assert!((mid.ankle[0].z - mid.ankle[1].z).abs() > 0.15, "a stride: the feet apart");
}

#[test]
fn a_sidestep_cuts_a_loop_each_way_facing_forward() {
    let r = Rigged::new(Bvh::parse(&sidestepper()).unwrap(), None).unwrap();
    let take = BvhTake(&r);
    let body = takes::Body { rest: r.body.rest.clone(), fwd: r.body.fwd, right: r.body.right };
    let all = Some((0.0, 6.0));
    let (a, b) = takes::find(&take, 60.0, Find::Going(Way::Left), all, &body).expect("a stretch to the left");
    assert!((a - 0.3).abs() < 0.15 && (b - 2.7).abs() < 0.15, "left for the first three seconds: {a}-{b}");
    let (a, b) = takes::find(&take, 60.0, Find::Going(Way::Right), all, &body).expect("a stretch to the right");
    assert!((a - 3.3).abs() < 0.15 && (b - 5.6).abs() < 0.15, "then right: {a}-{b}");
    assert_eq!(takes::find(&take, 60.0, Find::Going(Way::Forward), all, &body), None, "never forward");
    // Straight alone runs over the turn back (the hips stay on one line).
    let (a, b) = takes::find(&take, 60.0, Find::Straight, all, &body).unwrap();
    assert!(a < 2.0 && b > 4.0, "{a}-{b}");
    let pick = |name: &str, way| Pick {
        name: name.into(),
        take: "side".into(),
        from: Some(0.0),
        to: Some(6.0),
        min_cycle: Some(0.8),
        fps: 60.0,
        looping: true,
        find: Some(Find::Going(way)),
    };
    let takes: std::collections::HashMap<String, Box<dyn Take>> =
        [("side".to_string(), Box::new(BvhTake(&r)) as Box<dyn Take>)].into();
    let mut log = Vec::new();
    let caps =
        takes::cut(&body, &takes, &[pick("Side_Left", Way::Left), pick("Side_Right", Way::Right)], 30.0, &mut log).unwrap();
    for c in &caps[..2] {
        // Played in place, the body faces forward (the rig's frame: forward, right, up), and
        // the cycle carried it about 0.8 m a second.
        let (p, f) = (mocap::readable::PELVIS * 3, mocap::readable::PELVIS_F * 3);
        let (fwd, right) = (c.data[f] - c.data[p], c.data[f + 1] - c.data[p + 1]);
        assert!(fwd > 50.0 && right.abs() < 5.0, "{}: faces forward ({fwd}, {right})", c.name);
        let speed = c.stride.unwrap() / c.dur;
        assert!((speed - 800.0).abs() < 60.0, "{}: {speed} mm a second", c.name);
    }
}

/// A property of a node in the little FBX writer below.
enum FbxProp<'a> {
    S(&'a str),
    L(i64),
    I(i32),
    D(f64),
    Longs(Vec<i64>),
    Floats(Vec<f32>),
}

/// A node of a binary FBX file (version 7400), written at offset `at`: its end, property count
/// and length, name, properties, children (each written where it falls) and the empty record
/// that closes a node with children.
fn fbx_node(at: usize, name: &str, props: &[FbxProp], kids: &[&dyn Fn(usize) -> Vec<u8>]) -> Vec<u8> {
    let mut pb = Vec::new();
    for p in props {
        match p {
            FbxProp::S(v) => {
                pb.push(b'S');
                pb.extend((v.len() as u32).to_le_bytes());
                pb.extend(v.as_bytes());
            }
            FbxProp::L(v) => {
                pb.push(b'L');
                pb.extend(v.to_le_bytes());
            }
            FbxProp::I(v) => {
                pb.push(b'I');
                pb.extend(v.to_le_bytes());
            }
            FbxProp::D(v) => {
                pb.push(b'D');
                pb.extend(v.to_le_bytes());
            }
            FbxProp::Longs(v) => {
                pb.push(b'l');
                pb.extend([(v.len() as u32).to_le_bytes(), 0u32.to_le_bytes(), ((v.len() * 8) as u32).to_le_bytes()].concat());
                v.iter().for_each(|x| pb.extend(x.to_le_bytes()));
            }
            FbxProp::Floats(v) => {
                pb.push(b'f');
                pb.extend([(v.len() as u32).to_le_bytes(), 0u32.to_le_bytes(), ((v.len() * 4) as u32).to_le_bytes()].concat());
                v.iter().for_each(|x| pb.extend(x.to_le_bytes()));
            }
        }
    }
    let head = 13 + name.len() + pb.len();
    let mut body = Vec::new();
    for k in kids {
        body.extend(k(at + head + body.len()));
    }
    if !kids.is_empty() {
        body.extend([0u8; 13]);
    }
    let mut out = Vec::new();
    out.extend(((at + head + body.len()) as u32).to_le_bytes());
    out.extend((props.len() as u32).to_le_bytes());
    out.extend((pb.len() as u32).to_le_bytes());
    out.push(name.len() as u8);
    out.extend(name.as_bytes());
    out.extend(pb);
    out.extend(body);
    out
}

/// A Properties70 entry.
fn p70<'a>(name: &'a str, values: Vec<FbxProp<'a>>) -> impl Fn(usize) -> Vec<u8> + 'a {
    let values = std::rc::Rc::new(values);
    move |at| {
        let mut props = vec![FbxProp::S(name), FbxProp::S(""), FbxProp::S(""), FbxProp::S("A")];
        for v in values.iter() {
            props.push(match v {
                FbxProp::D(x) => FbxProp::D(*x),
                FbxProp::I(x) => FbxProp::I(*x),
                FbxProp::L(x) => FbxProp::L(*x),
                _ => FbxProp::S(""),
            });
        }
        fbx_node(at, "P", &props, &[])
    }
}

/// A binary FBX file as Blender writes one: z up, centimetres, the MotionBuilder skeleton of
/// `walker` standing (the hips 92 cm up), and one stack, `Armature|Wave`, in which the left
/// forearm turns 90 degrees about the vertical over a second.
fn fbx_file() -> Vec<u8> {
    let joints: [(&str, Option<usize>, [f64; 3]); 22] = [
        ("Hips", None, [0.0, 92.0, 0.0]),
        ("Spine", Some(0), [0.0, 10.0, 0.0]),
        ("Spine1", Some(1), [0.0, 10.0, 0.0]),
        ("Spine2", Some(2), [0.0, 10.0, 0.0]),
        ("Neck", Some(3), [0.0, 15.0, 0.0]),
        ("Head", Some(4), [0.0, 10.0, 0.0]),
        ("LeftShoulder", Some(3), [3.0, 12.0, 0.0]),
        ("LeftArm", Some(6), [15.0, 0.0, 0.0]),
        ("LeftForeArm", Some(7), [28.0, 0.0, 0.0]),
        ("LeftHand", Some(8), [25.0, 0.0, 0.0]),
        ("RightShoulder", Some(3), [-3.0, 12.0, 0.0]),
        ("RightArm", Some(10), [-15.0, 0.0, 0.0]),
        ("RightForeArm", Some(11), [-28.0, 0.0, 0.0]),
        ("RightHand", Some(12), [-25.0, 0.0, 0.0]),
        ("LeftUpLeg", Some(0), [10.0, 0.0, 0.0]),
        ("LeftLeg", Some(14), [0.0, -44.0, 0.0]),
        ("LeftFoot", Some(15), [0.0, -43.0, 0.0]),
        ("LeftToeBase", Some(16), [0.0, -8.0, 14.0]),
        ("RightUpLeg", Some(0), [-10.0, 0.0, 0.0]),
        ("RightLeg", Some(18), [0.0, -44.0, 0.0]),
        ("RightFoot", Some(19), [0.0, -43.0, 0.0]),
        ("RightToeBase", Some(20), [0.0, -8.0, 14.0]),
    ];
    let (stack, layer, node, curve) = (900_i64, 901_i64, 902_i64, 903_i64);
    let names: Vec<String> = joints.iter().map(|j| format!("{}\u{0}\u{1}Model", j.0)).collect();
    let settings = |at: usize| {
        fbx_node(
            at,
            "GlobalSettings",
            &[],
            &[&|at| {
                fbx_node(
                    at,
                    "Properties70",
                    &[],
                    &[
                        &p70("UpAxis", vec![FbxProp::I(2)]),
                        &p70("UpAxisSign", vec![FbxProp::I(1)]),
                        &p70("UnitScaleFactor", vec![FbxProp::D(1.0)]),
                    ],
                )
            }],
        )
    };
    let model = |k: usize| {
        let names = &names;
        move |at: usize| {
            // Y up written z up: (x, y, z) is (x, -z, y).
            let o = joints[k].2;
            fbx_node(
                at,
                "Model",
                &[FbxProp::L(100 + k as i64), FbxProp::S(&names[k]), FbxProp::S("LimbNode")],
                &[&|at| {
                    fbx_node(
                        at,
                        "Properties70",
                        &[],
                        &[&p70("Lcl Translation", vec![FbxProp::D(o[0]), FbxProp::D(-o[2]), FbxProp::D(o[1])])],
                    )
                }],
            )
        }
    };
    let models: Vec<Box<dyn Fn(usize) -> Vec<u8> + '_>> =
        (0..joints.len()).map(|k| Box::new(model(k)) as Box<dyn Fn(usize) -> Vec<u8>>).collect();
    let second = 46_186_158_000_i64;
    let anim = |at: usize| {
        let mut out = fbx_node(
            at,
            "AnimationStack",
            &[FbxProp::L(stack), FbxProp::S("Armature|Wave\u{0}\u{1}AnimStack"), FbxProp::S("")],
            &[&|at| {
                fbx_node(
                    at,
                    "Properties70",
                    &[],
                    &[&p70("LocalStart", vec![FbxProp::L(0)]), &p70("LocalStop", vec![FbxProp::L(second)])],
                )
            }],
        );
        let at2 = at + out.len();
        out.extend(fbx_node(
            at2,
            "AnimationLayer",
            &[FbxProp::L(layer), FbxProp::S("Layer\u{0}\u{1}AnimLayer"), FbxProp::S("")],
            &[],
        ));
        let at3 = at + out.len();
        out.extend(fbx_node(
            at3,
            "AnimationCurveNode",
            &[FbxProp::L(node), FbxProp::S("R\u{0}\u{1}AnimCurveNode"), FbxProp::S("")],
            &[&|at| {
                fbx_node(
                    at,
                    "Properties70",
                    &[],
                    &[&p70("d|X", vec![FbxProp::D(0.0)]), &p70("d|Y", vec![FbxProp::D(0.0)]), &p70("d|Z", vec![FbxProp::D(0.0)])],
                )
            }],
        ));
        let at4 = at + out.len();
        out.extend(fbx_node(
            at4,
            "AnimationCurve",
            &[FbxProp::L(curve), FbxProp::S("\u{0}\u{1}AnimCurve"), FbxProp::S("")],
            &[&|at| fbx_node(at, "KeyTime", &[FbxProp::Longs(vec![0, second])], &[]), &|at| {
                fbx_node(at, "KeyValueFloat", &[FbxProp::Floats(vec![0.0, 90.0])], &[])
            }],
        ));
        out
    };
    let objects = |at: usize| {
        let mut kids: Vec<&dyn Fn(usize) -> Vec<u8>> = models.iter().map(|m| m.as_ref()).collect();
        kids.push(&anim);
        fbx_node(at, "Objects", &[], &kids)
    };
    let mut links: Vec<Box<dyn Fn(usize) -> Vec<u8>>> = Vec::new();
    for (k, j) in joints.iter().enumerate() {
        let parent = j.1.map_or(0, |p| 100 + p as i64);
        links.push(Box::new(move |at| {
            fbx_node(at, "C", &[FbxProp::S("OO"), FbxProp::L(100 + k as i64), FbxProp::L(parent)], &[])
        }));
    }
    links.push(Box::new(move |at| fbx_node(at, "C", &[FbxProp::S("OO"), FbxProp::L(layer), FbxProp::L(stack)], &[])));
    links.push(Box::new(move |at| fbx_node(at, "C", &[FbxProp::S("OO"), FbxProp::L(node), FbxProp::L(layer)], &[])));
    // The curve node turns the left forearm (joint 8); the curve drives its z.
    links.push(Box::new(move |at| {
        fbx_node(at, "C", &[FbxProp::S("OP"), FbxProp::L(node), FbxProp::L(108), FbxProp::S("Lcl Rotation")], &[])
    }));
    links.push(Box::new(move |at| {
        fbx_node(at, "C", &[FbxProp::S("OP"), FbxProp::L(curve), FbxProp::L(node), FbxProp::S("d|Z")], &[])
    }));
    let connections = |at: usize| fbx_node(at, "Connections", &[], &links.iter().map(|l| l.as_ref()).collect::<Vec<_>>());
    let mut out = b"Kaydara FBX Binary  \x00\x1a\x00".to_vec();
    out.extend(7400u32.to_le_bytes());
    for part in [&settings as &dyn Fn(usize) -> Vec<u8>, &objects, &connections] {
        let at = out.len();
        out.extend(part(at));
    }
    out.extend([0u8; 13]);
    out
}

#[test]
fn an_fbx_stack_reads_as_a_take() {
    let path = std::env::temp_dir().join(format!("pav_wave_{}.fbx", std::process::id()));
    std::fs::write(&path, fbx_file()).unwrap();
    let mut stacks = mocap::fbx::read(&path, 30.0).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(stacks.len(), 1);
    assert_eq!(
        (stacks[0].0.as_str(), stacks[0].1.frames()),
        ("Wave", 31),
        "Blender's Armature| dropped; a second at 30 a second"
    );
    let r = Rigged::new(stacks.remove(0).1, None).unwrap();
    assert_eq!((r.map, r.scale), ("motionbuilder", 0.01));
    assert!(r.rest_frame.is_none(), "the bind pose stands");
    let take = BvhTake(&r);
    let p = |f: usize, n: &str| DVec3::from(take.points(f)[mocap::readable::index(n).unwrap()]);
    // Turned so y is up: the hips 92 cm up, the head above them, the toes ahead.
    assert!((p(0, "pelvis").y - 0.92).abs() < 1e-9, "{}", p(0, "pelvis"));
    assert!(p(0, "head").y > p(0, "pelvis").y + 0.4);
    assert!((DVec3::from(r.body.fwd) - DVec3::Z).length() < 1e-9, "faces the way the toes point: {:?}", r.body.fwd);
    // The forearm turns a quarter about the vertical over the stack, half-way at its middle.
    let fore = |f: usize| p(f, "wristL") - p(f, "elbowL");
    assert!((fore(0) - DVec3::new(0.25, 0.0, 0.0)).length() < 1e-9, "{}", fore(0));
    assert!((fore(30).x.abs()) < 1e-9 && (fore(30).z.abs() - 0.25).abs() < 1e-9, "{}", fore(30));
    assert!((fore(15).angle_between(fore(0)).to_degrees() - 45.0).abs() < 1e-6);
}
