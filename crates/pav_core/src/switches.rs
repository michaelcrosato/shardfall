//! Mission systems for room runs: uplinks, switches, the alarm, pickups and a crew that walks
//! the player's path.
//!
//! - Uplinks are `hack` zones: standing inside fills them over the zone's `time` (scaled by
//!   `hack.speed`, so rigs/pads can make faster deckers). Getting hit while jacked in drops the
//!   trace back to zero, and so does a respawn. A full uplink sends its signal and pays its score,
//!   once.
//! - Switches: objects and characters with `off = [signals]` shut down once every listed signal
//!   has been heard (turrets go dark, cameras go blind, laser grates drop); ones with
//!   `on = [signals]` are held back until then and come online (lockdowns, reinforcements, loot
//!   that spills out of a cracked cache).
//! - Guards that spot the player send the `spotted` signal, so a room can escalate.
//! - Pickups are collected by walking over them: points and a HUD message.
//! - Followers walk the trail the player walked (round corners, through doors) instead of a
//!   straight line, hurry when they fall behind and regroup at checkpoints.

use std::collections::VecDeque;

use glam::Vec3;
use serde::{Deserialize, Deserializer, Serialize};

use crate::entity::EntityId;
use crate::frame::SimEvent;
use crate::params::{ParamVisitor, Tunable};
use crate::room::{NpcDef, ObjectDef};
use crate::sim::Sim;
use crate::statics::{RegionKey, ZoneRef};
use crate::zones::ZoneKind;

/// Sent when a guard raises the alarm.
pub const SPOTTED: &str = "spotted";
/// Distance between trail points (m) and how many are kept.
const TRAIL_STEP: f32 = 0.5;
const TRAIL_MAX: usize = 160;

/// Uplink tuning (`hack.*` parameters; pads can change them, e.g. a decker rig).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HackParams {
    /// Hacking speed multiplier.
    pub speed: f32,
    /// Share of an unfinished uplink lost per second while nobody stands in it.
    pub decay: f32,
}

impl Default for HackParams {
    fn default() -> Self {
        Self { speed: 1.0, decay: 0.0 }
    }
}

impl Tunable for HackParams {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.float("speed", &mut self.speed, 0.1, 10.0, "Uplink hacking speed multiplier");
        v.float("decay", &mut self.decay, 0.0, 2.0, "Unfinished uplink progress lost per second while outside");
    }
}

/// A signal list in room files: `off = "a"` or `off = ["a", "b"]` (all of them are needed).
pub fn one_or_many<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) if s.is_empty() => Vec::new(),
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    })
}

/// What a live object or character listens for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Switch {
    /// Shut down once all of these were heard.
    pub off: Vec<String>,
    /// Signals it came online on (kept so edit mode writes it back as authored).
    pub on: Vec<String>,
    /// Signals of `off` heard so far.
    pub heard: Vec<String>,
}

/// Walk over it to collect it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PickupDef {
    /// Points added to the running course.
    pub score: u32,
    /// HUD message ("+25 CREDSTICK").
    pub label: String,
    /// Pick-up distance from the player's centre (m, horizontal).
    pub radius: f32,
}

impl Default for PickupDef {
    fn default() -> Self {
        Self { score: 25, label: "CREDSTICK".into(), radius: 0.8 }
    }
}

/// Something a room holds back until its signals were heard.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ArmedThing {
    Object(Box<ObjectDef>),
    Npc(Box<NpcDef>),
    /// Story text revealed by a signal.
    Label(Box<crate::level::LabelDef>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Armed {
    pub region: RegionKey,
    pub on: Vec<String>,
    pub heard: Vec<String>,
    pub thing: ArmedThing,
}

/// One uplink's progress (0..1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hack {
    pub zone: ZoneRef,
    pub progress: f32,
    pub done: bool,
}

/// Mission state of the rooms (part of the snapshot).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Switchboard {
    pub hacks: Vec<Hack>,
    pub armed: Vec<Armed>,
    /// The player's recent path (feet), oldest first; `trail_base` numbers `trail[0]`.
    pub trail: VecDeque<Vec3>,
    pub trail_base: u64,
}

