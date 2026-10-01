//! Scripted entity behaviours: spinning, prop rain, movers (platforms, doors, crushers),
//! rotators (sweepers, pendulums) and projectile emitters. Motion is a function of the tick,
//! so it replays exactly after a rewind.

use glam::{Quat, Vec3};
use rapier::prelude::*;

use crate::color::Color;
use crate::entity::{Behavior, BodyKind, EmitterDef, EntityId, Pattern, Spawn};
use crate::projectile::Projectile;
use crate::shape::{Shape, Visual};
use crate::sim::Sim;

impl Sim {
    /// Drives kinematic bodies to `pos`/`rot` over the next physics step (or moves visual-only
    /// entities directly).
    fn drive(&mut self, id: EntityId, pos: Vec3, rot: Quat, dt: f32) {
        let Some(e) = self.state.entities.get_mut(id) else { return };
        match e.body.and_then(|h| self.state.physics.bodies.get_mut(h)) {
            Some(b) if !b.is_dynamic() => {
                let cur = b.translation();
                let cur_rot = *b.rotation();
                b.set_linvel((pos - cur) / dt, true);
                let dq = rot * cur_rot.inverse();
                let (axis, mut angle) = dq.to_axis_angle();
                if angle > std::f32::consts::PI {
                    angle -= std::f32::consts::TAU;
                }
                b.set_angvel(if angle.abs() > 1e-6 { axis * (angle / dt) } else { Vector::ZERO }, true);
            }
            Some(_) => {}
            None => {
                e.pos = pos;
                e.rot = rot;
            }
        }
    }

