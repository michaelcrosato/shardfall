//! The simulation: a `World` plus a `Game`, stepped at a fixed 60 ticks per second, with
//! rewind (snapshots + recorded inputs) and replays (inputs from the start + a state hash).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::input::Input;
use crate::params::{ParamVisitor, Tunable};
use crate::view::Draw;
use crate::world::World;

/// A game: its rules and its own state. Keep ALL game state in the struct (it is cloned for
/// snapshots and rewinds) and use only `w.rng` for randomness, so runs repeat exactly.
///
/// ```ignore
/// #[derive(Clone, Default)]
/// struct MyGame { score: u32 }
/// impl Game for MyGame {
///     fn setup(&mut self, w: &mut World) { /* build the level, spawn the player */ }
///     fn update(&mut self, w: &mut World, input: &Input) { /* rules, AI, events */ }
/// }
/// ```
pub trait Game: GameClone + Send {
    /// Builds the world for a fresh start: level, player (`w.player = Some(id)`), enemies.
    fn setup(&mut self, w: &mut World);
    /// One tick of rules, before the engine moves anything. `input` is the player's input
    /// (it already drives the player's character). `w.events` holds the last tick's events.
    fn update(&mut self, w: &mut World, input: &Input);
    /// Extra drawing for the frame: HUD text, bars, labels, shapes. Must not change state.
    fn draw(&self, _w: &World, _d: &mut Draw) {}
    /// Game state for agents (the `status` tool shows it). Keep it small and useful.
    fn status(&self, _w: &World) -> Value {
        Value::Null
    }
    /// Game tunables, listed by `params` and changed by `set` as `game.<name>`.
    fn params(&mut self, _v: &mut dyn ParamVisitor) {}
    /// A scripted player for `autoplay` and tests: the input it would give this tick.
    fn bot(&mut self, _w: &World) -> Option<Input> {
        None
    }
}

/// Lets `Box<dyn Game>` be cloned; implemented for every `Game + Clone`.
pub trait GameClone {
    fn clone_box(&self) -> Box<dyn Game>;
}

impl<T: Game + Clone + 'static> GameClone for T {
    fn clone_box(&self) -> Box<dyn Game> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn Game> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// A registered game (see `src/games/mod.rs`).
pub struct GameDef {
    pub name: &'static str,
    pub about: &'static str,
    pub make: fn() -> Box<dyn Game>,
}

/// Recorded inputs from the start of a game, for exact reproduction.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Replay {
    pub game: String,
    pub seed: u64,
    /// Run-length encoded: (repeat count, input).
    pub inputs: Vec<(u32, Input)>,
    /// State hash after the last input.
    pub hash: Option<String>,
}

impl Replay {
    pub fn push(&mut self, input: &Input) {
        match self.inputs.last_mut() {
            Some((n, last)) if last == input && *n < u32::MAX => *n += 1,
            _ => self.inputs.push((1, *input)),
        }
    }
    pub fn ticks(&self) -> u64 {
        self.inputs.iter().map(|(n, _)| *n as u64).sum()
    }
    pub fn iter(&self) -> impl Iterator<Item = &Input> {
        self.inputs.iter().flat_map(|(n, f)| std::iter::repeat_n(f, *n as usize))
    }
}

pub struct Sim {
    pub name: String,
    pub world: World,
    pub game: Box<dyn Game>,
    /// Inputs since the start (for `record` / replays).
    pub recording: Replay,
    history: VecDeque<(World, Box<dyn Game>)>,
    inputs: VecDeque<(u64, Input)>,
    /// Ticks between rewind snapshots and how far back rewind reaches.
    pub snapshot_every: u64,
    pub history_ticks: u64,
}

impl Sim {
    pub fn new(def: &GameDef, seed: u64) -> Sim {
        let mut world = World::new(seed);
        let mut game = (def.make)();
        game.setup(&mut world);
        Sim {
            name: def.name.into(),
            world,
            game,
            recording: Replay { game: def.name.into(), seed, ..Default::default() },
            history: VecDeque::new(),
            inputs: VecDeque::new(),
            snapshot_every: 30,
            history_ticks: 60 * 60,
        }
    }

