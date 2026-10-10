//! The Hall of Heroes (Emberwatch): every playable character (game/heroes.toml) on a pedestal,
//! showing off how they move, and the change from one hero to another. Each character is a
//! hero of their own (level, gear, passives, waypoints) waiting in the hall while another is
//! played; the stash is shared, so it goes with whoever steps forward.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::cmd::{Spot, SpotKind};
use super::data::{Behavior, CharacterDef, Data, data};
use super::hero::Hero;
use super::{Game, refresh_hero};
use crate::color::Color;
use crate::entity::EntityId;
use crate::moves::MoveId;
use crate::puppet::{BodyPlan, PuppetDef};
use crate::sim::Sim;
use crate::statics::{Block, RegionKey};
use crate::zones::{Label, LabelMode};

/// The middle of the row of pedestals (south side of the square, faces to the camera).
pub const HALL_AT: Vec3 = Vec3::new(0.0, 0.0, 13.5);
/// Metres between pedestals; their height.
const SPACING: f32 = 4.5;
const PLINTH: f32 = 0.3;
/// Seconds of idle between showreel items.
const REST: f32 = 0.9;

/// One beat of a showreel: a move (at the skill's own timing), a captured clip (from a moment
/// in), or a rest that lets the idle show.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Beat {
    Move {
        anim: MoveId,
        side: f32,
        hit: f32,
        secs: f32,
        /// Channels: one turn every `cycle` seconds; leaps rise and fall.
        cycle: f32,
        leap: bool,
    },
    Clip {
        id: u32,
        from: f32,
        secs: f32,
    },
    Rest {
        secs: f32,
    },
}

impl Beat {
    pub fn secs(&self) -> f32 {
        match self {
            Beat::Move { secs, .. } | Beat::Clip { secs, .. } | Beat::Rest { secs } => *secs,
        }
    }
}

/// A character performing on a pedestal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Performer {
    pub id: EntityId,
    pub character: String,
    pub reel: Vec<Beat>,
    /// The beat playing and seconds into it.
    pub index: usize,
    pub t: f32,
}

/// What a character performs, beat by beat: each skill's swings as they strike them (their
/// captured attack or move for it, else the skill's own), moves and clips, a rest after each.
pub fn reel(d: &Data, c: &CharacterDef, look: &PuppetDef) -> Vec<Beat> {
    let biped = look.body == BodyPlan::Biped;
    let mut out = Vec::new();
    for item in &c.showreel {
        if let Some(i) = d.skill_id(item) {
            let sk = d.skill(i);
            let swings = if sk.behavior == Behavior::Channel { 1 } else { sk.combo.max(1) };
            for combo in 0..swings {
                let side = if combo % 2 == 0 { 1.0 } else { -1.0 };
                let captured = (biped && !matches!(sk.behavior, Behavior::Channel | Behavior::Leap))
                    .then(|| crate::clips::attack_clip(look, &sk.key, combo))
                    .flatten();
                if let Some((id, strike)) = captured {
                    // From a little before the strike, through its follow-through.
                    let dur = crate::clips::with(id, |k| k.dur).unwrap_or(1.0);
                    let from = (strike - 0.45).max(0.0);
                    out.push(Beat::Clip { id, from, secs: (dur - from).clamp(0.5, 1.5) });
                    continue;
                }
                let (anim, side) = crate::moves::attack_move(look, &sk.key, combo, side).unwrap_or((sk.anim, side));
                let channel = sk.behavior == Behavior::Channel;
                out.push(Beat::Move {
                    anim,
                    side,
                    hit: sk.hit,
                    secs: if channel { 1.8 } else { sk.time.max(0.3) * if sk.behavior == Behavior::Leap { 1.6 } else { 1.2 } },
                    cycle: if channel { sk.interval.max(0.2) * 1.6 } else { 0.0 },
                    leap: sk.behavior == Behavior::Leap,
                });
            }
        } else if let Some(id) = crate::clips::find(item) {
            let dur = crate::clips::with(id, |k| k.dur).unwrap_or(1.0);
            out.push(Beat::Clip { id, from: 0.0, secs: dur.min(4.5) });
        } else if let Some(anim) = MoveId::named(item).filter(|m| *m != MoveId::NONE) {
            let table = crate::moves::table();
            let mv = table.get(anim.index()).cloned().unwrap_or_default();
            let len = (mv.wind + mv.active + mv.recover).max(0.2);
            let hit = (mv.wind + mv.hit * mv.active) / len;
            out.push(Beat::Move { anim, side: 1.0, hit, secs: len, cycle: 0.0, leap: false });
        }
        out.push(Beat::Rest { secs: REST });
    }
    out
}

