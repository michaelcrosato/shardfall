//! Shardfall, the showcase hack-and-slash (docs/GAME.md). The game lives inside the
//! simulation (`SimState::game`), so snapshots, rewind, replays, agent tools and the live bridge
//! all work on it. Each tick: `game_pre` turns input and monster brains into movement and skill
//! use before characters move; `game_post` lands hits, moves projectiles, ticks ailments and
//! hands out rewards after physics.

pub mod boss;
pub mod bot;
pub mod brain;
pub mod cmd;
pub mod combat;
pub mod data;
pub mod genome;
pub mod hero;
pub mod items;
pub mod levelgen;
pub mod loot;
pub mod mechanics;
pub mod powers;
pub mod scene;
pub mod skills;
pub mod stats;
pub mod tree;
pub mod world;

pub use cmd::{GameCmd, Place, Spot, SpotKind};

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::entity::EntityId;
use crate::frame::SimEvent;
use crate::input::{InputFrame, buttons};
use crate::params::{ParamVisitor, Tunable};
use crate::sim::Sim;
use brain::{AGGRO_RANGE, Brain, PACK_RANGE, SkillOption};
use combat::*;
use data::{Behavior, data};
use hero::Hero;
use stats::{Base, Mods, Sheet};

/// Difficulty multipliers for play-testing (pause menu, `difficulty.*` parameters).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Difficulty {
    pub player_damage: f32,
    pub player_life: f32,
    pub enemy_damage: f32,
    pub enemy_life: f32,
    pub enemy_speed: f32,
    /// Feel: hit-stop and screen shake strength.
    pub hitstop: f32,
    pub shake: f32,
}

impl Default for Difficulty {
    fn default() -> Self {
        Self {
            player_damage: 1.0,
            player_life: 1.0,
            enemy_damage: 1.0,
            enemy_life: 1.0,
            enemy_speed: 1.0,
            hitstop: 1.0,
            shake: 1.0,
        }
    }
}

impl Tunable for Difficulty {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.float("player_damage", &mut self.player_damage, 0.1, 10.0, "Damage the hero deals (x)");
        v.float("player_life", &mut self.player_life, 0.1, 10.0, "Hero life (x)");
        v.float("enemy_damage", &mut self.enemy_damage, 0.0, 10.0, "Damage monsters deal (x)");
        v.float("enemy_life", &mut self.enemy_life, 0.1, 10.0, "Monster life (x, new spawns)");
        v.float("enemy_speed", &mut self.enemy_speed, 0.3, 3.0, "Monster movement and attack speed (x)");
        v.float("hitstop", &mut self.hitstop, 0.0, 3.0, "Hit-stop strength (feel)");
        v.float("shake", &mut self.shake, 0.0, 3.0, "Screen shake strength (feel)");
    }
}

/// Dodge: speed (m/s), duration (s) and recovery (s).
pub const DODGE_SPEED: f32 = 17.0;
pub const DODGE_TIME: f32 = 0.26;
pub const DODGE_RECOVERY: f32 = 0.75;
/// Hero base movement speed comes from `movement.speed`; monsters move at this share of it.
pub const MONSTER_PACE: f32 = 0.78;

/// Captured falls the hero dies with, in turn (anim/*.json, from open motion libraries).
pub const HERO_DEATHS: &[&str] =
    &["QUATERNIUS/Death01", "MESH2MOTION/Death_D", "MESH2MOTION/Death_C", "CMU/Fall_On_Face", "MESH2MOTION/Death_A"];
/// Captured gestures for the hero's big moments (upper body, so they never stop the hero).
pub const HERO_LEVEL_UP: &str = "MESH2MOTION/Cheer_One_arm";
pub const HERO_BOSS_DOWN: &str = "MESH2MOTION/Cheering_Two_Hands";

/// The wave arena (G1 test ground; later the Proving Grounds in town).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ArenaState {
    pub wave: u32,
    pub next_in: f32,
    pub center: Vec3,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Game {
    pub hero: Hero,
    pub hero_id: Option<EntityId>,
    pub actors: BTreeMap<EntityId, Actor>,
    pub shots: Vec<Shot>,
    pub effects: Vec<Effect>,
    pub floaters: Vec<Floater>,
    /// Hit-stop (seconds of near-frozen time left) and screen shake (decaying strength).
    pub hitstop: f32,
    pub shake: f32,
    /// Banner message and seconds left.
    pub message: Option<(String, f32)>,
    pub arena: Option<ArenaState>,
    /// Hero is down: seconds until respawning at `spawn`.
    pub respawn: f32,
    pub spawn: Vec3,
    pub next_pack: u32,
    pub time: f32,
    /// Seconds since the last level-up (HUD flash).
    pub level_flash: f32,
    /// Where we are, and where we're going at the end of this tick.
    pub place: Place,
    pub travel: Option<u32>,
    /// Loot on the ground.
    pub loot: Vec<loot::GroundItem>,
    pub gold: Vec<loot::GoldPile>,
    /// Item drop rate multiplier (tools and tests).
    pub loot_rate: f32,
    /// Auto-loot items at least this rare (unless off).
    pub auto_loot: Rarity,
    pub auto_loot_off: bool,
    pub full_warned: f32,
    /// The vendor's wares and items sold back to it.
    pub vendor: Vec<items::Item>,
    pub buyback: Vec<items::Item>,
    /// Things to use in this place (vendor, stash, portal).
    pub spots: Vec<Spot>,
    /// Pending echo strikes and the powers' clock.
    pub echoes: Vec<powers::Echo>,
    pub power_clock: f32,
    /// Bumped when anything in the bags changes; the inventory view is cached per revision.
    pub inv_rev: u64,
    #[serde(skip)]
    pub inv_cache: Option<Arc<InvView>>,
    /// Town folk (animated, not fighting).
    pub npcs: Vec<scene::Npc>,
    /// The Menagerie's creatures on show (and the seed of the current set).
    pub exhibits: Vec<scene::Exhibit>,
    pub lab_seed: u64,
    /// The level in progress (levels only).
    pub level: Option<Box<mechanics::LevelState>>,
    /// Kill streak: kills chained within a moment of each other, the clock, the xp they gave.
    #[serde(default)]
    pub streak: (u32, f32, f64),
    /// How many gib chunks are flying (keeps big fights in budget).
    #[serde(default)]
    pub gib_load: f32,
}

impl Game {
    pub fn new(hero: Hero) -> Self {
        Self {
            hero,
            hero_id: None,
            actors: BTreeMap::new(),
            shots: Vec::new(),
            effects: Vec::new(),
            floaters: Vec::new(),
            hitstop: 0.0,
            shake: 0.0,
            message: None,
            arena: None,
            respawn: 0.0,
            spawn: Vec3::ZERO,
            next_pack: 1,
            time: 0.0,
            level_flash: 10.0,
            place: Place::Arena,
            travel: None,
            loot: Vec::new(),
            gold: Vec::new(),
            loot_rate: 1.0,
            auto_loot: Rarity::Magic,
            auto_loot_off: false,
            full_warned: -10.0,
            vendor: Vec::new(),
            buyback: Vec::new(),
            spots: Vec::new(),
            echoes: Vec::new(),
            power_clock: 0.0,
            inv_rev: 0,
            inv_cache: None,
            npcs: Vec::new(),
            exhibits: Vec::new(),
            lab_seed: 0,
            level: None,
            streak: (0, 0.0, 0.0),
            gib_load: 0.0,
        }
    }

    /// Marks the bags (or the sheet) as changed for the inventory view.
    pub fn inv_changed(&mut self) {
        self.inv_rev += 1;
        self.inv_cache = None;
    }

    fn inv_view(&self) -> InvView {
        InvView {
            rev: self.inv_rev,
            equipment: self.hero.equipment.to_vec(),
            inventory: self.hero.inventory.clone(),
            stash: self.hero.stash.clone(),
            vendor: self.vendor.clone(),
            buyback: self.buyback.clone(),
            gold: self.hero.gold,
            bar: self.hero.bar.clone(),
            level: self.hero.level,
            auto_loot: if self.auto_loot_off { 4 } else { self.auto_loot as u8 },
            sheet: self.hero_actor().map(|a| a.sheet.clone()).unwrap_or_default(),
            weapon: self.hero.weapon.clone(),
            powers: self.hero.powers.clone(),
            tree: self.hero.tree.clone(),
            masteries: self.hero.masteries.clone(),
            points: self.hero.points(),
            refund_cost: self.hero.refund_cost(),
            respec_cost: self.hero.respec_cost(),
            tweaks: self.hero_actor().map(|a| a.tweaks.clone()).unwrap_or_default(),
            max_depth: self.hero.max_depth,
            brew: [scene::brew_price(&self.hero, 0), scene::brew_price(&self.hero, 1)],
            potion_max: self.hero.potion_max,
        }
    }

