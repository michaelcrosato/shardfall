//! A shared world on the AI Hub (Session Protocol v1, docs/AI_HUB.md): every player's hero in
//! one world, run by the host.
//!
//! - The host's copy runs the world. Each tick it takes every player's input (its own, and what
//!   the others sent it), applies them, and sends the tick's inputs to everyone in bundles
//!   (`b`, a few ticks each). It never waits for a player: one whose input hasn't come keeps
//!   moving as they last did, with no new presses.
//! - The other copies send the host what their player does (`in`), and run the same world from
//!   the host's bundles, tick by tick. The game's world is the same on every copy given the same
//!   inputs (one browser build, no clock, every random number from the seed), so what each copy
//!   shows is the host's world. The host adds a hash of its world now and then (`h`); a copy
//!   that disagrees says so in its log.
//! - The host also decides who left (`g`: from that tick their hero is gone) and finishes the
//!   round: the standings by kills, won when the party gets deep enough in time.
//!
//! Inputs travel quantized (`encode`), and the host applies its own input the same way, so
//! every copy runs exactly the same numbers.

use std::collections::{BTreeMap, VecDeque};

use pav_core::InputFrame;
use pav_core::arpg::GameCmd;
use serde_json::{Value, json};

use crate::hub::{self, Info};
use crate::simhost::SimHost;

/// The world's ticks per second (the game's default rate).
const HZ: f64 = 60.0;
/// Ticks per bundle: 15 bundles a second, under the hub's 20 messages a second.
const BUNDLE: usize = 4;
/// The host hashes its world every this many ticks.
const HASH_EVERY: u64 = 120;
/// The depth a party reaches to win the round.
pub const GOAL_DEPTH: u32 = 4;

/// One tick: each seat's input, and the seats whose heroes leave at it.
#[derive(Clone, Debug, Default)]
struct Tick {
    inputs: Vec<(u32, InputFrame)>,
    gone: Vec<u32>,
}

/// An input as it travels: whole numbers (thousandths of the stick, centimetres of aim).
fn encode(i: &InputFrame) -> Value {
    let q = |v: f32, k: f32| (v * k).round() as i64;
    json!([
        q(i.move_dir.x, 1000.0),
        q(i.move_dir.y, 1000.0),
        i.held,
        i.pressed,
        i.aim.map(|a| json!([q(a.x, 100.0), q(a.y, 100.0), q(a.z, 100.0)])),
        i.cmd.map(|c| serde_json::to_value(c).unwrap_or(Value::Null)),
    ])
}

fn decode(v: &Value) -> InputFrame {
    let a = v.as_array().cloned().unwrap_or_default();
    let n = |k: usize| a.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    let aim = a.get(4).and_then(|x| x.as_array()).map(|p| {
        let c = |k: usize| p.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32 / 100.0;
        glam::Vec3::new(c(0), c(1), c(2))
    });
    InputFrame {
        move_dir: glam::Vec2::new(n(0) as f32 / 1000.0, n(1) as f32 / 1000.0),
        held: n(2) as u32,
        pressed: n(3) as u32,
        aim,
        cmd: a.get(5).filter(|c| !c.is_null()).and_then(|c| serde_json::from_value::<GameCmd>(c.clone()).ok()),
        ..Default::default()
    }
}

/// What every copy does to its world for a tick.
fn apply(sim: &mut pav_core::Sim, primary: u32, tick: &Tick) {
    for seat in &tick.gone {
        sim.remove_party_hero(*seat);
    }
    let mut own = InputFrame::default();
    for (seat, input) in &tick.inputs {
        if *seat == primary {
            own = *input;
        } else {
            sim.set_party_input(*seat, *input);
        }
    }
    sim.step(&own);
}

pub struct Coop {
    /// The players in the round, by id (seat order); the first plays the game's own hero.
    pub seats: Vec<u32>,
    pub primary: u32,
    /// This copy's player (None: a host with no player of its own).
    pub me: Option<u32>,
    pub host: u32,
    pub is_host: bool,
    /// Ticks run so far.
    pub tick: u64,
    /// Ticks owed to the clock (the host makes them; a player replays them).
    due: f64,
    // The host's side.
    queues: BTreeMap<u32, VecDeque<InputFrame>>,
    last: BTreeMap<u32, InputFrame>,
    leaving: Vec<u32>,
    out: Vec<Tick>,
    out_first: u64,
    out_hash: Option<(u64, u64)>,
    since_send: f64,
    // A player's side.
    inbox: VecDeque<(u64, Tick)>,
    hashes: BTreeMap<u64, u64>,
    own: Vec<InputFrame>,
    own_due: f64,
    /// A player's copy disagreed with the host's world.
    pub desync: bool,
    pub finished: bool,
}

