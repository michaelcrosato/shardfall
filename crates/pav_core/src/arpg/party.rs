//! Shared-world co-op (docs/AI_HUB.md): heroes besides the game's own (`Game::hero`), each a
//! player's, in the same world. A party member keeps its own profile (level, gear, skill bar,
//! potions), entity and respawn timer; each tick the game runs a member's input, casts, pickups
//! and rewards by swapping it into the hero's place (`with_member`), so every rule written for
//! the hero holds for every hero. Monsters go for the nearest living hero.
//!
//! The world is the same on every player's copy: the app (`pav_app::coop`) gives every copy
//! the same inputs for the same ticks, and each copy shows its own hero (`Sim::viewer`).

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::combat::{Actor, Team};
use super::hero::Hero;
use super::{Game, feet_of, refresh_hero};
use crate::entity::EntityId;
use crate::input::InputFrame;
use crate::{Sim, SimEvent};

/// Another player's hero.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartyMember {
    /// The player's seat (the hub's player id).
    pub seat: u32,
    pub hero: Hero,
    pub id: Option<EntityId>,
    /// Seconds until a fallen member gets up.
    #[serde(default)]
    pub respawn: f32,
    /// This tick's input.
    #[serde(default)]
    pub input: InputFrame,
}

impl Game {
    /// Runs `f` with party member `i` in the hero's place (`hero`, `hero_id`, `respawn`), then
    /// puts everything back.
    pub(crate) fn with_member<R>(&mut self, sim: &mut Sim, i: usize, f: impl FnOnce(&mut Game, &mut Sim) -> R) -> R {
        self.swap_member(i);
        let r = f(self, sim);
        self.swap_member(i);
        r
    }

    fn swap_member(&mut self, i: usize) {
        let m = &mut self.party[i];
        std::mem::swap(&mut self.hero, &mut m.hero);
        std::mem::swap(&mut self.hero_id, &mut m.id);
        std::mem::swap(&mut self.respawn, &mut m.respawn);
    }

    /// The frame as `viewer`'s player sees it: a party member's own hero in the HUD (the game
    /// is copied with them in the hero's place).
    pub fn frame_for(&self, sim: &Sim, viewer: Option<EntityId>) -> super::GameFrame {
        match viewer.and_then(|v| self.member_of(v)) {
            Some(i) => {
                let mut g = self.clone();
                g.swap_member(i);
                let mut f = g.frame(sim);
                f.heroes = self.hero_seats();
                f
            }
            None => self.frame(sim),
        }
    }

    /// Each player's seat and their hero's entity (the game's own first).
    pub fn hero_seats(&self) -> Vec<(u32, EntityId)> {
        if self.party.is_empty() {
            return Vec::new();
        }
        self.hero_id.map(|h| (self.seat, h)).into_iter().chain(self.party.iter().filter_map(|m| Some((m.seat, m.id?)))).collect()
    }

    /// The party member whose hero this entity is.
    pub fn member_of(&self, id: EntityId) -> Option<usize> {
        self.party.iter().position(|m| m.id == Some(id))
    }

    /// Every hero's entity: the game's own first, then the party's.
    pub fn hero_ids(&self) -> Vec<EntityId> {
        self.hero_id.into_iter().chain(self.party.iter().filter_map(|m| m.id)).collect()
    }

    /// The living heroes' feet and radius.
    pub(crate) fn living_heroes(&self, sim: &Sim) -> Vec<(EntityId, Vec3, f32)> {
        self.hero_ids()
            .into_iter()
            .filter_map(|h| {
                let a = self.actors.get(&h)?;
                if a.dead {
                    return None;
                }
                Some((h, feet_of(sim, h)?.0, a.radius))
            })
            .collect()
    }

    /// The hero with this seat: the game's own (`None`) or a party member's index.
    pub fn seat_slot(&self, seat: u32) -> Option<Option<usize>> {
        if self.seat == seat {
            return Some(None);
        }
        self.party.iter().position(|m| m.seat == seat).map(Some)
    }
}

/// Spawns a member's hero at the place's spawn point, a step to the side for each seat.
pub(crate) fn spawn_member(sim: &mut Sim, g: &mut Game, i: usize) {
    let offset = Vec3::new(((i as f32) - 1.0) * 1.2, 0.0, 1.0);
    let at = sim.state.spawn + offset;
    let id = sim.spawn_character(&format!("hero {}", g.party[i].seat), at);
    let m = &mut g.party[i];
    m.hero.potions = m.hero.potion_max;
    m.respawn = 0.0;
    let mut a = Actor::new(Team::Hero, &m.hero.name, m.hero.level, m.hero.sheet());
    a.radius = 0.42;
    g.actors.insert(id, a);
    m.id = Some(id);
    g.with_member(sim, i, |g, sim| refresh_hero(sim, g, true));
}

impl Sim {
    /// Adds a player's hero to the running game (shared world): `seat` plays `character`.
    pub fn add_party_hero(&mut self, seat: u32, character: &str) {
        let Some(mut g) = self.state.game.take() else { return };
        let d = super::data::data();
        let hero = Hero::new_character(&d, character);
        g.party.push(PartyMember { seat, hero, id: None, respawn: 0.0, input: InputFrame::default() });
        let i = g.party.len() - 1;
        spawn_member(self, &mut g, i);
        self.state.game = Some(g);
    }

    /// A player left a shared world: their hero leaves with them.
    pub fn remove_party_hero(&mut self, seat: u32) {
        let Some(mut g) = self.state.game.take() else { return };
        if let Some(i) = g.party.iter().position(|m| m.seat == seat) {
            let m = g.party.remove(i);
            if let Some(id) = m.id {
                g.actors.remove(&id);
                self.despawn(id);
            }
        }
        self.state.game = Some(g);
    }

    /// Sets a party member's input for the next tick.
    pub fn set_party_input(&mut self, seat: u32, input: InputFrame) {
        if let Some(m) = self.state.game.as_mut().and_then(|g| g.party.iter_mut().find(|m| m.seat == seat)) {
            m.input = input;
        }
    }
}

/// Before movement: each member's commands and input, as the hero's.
pub(crate) fn member_inputs(
    g: &mut Game,
    sim: &mut Sim,
    out: &mut std::collections::BTreeMap<EntityId, InputFrame>,
    events: &mut Vec<SimEvent>,
) {
    for i in 0..g.party.len() {
        let input = g.party[i].input;
        g.with_member(sim, i, |g, sim| {
            if let Some(c) = input.cmd {
                g.command(sim, c, events);
            }
            if let Some(hid) = g.hero_id {
                g.hero_input(sim, hid, &input, out, events);
            }
        });
    }
}

impl Sim {
    /// A hash of the world's play (the tick, the random stream, every body's motion and every
    /// actor's life and mana): equal on every copy of a shared world while they agree. Unlike
    /// `state_hash` it leaves out text (messages, names), which follows each player's language.
    pub fn world_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |v: u64| {
            for b in v.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        eat(self.state.tick);
        eat(self.state.rng.state());
        for (_, b) in self.state.physics.bodies.iter() {
            for v in b.translation().to_array().into_iter().chain(b.linvel().to_array()) {
                eat((v as f64).to_bits());
            }
        }
        if let Some(g) = &self.state.game {
            for (id, a) in &g.actors {
                eat(id.0 as u64);
                eat(a.life.to_bits() as u64);
                eat(a.mana.to_bits() as u64);
                eat(a.dead as u64);
            }
            eat(g.loot.len() as u64);
            eat(g.hero.kills);
            for m in &g.party {
                eat(m.hero.kills);
            }
        }
        h
    }
}