    pub fn hero_actor(&self) -> Option<&Actor> {
        self.hero_id.and_then(|id| self.actors.get(&id))
    }

    /// Time runs at this rate (hit-stop nearly freezes it).
    pub fn time_scale(&self) -> f32 {
        if self.hitstop > 0.0 { 0.06 } else { 1.0 }
    }

    pub fn say(&mut self, text: impl Into<String>, secs: f32) {
        self.message = Some((text.into(), secs));
    }

    pub fn float(&mut self, pos: Vec3, value: f32, kind: FloatKind) {
        if self.floaters.len() > 160 {
            self.floaters.remove(0);
        }
        self.floaters.push(Floater { pos, value, kind, text: String::new(), age: 0.0 });
    }

    pub fn float_text(&mut self, pos: Vec3, text: impl Into<String>) {
        self.floaters.push(Floater { pos, value: 0.0, kind: FloatKind::Text, text: text.into(), age: 0.0 });
    }

    pub fn monsters_alive(&self) -> usize {
        self.actors.values().filter(|a| a.team == Team::Monster && !a.dead).count()
    }
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

fn yaw_of(d: Vec3) -> f32 {
    d.x.atan2(d.z)
}

/// Feet position and body height of a character entity (or of a prop actor, from its shape:
/// kegs, totems).
pub(crate) fn feet_of(sim: &Sim, id: EntityId) -> Option<(Vec3, f32)> {
    let e = sim.state.entities.get(id)?;
    match e.character.as_ref() {
        Some(ch) => Some((e.pos - Vec3::Y * ch.height() * 0.5, ch.height())),
        None => {
            let h = e.visual.as_ref()?.shape.half_extents().y;
            Some((e.pos - Vec3::Y * h, h * 2.0))
        }
    }
}

impl Sim {
    /// Turns the player into the hero and starts the game rules in this scene.
    pub fn start_game(&mut self, hero: Hero) {
        self.resume_game(Game::new(hero));
    }

    /// Continues a game in this (freshly built) scene: the hero, their bags and settings come
    /// along; everything that belonged to the old place is left behind.
    pub fn resume_game(&mut self, mut g: Game) {
        let pid = match self.state.player {
            Some(p) => p,
            None => self.spawn_player(),
        };
        g.actors.clear();
        g.shots.clear();
        g.effects.clear();
        g.floaters.clear();
        g.loot.clear();
        g.gold.clear();
        g.spots.clear();
        g.echoes.clear();
        g.npcs.clear();
        g.exhibits.clear();
        g.arena = None;
        g.level = None;
        g.respawn = 0.0;
        g.message = None;
        g.hitstop = 0.0;
        g.shake = 0.0;
        g.travel = None;
        g.hero.potions = g.hero.potion_max;
        g.spawn = self.state.spawn;
        let mut a = Actor::new(Team::Hero, &g.hero.name, g.hero.level, g.hero.sheet());
        a.radius = 0.42;
        g.actors.insert(pid, a);
        g.hero_id = Some(pid);
        refresh_hero(self, &mut g, true);
        self.state.game = Some(Box::new(g));
    }

    /// Puts a saved hero into the running game: gear, bags, passives, potions and waypoints,
    /// recomputed and healed; the vendor restocks for their level.
    pub fn load_hero(&mut self, hero: Hero) -> bool {
        let Some(mut g) = self.state.game.take() else { return false };
        g.hero = hero;
        g.hero.potions = g.hero.potion_max;
        refresh_hero(self, &mut g, true);
        if g.place == Place::Town {
            g.restock(self);
        }
        g.inv_changed();
        self.state.game = Some(g);
        true
    }

    /// Spawns a monster of a family (game/monsters.toml) at `feet`.
    pub fn spawn_monster(&mut self, family: &str, level: u32, rarity: Rarity, feet: Vec3, pack: u32) -> Option<EntityId> {
        let mut g = self.state.game.take()?;
        let id = spawn_monster_into(self, &mut g, family, level, rarity, feet, pack);
        self.state.game = Some(g);
        id
    }

    /// Time scale for this tick (hit-stop).
    pub(crate) fn game_time_scale(&self) -> f32 {
        self.state.game.as_ref().map(|g| g.time_scale()).unwrap_or(1.0)
    }

    pub(crate) fn game_pre(&mut self, input: &InputFrame, dt: f32, events: &mut Vec<SimEvent>) -> BTreeMap<EntityId, InputFrame> {
        let Some(mut g) = self.state.game.take() else { return BTreeMap::new() };
        let out = g.pre(self, input, dt, events);
        self.state.game = Some(g);
        out
    }

    /// Travel requested this tick: the next place is built around the same hero (same tick,
    /// same random stream, so replays and rewind stay exact).
    pub(crate) fn game_travel(&mut self, events: &mut Vec<SimEvent>) {
        let Some(dest) = self.state.game.as_mut().and_then(|g| g.travel.take()) else { return };
        let Some(place) = Place::from_code(dest) else { return };
        let g = self.state.game.take().unwrap();
        let tick = self.state.tick;
        let mut fresh = Sim::empty(self.state.seed);
        fresh.config = self.config.clone();
        fresh.state.rng = self.state.rng.clone();
        // New entity ids and region versions so nothing is mistaken for the old place.
        fresh.state.entities.next = self.state.entities.next + 1;
        scene::build_place(&mut fresh, place, *g);
        let bump = (tick + 1) << 20;
        for c in fresh.state.statics.chunks.values_mut() {
            std::sync::Arc::make_mut(c).version += bump;
        }
        fresh.state.tick = tick;
        fresh.state.scene = place.scene();
        self.config = fresh.config.clone();
        self.restore(fresh.state);
        events.push(SimEvent::Travel { place: dest });
    }

