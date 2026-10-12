//! One placed prop is one entity. Its immutable definition travels with snapshots, so a
//! restored world does not silently acquire a newer asset from the process-wide library.

use std::sync::Arc;

use glam::{Quat, Vec3};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::entity::{BodyKind, EntityId, Spawn};
use crate::physics::entity_tag;
use crate::props::geom::{self, Primitive};
use crate::props::{self, Bounds, PropDef, ResolvedPart};
use crate::shape::{Shape, Visual};
use crate::statics::RegionKey;
use crate::{Color, Sim};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropInstance {
    pub asset: String,
    #[serde(with = "props::embedded")]
    pub definition: Arc<PropDef>,
    pub scale: f32,
    /// When true, only the definition's solid parts contribute to one fixed collider.
    pub collide: bool,
}

#[derive(Clone, Debug)]
pub struct PropFrame {
    pub definition: Arc<PropDef>,
    pub scale: f32,
}

pub fn validate_scale(scale: f32) -> Result<(), String> {
    if !scale.is_finite() || !(0.01..=100.0).contains(&scale) {
        return Err("scale must be a finite number from 0.01 to 100".into());
    }
    Ok(())
}

pub fn validate_pose(pos: Vec3, rot: Quat) -> Result<(), String> {
    if !pos.is_finite() || pos.abs().max_element() > 1_000_000.0 {
        return Err("pos must contain finite world coordinates from -1000000 to 1000000".into());
    }
    if !rot.is_finite() || (rot.length_squared() - 1.0).abs() > 0.001 {
        return Err("rotation must be a finite unit quaternion".into());
    }
    Ok(())
}

impl PropInstance {
    pub fn new(asset: &str, definition: Arc<PropDef>, scale: f32, collide: bool) -> Result<Self, String> {
        let asset = props::canonical(asset)?;
        validate_scale(scale)?;
        if asset.split_once('/').map(|(_, leaf)| leaf) != Some(definition.doc.name.as_str()) {
            return Err("asset reference does not match its embedded definition".into());
        }
        Ok(Self { asset, definition, scale, collide })
    }

    pub fn frame(&self) -> PropFrame {
        PropFrame { definition: self.definition.clone(), scale: self.scale }
    }

    /// A conservative root-centred proxy for existing selection and spatial helpers. The
    /// renderer draws the parts, and physics uses their compound shape instead of this box.
    pub fn visual(&self) -> Visual {
        let b = self.definition.bounds(self.scale);
        Visual::new(Shape::Box { half: b.min.abs().max(b.max.abs()).max(Vec3::splat(0.001)) }, Color::WHITE)
    }

    pub fn bounds_at(&self, pos: Vec3, rot: Quat) -> Bounds {
        transform_bounds(self.definition.bounds(self.scale), pos, rot)
    }

    /// Used both on initial spawn and streaming wake. The collider belongs to the root ID;
    /// picking any solid part therefore selects/moves/removes the complete prop.
    pub fn collider(&self) -> Option<ColliderBuilder> {
        if !self.collide {
            return None;
        }
        let mut shapes = Vec::new();
        for part in self.definition.compiled.parts.iter().filter(|part| part.solid) {
            part_colliders(part, self.scale, &mut shapes);
        }
        (!shapes.is_empty()).then(|| ColliderBuilder::compound(shapes))
    }
}

/// One solid part's shapes in the compound (hidden parts collide too): format 1's shapes as
/// format 1 built them; a capsule or cone as itself; walls round a tube and a ring of capsules
/// round a torus; a convex hull for the rest.
fn part_colliders(part: &ResolvedPart, scale: f32, out: &mut Vec<(Pose, SharedShape)>) {
    let s = part.scale * scale;
    let pose = Pose::from_parts(part.pos * scale, part.rot);
    if let Some(shape) = part.prim.shape(s) {
        out.push((pose, shape.collider().build().shared_shape().clone()));
        return;
    }
    match &part.prim {
        Primitive::Capsule { a, b, radius, radius_end } if radius == radius_end => {
            out.push((pose, SharedShape::capsule(Vec3::Y * (a * s), Vec3::Y * (b * s), radius * s)));
        }
        Primitive::Cylinder { half_height, radius, radius_end: 0.0, arc, .. } if *arc >= 360.0 => {
            out.push((pose, SharedShape::cone(half_height * s, radius * s)));
        }
        Primitive::Tube { half_height, radius, thickness, arc } => {
            for (centre, turn, half) in geom::tube_walls(*half_height, *radius, *thickness, *arc) {
                let wall = Pose::from_parts(part.pos * scale + part.rot * (centre * s), part.rot * turn);
                out.push((wall, SharedShape::cuboid(half.x * s, half.y * s, half.z * s)));
            }
        }
        Primitive::Torus { radius, minor_radius, arc } => {
            for (a, b) in geom::torus_ring(*radius, *arc) {
                out.push((pose, SharedShape::capsule(a * s, b * s, minor_radius * s)));
            }
        }
        prim => {
            let flip = if part.mirrored { Vec3::new(-1.0, 1.0, 1.0) } else { Vec3::ONE };
            let points: Vec<Vec3> = prim.hull().iter().map(|p| *p * flip * s).collect();
            let shape = SharedShape::convex_hull(&points).unwrap_or_else(|| {
                // Flat or degenerate points: the box round them.
                let half = prim.local_box().1 * s;
                SharedShape::cuboid(half.x.max(0.001), half.y.max(0.001), half.z.max(0.001))
            });
            out.push((pose, shape));
        }
    }
}