impl Switchboard {
    /// Forgets a region's uplinks and held-back things (the room is being rebuilt).
    pub fn clear_region(&mut self, region: RegionKey) {
        self.hacks.retain(|h| h.zone.region != region);
        self.armed.retain(|a| a.region != region);
    }

    fn record(&mut self, feet: Vec3) {
        if self.trail.back().is_none_or(|p| p.distance(feet) >= TRAIL_STEP) {
            self.trail.push_back(feet);
            while self.trail.len() > TRAIL_MAX {
                self.trail.pop_front();
                self.trail_base += 1;
            }
        }
    }

    /// Where a follower heading for trail point `seq` should walk to, keeping `distance` behind
    /// the player along the path. Skips ahead when a later point is closer (the player doubled
    /// back) and jumps onto the trail when it lost it.
    pub fn follow_goal(&self, seq: &mut u64, feet: Vec3, distance: f32) -> Option<Vec3> {
        let n = self.trail.len() as u64;
        if n == 0 {
            return None;
        }
        let base = self.trail_base;
        let keep = (distance / TRAIL_STEP).ceil() as u64;
        let last = (base + n - 1).saturating_sub(keep).max(base);
        let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
        let at = |s: u64| self.trail[(s - base) as usize];
        if *seq < base || *seq > last {
            // Lost (fell off the back or the trail restarted): the nearest point.
            *seq = (base..=last).min_by(|a, b| flat(at(*a), feet).total_cmp(&flat(at(*b), feet))).unwrap_or(base);
        }
        // Shortcut to the closest point ahead (a short window, so it never cuts through walls
        // much further along).
        let ahead = (*seq..=last.min(*seq + 12)).min_by(|a, b| flat(at(*a), feet).total_cmp(&flat(at(*b), feet)).then(b.cmp(a)));
        if let Some(s) = ahead {
            *seq = s;
        }
        while *seq < last && flat(at(*seq), feet) < 0.6 {
            *seq += 1;
        }
        Some(at(*seq))
    }
}

/// The uplink the player stands in, for the HUD.
#[derive(Clone, Debug, PartialEq)]
pub struct UplinkHud {
    pub label: String,
    pub progress: f32,
    pub done: bool,
}

fn all_heard(want: &[String], heard: &[String]) -> bool {
    want.iter().all(|w| heard.contains(w))
}

fn hear(want: &[String], heard: &mut Vec<String>, signals: &[String]) -> bool {
    let mut new = false;
    for s in signals {
        if want.contains(s) && !heard.contains(s) {
            heard.push(s.clone());
            new = true;
        }
    }
    new
}

impl Sim {
    /// Delivers this tick's signals to switches and held-back things.
    pub(crate) fn run_switches(&mut self, signals: &[String], events: &mut Vec<SimEvent>) {
        if signals.is_empty() {
            return;
        }
        let mut off = Vec::new();
        for e in self.state.entities.map.values_mut() {
            let Some(sw) = e.switch.as_mut() else { continue };
            if sw.off.is_empty() {
                continue;
            }
            if hear(&sw.off, &mut sw.heard, signals) && all_heard(&sw.off, &sw.heard) {
                let size = e.visual.as_ref().map(|v| v.shape.half_extents().max_element()).unwrap_or(0.5);
                off.push((e.id, e.pos, size));
            }
        }
        for (id, pos, size) in off {
            self.despawn(id);
            events.push(SimEvent::Switched { pos, on: false, size });
        }
        let mut ready = Vec::new();
        let mut i = 0;
        while i < self.state.switchboard.armed.len() {
            let a = &mut self.state.switchboard.armed[i];
            if hear(&a.on, &mut a.heard, signals) && all_heard(&a.on, &a.heard) {
                ready.push(self.state.switchboard.armed.remove(i));
            } else {
                i += 1;
            }
        }
        for a in ready {
            let RegionKey::Room(room) = a.region else { continue };
            let Some(slot) = self.state.world.rooms.get(room as usize).cloned() else { continue };
            let (id, size) = match &a.thing {
                ArmedThing::Object(o) => {
                    let size = o.shape.half_extents().max_element();
                    (self.spawn_room_object(&slot, o, a.region), size)
                }
                ArmedThing::Npc(n) => match self.spawn_room_npc(&slot, n, a.region) {
                    Some(id) => (id, 0.6),
                    None => continue,
                },
                ArmedThing::Label(l) => {
                    let Some(p) = l.pos else { continue };
                    let label = l.to_label(&slot.place, p);
                    let pos = label.pos;
                    self.state.statics.add_label_to(a.region, label);
                    self.state.courses.message = Some((l.text.clone(), self.state.tick));
                    events.push(SimEvent::Pad { pos });
                    continue;
                }
            };
            if let Some(e) = self.state.entities.get(id) {
                events.push(SimEvent::Switched { pos: e.pos, on: true, size });
            }
        }
    }

