//! Games built on the engine: the hook a new game implements, and the list of games.
//!
//! A game is a struct holding its rules' state (`Clone` and serde, like everything in
//! `SimState`) that implements [`Game`], plus a `build` function that makes its scene. One
//! line in [`GAMES`] and `load scene=<key>` starts it in the tools, the window
//! (`shardfall --scene <key>`) and the browser (`?scene=<key>`). `starter.rs` is the template
//! to copy; `docs/MAKE_A_GAME.md` is the guide. Shardfall (`crate::arpg`) predates this hook and
//! keeps its own.

use std::any::Any;
use std::collections::BTreeMap;

use anyhow::Result;
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::entity::EntityId;
use crate::frame::SimEvent;
use crate::input::InputFrame;
use crate::sim::Sim;

pub mod starter;

/// Every game the engine knows. `load scene=<key>` (or `<key>/<rest>`) builds it.
pub static GAMES: &[Entry] =
    &[entry::<starter::Starter>("Starter: collect every shard while slimes chase you (the template to copy).", starter::build)];

/// Still playing, won or lost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    #[default]
    Playing,
    Won,
    Lost,
}

/// A game's rules. Every method has a default, so a game implements only what it uses.
///
/// The engine calls `pre` and `post` once per tick (60 a second) from `Sim::step`; everything
/// else is read-only. While `pre` or `post` runs, the game is out of `sim.state.play`, so use
/// `self` for the game's own state and `sim` for the world.
pub trait Game: Clone + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The scene key that starts the game: `load scene=<KEY>`, `--scene <KEY>`, `?scene=<KEY>`.
    const KEY: &'static str;

    /// Before characters move: read the player's input and decide what the characters the
    /// game drives do this tick. A returned input replaces that character's own (its `ai`
    /// brain, or the player's keys for the player).
    fn pre(
        &mut self,
        _sim: &mut Sim,
        _input: &InputFrame,
        _dt: f32,
        _events: &mut Vec<SimEvent>,
    ) -> BTreeMap<EntityId, InputFrame> {
        BTreeMap::new()
    }

    /// After physics: the rules (pickups, damage, score, win and loss). Push `SimEvent`s for
    /// sounds and effects (`Coin`, `Pickup`, `Hit`, `Explosion`...).
    fn post(&mut self, _sim: &mut Sim, _dt: f32, _events: &mut Vec<SimEvent>) {}

    /// HUD lines: drawn at the top left of the window, returned by the `play` tool.
    fn hud(&self, _sim: &Sim) -> Vec<String> {
        Vec::new()
    }

    /// The game's state for tools and tests (`play`), as JSON.
    fn status(&self, _sim: &Sim) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn outcome(&self) -> Outcome {
        Outcome::Playing
    }

    /// The input a good player would give this tick. `play bot=true` and the tests drive the
    /// player with it; teach it every new rule, and keep a test that it wins.
    fn bot(&self, _sim: &Sim) -> InputFrame {
        InputFrame::default()
    }
}

/// One line of [`GAMES`]: made with [`entry`].
pub struct Entry {
    pub key: &'static str,
    pub about: &'static str,
    /// Builds the scene and sets `sim.state.play`. The `&str` is the rest of the scene name
    /// after `<key>/` (a level number, say), if any.
    pub build: fn(&mut Sim, Option<&str>) -> Result<()>,
    load: fn(ciborium::Value) -> Result<GameBox, String>,
}

/// A [`GAMES`] line for the game type `G`.
pub const fn entry<G: Game>(about: &'static str, build: fn(&mut Sim, Option<&str>) -> Result<()>) -> Entry {
    Entry { key: G::KEY, about, build, load: load_as::<G> }
}

fn load_as<G: Game>(v: ciborium::Value) -> Result<GameBox, String> {
    v.deserialized::<G>().map(GameBox::new).map_err(|e| e.to_string())
}

/// The game registered under `key`.
pub fn find(key: &str) -> Option<&'static Entry> {
    GAMES.iter().find(|g| g.key == key)
}

/// A running game in `SimState::play`: clones (rewind, snapshots) and serializes (snapshot
/// files, the state hash) through the game's own derives.
pub struct GameBox(Box<dyn DynGame>);

impl GameBox {
    pub fn new<G: Game>(game: G) -> Self {
        Self(Box::new(game))
    }

    pub fn key(&self) -> &'static str {
        self.0.key()
    }

    /// The game as its own type (None for another game).
    pub fn get<G: Game>(&self) -> Option<&G> {
        self.0.as_any().downcast_ref()
    }

    pub fn get_mut<G: Game>(&mut self) -> Option<&mut G> {
        self.0.as_any_mut().downcast_mut()
    }

    pub fn hud(&self, sim: &Sim) -> Vec<String> {
        self.0.hud(sim)
    }

    pub fn status(&self, sim: &Sim) -> serde_json::Value {
        self.0.status(sim)
    }

    pub fn outcome(&self) -> Outcome {
        self.0.outcome()
    }

    pub fn bot(&self, sim: &Sim) -> InputFrame {
        self.0.bot(sim)
    }
}