pub fn transform_bounds(bounds: Bounds, pos: Vec3, rot: Quat) -> Bounds {
    let center = pos + rot * bounds.center();
    let half = bounds.size() * 0.5;
    let extent = (rot * Vec3::X).abs() * half.x + (rot * Vec3::Y).abs() * half.y + (rot * Vec3::Z).abs() * half.z;
    Bounds { min: center - extent, max: center + extent }
}

impl Sim {
    pub fn spawn_prop(
        &mut self,
        asset: &str,
        pos: Vec3,
        rot: Quat,
        scale: f32,
        collide: bool,
        region: Option<RegionKey>,
    ) -> Result<EntityId, String> {
        validate_pose(pos, rot)?;
        let definition = props::get(asset).ok_or_else(|| format!("no prop '{asset}' (assets lists available props)"))?;
        let instance = PropInstance::new(asset, definition, scale, collide)?;
        let mut spawn = Spawn::new(&instance.definition.doc.name, pos).rot(rot).prop(instance);
        spawn.region = region;
        Ok(self.spawn(spawn))
    }

    /// Edits a root in place, preserving its ID, room, label and embedded definition.
    pub fn update_prop_instance(&mut self, id: EntityId, pos: Vec3, rot: Quat, scale: f32, collide: bool) -> Result<(), String> {
        validate_pose(pos, rot)?;
        validate_scale(scale)?;
        let entity = self.state.entities.get_mut(id).ok_or_else(|| format!("no active entity {}", id.0))?;
        let prop = entity.prop.as_mut().ok_or_else(|| format!("entity {} is not a prop instance", id.0))?;
        prop.scale = scale;
        prop.collide = collide;
        entity.pos = pos;
        entity.rot = rot;
        entity.visual = Some(prop.visual());
        entity.body_kind = if collide { BodyKind::Fixed } else { BodyKind::None };
        self.rebuild_prop_body(id);
        self.invalidate_prop_navigation();
        Ok(())
    }

    /// Installs the accepted revision in every active and sleeping instance of this asset.
    /// Old snapshots retain their own definitions. Transforms and entity identities stay put.
    pub fn refresh_prop_instances(&mut self, name: &str, definition: Arc<PropDef>) -> usize {
        let Ok(name) = props::canonical(name) else { return 0 };
        if name.split_once('/').map(|(_, leaf)| leaf) != Some(definition.doc.name.as_str()) {
            return 0;
        }
        let ids: Vec<_> =
            self.state.entities.iter().filter(|e| e.prop.as_ref().is_some_and(|p| p.asset == name)).map(|e| e.id).collect();
        for id in &ids {
            let entity = self.state.entities.get_mut(*id).unwrap();
            let prop = entity.prop.as_mut().unwrap();
            prop.definition = definition.clone();
            entity.visual = Some(prop.visual());
            self.rebuild_prop_body(*id);
        }
        let mut count = ids.len();
        for dormant in self.state.world.dormant_entities.values_mut().flatten() {
            if let Some(prop) = dormant.entity.prop.as_mut().filter(|p| p.asset == name) {
                prop.definition = definition.clone();
                dormant.entity.visual = Some(prop.visual());
                count += 1;
            }
        }
        if count > 0 {
            self.invalidate_prop_navigation();
        }
        count
    }

    fn rebuild_prop_body(&mut self, id: EntityId) {
        let Some(entity) = self.state.entities.get_mut(id) else { return };
        let Some(prop) = &entity.prop else { return };
        let collider = prop.collider();
        if let (Some(handle), Some(next)) = (entity.body, &collider) {
            if let Some(body) = self.state.physics.bodies.get_mut(handle) {
                if let Some(current) = body.colliders().first().and_then(|h| self.state.physics.colliders.get_mut(*h)) {
                    // Keep handles and attached joints while changing the compound geometry.
                    current.set_shape(next.shape.clone());
                    body.set_position(Pose::from_parts(entity.pos, entity.rot), true);
                    return;
                }
            }
        }
        if let Some(body) = entity.body.take() {
            self.state.physics.remove_body(body);
        }
        if let Some(collider) = collider {
            let m = entity.material;
            let body = RigidBodyBuilder::fixed().pose(Pose::from_parts(entity.pos, entity.rot));
            let collider = collider
                .density(m.density as Real)
                .friction(m.friction as Real)
                .restitution(m.restitution as Real)
                .user_data(entity_tag(id.0));
            entity.body = Some(self.state.physics.insert(body, collider).0);
        }
    }

    /// Placing or resizing fixed scenery changes paths around it.
    pub fn invalidate_prop_navigation(&mut self) {
        if let Some(level) = self.state.game.as_mut().and_then(|g| g.level.as_mut()) {
            level.nav = None;
            level.flow = None;
        }
    }
}