    pub(crate) fn game_post(&mut self, dt: f32, raw_dt: f32, events: &mut Vec<SimEvent>) {
        let Some(mut g) = self.state.game.take() else { return };
        g.post(self, dt, raw_dt, events);
        self.state.game = Some(g);
    }
}

/// Lands a hit on `target` (tools and tests; the game's own hits come from skills).
pub fn debug_hit(sim: &mut Sim, g: &mut Game, target: EntityId, dmg: &Damage, events: &mut Vec<SimEvent>) -> f32 {
    let from = feet_of(sim, target).map(|f| f.0).unwrap_or(Vec3::ZERO);
    g.hit(sim, target, dmg, from, events)
}

/// Recomputes the hero's numbers from the profile (level, gear, passives, difficulty) and their
/// look (weapon in hand).
pub fn refresh_hero(sim: &mut Sim, g: &mut Game, heal: bool) {
    let Some(hid) = g.hero_id else { return };
    let d = data();
    let gear = g.hero.gear(&d);
    let tree = d.tree.bonus(&g.hero.tree, &g.hero.masteries);
    let mut mods = gear.mods.clone();
    mods.merge(&tree.mods);
    mods.merge(&g.hero.mods);
    let mut powers = gear.powers.clone();
    powers.extend(tree.powers.iter().copied());
    if let Some(a) = g.actors.get(&hid) {
        for b in &a.buffs {
            mods.merge(&b.mods);
        }
    }
    g.hero.weapon = gear.weapon.clone();
    g.hero.powers = powers.clone();
    let mut sheet = Sheet::compute(g.hero.base(), &mods);
    if let Some(p) = powers.iter().find(|p| p.kind == powers::PowerKind::BloodMagic) {
        sheet.life_max += sheet.mana_max * p.a / 100.0;
        sheet.mana_max = 0.0;
    }
    sheet.life_max *= sim.config.difficulty.player_life;
    g.inv_changed();
    if let Some(a) = g.actors.get_mut(&hid) {
        a.powers = powers;
        a.tweaks = tree.tweaks;
        let frac = if a.sheet.life_max > 0.0 { a.life / a.sheet.life_max } else { 1.0 };
        let mfrac = if a.sheet.mana_max > 0.0 { a.mana / a.sheet.mana_max } else { 1.0 };
        a.level = g.hero.level;
        a.name = g.hero.name.clone();
        a.sheet = sheet;
        a.life = if heal { a.sheet.life_max } else { (frac * a.sheet.life_max).min(a.sheet.life_max) };
        a.mana = if heal { a.sheet.mana_max } else { mfrac * a.sheet.mana_max };
    }
    // What the hero looks like: weapon, off-hand and armour.
    let look = hero_look(&sim.config.puppet, &g.hero, &d);
    if let Some(ch) = sim.state.entities.get_mut(hid).and_then(|e| e.character.as_mut()) {
        ch.puppet = Some(std::sync::Arc::new(look));
    }
}

/// The hero's puppet dressed in their gear.
pub fn hero_look(base: &crate::puppet::PuppetDef, hero: &Hero, d: &data::Data) -> crate::puppet::PuppetDef {
    use items::EquipSlot as E;
    let mut look = base.clone();
    look.weapon.kind = hero.weapon.kind;
    let worn = |s: E| hero.worn(s).and_then(|it| Some((it, it.base_def(d)?)));
    if let Some((it, b)) = worn(E::Weapon) {
        look.weapon.color = b.color.clone();
        look.weapon.glow = b.glow + if it.rarity == Rarity::Unique { 0.5 } else { 0.0 };
        look.weapon.size = 1.0 + (b.level as f32 / 75.0) * 0.12;
    }
    look.weapon.offhand = crate::puppet::OffhandKind::None;
    if let Some((_, b)) = worn(E::Offhand) {
        look.weapon.offhand = b.offhand_kind();
        look.weapon.offhand_color = b.color.clone();
    }
    let gear = &mut look.gear;
    if let Some((it, b)) = worn(E::Helmet) {
        gear.helm = crate::params::ChoiceParam::from_index(
            ["none", "cap", "helm", "great", "crown", "horned", "halo"].iter().position(|k| *k == b.look).unwrap_or(2),
        );
        gear.helm_color = b.color.clone();
        gear.glow = gear.glow.max(b.glow + if it.rarity == Rarity::Unique { 0.4 } else { 0.0 });
    }
    if let Some((it, b)) = worn(E::Body) {
        look.shirt = b.color.clone();
        gear.armor_color = b.color.clone();
        gear.pauldrons = match b.look.as_str() {
            "vest" => 0.0,
            "mail" => 0.7,
            "scale" => 0.85,
            _ => 1.0,
        };
        gear.cape = if b.look.ends_with("cape") || it.rarity == Rarity::Unique { 1.0 } else { 0.0 };
        gear.cape_color = if it.rarity == Rarity::Unique { "#8a2a1a".into() } else { "#3a2f4a".into() };
    }
    if let Some((_, b)) = worn(E::Gloves) {
        gear.gloves = b.color.clone();
    }
    if let Some((_, b)) = worn(E::Boots) {
        look.shoes = b.color.clone();
    }
    if let Some((_, b)) = worn(E::Belt) {
        gear.belt = b.color.clone();
    }
    look
}

pub(crate) fn spawn_monster_into(
    sim: &mut Sim,
    g: &mut Game,
    family: &str,
    level: u32,
    rarity: Rarity,
    feet: Vec3,
    pack: u32,
) -> Option<EntityId> {
    let d = data();
    let spec = d.family(family)?.spec();
    spawn_spec_into(sim, g, &spec, level, rarity, feet, pack)
}

/// Spawns a monster from a spec (designed family or genome). Magic monsters roll one affix,
/// rares two or three and a name of their own.
pub fn spawn_spec_into(
    sim: &mut Sim,
    g: &mut Game,
    spec: &genome::MonsterSpec,
    level: u32,
    rarity: Rarity,
    feet: Vec3,
    pack: u32,
) -> Option<EntityId> {
    let d = data();
    let n_affixes = match rarity {
        Rarity::Magic => 1,
        Rarity::Rare => 2 + sim.state.rng.below(2) as usize,
        _ => 0,
    };
    let affixes = genome::roll_affixes(&d, &mut sim.state.rng, level, n_affixes);
    let mut spec = spec.clone();
    let (mods, affix_names) = genome::apply_affixes(&d, &mut spec, &affixes);
    let name = if rarity == Rarity::Rare { genome::rare_name(&mut sim.state.rng) } else { spec.name.clone() };
    spawn_actor(sim, g, &spec, &name, level, rarity, feet, pack, mods, affix_names)
}

/// Puts a monster in the world: entity, stats, brain.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_actor(
    sim: &mut Sim,
    g: &mut Game,
    spec: &genome::MonsterSpec,
    name: &str,
    level: u32,
    rarity: Rarity,
    feet: Vec3,
    pack: u32,
    mods: Mods,
    affixes: Vec<String>,
) -> Option<EntityId> {
    let mut look = spec.puppet.clone();
    if rarity >= Rarity::Rare {
        look.scale *= 1.15;
    }
    let facing = sim.state.rng.range(-3.1, 3.1);
    let id = sim.spawn_npc(name, feet, facing, look.clone(), None, None);
    let diff = sim.config.difficulty.clone();
    let base = Base {
        life: monster_life(level) * spec.life * rarity.life_mult() * diff.enemy_life,
        mana: 100.0,
        life_regen: 0.0,
        mana_regen: 10.0,
        armor: level as f32 * 2.0,
        res: 0.0,
    };
    let mut a = Actor::new(Team::Monster, name, level, Sheet::compute(base, &mods));
    a.base = base;
    a.mods = mods;
    a.family = spec.key.clone();
    a.rarity = rarity;
    a.radius = spec.radius * look.scale;
    a.base_damage = monster_damage(level) * spec.damage * rarity.damage_mult();
    a.speed = spec.speed * MONSTER_PACE;
    a.skills = spec.skills.clone();
    a.brain = Some(Brain::new(spec.brain, feet));
    a.xp = monster_xp(level) * spec.xp * rarity.xp_mult();
    a.pack = pack;
    a.powers = spec.powers.clone();
    a.tweaks = spec.tweaks.clone();
    a.genome = spec.genome;
    a.affixes = affixes;
    a.life = a.sheet.life_max;
    a.mana = a.sheet.mana_max;
    g.actors.insert(id, a);
    Some(id)
}

