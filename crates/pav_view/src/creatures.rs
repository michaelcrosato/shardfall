//! Native preview drawing for generated creatures. Skinning is CPU math in pav_core; the
//! resulting vertices use the same dynamic mesh renderer as the rest of the scene.

use glam::Vec3;
use pav_core::RenderFrame;
use pav_core::creature_preview::CreatureFrame;
use pav_render::mesh::{MeshData, Vertex};
use pav_render::scene::{DynamicMesh, Scene, Style, flags};

/// Interpolate only within one accepted definition and clip. A publication ticket or workspace
/// change draws the current revision immediately, so a submitted frame cannot show stale data.
pub(crate) fn interpolate(prev: &RenderFrame, curr: &RenderFrame, alpha: f32) -> Option<CreatureFrame> {
    let mut frame = curr.creature.clone()?;
    let Some(old) = prev.creature.as_ref() else {
        return Some(frame);
    };
    if prev.live_edit_ticket != curr.live_edit_ticket
        || prev.studio_mode() != curr.studio_mode()
        || old.definition.revision() != frame.definition.revision()
        || old.clip != frame.clip
        || old.id != frame.id
    {
        return Some(frame);
    }
    let alpha = if alpha.is_finite() { alpha.clamp(0.0, 1.0) } else { 1.0 };
    frame.pos = old.pos.lerp(frame.pos, alpha);
    frame.rot = old.rot.slerp(frame.rot, alpha);
    frame.scale = old.scale + (frame.scale - old.scale) * alpha;
    let duration = frame.clip.as_ref().and_then(|name| frame.definition.clips.get(name)).map_or(0.0, |clip| clip.duration);
    let end = frame.time + if frame.looping && frame.time < old.time { duration } else { 0.0 };
    frame.time = old.time + (end - old.time) * alpha;
    if frame.looping && duration > 0.0 && frame.time > duration {
        frame.time = frame.time.rem_euclid(duration);
    }
    Some(frame)
}

pub(crate) fn emit(scene: &mut Scene, frame: &CreatureFrame, style: Style) -> Result<(), String> {
    let pose = frame.definition.sample_pose(frame.clip.as_deref(), frame.time, frame.looping)?;
    let skinned = frame.definition.skin(&pose)?;
    for (name, skin) in skinned {
        let source = &frame.definition.meshes[&name];
        let vertices = skin
            .positions
            .iter()
            .zip(&skin.normals)
            .enumerate()
            .map(|(i, (position, normal))| Vertex {
                pos: (frame.pos + frame.rot * (*position * frame.scale)).to_array(),
                normal: (frame.rot * *normal).normalize_or(Vec3::Y).to_array(),
                uv: [0.0, 0.0],
                color: [source.colors[i * 3], source.colors[i * 3 + 1], source.colors[i * 3 + 2], 1.0],
            })
            .collect();
        let two_sided = source.double_sided || name == "membranes";
        scene.dynamic.push(DynamicMesh {
            data: MeshData { vertices, indices: source.indices.clone() },
            color: Vec3::ONE,
            emissive: 0.0,
            style,
            flags: flags::OBJECT | if two_sided { flags::TWO_SIDED } else { 0 },
            group: frame.id.0 + 2,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;
    use pav_core::creatures::{CreatureAsset, FORMAT, GENERATOR_REVISION};
    use pav_core::{EntityId, Sim};
    use serde_json::json;
    use std::sync::Arc;

    fn definition(red: f32) -> Arc<CreatureAsset> {
        Arc::new(
            serde_json::from_value(json!({
                "format":FORMAT,"generator_revision":GENERATOR_REVISION,"name":"view-fixture",
                "source_revision":"view-source","quality":"low",
                "bones":{"names":["root"],"parents":[-1],"positions":[0,0,0],"rotations":[0,0,0,1],"lengths":[1]},
                "meshes":{"membranes":{"positions":[0,0,0,1,0,0,0,1,0],"normals":[0,0,1,0,0,1,0,0,1],
                    "indices":[0,1,2],"colors":[red,0.5,0.75,red,0.5,0.75,red,0.5,0.75],
                    "skin_indices":[0,0,0,0,0,0,0,0,0,0,0,0],"skin_weights":[1,0,0,0,1,0,0,0,1,0,0,0]}},
                "bounds":{"min":[0,0,0],"max":[1,1,0]}
            }))
            .unwrap(),
        )
    }

    fn subject(definition: Arc<CreatureAsset>) -> CreatureFrame {
        CreatureFrame {
            id: EntityId(1_000_000),
            definition,
            clip: None,
            time: 0.0,
            looping: true,
            pos: Vec3::new(2.0, 3.0, 4.0),
            rot: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            scale: 2.0,
        }
    }

    #[test]
    fn native_mesh_keeps_linear_colors_rotated_normals_and_double_sided_membranes() {
        let mut scene = Scene::default();
        emit(&mut scene, &subject(definition(0.25)), Style::Lit).unwrap();
        let mesh = &scene.dynamic[0];
        assert_eq!(mesh.data.vertices[0].color, [0.25, 0.5, 0.75, 1.0]);
        assert!((Vec3::from_array(mesh.data.vertices[1].pos) - Vec3::new(4.0, 3.0, 4.0)).length() < 1e-5);
        assert!((Vec3::from_array(mesh.data.vertices[1].normal) - Vec3::NEG_Y).length() < 1e-5);
        assert_ne!(mesh.flags & flags::TWO_SIDED, 0);
        assert_ne!(mesh.flags & flags::OBJECT, 0);
        assert_eq!(mesh.group, 1_000_002);
    }

    #[test]
    fn published_definition_is_not_blended_with_an_older_frame() {
        let mut sim = Sim::empty(1);
        let mut old = sim.frame();
        old.creature = Some(subject(definition(0.25)));
        let mut current = old.clone();
        current.creature = Some(subject(definition(0.75)));
        current.creature.as_mut().unwrap().pos = Vec3::new(9.0, 0.0, 0.0);
        let result = interpolate(&old, &current, 0.0).unwrap();
        assert_eq!(result.pos, Vec3::new(9.0, 0.0, 0.0));
        assert_eq!(result.definition.revision(), current.creature.as_ref().unwrap().definition.revision());
        old.creature = None;
        // The first creature frame must also draw without requiring a previous subject.
        let result = interpolate(&old, &current, 0.0);
        assert!(result.is_some());
    }
}