impl Coop {
    pub fn new(info: &Info) -> Self {
        let mut seats: Vec<u32> = info.players.iter().map(|p| p.id).collect();
        seats.sort_unstable();
        let primary = seats.first().copied().unwrap_or(0);
        Self {
            primary,
            me: info.playing().then_some(info.me),
            host: info.host,
            is_host: info.is_host,
            seats,
            tick: 0,
            due: 0.0,
            queues: BTreeMap::new(),
            last: BTreeMap::new(),
            leaving: Vec::new(),
            out: Vec::new(),
            out_first: 0,
            out_hash: None,
            since_send: 0.0,
            inbox: VecDeque::new(),
            hashes: BTreeMap::new(),
            own: Vec::new(),
            own_due: 0.0,
            desync: false,
            finished: false,
        }
    }

    /// Who plays which character, in seat order (the same on every copy).
    pub fn cast(&self) -> Vec<(u32, &'static str)> {
        self.seats.iter().enumerate().map(|(k, s)| (*s, pav_core::arpg::round::SEAT_CHARACTERS[k % 4])).collect()
    }

    /// Builds the round's world, the same on every copy: depth 1, a hero per player.
    pub fn build(&self, host: &SimHost, seed: u64) {
        let cast = self.cast();
        let me = self.me;
        host.exec(move |sim| {
            let cfg = sim.config.clone();
            let (first, first_char) = cast[0];
            *sim = pav_core::arpg::round::round_sim(seed, 1, first_char);
            sim.config = cfg;
            if let Some(g) = sim.state.game.as_mut() {
                g.seat = first;
            }
            for (seat, character) in &cast[1..] {
                sim.add_party_hero(*seat, character);
            }
            // This copy shows its own player's hero (the first player's is the game's own).
            sim.viewer = me.filter(|m| *m != first).and_then(|m| {
                let g = sim.state.game.as_ref()?;
                g.party.iter().find(|p| p.seat == m).and_then(|p| p.id)
            });
        });
    }