impl Game {
    // ------------------------------------------------------------------ before movement
    fn pre(&mut self, sim: &mut Sim, input: &InputFrame, dt: f32, events: &mut Vec<SimEvent>) -> BTreeMap<EntityId, InputFrame> {
        let d = data();
        let mut out = BTreeMap::new();
        if let Some(c) = input.cmd {
            self.command(sim, c, events);
        }
        if let Some(hid) = self.hero_id {
            self.hero_input(sim, hid, input, &mut out, events);
        }
        if !self.npcs.is_empty() {
            scene::npc_inputs(self, sim, dt, &mut out);
        }
        let hero = self.hero_id.and_then(|h| {
            let a = self.actors.get(&h)?;
            if a.dead {
                return None;
            }
            Some((feet_of(sim, h)?.0, a.radius))
        });
        let speed_k = sim.config.difficulty.enemy_speed;
        // Levels: monsters chase the hero around walls along a flow field.
        let flow = match (self.level.as_mut(), hero) {
            (Some(lv), Some((hf, _))) => lv.hero_flow(sim, &self.actors, hf).zip(lv.nav.clone()),
            _ => None,
        };
        let ids: Vec<EntityId> = self.actors.iter().filter(|(_, a)| a.brain.is_some()).map(|(id, _)| *id).collect();
        // Waking up: packs aggro together.
        let mut woke = Vec::new();
        for id in &ids {
            let (Some((feet, _)), Some((hf, _))) = (feet_of(sim, *id), hero) else { continue };
            let a = &self.actors[id];
            if !a.dead && !a.brain.as_ref().unwrap().aggro && flat(hf - feet).length() < AGGRO_RANGE {
                woke.push((a.pack, feet));
            }
        }
        for (pack, at) in woke {
            for id in &ids {
                let Some((feet, _)) = feet_of(sim, *id) else { continue };
                let a = self.actors.get_mut(id).unwrap();
                if a.pack == pack && flat(feet - at).length() < PACK_RANGE.max(AGGRO_RANGE) {
                    a.brain.as_mut().unwrap().aggro = true;
                }
            }
        }
        let mut burst = Vec::new();
        for id in ids {
            let Some((feet, _)) = feet_of(sim, id) else { continue };
            let a = self.actors.get_mut(&id).unwrap();
            let mut inp = InputFrame::default();
            if !a.dead && !a.frozen() && a.cast.is_none() {
                let options: Vec<SkillOption> =
                    a.skills.iter().map(|s| SkillOption { id: *s, def: d.skill(*s), ready: a.cooldown(*s) <= 0.0 }).collect();
                let reach = a.radius + hero.map(|h| h.1).unwrap_or(0.4);
                let dec = a.brain.as_mut().unwrap().think(feet, hero.map(|h| h.0), reach, &options, &mut sim.state.rng, dt);
                inp.move_dir = dec.move_dir;
                if let (Some((field, grid)), Some((hf, _))) = (&flow, hero) {
                    let to = flat(hf - feet);
                    let mv = Vec3::new(inp.move_dir.x, 0.0, inp.move_dir.y);
                    let chasing = mv.length() > 0.1 && to.length() > 1.6 && mv.normalize().dot(to.normalize()) > 0.3;
                    if chasing && !grid.clear(feet, hf) {
                        if let Some(d) = field.dir(grid, feet) {
                            inp.move_dir = glam::Vec2::new(d.x, d.z) * mv.length().min(1.0);
                        }
                    }
                }
                if let Some((skill, target)) = dec.cast {
                    skills::try_cast(self, sim, id, skill, target);
                }
                if dec.detonate {
                    burst.push(id);
                }
            }
            let a = &self.actors[&id];
            let chill = a.ailments.chill.0;
            if let Some(ch) = sim.state.entities.get_mut(id).and_then(|e| e.character.as_mut()) {
                ch.haste = a.speed * a.sheet.move_speed * speed_k * (1.0 - chill) - 1.0;
                ch.slow = if a.dead || a.frozen() || a.cast.is_some() { 1.0 } else { 0.0 };
                ch.face = a.cast.as_ref().map(|c| yaw_of(c.dir));
            }
            out.insert(id, inp);
        }
        // Bombers that reached you.
        for id in burst {
            self.kill(sim, id, events);
        }
        // Skills that move their user (leaps, dashes, charges).
        let casting: Vec<EntityId> = self.actors.iter().filter(|(_, a)| a.cast.is_some()).map(|(id, _)| *id).collect();
        for id in casting {
            skills::steer_cast(self, sim, id);
        }
        out
    }

    fn hero_input(
        &mut self,
        sim: &mut Sim,
        hid: EntityId,
        input: &InputFrame,
        out: &mut BTreeMap<EntityId, InputFrame>,
        events: &mut Vec<SimEvent>,
    ) {
        let d = data();
        let Some((feet, _)) = feet_of(sim, hid) else { return };
        let facing = sim.state.entities.get(hid).and_then(|e| e.character.as_ref()).map(|c| c.facing).unwrap_or(0.0);
        let Some(a) = self.actors.get(&hid) else { return };
        if a.dead {
            out.insert(hid, InputFrame::default());
            if let Some(ch) = sim.state.entities.get_mut(hid).and_then(|e| e.character.as_mut()) {
                ch.slow = 1.0;
            }
            return;
        }
        let frozen = a.frozen();
        let aim = input.aim.map(flat).unwrap_or(feet + Vec3::new(facing.sin(), 0.0, facing.cos()) * 4.0);
        let aim = Vec3::new(aim.x, feet.y, aim.z);
        // Dodge: cancels whatever you were doing.
        if input.just(buttons::DODGE) && a.dodge_cd <= 0.0 && !frozen {
            let mv = Vec3::new(input.move_dir.x, 0.0, input.move_dir.y);
            let dir = if mv.length() > 0.2 { mv.normalize() } else { Vec3::new(facing.sin(), 0.0, facing.cos()) };
            let recovery = DODGE_RECOVERY / a.sheet.dodge_recovery.max(0.2);
            let a = self.actors.get_mut(&hid).unwrap();
            a.cast = None;
            a.queued = None;
            a.iframes = DODGE_TIME + 0.06;
            a.dodge_cd = recovery;
            if let Some(ch) = sim.state.entities.get_mut(hid).and_then(|e| e.character.as_mut()) {
                ch.dash_vel = dir * DODGE_SPEED;
                ch.dash_time = DODGE_TIME;
                ch.dash_roll = true;
                ch.face = Some(yaw_of(dir));
            }
            events.push(SimEvent::Roll { pos: feet });
        }
        // Potion: a big heal over a moment.
        if input.just(buttons::POTION) && self.hero.potions > 0 {
            let a = self.actors.get_mut(&hid).unwrap();
            if a.life < a.sheet.life_max {
                self.hero.potions -= 1;
                let heal = a.sheet.life_max * 0.45 * a.sheet.potion;
                a.life = (a.life + heal).min(a.sheet.life_max);
                self.float(feet + Vec3::Y * 2.0, heal, FloatKind::Heal);
                events.push(SimEvent::Potion { pos: feet });
            }
        }
        // Skills: a fresh press wins over a held button; held buttons keep repeating.
        let slots = [buttons::PRIMARY, buttons::SECONDARY, buttons::SKILL3, buttons::SKILL4, buttons::SKILL5, buttons::SKILL6];
        let want = slots.iter().position(|b| input.just(*b)).or_else(|| slots.iter().position(|b| input.down(*b)));
        if let Some(slot) = want.filter(|_| !frozen) {
            if let Some(skill) = d.skill_id(&self.hero.bar[slot]) {
                let a = &self.actors[&hid];
                let dodging = sim
                    .state
                    .entities
                    .get(hid)
                    .and_then(|e| e.character.as_ref())
                    .is_some_and(|c| c.dash_time > 0.0 && c.dash_roll);
                match &a.cast {
                    None if !dodging => {
                        if skills::try_cast(self, sim, hid, skill, aim) {
                            if let Some(c) = self.actors.get_mut(&hid).and_then(|a| a.cast.as_mut()) {
                                c.button = slots[slot];
                            }
                        }
                    }
                    Some(c) if c.fired && c.dur - c.t < c.dur * 0.45 && input.just(slots[slot]) => {
                        self.actors.get_mut(&hid).unwrap().queued = Some((skill, aim));
                    }
                    _ => {}
                }
            }
        }
        // Channels last while their button is held.
        let channel_released = self.actors[&hid]
            .cast
            .as_ref()
            .is_some_and(|c| d.skill(c.skill).behavior == Behavior::Channel && c.button != 0 && !input.down(c.button));
        if channel_released {
            self.actors.get_mut(&hid).unwrap().cast = None;
        }
        let a = &self.actors[&hid];
        let def = a.cast.as_ref().map(|c| d.skill(c.skill));
        let stand = input.down(buttons::FOCUS);
        if let Some(ch) = sim.state.entities.get_mut(hid).and_then(|e| e.character.as_mut()) {
            ch.haste = a.sheet.move_speed * (1.0 - a.ailments.chill.0) - 1.0;
            ch.slow = match (def, &a.cast) {
                _ if frozen => 1.0,
                (Some(def), Some(c)) => match def.behavior {
                    Behavior::Dash | Behavior::Leap | Behavior::Charge => 0.0,
                    Behavior::Channel => 0.45,
                    _ if stand => 1.0,
                    _ if !c.fired => 0.72,
                    _ => 0.35,
                },
                _ => 0.0,
            };
            let channeling = def.is_some_and(|d| d.behavior == Behavior::Channel);
            if !(ch.dash_time > 0.0 && ch.dash_roll) {
                ch.face = if channeling { None } else { a.cast.as_ref().map(|c| yaw_of(c.dir)) };
            }
        }
        let mut movement = if frozen { InputFrame::default() } else { *input };
        // The primary skill already consumed this attack; the character controller must
        // not also throw a demo bomb or fire its blaster.
        movement.held &= !buttons::PRIMARY;
        movement.pressed &= !buttons::PRIMARY;
        out.insert(hid, movement);
    }

