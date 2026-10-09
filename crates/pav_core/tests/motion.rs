//! Motion clips and moves: imported clips decode like the format's reference player, every
//! embedded clip plays cleanly on the puppet, performers take turns, chained moves wind up from
//! where the hands were, idle clips play while standing still, and the hero falls with a
//! captured death.

use glam::Vec3;
use pav_core::clips;
use pav_core::moves::MoveId;
use pav_core::puppet::{PuppetDef, PuppetState};
use pav_core::{InputFrame, Sim};

/// Direction in the format's rig frame (forward, right, up) from character space (x right,
/// y up, z forward).
fn rig(v: Vec3) -> Vec3 {
    let n = v.normalize();
    Vec3::new(n.z, n.x, n.y)
}

fn angle(a: Vec3, b: Vec3) -> f32 {
    a.normalize().dot(b.normalize()).clamp(-1.0, 1.0).acos().to_degrees()
}

#[test]
fn clips_decode_like_the_reference_player() {
    // Limb and spine directions from my-3D2dge's own decoder (src/mocap/readable.js), on the
    // libraries' bodies: our puppet has other proportions, but every limb must point the same way.
    #[rustfmt::skip]
    let reference: &[(&str, f32, [[f32; 3]; 6])] = &[
        // clip, time: spine, armL, armR, legL, legR, head
        ("QUATERNIUS/Sword_Regular_Combo", 1.0, [[0.764, 0.193, 0.615], [0.452, -0.427, -0.784], [-0.842, 0.471, 0.261], [-0.882, -0.198, -0.429], [0.408, 0.134, -0.903], [0.492, 0.424, 0.76]]),
        ("QUATERNIUS/Sword_Regular_Combo", 2.0, [[0.771, -0.264, 0.579], [-0.948, -0.127, 0.293], [-0.683, -0.593, -0.427], [0.597, -0.084, -0.798], [-0.875, 0.1, -0.473], [0.824, -0.126, 0.552]]),
        ("CMU/Breakdance_Helicopter", 3.0, [[0.455, -0.582, 0.674], [-0.504, -0.749, -0.429], [0.657, 0.136, -0.742], [0.413, -0.213, -0.885], [-0.632, 0.434, -0.643], [0.424, -0.219, 0.879]]),
        ("MESH2MOTION/Dance Charleston", 0.9, [[0.277, 0.001, 0.961], [0.074, -0.352, -0.933], [0.055, 0.164, -0.985], [-0.152, 0.199, -0.968], [-0.096, 0.204, -0.974], [0.412, 0.034, 0.91]]),
    ];
    let def = PuppetDef::default();
    let lib = clips::library();
    for (name, t, want) in reference {
        let id = lib.find(name).unwrap_or_else(|| panic!("{name} is in the library"));
        let sk = clips::skel(&def, &lib.get(id).unwrap().key_at(*t), false);
        let ours = [
            sk.neck - sk.pelvis,
            sk.hand[0] - sk.shoulder[0],
            sk.hand[1] - sk.shoulder[1],
            sk.ankle[0] - sk.hip[0],
            sk.ankle[1] - sk.hip[1],
            sk.crown,
        ];
        for (k, (o, w)) in ours.iter().zip(want).enumerate() {
            let a = angle(rig(*o), Vec3::from(*w));
            assert!(a < 9.0, "{name} at {t}s: part {k} points {a:.1} degrees off the reference");
        }
    }
}

#[test]
fn every_embedded_clip_plays_cleanly() {
    let def = PuppetDef::default();
    let arm = def.arm_length * def.scale * 0.5;
    let leg = def.leg_length * def.scale * 0.5;
    let lib = clips::library();
    assert!(lib.len() >= 300, "the three libraries are embedded: {}", lib.len());
    for set in &lib.sets {
        assert!(!set.credit.is_empty() && !set.sources.is_empty(), "{} names its sources", set.set);
        for (name, c) in &set.clips {
            for i in 0..4 {
                let sk = clips::skel(&def, &c.key_at(c.dur * i as f32 / 3.0), false);
                let pts =
                    [sk.pelvis, sk.chest, sk.head, sk.hand[0], sk.hand[1], sk.ankle[0], sk.ankle[1], sk.knee[0], sk.elbow[1]];
                assert!(pts.iter().all(|p| p.is_finite()), "{}/{name}: finite", set.set);
                for s in 0..2 {
                    assert!(sk.ankle[s].y > -1e-3, "{}/{name}: a foot under the floor {}", set.set, sk.ankle[s]);
                    assert!(((sk.elbow[s] - sk.shoulder[s]).length() - arm).abs() < 2e-3, "{}/{name}: upper arm length", set.set);
                    assert!(((sk.knee[s] - sk.hip[s]).length() - leg).abs() < 2e-3, "{}/{name}: thigh length", set.set);
                }
            }
        }
    }
}

