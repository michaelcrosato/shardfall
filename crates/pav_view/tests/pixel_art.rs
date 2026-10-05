//! Pixel art for part of the scene: each `pixel_target` flags exactly the instances of what it
//! names (all, characters, the hero, other characters, the world, one entity).

use pav_core::Sim;
use pav_render::scene::{Scene, flags};
use pav_view::build::{PixelTarget, ViewBuilder, ViewSettings};
use pav_view::camera::CameraRig;

fn scene(sim: &mut Sim, target: PixelTarget, entity: i32, size: f32) -> Scene {
    let frame = sim.frame();
    let mut view = ViewSettings::default();
    view.filter.pixel_art = size;
    view.filter.pixel_target = target;
    view.filter.pixel_entity = entity;
    ViewBuilder::new().build(&frame, &frame, 1.0, &CameraRig::default(), 16.0 / 9.0, &view, glam::Vec3::ZERO)
}

/// Groups of the flagged and unflagged instances.
fn split(s: &Scene) -> (Vec<u32>, Vec<u32>) {
    let all = s.meshes.iter().map(|m| (m.group, m.flags)).chain(s.sdfs.iter().map(|d| (d.group, d.flags)));
    let (on, off): (Vec<_>, Vec<_>) = all.partition(|(_, f)| f & flags::PIXEL != 0);
    (on.into_iter().map(|x| x.0).collect(), off.into_iter().map(|x| x.0).collect())
}

#[test]
fn pixel_art_targets_pick_the_right_things() {
    let mut sim = Sim::new("town", 1).unwrap();
    for _ in 0..30 {
        sim.step(&Default::default());
    }
    let frame = sim.frame();
    let hero = frame.player.unwrap().0 + 2;
    let puppets: Vec<u32> = frame.objects.iter().filter(|o| o.puppet.is_some()).map(|o| o.id.0 + 2).collect();
    let other = *puppets.iter().find(|g| **g != hero).expect("townsfolk");
    assert!(puppets.contains(&hero));

    let (on, _) = split(&scene(&mut sim, PixelTarget::All, 0, 1.0));
    assert!(on.is_empty(), "pixel_art = 1 is off");

    let (on, off) = split(&scene(&mut sim, PixelTarget::All, 0, 4.0));
    assert!(off.is_empty() && !on.is_empty());

    let (on, off) = split(&scene(&mut sim, PixelTarget::Characters, 0, 4.0));
    assert!(on.iter().all(|g| puppets.contains(g)) && on.contains(&hero) && on.contains(&other));
    assert!(off.iter().all(|g| !puppets.contains(g)) && off.contains(&1), "level geometry stays sharp");

    let (on, off) = split(&scene(&mut sim, PixelTarget::Hero, 0, 4.0));
    assert!(on.iter().all(|g| *g == hero) && !on.is_empty());
    assert!(off.contains(&other));

    let (on, _) = split(&scene(&mut sim, PixelTarget::Others, 0, 4.0));
    assert!(on.iter().all(|g| puppets.contains(g) && *g != hero) && on.contains(&other));

    let (on, off) = split(&scene(&mut sim, PixelTarget::World, 0, 4.0));
    assert!(on.contains(&1) && on.iter().all(|g| !puppets.contains(g)));
    assert!(off.contains(&hero));

    let (on, _) = split(&scene(&mut sim, PixelTarget::Entity, other as i32 - 2, 4.0));
    assert!(on.iter().all(|g| *g == other) && !on.is_empty());
}