    // ------------------------------------------------------------------ after physics
    fn post(&mut self, sim: &mut Sim, dt: f32, raw_dt: f32, events: &mut Vec<SimEvent>) {
        let d = data();
        self.time += dt;
        // Casts: land hits, finish, start the buffered one.
        let ids: Vec<EntityId> = self.actors.keys().copied().collect();
        for id in &ids {
            skills::advance_cast(self, sim, *id, dt, events);
        }
        skills::update_shots(self, sim, dt, events);
        skills::update_effects(self, sim, dt, events);
        self.power_tick(sim, dt, events);
        self.tick_monster_powers(sim, dt);
        self.update_bosses(sim, events);
        self.update_loot(sim, dt, events);
        // Ailments, regeneration, timers.
        for id in &ids {
            self.tick_actor(sim, *id, dt, events);
        }
        // Deaths: topple, sink, vanish; the hero gets back up in a moment.
        let mut gone = Vec::new();
        for (id, a) in self.actors.iter_mut() {
            if a.dead {
                a.death_t += dt;
                if a.team != Team::Hero && a.death_t > 1.7 {
                    gone.push(*id);
                }
            }
        }
        for id in gone {
            self.actors.remove(&id);
            sim.despawn(id);
        }
        if let Some(hid) = self.hero_id {
            if self.actors.get(&hid).is_some_and(|a| a.dead) {
                self.respawn -= dt;
                if self.respawn <= 0.0 {
                    let spawn = self.spawn;
                    sim.set_position(hid, spawn);
                    let a = self.actors.get_mut(&hid).unwrap();
                    a.dead = false;
                    a.death_t = 0.0;
                    a.ailments = Ailments::default();
                    a.iframes = 2.0;
                    refresh_hero(sim, self, true);
                    self.hero.potions = self.hero.potion_max;
                }
            }
        }
        // Animation state for every actor.
        for (id, a) in &self.actors {
            let Some(ch) = sim.state.entities.get_mut(*id).and_then(|e| e.character.as_mut()) else { continue };
            match &a.cast {
                Some(c) => {
                    let def = d.skill(c.skill);
                    let t = if def.behavior == Behavior::Channel {
                        // Spinning: one turn per pulse.
                        ((c.t - c.hit_at).max(0.0) / def.interval.max(0.05)).fract()
                    } else {
                        (c.t / c.dur.max(1e-3)).clamp(0.0, 1.0)
                    };
                    ch.anim.set_action(def.anim, t, def.hit, c.side);
                    ch.anim.lift = if def.behavior == Behavior::Leap {
                        let start = c.dur * 0.15;
                        let p = ((c.t - start) / (c.hit_at - start).max(1e-3)).clamp(0.0, 1.0);
                        (p * std::f32::consts::PI).sin() * 1.5
                    } else {
                        0.0
                    };
                }
                None => {
                    ch.anim.set_action(crate::moves::MoveId::NONE, 0.0, 0.0, 1.0);
                    ch.anim.lift = 0.0;
                }
            }
            ch.anim.down = if a.dead { (a.death_t / 0.9).min(1.0) } else { 0.0 };
            // The hero falls with a captured death and lies still until rising again (monsters
            // topple: their bodies have to clear quickly).
            if a.team == Team::Hero && ch.puppet.as_ref().is_none_or(|p| p.body == crate::puppet::BodyPlan::Biped) {
                // (The count already includes this death.)
                let death =
                    crate::clips::find_cached(HERO_DEATHS[(self.hero.deaths as usize).saturating_sub(1) % HERO_DEATHS.len()]);
                if a.dead && death != 0 && ch.anim.clip != death {
                    ch.anim.play_clip(death, 0, 1.0);
                } else if !a.dead && HERO_DEATHS.iter().any(|d| crate::clips::find_cached(d) == ch.anim.clip) {
                    ch.anim.stop_clip();
                }
            }
        }
        // Floating numbers and banners run on real time.
        for f in &mut self.floaters {
            f.age += raw_dt;
            f.pos.y += raw_dt * (1.4 - f.age).max(0.2);
        }
        self.floaters.retain(|f| f.age < 1.1);
        if let Some((_, t)) = &mut self.message {
            *t -= raw_dt;
            if *t <= 0.0 {
                self.message = None;
            }
        }
        self.hitstop = (self.hitstop - raw_dt).max(0.0);
        self.gib_load = (self.gib_load - 30.0 * dt).max(0.0);
        self.tick_streak(sim, dt, events);
        self.shake *= (-9.0 * raw_dt).exp();
        self.level_flash += raw_dt;
        scene::update_arena(self, sim, dt);
        mechanics::update_level(self, sim, dt, events);
        scene::update_npcs(self, sim, dt, events);
        if self.inv_cache.is_none() {
            self.inv_cache = Some(Arc::new(self.inv_view()));
        }
    }

    fn tick_actor(&mut self, sim: &mut Sim, id: EntityId, dt: f32, events: &mut Vec<SimEvent>) {
        let Some(a) = self.actors.get_mut(&id) else { return };
        a.flash = (a.flash - dt).max(0.0);
        a.iframes = (a.iframes - dt).max(0.0);
        a.dodge_cd = (a.dodge_cd - dt).max(0.0);
        for c in &mut a.cooldowns {
            c.1 = (c.1 - dt).max(0.0);
        }
        if a.cast.is_none() {
            a.combo_timer = (a.combo_timer - dt).max(0.0);
        }
        let mut buffs_changed = false;
        for b in &mut a.buffs {
            b.time -= dt;
        }
        a.buffs.retain(|b| {
            let keep = b.time > 0.0;
            buffs_changed |= !keep;
            keep
        });
        if a.dead {
            return;
        }
        let ail = &mut a.ailments;
        ail.freeze = (ail.freeze - dt).max(0.0);
        ail.chill.1 = (ail.chill.1 - dt).max(0.0);
        if ail.chill.1 <= 0.0 {
            ail.chill.0 = 0.0;
        }
        ail.shock.1 = (ail.shock.1 - dt).max(0.0);
        if ail.shock.1 <= 0.0 {
            ail.shock.0 = 0.0;
        }
        let mut dot = 0.0;
        for x in [&mut ail.bleed, &mut ail.ignite] {
            if x.time > 0.0 {
                dot += x.dps * dt;
                x.time -= dt;
            }
        }
        for p in &mut ail.poison {
            dot += p.dps * dt;
            p.time -= dt;
        }
        ail.poison.retain(|p| p.time > 0.0);
        let killer = ail.ignite.source.or(ail.bleed.source).or(ail.poison.first().and_then(|p| p.source));
        a.life -= dot * a.sheet.taken;
        a.life = (a.life + a.sheet.life_regen * dt).min(a.sheet.life_max);
        a.mana = (a.mana + a.sheet.mana_regen * dt).min(a.sheet.mana_max);
        if a.life <= 0.0 {
            if a.last_hit.is_none() {
                a.last_hit = killer;
            }
            self.kill(sim, id, events);
        }
        if buffs_changed {
            if Some(id) == self.hero_id {
                refresh_hero(sim, self, false);
            } else if let Some(a) = self.actors.get_mut(&id) {
                a.recompute();
            }
        }
    }

