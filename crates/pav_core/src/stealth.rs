//! Stealth: guards see the player within their range and view angle when nothing blocks the
//! line of sight (crouching shortens the range). An alert meter fills while the player is seen;
//! a full meter sends the player back to the last checkpoint. Vision cones (clipped by walls) are
//! handed to the view.

use glam::Vec3;
use rapier::prelude::*;

use crate::ai::AiDef;
use crate::character::Posture;
use crate::frame::SimEvent;
use crate::sim::Sim;

/// A guard's vision cone for drawing: apex, wall-clipped edge points, alert (0..1).
#[derive(Clone, Debug, PartialEq)]
pub struct ConeView {
    pub origin: Vec3,
    pub points: Vec<Vec3>,
    pub alert: f32,
}

const EYE: f32 = 1.3;

impl Sim {
    pub(crate) fn update_guards(&mut self, dt: f32, events: &mut Vec<SimEvent>) {
        let Some(pid) = self.state.player else { return };
        let Some(p) = self.state.entities.get(pid) else { return };
        let Some(pch) = p.character.as_ref() else { return };
        if pch.riding.is_some() {
            return;
        }
        let low = pch.posture != Posture::Stand;
        let target = p.pos + Vec3::Y * (pch.height() * 0.25);
        let pbody = p.body;
        let time = self.state.tick as f32 * dt;
        let guards: Vec<_> = self
            .state
            .entities
            .iter()
            .filter_map(|e| match &e.ai.as_ref()?.def {
                AiDef::Guard { range, angle, .. } => {
                    let ch = e.character.as_ref()?;
                    let feet = e.pos - Vec3::Y * ch.height() * 0.5;
                    Some((e.id, feet + Vec3::Y * EYE, ch.facing, *range, *angle, e.body))
                }
                _ => None,
            })
            .collect();
        let mut caught = None;
        for (id, eye, facing, range, angle, body) in guards {
            let to = target - eye;
            let flat = Vec3::new(to.x, 0.0, to.z);
            let reach = range * if low { 0.6 } else { 1.0 };
            let fwd = Vec3::new(facing.sin(), 0.0, facing.cos());
            let mut seen = flat.length() < reach && fwd.dot(flat.normalize_or(fwd)) >= (angle.to_radians() * 0.5).cos();
            if seen {
                // (A filter excludes one body only: the ray starts outside the guard's capsule
                // and ignores the player's.)
                let _ = body;
                let mut filter = QueryFilter::default().exclude_sensors();
                if let Some(b) = pbody {
                    filter = filter.exclude_rigid_body(b);
                }
                let dist = to.length();
                let dir = to / dist.max(1e-4);
                let start = 0.45;
                let qp = self.state.physics.query_filtered(filter);
                if dist > start
                    && qp.cast_ray(&Ray::new(eye + dir * start, dir), (dist - start - 0.2).max(0.0) as Real, true).is_some()
                {
                    seen = false;
                }
            }
            let Some(e) = self.state.entities.get_mut(id) else { continue };
            let Some(ai) = e.ai.as_mut() else { continue };
            ai.sees = seen;
            ai.alert = if seen { ai.alert + dt / 0.4 } else { (ai.alert - dt * 0.6).max(0.0) };
            // Look around while standing at a patrol stop.
            if ai.wait > 0.0 && !seen {
                if let Some(ch) = e.character.as_mut() {
                    ch.facing = ai.look + (time * 1.7 + id.0 as f32).sin() * 0.9;
                }
            }
            if ai.alert >= 1.0 {
                caught = Some(eye);
            }
        }
        if let Some(at) = caught {
            for e in self.state.entities.map.values_mut() {
                if let Some(ai) = e.ai.as_mut() {
                    ai.alert = 0.0;
                }
            }
            events.push(SimEvent::Spotted { pos: at });
            self.state.courses.message = Some(("SPOTTED!".into(), self.state.tick));
            self.respawn_player(events);
        }
    }

    /// A guard's vision cone, clipped by walls (for the view).
    pub(crate) fn guard_cone(&self, e: &crate::entity::Entity) -> Option<ConeView> {
        let ai = e.ai.as_ref()?;
        let AiDef::Guard { range, angle, .. } = &ai.def else { return None };
        let ch = e.character.as_ref()?;
        let feet = e.pos - Vec3::Y * ch.height() * 0.5;
        let eye = feet + Vec3::Y * 0.6;
        let half = angle.to_radians() * 0.5;
        let n = 20;
        let mut filter = QueryFilter::only_fixed().exclude_sensors();
        if let Some(b) = e.body {
            filter = filter.exclude_rigid_body(b);
        }
        let qp = self.state.physics.query_filtered(filter);
        let points = (0..=n)
            .map(|i| {
                let a = ch.facing - half + 2.0 * half * i as f32 / n as f32;
                let d = Vec3::new(a.sin(), 0.0, a.cos());
                let len = qp.cast_ray(&Ray::new(eye, d), *range as Real, true).map(|(_, t)| t as f32).unwrap_or(*range);
                feet + Vec3::Y * 0.04 + d * len
            })
            .collect();
        Some(ConeView { origin: feet + Vec3::Y * 0.04, points, alert: ai.alert.min(1.0) })
    }
}
