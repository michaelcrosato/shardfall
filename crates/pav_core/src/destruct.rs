//! Floors that react: tiles that crumble after you step on them (and regrow), tiles that break
//! when something hits them hard enough, and the debris they leave. Also conveyors and bounce
//! pads acting on props (characters handle them in `character.rs`).

use glam::{Quat, Vec3};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::entity::{BodyKind, Spawn};
use crate::frame::SimEvent;
use crate::shape::{Shape, Visual};
use crate::sim::Sim;
use crate::statics::{BlockRef, block_flags};
use crate::zones::ZoneKind;

/// A tile that is about to fall, or waiting to come back.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Crumble {
    pub block: BlockRef,
    /// Seconds until it falls (or regrows once fallen).
    pub timer: f32,
    pub fallen: bool,
}

impl Sim {
    /// Breaks a block into debris (bombs, impacts, crumbling).
    pub(crate) fn shatter(&mut self, r: BlockRef, from: Vec3, pieces: u32) -> Option<(Vec3, Vec3, Color)> {
        let st = &mut self.state;
        let b = st.statics.destroy(&mut st.physics, r)?;
        let (c, half, color) = (b.center(), b.half(), b.color);
        for _ in 0..pieces {
            let rng = &mut self.state.rng;
            let off = Vec3::new(rng.range(-half.x, half.x), rng.range(-half.y, half.y), rng.range(-half.z, half.z));
            let size =
                Vec3::new(rng.range(0.08, 0.2), rng.range(0.06, 0.14), rng.range(0.08, 0.2)).min(half.max(Vec3::splat(0.05)));
            let out = (c + off - from).normalize_or(Vec3::Y);
            let vel = out * rng.range(1.5, 5.0) + Vec3::Y * rng.range(0.5, 3.0);
            let life = (rng.range(2.5, 4.0) * self.config.tick_rate.hz() as f32) as u32;
            let rot = Quat::from_euler(glam::EulerRot::XYZ, rng.range(0.0, 3.0), rng.range(0.0, 3.0), 0.0);
            let id = self.spawn(
                Spawn::new("~debris", c + off)
                    .visual(Visual::new(Shape::Box { half: size }, color))
                    .body(BodyKind::Dynamic)
                    .rot(rot),
            );
            if let Some(e) = self.state.entities.get_mut(id) {
                e.lifetime = Some(life);
                if let Some(bd) = e.body.and_then(|h| self.state.physics.bodies.get_mut(h)) {
                    bd.set_linvel(vel, true);
                }
            }
        }
        Some((c, half, color))
    }

    /// Crumbling tiles: start under characters, fall when their time is up, regrow later.
    pub(crate) fn update_crumbles(&mut self, dt: f32, events: &mut Vec<SimEvent>) {
        // Tiles under characters start crumbling.
        let player = self.state.player;
        let feet: Vec<(Vec3, RigidBodyHandle, bool)> = self
            .state
            .entities
            .iter()
            .filter_map(|e| Some((e.pos - Vec3::Y * e.character.as_ref()?.height() * 0.5, e.body?, Some(e.id) == player)))
            .collect();
        for (f, body, is_player) in feet {
            let filter = QueryFilter::default().exclude_rigid_body(body).exclude_sensors();
            let hit = {
                let qp = self.state.physics.query_filtered(filter);
                qp.cast_ray(&Ray::new(f + Vec3::Y * 0.05, Vec3::NEG_Y), 0.25, true).map(|(h, _)| h)
            };
            let Some(r) = hit.and_then(|h| self.state.physics.colliders.get(h)).and_then(|c| BlockRef::from_tag(c.user_data))
            else {
                continue;
            };
            let Some(b) = self.state.statics.get(r) else { continue };
            if b.flags & block_flags::PLAYER_CRUMBLE != 0 && !is_player {
                continue;
            }
            if b.crumble > 0.0 && b.alive && !self.state.crumbles.iter().any(|c| c.block == r) {
                let t = b.crumble;
                self.state.crumbles.push(Crumble { block: r, timer: t, fallen: false });
                self.state.statics.set_block_flags(r, block_flags::CRACKED, 0);
                events.push(SimEvent::Crack { pos: f });
            }
        }
        if self.state.crumbles.is_empty() {
            return;
        }
        let mut list = std::mem::take(&mut self.state.crumbles);
        list.retain_mut(|c| {
            c.timer -= dt;
            if c.timer > 0.0 {
                return true;
            }
            if !c.fallen {
                let regrow = self.state.statics.get(c.block).map(|b| b.regrow).unwrap_or(0.0);
                if let Some((center, half, color)) = self.state.statics.get(c.block).map(|b| (b.center(), b.half(), b.color)) {
                    let st = &mut self.state;
                    st.statics.destroy(&mut st.physics, c.block);
                    // The tile falls as one piece.
                    let id = self.spawn(
                        Spawn::new("~falling", center)
                            .visual(Visual::new(Shape::Box { half: half * 0.98 }, color))
                            .body(BodyKind::Dynamic),
                    );
                    if let Some(e) = self.state.entities.get_mut(id) {
                        e.lifetime = Some((3.0 * self.config.tick_rate.hz() as f32) as u32);
                    }
                    events.push(SimEvent::Break { pos: center });
                }
                c.fallen = true;
                c.timer = regrow;
                return regrow > 0.0;
            }
            // Regrow, unless something is in the way.
            let blocked = self.state.statics.get(c.block).is_some_and(|b| {
                let (lo, hi) = (b.min, b.max);
                self.state.entities.iter().any(|e| {
                    e.character.is_some() && e.pos.cmpge(lo - Vec3::splat(0.4)).all() && e.pos.cmple(hi + Vec3::splat(1.0)).all()
                })
            });
            if blocked {
                c.timer = 0.5;
                return true;
            }
            let st = &mut self.state;
            st.statics.restore(&mut st.physics, c.block);
            false
        });
        list.append(&mut self.state.crumbles);
        self.state.crumbles = list;
    }

