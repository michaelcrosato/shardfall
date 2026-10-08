//! Parts of the scene for filters: the characters and objects are flagged `OBJECT`, the
//! environment (level geometry, fixed scenery) is not; pixel art, styles and screen filters
//! can aim at either part.

use glam::Vec3;
use pav_core::entity::{BodyKind, Spawn};
use pav_core::shape::{Shape, Visual};
use pav_core::{Color, Sim};
use pav_render::scene::{Part, Scene, Style, Stylize, flags};
use pav_view::build::{PartChoice, PixelTarget, StyleOverride, StylizeChoice, ViewBuilder, ViewSettings};
use pav_view::camera::CameraRig;

fn build(sim: &mut Sim, view: &ViewSettings) -> Scene {
    let frame = sim.frame();
    ViewBuilder::new().build(&frame, &frame, 1.0, &CameraRig::default(), 16.0 / 9.0, view, Vec3::ZERO)
}

/// (group, flags, style) of every mesh and SDF instance.
fn instances(s: &Scene) -> Vec<(u32, u32, Style)> {
    s.meshes.iter().map(|m| (m.group, m.flags, m.style)).chain(s.sdfs.iter().map(|d| (d.group, d.flags, d.style))).collect()
}

fn town() -> (Sim, u32, Vec<u32>, u32, u32) {
    let mut sim = Sim::new("town", 1).unwrap();
    let crate_id = sim.spawn(
        Spawn::new("test crate", Vec3::new(2.0, 1.0, 9.0))
            .visual(Visual::new(Shape::Box { half: Vec3::splat(0.3) }, Color::hex("#c86a3c")))
            .body(BodyKind::Dynamic),
    );
    for _ in 0..30 {
        sim.step(&Default::default());
    }
    let frame = sim.frame();
    let hero = frame.player.unwrap().0 + 2;
    let puppets: Vec<u32> = frame.objects.iter().filter(|o| o.puppet.is_some()).map(|o| o.id.0 + 2).collect();
    let scenery = frame.objects.iter().find(|o| o.scenery).expect("the town has fixed scenery").id.0 + 2;
    assert!(!frame.objects.iter().any(|o| o.scenery && o.puppet.is_some()), "characters are never scenery");
    (sim, hero, puppets, scenery, crate_id.0 + 2)
}

#[test]
fn characters_and_objects_are_flagged_the_environment_is_not() {
    let (mut sim, hero, puppets, scenery, crate_group) = town();
    let s = build(&mut sim, &ViewSettings::default());
    let all = instances(&s);
    let object = |g: u32| all.iter().filter(|i| i.0 == g).all(|i| i.1 & flags::OBJECT != 0);
    let environment = |g: u32| all.iter().filter(|i| i.0 == g).all(|i| i.1 & flags::OBJECT == 0);
    assert!(object(hero) && puppets.iter().all(|g| object(*g)), "characters are objects");
    assert!(all.iter().any(|i| i.0 == crate_group) && object(crate_group), "a dynamic prop is an object");
    assert!(all.iter().any(|i| i.0 == scenery) && environment(scenery), "fixed scenery is environment");
    // The static level geometry comes first and is all environment.
    assert!(s.meshes.first().is_some_and(|m| m.group == 1 && m.flags & flags::OBJECT == 0));
}

#[test]
fn pixel_art_on_objects_or_environment() {
    let (mut sim, hero, _, scenery, crate_group) = town();
    let mut view = ViewSettings::default();
    view.filter.pixel_art = 5.0;
    view.filter.pixel_target = PixelTarget::Objects;
    let s = instances(&build(&mut sim, &view));
    assert!(s.iter().all(|i| (i.1 & flags::PIXEL != 0) == (i.1 & flags::OBJECT != 0)));
    assert!(s.iter().any(|i| i.0 == hero && i.1 & flags::PIXEL != 0));
    assert!(s.iter().any(|i| i.0 == crate_group && i.1 & flags::PIXEL != 0));

    view.filter.pixel_target = PixelTarget::Environment;
    let s = instances(&build(&mut sim, &view));
    assert!(s.iter().all(|i| (i.1 & flags::PIXEL != 0) == (i.1 & flags::OBJECT == 0)));
    assert!(s.iter().any(|i| i.0 == scenery && i.1 & flags::PIXEL != 0));
    assert!(s.iter().filter(|i| i.0 == hero).all(|i| i.1 & flags::PIXEL == 0));
}

#[test]
fn each_part_gets_its_own_style() {
    let (mut sim, ..) = town();
    let mut view =
        ViewSettings { style_objects: StyleOverride::Lit, style_environment: StyleOverride::Flat, ..Default::default() };
    let s = instances(&build(&mut sim, &view));
    let lit = s.iter().filter(|i| i.1 & flags::OBJECT != 0 && i.2 != Style::Unlit);
    assert!(lit.clone().count() > 0 && lit.into_iter().all(|i| i.2 == Style::Lit));
    let flat = s.iter().filter(|i| i.1 & flags::OBJECT == 0 && i.2 != Style::Unlit);
    assert!(flat.clone().count() > 0 && flat.into_iter().all(|i| i.2 == Style::Flat));

    // The global override still forces one style everywhere (unlit markers included).
    view.style = StyleOverride::Cel;
    let s = instances(&build(&mut sim, &view));
    assert!(s.iter().all(|i| i.2 == Style::Cel));
}

#[test]
fn filter_parts_reach_the_renderer() {
    let (mut sim, ..) = town();
    let mut view = ViewSettings { outlines_on: PartChoice::Objects, ..Default::default() };
    view.filter.color_on = PartChoice::Environment;
    view.filter.grade_on = PartChoice::Objects;
    view.filter.scanlines_on = PartChoice::Environment;
    view.filter.grain_on = PartChoice::Objects;
    view.filter.chroma_on = PartChoice::Environment;
    view.filter.stylize = StylizeChoice::Halftone;
    view.filter.stylize_on = PartChoice::Objects;
    view.haze = 0.8;
    view.halos = 1.5;
    let s = build(&mut sim, &view);
    assert_eq!(s.post.outline_part, Part::Objects);
    let f = &s.filter;
    assert_eq!(
        [f.color_part, f.grade_part, f.scanline_part, f.grain_part, f.chroma_part, f.stylize_part],
        [Part::Environment, Part::Objects, Part::Environment, Part::Objects, Part::Environment, Part::Objects]
    );
    assert_eq!(f.stylize, Stylize::Halftone);
    assert_eq!((s.post.haze, s.post.halos), (0.8, 1.5));
    // No transition unless one is playing.
    assert_eq!(f.transition, 0.0);
}

#[test]
fn a_transition_loop_closes_and_opens() {
    use pav_view::build::transition_loop;
    let samples: Vec<f32> = (0..200).map(|i| transition_loop(i as f32 * 0.02, 0.7)).collect();
    assert!(samples.iter().all(|c| (0.0..=1.0).contains(c)));
    assert_eq!(samples[0], 0.0, "it starts open");
    assert!(samples.contains(&1.0), "it closes fully");
    // Open again by the end of the period (3 s), then the next one starts open.
    assert!(transition_loop(2.99, 0.7) < 0.01);
    assert_eq!(transition_loop(3.1, 0.7), 0.0);
}
