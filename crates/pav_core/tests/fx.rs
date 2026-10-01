//! M7 visual-effect data on room objects (lights, particle emitters, distortion) and room view
//! tables: they parse, reach the render frame, survive an edit-mode save, and pads hand
//! `view.*` settings to the app.

use pav_core::room::RoomDef;
use pav_core::{InputFrame, Sim};

fn room() -> Sim {
    let def = RoomDef::parse(include_str!("fx_room.toml")).expect("room parses");
    let mut sim = Sim::empty(1);
    pav_core::scenes::build_standalone_room(&mut sim, "fx", def);
    sim.state.scene = "fx".into();
    sim
}

#[test]
fn effects_reach_the_frame_and_survive_saving() {
    let mut sim = room();
    sim.run(5, &InputFrame::default());
    let frame = sim.frame();
    let torch = frame.objects.iter().find(|o| o.visual.light.is_some()).expect("a lit object");
    assert!(torch.visual.particles.is_some() && torch.visual.distortion.is_some());
    assert_eq!(frame.room.as_ref().map(|r| r.def.view.len()), Some(4), "room view table");
    let saved = sim.room_objects(0);
    let text: String = saved.iter().map(pav_core::world::object_toml).collect::<Vec<_>>().join("\n");
    assert!(text.contains("light = {") && text.contains("particles = {") && text.contains("distortion = {"), "{text}");
}
