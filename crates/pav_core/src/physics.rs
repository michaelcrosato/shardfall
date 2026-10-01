//! Physics state (rapier3d). `PhysicsState` is plain cloneable data so it can live inside
//! snapshots; the pipeline (workspace only) is kept outside by the simulation.

use rapier::prelude::*;

/// Collider user-data tags: which game object a collider belongs to.
pub const TAG_ENTITY: u128 = 1 << 112;
pub const TAG_BLOCK: u128 = 2 << 112;
pub const TAG_MASK: u128 = 0xFFFF << 112;

pub fn entity_tag(id: u32) -> u128 {
    TAG_ENTITY | id as u128
}

/// The entity id stored in a collider's user data, if it belongs to an entity.
pub fn entity_from_tag(tag: u128) -> Option<u32> {
    (tag & TAG_MASK == TAG_ENTITY).then_some(tag as u32)
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct PhysicsState {
    pub gravity: Vector,
    pub params: IntegrationParameters,
    pub islands: IslandManager,
    pub broad_phase: DefaultBroadPhase,
    pub narrow_phase: NarrowPhase,
    pub bodies: RigidBodySet,
    pub colliders: ColliderSet,
    pub impulse_joints: ImpulseJointSet,
    pub multibody_joints: MultibodyJointSet,
    pub soft_bodies: SoftBodySet,
    pub ccd: CCDSolver,
    /// Fixed body that world-anchored joints attach to.
    #[serde(default)]
    pub anchor: Option<RigidBodyHandle>,
}

impl PhysicsState {
    pub fn new(dt: f32) -> Self {
        let params = IntegrationParameters { dt: dt as Real, ..Default::default() };
        Self {
            gravity: Vector::new(0.0, -9.81, 0.0),
            params,
            islands: IslandManager::new(),
            broad_phase: DefaultBroadPhase::default(),
            narrow_phase: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            soft_bodies: SoftBodySet::new(),
            ccd: CCDSolver::new(),
            anchor: None,
        }
    }

    pub fn step(&mut self, pipeline: &mut PhysicsPipeline, events: &dyn EventHandler) {
        pipeline.step(
            self.gravity,
            &self.params,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.soft_bodies,
            &mut self.ccd,
            &(),
            events,
        );
    }

    pub fn insert(&mut self, body: impl Into<RigidBody>, collider: impl Into<Collider>) -> (RigidBodyHandle, ColliderHandle) {
        let b = self.bodies.insert(body);
        let c = self.colliders.insert_with_parent(collider, b, &mut self.bodies);
        (b, c)
    }

    pub fn insert_static(&mut self, collider: impl Into<Collider>) -> ColliderHandle {
        self.colliders.insert(collider)
    }

    pub fn remove_body(&mut self, h: RigidBodyHandle) {
        self.bodies.remove(
            h,
            &mut self.islands,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.soft_bodies,
            true,
        );
    }

    pub fn remove_collider(&mut self, h: ColliderHandle) {
        self.colliders.remove(h, &mut self.islands, &mut self.bodies, &mut self.soft_bodies, true);
    }

    pub fn query(&self) -> QueryPipeline<'_> {
        self.query_filtered(QueryFilter::default())
    }

    pub fn query_filtered<'a>(&'a self, filter: QueryFilter<'a>) -> QueryPipeline<'a> {
        self.broad_phase.as_query_pipeline(self.narrow_phase.query_dispatcher(), &self.bodies, &self.colliders, filter)
    }

    /// Wakes every dynamic body within `radius` of `center` (e.g. after removing the floor under them).
    pub fn wake_near(&mut self, center: Vector, radius: Real) {
        let handles: Vec<_> = self
            .bodies
            .iter()
            .filter(|(_, b)| b.is_dynamic() && (b.translation() - center).length() < radius)
            .map(|(h, _)| h)
            .collect();
        for h in handles {
            self.islands.wake_up(&mut self.bodies, h, true);
        }
    }
}

/// Collects contact force events above a threshold (used for impact sounds).
#[derive(Default)]
pub struct EventCollector {
    pub contacts: std::sync::Mutex<Vec<(ColliderHandle, ColliderHandle, Real)>>,
}

impl EventHandler for EventCollector {
    fn handle_collision_event(&self, _: &RigidBodySet, _: &ColliderSet, _: CollisionEvent, _: Option<&ContactPair>) {}
    fn handle_contact_force_event(
        &self,
        _dt: Real,
        _: &RigidBodySet,
        _: &ColliderSet,
        pair: &ContactPair,
        total_force_magnitude: Real,
    ) {
        if let Ok(mut v) = self.contacts.lock() {
            v.push((pair.collider1, pair.collider2, total_force_magnitude));
        }
    }
    fn handle_soft_body_tear_event(&self, _: &SoftBodySet, _: &SoftBodyTearEvent) {}
}
