//! Sandbox editing (F10): spawn, drag and delete objects with the mouse, then save them into
//! the current room's file (the `[[object]]` tables; maps and comments are kept).

use glam::{Mat4, Quat, Vec2, Vec3};
use pav_core::statics::RegionKey;
use pav_core::world::RayHit;
use pav_core::{BodyKind, Color, EntityId, Shape, Spawn, Visual};
use pav_render::scene::{MeshInstance, MeshKey, Scene, SdfInstance, Style, flags};
use pav_view::CameraRig;

use crate::simhost::SimHost;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Place,
    Move,
    Delete,
}

pub const SHAPES: &[&str] = &["crate", "box", "sphere", "capsule", "cylinder", "rounded box", "wall", "platform", "pillar"];

pub struct Editor {
    pub on: bool,
    pub tool: Tool,
    pub shape: usize,
    pub size: f32,
    pub color: [u8; 3],
    pub dynamic: bool,
    pub hover: Option<RayHit>,
    pub dragging: Option<EntityId>,
    pub status: String,
    was_down: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            on: false,
            tool: Tool::Place,
            shape: 0,
            size: 1.0,
            color: [0xe8, 0x70, 0x4a],
            dynamic: true,
            hover: None,
            dragging: None,
            status: String::new(),
            was_down: false,
        }
    }
}

impl Editor {
    fn shape(&self) -> Shape {
        let k = self.size;
        match SHAPES[self.shape] {
            "crate" => Shape::Box { half: Vec3::splat(0.4 * k) },
            "box" => Shape::Box { half: Vec3::new(0.5, 0.25, 0.35) * k },
            "sphere" => Shape::Sphere { radius: 0.4 * k },
            "capsule" => Shape::Capsule { half_height: 0.35 * k, radius: 0.25 * k },
            "cylinder" => Shape::Cylinder { half_height: 0.4 * k, radius: 0.35 * k },
            "rounded box" => Shape::RoundedBox { half: Vec3::new(0.5, 0.3, 0.5) * k, radius: 0.12 * k },
            "wall" => Shape::Box { half: Vec3::new(1.0, 1.0, 0.15) * k },
            "platform" => Shape::Box { half: Vec3::new(1.0, 0.15, 1.0) * k },
            _ => Shape::Cylinder { half_height: 1.2 * k, radius: 0.25 * k },
        }
    }

    fn color(&self) -> Color {
        Color::srgb8(self.color[0], self.color[1], self.color[2])
    }