    pub(crate) fn run_behaviors(&mut self, dt: f32) {
        let ids: Vec<EntityId> =
            self.state.entities.iter().filter(|e| !matches!(e.behavior, Behavior::None)).map(|e| e.id).collect();
        // Time at the end of this tick: movers arrive there after the physics step.
        let t = (self.state.tick + 1) as f32 * dt;
        let signals = std::mem::take(&mut self.state.signals);
        let player = self.player().map(|p| p.pos);
        for id in ids {
            let Some(e) = self.state.entities.get(id) else { continue };
            let (epos, erot) = (e.pos, e.rot);
            match e.behavior.clone() {
                Behavior::None => {}
                Behavior::Spin { speed } => {
                    if let Some(b) = e.body.and_then(|h| self.state.physics.bodies.get_mut(h)) {
                        b.set_angvel(Vector::new(0.0, speed as Real, 0.0), true);
                    } else if let Some(e) = self.state.entities.get_mut(id) {
                        e.rot = Quat::from_rotation_y(speed * dt) * e.rot;
                    }
                }
                Behavior::Move(mut m) => {
                    let (o, q) = *m.origin.get_or_insert((epos, erot));
                    let target = o + q * m.offset * m.amount(t);
                    self.drive(id, target, q, dt);
                    if let Some(e) = self.state.entities.get_mut(id) {
                        e.behavior = Behavior::Move(m);
                    }
                }
                Behavior::Rotate(mut r) => {
                    let (o, q0) = *r.origin.get_or_insert((epos, erot));
                    let axis = (q0 * r.axis).normalize_or(Vec3::Y);
                    let q = Quat::from_axis_angle(axis, r.angle(t));
                    let pivot = o + q0 * r.pivot;
                    let pos = pivot + q * (o - pivot);
                    self.drive(id, pos, q * q0, dt);
                    if let Some(e) = self.state.entities.get_mut(id) {
                        e.behavior = Behavior::Rotate(r);
                    }
                }
                Behavior::Emitter(mut em) => {
                    let near = player.is_some_and(|p| p.distance(epos) < em.range);
                    em.timer += dt;
                    if near && em.timer >= em.delay {
                        let wait = if em.burst > 0 && em.shots > 0 && em.shots % em.burst == 0 { em.pause } else { 0.0 };
                        if em.timer - em.delay >= em.interval + wait {
                            em.timer = em.delay;
                            self.fire(id, epos, erot, &mut em, player);
                        }
                    } else if !near {
                        em.timer = em.timer.min(em.delay);
                    }
                    if let Some(e) = self.state.entities.get_mut(id) {
                        e.behavior = Behavior::Emitter(em);
                    }
                }
                Behavior::Spawner(mut sp) => {
                    let region = e.region;
                    if !sp.clear.is_empty() && signals.contains(&sp.clear) {
                        for old in std::mem::take(&mut sp.spawned) {
                            self.despawn(old);
                        }
                        sp.pending = 0;
                    }
                    if !sp.signal.is_empty() && signals.contains(&sp.signal) {
                        sp.pending += sp.count;
                    }
                    if sp.interval > 0.0 {
                        sp.timer += dt;
                        if sp.timer >= sp.interval {
                            sp.timer -= sp.interval;
                            sp.pending += sp.count;
                        }
                    }
                    // Spread big drops over several ticks.
                    let n = sp.pending.min(25);
                    sp.pending -= n;
                    for _ in 0..n {
                        let rng = &mut self.state.rng;
                        let off = Vec3::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)) * sp.area;
                        let color = if sp.color.is_empty() {
                            let palette = ["#e8704a", "#f2c14e", "#5b8def", "#9b5de5", "#3bb273", "#f15bb5", "#00bbf9"];
                            Color::hex(palette[rng.below(palette.len() as u32) as usize])
                        } else {
                            Color::hex(&sp.color)
                        };
                        let rot = if sp.tumble {
                            Quat::from_euler(glam::EulerRot::XYZ, rng.range(0.0, 3.1), rng.range(0.0, 3.1), rng.range(0.0, 3.1))
                        } else {
                            erot
                        };
                        let mut v = Visual::new(sp.shape, color);
                        v.look = sp.look;
                        let mut s = Spawn::new("spawned", epos + erot * off)
                            .visual(v)
                            .body(BodyKind::Dynamic)
                            .rot(rot)
                            .density(sp.density)
                            .friction(sp.friction)
                            .restitution(sp.restitution);
                        s.region = region;
                        let new_id = self.spawn(s);
                        sp.spawned.push_back(new_id);
                    }
                    while sp.spawned.len() > sp.max as usize {
                        if let Some(old) = sp.spawned.pop_front() {
                            self.despawn(old);
                        }
                    }
                    if let Some(e) = self.state.entities.get_mut(id) {
                        e.behavior = Behavior::Spawner(sp);
                    }
                }
                Behavior::Rain { interval, max, area, height, mut timer, mut spawned } => {
                    let origin = e.pos;
                    timer += 1;
                    if timer >= interval {
                        timer = 0;
                        let rng = &mut self.state.rng;
                        let pos = origin + Vec3::new(rng.range(-area, area), height, rng.range(-area, area));
                        let palette = ["#e8704a", "#f2c14e", "#5b8def", "#9b5de5", "#3bb273", "#f15bb5", "#00bbf9"];
                        let color = Color::hex(palette[rng.below(palette.len() as u32) as usize]);
                        let shape = match rng.below(4) {
                            0 => Shape::Box { half: Vec3::splat(rng.range(0.2, 0.45)) },
                            1 => Shape::Sphere { radius: rng.range(0.2, 0.45) },
                            2 => Shape::Capsule { half_height: rng.range(0.15, 0.35), radius: rng.range(0.15, 0.3) },
                            _ => Shape::RoundedBox { half: Vec3::new(0.45, 0.25, 0.3), radius: 0.1 },
                        };
                        let rot = Quat::from_euler(glam::EulerRot::XYZ, rng.range(0.0, 3.0), rng.range(0.0, 3.0), 0.0);
                        let bounce = rng.range(0.0, 0.5);
                        let new_id = self.spawn(
                            Spawn::new("rain", pos)
                                .visual(Visual::new(shape, color))
                                .body(BodyKind::Dynamic)
                                .rot(rot)
                                .restitution(bounce),
                        );
                        spawned.push_back(new_id);
                        while spawned.len() > max as usize {
                            if let Some(old) = spawned.pop_front() {
                                self.despawn(old);
                            }
                        }
                    }
                    if let Some(e) = self.state.entities.get_mut(id) {
                        e.behavior = Behavior::Rain { interval, max, area, height, timer, spawned };
                    }
                }
            }
        }
    }

    /// One shot of an emitter.
    fn fire(&mut self, id: EntityId, pos: Vec3, rot: Quat, em: &mut EmitterDef, player: Option<Vec3>) {
        em.shots += 1;
        let muzzle = pos + rot * Vec3::Y * em.height;
        let fwd = rot * Vec3::Z;
        let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).normalize_or(Vec3::Z);
        let base = match em.pattern {
            Pattern::Aimed => player.map(|p| flat(p + Vec3::Y * 0.9 - muzzle)).unwrap_or(flat(fwd)),
            _ => flat(fwd),
        };
        let yaw0 = base.x.atan2(base.z);
        let n = em.count.max(1);
        let color = Color::hex(&em.color);
        let mut dirs = Vec::with_capacity(n as usize);
        match em.pattern {
            Pattern::Ring | Pattern::Spiral => {
                if em.pattern == Pattern::Spiral {
                    em.angle += em.spin;
                }
                for i in 0..n {
                    let a = yaw0 + (em.angle + 360.0 * i as f32 / n as f32).to_radians();
                    dirs.push(Vec3::new(a.sin(), 0.0, a.cos()));
                }
            }
            Pattern::Random => {
                for _ in 0..n {
                    let a = yaw0 + self.state.rng.range(-0.5, 0.5) * em.spread.to_radians();
                    dirs.push(Vec3::new(a.sin(), 0.0, a.cos()));
                }
            }
            Pattern::Aimed | Pattern::Forward => {
                for i in 0..n {
                    let f = if n == 1 { 0.0 } else { i as f32 / (n - 1) as f32 - 0.5 };
                    let a = yaw0 + (f * em.spread).to_radians();
                    dirs.push(Vec3::new(a.sin(), 0.0, a.cos()));
                }
            }
        }
        for d in dirs {
            self.state.projectiles.spawn(Projectile {
                pos: muzzle + d * 0.3,
                vel: d * em.speed,
                radius: em.radius,
                life: em.life,
                color,
                knockback: em.knockback,
                gravity: em.gravity,
                owner: Some(id),
            });
        }
    }
}
