//! Lightweight projectiles: plain structs moved by the simulation (no physics bodies), so
//! thousands are cheap. They stop at static geometry and hit characters (capsule test).

use glam::Vec3;
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::character::RADIUS;
use crate::color::Color;
use crate::entity::EntityId;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Projectile {
    pub pos: Vec3,
    pub vel: Vec3,
    pub radius: f32,
    /// Seconds left.
    pub life: f32,
    pub color: Color,
    pub knockback: f32,
    pub gravity: f32,
    /// The emitter (or character) that fired it; it never hits its owner.
    pub owner: Option<EntityId>,
    /// Player shots damage enemies (things with health); enemy shots hit characters.
    #[serde(default)]
    pub team: Team,
    #[serde(default = "one")]
    pub damage: f32,
}

fn one() -> f32 {
    1.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Team {
    #[default]
    Enemy,
    Player,
}

/// Something with health that player shots can hit: id, centre, radius.
#[derive(Clone, Copy, Debug)]
pub struct Enemy {
    pub id: EntityId,
    pub center: Vec3,
    pub radius: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Projectiles {
    pub list: Vec<Projectile>,
}

/// Hard cap so a runaway emitter can't eat the machine.
pub const MAX_PROJECTILES: usize = 6000;

/// A character's capsule for hit tests: feet position, height.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub id: EntityId,
    pub feet: Vec3,
    pub height: f32,
    /// Hit radius (a small hitbox makes dense bullet patterns fair).
    pub radius: f32,
}

/// What happened to projectiles this tick.
#[derive(Default)]
pub struct ProjectileStep {
    pub hits: Vec<(EntityId, Vec3, Vec3, f32)>, // target, position, direction, knockback
    /// Enemies hit by player shots: enemy, position, damage.
    pub damage: Vec<(EntityId, Vec3, f32)>,
    pub impacts: usize,
}

fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

impl Projectiles {
    pub fn spawn(&mut self, p: Projectile) {
        if self.list.len() < MAX_PROJECTILES {
            self.list.push(p);
        }
    }

    /// Moves every projectile; removes the ones that hit geometry, characters or expire.
    /// `blocked(from, dir, len)` casts against static geometry.
    pub fn step(
        &mut self,
        dt: f32,
        targets: &[Target],
        enemies: &[Enemy],
        mut blocked: impl FnMut(Vec3, Vec3, f32, Option<EntityId>) -> bool,
    ) -> ProjectileStep {
        let mut out = ProjectileStep::default();
        self.list.retain_mut(|p| {
            p.life -= dt;
            if p.life <= 0.0 {
                return false;
            }
            p.vel.y -= p.gravity * dt;
            let step = p.vel * dt;
            let len = step.length();
            let next = p.pos + step;
            if p.team == Team::Player {
                for e in enemies {
                    if Some(e.id) == p.owner {
                        continue;
                    }
                    if segment_distance(e.center, p.pos, next) < e.radius + p.radius {
                        out.damage.push((e.id, next, p.damage));
                        return false;
                    }
                }
            } else {
                for t in targets {
                    if Some(t.id) == p.owner {
                        continue;
                    }
                    let a = t.feet + Vec3::Y * RADIUS;
                    let b = t.feet + Vec3::Y * (t.height - RADIUS).max(RADIUS);
                    // Test the midpoint and the end so fast bullets don't skip through.
                    let d = segment_distance(next, a, b).min(segment_distance(p.pos + step * 0.5, a, b));
                    if d < t.radius + p.radius {
                        out.hits.push((t.id, next, p.vel.normalize_or(Vec3::X), p.knockback));
                        return false;
                    }
                }
            }
            if len > 1e-6 && blocked(p.pos, step / len, len + p.radius * 0.5, p.owner) {
                out.impacts += 1;
                return false;
            }
            p.pos = next;
            true
        });
        out
    }
}

/// Ray test against fixed geometry (static blocks, terrain, fixed bodies) only, ignoring the
/// shooter's own body (a turret).
pub fn static_blocked(
    physics: &crate::physics::PhysicsState,
    from: Vec3,
    dir: Vec3,
    len: f32,
    ignore: Option<RigidBodyHandle>,
) -> bool {
    let mut filter = QueryFilter::only_fixed().exclude_sensors();
    if let Some(b) = ignore {
        filter = filter.exclude_rigid_body(b);
    }
    let qp = physics.query_filtered(filter);
    let ray = Ray::new(from, dir);
    qp.cast_ray(&ray, len as Real, true).is_some()
}

/// Distance along `dir` to fixed geometry within `len`, if any.
pub fn static_hit(
    physics: &crate::physics::PhysicsState,
    from: Vec3,
    dir: Vec3,
    len: f32,
    ignore: Option<RigidBodyHandle>,
) -> Option<f32> {
    let mut filter = QueryFilter::only_fixed().exclude_sensors();
    if let Some(b) = ignore {
        filter = filter.exclude_rigid_body(b);
    }
    let qp = physics.query_filtered(filter);
    qp.cast_ray(&Ray::new(from, dir), len as Real, true).map(|(_, t)| t as f32)
}
