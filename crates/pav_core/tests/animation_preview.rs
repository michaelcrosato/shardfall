//! The animation stage must share the engine pose and preserve the paused game.

use pav_core::animation_preview::PreviewChange;
use pav_core::{InputFrame, Sim};

const CLIP: &str = "QUATERNIUS/Sword_Regular_Combo";

#[test]
fn preview_freezes_the_world_and_closing_resumes_it() {
    let mut sim = Sim::new("empty", 23).unwrap();
    sim.run(30, &InputFrame::default());
    let before = sim.state_hash();
    let tick = sim.state.tick;
    let recording = sim.recording.ticks();
    let newest = sim.history.newest_tick();
    sim.preview_open(Some(CLIP), None).unwrap();
    sim.run(90, &InputFrame::default());
    assert_eq!(sim.state.tick, tick);
    assert_eq!(sim.state_hash(), before);
    assert_eq!(sim.recording.ticks(), recording);
    assert_eq!(sim.history.newest_tick(), newest);
    let frame = sim.frame();
    assert!(frame.animation_preview.is_some());
    assert!(frame.game.is_none() && frame.room.is_none());
    assert_eq!(frame.objects.iter().filter(|e| e.puppet.is_some()).count(), 1);
    sim.preview_close();
    assert_eq!(sim.state_hash(), before);
    sim.step(&InputFrame::default());
    assert_eq!(sim.state.tick, tick + 1);
    assert!(sim.frame().animation_preview.is_none());
}

#[test]
fn scrubbing_speed_repeat_and_source_frame_steps_share_one_clock() {
    let mut sim = Sim::empty(1);
    let initial = sim.preview_open(Some(CLIP), None).unwrap();
    let duration = initial.duration;
    sim.preview_change(&PreviewChange { time: Some(0.2), speed: Some(2.0), ..Default::default() }).unwrap();
    sim.run(30, &InputFrame::default());
    assert_eq!(sim.preview_info().unwrap().time, 0.2);
    sim.preview_change(&PreviewChange { playing: Some(true), ..Default::default() }).unwrap();
    sim.run(15, &InputFrame::default());
    assert!((sim.preview_info().unwrap().time - 0.7).abs() < 1e-4);
    let p = sim.preview_change(&PreviewChange { time: Some(duration), repeat: Some(false), ..Default::default() }).unwrap();
    assert_eq!(p.time, duration);
    assert!(!p.playing);
    let p = sim.preview_change(&PreviewChange { step: Some(-1), ..Default::default() }).unwrap();
    assert!((p.time - (duration - 1.0 / initial.fps)).abs() < 1e-5);
    sim.preview_change(&PreviewChange { playing: Some(true), ..Default::default() }).unwrap();
    sim.run(30, &InputFrame::default());
    let p = sim.preview_info().unwrap();
    assert_eq!(p.time, duration);
    assert!(!p.playing);
    sim.preview_change(&PreviewChange {
        time: Some(duration - 0.01),
        repeat: Some(true),
        playing: Some(true),
        ..Default::default()
    })
    .unwrap();
    sim.step(&InputFrame::default());
    assert!(sim.preview_info().unwrap().time < 0.05);
}

#[test]
fn selecting_the_same_asset_and_restoring_a_snapshot_keep_the_playhead() {
    let mut sim = Sim::empty(1);
    sim.preview_open(Some(CLIP), None).unwrap();
    sim.preview_change(&PreviewChange { time: Some(0.6), mirror: Some(true), travel: Some(true), ..Default::default() }).unwrap();
    let snapshot = sim.snapshot();
    sim.preview_open(Some(CLIP), None).unwrap();
    let p = sim.preview_info().unwrap();
    assert_eq!(p.time, 0.6);
    assert!(!p.playing && p.mirror && p.travel);
    sim.preview_close();
    sim.restore(snapshot);
    let p = sim.preview_info().unwrap();
    assert_eq!(p.time, 0.6);
    assert!(!p.playing && p.mirror && p.travel);
    let state = sim.frame().objects.into_iter().find_map(|o| o.puppet).unwrap().state;
    assert_eq!(state.clip_t, 0.6);
    assert_eq!(state.clip_w, 1.0);
    assert_ne!(state.clip_flags & pav_core::clips::CLAMP, 0);
}

#[test]
fn rejected_controls_leave_preview_unchanged_and_moves_use_engine_poses() {
    let mut sim = Sim::empty(1);
    sim.preview_open(None, Some("slash")).unwrap();
    let before = serde_json::to_value(sim.preview_info()).unwrap();
    let bad = PreviewChange { time: Some(0.8), speed: Some(f32::NAN), ..Default::default() };
    assert!(sim.preview_change(&bad).is_err());
    assert_eq!(serde_json::to_value(sim.preview_info()).unwrap(), before);
    let duration = sim.preview_info().unwrap().duration;
    sim.preview_change(&PreviewChange { time: Some(duration * 0.4), ..Default::default() }).unwrap();
    let p = sim.state.animation_preview.as_ref().unwrap();
    let st = p.state_at(p.info().time);
    assert_eq!(st.act_kind, pav_core::moves::MoveId::named("slash").unwrap().index());
    assert!((st.act - 0.4).abs() < 1e-5);
    assert!(p.skeleton().unwrap().hand.iter().all(|p| p.is_finite()));
}