/// Builds the hall: a pedestal for every character with them on it, a name on the floor in
/// front, and a spot to step up to.
pub fn build(sim: &mut Sim, g: &mut Game) {
    let d = data();
    let n = d.characters.len();
    for (i, c) in d.characters.iter().enumerate() {
        let x = (i as f32 - (n as f32 - 1.0) * 0.5) * SPACING;
        let at = HALL_AT + Vec3::new(x, 0.0, 0.0);
        let st = &mut sim.state;
        st.statics.add(
            &mut st.physics,
            Block::new(at + Vec3::new(-0.9, 0.0, -0.9), at + Vec3::new(0.9, PLINTH, 0.9), Color::hex("#6e655d"))
                .with_flags(crate::statics::block_flags::ROUNDED),
        );
        // A trim in the character's own colour.
        let trim = Color::try_hex(&c.puppet.shirt).unwrap_or(Color::hex("#8a2a24"));
        st.statics.add(
            &mut st.physics,
            Block::new(at + Vec3::new(-0.92, PLINTH - 0.08, -0.92), at + Vec3::new(0.92, PLINTH - 0.02, 0.92), trim),
        );
        let label_at = at + Vec3::new(0.0, 0.02, 1.55);
        st.statics.add_label_to(
            RegionKey::chunk_of(label_at),
            Label {
                text: c.name.to_uppercase(),
                pos: label_at,
                size: 0.42,
                color: Color::hex("#f2e6c9"),
                mode: LabelMode::Floor,
                facing: Default::default(),
            },
        );
        let look = c.puppet_over(&sim.config.puppet);
        let reel = reel(&d, c, &look);
        // An idle brain walks them back onto the pedestal if the hero shoves them off.
        let feet = at + Vec3::Y * PLINTH;
        let brain = crate::ai::Ai::new(crate::ai::AiDef::Idle, feet, Vec::new(), 0.3, 0.0, 0.0);
        let id = sim.spawn_npc(&c.full_name(), feet, 0.0, look, Some(brain), None);
        let start = reel.len().saturating_sub(1);
        g.performers.push(Performer { id, character: c.key.clone(), reel, index: start, t: f32::MAX });
        g.spots.push(Spot {
            kind: SpotKind::Hero,
            name: c.full_name(),
            pos: at + Vec3::new(0.0, 0.0, 1.3),
            reach: 2.2,
            info: Vec::new(),
        });
    }
    // Braziers at either end light the row.
    let end = (n as f32 * 0.5 + 0.2) * SPACING;
    for x in [-end, end] {
        super::scene::brazier(sim, HALL_AT + Vec3::new(x, 0.0, 0.0));
    }
    describe(g);
}

/// Writes each pedestal's card: who they are, how they move, and where their hero stands.
pub fn describe(g: &mut Game) {
    let d = data();
    let playing = g.hero.character_key(&d).to_string();
    for (s, c) in g.spots.iter_mut().filter(|s| s.kind == SpotKind::Hero).zip(&d.characters) {
        let mut info = vec![c.about.clone(), motion_line(c)];
        info.push(if c.key == playing {
            format!("You are playing {} (level {}).", c.name, g.hero.level)
        } else {
            match g.roster.get(&c.key) {
                Some(h) => format!("{} waits here at level {}, deepest depth {}.", c.name, h.level, h.max_depth),
                None => format!("A new hero: {} starts at level 1. The stash comes along.", c.name),
            }
        });
        s.info = info;
    }
}