    /// Fills the uplinks the player stands in; completes them; lets abandoned ones decay.
    pub(crate) fn update_uplinks(&mut self, dt: f32, events: &mut Vec<SimEvent>) {
        let inside: Vec<ZoneRef> = self
            .state
            .courses
            .inside
            .iter()
            .copied()
            .filter(|r| self.state.statics.zone(*r).is_some_and(|z| z.kind == ZoneKind::Hack))
            .collect();
        let speed = self.config.hack.speed.max(0.0);
        let decay = self.config.hack.decay.max(0.0);
        for h in &mut self.state.switchboard.hacks {
            if !h.done && !inside.contains(&h.zone) {
                h.progress = (h.progress - decay * dt).max(0.0);
            }
        }
        for r in inside {
            let Some(z) = self.state.statics.zone(r).cloned() else { continue };
            let hacks = &mut self.state.switchboard.hacks;
            let i = match hacks.iter().position(|h| h.zone == r) {
                Some(i) => i,
                None => {
                    hacks.push(Hack { zone: r, progress: 0.0, done: false });
                    hacks.len() - 1
                }
            };
            let h = &mut hacks[i];
            if h.done {
                continue;
            }
            let time = if z.time > 0.0 { z.time } else { 3.0 };
            h.progress += dt * speed / time;
            if h.progress < 1.0 {
                continue;
            }
            h.progress = 1.0;
            h.done = true;
            if !z.signal.is_empty() {
                self.state.signals.push(z.signal.clone());
            }
            if let Some(run) = &mut self.state.courses.run {
                run.score += z.score;
            }
            let name = if z.label.is_empty() { "UPLINK".to_string() } else { z.label.clone() };
            let msg = if z.score > 0 { format!("HACKED · {name} · +{}", z.score) } else { format!("HACKED · {name}") };
            self.state.courses.message = Some((msg, self.state.tick));
            events.push(SimEvent::Hacked { pos: z.floor_center() });
        }
    }

    /// The player got hit: unfinished uplinks they stand in lose their trace.
    pub(crate) fn trace_lost(&mut self, events: &mut Vec<SimEvent>) {
        let inside = &self.state.courses.inside;
        let mut lost = None;
        for h in &mut self.state.switchboard.hacks {
            if !h.done && h.progress > 0.0 && inside.contains(&h.zone) {
                h.progress = 0.0;
                lost = Some(h.zone);
            }
        }
        if let Some(r) = lost {
            let pos = self.state.statics.zone(r).map(|z| z.floor_center()).unwrap_or_default();
            self.state.courses.message = Some(("TRACE LOST".into(), self.state.tick));
            events.push(SimEvent::TraceLost { pos });
        }
    }

    /// After a respawn: unfinished uplinks start over, and the crew regroups at the player.
    pub(crate) fn mission_respawn(&mut self, at: Vec3, yaw: f32) {
        for h in &mut self.state.switchboard.hacks {
            if !h.done {
                h.progress = 0.0;
            }
        }
        let sb = &mut self.state.switchboard;
        sb.trail_base += sb.trail.len() as u64;
        sb.trail.clear();
        sb.trail.push_back(at);
        self.regroup_followers(at, yaw);
    }