    /// Advances one tick with the player's input.
    pub fn step(&mut self, input: &Input) {
        let tick = self.world.tick;
        if self.history.back().is_none_or(|(w, _)| tick >= w.tick + self.snapshot_every) {
            self.history.push_back((self.world.clone(), self.game.clone()));
            while self.history.len() > 1 && self.history[1].0.tick + self.history_ticks <= tick {
                self.history.pop_front();
            }
            let oldest = self.history.front().map(|h| h.0.tick).unwrap_or(0);
            while self.inputs.front().is_some_and(|(t, _)| *t < oldest) {
                self.inputs.pop_front();
            }
        }
        self.inputs.push_back((tick, *input));
        if self.recording.ticks() < 60 * 60 * 60 {
            self.recording.push(input);
        }
        self.step_inner(input);
    }

    fn step_inner(&mut self, input: &Input) {
        let w = &mut self.world;
        // Characters do nothing unless driven this tick; the player is driven by `input`.
        let player = w.player;
        for e in w.entities.values_mut() {
            if let Some(c) = &mut e.character {
                c.input = if Some(e.id) == player { *input } else { Input::default() };
            }
        }
        self.game.update(w, input);
        w.simulate();
    }

    pub fn run(&mut self, ticks: u64, input: &Input) {
        for _ in 0..ticks {
            self.step(input);
        }
    }

    /// The earliest tick `rewind` can reach (history restarts at `new`, `load` and `restore`).
    pub fn oldest_tick(&self) -> u64 {
        self.history.front().map(|h| h.0.tick).unwrap_or(self.world.tick)
    }

    /// Goes back `ticks` (restores the nearest snapshot and re-simulates the recorded inputs).
    /// What happened after that point is forgotten. Returns the tick reached.
    pub fn rewind(&mut self, ticks: u64) -> u64 {
        let target = self.world.tick.saturating_sub(ticks);
        let Some(i) = self.history.iter().rposition(|(w, _)| w.tick <= target) else { return self.world.tick };
        let (w, g) = self.history[i].clone();
        self.world = w;
        self.game = g;
        while self.world.tick < target {
            let t = self.world.tick;
            let first = self.inputs.front().map(|(k, _)| *k).unwrap_or(0);
            let Some(input) = t.checked_sub(first).and_then(|i| self.inputs.get(i as usize)).map(|(_, f)| *f) else { break };
            self.step_inner(&input);
        }
        let t = self.world.tick;
        self.history.truncate(i + 1);
        while self.inputs.back().is_some_and(|(k, _)| *k >= t) {
            self.inputs.pop_back();
        }
        let mut r = Replay { game: self.recording.game.clone(), seed: self.recording.seed, ..Default::default() };
        for f in self.recording.iter().take(t as usize) {
            r.push(f);
        }
        self.recording = r;
        t
    }

    /// A copy with the same state and no rewind history (for experiments and benchmarks).
    pub fn fork(&self) -> Sim {
        Sim {
            name: self.name.clone(),
            world: self.world.clone(),
            game: self.game.clone(),
            recording: self.recording.clone(),
            history: VecDeque::new(),
            inputs: VecDeque::new(),
            snapshot_every: self.snapshot_every,
            history_ticks: self.history_ticks,
        }
    }

    /// Replaces the state (named snapshots); rewind history starts over.
    pub fn restore(&mut self, world: World, game: Box<dyn Game>, recording: Replay) {
        self.world = world;
        self.game = game;
        self.recording = recording;
        self.history.clear();
        self.inputs.clear();
    }

    /// Hash of the moving state (replays compare it).
    pub fn hash(&self) -> String {
        format!("{:016x}", self.world.state_hash())
    }

    /// The recording with its final hash, ready to save.
    pub fn replay(&self) -> Replay {
        let mut r = self.recording.clone();
        r.hash = Some(self.hash());
        r
    }
}

/// Engine and game parameters together (`params` / `set`).
impl Tunable for Sim {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        self.world.visit(v);
        v.enter("game");
        self.game.params(v);
        v.exit();
    }
}