    /// Lands a hit on `target` from `from` (for knockback). Returns the damage dealt.
    pub(crate) fn hit(&mut self, sim: &mut Sim, target: EntityId, dmg: &Damage, from: Vec3, events: &mut Vec<SimEvent>) -> f32 {
        let diff = sim.config.difficulty.clone();
        let Some((feet, height)) = feet_of(sim, target) else { return 0.0 };
        let Some(t) = self.actors.get_mut(&target) else { return 0.0 };
        if t.dead || t.iframes > 0.0 {
            return 0.0;
        }
        let head = feet + Vec3::Y * (height + 0.3);
        if t.team == Team::Hero && dmg.attack {
            let rng = &mut sim.state.rng;
            if rng.f32() * 100.0 < t.sheet.evasion {
                self.float_text(head, "Evade");
                return 0.0;
            }
            if rng.f32() * 100.0 < t.sheet.block {
                let nova = t.power(powers::PowerKind::BlockNova);
                self.float_text(head, "Block");
                events.push(SimEvent::Block { pos: feet });
                if let Some(p) = nova {
                    self.block_nova(sim, target, feet, p, events);
                }
                return 0.0;
            }
        }
        // The attacker's powers: crits freeze (and hit frozen things harder).
        let mut dmg = std::borrow::Cow::Borrowed(dmg);
        let src_powers = dmg.source.and_then(|s| self.actors.get(&s)).map(|s| s.powers.clone()).unwrap_or_default();
        let Some(t) = self.actors.get_mut(&target) else { return 0.0 };
        if dmg.crit {
            if let Some(p) = src_powers.iter().find(|p| p.kind == powers::PowerKind::FrostCrits) {
                if t.frozen() {
                    dmg.to_mut().amount.iter_mut().for_each(|x| *x *= 1.0 + p.a / 100.0);
                }
            }
        }
        let dmg = dmg.as_ref();
        let mut parts = mitigate(dmg, t);
        if let Some(p) = t.power(powers::PowerKind::StandFirm).filter(|_| t.cast.is_some()) {
            parts.iter_mut().for_each(|x| *x *= (1.0 - p.a / 100.0).max(0.0));
        }
        let mut total: f32 = parts.iter().sum();
        if let Some(p) = t.power(powers::PowerKind::ManaShield) {
            let absorb = (total * p.a / 100.0).min(t.mana);
            t.mana -= absorb;
            total -= absorb;
        }
        t.life -= total;
        t.flash = 0.12;
        t.last_hit = dmg.source;
        if let Some(b) = &mut t.brain {
            b.aggro = true;
        }
        let life_max = t.sheet.life_max.max(1.0);
        let boss = t.rarity == Rarity::Unique;
        // Ailments.
        let rng = &mut sim.state.rng;
        let am = dmg.ailment_mult.max(0.1);
        if parts[0] > 0.0 && rng.f32() < dmg.ailment[0] {
            let dps = parts[0] * 0.7 / 4.0 * am;
            if dps > t.ailments.bleed.dps || t.ailments.bleed.time <= 0.0 {
                t.ailments.bleed = Dot { dps, time: 4.0, source: dmg.source };
            }
        }
        if parts[1] > 0.0 && rng.f32() < dmg.ailment[1] {
            let dps = parts[1] * 0.9 / 4.0 * am;
            if dps > t.ailments.ignite.dps || t.ailments.ignite.time <= 0.0 {
                t.ailments.ignite = Dot { dps, time: 4.0, source: dmg.source };
            }
        }
        if parts[2] > 0.0 {
            let amount = (parts[2] / life_max * 3.0).clamp(0.12, 0.5);
            t.ailments.chill = (t.ailments.chill.0.max(amount), 2.0);
            if rng.f32() < dmg.ailment[2] || parts[2] >= 0.2 * life_max {
                let time = (0.5 + parts[2] / life_max * 2.0).min(1.8) * if boss { 0.35 } else { 1.0 };
                t.ailments.freeze = t.ailments.freeze.max(time);
                if let Some(c) = &t.cast {
                    if !c.fired {
                        t.cast = None;
                    }
                }
            }
        }
        if dmg.crit && src_powers.iter().any(|p| p.kind == powers::PowerKind::FrostCrits) {
            t.ailments.freeze = t.ailments.freeze.max(if boss { 0.4 } else { 1.2 });
            if t.cast.as_ref().is_some_and(|c| !c.fired) {
                t.cast = None;
            }
        }
        if let Some(p) = src_powers.iter().find(|p| p.kind == powers::PowerKind::Execute) {
            if !boss && t.life > 0.0 && t.life < life_max * p.a / 100.0 {
                t.life = 0.0;
            }
        }
        if parts[3] > 0.0 && rng.f32() < dmg.ailment[3] {
            let amount = (parts[3] / life_max * 2.0).clamp(0.1, 0.5);
            t.ailments.shock = (t.ailments.shock.0.max(amount), 4.0);
        }
        if parts[4] > 0.0 && rng.f32() < dmg.ailment[4] && t.ailments.poison.len() < 25 {
            t.ailments.poison.push(Dot { dps: parts[4] * 0.3 / 2.0 * am, time: 2.0, source: dmg.source });
        }
        // Frozen on ice: one more hit and they shatter.
        let shattered =
            t.team == Team::Monster && !boss && t.life > 0.0 && t.frozen() && self.level.as_ref().is_some_and(|l| l.on_ice(feet));
        if shattered {
            t.life = 0.0;
        }
        let team = t.team;
        let immovable = t.immovable || boss || t.has_power(powers::PowerKind::StandFirm);
        let killed = t.life <= 0.0;
        let kind = if dmg.crit { FloatKind::Crit(dmg.main_element() as u8) } else { FloatKind::Damage(dmg.main_element() as u8) };
        self.float(head, total, kind);
        if shattered {
            self.float_text(head + Vec3::Y * 0.4, "Shatter!");
            events.push(SimEvent::Break { pos: feet + Vec3::Y * 0.6 });
        }
        // Knockback and flinch.
        let dir = flat(feet - from).normalize_or(Vec3::X);
        if let Some(ch) = sim.state.entities.get_mut(target).and_then(|e| e.character.as_mut()) {
            if !immovable && dmg.knockback > 0.0 {
                ch.impulse += dir * dmg.knockback * if team == Team::Hero { 0.6 } else { 1.0 };
                if dmg.knockback >= 2.0 && team != Team::Hero {
                    ch.stun = ch.stun.max(0.18);
                }
            }
            ch.anim.hit(dir, 0.5 + 0.25 * dmg.knockback.min(4.0) + if dmg.crit { 0.5 } else { 0.0 });
        }
        // The attacker: leech and on-hit gains; the hero's hits stop time a little.
        if let Some(src) = dmg.source {
            let gain = data().skill(dmg.skill).mana_gain;
            if let Some(s) = self.actors.get_mut(&src) {
                if !s.dead {
                    s.life = (s.life + s.sheet.life_on_hit + total * s.sheet.life_leech).min(s.sheet.life_max);
                    s.mana = (s.mana + s.sheet.mana_on_hit + gain).min(s.sheet.mana_max);
                }
                if s.team == Team::Hero {
                    let d = data();
                    let def = d.skill(dmg.skill);
                    let stop = def.hitstop * if dmg.crit { 1.7 } else { 1.0 } * diff.hitstop;
                    self.hitstop = self.hitstop.max(stop);
                    self.shake = (self.shake + def.shake * 0.5 * diff.shake).min(1.5);
                }
            }
        }
        if team == Team::Hero {
            self.shake = (self.shake + (total / life_max * 2.0).min(0.6) * diff.shake).min(1.5);
        }
        events.push(SimEvent::Strike {
            pos: feet + Vec3::Y * height * 0.6,
            power: total,
            element: dmg.main_element() as u8,
            crit: dmg.crit,
        });
        if killed {
            // A crushing blow bursts it apart.
            if team == Team::Monster && (dmg.crit || total >= life_max * 0.6 || shattered) {
                self.gibs(sim, target, feet, height, from, (total / life_max).min(2.0));
            }
            self.kill(sim, target, events);
        }
        total
    }

    /// Kill streaks: kills chained within 1.6 s of each other pay a bonus when the chain ends
    /// (from six kills: +3% of the chain's xp per kill past five, up to +75%).
    fn tick_streak(&mut self, sim: &mut Sim, dt: f32, events: &mut Vec<SimEvent>) {
        if self.streak.0 == 0 {
            return;
        }
        self.streak.1 -= dt;
        if self.streak.1 > 0.0 {
            return;
        }
        let (n, _, xp) = std::mem::take(&mut self.streak);
        if n < 6 {
            return;
        }
        let bonus = (xp * (0.03 * (n - 5) as f64).min(0.75)).round();
        let ups = self.hero.gain_xp(bonus);
        let name = match n {
            6..=11 => "Rampage",
            12..=24 => "Massacre",
            _ => "Annihilation",
        };
        if let Some(hf) = self.hero_id.and_then(|h| feet_of(sim, h)).map(|f| f.0) {
            self.float_text(hf + Vec3::Y * 2.7, format!("{name}! x{n}  +{bonus} xp"));
            events.push(SimEvent::Coin { pos: hf });
            if ups > 0 {
                refresh_hero(sim, self, true);
                self.level_flash = 0.0;
                events.push(SimEvent::LevelUp { pos: hf });
                self.hero_gesture(sim, HERO_LEVEL_UP);
            }
        }
    }