    /// A message from another copy.
    pub fn message(&mut self, data: &Value, from: u32) {
        match data.get("k").and_then(|k| k.as_str()) {
            Some("in") if self.is_host => {
                let q = self.queues.entry(from).or_default();
                for f in data.get("f").and_then(|f| f.as_array()).into_iter().flatten() {
                    q.push_back(decode(f));
                }
            }
            Some("b") if !self.is_host => {
                let first = data.get("t").and_then(|t| t.as_u64()).unwrap_or(0);
                let gone: Vec<(u64, u32)> = data
                    .get("g")
                    .and_then(|g| g.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|e| Some((e.get(0)?.as_u64()?, e.get(1)?.as_u64()? as u32)))
                    .collect();
                for (k, f) in data.get("f").and_then(|f| f.as_array()).into_iter().flatten().enumerate() {
                    let t = first + k as u64;
                    let inputs = f
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|e| Some((e.get(0)?.as_u64()? as u32, decode(e.get(1)?))))
                        .collect();
                    let gone = gone.iter().filter(|(gt, _)| *gt == t).map(|(_, s)| *s).collect();
                    self.inbox.push_back((t, Tick { inputs, gone }));
                }
                if let Some(h) = data.get("h").and_then(|h| h.as_array()) {
                    if let (Some(t), Some(v)) = (h.first().and_then(|x| x.as_u64()), h.get(1).and_then(|x| x.as_str())) {
                        if let Ok(v) = u64::from_str_radix(v, 16) {
                            self.hashes.insert(t, v);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// A player left (the hub's `leave`): the host takes their hero out at the next tick.
    pub fn leave(&mut self, id: u32) {
        if self.is_host && self.seats.contains(&id) && id != self.primary {
            self.leaving.push(id);
        }
        self.seats.retain(|s| *s != id || *s == self.primary);
        self.queues.remove(&id);
    }

    /// This copy's input for the frame, taken from the app (the stick, buttons, aim and one
    /// queued menu command); `playing`: the player may act.
    fn take_input(host: &SimHost, playing: bool) -> InputFrame {
        let mut slot = host.shared.input.lock().unwrap();
        let mut i = *slot;
        slot.pressed = 0;
        drop(slot);
        let cmd = host.shared.cmds.lock().unwrap().pop_front();
        if !playing {
            return InputFrame::default();
        }
        i.cmd = cmd;
        // As it travels, so the host's own input is the same number everyone applies.
        decode(&encode(&i))
    }

    /// One frame of the round while it plays: inputs out, ticks run.
    pub fn frame(&mut self, host: &SimHost, dt: f32, playing: bool) {
        let input = Self::take_input(host, playing);
        if !playing {
            return;
        }
        let dt = dt as f64;
        if self.is_host {
            self.host_frame(host, dt, input);
        } else {
            self.player_frame(host, dt, input);
        }
    }

    fn host_frame(&mut self, host: &SimHost, dt: f64, input: InputFrame) {
        self.due = (self.due + dt * HZ).min(12.0);
        let n = self.due.floor() as usize;
        self.due -= n as f64;
        let mut ticks = Vec::with_capacity(n);
        for k in 0..n {
            let mut tick = Tick { inputs: Vec::new(), gone: std::mem::take(&mut self.leaving) };
            for seat in self.seats.clone() {
                let inp = if Some(seat) == self.me {
                    let mut i = input;
                    if k > 0 {
                        // A press counts once, on the frame's first tick.
                        i.pressed = 0;
                        i.cmd = None;
                    }
                    i
                } else {
                    let q = self.queues.entry(seat).or_default();
                    // A player far behind (a slow frame) catches up: the oldest frames go.
                    while q.len() > 8 {
                        let skipped = q.pop_front().unwrap();
                        if let Some(next) = q.front_mut() {
                            next.pressed |= skipped.pressed;
                            if next.cmd.is_none() {
                                next.cmd = skipped.cmd;
                            }
                        }
                    }
                    match q.pop_front() {
                        Some(i) => i,
                        None => InputFrame { pressed: 0, cmd: None, ..self.last.get(&seat).copied().unwrap_or_default() },
                    }
                };
                self.last.insert(seat, inp);
                tick.inputs.push((seat, inp));
            }
            ticks.push(tick);
        }
        if !ticks.is_empty() {
            let primary = self.primary;
            let first = self.tick;
            let run = ticks.clone();
            let hash = host
                .query(move |sim| {
                    let mut out = None;
                    for (k, t) in run.iter().enumerate() {
                        apply(sim, primary, t);
                        if (first + k as u64 + 1) % HASH_EVERY == 0 {
                            out = Some((first + k as u64 + 1, sim.world_hash()));
                        }
                    }
                    out
                })
                .flatten();
            if self.out.is_empty() {
                self.out_first = self.tick;
            }
            self.tick += ticks.len() as u64;
            self.out.extend(ticks);
            if hash.is_some() {
                self.out_hash = hash;
            }
        }
        self.since_send += dt;
        if self.out.len() >= BUNDLE || (!self.out.is_empty() && self.since_send > 0.12) {
            self.send_bundle();
        }
    }

    fn send_bundle(&mut self) {
        let f: Vec<Value> =
            self.out.iter().map(|t| Value::Array(t.inputs.iter().map(|(s, i)| json!([s, encode(i)])).collect())).collect();
        let first = self.out_first;
        let g: Vec<Value> = self
            .out
            .iter()
            .enumerate()
            .flat_map(|(k, t)| t.gone.iter().map(move |s| json!([first + k as u64, s])))
            .collect();
        let mut msg = json!({ "k": "b", "t": self.out_first, "f": f });
        if !g.is_empty() {
            msg["g"] = Value::Array(g);
        }
        if let Some((t, h)) = self.out_hash.take() {
            msg["h"] = json!([t, format!("{h:x}")]);
        }
        let ok = hub::send(&msg, None, true);
        if ok {
            self.out.clear();
            self.since_send = 0.0;
        }
    }

    fn player_frame(&mut self, host: &SimHost, dt: f64, input: InputFrame) {
        // What this player does goes to the host, a few ticks a message.
        if self.me.is_some() {
            self.own_due = (self.own_due + dt * HZ).min(12.0);
            let n = self.own_due.floor() as usize;
            self.own_due -= n as f64;
            for k in 0..n {
                let mut i = input;
                if k > 0 {
                    i.pressed = 0;
                    i.cmd = None;
                }
                self.own.push(i);
            }
            self.since_send += dt;
            if self.own.len() >= BUNDLE || (!self.own.is_empty() && self.since_send > 0.12) {
                let f: Vec<Value> = self.own.iter().map(encode).collect();
                if hub::send(&json!({ "k": "in", "f": f }), Some(self.host), true) {
                    self.own.clear();
                    self.since_send = 0.0;
                }
            }
        }
        // The host's world, replayed at its own pace; a copy behind catches up.
        self.due = (self.due + dt * HZ).min(30.0);
        let behind = self.inbox.len() as f64 > 12.0;
        let mut n = if behind { self.inbox.len().min(30) } else { (self.due.floor() as usize).min(self.inbox.len()) };
        if n == 0 {
            return;
        }
        self.due = (self.due - n as f64).max(0.0);
        let mut run = Vec::with_capacity(n);
        while n > 0 {
            match self.inbox.pop_front() {
                Some((t, tick)) if t == self.tick + run.len() as u64 => run.push(tick),
                Some((t, _)) if t < self.tick + run.len() as u64 => continue,
                Some(other) => {
                    // A gap (shouldn't happen: the bundles are reliable and in order).
                    log::warn!("shared world: tick {} missing, have {}", self.tick + run.len() as u64, other.0);
                    self.inbox.push_front(other);
                    break;
                }
                None => break,
            }
            n -= 1;
        }
        if run.is_empty() {
            return;
        }
        let primary = self.primary;
        let first = self.tick;
        let wanted: Vec<(u64, u64)> = self.hashes.range(first + 1..=first + run.len() as u64).map(|(t, h)| (*t, *h)).collect();
        let count = run.len() as u64;
        let mismatch = host
            .query(move |sim| {
                let mut bad = None;
                for (k, t) in run.iter().enumerate() {
                    apply(sim, primary, t);
                    let at = first + k as u64 + 1;
                    if let Some((_, h)) = wanted.iter().find(|(wt, _)| *wt == at) {
                        let mine = sim.world_hash();
                        if mine != *h && bad.is_none() {
                            bad = Some((at, mine, *h));
                        }
                    }
                }
                bad
            })
            .flatten();
        if let Some((at, mine, theirs)) = mismatch {
            if !self.desync {
                log::warn!("shared world: this copy differs from the host's at tick {at} ({mine:x} vs {theirs:x})");
            }
            self.desync = true;
        }
        self.tick += count;
        self.hashes = self.hashes.split_off(&(self.tick + 1));
    }

    /// The host's report of the round: each player's kills, best first; won when the party
    /// reached the goal depth.
    pub fn finish(&mut self, host: &SimHost) {
        if !self.is_host || self.finished {
            return;
        }
        self.finished = true;
        let primary = self.primary;
        let (kills, depth) = host
            .query(move |sim| {
                let g = sim.state.game.as_ref()?;
                let mut k: Vec<(u32, u64)> = vec![(primary, g.hero.kills)];
                k.extend(g.party.iter().map(|m| (m.seat, m.hero.kills)));
                Some((k, pav_core::arpg::round::standing(g).0))
            })
            .flatten()
            .unwrap_or_default();
        let mut by: BTreeMap<std::cmp::Reverse<u64>, Vec<u32>> = BTreeMap::new();
        for (seat, k) in &kills {
            by.entry(std::cmp::Reverse(*k)).or_default().push(*seat);
        }
        let standings: Vec<Vec<u32>> = by.into_values().collect();
        let scores: serde_json::Map<String, Value> = kills.iter().map(|(s, k)| (s.to_string(), json!(k))).collect();
        hub::finish(&json!({ "standings": standings, "scores": scores, "won": depth >= GOAL_DEPTH }));
    }

    /// The host: the party is deep enough to win.
    pub fn goal_reached(&self, depth: u32) -> bool {
        self.is_host && depth >= GOAL_DEPTH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_travel_and_come_back_the_same() {
        let i = InputFrame {
            move_dir: glam::Vec2::new(0.70710678, -0.70710678),
            held: 3,
            pressed: 1,
            aim: Some(glam::Vec3::new(12.345, 0.02, -7.891)),
            cmd: Some(GameCmd::Pickup(7)),
            ..Default::default()
        };
        let once = decode(&encode(&i));
        let twice = decode(&encode(&once));
        assert_eq!(format!("{once:?}"), format!("{twice:?}"), "quantized inputs are stable");
        assert_eq!(once.cmd, Some(GameCmd::Pickup(7)));
        assert!((once.move_dir.x - 0.707).abs() < 1e-6);
    }
}