    /// Puts followers in a short line behind `at` (facing `yaw`).
    pub fn regroup_followers(&mut self, at: Vec3, yaw: f32) {
        let back = -Vec3::new(yaw.sin(), 0.0, yaw.cos());
        let side = Vec3::new(back.z, 0.0, -back.x);
        let ids: Vec<EntityId> = self.followers_here().into_iter().map(|(id, _)| id).collect();
        for (i, id) in ids.into_iter().enumerate() {
            let row = (i / 2) as f32;
            let lr = if i % 2 == 0 { -0.45 } else { 0.45 };
            self.set_position(id, at + back * (0.9 + row * 0.8) + side * lr);
            if let Some(ai) = self.state.entities.get_mut(id).and_then(|e| e.ai.as_mut()) {
                ai.stuck = 0.0;
                ai.seq = 0;
            }
        }
    }

    /// Records the player's path for followers; teleports followers that got stuck or left far
    /// behind onto the trail.
    pub(crate) fn update_crew(&mut self) {
        let Some(p) = self.player() else { return };
        let Some(ch) = p.character.as_ref() else { return };
        let feet = p.pos - Vec3::Y * ch.height() * 0.5;
        if ch.riding.is_none() {
            self.state.switchboard.record(feet);
        }
        let mut moves = Vec::new();
        for (id, f) in self.followers_here() {
            let Some(ai) = self.state.entities.get(id).and_then(|e| e.ai.as_ref()) else { continue };
            if ai.stuck > 1.5 || f.distance(feet) > 24.0 {
                if let Some(g) = ai.goal {
                    moves.push((id, g));
                }
            }
        }
        for (id, g) in moves {
            self.set_position(id, g);
            if let Some(ai) = self.state.entities.get_mut(id).and_then(|e| e.ai.as_mut()) {
                ai.stuck = 0.0;
            }
        }
    }

    /// Followers in the room the player is in, with their feet.
    fn followers_here(&self) -> Vec<(EntityId, Vec3)> {
        let here = self.state.world.current_room.map(RegionKey::Room);
        self.state
            .entities
            .iter()
            .filter(|e| e.region.is_none() || e.region == here)
            .filter(|e| e.ai.as_ref().is_some_and(|a| matches!(a.def, crate::ai::AiDef::Follow { .. })))
            .filter_map(|e| Some((e.id, e.pos - Vec3::Y * e.character.as_ref()?.height() * 0.5)))
            .collect()
    }

    /// Walk-over pickups.
    pub(crate) fn update_pickups(&mut self, events: &mut Vec<SimEvent>) {
        let Some(p) = self.player() else { return };
        let center = p.pos;
        let got: Vec<(EntityId, Vec3, crate::switches::PickupDef)> = self
            .state
            .entities
            .iter()
            .filter_map(|e| {
                let pk = e.pickup.as_ref()?;
                let d = e.pos - center;
                (Vec3::new(d.x, 0.0, d.z).length() < pk.radius && d.y.abs() < 1.6).then(|| (e.id, e.pos, pk.clone()))
            })
            .collect();
        for (id, pos, pk) in got {
            self.despawn(id);
            if let Some(run) = &mut self.state.courses.run {
                run.score += pk.score;
            }
            let msg = if pk.score > 0 { format!("+{} {}", pk.score, pk.label) } else { pk.label.clone() };
            self.state.courses.message = Some((msg, self.state.tick));
            events.push(SimEvent::Coin { pos });
        }
    }

    /// The uplink the player stands in (HUD).
    pub fn uplink_hud(&self) -> Option<UplinkHud> {
        self.state.courses.inside.iter().find_map(|r| {
            let z = self.state.statics.zone(*r).filter(|z| z.kind == ZoneKind::Hack)?;
            let h = self.state.switchboard.hacks.iter().find(|h| h.zone == *r);
            Some(UplinkHud {
                label: if z.label.is_empty() { "UPLINK".into() } else { z.label.clone() },
                progress: h.map(|h| h.progress).unwrap_or(0.0),
                done: h.is_some_and(|h| h.done),
            })
        })
    }
}