    /// Chunks of a monster's colours that fly from the blow and bounce about (physics bodies).
    fn gibs(&mut self, sim: &mut Sim, id: EntityId, feet: Vec3, height: f32, from: Vec3, power: f32) {
        if self.gib_load > 70.0 {
            return;
        }
        let Some(p) = sim.state.entities.get(id).and_then(|e| e.character.as_ref()?.puppet.clone()) else { return };
        let cols: Vec<crate::color::Color> =
            [&p.skin, &p.shirt, &p.accent].iter().filter_map(|c| crate::color::Color::try_hex(c)).collect();
        if cols.is_empty() {
            return;
        }
        let n = (4.0 + power * 4.0).min(10.0) as usize;
        self.gib_load += n as f32;
        let away = flat(feet - from).normalize_or(Vec3::X);
        let hz = sim.config.tick_rate.hz() as f32;
        for i in 0..n {
            let rng = &mut sim.state.rng;
            let at = feet + Vec3::new(rng.range(-0.25, 0.25), rng.range(0.25, 0.9) * height, rng.range(-0.25, 0.25));
            let s = rng.range(0.05, 0.12) * p.scale.max(0.5);
            let half = Vec3::new(s, s * rng.range(0.6, 1.0), s * rng.range(0.7, 1.3));
            let dir = (away + Vec3::new(rng.range(-0.7, 0.7), 0.0, rng.range(-0.7, 0.7))).normalize_or(away);
            let vel = dir * rng.range(2.0, 4.5) * (0.8 + power * 0.4) + Vec3::Y * rng.range(2.0, 4.5);
            let rot = glam::Quat::from_euler(glam::EulerRot::XYZ, rng.range(0.0, 3.0), rng.range(0.0, 3.0), 0.0);
            let life = (rng.range(1.8, 2.8) * hz) as u32;
            let mut v = crate::shape::Visual::new(crate::shape::Shape::Box { half }, cols[i % cols.len()]);
            v.look = crate::shape::Look::Lit;
            let gid = sim.spawn(crate::entity::Spawn::new("~gib", at).visual(v).body(crate::entity::BodyKind::Dynamic).rot(rot));
            if let Some(e) = sim.state.entities.get_mut(gid) {
                e.lifetime = Some(life);
                if let Some(bd) = e.body.and_then(|h| sim.state.physics.bodies.get_mut(h)) {
                    bd.set_linvel(vel, true);
                }
            }
        }
    }

    /// Something died: rewards for the hero, or the hero is down.
    pub(crate) fn kill(&mut self, sim: &mut Sim, id: EntityId, events: &mut Vec<SimEvent>) {
        let Some((feet, _)) = feet_of(sim, id) else { return };
        let Some(a) = self.actors.get_mut(&id) else { return };
        if a.dead {
            return;
        }
        a.dead = true;
        a.death_t = 0.0;
        a.life = 0.0;
        a.cast = None;
        a.ailments = Ailments::default();
        let (team, xp, level, rarity, radius, life_max, killer) =
            (a.team, a.xp, a.level, a.rarity, a.radius, a.sheet.life_max, a.last_hit);
        let was_boss = a.boss.is_some();
        let burst = a.power(powers::PowerKind::DeathBurst);
        events.push(SimEvent::Slain { pos: feet, size: radius });
        if let Some(p) = burst {
            self.death_burst(sim, team, feet, life_max / rarity.life_mult(), p);
        }
        if team == Team::Hero {
            self.hero.deaths += 1;
            self.respawn = 3.0;
            self.say("You fell. Rising again...", 3.0);
            return;
        }
        if team != Team::Monster {
            return;
        }
        // Rewards.
        self.hero.kills += 1;
        if was_boss {
            self.hero_gesture(sim, HERO_BOSS_DOWN);
        }
        if let Some(k) = killer {
            self.power_on_kill(sim, k, feet, life_max, events);
        }
        let Some(hid) = self.hero_id else { return };
        let (xp_gain, on_kill) = match self.actors.get(&hid) {
            Some(h) => (h.sheet.xp_gain, h.sheet.life_on_kill),
            None => (1.0, 0.0),
        };
        let ups = self.hero.gain_xp((xp * xp_gain) as f64);
        self.streak = (self.streak.0 + 1, 1.6, self.streak.2 + (xp * xp_gain) as f64);
        if let Some(h) = self.actors.get_mut(&hid) {
            h.life = (h.life + on_kill).min(h.sheet.life_max);
        }
        self.drop_loot(sim, feet, level, rarity, events);
        let rng = &mut sim.state.rng;
        if self.hero.potions < self.hero.potion_max && rng.f32() < [0.06, 0.2, 0.6, 1.0][rarity as usize] {
            self.hero.potions += 1;
            self.float_text(feet + Vec3::Y * 1.6, "+ Potion");
        }
        if ups > 0 {
            let hf = feet_of(sim, hid).map(|f| f.0).unwrap_or(feet);
            refresh_hero(sim, self, true);
            self.level_flash = 0.0;
            self.say(
                format!(
                    "Level {}  ·  {} passive point{} (P)",
                    self.hero.level,
                    self.hero.points(),
                    if self.hero.points() == 1 { "" } else { "s" }
                ),
                2.5,
            );
            events.push(SimEvent::LevelUp { pos: hf });
            self.hero_gesture(sim, HERO_LEVEL_UP);
        }
    }

    /// The hero acts out a moment with a captured gesture on the upper body (it fades out at its
    /// end, and any action interrupts it).
    fn hero_gesture(&self, sim: &mut Sim, name: &str) {
        let Some(hid) = self.hero_id else { return };
        let id = crate::clips::find_cached(name);
        if let Some(ch) = sim.state.entities.get_mut(hid).and_then(|e| e.character.as_mut()) {
            if id != 0 && ch.anim.act_kind == 0 && ch.anim.down <= 0.0 {
                ch.anim.play_clip(id, crate::clips::UPPER | crate::clips::ONCE, 1.0);
            }
        }
    }
}

// ---------------------------------------------------------------------- what the view needs

/// A skill slot on the HUD.
#[derive(Clone, Debug, Default)]
pub struct SlotHud {
    pub key: String,
    pub name: String,
    pub cooldown: f32,
    pub cooldown_max: f32,
    pub cost: f32,
    pub affordable: bool,
    pub element: u8,
    pub spell: bool,
}

#[derive(Clone, Debug, Default)]
pub struct HeroHud {
    pub name: String,
    pub level: u32,
    pub xp: f64,
    pub xp_next: f64,
    pub life: f32,
    pub life_max: f32,
    pub mana: f32,
    pub mana_max: f32,
    pub gold: u64,
    pub potions: u32,
    pub potion_max: u32,
    pub slots: Vec<SlotHud>,
    pub dead: bool,
    pub respawn: f32,
    pub dodge_cd: f32,
    pub level_flash: f32,
    pub kills: u64,
}