/// One line on how a character moves: their captured idle, gait, attacks and dodge.
pub fn motion_line(c: &CharacterDef) -> String {
    let p = &c.puppet;
    let short = |n: &str| n.rsplit('/').next().unwrap_or(n).replace('_', " ");
    let mut bits = Vec::new();
    if !p.idle_clip.is_empty() {
        bits.push(format!("idle: {}", short(&p.idle_clip)));
    }
    if !p.run_clip.is_empty() {
        bits.push(format!("run: {}", short(&p.run_clip)));
    }
    let strikes: usize = p.attack_clips.values().map(|s| s.names().len()).sum();
    let moves: usize = p.attack_moves.values().map(|s| s.names().len()).sum();
    if strikes + moves > 0 {
        bits.push(format!("{strikes} captured strikes, {moves} moves of their own"));
    }
    if !p.dodge_clip.is_empty() {
        bits.push(format!("dodge: {}", short(&p.dodge_clip)));
    }
    if bits.is_empty() {
        "Moves with the engine's procedural animation.".into()
    } else {
        format!("Moves: {}.", bits.join(" · "))
    }
}

/// Plays every performer's showreel.
pub fn update(g: &mut Game, sim: &mut Sim, dt: f32) {
    for p in &mut g.performers {
        if p.reel.is_empty() {
            continue;
        }
        let Some(ch) = sim.state.entities.get_mut(p.id).and_then(|e| e.character.as_mut()) else { continue };
        p.t += dt;
        if p.t >= p.reel[p.index % p.reel.len()].secs() {
            // The beat is over: clear it away and start the next.
            match p.reel[p.index % p.reel.len()] {
                Beat::Move { .. } => {
                    ch.anim.set_action(MoveId::NONE, 0.0, 0.0, 1.0);
                    ch.anim.lift = 0.0;
                }
                Beat::Clip { id, .. } if ch.anim.clip == id => ch.anim.stop_clip(),
                _ => {}
            }
            p.index = (p.index + 1) % p.reel.len();
            p.t = 0.0;
            if let Beat::Clip { id, from, .. } = p.reel[p.index] {
                ch.anim.set_action(MoveId::NONE, 0.0, 0.0, 1.0);
                ch.anim.replay_clip(id, crate::clips::ONCE | crate::clips::QUICK, 1.0);
                ch.anim.clip_t = from;
            }
        }
        if let Beat::Move { anim, side, hit, secs, cycle, leap } = p.reel[p.index] {
            let k = if cycle > 0.0 { (p.t / cycle).fract() } else { (p.t / secs).min(1.0) };
            ch.anim.set_action(anim, k, hit, side);
            ch.anim.lift = if leap {
                let start = 0.15;
                let q = ((k - start) / (hit - start).max(1e-3)).clamp(0.0, 1.0);
                (q * std::f32::consts::PI).sin() * 1.2
            } else {
                0.0
            };
        }
    }
}

/// Plays as character `key`: the hero now played waits in the hall and theirs steps forward
/// (a new one at level 1 the first time), healed, with the shared stash. The vendor restocks
/// for their level.
pub fn switch(g: &mut Game, sim: &mut Sim, key: &str) -> Result<(), String> {
    let d = data();
    let c = d.character(key).ok_or_else(|| format!("no hero '{key}'"))?;
    let playing = g.hero.character_key(&d).to_string();
    if c.key == playing {
        return Err(format!("You are already {}", c.name));
    }
    let mut next = g.roster.remove(&c.key).unwrap_or_else(|| Hero::new_character(&d, &c.key));
    // The stash is shared: it comes along, its items numbered for the new hero's bags.
    let stash = std::mem::take(&mut g.hero.stash);
    for mut it in stash {
        it.id = next.new_id();
        next.stash.push(it);
    }
    next.potions = next.potion_max;
    let mut old = std::mem::replace(&mut g.hero, next);
    old.character = playing.clone();
    g.roster.insert(playing, old);
    g.buyback.clear();
    if let Some(a) = g.hero_id.and_then(|h| g.actors.get_mut(&h)) {
        a.buffs.clear();
        a.cast = None;
        a.queued = None;
    }
    refresh_hero(sim, g, true);
    if g.place == super::cmd::Place::Town {
        g.restock(sim);
    }
    g.inv_changed();
    describe(g);
    g.say(format!("{} steps forward", c.full_name()), 2.5);
    Ok(())
}

/// The playable character's look for a hero, over a scene's own puppet.
pub fn base_look(d: &Data, hero: &Hero, scene: &PuppetDef) -> PuppetDef {
    d.character(&hero.character).map_or_else(|| scene.clone(), |c| c.puppet_over(scene))
}
