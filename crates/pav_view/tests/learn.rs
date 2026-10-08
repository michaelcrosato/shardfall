//! Station guides stay honest: every word a room names is in the field guide, every knob is a
//! real setting, every pad that changes something says what, and the field guide's own
//! cross references resolve.

use pav_core::guide;
use pav_core::params::{self, ParamVisitor, Tunable, nested};
use pav_core::room::{load_sources, parse_all};
use pav_core::sim::SimConfig;
use pav_core::zones::ZoneKind;
use pav_view::{CameraParams, ViewSettings};

/// Every setting the game's panel (and so the station guide's knobs) can reach.
struct Root {
    sim: SimConfig,
    camera: CameraParams,
    view: ViewSettings,
}

impl Tunable for Root {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        self.sim.visit_groups(v);
        nested(v, "camera", &mut self.camera);
        nested(v, "view", &mut self.view);
    }
}

#[test]
fn field_guide_cross_references_resolve() {
    assert!(guide::terms().len() >= 60);
    for t in guide::terms() {
        assert_eq!(t.key, t.key.to_lowercase(), "keys are lower case: {}", t.key);
        for s in &t.see {
            assert!(guide::term(s).is_some(), "'{}' sees unknown '{s}'", t.key);
        }
    }
}

#[test]
fn station_guides_name_real_words_and_settings() {
    let (defs, errors) = parse_all(&load_sources(None));
    assert!(errors.is_empty(), "{errors:?}");
    let mut root = Root { sim: SimConfig::default(), camera: CameraParams::default(), view: ViewSettings::default() };
    let paths: Vec<String> = params::list(&mut root).into_iter().map(|p| p.path).collect();
    let mut guides = 0;
    for (key, def) in &defs {
        let l = &def.learn;
        if l.is_empty() {
            continue;
        }
        guides += 1;
        assert!(!l.what.is_empty() && !l.how.is_empty(), "{key}: a guide says what you see and how it works");
        for t in &l.terms {
            assert!(guide::term(t).is_some(), "{key}: unknown field guide word '{t}'");
        }
        for k in &l.knobs {
            assert!(paths.contains(k), "{key}: knob '{k}' is not a setting");
        }
        for c in &l.code {
            assert!(!c.src.trim().is_empty() && !c.title.is_empty(), "{key}: empty code snippet");
            if !c.file.is_empty() {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(&c.file);
                assert!(path.exists(), "{key}: code from a file that does not exist: {}", c.file);
            }
        }
        // Pads that change something say what (and why it looks that way).
        for z in def.layout.legend.values().filter_map(|t| t.zone.as_ref()) {
            if z.kind == ZoneKind::Pad && !z.params.is_empty() {
                assert!(!z.note.is_empty(), "{key}: pad '{}' has no note", z.label);
            }
        }
    }
    assert!(guides >= 10, "station guides: {guides}");
}
