//! Rippling water: dents spread as rings, bounce around and fade; things moving through the
//! water leave a wake; adjacent strips of one pool ripple as one surface.

use glam::Vec3;
use pav_core::zones::{Zone, ZoneKind};
use pav_view::water::{WaterSettings, WaterSurface, merge_zones};

fn pool(min: Vec3, max: Vec3) -> Zone {
    Zone {
        min,
        max,
        kind: ZoneKind::Water,
        course: String::new(),
        index: 0,
        params: Default::default(),
        camera: None,
        label: String::new(),
        label_size: None,
        color: None,
        facing: None,
        speed: 0.0,
        signal: String::new(),
        note: String::new(),
    }
}

const SETTINGS: WaterSettings = WaterSettings { speed: 1.6, fade: 1.4, rain: 0.0 };

#[test]
fn a_dent_spreads_as_a_ring_and_fades() {
    let mut w = WaterSurface::new(&pool(Vec3::new(0.0, -2.0, 0.0), Vec3::new(8.0, 0.0, 8.0)));
    let centre = Vec3::new(4.0, 0.0, 4.0);
    w.dent(centre, 0.4, 0.1);
    assert!(w.height_at(centre) < -0.05);
    let far = centre + Vec3::new(1.6, 0.0, 0.0);
    assert_eq!(w.height_at(far), 0.0);
    // After half a second the ring has reached 1.6 m away (wave speed 1.6 m/s).
    let mut reached = 0.0f32;
    for _ in 0..60 {
        w.step(1.0 / 60.0, &SETTINGS, &[]);
        reached = reached.max(w.height_at(far).abs());
    }
    assert!(reached > 0.002, "the ring got there ({reached})");
    let early = w.roughness();
    for _ in 0..600 {
        w.step(1.0 / 60.0, &SETTINGS, &[]);
    }
    assert!(w.roughness() < early * 0.1, "ripples fade ({} -> {})", early, w.roughness());
}

#[test]
fn rain_keeps_the_surface_moving() {
    let mut w = WaterSurface::new(&pool(Vec3::ZERO, Vec3::new(4.0, 1.0, 4.0)));
    let rainy = WaterSettings { rain: 1.0, ..SETTINGS };
    for _ in 0..120 {
        w.step(1.0 / 60.0, &rainy, &[]);
    }
    assert!(w.roughness() > 0.005);
}

#[test]
fn pool_strips_merge_into_one_surface() {
    // A pool made of three strips (a step along one side), plus a separate pond.
    let a = pool(Vec3::new(0.0, -2.0, 0.0), Vec3::new(5.0, -0.2, 2.0));
    let b = pool(Vec3::new(0.0, -1.5, 2.0), Vec3::new(5.0, -0.2, 4.0));
    let c = pool(Vec3::new(0.0, -2.0, 4.0), Vec3::new(5.0, -0.2, 6.0));
    let pond = pool(Vec3::new(20.0, -1.0, 0.0), Vec3::new(23.0, -0.2, 3.0));
    let merged = merge_zones(&[&a, &b, &c, &pond]);
    assert_eq!(merged.len(), 2);
    let big = merged.iter().find(|z| z.min.x == 0.0).unwrap();
    assert_eq!((big.min.z, big.max.z), (0.0, 6.0));
    // An L shape stays two surfaces (one rectangle would cover dry ground).
    let l1 = pool(Vec3::new(0.0, -1.0, 0.0), Vec3::new(4.0, -0.2, 1.0));
    let l2 = pool(Vec3::new(0.0, -1.0, 1.0), Vec3::new(1.0, -0.2, 4.0));
    assert_eq!(merge_zones(&[&l1, &l2]).len(), 2);
}
