//! Every room file parses, builds on its own, and its courses are complete.

use std::collections::BTreeMap;

use pav_core::Sim;
use pav_core::room::{load_sources, parse_all};
use pav_core::statics::RegionKey;
use pav_core::zones::ZoneKind;

#[test]
fn all_rooms_build_and_courses_are_complete() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rooms");
    let sources = load_sources(Some(&dir));
    let (rooms, errors) = parse_all(&sources);
    assert!(errors.is_empty(), "room files with errors: {errors:?}");
    assert!(rooms.len() >= 8, "found {} rooms", rooms.len());
    for (key, _) in &rooms {
        let mut sim = Sim::new(key, 1).unwrap_or_else(|e| panic!("room {key}: {e:#}"));
        sim.run(30, &Default::default());
        let zones = sim.state.statics.region_zones(RegionKey::Room(0));
        // course -> (starts, finishes)
        let mut courses: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for z in zones {
            match z.kind {
                ZoneKind::Start => courses.entry(&z.course).or_default().0 += 1,
                ZoneKind::Finish => courses.entry(&z.course).or_default().1 += 1,
                ZoneKind::Gate => {
                    courses.entry(&z.course).or_default();
                }
                _ => {}
            }
        }
        for (c, (s, f)) in courses {
            assert!(s > 0 && f > 0, "room {key}: course '{c}' needs a START and a FINISH (has {s} / {f})");
        }
        let p = sim.player().expect("player").pos;
        assert!(p.y > -5.0 && p.is_finite(), "room {key}: player fell out ({p})");
    }
}