    /// Per-frame update: hover raycast and mouse actions. Returns true if the left mouse
    /// button was used by the editor (so it must not throw bombs).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        host: &SimHost,
        rig: &CameraRig,
        cursor: Vec2,
        size: Vec2,
        left_down: bool,
        left_tap: bool,
        room: Option<u16>,
    ) -> bool {
        if !self.on {
            self.was_down = left_down;
            return false;
        }
        let (o, d) = rig.screen_ray(cursor, size);
        let exclude = self.dragging;
        self.hover = host.query(move |sim| editor_raycast(sim, o, d, 500.0, exclude)).flatten();
        let pressed = left_tap || (left_down && !self.was_down);
        let released = !left_down && (self.was_down || left_tap);
        self.was_down = left_down;
        match self.tool {
            Tool::Place => {
                if pressed {
                    if let Some(h) = self.hover {
                        let shape = self.shape();
                        let up = shape.half_extents().y;
                        let pos = h.point + h.normal * 0.02 + Vec3::Y * up;
                        let body = if self.dynamic { BodyKind::Dynamic } else { BodyKind::Fixed };
                        let name = SHAPES[self.shape].replace(' ', "_");
                        let mut sp = Spawn::new(&name, pos).visual(Visual::new(shape, self.color())).body(body);
                        sp.region = room.map(RegionKey::Room);
                        host.exec(move |sim| {
                            sim.spawn(sp);
                        });
                    }
                }
            }
            Tool::Move => {
                if pressed {
                    self.dragging = self.hover.and_then(|h| h.entity);
                }
                if let (Some(id), Some(h)) = (self.dragging, self.hover) {
                    host.exec(move |sim| {
                        let lift = sim
                            .state
                            .entities
                            .get(id)
                            .map(|e| {
                                e.prop
                                    .as_ref()
                                    .map(|p| -p.bounds_at(Vec3::ZERO, e.rot).min.y)
                                    .or_else(|| e.visual.as_ref().map(|v| v.shape.half_extents().y))
                                    .unwrap_or(0.3)
                            })
                            .unwrap_or(0.3);
                        sim.set_position(id, h.point + Vec3::Y * (lift + 0.02));
                    });
                }
                if released {
                    self.dragging = None;
                }
            }
            Tool::Delete => {
                if pressed {
                    if let Some(id) = self.hover.and_then(|h| h.entity) {
                        host.exec(move |sim| {
                            if sim.state.player != Some(id) && sim.state.entities.get(id).is_some_and(|e| e.character.is_none()) {
                                sim.despawn(id);
                            }
                        });
                    }
                }
            }
        }
        true
    }

    /// Preview of what will be placed.
    pub fn draw_preview(&self, scene: &mut Scene) {
        if !self.on || self.tool != Tool::Place {
            return;
        }
        let Some(h) = self.hover else { return };
        let shape = self.shape();
        let pos = h.point + Vec3::Y * shape.half_extents().y;
        let c = Vec3::new(0.75, 0.95, 1.0);
        let fl = flags::NO_SHADOW | flags::NO_CUT;
        match shape {
            Shape::Sphere { radius } => {
                let mut s = SdfInstance::sphere(pos, radius, c);
                s.style = Style::Unlit;
                s.flags = fl;
                scene.sdfs.push(s);
            }
            Shape::Capsule { half_height, radius } => {
                let mut s = SdfInstance::capsule(pos - Vec3::Y * half_height, pos + Vec3::Y * half_height, radius, c);
                s.style = Style::Unlit;
                s.flags = fl;
                scene.sdfs.push(s);
            }
            other => {
                let (mesh, scale) = match other {
                    Shape::Cylinder { half_height, radius } => {
                        (MeshKey::Cylinder, Vec3::new(radius * 2.0, half_height * 2.0, radius * 2.0))
                    }
                    _ => (MeshKey::Cube, other.half_extents() * 2.0),
                };
                scene.meshes.push(MeshInstance {
                    mesh,
                    transform: Mat4::from_scale_rotation_translation(scale, Quat::IDENTITY, pos),
                    color: c,
                    emissive: 0.0,
                    style: Style::Unlit,
                    flags: fl,
                    group: 0xffe0,
                });
            }
        }
    }

    /// The edit-mode window. Returns true when "Save room" was clicked.
    pub fn ui(&mut self, ctx: &egui::Context, room_name: Option<&str>) -> bool {
        if !self.on {
            return false;
        }
        let mut save = false;
        egui::Window::new("Edit mode (F10)").anchor(egui::Align2::LEFT_BOTTOM, [10.0, -40.0]).resizable(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tool, Tool::Place, "Place");
                ui.selectable_value(&mut self.tool, Tool::Move, "Move");
                ui.selectable_value(&mut self.tool, Tool::Delete, "Delete");
            });
            if self.tool == Tool::Place {
                egui::ComboBox::from_label("shape").selected_text(SHAPES[self.shape]).show_ui(ui, |ui| {
                    for (i, s) in SHAPES.iter().enumerate() {
                        ui.selectable_value(&mut self.shape, i, *s);
                    }
                });
                ui.add(egui::Slider::new(&mut self.size, 0.25..=4.0).text("size"));
                ui.horizontal(|ui| {
                    ui.label("color");
                    ui.color_edit_button_srgb(&mut self.color);
                    ui.checkbox(&mut self.dynamic, "physics (falls, can be pushed)");
                });
            }
            let hover = match self.hover.and_then(|h| h.entity) {
                Some(id) => format!("pointing at entity #{}", id.0),
                None => "pointing at static geometry".into(),
            };
            ui.label(egui::RichText::new(hover).small());
            ui.separator();
            match room_name {
                Some(r) => {
                    if ui.button(format!("Save objects into room '{r}'")).clicked() {
                        save = true;
                    }
                }
                None => {
                    ui.label(egui::RichText::new("Stand in a room to save edits into its file.").small());
                }
            }
            if !self.status.is_empty() {
                ui.label(egui::RichText::new(&self.status).small());
            }
            ui.label(
                egui::RichText::new("Left click: place / drag / delete · right-drag: camera · WASD still walks").small().weak(),
            );
        });
        save
    }
}

