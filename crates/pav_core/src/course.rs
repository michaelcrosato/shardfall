//! What zones do to the player: course timers with gates and best times, checkpoints and
//! respawning (pits, hazards, falling out of the world), parameter pads and camera cues.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::entity::EntityId;
use crate::frame::SimEvent;
use crate::params::{self, ParamValue};
use crate::sim::Sim;
use crate::statics::{RegionKey, ZoneRef};
use crate::zones::{CameraCue, Zone, ZoneKind};

/// Seconds added per missed gate.
pub const GATE_PENALTY: f32 = 2.0;
/// Below this height the player is respawned (fell out of the world).
pub const FALL_LIMIT: f32 = -30.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CourseRun {
    pub region: RegionKey,
    pub course: String,
    /// "room/course", the key for best times.
    pub key: String,
    pub start_tick: u64,
    /// The timer runs once the player leaves the start zone.
    pub started: bool,
    pub gates: Vec<i32>,
    /// Position in `gates` of the next gate to pass.
    pub next: usize,
    pub passed: u32,
    pub missed: u32,
    pub hits: u32,
    pub falls: u32,
    /// Points for destroyed enemies.
    #[serde(default)]
    pub score: u32,
    /// Times a guard raised the alarm.
    #[serde(default)]
    pub alarms: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CourseResult {
    pub course: String,
    pub key: String,
    /// Final time including penalties (s).
    pub time: f32,
    pub raw: f32,
    pub missed: u32,
    pub hits: u32,
    pub falls: u32,
    pub best: f32,
    pub new_best: bool,
    pub tick: u64,
    #[serde(default)]
    pub score: u32,
    #[serde(default)]
    pub alarms: u32,
}

/// Course state shown by the HUD.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CourseHud {
    pub course: String,
    pub running: bool,
    pub time: f32,
    pub gates_passed: u32,
    pub gates_total: u32,
    pub missed: u32,
    pub hits: u32,
    pub falls: u32,
    pub best: Option<f32>,
    pub score: u32,
    pub alarms: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Courses {
    pub run: Option<CourseRun>,
    pub best: BTreeMap<String, f32>,
    pub last: Option<CourseResult>,
    /// Respawn point: feet position and facing (radians).
    pub checkpoint: Option<(Vec3, f32)>,
    /// Zones the player was inside last tick.
    pub inside: Vec<ZoneRef>,
    /// Camera cue from the last pad stepped on (lasts until leaving the room).
    pub sticky_cue: Option<Arc<CameraCue>>,
    /// Camera cue of the camera zone the player is in.
    pub zone_cue: Option<Arc<CameraCue>>,
    /// Bumped whenever the active cue changes.
    pub cue_serial: u64,
    /// Short message for the HUD and the tick it was set.
    pub message: Option<(String, u64)>,
    /// View settings set by pads (`view.*` params, prefix removed); until leaving the room.
    #[serde(default)]
    pub view: BTreeMap<String, ParamValue>,
    #[serde(default)]
    pub view_serial: u64,
}

impl Courses {
    /// The camera cue in effect (camera zones win over pads).
    pub fn cue(&self) -> Option<Arc<CameraCue>> {
        self.zone_cue.clone().or_else(|| self.sticky_cue.clone())
    }

    pub fn hud(&self, tick: u64, dt: f32) -> Option<CourseHud> {
        let r = self.run.as_ref()?;
        Some(CourseHud {
            course: r.course.clone(),
            running: r.started,
            time: if r.started { (tick - r.start_tick) as f32 * dt } else { 0.0 },
            gates_passed: r.passed,
            gates_total: r.gates.len() as u32,
            missed: r.missed,
            hits: r.hits,
            falls: r.falls,
            best: self.best.get(&r.key).copied(),
            score: r.score,
            alarms: r.alarms,
        })
    }
}

fn yaw_of(d: Vec3) -> f32 {
    d.x.atan2(d.z)
}

impl Sim {
    fn region_name(&self, region: RegionKey) -> String {
        match region {
            RegionKey::Room(id) => self.state.world.rooms.get(id as usize).map(|r| r.key.clone()).unwrap_or_default(),
            RegionKey::Hub => "hub".into(),
            RegionKey::Chunk(x, z) => format!("chunk{x}_{z}"),
        }
    }

    fn say(&mut self, msg: impl Into<String>) {
        self.state.courses.message = Some((msg.into(), self.state.tick));
    }

    /// Player feet position and facing.
    fn player_feet(&self) -> Option<(EntityId, Vec3, f32)> {
        let id = self.state.player?;
        let e = self.state.entities.get(id)?;
        let ch = e.character.as_ref()?;
        Some((id, e.pos - Vec3::Y * ch.height() * 0.5, ch.facing))
    }

    /// Puts the player back at the last checkpoint (or the room entrance / scene start).
    pub fn respawn_player(&mut self, events: &mut Vec<SimEvent>) {
        let Some(pid) = self.state.player else { return };
        let (pos, yaw) = match self.state.courses.checkpoint {
            Some(c) => c,
            None => match self.state.world.current_room.and_then(|i| self.state.world.rooms.get(i as usize)) {
                Some(r) => (r.inside, yaw_of(r.inward)),
                None => (self.state.spawn, 0.0),
            },
        };
        match self.state.entities.get(pid).and_then(|e| e.character.as_ref()).and_then(|c| c.riding) {
            // Driving: the vehicle goes back to the checkpoint with you in it.
            Some(vid) => self.place_vehicle(vid, pos, yaw),
            None => {
                self.set_position(pid, pos);
            }
        }
        if let Some(ch) = self.state.entities.get_mut(pid).and_then(|e| e.character.as_mut()) {
            ch.facing = yaw;
            ch.anim.facing = yaw;
            ch.hang = None;
            ch.stun = 0.0;
            ch.invuln = 0.5;
        }
        if let Some(r) = &mut self.state.courses.run {
            r.falls += 1;
        }
        // Zones at the respawn point count as already entered (respawning onto START must not
        // restart the run).
        self.state.courses.inside = self.state.statics.zones_at(pos + Vec3::Y * 0.1).map(|(r, _)| r).collect();
        self.mission_respawn(pos, yaw);
        events.push(SimEvent::Respawn { pos });
    }

    /// Knocks a character back (hazards, projectiles). `respawn` sends the player to the last
    /// checkpoint instead.
    pub fn hit_character(
        &mut self,
        id: EntityId,
        at: Vec3,
        dir: Vec3,
        knockback: f32,
        respawn: bool,
        events: &mut Vec<SimEvent>,
    ) {
        let stun = self.config.movement.hit_stun;
        let is_player = self.state.player == Some(id);
        let Some(ch) = self.state.entities.get_mut(id).and_then(|e| e.character.as_mut()) else { return };
        if ch.invuln > 0.0 {
            return;
        }
        ch.invuln = 0.6;
        ch.stun = stun;
        ch.hang = None;
        ch.climbing = None;
        let flat = Vec3::new(dir.x, 0.0, dir.z).normalize_or(Vec3::X);
        ch.impulse += flat * knockback + Vec3::Y * (knockback * 0.35 + 1.5);
        ch.anim.recoil = 1.0;
        ch.anim.hit(dir, (knockback / 6.0).clamp(0.4, 3.0));
        events.push(SimEvent::Hit { pos: at, strength: knockback });
        if is_player {
            if let Some(r) = &mut self.state.courses.run {
                r.hits += 1;
            }
            self.trace_lost(events);
            if respawn {
                self.respawn_player(events);
            }
        }
    }

    /// Room enter/exit hooks for courses and cues.
    pub(crate) fn courses_on_room_change(&mut self, entered: Option<u16>) {
        let c = &mut self.state.courses;
        c.run = None;
        c.inside.clear();
        if c.sticky_cue.take().is_some() | c.zone_cue.take().is_some() {
            c.cue_serial += 1;
        }
        if !c.view.is_empty() {
            c.view.clear();
            c.view_serial += 1;
        }
        c.checkpoint = entered.and_then(|i| self.state.world.rooms.get(i as usize)).map(|r| (r.inside, yaw_of(r.inward)));
    }

    /// Applies a pad's parameters; values are restored when the player leaves the room.
    fn apply_pad_params(&mut self, region: RegionKey, values: &BTreeMap<String, ParamValue>) {
        let quarters = match region {
            RegionKey::Room(id) => self.state.world.rooms.get(id as usize).map(|r| r.place.quarters).unwrap_or(0),
            _ => 0,
        };
        let mut values = values.clone();
        // View settings are the app's business: hand them over through the frame.
        let view: Vec<(String, ParamValue)> =
            values.iter().filter_map(|(k, v)| Some((k.strip_prefix("view.")?.to_string(), v.clone()))).collect();
        if !view.is_empty() {
            values.retain(|k, _| !k.starts_with("view."));
            self.state.courses.view.extend(view);
            self.state.courses.view_serial += 1;
        }
        rotate_axis_params(&mut values, quarters);
        if self.state.world.current_room.is_some() {
            let mut root = crate::world::ConfigRoot(&mut self.config);
            for k in values.keys() {
                if !self.state.world.saved_params.contains_key(k) {
                    if let Some(v) = params::get(&mut root, k) {
                        self.state.world.saved_params.insert(k.clone(), v);
                    }
                }
            }
        }
        let unknown = params::apply_map(&mut crate::world::ConfigRoot(&mut self.config), &values);
        for u in unknown {
            log::warn!("pad: unknown parameter '{u}'");
        }
    }

    /// Ends the running course: final time with penalties, best times, result card.
    pub(crate) fn finish_course(&mut self, pos: Vec3, events: &mut Vec<SimEvent>) {
        let tick = self.state.tick;
        let dt = self.dt();
        let Some(run) = self.state.courses.run.take() else { return };
        let missed = run.missed + (run.gates.len() - run.next.min(run.gates.len())) as u32;
        let raw = (tick - run.start_tick) as f32 * dt;
        let time = raw + missed as f32 * GATE_PENALTY;
        let old = self.state.courses.best.get(&run.key).copied();
        let new_best = old.is_none_or(|b| time < b);
        if new_best {
            self.state.courses.best.insert(run.key.clone(), time);
        }
        let best = old.map(|b| b.min(time)).unwrap_or(time);
        self.state.courses.last = Some(CourseResult {
            course: run.course.clone(),
            key: run.key.clone(),
            time,
            raw,
            missed,
            hits: run.hits,
            falls: run.falls,
            best,
            new_best,
            tick,
            score: run.score,
            alarms: run.alarms,
        });
        events.push(SimEvent::CourseFinish { pos, time, new_best });
    }

    /// Zone enter/exit for the player: courses, checkpoints, pits, pads, camera cues.
    pub(crate) fn update_zones(&mut self, events: &mut Vec<SimEvent>) {
        let Some((_pid, feet, facing)) = self.player_feet() else { return };
        if feet.y < FALL_LIMIT {
            self.respawn_player(events);
            return;
        }
        let probe = feet + Vec3::Y * 0.1;
        let now: Vec<(ZoneRef, Zone)> = self.state.statics.zones_at(probe).map(|(r, z)| (r, z.clone())).collect();
        let prev = std::mem::take(&mut self.state.courses.inside);
        self.state.courses.inside = now.iter().map(|(r, _)| *r).collect();
        let tick = self.state.tick;

        // Exits.
        for r in &prev {
            if now.iter().any(|(n, _)| n == r) {
                continue;
            }
            let Some(z) = self.state.statics.zone(*r).cloned() else { continue };
            match z.kind {
                ZoneKind::Start => {
                    let c = &mut self.state.courses;
                    if let Some(run) =
                        c.run.as_mut().filter(|run| run.region == r.region && run.course == z.course && !run.started)
                    {
                        run.started = true;
                        run.start_tick = tick;
                        events.push(SimEvent::CourseStart { pos: feet });
                    }
                }
                ZoneKind::Camera => {
                    if self.state.courses.zone_cue.take().is_some() {
                        self.state.courses.cue_serial += 1;
                    }
                }
                _ => {}
            }
        }

        // Entries.
        let mut respawn = false;
        for (r, z) in &now {
            if prev.contains(r) {
                continue;
            }
            match z.kind {
                ZoneKind::Start => {
                    // A running timer of another course is not cancelled by clipping this start.
                    let busy = self
                        .state
                        .courses
                        .run
                        .as_ref()
                        .is_some_and(|x| x.started && (x.region != r.region || x.course != z.course));
                    if busy {
                        continue;
                    }
                    let mut gates: Vec<i32> = self
                        .state
                        .statics
                        .region_zones(r.region)
                        .iter()
                        .filter(|g| g.kind == ZoneKind::Gate && g.course == z.course)
                        .map(|g| g.index)
                        .collect();
                    gates.sort();
                    gates.dedup();
                    let key = format!("{}/{}", self.region_name(r.region), z.course);
                    self.state.courses.run = Some(CourseRun {
                        region: r.region,
                        course: z.course.clone(),
                        key,
                        start_tick: tick,
                        started: false,
                        gates,
                        next: 0,
                        passed: 0,
                        missed: 0,
                        hits: 0,
                        falls: 0,
                        score: 0,
                        alarms: 0,
                    });
                    let yaw = z.facing.map(|f| yaw_of(f.dir())).unwrap_or(facing);
                    self.state.courses.checkpoint = Some((z.floor_center(), yaw));
                }
                ZoneKind::Gate => {
                    let mut msg = None;
                    if let Some(run) =
                        self.state.courses.run.as_mut().filter(|x| x.region == r.region && x.course == z.course && x.started)
                    {
                        if let Some(pos) = run.gates.iter().position(|g| *g == z.index) {
                            if pos >= run.next {
                                let skipped = (pos - run.next) as u32;
                                run.missed += skipped;
                                run.passed += 1;
                                run.next = pos + 1;
                                events.push(SimEvent::Gate { pos: feet, ok: skipped == 0 });
                                if skipped > 0 {
                                    msg = Some(format!(
                                        "missed {skipped} gate{} (+{:.0} s)",
                                        if skipped > 1 { "s" } else { "" },
                                        skipped as f32 * GATE_PENALTY
                                    ));
                                }
                            }
                        }
                    }
                    if let Some(m) = msg {
                        self.say(m);
                    }
                }
                ZoneKind::Finish => {
                    let matches = self
                        .state
                        .courses
                        .run
                        .as_ref()
                        .is_some_and(|x| x.region == r.region && x.course == z.course && x.started && tick > x.start_tick);
                    if matches {
                        self.finish_course(feet, events);
                    }
                }
                ZoneKind::Checkpoint => {
                    let yaw = z.facing.map(|f| yaw_of(f.dir())).unwrap_or(facing);
                    let p = z.floor_center();
                    if self.state.courses.checkpoint.is_none_or(|c| c.0.distance(p) > 0.5) {
                        self.state.courses.checkpoint = Some((p, yaw));
                        events.push(SimEvent::Checkpoint { pos: p });
                    }
                }
                ZoneKind::Kill => respawn = true,
                ZoneKind::Water => events.push(SimEvent::Splash { pos: feet }),
                ZoneKind::Pad => {
                    if !z.params.is_empty() {
                        self.apply_pad_params(r.region, &z.params);
                    }
                    if let Some(c) = &z.camera {
                        self.state.courses.sticky_cue = Some(Arc::new(c.clone()));
                        self.state.courses.cue_serial += 1;
                    }
                    if !z.label.is_empty() {
                        self.say(z.label.clone());
                    }
                    if !z.signal.is_empty() {
                        self.state.signals.push(z.signal.clone());
                    }
                    events.push(SimEvent::Pad { pos: feet });
                }
                ZoneKind::Camera => {
                    if let Some(c) = &z.camera {
                        self.state.courses.zone_cue = Some(Arc::new(c.clone()));
                        self.state.courses.cue_serial += 1;
                    }
                }
                ZoneKind::Conveyor | ZoneKind::Bounce | ZoneKind::Hack => {}
            }
        }
        if respawn {
            self.respawn_player(events);
        }
    }

    /// Mark zones the player stands in as entered without triggering them (after teleports).
    pub fn settle_zones(&mut self) {
        if let Some((_, feet, _)) = self.player_feet() {
            self.state.courses.inside = self.state.statics.zones_at(feet + Vec3::Y * 0.1).map(|(r, _)| r).collect();
        }
    }
}

/// Axis-lock parameters name layout axes (a room turned by a quarter swaps x and z), and a fixed
/// blaster direction is an angle in the room's frame.
pub fn rotate_axis_params(values: &mut BTreeMap<String, ParamValue>, quarters: u8) {
    // A fixed blaster direction turns with the room (-1 = aim, unchanged).
    if let Some(a) = values.get("bombs.shoot_angle").and_then(|v| v.as_f64()) {
        if a >= 0.0 && !quarters.is_multiple_of(4) {
            values.insert("bombs.shoot_angle".into(), ParamValue::Float((a - 90.0 * quarters as f64).rem_euclid(360.0)));
        }
    }
    if quarters.is_multiple_of(2) {
        return;
    }
    if let Some(ParamValue::Text(t)) = values.get_mut("movement.lock_axis") {
        *t = match t.as_str() {
            "x" => "z".into(),
            "z" => "x".into(),
            o => o.into(),
        };
    }
}