impl Clone for GameBox {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    key: String,
    state: ciborium::Value,
}

impl Serialize for GameBox {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let state = self.0.save().map_err(serde::ser::Error::custom)?;
        Saved { key: self.key().to_string(), state }.serialize(s)
    }
}

impl<'de> Deserialize<'de> for GameBox {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let saved = Saved::deserialize(d)?;
        let Some(entry) = find(&saved.key) else {
            let known: Vec<&str> = GAMES.iter().map(|g| g.key).collect();
            return Err(D::Error::custom(format!("unknown game '{}' (known: {})", saved.key, known.join(", "))));
        };
        (entry.load)(saved.state).map_err(D::Error::custom)
    }
}

/// [`Game`] as an object (the trait itself isn't one: `Clone` and serde).
trait DynGame: Send + Sync {
    fn key(&self) -> &'static str;
    fn clone_box(&self) -> Box<dyn DynGame>;
    fn save(&self) -> Result<ciborium::Value, String>;
    fn pre(&mut self, sim: &mut Sim, input: &InputFrame, dt: f32, events: &mut Vec<SimEvent>) -> BTreeMap<EntityId, InputFrame>;
    fn post(&mut self, sim: &mut Sim, dt: f32, events: &mut Vec<SimEvent>);
    fn hud(&self, sim: &Sim) -> Vec<String>;
    fn status(&self, sim: &Sim) -> serde_json::Value;
    fn outcome(&self) -> Outcome;
    fn bot(&self, sim: &Sim) -> InputFrame;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<G: Game> DynGame for G {
    fn key(&self) -> &'static str {
        G::KEY
    }
    fn clone_box(&self) -> Box<dyn DynGame> {
        Box::new(self.clone())
    }
    fn save(&self) -> Result<ciborium::Value, String> {
        ciborium::Value::serialized(self).map_err(|e| e.to_string())
    }
    fn pre(&mut self, sim: &mut Sim, input: &InputFrame, dt: f32, events: &mut Vec<SimEvent>) -> BTreeMap<EntityId, InputFrame> {
        Game::pre(self, sim, input, dt, events)
    }
    fn post(&mut self, sim: &mut Sim, dt: f32, events: &mut Vec<SimEvent>) {
        Game::post(self, sim, dt, events)
    }
    fn hud(&self, sim: &Sim) -> Vec<String> {
        Game::hud(self, sim)
    }
    fn status(&self, sim: &Sim) -> serde_json::Value {
        Game::status(self, sim)
    }
    fn outcome(&self) -> Outcome {
        Game::outcome(self)
    }
    fn bot(&self, sim: &Sim) -> InputFrame {
        Game::bot(self, sim)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Sim {
    /// The running game from [`GAMES`], if this scene is one.
    pub fn play(&self) -> Option<&GameBox> {
        self.state.play.as_ref()
    }

    /// The running game's outcome (`Playing` when there is none).
    pub fn outcome(&self) -> Outcome {
        self.play().map(|g| g.outcome()).unwrap_or_default()
    }

    /// The running game's bot input (none without a game).
    pub fn bot_input(&self) -> InputFrame {
        self.play().map(|g| g.bot(self)).unwrap_or_default()
    }

    pub(crate) fn play_pre(&mut self, input: &InputFrame, dt: f32, events: &mut Vec<SimEvent>) -> BTreeMap<EntityId, InputFrame> {
        let Some(mut g) = self.state.play.take() else { return BTreeMap::new() };
        let out = g.0.pre(self, input, dt, events);
        self.state.play = Some(g);
        out
    }

    pub(crate) fn play_post(&mut self, dt: f32, events: &mut Vec<SimEvent>) {
        let Some(mut g) = self.state.play.take() else { return };
        g.0.post(self, dt, events);
        self.state.play = Some(g);
    }

    pub(crate) fn play_hud(&self) -> Vec<String> {
        self.play().map(|g| g.hud(self)).unwrap_or_default()
    }

    /// Plays the running game with its bot until it is won or lost, or `ticks` pass.
    /// Returns the outcome and the ticks played.
    pub fn run_bot(&mut self, ticks: u64) -> (Outcome, u64) {
        for t in 0..ticks {
            let o = self.outcome();
            if o != Outcome::Playing {
                return (o, t);
            }
            let input = self.bot_input();
            self.step(&input);
        }
        (self.outcome(), ticks)
    }
}