/// Writes the room's current objects into its file (in the rooms directory, created next to
/// the executable if there is none). Returns the path written.
pub fn save_room(host: &SimHost, room_id: u16) -> anyhow::Result<std::path::PathBuf> {
    let (key, objects) = host
        .query(move |sim| {
            let key = sim.state.world.rooms.get(room_id as usize).map(|r| r.key.clone()).unwrap_or_default();
            (key, sim.room_objects(room_id))
        })
        .ok_or_else(|| anyhow::anyhow!("simulation did not answer"))?;
    let dir = pav_core::room::rooms_dir().unwrap_or_else(|| crate::boot::exe_dir().join("rooms"));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{key}.toml"));
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => pav_core::room::load_sources(None)
            .into_iter()
            .find(|s| s.key == key)
            .map(|s| s.text)
            .ok_or_else(|| anyhow::anyhow!("no source for room '{key}'"))?,
    };
    let out = pav_core::world::rewrite_room_objects(&text, &objects);
    pav_core::room::RoomDef::parse(&out).map_err(|e| anyhow::anyhow!("refusing to save an invalid room: {e}"))?;
    std::fs::write(&path, out)?;
    Ok(path)
}

/// Props can be selected while paused, including visual-only props. Physics query bounds
/// update on a physics tick; these local bounds use the current root transform immediately.
fn editor_raycast(sim: &pav_core::Sim, origin: Vec3, dir: Vec3, max: f32, exclude: Option<EntityId>) -> Option<RayHit> {
    let dir = dir.normalize_or(Vec3::NEG_Y);
    let mut hit = sim.raycast(origin, dir, max, exclude);
    let mut nearest = hit.map(|h| h.point.distance(origin)).unwrap_or(max);
    for entity in sim.state.entities.iter().filter(|e| Some(e.id) != exclude) {
        let Some(prop) = &entity.prop else { continue };
        let bounds = prop.definition.bounds(prop.scale);
        let inverse = entity.rot.inverse();
        let local_origin = inverse * (origin - entity.pos);
        let local_dir = inverse * dir;
        if let Some((distance, normal)) = ray_bounds(local_origin, local_dir, bounds.min, bounds.max, nearest) {
            nearest = distance;
            hit = Some(RayHit { point: origin + dir * distance, normal: entity.rot * normal, entity: Some(entity.id) });
        }
    }
    hit
}

fn ray_bounds(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3, limit: f32) -> Option<(f32, Vec3)> {
    let (mut near, mut far, mut normal) = (0.0f32, limit, -dir);
    for axis in 0..3 {
        if dir[axis].abs() < 1.0e-8 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let a = (min[axis] - origin[axis]) / dir[axis];
        let b = (max[axis] - origin[axis]) / dir[axis];
        let enter = a.min(b);
        if enter > near {
            near = enter;
            normal = Vec3::ZERO;
            normal[axis] = -dir[axis].signum();
        }
        far = far.min(a.max(b));
        if near > far {
            return None;
        }
    }
    Some((near, normal))
}

#[cfg(test)]
mod prop_selection_tests {
    use super::*;

    #[test]
    fn paused_visual_prop_can_be_picked_after_spawn_and_move() {
        let mut sim = pav_core::Sim::empty(1);
        let id = sim.spawn_prop("BUILTIN/box", Vec3::new(2.0, 0.0, 1.0), Quat::from_rotation_y(0.6), 1.0, false, None).unwrap();
        let hit = editor_raycast(&sim, Vec3::new(2.0, 5.0, 1.0), Vec3::NEG_Y, 10.0, None).unwrap();
        assert_eq!(hit.entity, Some(id));
        sim.set_position(id, Vec3::new(5.0, 0.0, 1.0));
        let hit = editor_raycast(&sim, Vec3::new(5.0, 5.0, 1.0), Vec3::NEG_Y, 10.0, None).unwrap();
        assert_eq!(hit.entity, Some(id));
        assert_eq!(sim.state.tick, 0);
        assert!(editor_raycast(&sim, Vec3::new(2.0, 5.0, 1.0), Vec3::NEG_Y, 10.0, None).is_none());
        assert!(editor_raycast(&sim, Vec3::new(5.0, 5.0, 1.0), Vec3::NEG_Y, 10.0, Some(id)).is_none());
    }
}