#[test]
fn a_set_file_round_trips() {
    let lib = clips::library();
    let set = lib.sets.iter().find(|s| s.set == "QUATERNIUS").unwrap();
    let back = clips::ClipSet::parse(&set.to_text()).expect("the written set reads back");
    assert_eq!(back.clips.len(), set.clips.len());
    for (k, c) in &set.clips {
        assert_eq!(back.clips[k].keys.len(), c.keys.len(), "{k}");
    }
}

#[test]
fn clips_fade_in_and_out_over_the_walk() {
    let id = clips::find("MESH2MOTION/Dance Charleston").unwrap();
    let mut st = PuppetState::default();
    st.play_clip(id, 0, 1.0);
    let def = PuppetDef::default();
    let input = pav_core::puppet::AnimInput { grounded: true, ..Default::default() };
    for _ in 0..30 {
        st.update(&def, &input, 1.0 / 60.0);
    }
    assert!((st.clip_w - 1.0).abs() < 1e-5 && st.clip_t > 0.45, "faded in and playing: {} {}", st.clip_w, st.clip_t);
    // A second clip comes in over the first, which is held underneath until it has.
    let other = clips::find("QUATERNIUS/Idle_Loop").unwrap();
    st.play_clip(other, 0, 1.0);
    assert_eq!((st.clip, st.clip2), (other, id));
    for _ in 0..30 {
        st.update(&def, &input, 1.0 / 60.0);
    }
    assert_eq!(st.clip2, 0, "the old clip is let go once the new one is in");
    st.stop_clip();
    for _ in 0..30 {
        st.update(&def, &input, 1.0 / 60.0);
    }
    assert_eq!((st.clip, st.clip_w), (0, 0.0), "faded out");
}

#[test]
fn chained_moves_wind_up_from_where_the_hands_were() {
    let mut st = PuppetState::default();
    // Mid-strike of a slash, then a thrust begins.
    st.set_action(MoveId::of("slash"), 0.55, 0.5, 1.0);
    st.set_action(MoveId::of("thrust"), 0.0, 0.5, 1.0);
    assert!(st.chain_w > 0.5, "the thrust remembers the slash's hands: {}", st.chain_w);
    let table = pav_core::moves::table();
    let slash = pav_core::moves::frame(&table, MoveId::of("slash").index(), 0.55, 0.5, 1.0).unwrap();
    let held = slash.hands(false)[1].0;
    assert!((glam::Vec3::from(st.chain_r) - held).length() < 1e-4, "right hand where the slash had it");
    // A fresh action from rest remembers nothing.
    let mut calm = PuppetState::default();
    calm.set_action(MoveId::of("thrust"), 0.0, 0.5, 1.0);
    assert_eq!(calm.chain_w, 0.0);
    // The pose at the very start of the chained thrust is the slash's, not the walk's.
    let def = PuppetDef {
        weapon: pav_core::puppet::WeaponLook { kind: pav_core::puppet::WeaponKind::Sword, ..Default::default() },
        ..Default::default()
    };
    let start = pav_core::puppet::procedural(&def, &st, &table);
    let rest = pav_core::puppet::procedural(&def, &calm, &table);
    assert!(start.hand[1].x < rest.hand[1].x - 0.15, "the right hand starts across the body, where the slash left it");
}

#[test]
fn a_kick_starts_from_the_standing_foot() {
    let table = pav_core::moves::table();
    let def = PuppetDef::default();
    let mut st = PuppetState::default();
    st.set_action(MoveId::of("roundhouse"), 0.0, 0.4, 1.0);
    let start = pav_core::puppet::procedural(&def, &st, &table);
    assert!(start.ankle[1].y < 0.05, "the kicking foot starts on the ground: {}", start.ankle[1]);
    st.set_action(MoveId::of("roundhouse"), 0.45, 0.4, 1.0);
    let high = pav_core::puppet::procedural(&def, &st, &table);
    assert!(high.ankle[1].y > 0.8, "and swings up to head height: {}", high.ankle[1]);
}

#[test]
fn performers_take_turns_with_their_clips() {
    let mut sim = Sim::new("motion_library", 1).unwrap();
    let find = |sim: &Sim, n: &str| sim.state.entities.iter().find(|e| e.name == n).map(|e| e.id).unwrap();
    let dancer = find(&sim, "dance");
    sim.run(30, &InputFrame::default());
    let first = sim.state.entities.get(dancer).unwrap().character.as_ref().unwrap().anim.clip;
    assert_eq!(first, clips::find("MESH2MOTION/Dance Charleston").unwrap(), "starts with its first clip");
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..60 * 14 {
        sim.step(&InputFrame::default());
        let a = &sim.state.entities.get(dancer).unwrap().character.as_ref().unwrap().anim;
        if a.clip_w > 0.99 {
            seen.insert(a.clip);
        }
    }
    assert!(seen.len() >= 2, "moves on to the next clip: {seen:?}");
    // The moves performer swings through the moves table.
    let mut sim = Sim::new("action_moves", 1).unwrap();
    let blades = find(&sim, "blades");
    let mut acted = std::collections::BTreeSet::new();
    for _ in 0..60 * 6 {
        sim.step(&InputFrame::default());
        acted.insert(sim.state.entities.get(blades).unwrap().character.as_ref().unwrap().anim.act_kind);
    }
    acted.remove(&0);
    assert!(acted.len() >= 3, "several moves in six seconds: {acted:?}");
}

