//! One placed prop is one entity. Its immutable definition travels with snapshots, so a
//! restored world does not silently acquire a newer asset from the process-wide library.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{Quat, Vec3};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::entity::{BodyKind, EntityId, Spawn};
use crate::issue;
use crate::physics::entity_tag;
use crate::props::geom::{self, Primitive};
use crate::props::{self, Bounds, CompiledProp, PropDef, PropDefTable, ResolvedPart};
use crate::shape::{Shape, Visual};
use crate::statics::RegionKey;
use crate::{Color, Sim, SimState};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropInstance {
    pub asset: String,
    /// Snapshots name it by key in `SimState::prop_defs`.
    #[serde(with = "props::table::by_key")]
    pub definition: Arc<PropDef>,
    pub scale: f32,
    /// When true, only the definition's solid parts contribute to one fixed collider.
    pub collide: bool,
    /// The variant drawn and collided: chance, jitter and pick resolve with it (0: as
    /// authored).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub seed: u32,
    /// Colours for the theme roles its parts name (`$wall` → `#rrggbb`); other roles take
    /// `props::DEFAULT_ROLES`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub roles: BTreeMap<String, String>,
}

fn is_zero(seed: &u32) -> bool {
    *seed == 0
}

#[derive(Clone, Debug)]
pub struct PropFrame {
    pub definition: Arc<PropDef>,
    /// The parts of the instance's variant.
    pub parts: Arc<CompiledProp>,
    pub scale: f32,
    pub roles: BTreeMap<String, String>,
}

/// A palette: each key one of `props::doc::ROLES`, each colour `#rrggbb`.
pub fn validate_roles(roles: &BTreeMap<String, String>) -> Result<(), String> {
    for (role, color) in roles {
        if !props::doc::ROLES.contains(&role.as_str()) {
            let fix =
                issue::suggestion(role, props::doc::ROLES.iter().copied()).map(|fix| format!(" — {fix}")).unwrap_or_default();
            return Err(format!("roles: unknown role \"{role}\" (expected {}){fix}", props::doc::ROLES.join(", ")));
        }
        if !(color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|b| b.is_ascii_hexdigit())) {
            return Err(format!("roles.{role}: \"{color}\" is not a #rrggbb colour"));
        }
    }
    Ok(())
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
        if props::leaf(&asset) != definition.doc.name {
            return Err("asset reference does not match its embedded definition".into());
        }
        Ok(Self { asset, definition, scale, collide, seed: 0, roles: BTreeMap::new() })
    }

    /// This copy's variant: chance, jitter and pick resolve with `seed`.
    pub fn with_seed(mut self, seed: u32) -> Self {
        self.seed = seed;
        self
    }

    /// Colours for the theme roles its parts name, e.g. `{"$wall":"#5a6b7c"}`.
    pub fn with_roles(mut self, roles: BTreeMap<String, String>) -> Result<Self, String> {
        validate_roles(&roles)?;
        self.roles = roles;
        Ok(self)
    }

    /// The parts this copy draws and collides with.
    pub fn parts(&self) -> Arc<CompiledProp> {
        self.definition.variant(self.seed)
    }

    pub fn frame(&self) -> PropFrame {
        PropFrame { definition: self.definition.clone(), parts: self.parts(), scale: self.scale, roles: self.roles.clone() }
    }

    /// A conservative root-centred proxy for existing selection and spatial helpers. The
    /// renderer draws the parts, and physics uses their compound shape instead of this box.
    pub fn visual(&self) -> Visual {
        let b = self.parts().bounds_at(self.scale);
        Visual::new(Shape::Box { half: b.min.abs().max(b.max.abs()).max(Vec3::splat(0.001)) }, Color::WHITE)
    }

    pub fn bounds_at(&self, pos: Vec3, rot: Quat) -> Bounds {
        transform_bounds(self.parts().bounds_at(self.scale), pos, rot)
    }

    /// Used both on initial spawn and streaming wake. The collider belongs to the root ID;
    /// picking any solid part therefore selects/moves/removes the complete prop.
    pub fn collider(&self) -> Option<ColliderBuilder> {
        if !self.collide {
            return None;
        }
        let mut shapes = Vec::new();
        for part in self.parts().parts.iter().filter(|part| part.solid) {
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

impl SimState {
    /// The definitions placed copies use, awake or asleep, and nothing else: what a snapshot
    /// carries. A state restored from a snapshot older than the table gets its table here.
    pub fn rebuild_prop_defs(&mut self) {
        let mut table = PropDefTable::default();
        let active = self.entities.iter().filter_map(|e| e.prop.as_ref());
        let asleep = self.world.dormant_entities.values().flatten().filter_map(|d| d.entity.prop.as_ref());
        for prop in active.chain(asleep) {
            table.insert(&prop.definition);
        }
        self.prop_defs = table;
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
        let definition = props::resolve_ref(asset)?;
        self.place_prop(PropInstance::new(asset, definition, scale, collide)?, pos, rot, region)
    }

    /// Place a prepared copy (its seed and palette set).
    pub fn place_prop(
        &mut self,
        instance: PropInstance,
        pos: Vec3,
        rot: Quat,
        region: Option<RegionKey>,
    ) -> Result<EntityId, String> {
        validate_pose(pos, rot)?;
        let mut spawn = Spawn::new(&instance.definition.doc.name, pos).rot(rot).prop(instance);
        spawn.region = region;
        Ok(self.spawn(spawn))
    }

    /// A copy's variant and palette: its parts and collision follow the seed.
    pub fn restyle_prop(
        &mut self,
        id: EntityId,
        seed: Option<u32>,
        roles: Option<BTreeMap<String, String>>,
    ) -> Result<(), String> {
        if let Some(roles) = &roles {
            validate_roles(roles)?;
        }
        let entity = self.state.entities.get_mut(id).ok_or_else(|| format!("no active entity {}", id.0))?;
        let prop = entity.prop.as_mut().ok_or_else(|| format!("entity {} is not a prop instance", id.0))?;
        prop.seed = seed.unwrap_or(prop.seed);
        prop.roles = roles.unwrap_or_else(|| prop.roles.clone());
        entity.visual = Some(prop.visual());
        self.rebuild_prop_body(id);
        self.invalidate_prop_navigation();
        Ok(())
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
        if props::leaf(&name) != definition.doc.name {
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
            self.state.rebuild_prop_defs();
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
