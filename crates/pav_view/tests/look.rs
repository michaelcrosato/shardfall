//! The look layer: sections own exactly their view settings, presets only touch their own
//! section (and never the part of the scene a filter is on), whole looks switch on what they
//! name, and switched-off sections leave the scene's settings alone.

use std::collections::BTreeMap;

use pav_core::params::{self, ParamKind, ParamValue};
use pav_view::ViewSettings;
use pav_view::build::{PartChoice, PixelTarget};
use pav_view::look::{self, Look, Section};

/// A view where every setting differs from the default.
fn changed_view() -> ViewSettings {
    let mut v = ViewSettings::default();
    let mut map = BTreeMap::new();
    for p in params::list(&mut v) {
        let new = match (&p.kind, &p.value) {
            (ParamKind::Float { min, max }, cur) => {
                let x = cur.as_f64().unwrap_or(0.0) as f32;
                ParamValue::Float(if (x - max).abs() > 1e-3 { *max as f64 } else { *min as f64 })
            }
            (ParamKind::Int { min, max }, cur) => {
                let x = cur.as_f64().unwrap_or(0.0) as i32;
                ParamValue::Int(if x != *max { *max as i64 } else { *min as i64 })
            }
            (ParamKind::Bool, cur) => ParamValue::Bool(!cur.as_bool().unwrap_or(false)),
            (ParamKind::Choice { options }, ParamValue::Text(cur)) => {
                ParamValue::Text(options.iter().find(|o| *o != cur).cloned().unwrap_or_default())
            }
            _ => continue,
        };
        map.insert(p.path, new);
    }
    assert!(v.apply(&map).is_empty());
    v
}

fn diff(a: &ViewSettings, b: &ViewSettings) -> Vec<String> {
    let (ma, mb) = (params::to_map(&mut a.clone()), params::to_map(&mut b.clone()));
    ma.keys().filter(|k| ma.get(*k) != mb.get(*k)).cloned().collect()
}

#[test]
fn sections_copy_exactly_their_paths() {
    let from = changed_view();
    let mut owned = Vec::new();
    for s in Section::ALL {
        let mut to = ViewSettings::default();
        s.copy(&from, &mut to);
        let mut changed = diff(&to, &ViewSettings::default());
        let mut paths: Vec<String> = s.paths().iter().map(|p| p.to_string()).collect();
        changed.sort();
        paths.sort();
        assert_eq!(changed, paths, "section {}", s.key());
        owned.extend(paths);
    }
    let n = owned.len();
    owned.sort();
    owned.dedup();
    assert_eq!(owned.len(), n, "no path belongs to two sections");
}

#[test]
fn presets_are_valid_and_stay_in_their_section() {
    let mut total = 0;
    for s in Section::ALL {
        let list: Vec<_> = look::presets(s).collect();
        assert!(list.len() >= 3, "section {} has a few presets", s.key());
        for p in list {
            total += 1;
            for k in p.set.keys() {
                assert_eq!(Section::of_path(k), Some(s), "preset {} / {}: {k}", s.key(), p.name);
            }
            let mut v = ViewSettings::default();
            assert!(v.apply(&p.set).is_empty(), "preset {} applies cleanly", p.name);
        }
    }
    assert!(total >= 40);
    assert!(look::looks().len() >= 8);
    for l in look::looks() {
        for k in l.set.keys() {
            assert!(Section::of_path(k).is_some(), "look {}: {k} belongs to a section", l.name);
        }
        let mut v = ViewSettings::default();
        assert!(v.apply(&l.set).is_empty(), "look {} applies cleanly", l.name);
    }
}

#[test]
fn a_look_replaces_only_the_sections_that_are_on() {
    let base = changed_view();
    let l = Look::from_preset(look::look_preset("Pixel heroes").unwrap());
    assert_eq!(l.on.iter().copied().collect::<Vec<_>>(), vec![Section::Pixel]);
    let mut v = base.clone();
    l.apply(&mut v);
    assert_eq!(v.filter.pixel_target, PixelTarget::Objects);
    assert_eq!(v.filter.pixel_art, 5.0);
    let changed = diff(&v, &base);
    assert!(changed.iter().all(|k| Section::of_path(k) == Some(Section::Pixel)), "{changed:?}");

    // Nothing on: the scene's own settings.
    let mut v = base.clone();
    Look::default().apply(&mut v);
    assert!(diff(&v, &base).is_empty());
}

#[test]
fn presets_keep_the_part_of_the_scene() {
    let current = ViewSettings::default();
    let mut l = Look::default();
    l.set_on(Section::Palette, true, &current);
    l.values.filter.color_on = PartChoice::Environment;
    l.apply_preset(look::preset(Section::Palette, "Game Boy").unwrap(), &current);
    assert_eq!(l.values.filter.color_on, PartChoice::Environment);
    assert_eq!(l.matching_preset(Section::Palette).map(|p| p.name.as_str()), Some("Game Boy"));
    l.values.filter.dither = 0.123;
    assert!(l.matching_preset(Section::Palette).is_none());

    // Switching a section on starts from what is on screen.
    let shown = ViewSettings { bloom: 1.4, ..Default::default() };
    let mut l = Look::default();
    l.set_on(Section::Glow, true, &shown);
    assert_eq!(l.values.bloom, 1.4);
}

#[test]
fn looks_round_trip_through_json() {
    let mut l = Look::from_preset(look::look_preset("Game Boy world").unwrap());
    l.compare = 0.5;
    let back: Look = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
    assert_eq!(back.on, l.on);
    assert_eq!(back.to_map(), l.to_map());
    assert_eq!(back.to_map().get("filter.split"), Some(&ParamValue::Float(0.5)));
}
