//! Shootable things: enemies and bosses lose health to player shots, flash when hit, sway side to
//! side, switch attack patterns as their health drops, and on death add score, send a signal and
//! can finish the running course.

use glam::Vec3;

use crate::entity::EntityId;
use crate::frame::SimEvent;
use crate::sim::Sim;

impl Sim {
    /// Sway and hit flash of everything with health.
    pub(crate) fn update_health(&mut self, dt: f32) {
        let ph = &mut self.state.physics;
        for e in self.state.entities.map.values_mut() {
            let Some(h) = e.health.as_mut() else { continue };
            h.flash = (h.flash - dt).max(0.0);
            h.time += dt;
            let [amp, period] = h.def.sway;
            if amp > 0.0 {
                let p = h.home + h.axis * amp * (h.time * std::f32::consts::TAU / period.max(0.1)).sin();
                e.pos = p;
                if let Some(b) = e.body.and_then(|b| ph.bodies.get_mut(b)) {
                    if b.is_kinematic() {
                        b.set_next_kinematic_translation(p);
                    } else {
                        b.set_translation(p, true);
                    }
                }
            }
        }
    }

    /// A player shot hit something with health.
    pub(crate) fn damage(&mut self, id: EntityId, at: Vec3, amount: f32, events: &mut Vec<SimEvent>) {
        let Some(e) = self.state.entities.get_mut(id) else { return };
        let pos = e.pos;
        let size = e.visual.as_ref().map(|v| v.shape.half_extents().max_element()).unwrap_or(0.5);
        let Some(h) = e.health.as_mut() else { return };
        h.hp -= amount;
        h.flash = 0.08;
        events.push(SimEvent::Damage { pos: at });
        // Attack patterns change as health falls.
        let frac = h.fraction();
        while let Some(p) = h.def.phases.get(h.phase) {
            if frac >= p.below {
                break;
            }
            e.behavior = p.behavior.clone();
            h.phase += 1;
        }
        if h.hp > 0.0 {
            return;
        }
        let def = h.def.clone();
        self.despawn(id);
        events.push(SimEvent::Destroyed { pos, size });
        if let Some(r) = &mut self.state.courses.run {
            r.score += def.score;
        }
        if !def.signal.is_empty() {
            self.state.signals.push(def.signal.clone());
        }
        if def.finish && self.state.courses.run.as_ref().is_some_and(|r| r.started) {
            self.finish_course(pos, events);
        }
    }

    /// The boss bar for the HUD: name and health fraction of the first `bar` enemy.
    pub fn boss_bar(&self) -> Option<(String, f32)> {
        // Only the room you are in (other rooms' bosses live on in the world).
        let here = self.state.world.current_room.map(crate::statics::RegionKey::Room);
        self.state.entities.iter().find_map(|e| {
            let h = e.health.as_ref()?;
            let near = !self.state.world.enabled || e.region.is_none() || e.region == here;
            (h.def.bar && near).then(|| (e.name.clone(), h.fraction()))
        })
    }
}
