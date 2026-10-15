//! The asset store's S4 (docs/AGENT_ASSET_STORE.md §13, §15): ten CC0 clips a second engine reads
//! from their store rows, with each clip's body points in millimetres recorded from Shardfall as
//! a fixture (`store/fixtures/clip_points.json`). A second engine decodes the same rows and
//! compares its points with the fixture; its result is "pending: my-3dge session"
//! (`docs/handoffs/S4-my-3dge.md`).
//!
//! The fixture is written by `PAV_WRITE_FIXTURE=1 cargo test -p pav_tools --test clip_fixture`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pav_core::clips::ClipSet;
use pav_tools::mocap::readable::{POINTS, Rest, pose_of};
use serde_json::{Value, json};

/// The ten entries: the set's clips that my-3dge has not read (MESH2MOTION) and some it has
/// (QUATERNIUS), across idles, locomotion, attacks, jumps, gestures and a name with a space.
const CLIPS: [&str; 10] = [
    "MESH2MOTION/Idle_A",
    "MESH2MOTION/Jog",
    "MESH2MOTION/Death_A",
    "MESH2MOTION/Greeting",
    "MESH2MOTION/Crouch_Walk",
    "MESH2MOTION/Climb Ladder",
    "QUATERNIUS/Walk_Loop",
    "QUATERNIUS/Sword_Attack",
    "QUATERNIUS/Jump_Start",
    "QUATERNIUS/Dance_Loop",
];
/// Sample times per clip: evenly spaced from 0 to the clip's end.
const SAMPLES: usize = 6;
/// How a second engine's decode is judged against the fixture (fixed when S4 started).
const MEAN_MM: f64 = 1.0;
const WORST_MM: f64 = 5.0;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The set files named in `store/STORE.tsv`'s header: `SET anim/<file>`.
fn set_files(store: &str) -> BTreeMap<String, String> {
    let line = store.lines().find(|line| line.starts_with("# Clip sets:")).expect("STORE.tsv names its clip set files");
    line.trim_start_matches("# Clip sets:")
        .split(',')
        .filter_map(|pair| {
            let mut words = pair.split_whitespace();
            Some((words.next()?.to_owned(), words.next()?.trim_end_matches('.').to_owned()))
        })
        .collect()
}

fn round1(v: f32) -> f64 {
    (v as f64 * 10.0).round() / 10.0
}

/// Every clip's samples, decoded from its row's set file on the body its source was measured on.
fn record(store: &str) -> Value {
    let files = set_files(store);
    let mut clips = Vec::new();
    for id in CLIPS {
        let (set_name, clip_name) = id.split_once('/').unwrap();
        let row =
            store.lines().find(|line| line.split('\t').next() == Some(id)).unwrap_or_else(|| panic!("{id}: no STORE.tsv row"));
        let cells: Vec<&str> = row.split('\t').collect();
        assert_eq!((cells[1], cells[3]), ("clip", "CC0-1.0"), "{id}: a CC0 clip row");
        let file = files.get(set_name).unwrap_or_else(|| panic!("{set_name}: no set file in STORE.tsv's header"));
        let text = std::fs::read_to_string(repo().join(file)).unwrap();
        let set = ClipSet::parse(&text).unwrap();
        let raw: Value = serde_json::from_str(&text).unwrap();
        let clip = set.clips.get(clip_name).unwrap_or_else(|| panic!("{id}: not in {file}"));
        let rest: Rest = serde_json::from_value(raw["sources"][&clip.src]["rest"].clone()).unwrap();
        let fit = set.fit.get(clip_name).copied().unwrap_or([f32::NAN, f32::NAN]);
        let mut samples = Vec::new();
        let mut out = vec![0f32; POINTS.len() * 3];
        for k in 0..SAMPLES {
            let t = clip.dur as f64 * k as f64 / (SAMPLES - 1) as f64;
            pose_of(&rest, clip, t, &mut out);
            samples.push(json!({"t": (t * 1000.0).round() / 1000.0, "p": out.iter().map(|v| round1(*v)).collect::<Vec<_>>()}));
        }
        clips.push(json!({
            "id": id, "file": file, "set": set_name, "clip": clip_name, "src": clip.src, "dur": clip.dur,
            "loop": clip.looping, "capture_fit_mm": [fit[0], fit[1]], "samples": samples,
        }));
    }
    json!({
        "what": "The asset store's S4 fixture (docs/AGENT_ASSET_STORE.md §15): ten CC0 clips, each read id -> set file (STORE.tsv's '# Clip sets:' line) -> clip, decoded on the body its source was measured on (the set file's sources.<src>.rest) at evenly spaced times; body points in mm, the rig's frame (forward, right, up), 36 points per sample in `points` order, rounded to 0.1 mm. capture_fit_mm is the set file's fit: the clip's mean and worst error against its capture.",
        "fit_matches": format!("A second engine's decode matches when, per clip, the mean distance between its points and these is at most {MEAN_MM} mm and the worst at most {WORST_MM} mm."),
        "tolerance_mm": {"mean": MEAN_MM, "worst": WORST_MM},
        "points": POINTS.to_vec(),
        "clips": clips,
    })
}

#[test]
fn ten_clips_read_from_their_rows_and_hold_their_points() {
    let store = std::fs::read_to_string(repo().join("store/STORE.tsv")).unwrap();
    let now = record(&store);
    let path = repo().join("store/fixtures/clip_points.json");
    if std::env::var_os("PAV_WRITE_FIXTURE").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string(&now).unwrap() + "\n").unwrap();
    }
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("write it: PAV_WRITE_FIXTURE=1")).unwrap();
    let (old, new) = (fixture["clips"].as_array().unwrap(), now["clips"].as_array().unwrap());
    assert_eq!(old.len(), 10);
    for (a, b) in old.iter().zip(new) {
        assert_eq!((&a["id"], &a["file"]), (&b["id"], &b["file"]));
        for (sa, sb) in a["samples"].as_array().unwrap().iter().zip(b["samples"].as_array().unwrap()) {
            let (pa, pb) = (sa["p"].as_array().unwrap(), sb["p"].as_array().unwrap());
            let mut sum = 0.0;
            let mut worst: f64 = 0.0;
            for i in 0..POINTS.len() {
                let d: f64 = (0..3)
                    .map(|j| (pa[i * 3 + j].as_f64().unwrap() - pb[i * 3 + j].as_f64().unwrap()).powi(2))
                    .sum::<f64>()
                    .sqrt();
                sum += d;
                worst = worst.max(d);
            }
            // Shardfall must reproduce its own fixture far inside the cross-engine tolerance.
            assert!(
                sum / POINTS.len() as f64 <= 0.1 && worst <= 0.2,
                "{}: t {} moved (mean {}, worst {worst})",
                a["id"],
                sa["t"],
                sum / 36.0
            );
        }
    }
}
