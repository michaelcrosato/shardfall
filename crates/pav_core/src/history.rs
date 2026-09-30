//! Time travel: periodic snapshots plus the input of every tick. Rewinding restores the nearest
//! earlier snapshot and re-simulates forward with the recorded inputs.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::input::InputFrame;
use crate::sim::SimState;

pub struct History {
    pub snapshots: VecDeque<SimState>,
    /// (tick, input used to advance from `tick` to `tick + 1`)
    pub inputs: VecDeque<(u64, InputFrame)>,
    /// Ticks between snapshots.
    pub interval: u64,
    /// How far back rewind can go (ticks).
    pub window: u64,
    pub enabled: bool,
}

impl Default for History {
    fn default() -> Self {
        Self { snapshots: VecDeque::new(), inputs: VecDeque::new(), interval: 20, window: 60 * 30, enabled: true }
    }
}

impl History {
    pub fn clear(&mut self) {
        self.snapshots.clear();
        self.inputs.clear();
    }

    /// Call before stepping from `state.tick` with `input`.
    pub fn record(&mut self, state: &SimState, input: &InputFrame) {
        if !self.enabled {
            return;
        }
        if self.snapshots.back().is_none_or(|s| state.tick >= s.tick + self.interval) {
            self.snapshots.push_back(state.clone());
        }
        self.inputs.push_back((state.tick, *input));
        while self.snapshots.len() > 1 && self.snapshots[1].tick + self.window <= state.tick {
            self.snapshots.pop_front();
        }
        let oldest = self.snapshots.front().map(|s| s.tick).unwrap_or(0);
        while self.inputs.front().is_some_and(|(t, _)| *t < oldest) {
            self.inputs.pop_front();
        }
    }

    pub fn oldest_tick(&self) -> Option<u64> {
        self.snapshots.front().map(|s| s.tick)
    }

    pub fn newest_tick(&self) -> Option<u64> {
        self.inputs.back().map(|(t, _)| t + 1)
    }

    pub fn input_at(&self, tick: u64) -> Option<InputFrame> {
        let front = self.inputs.front()?.0;
        let i = tick.checked_sub(front)? as usize;
        self.inputs.get(i).filter(|(t, _)| *t == tick).map(|(_, f)| *f)
    }

    /// Forgets everything after `tick` (acting after a rewind starts a new timeline).
    pub fn truncate_after(&mut self, tick: u64) {
        self.snapshots.retain(|s| s.tick <= tick);
        while self.inputs.back().is_some_and(|(t, _)| *t >= tick) {
            self.inputs.pop_back();
        }
    }

    /// Rough memory use in bytes.
    pub fn approx_bytes(&self) -> usize {
        self.snapshots.iter().map(|s| s.approx_bytes()).sum::<usize>()
            + self.inputs.len() * std::mem::size_of::<(u64, InputFrame)>()
    }
}

/// A recorded play session: rebuild the scene, then feed the inputs tick by tick.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Replay {
    pub scene: String,
    pub seed: u64,
    /// Parameter values at the start (path -> value).
    #[serde(default)]
    pub params: std::collections::BTreeMap<String, crate::params::ParamValue>,
    /// Run-length encoded inputs: (repeat count, input).
    pub inputs: Vec<(u32, InputFrame)>,
    /// State hash after the last input, for verification.
    #[serde(default)]
    pub final_hash: Option<String>,
}

impl Replay {
    pub fn push(&mut self, input: &InputFrame) {
        match self.inputs.last_mut() {
            Some((n, last)) if last == input && *n < u32::MAX => *n += 1,
            _ => self.inputs.push((1, *input)),
        }
    }
    pub fn ticks(&self) -> u64 {
        self.inputs.iter().map(|(n, _)| *n as u64).sum()
    }
    pub fn iter(&self) -> impl Iterator<Item = &InputFrame> {
        self.inputs.iter().flat_map(|(n, f)| std::iter::repeat_n(f, *n as usize))
    }
}