#[test]
fn an_idle_clip_plays_while_standing_still() {
    let mut sim = Sim::new("empty", 1).unwrap();
    sim.config.puppet.idle_clip = "QUATERNIUS/Idle_Loop".into();
    let id = clips::find("QUATERNIUS/Idle_Loop").unwrap();
    sim.run(60, &InputFrame::default());
    let anim = |sim: &Sim| sim.player().unwrap().character.as_ref().unwrap().anim;
    assert_eq!(anim(&sim).clip, id, "standing still: the idle clip");
    assert!(anim(&sim).clip_w > 0.99);
    sim.run(40, &InputFrame { move_dir: glam::Vec2::new(1.0, 0.0), ..Default::default() });
    assert_eq!(anim(&sim).clip, 0, "walking: the procedural walk again");
}

#[test]
fn the_hero_falls_with_a_captured_death() {
    let mut sim = Sim::new("arena", 1).unwrap();
    sim.run(30, &InputFrame::default());
    let hid = sim.state.game.as_ref().unwrap().hero_id.unwrap();
    sim.state.game.as_mut().unwrap().actors.get_mut(&hid).unwrap().life = -1.0e6;
    sim.run(20, &InputFrame::default());
    let death = clips::find(pav_core::arpg::HERO_DEATHS[0]).unwrap();
    let a = sim.player().unwrap().character.as_ref().unwrap().anim;
    assert_eq!(a.clip, death, "the first death is the first captured fall");
    // Back on the feet after the respawn, the fall fades away.
    sim.run(60 * 4, &InputFrame::default());
    let a = sim.player().unwrap().character.as_ref().unwrap().anim;
    assert!(a.clip != death || a.clip_flags & clips::STOP != 0, "the fall ends when the hero rises");
}

#[test]
fn walkers_play_their_style_as_fast_as_they_move() {
    let mut sim = Sim::new("walk_styles", 1).unwrap();
    sim.run(60 * 3, &InputFrame::default());
    let def = PuppetDef::default();
    for (name, clip) in [("old", "STYLE100/Old_Walk"), ("zombie", "STYLE100/Zombie_Walk"), ("elated", "STYLE100/Elated_Walk")] {
        let e = sim.state.entities.iter().find(|e| e.name == name).unwrap();
        let ch = e.character.as_ref().unwrap();
        let ground = glam::Vec2::new(ch.vel.x, ch.vel.z).length();
        assert_eq!(ch.anim.clip, clips::find(clip).unwrap(), "{name} walks in its style");
        // Each walker moves at its style's own pace, so the clip plays at about its own speed.
        let pace = clips::pace(&def, ch.anim.clip).expect("a walk loop knows its pace");
        assert!((ground / pace - 1.0).abs() < 0.3, "{name}: moving {ground} m/s, the style's pace {pace}");
        assert!((ch.anim.clip_speed - ground / pace).abs() < 1e-3, "{name}: played at {}", ch.anim.clip_speed);
    }
    // The slowest and the quickest: an old man a seventh of an elated walker's pace.
    let pace = |c: &str| clips::pace(&def, clips::find(c).unwrap()).unwrap();
    assert!(pace("STYLE100/Old_Walk") * 5.0 < pace("STYLE100/Elated_Walk"));
}

#[test]
fn the_villagers_walk_their_rounds_in_style() {
    let mut sim = Sim::new("town", 1).unwrap();
    let styles: Vec<u32> = pav_core::arpg::scene::VILLAGER_CLIPS.iter().map(|(_, w)| clips::find(w).unwrap()).collect();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..60 * 20 {
        sim.step(&InputFrame::default());
        for e in sim.state.entities.iter() {
            if let Some(ch) = e.character.as_ref() {
                if styles.contains(&ch.anim.clip) && ch.anim.clip_w > 0.99 {
                    let pace = clips::pace(&PuppetDef::default(), ch.anim.clip).unwrap();
                    let ground = glam::Vec2::new(ch.vel.x, ch.vel.z).length();
                    assert!(ground < pace * 1.6, "{} walks at about its style's pace: {ground} vs {pace}", e.name);
                    seen.insert(e.name.clone());
                }
            }
        }
    }
    assert_eq!(seen.len(), 2, "both villagers walked in style: {seen:?}");
}