    /// Tiles hit harder than their strength break (contact force events from the physics step).
    pub(crate) fn break_blocks(&mut self, contacts: Vec<(ColliderHandle, ColliderHandle, Real)>, events: &mut Vec<SimEvent>) {
        for (c1, c2, force) in contacts {
            for (h, other) in [(c1, c2), (c2, c1)] {
                let Some(r) = self.state.physics.colliders.get(h).and_then(|c| BlockRef::from_tag(c.user_data)) else { continue };
                let Some(b) = self.state.statics.get(r) else { continue };
                if b.strength <= 0.0 || !b.alive || (force as f32) < b.strength {
                    continue;
                }
                let regrow = b.regrow;
                let from = self.state.physics.colliders.get(other).map(|c| c.position().translation).unwrap_or(b.center());
                if let Some((center, _, _)) = self.shatter(r, from, 6) {
                    events.push(SimEvent::Break { pos: center });
                    if regrow > 0.0 {
                        self.state.crumbles.push(Crumble { block: r, timer: regrow, fallen: true });
                    }
                }
            }
        }
    }

    /// Conveyors move props lying on them; bounce pads launch them.
    pub(crate) fn zone_props(&mut self, dt: f32) {
        let zones: Vec<_> = self
            .state
            .statics
            .chunks
            .values()
            .flat_map(|c| c.zones.iter())
            .filter(|z| matches!(z.kind, ZoneKind::Conveyor | ZoneKind::Bounce))
            .cloned()
            .collect();
        if zones.is_empty() {
            return;
        }
        let ph = &mut self.state.physics;
        for z in zones {
            let aabb = rapier::parry::bounding_volume::Aabb::new(z.min, Vec3::new(z.max.x, z.min.y + 1.5, z.max.z));
            let bodies: Vec<RigidBodyHandle> = {
                let qp = ph.query_filtered(QueryFilter::only_dynamic().exclude_sensors());
                qp.intersect_aabb_conservative(aabb).filter_map(|(_, c)| c.parent()).collect()
            };
            for h in bodies {
                let Some(b) = ph.bodies.get_mut(h) else { continue };
                let p = b.translation();
                if !(z.contains(Vec3::new(p.x, z.min.y + 0.01, p.z)) && p.y < z.min.y + 1.5) {
                    continue;
                }
                let v = b.linvel();
                match z.kind {
                    ZoneKind::Conveyor => {
                        let target = z.conveyor_velocity();
                        let k = (8.0 * dt).min(1.0);
                        let nv = Vec3::new(v.x + (target.x - v.x) * k, v.y, v.z + (target.z - v.z) * k);
                        b.set_linvel(nv, true);
                    }
                    ZoneKind::Bounce => {
                        if v.y < 0.5 && p.y < z.min.y + 0.8 {
                            b.set_linvel(Vec3::new(v.x, z.speed, v.z), true);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
