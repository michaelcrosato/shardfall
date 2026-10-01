//! Joints between entities (or an entity and the world): hinges, ball joints, sliders, ropes and
//! springs. Joint definitions live on the entity that owns them, so they can be recreated when a
//! dormant region wakes up (rapier drops a body's joints when the body is removed).

use glam::{Quat, Vec3};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::entity::EntityId;
use crate::physics::PhysicsState;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JointKind {
    /// Glued together.
    Fixed,
    /// Ball joint: free rotation about the anchor.
    Ball,
    /// Hinge about `axis` (world/layout direction), with optional limits in degrees.
    Hinge {
        #[serde(default = "x_axis")]
        axis: Vec3,
        #[serde(default)]
        limits: Option<[f32; 2]>,
        /// Motor: target speed (deg/s) and strength.
        #[serde(default)]
        motor_speed: f32,
        #[serde(default)]
        motor_force: f32,
        /// Spring back to the start angle (self-closing doors); 0 = none.
        #[serde(default)]
        spring: f32,
        #[serde(default)]
        damping: f32,
    },
    /// Slider along `axis`, optional limits (m).
    Slider {
        #[serde(default = "y_axis")]
        axis: Vec3,
        #[serde(default)]
        limits: Option<[f32; 2]>,
        /// Spring back to the start position (stiffness, damping); 0 = free.
        #[serde(default)]
        spring: f32,
        #[serde(default)]
        damping: f32,
    },
    /// Keeps the anchors at most `length` apart.
    Rope { length: f32 },
    /// A spring between the anchors (rest length, stiffness, damping).
    Spring {
        length: f32,
        #[serde(default = "stiff")]
        stiffness: f32,
        #[serde(default = "damp")]
        damping: f32,
    },
}

fn x_axis() -> Vec3 {
    Vec3::X
}
fn y_axis() -> Vec3 {
    Vec3::Y
}
fn stiff() -> f32 {
    200.0
}
fn damp() -> f32 {
    8.0
}

/// A joint owned by an entity: to another entity, or to a fixed point in the world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointLink {
    /// The other entity (None = the world).
    pub other: Option<EntityId>,
    /// Anchor in the owner's local frame.
    pub local_a: Vec3,
    /// Anchor in the other's local frame (world position when `other` is None).
    pub local_b: Vec3,
    /// Joint axis in the owner's local frame (hinges, sliders).
    pub axis_a: Vec3,
    pub axis_b: Vec3,
    pub kind: JointKind,
}

impl JointLink {
    /// A joint whose anchor (and axis) are given in world space, for two bodies at their
    /// current poses.
    pub fn at(kind: JointKind, anchor: Vec3, a: (Vec3, Quat), b: Option<(EntityId, Vec3, Quat)>) -> Self {
        Self::between(kind, anchor, anchor, a, b)
    }

    /// Like `at`, with separate anchors on the two sides (a spring that starts stretched).
    pub fn between(kind: JointKind, anchor: Vec3, anchor_b: Vec3, a: (Vec3, Quat), b: Option<(EntityId, Vec3, Quat)>) -> Self {
        let axis = match &kind {
            JointKind::Hinge { axis, .. } | JointKind::Slider { axis, .. } => axis.normalize_or(Vec3::X),
            _ => Vec3::X,
        };
        let local_a = a.1.inverse() * (anchor - a.0);
        let axis_a = a.1.inverse() * axis;
        let (other, local_b, axis_b) = match b {
            Some((id, p, q)) => (Some(id), q.inverse() * (anchor_b - p), q.inverse() * axis),
            None => (None, anchor_b, axis),
        };
        Self { other, local_a, local_b, axis_a, axis_b, kind }
    }
}

fn generic(link: &JointLink) -> GenericJoint {
    let (a, b) = (link.local_a, link.local_b);
    match &link.kind {
        JointKind::Fixed => FixedJointBuilder::new().local_anchor1(a).local_anchor2(b).build().into(),
        JointKind::Ball => SphericalJointBuilder::new().local_anchor1(a).local_anchor2(b).build().into(),
        JointKind::Hinge { limits, motor_speed, motor_force, spring, damping, .. } => {
            let mut j = RevoluteJointBuilder::new(link.axis_a.normalize_or(Vec3::X))
                .local_anchor1(a)
                .local_anchor2(b);
            if let Some([lo, hi]) = limits {
                j = j.limits([lo.to_radians(), hi.to_radians()]);
            }
            if *spring > 0.0 {
                j = j.motor_position(0.0, *spring, *damping);
            } else if *motor_force > 0.0 {
                j = j.motor_velocity(motor_speed.to_radians(), *motor_force);
            }
            let mut g: GenericJoint = j.build().into();
            g.set_local_axis2(link.axis_b.normalize_or(Vec3::X));
            g
        }
        JointKind::Slider { limits, spring, damping, .. } => {
            let mut j = PrismaticJointBuilder::new(link.axis_a.normalize_or(Vec3::Y)).local_anchor1(a).local_anchor2(b);
            if let Some([lo, hi]) = limits {
                j = j.limits([*lo, *hi]);
            }
            if *spring > 0.0 {
                j = j.motor_position(0.0, *spring, *damping);
            }
            let mut g: GenericJoint = j.build().into();
            g.set_local_axis2(link.axis_b.normalize_or(Vec3::Y));
            g
        }
        JointKind::Rope { length } => RopeJointBuilder::new(*length).local_anchor1(a).local_anchor2(b).build().into(),
        JointKind::Spring { length, stiffness, damping } => {
            SpringJointBuilder::new(*length, *stiffness, *damping).local_anchor1(a).local_anchor2(b).build().into()
        }
    }
}

impl PhysicsState {
    /// A fixed body at the origin that world-anchored joints attach to (created on demand).
    pub fn world_anchor(&mut self) -> RigidBodyHandle {
        if let Some(h) = self.anchor.filter(|h| self.bodies.contains(*h)) {
            return h;
        }
        let h = self.bodies.insert(RigidBodyBuilder::fixed());
        self.anchor = Some(h);
        h
    }

    /// Creates the rapier joint for a link between `body` and `other` (None = world).
    pub fn insert_joint(&mut self, body: RigidBodyHandle, other: Option<RigidBodyHandle>, link: &JointLink) -> ImpulseJointHandle {
        match other {
            Some(b) => self.impulse_joints.insert(body, b, generic(link), true),
            None => {
                // World joints measure the body against the world, so limits and motors read
                // naturally (a vertical slider's +limit is up, a hinge turns about +axis).
                let flipped = JointLink {
                    other: None,
                    local_a: link.local_b,
                    local_b: link.local_a,
                    axis_a: link.axis_b,
                    axis_b: link.axis_a,
                    kind: link.kind.clone(),
                };
                let w = self.world_anchor();
                self.impulse_joints.insert(w, body, generic(&flipped), true)
            }
        }
    }
}
