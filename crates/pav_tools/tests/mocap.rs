//! The motion importer against my-3D2dge's own (tools/anim-import.mjs): a small glTF rig and a
//! CMU take, translated by both, agree number for number (the expected sets in
//! tests/fixtures/mocap were written by that tool). A BVH take cuts into a loop that plays.

use std::path::{Path, PathBuf};

use glam::DVec3;
use pav_core::clips::ClipSet;
use pav_tools::mocap::bvh::{Bvh, BvhTake, Rigged};
use pav_tools::mocap::takes::{self, Find, Pick, Take};
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
    let libs = mocap::glb_libraries(std::slice::from_ref(&glb), &["RIG".into()], &[], &cat, 30.0, &mut log);
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

/// A walker in the MotionBuilder skeleton: legs swinging once a second, the hips bobbing
/// twice, walking forward 1.2 m a second for 3 s, then turning on the spot.
fn walker() -> String {
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
    let (fps, secs) = (60, 5);
    text += &format!("MOTION\nFrames: {}\nFrame Time: {}\n", fps * secs, 1.0 / fps as f64);
    for f in 0..fps * secs {
        let t = f as f64 / fps as f64;
        let ph = std::f64::consts::TAU * t;
        let walking = t < 3.0;
        let mut v = vec![0.0; 6 + 3 * (joints.len() - 1)];
        let z = 120.0 * t.min(3.0);
        v[0] = 0.0;
        v[1] = 92.0 + if walking { 2.0 * (2.0 * ph).cos() } else { 0.0 };
        v[2] = z;
        // Turning on the spot after three seconds: yaw (the root's third rotation channel, Y).
        v[5] = if walking { 0.0 } else { 90.0 * (t - 3.0) };
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
    let (a, b) = takes::find(&take, 60.0, Find::Still, None).expect("a still stretch");
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