/// A monster (or the hero) as the HUD sees it.
#[derive(Clone, Debug)]
pub struct ActorView {
    pub id: EntityId,
    pub feet: Vec3,
    pub height: f32,
    pub radius: f32,
    pub team: Team,
    pub name: String,
    pub level: u32,
    pub rarity: Rarity,
    pub life: f32,
    pub dead: bool,
    pub aggro: bool,
    /// Bleed, ignite, chill, freeze, shock, poison.
    pub ailments: [bool; 6],
    pub affixes: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub enum TeleShape {
    Arc { center: Vec3, dir: Vec3, range: f32, angle: f32 },
    Circle { center: Vec3, radius: f32 },
    Line { from: Vec3, dir: Vec3, length: f32, width: f32 },
}

/// A monster attack winding up: where it will land and how close it is (0..1).
#[derive(Clone, Copy, Debug)]
pub struct Telegraph {
    pub shape: TeleShape,
    pub progress: f32,
    pub color: [f32; 3],
}

/// The hero's bags, gear and the vendor's wares (shared between frames until they change).
#[derive(Clone, Debug, Default)]
pub struct InvView {
    pub rev: u64,
    pub equipment: Vec<Option<items::Item>>,
    pub inventory: Vec<items::Item>,
    pub stash: Vec<items::Item>,
    pub vendor: Vec<items::Item>,
    pub buyback: Vec<items::Item>,
    pub gold: u64,
    pub bar: [String; 6],
    pub level: u32,
    /// Auto-loot filter: rarity index, 4 = off.
    pub auto_loot: u8,
    pub sheet: Sheet,
    pub weapon: hero::WeaponStats,
    pub powers: Vec<powers::Power>,
    /// The passive tree: allocated nodes, mastery picks, points left and what changes cost.
    pub tree: std::collections::BTreeSet<u32>,
    pub masteries: BTreeMap<u32, u8>,
    pub points: u32,
    pub refund_cost: u64,
    pub respec_cost: u64,
    pub tweaks: Vec<skills::Tweak>,
    /// Deepest level reached (waypoints).
    pub max_depth: u32,
    /// The alchemist's brews (more potions, stronger potions): price, or None when maxed.
    pub brew: [Option<u64>; 2],
    pub potion_max: u32,
}

/// An item on the ground as the HUD and view see it.
#[derive(Clone, Debug)]
pub struct LootView {
    pub id: u32,
    pub pos: Vec3,
    pub name: String,
    pub rarity: Rarity,
    pub slot: items::Slot,
    pub color: [f32; 3],
    pub rest: bool,
    pub age: f32,
}

/// Something usable nearby (vendor, stash, portal).
#[derive(Clone, Debug)]
pub struct SpotView {
    pub kind: SpotKind,
    pub name: String,
    pub pos: Vec3,
    pub info: Vec<String>,
}

/// The boss being fought (for the big bar).
#[derive(Clone, Debug)]
pub struct BossView {
    pub name: String,
    pub title: String,
    pub life: f32,
    /// Phases passed and the life shares where they start.
    pub phase: u8,
    pub marks: Vec<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct GameFrame {
    pub place: Place,
    /// The level in progress: card, map, marks, mood.
    pub level: Option<mechanics::LevelView>,
    pub boss: Option<BossView>,
    pub inv: Option<Arc<InvView>>,
    pub loot: Vec<LootView>,
    pub gold: Vec<Vec3>,
    pub spots: Vec<SpotView>,
    /// Index into `spots` of the one the hero can use right now.
    pub near: Option<usize>,
    pub hero: Option<HeroHud>,
    pub actors: Vec<ActorView>,
    pub shots: Vec<Shot>,
    pub effects: Vec<Effect>,
    pub floaters: Vec<Floater>,
    pub telegraphs: Vec<Telegraph>,
    pub shake: f32,
    pub message: Option<String>,
    pub wave: u32,
    pub monsters: usize,
}

impl Game {
    /// Puppet tint for an entity: hit flash, frozen, burning, shocked.
    pub fn tint(&self, id: EntityId) -> Option<([f32; 3], f32)> {
        let a = self.actors.get(&id)?;
        if a.flash > 0.0 {
            return Some(([1.0, 1.0, 1.0], (a.flash / 0.12).min(1.0) * 0.8));
        }
        if a.dead {
            return None;
        }
        let ail = &a.ailments;
        if ail.freeze > 0.0 {
            return Some(([0.55, 0.85, 1.0], 0.55));
        }
        if ail.ignite.time > 0.0 {
            let f = 0.25 + 0.15 * (self.time * 18.0 + id.0 as f32).sin();
            return Some(([1.0, 0.45, 0.1], f));
        }
        if ail.shock.1 > 0.0 {
            return Some(([1.0, 0.95, 0.4], 0.25 + 0.2 * (self.time * 30.0).sin().abs()));
        }
        if ail.chill.1 > 0.0 {
            return Some(([0.6, 0.85, 1.0], 0.25));
        }
        if !ail.poison.is_empty() {
            return Some(([0.5, 0.95, 0.3], 0.25));
        }
        None
    }

    pub fn frame(&self, sim: &Sim) -> GameFrame {
        let d = data();
        let hero = self.hero_id.and_then(|h| self.actors.get(&h)).map(|a| HeroHud {
            name: self.hero.name.clone(),
            level: self.hero.level,
            xp: self.hero.xp,
            xp_next: hero::xp_to_next(self.hero.level),
            life: a.life,
            life_max: a.sheet.life_max,
            mana: a.mana,
            mana_max: a.sheet.mana_max,
            gold: self.hero.gold,
            potions: self.hero.potions,
            potion_max: self.hero.potion_max,
            slots: self
                .hero
                .bar
                .iter()
                .map(|k| match d.skill_id(k) {
                    Some(id) => {
                        let s = &skills::skill_of(a, id);
                        let cost = s.cost * a.sheet.mana_cost;
                        SlotHud {
                            key: k.clone(),
                            name: s.name.clone(),
                            cooldown: a.cooldown(id),
                            cooldown_max: s.cooldown / a.sheet.cooldown.max(0.1),
                            cost,
                            affordable: a.mana >= cost,
                            element: s.element as u8,
                            spell: s.has("spell"),
                        }
                    }
                    None => SlotHud::default(),
                })
                .collect(),
            dead: a.dead,
            respawn: self.respawn,
            dodge_cd: a.dodge_cd,
            level_flash: self.level_flash,
            kills: self.hero.kills,
        });
        let mut actors = Vec::new();
        let mut telegraphs = Vec::new();
        for (id, a) in &self.actors {
            let Some((feet, height)) = feet_of(sim, *id) else { continue };
            actors.push(ActorView {
                id: *id,
                feet,
                height,
                radius: a.radius,
                team: a.team,
                name: a.name.clone(),
                level: a.level,
                rarity: a.rarity,
                life: (a.life / a.sheet.life_max.max(1.0)).clamp(0.0, 1.0),
                dead: a.dead,
                aggro: a.brain.as_ref().is_some_and(|b| b.aggro),
                ailments: [
                    a.ailments.bleed.time > 0.0,
                    a.ailments.ignite.time > 0.0,
                    a.ailments.chill.1 > 0.0,
                    a.ailments.freeze > 0.0,
                    a.ailments.shock.1 > 0.0,
                    !a.ailments.poison.is_empty(),
                ],
                affixes: a.affixes.clone(),
            });
            if let Some(t) = a.cast.as_ref().and_then(|c| skills::telegraph(a, c, feet)) {
                telegraphs.push(t);
            }
        }
        let loot = self
            .loot
            .iter()
            .map(|g| {
                let base = g.item.base_def(&d);
                LootView {
                    id: g.item.id,
                    pos: g.pos,
                    name: g.item.name.clone(),
                    rarity: g.item.rarity,
                    slot: base.map(|b| b.slot).unwrap_or_default(),
                    color: base.and_then(|b| crate::color::Color::try_hex(&b.color)).map(|c| c.0).unwrap_or([0.8; 3]),
                    rest: g.rest,
                    age: g.age,
                }
            })
            .collect();
        let boss = self
            .actors
            .values()
            .filter(|a| !a.dead && a.boss.is_some() && a.brain.as_ref().is_none_or(|b| b.aggro))
            .max_by(|a, b| a.sheet.life_max.total_cmp(&b.sheet.life_max))
            .and_then(|a| {
                let st = a.boss.as_ref()?;
                let def = boss::boss_def(&d, &st.key, a.level)?;
                Some(BossView {
                    name: a.name.clone(),
                    title: def.title.clone(),
                    life: (a.life / a.sheet.life_max.max(1.0)).clamp(0.0, 1.0),
                    phase: st.phase,
                    marks: def.phases.iter().map(|p| p.at).collect(),
                })
            });
        if let Some(l) = &self.level {
            telegraphs.extend(l.telegraphs());
        }
        GameFrame {
            place: self.place,
            level: self.level.as_ref().map(|l| l.view(self)),
            boss,
            inv: self.inv_cache.clone(),
            loot,
            gold: self.gold.iter().map(|g| g.pos).collect(),
            spots: self
                .spots
                .iter()
                .map(|s| SpotView { kind: s.kind, name: s.name.clone(), pos: s.pos, info: s.info.clone() })
                .collect(),
            near: self.near_spot(sim),
            hero,
            actors,
            shots: self.shots.clone(),
            effects: self.effects.clone(),
            floaters: self.floaters.clone(),
            telegraphs,
            shake: self.shake,
            message: self.message.as_ref().map(|m| m.0.clone()),
            wave: self.arena.as_ref().map(|a| a.wave).unwrap_or(0),
            monsters: self.monsters_alive(),
        }
    }
}

/// Unused-move helper kept for brains that steer by a 2D direction.
pub fn dir2(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z).normalize_or_zero()
}
