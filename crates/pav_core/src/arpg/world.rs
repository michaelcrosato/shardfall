//! The descent: themes (game/themes.toml), the designed levels (game/levels.toml) and the
//! endless Depths after them. `plan` says what a depth *is* (name, colours, mechanics,
//! monsters, boss); `build_level` lays it out (levelgen), then raises the floor, walls, lights,
//! props, the mechanics' pieces and the monster packs. Every part is data or a seed, so the
//! same building blocks keep making new levels forever.

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::Game;
use super::cmd::{Place, Spot, SpotKind};
use super::combat::{Rarity, Team};
use super::data::{Data, Element, data};
use super::hero::Hero;
use super::levelgen::{Layout, Rect, RoomRole, RoomShape};
use super::mechanics::{Boon, ChestState, Feature, FeatureKind, LevelState};
use crate::color::{Color, to_hex};
use crate::entity::{BodyKind, EntityId, Spawn};
use crate::fxdef::{DistortDef, EmitterDef, LightDef};
use crate::rng::{Rng, hash3};
use crate::shape::{Look, Shape, Visual};
use crate::sim::Sim;
use crate::statics::{Block, Facing, RegionKey, block_flags};
use crate::zones::{Zone, ZoneKind};

/// A level's look and its inhabitants.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeDef {
    pub key: String,
    pub name: String,
    pub floor: [String; 2],
    pub wall: String,
    pub pillar: String,
    pub accent: String,
    pub light: String,
    /// Metres of wall between wall lights.
    pub light_every: f32,
    pub sky: String,
    pub sun: f32,
    pub sun_angle: f32,
    pub ambient: f32,
    pub particles: String,
    /// Hazy air (volumetric light: torch halos, sunbeams) and how strongly lamps glow in it.
    pub haze: f32,
    pub halos: f32,
    /// Grass, weeds or seaweed in patches on the floor ("" = none): it sways and parts around
    /// whoever walks through (drawing only).
    pub grass: String,
    /// Element weights for grown creatures: physical, fire, cold, lightning, poison.
    pub elements: [f32; 5],
    pub families: Vec<String>,
    pub props: Vec<String>,
}

/// The twelve level mechanics. Each designed level introduces one and is named after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mechanic {
    Shrines,
    PowderKeg,
    Gauntlet,
    RiftGates,
    Windways,
    Totems,
    MoltenFloor,
    FrozenLake,
    CrumblingHalls,
    LightlessDeep,
    CursedVaults,
    TimeRift,
}

impl Mechanic {
    pub const ALL: [Mechanic; 12] = [
        Mechanic::Shrines,
        Mechanic::PowderKeg,
        Mechanic::Gauntlet,
        Mechanic::RiftGates,
        Mechanic::Windways,
        Mechanic::Totems,
        Mechanic::MoltenFloor,
        Mechanic::FrozenLake,
        Mechanic::CrumblingHalls,
        Mechanic::LightlessDeep,
        Mechanic::CursedVaults,
        Mechanic::TimeRift,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Mechanic::Shrines => "shrines",
            Mechanic::PowderKeg => "powder_keg",
            Mechanic::Gauntlet => "gauntlet",
            Mechanic::RiftGates => "rift_gates",
            Mechanic::Windways => "windways",
            Mechanic::Totems => "totems",
            Mechanic::MoltenFloor => "molten_floor",
            Mechanic::FrozenLake => "frozen_lake",
            Mechanic::CrumblingHalls => "crumbling_halls",
            Mechanic::LightlessDeep => "lightless_deep",
            Mechanic::CursedVaults => "cursed_vaults",
            Mechanic::TimeRift => "time_rift",
        }
    }
    pub fn from_key(k: &str) -> Option<Mechanic> {
        Mechanic::ALL.into_iter().find(|m| m.key() == k)
    }
    /// What the player sees in the level card.
    pub fn name(self) -> &'static str {
        match self {
            Mechanic::Shrines => "Shrines",
            Mechanic::PowderKeg => "Powder kegs",
            Mechanic::Gauntlet => "Spike plates",
            Mechanic::RiftGates => "Rift gates",
            Mechanic::Windways => "Windways",
            Mechanic::Totems => "Ward totems",
            Mechanic::MoltenFloor => "Lava",
            Mechanic::FrozenLake => "Ice",
            Mechanic::CrumblingHalls => "Crumbling floors",
            Mechanic::LightlessDeep => "Darkness",
            Mechanic::CursedVaults => "Cursed chests",
            Mechanic::TimeRift => "Time bubbles",
        }
    }
    /// For the names of generated depths ("The Howling Molten Caverns").
    pub fn adjective(self) -> &'static str {
        match self {
            Mechanic::Shrines => "Hallowed",
            Mechanic::PowderKeg => "Smouldering",
            Mechanic::Gauntlet => "Spiked",
            Mechanic::RiftGates => "Folded",
            Mechanic::Windways => "Howling",
            Mechanic::Totems => "Warded",
            Mechanic::MoltenFloor => "Molten",
            Mechanic::FrozenLake => "Frozen",
            Mechanic::CrumblingHalls => "Crumbling",
            Mechanic::LightlessDeep => "Lightless",
            Mechanic::CursedVaults => "Cursed",
            Mechanic::TimeRift => "Timeless",
        }
    }
    /// One line on how to use it.
    pub fn hint(self) -> &'static str {
        match self {
            Mechanic::Shrines => "walk through a shrine for a boon",
            Mechanic::PowderKeg => "hit a keg near a pack",
            Mechanic::Gauntlet => "plates spike on a beat",
            Mechanic::RiftGates => "gates fold the halls together",
            Mechanic::Windways => "the wind carries everyone",
            Mechanic::Totems => "totems shield nearby monsters",
            Mechanic::MoltenFloor => "lava burns whoever stands in it",
            Mechanic::FrozenLake => "ice: everyone slides; frozen foes shatter",
            Mechanic::CrumblingHalls => "the floor falls away behind you",
            Mechanic::LightlessDeep => "light the wells",
            Mechanic::CursedVaults => "open a chest, survive its keepers",
            Mechanic::TimeRift => "monsters crawl inside the bubbles",
        }
    }
}

/// A designed level (game/levels.toml).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelDef {
    pub name: String,
    pub mechanic: Mechanic,
    pub theme: String,
    pub rooms: usize,
    pub about: String,
    #[serde(default)]
    pub also: Vec<Mechanic>,
    #[serde(default)]
    pub boss: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LevelsFile {
    pub level: Vec<LevelDef>,
}

/// Light and air of a level, for the view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mood {
    pub sky: String,
    pub sun: f32,
    pub sun_angle: f32,
    pub ambient: f32,
    /// Fog end (m); 0 = no fog.
    pub fog: f32,
    /// Hazy air and lamp halos (0 = clear air).
    #[serde(default)]
    pub haze: f32,
    #[serde(default)]
    pub halos: f32,
}

/// What a depth is: everything decided before a single block is placed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelPlan {
    pub depth: u32,
    pub name: String,
    pub about: String,
    pub theme: ThemeDef,
    /// The signature mechanic first, then the ones mixed in.
    pub mechanics: Vec<Mechanic>,
    pub rooms: usize,
    pub boss: Option<String>,
    pub monster_level: u32,
    /// Favoured behaviour archetypes for grown creatures (endless depths).
    pub archetypes: Vec<String>,
    /// Share of packs that are grown from genomes rather than designed families.
    pub genome_share: f32,
    pub endless: bool,
}

impl LevelPlan {
    pub fn has(&self, m: Mechanic) -> bool {
        self.mechanics.contains(&m)
    }
    /// 1 for the signature mechanic, 0.5 for a mixed-in one, 0 if absent.
    pub fn weight(&self, m: Mechanic) -> f32 {
        match self.mechanics.iter().position(|x| *x == m) {
            Some(0) => 1.0,
            Some(_) => 0.5,
            None => 0.0,
        }
    }
    pub fn mood(&self) -> Mood {
        let t = &self.theme;
        let dark = self.weight(Mechanic::LightlessDeep);
        Mood {
            sky: t.sky.clone(),
            sun: t.sun * (1.0 - dark * 0.9),
            sun_angle: t.sun_angle,
            ambient: t.ambient * (1.0 - dark * 0.6),
            fog: if dark >= 1.0 { 26.0 } else { 70.0 },
            haze: t.haze,
            // In the dark, the torches' glow is most of what you see.
            halos: t.halos * (1.0 + dark * 0.8),
        }
    }
    /// "Level 3" or "Depth 17".
    pub fn label(&self) -> String {
        if self.endless { format!("Depth {}", self.depth) } else { format!("Level {}", self.depth) }
    }
}

/// How many levels are designed (the rest are generated).
pub fn designed(d: &Data) -> u32 {
    d.levels.len() as u32
}

/// Monster level at a depth: 1, 3, 5 ... (23 at the last designed level), then +2 per depth.
pub fn monster_level(d: &Data, depth: u32) -> u32 {
    let _ = d;
    1 + (depth.max(1) - 1) * 2
}

/// What depth `depth` is. Designed levels come from game/levels.toml; past them, a depth is a
/// fixed combination (the same depth is always the same kind of place) of a theme blended
/// with another's colours, a signature mechanic plus others, favoured monster archetypes and,
/// every third depth, a boss.
pub fn plan(d: &Data, depth: u32) -> LevelPlan {
    let depth = depth.max(1);
    let ml = monster_level(d, depth);
    if let Some(l) = d.levels.get(depth as usize - 1) {
        let mut mechanics = vec![l.mechanic];
        mechanics.extend(l.also.iter().copied().filter(|m| *m != l.mechanic));
        return LevelPlan {
            depth,
            name: l.name.clone(),
            about: l.about.clone(),
            theme: d.themes.get(&l.theme).cloned().unwrap_or_default(),
            mechanics,
            rooms: l.rooms,
            boss: l.boss.clone(),
            monster_level: ml,
            archetypes: Vec::new(),
            genome_share: (0.12 + 0.06 * (depth - 1) as f32).min(0.8),
            endless: false,
        };
    }
    let n = designed(d);
    let k = depth - n;
    let mut rng = Rng::new(0x5ad_e9d5 ^ (depth as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    let keys: Vec<&String> = d.themes.keys().collect();
    let ia = rng.below(keys.len() as u32) as usize;
    let ib = (ia + 1 + rng.below(keys.len().max(2) as u32 - 1) as usize) % keys.len();
    let (a, b) = (&d.themes[keys[ia]], &d.themes[keys[ib]]);
    let hue = [0.0, 25.0, -25.0, 50.0, -50.0, 90.0, 180.0, -90.0][rng.below(8) as usize];
    let theme = blend_theme(a, b, hue);
    // Mechanics: one signature, one to three mixed in as the depths go on.
    let mut pool: Vec<Mechanic> = Mechanic::ALL.to_vec();
    let mut mechanics = Vec::new();
    let count = 2 + (k >= 8) as usize + (k >= 20) as usize;
    for _ in 0..count {
        let i = rng.below(pool.len() as u32) as usize;
        mechanics.push(pool.remove(i));
    }
    let noun = theme.name.rsplit(' ').next().unwrap_or("Depths").to_string();
    let name = format!("The {} {}", mechanics.iter().take(2).map(|m| m.adjective()).collect::<Vec<_>>().join(" "), noun);
    let names: Vec<&str> = mechanics.iter().map(|m| m.name()).collect();
    let about = format!("{}: everything you have learned, all at once.", join_and(&names));
    let mut arch: Vec<&String> = d.genome.archetype.keys().collect();
    let mut archetypes = Vec::new();
    for _ in 0..2.min(arch.len()) {
        archetypes.push(arch.remove(rng.below(arch.len() as u32) as usize).clone());
    }
    // Bosses: the designed ones no level used yet, then grown ones, every third depth.
    let boss = if k.is_multiple_of(3) {
        let unused: Vec<&super::boss::BossDef> =
            d.bosses.iter().filter(|b| !d.levels.iter().any(|l| l.boss.as_deref() == Some(b.key.as_str()))).collect();
        match unused.get((k / 3 - 1) as usize) {
            Some(b) => Some(b.key.clone()),
            None => Some(format!("gen:{}", (depth as u64).wrapping_mul(0x51ed_270b) ^ 0xb055)),
        }
    } else {
        None
    };
    LevelPlan {
        depth,
        name,
        about,
        theme,
        mechanics,
        rooms: 9 + (k as usize / 4).min(6),
        boss,
        monster_level: ml,
        archetypes,
        genome_share: 0.75,
        endless: true,
    }
}

fn join_and(v: &[&str]) -> String {
    match v.len() {
        0 => String::new(),
        1 => v[0].to_string(),
        n => format!("{} and {}", v[..n - 1].join(", "), v[n - 1]),
    }
}

fn shift(c: &str, deg: f32) -> String {
    to_hex(Color::hex(c).hue_shift(deg))
}

/// A new palette: `a`'s architecture turned around the colour wheel, `b`'s lights.
pub fn blend_theme(a: &ThemeDef, b: &ThemeDef, hue: f32) -> ThemeDef {
    let mut t = a.clone();
    t.key = format!("{}+{}@{}", a.key, b.key, hue as i32);
    t.floor = [shift(&a.floor[0], hue), shift(&a.floor[1], hue)];
    t.wall = shift(&a.wall, hue);
    t.pillar = shift(&a.pillar, hue);
    t.sky = shift(&a.sky, hue);
    t.accent = shift(&b.accent, hue * 0.5);
    t.light = shift(&b.light, hue * 0.5);
    t.particles = b.particles.clone();
    t.haze = (a.haze + b.haze) * 0.5;
    t.halos = (a.halos + b.halos) * 0.5;
    let grass = if a.grass.is_empty() { &b.grass } else { &a.grass };
    t.grass = if grass.is_empty() { String::new() } else { shift(grass, hue * 0.5) };
    for (i, e) in t.elements.iter_mut().enumerate() {
        *e = (*e + b.elements[i]) * 0.5;
    }
    for p in &b.props {
        if !t.props.contains(p) && t.props.len() < 4 {
            t.props.push(p.clone());
        }
    }
    for f in &b.families {
        if !t.families.contains(f) {
            t.families.push(f.clone());
        }
    }
    t
}

/// Who lives in a level: enough to raise more packs later (a cursed chest's keepers).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PackInfo {
    pub families: Vec<String>,
    pub elements: [f32; 5],
    pub archetypes: Vec<String>,
    pub genome_share: f32,
    pub depth: u32,
}

impl PackInfo {
    pub fn of(plan: &LevelPlan) -> Self {
        Self {
            families: plan.theme.families.clone(),
            elements: plan.theme.elements,
            archetypes: plan.archetypes.clone(),
            genome_share: plan.genome_share,
            depth: plan.depth,
        }
    }
}

/// A pack: a designed family of the theme, or creatures grown from a genome (element by the
/// theme's weights, archetype often one the depth favours). Rares bring normal followers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_pack(
    g: &mut Game,
    sim: &mut Sim,
    rng: &mut Rng,
    info: &PackInfo,
    feet: Vec3,
    level: u32,
    rarity: Rarity,
    aggro: bool,
) -> Vec<EntityId> {
    let d = data();
    let generated = rng.f32() < info.genome_share || info.families.is_empty();
    let (spec, k, big) = if generated {
        let seed = ((rng.next_u32() as u64) << 16) | info.depth as u64;
        let total: f32 = info.elements.iter().sum::<f32>().max(1e-3);
        let mut pick = rng.f32() * total;
        let mut element = Element::Physical;
        for (i, w) in info.elements.iter().enumerate() {
            if pick < *w {
                element = Element::ALL[i];
                break;
            }
            pick -= w;
        }
        let archetype = if !info.archetypes.is_empty() && rng.f32() < 0.6 {
            Some(info.archetypes[rng.below(info.archetypes.len() as u32) as usize].clone())
        } else {
            None
        };
        let opts = super::genome::GenomeOpts { archetype, element: Some(element), ..Default::default() };
        match super::genome::Genome::generate(&d, seed, level, &opts) {
            Ok(gn) => {
                let big = gn.puppet.scale > 1.3 || gn.archetype == "tank";
                (gn.spec(&d), gn.pack, big)
            }
            Err(_) => return Vec::new(),
        }
    } else {
        let fam = info.families[rng.below(info.families.len() as u32) as usize].clone();
        let Some(f) = d.family(&fam) else { return Vec::new() };
        let k = if fam == "skitterer" { 2.0 } else { 1.0 };
        (f.spec(), k, fam == "bonecrusher")
    };
    let count = if big { 1 } else { ((3.0 + rng.below(3) as f32) * k).round().max(1.0) as u32 };
    let pack = g.next_pack;
    g.next_pack += 1;
    let mut out = Vec::new();
    for i in 0..count {
        let r = if rarity == Rarity::Rare && i > 0 { Rarity::Normal } else { rarity };
        let a = i as f32 * 2.4;
        let at = feet + Vec3::new(a.cos(), 0.0, a.sin()) * (0.6 + i as f32 * 0.4);
        if let Some(id) = super::spawn_spec_into(sim, g, &spec, level, r, at, pack) {
            if let Some(b) = g.actors.get_mut(&id).and_then(|a| a.brain.as_mut()) {
                b.aggro = aggro;
            }
            out.push(id);
        }
    }
    out
}

// ------------------------------------------------------------------------------- building

const WALL_H: f32 = 1.9;

/// Builds depth `depth` around a game in progress (travel) or a fresh one.
pub fn build_level(sim: &mut Sim, depth: u32, game: Option<Game>) {
    let d = data();
    super::scene::game_movement(sim);
    let plan = plan(&d, depth);
    let seed = (sim.state.rng.next_u32() as u64) | ((depth as u64) << 32);
    let layout = Layout::generate(seed, plan.rooms);
    let mut b = Builder::new(sim, &plan, layout, seed);
    b.choose_floors();
    b.geometry();
    b.decorate();
    let start = b.layout.rooms[b.layout.start].rect.center();
    let spawn = Vec3::new(start.x, 0.1, start.y + 2.0);
    let portal_at = Vec3::new(start.x, 0.0, start.y - 3.0);
    // A softer portal down here: the levels are darker than the town.
    let pool = super::scene::portal(b.sim, portal_at);
    if let Some(v) = b.sim.state.entities.get_mut(pool).and_then(|e| e.visual.as_mut()) {
        v.emissive = 0.3;
        v.color = Color::hex("#4aa8d8");
        if let Some(l) = v.light.as_mut() {
            l.intensity = 1.2;
        }
    }
    b.taken[b.layout.start].push((Vec2::new(portal_at.x, portal_at.z), 3.0));
    b.taken[b.layout.start].push((Vec2::new(spawn.x, spawn.z), 2.5));
    let exit = b.layout.rooms[b.layout.exit].rect.center();
    let exit_at = Vec3::new(exit.x, 0.0, exit.y);
    b.exit_gate(exit_at);
    b.taken[b.layout.exit].push((exit, 4.0));
    // The hero arrives.
    let sim = &mut *b.sim;
    sim.state.spawn = spawn;
    sim.spawn_player();
    match game {
        Some(g) => sim.resume_game(g),
        None => sim.start_game(Hero::default()),
    }
    let mut g = sim.state.game.take().unwrap();
    g.place = Place::Level(depth);
    g.hero.max_depth = g.hero.max_depth.max(depth);
    g.inv_changed();
    g.spots.push(Spot {
        kind: SpotKind::Portal,
        name: "Portal to Emberwatch".into(),
        pos: portal_at,
        reach: 2.8,
        info: Vec::new(),
    });
    let exit_spot = g.spots.len();
    g.spots.push(Spot {
        kind: SpotKind::Exit,
        name: format!(
            "The way down ({})",
            if plan.endless || depth >= designed(&d) { format!("Depth {}", depth + 1) } else { format!("Level {}", depth + 1) }
        ),
        pos: exit_at,
        reach: 3.0,
        info: Vec::new(),
    });
    let mut st = LevelState::new(&plan, b.layout.clone(), seed, exit_spot);
    st.safe = spawn;
    b.game = Some(*g);
    b.mechanics();
    b.monsters(&mut st);
    st.features = std::mem::take(&mut b.features);
    st.seen[st.layout.start] = true;
    st.exit_open = st.boss.is_none();
    if !st.exit_open {
        st.seal = Some(b.exit_seal(exit_at));
    }
    let mut g = b.game.take().unwrap();
    g.say(format!("{} · {}", plan.label(), plan.name), 4.0);
    g.level = Some(Box::new(st));
    b.sim.state.game = Some(Box::new(g));
}

struct Builder<'a> {
    sim: &'a mut Sim,
    plan: &'a LevelPlan,
    layout: Layout,
    rng: Rng,
    /// Per room: circles already used (obstacles, pieces, packs).
    taken: Vec<Vec<(Vec2, f32)>>,
    ice: Vec<bool>,
    crumble: Vec<bool>,
    features: Vec<Feature>,
    game: Option<Game>,
}

impl<'a> Builder<'a> {
    fn new(sim: &'a mut Sim, plan: &'a LevelPlan, layout: Layout, seed: u64) -> Self {
        let n = layout.rooms.len();
        Self {
            sim,
            plan,
            layout,
            rng: Rng::new(seed ^ 0xb111_d3e5),
            taken: vec![Vec::new(); n],
            ice: vec![false; n],
            crumble: vec![false; n],
            features: Vec::new(),
            game: None,
        }
    }

    fn color(&self, hex: &str) -> Color {
        Color::hex(hex)
    }

    fn block(&mut self, min: Vec3, max: Vec3, c: Color) {
        let st = &mut self.sim.state;
        st.statics.add(&mut st.physics, Block::new(min, max, c));
    }

    /// Rooms between the start and the exit (not those two).
    fn middle_rooms(&self) -> Vec<usize> {
        (0..self.layout.rooms.len()).filter(|i| *i != self.layout.start && *i != self.layout.exit).collect()
    }

    fn chance(&mut self, p: f32) -> bool {
        self.rng.f32() < p
    }

    /// Which rooms are ice and which crumble (the floor is built from this).
    fn choose_floors(&mut self) {
        let wi = self.plan.weight(Mechanic::FrozenLake);
        let wc = self.plan.weight(Mechanic::CrumblingHalls);
        for i in self.middle_rooms() {
            if wi > 0.0 && self.chance(0.65 * wi) {
                self.ice[i] = true;
            } else if wc > 0.0 && self.chance(0.6 * wc) {
                self.crumble[i] = true;
            }
        }
        // At least one of each mechanic the level has.
        let mid = self.middle_rooms();
        if wi > 0.0 && !self.ice.iter().any(|x| *x) && !mid.is_empty() {
            self.ice[mid[0]] = true;
        }
        if wc > 0.0 && !self.crumble.iter().any(|x| *x) && mid.len() > 1 {
            let r = mid[mid.len() / 2];
            self.ice[r] = false;
            self.crumble[r] = true;
        }
    }

    /// Floor, walls and room furniture.
    fn geometry(&mut self) {
        let t = self.plan.theme.clone();
        let floor = [self.color(&t.floor[0]), self.color(&t.floor[1])];
        let ice = [Color::hex("#bcdcef"), Color::hex("#a9cde4")];
        let wall = self.color(&t.wall);
        for i in 0..self.layout.rooms.len() {
            let r = self.layout.rooms[i].rect;
            let cols = if self.ice[i] { ice } else { floor };
            self.floor(r, if self.crumble[i] { 2.0 } else { 4.0 }, cols, self.crumble[i], i as i32);
            self.room_walls(i, wall);
            self.furniture(i);
        }
        for c in 0..self.layout.corridors.len() {
            let cor = self.layout.corridors[c].clone();
            let (a, b) = cor.rooms;
            let cols = if self.ice[a] && self.ice[b] { ice } else { floor };
            self.floor(cor.rect, 4.0, cols, false, 1000 + c as i32);
            let r = cor.rect;
            if cor.along_x {
                if r.max.x - r.min.x > 2.0 {
                    self.block(Vec3::new(r.min.x + 1.0, 0.0, r.min.y - 1.0), Vec3::new(r.max.x - 1.0, WALL_H, r.min.y), wall);
                    self.block(Vec3::new(r.min.x + 1.0, 0.0, r.max.y), Vec3::new(r.max.x - 1.0, WALL_H, r.max.y + 1.0), wall);
                }
            } else if r.max.y - r.min.y > 2.0 {
                self.block(Vec3::new(r.min.x - 1.0, 0.0, r.min.y + 1.0), Vec3::new(r.min.x, WALL_H, r.max.y - 1.0), wall);
                self.block(Vec3::new(r.max.x, 0.0, r.min.y + 1.0), Vec3::new(r.max.x + 1.0, WALL_H, r.max.y - 1.0), wall);
            }
        }
    }

    fn floor(&mut self, r: Rect, tile: f32, cols: [Color; 2], crumble: bool, salt: i32) {
        let nx = ((r.max.x - r.min.x) / tile).ceil() as i32;
        let nz = ((r.max.y - r.min.y) / tile).ceil() as i32;
        for i in 0..nx {
            for j in 0..nz {
                let x0 = r.min.x + i as f32 * tile;
                let z0 = r.min.y + j as f32 * tile;
                let x1 = (x0 + tile).min(r.max.x);
                let z1 = (z0 + tile).min(r.max.y);
                let v = (hash3(self.plan.depth as u64, i, salt, j) % 1000) as f32 / 1000.0;
                let c = cols[((i + j) % 2) as usize].scale(0.94 + 0.12 * v);
                let mut b = Block::new(Vec3::new(x0, -0.5, z0), Vec3::new(x1, 0.0, z1), c);
                if crumble {
                    b.crumble = 0.55;
                    b.regrow = 16.0;
                    b.flags |= block_flags::PLAYER_CRUMBLE;
                }
                let st = &mut self.sim.state;
                st.statics.add(&mut st.physics, b);
            }
        }
    }

    /// Walls around a room with gaps for its doors; lights along them.
    fn room_walls(&mut self, i: usize, wall: Color) {
        let r = self.layout.rooms[i].rect;
        let doors = self.layout.doors(i);
        let light_every = self.plan.theme.light_every.max(6.0);
        let lights = self.plan.weight(Mechanic::LightlessDeep) < 1.0;
        for side in 0..4 {
            let (lo, hi) = if side % 2 == 0 { (r.min.y - 1.0, r.max.y + 1.0) } else { (r.min.x - 1.0, r.max.x + 1.0) };
            let mut gaps: Vec<(f32, f32)> = doors.iter().filter(|d| d.0 == side).map(|d| (d.1, d.2)).collect();
            gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut segs = Vec::new();
            let mut at = lo;
            for (g0, g1) in &gaps {
                if *g0 > at {
                    segs.push((at, *g0));
                }
                at = at.max(*g1);
            }
            if hi > at {
                segs.push((at, hi));
            }
            for (s0, s1) in segs {
                let (min, max) = match side {
                    0 => (Vec3::new(r.max.x, 0.0, s0), Vec3::new(r.max.x + 1.0, WALL_H, s1)),
                    1 => (Vec3::new(s0, 0.0, r.max.y), Vec3::new(s1, WALL_H, r.max.y + 1.0)),
                    2 => (Vec3::new(r.min.x - 1.0, 0.0, s0), Vec3::new(r.min.x, WALL_H, s1)),
                    _ => (Vec3::new(s0, 0.0, r.min.y - 1.0), Vec3::new(s1, WALL_H, r.min.y)),
                };
                self.block(min, max, wall);
                // A few darker stones in the wall's top course.
                if s1 - s0 > 3.0 {
                    let cap = wall.scale(0.82);
                    let (cmin, cmax) = (Vec3::new(min.x, WALL_H, min.z), Vec3::new(max.x, WALL_H + 0.18, max.z));
                    self.block(cmin, cmax, cap);
                }
                if !lights {
                    continue;
                }
                // Sconces along the inner face, away from the corners and doors.
                let len = s1 - s0;
                let n = (len / light_every).floor() as i32;
                for k in 0..n {
                    let u = s0 + len * (k as f32 + 0.5) / n as f32;
                    if u < lo + 2.5 || u > hi - 2.5 {
                        continue;
                    }
                    let (pos, out) = match side {
                        0 => (Vec3::new(r.max.x - 0.15, 1.9, u), Vec3::NEG_X),
                        1 => (Vec3::new(u, 1.9, r.max.y - 0.15), Vec3::NEG_Z),
                        2 => (Vec3::new(r.min.x + 0.15, 1.9, u), Vec3::X),
                        _ => (Vec3::new(u, 1.9, r.min.y + 0.15), Vec3::Z),
                    };
                    self.sconce(pos, out);
                }
            }
        }
    }

    fn sconce(&mut self, at: Vec3, out: Vec3) {
        let t = &self.plan.theme;
        let iron = Color::hex("#2a2624");
        // Decoration only: a bracket at head height must never catch anyone.
        let b0 = at - Vec3::splat(0.12);
        let st = &mut self.sim.state;
        st.statics
            .add(&mut st.physics, Block::new(b0 - Vec3::Y * 0.1, b0 + Vec3::splat(0.24), iron).with_flags(block_flags::GHOST));
        let mut v = Visual::new(Shape::Sphere { radius: 0.11 }, Color::hex(&t.light));
        v.look = Look::Unlit;
        v.emissive = 2.4;
        v.light = Some(Box::new(LightDef {
            color: t.light.clone(),
            radius: 9.0,
            intensity: 1.7,
            flicker: 0.45,
            offset: out * 0.4,
            ..Default::default()
        }));
        v.particles = Some(Box::new(EmitterDef {
            preset: "fire".into(),
            color: t.light.clone(),
            size: 0.35,
            rate: 14.0,
            ..Default::default()
        }));
        self.sim.spawn(Spawn::new("sconce", at + out * 0.25 + Vec3::Y * 0.3).visual(v));
    }

    /// Pillars, a ring, a split wall or corner blocks, by the room's shape.
    fn furniture(&mut self, i: usize) {
        let room = self.layout.rooms[i].clone();
        let r = room.rect;
        let c = r.center();
        let s = r.size();
        let pillar = self.color(&self.plan.theme.pillar);
        let wall = self.color(&self.plan.theme.wall);
        // Doors: keep the way in clear.
        for (side, a, b) in self.layout.doors(i) {
            let m = (a + b) * 0.5;
            let p = match side {
                0 => Vec2::new(r.max.x - 2.5, m),
                1 => Vec2::new(m, r.max.y - 2.5),
                2 => Vec2::new(r.min.x + 2.5, m),
                _ => Vec2::new(m, r.min.y + 2.5),
            };
            self.taken[i].push((p, 3.0));
        }
        match room.shape {
            RoomShape::Plain => {}
            RoomShape::Pillars => {
                let along_x = s.x >= s.y;
                let (len, across) = if along_x { (s.x, s.y) } else { (s.y, s.x) };
                let n = ((len - 8.0) / 5.0).floor().max(1.0) as i32;
                for row in [-1.0f32, 1.0] {
                    for k in 0..n {
                        let u = -len * 0.5 + 4.0 + (len - 8.0) * (k as f32 + 0.5) / n as f32;
                        let v = row * across * 0.22;
                        let p = if along_x { c + Vec2::new(u, v) } else { c + Vec2::new(v, u) };
                        let st = &mut self.sim.state;
                        st.statics.add(
                            &mut st.physics,
                            Block::new(
                                Vec3::new(p.x - 0.6, 0.0, p.y - 0.6),
                                Vec3::new(p.x + 0.6, WALL_H + 0.6, p.y + 0.6),
                                pillar,
                            )
                            .with_flags(block_flags::ROUNDED),
                        );
                        self.taken[i].push((p, 1.2));
                    }
                }
            }
            RoomShape::Ring => {
                let h = s * 0.16;
                self.block(Vec3::new(c.x - h.x, 0.0, c.y - h.y), Vec3::new(c.x + h.x, 1.1, c.y + h.y), wall);
                self.block(
                    Vec3::new(c.x - h.x - 0.15, 1.1, c.y - h.y - 0.15),
                    Vec3::new(c.x + h.x + 0.15, 1.3, c.y + h.y + 0.15),
                    pillar,
                );
                self.taken[i].push((c, h.length() + 1.0));
            }
            RoomShape::Split => {
                // A low wall across the middle, open at both ends and in the centre.
                if s.x >= s.y {
                    for (z0, z1) in [(r.min.y + 3.0, c.y - 2.0), (c.y + 2.0, r.max.y - 3.0)] {
                        if z1 - z0 > 1.0 {
                            self.block(Vec3::new(c.x - 0.5, 0.0, z0), Vec3::new(c.x + 0.5, 1.4, z1), wall);
                            self.taken[i].push((Vec2::new(c.x, (z0 + z1) * 0.5), (z1 - z0) * 0.5 + 0.5));
                        }
                    }
                } else {
                    for (x0, x1) in [(r.min.x + 3.0, c.x - 2.0), (c.x + 2.0, r.max.x - 3.0)] {
                        if x1 - x0 > 1.0 {
                            self.block(Vec3::new(x0, 0.0, c.y - 0.5), Vec3::new(x1, 1.4, c.y + 0.5), wall);
                            self.taken[i].push((Vec2::new((x0 + x1) * 0.5, c.y), (x1 - x0) * 0.5 + 0.5));
                        }
                    }
                }
            }
            RoomShape::Cross => {
                for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    let p = c + Vec2::new(sx * (s.x * 0.5 - 4.0), sz * (s.y * 0.5 - 4.0));
                    self.block(Vec3::new(p.x - 1.4, 0.0, p.y - 1.4), Vec3::new(p.x + 1.4, WALL_H, p.y + 1.4), wall);
                    self.block(Vec3::new(p.x - 1.5, WALL_H, p.y - 1.5), Vec3::new(p.x + 1.5, WALL_H + 0.2, p.y + 1.5), pillar);
                    self.taken[i].push((p, 2.2));
                }
            }
        }
    }

    /// A free point in a room, `clear` metres from anything taken, at least `margin` from the walls.
    fn spot(&mut self, room: usize, clear: f32, margin: f32) -> Option<Vec2> {
        let r = self.layout.rooms[room].rect.shrink(margin);
        if r.max.x <= r.min.x || r.max.y <= r.min.y {
            return None;
        }
        for _ in 0..60 {
            let p = Vec2::new(self.rng.range(r.min.x, r.max.x), self.rng.range(r.min.y, r.max.y));
            if self.taken[room].iter().all(|(q, qr)| (p - *q).length() > qr + clear) {
                self.taken[room].push((p, clear));
                return Some(p);
            }
        }
        None
    }

    /// A point close to a wall (props).
    fn wall_spot(&mut self, room: usize) -> Option<Vec2> {
        let r = self.layout.rooms[room].rect;
        for _ in 0..30 {
            let side = self.rng.below(4);
            let inset = self.rng.range(0.9, 1.8);
            let u = self.rng.f32();
            let p = match side {
                0 => Vec2::new(r.max.x - inset, r.min.y + 1.0 + u * (r.size().y - 2.0)),
                1 => Vec2::new(r.min.x + 1.0 + u * (r.size().x - 2.0), r.max.y - inset),
                2 => Vec2::new(r.min.x + inset, r.min.y + 1.0 + u * (r.size().y - 2.0)),
                _ => Vec2::new(r.min.x + 1.0 + u * (r.size().x - 2.0), r.min.y + inset),
            };
            if self.taken[room].iter().all(|(q, qr)| (p - *q).length() > qr + 1.0) {
                self.taken[room].push((p, 1.0));
                return Some(p);
            }
        }
        None
    }

    /// Props along the walls and drifting particles in every room.
    fn decorate(&mut self) {
        let t = self.plan.theme.clone();
        for i in 0..self.layout.rooms.len() {
            let r = self.layout.rooms[i].rect;
            let n = 3 + self.rng.below(4);
            for _ in 0..n {
                if t.props.is_empty() {
                    break;
                }
                let Some(p) = self.wall_spot(i) else { continue };
                let kind = t.props[self.rng.below(t.props.len() as u32) as usize].clone();
                self.prop(&kind, p);
            }
            let s = r.size();
            if !t.particles.is_empty() {
                let mut v = Visual::new(Shape::Sphere { radius: 0.01 }, Color::hex("#000000"));
                v.look = Look::Unlit;
                v.particles = Some(Box::new(EmitterDef {
                    preset: t.particles.clone(),
                    color: if t.particles == "magic" || t.particles == "fireflies" { t.accent.clone() } else { String::new() },
                    rate: (s.x * s.y * 0.03).clamp(6.0, 22.0),
                    area: Vec3::new(s.x * 0.45, 1.2, s.y * 0.45),
                    ground: Some(-2.0),
                    ..Default::default()
                }));
                let c = r.center();
                self.sim.spawn(Spawn::new("~air", Vec3::new(c.x, 2.0, c.y)).visual(v));
            }
            if !t.grass.is_empty() {
                self.grass(i, &t.grass);
            }
        }
    }

    /// Patches of grass in a room. Positions come from a hash of the level and room, not the
    /// level's random stream, so a theme gaining grass builds the same level as before.
    fn grass(&mut self, room: usize, color: &str) {
        let r = self.layout.rooms[room].rect;
        let (c, s) = (r.center(), r.size());
        let seed = 0x6a55_u64 ^ ((self.plan.depth as u64) << 20) ^ room as u64;
        let patches = ((s.x * s.y) / 14.0).clamp(2.0, 10.0) as i32;
        let base = Color::hex(color);
        let st = &mut self.sim.state;
        for k in 0..patches {
            let f = |a: i32| crate::rng::hash_f32(seed, k, a, 7);
            let centre = c + Vec2::new((f(0) - 0.5) * (s.x - 2.0).max(0.5), (f(1) - 0.5) * (s.y - 2.0).max(0.5));
            let tufts = 8 + (f(2) * 10.0) as i32;
            for j in 0..tufts {
                let g = |a: i32| crate::rng::hash_f32(seed, k * 64 + j, a, 11);
                let a = g(0) * std::f32::consts::TAU;
                let p = centre + Vec2::new(a.cos(), a.sin()) * g(1).sqrt() * 1.4;
                let hh = 0.16 + g(2) * 0.18;
                let pos = Vec3::new(p.x, hh - 0.03, p.y);
                let d = crate::statics::Decor {
                    shape: Shape::Box { half: Vec3::new(0.28, hh, 0.28) },
                    pos,
                    rot: Quat::from_rotation_y(g(3) * 6.3),
                    color: base.scale(0.85 + 0.3 * g(4)),
                    look: Look::Cel,
                    emissive: 0.0,
                    solid: false,
                    collider: None,
                    sway: crate::shape::Sway::Grass,
                };
                st.statics.add_decor(&mut st.physics, RegionKey::chunk_of(pos), d);
            }
        }
    }

    fn prop(&mut self, kind: &str, p: Vec2) {
        let t = self.plan.theme.clone();
        let at = Vec3::new(p.x, 0.0, p.y);
        let rot = |a: f32| Quat::from_rotation_y(a);
        match kind {
            "rocks" => {
                let base = Color::hex(&t.wall);
                for k in 0..3 {
                    let o = Vec3::new(self.rng.range(-0.7, 0.7), 0.0, self.rng.range(-0.7, 0.7));
                    let h = self.rng.range(0.3, 0.9) * if k == 0 { 1.3 } else { 1.0 };
                    let w = self.rng.range(0.35, 0.6);
                    let c = base.scale(self.rng.range(0.75, 1.1));
                    let st = &mut self.sim.state;
                    st.statics.add(
                        &mut st.physics,
                        Block::new(at + o - Vec3::new(w, 0.0, w), at + o + Vec3::new(w, h, w), c)
                            .with_flags(block_flags::ROUNDED),
                    );
                }
            }
            "crates" => {
                let wood = Color::hex("#6a4a2c");
                let s = self.rng.range(0.4, 0.55);
                self.block(at - Vec3::new(s, 0.0, s), at + Vec3::new(s, s * 2.0, s), wood);
                if self.chance(0.5) {
                    let o = Vec3::new(self.rng.range(-0.15, 0.15), s * 2.0, self.rng.range(-0.15, 0.15));
                    let s2 = s * 0.8;
                    self.block(at + o - Vec3::new(s2, 0.0, s2), at + o + Vec3::new(s2, s2 * 2.0, s2), wood.scale(1.1));
                }
            }
            "columns" => {
                let h = self.rng.range(1.0, 2.8);
                let mut v = Visual::new(Shape::Cylinder { half_height: h * 0.5, radius: 0.42 }, Color::hex(&t.pillar));
                v.look = Look::Lit;
                self.sim.spawn(Spawn::new("column", at + Vec3::Y * h * 0.5).visual(v).body(BodyKind::Fixed));
                if h < 2.0 {
                    // Its broken top lies beside it.
                    let mut v = Visual::new(Shape::Cylinder { half_height: 0.5, radius: 0.4 }, Color::hex(&t.pillar).scale(0.9));
                    v.look = Look::Lit;
                    let mut s = Spawn::new("column piece", at + Vec3::new(0.9, 0.4, 0.3)).visual(v);
                    s.rot = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) * rot(self.rng.range(0.0, 3.0));
                    self.sim.spawn(s);
                }
            }
            "crystals" => {
                let n = 2 + self.rng.below(3);
                for k in 0..n {
                    let h = self.rng.range(0.4, 1.2);
                    let mut v = Visual::new(Shape::Box { half: Vec3::new(0.12, h * 0.5, 0.12) }, Color::hex(&t.accent));
                    v.look = Look::Unlit;
                    v.emissive = 1.1;
                    if k == 0 && self.chance(0.4) {
                        v.light = Some(Box::new(LightDef {
                            color: t.accent.clone(),
                            radius: 4.5,
                            intensity: 1.0,
                            pulse: 0.3,
                            ..Default::default()
                        }));
                    }
                    let o = Vec3::new(self.rng.range(-0.5, 0.5), h * 0.45, self.rng.range(-0.5, 0.5));
                    let mut s = Spawn::new("crystal", at + o).visual(v);
                    s.rot = rot(self.rng.range(0.0, 3.0)) * Quat::from_rotation_z(self.rng.range(-0.45, 0.45));
                    self.sim.spawn(s);
                }
            }
            "bones" => {
                let bone = Color::hex("#d9d0bb");
                for _ in 0..3 {
                    let mut v = Visual::new(Shape::Capsule { half_height: 0.25, radius: 0.05 }, bone);
                    v.look = Look::Lit;
                    let o = Vec3::new(self.rng.range(-0.5, 0.5), 0.05, self.rng.range(-0.5, 0.5));
                    let mut s = Spawn::new("bone", at + o).visual(v);
                    s.rot = rot(self.rng.range(0.0, 3.1)) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
                    self.sim.spawn(s);
                }
                let mut v = Visual::new(Shape::Sphere { radius: 0.16 }, bone);
                v.look = Look::Lit;
                self.sim.spawn(Spawn::new("skull", at + Vec3::new(0.2, 0.15, -0.1)).visual(v));
            }
            "mushrooms" => {
                let n = 2 + self.rng.below(3);
                for _ in 0..n {
                    let h = self.rng.range(0.3, 1.1);
                    let o = Vec3::new(self.rng.range(-0.6, 0.6), 0.0, self.rng.range(-0.6, 0.6));
                    let mut stem =
                        Visual::new(Shape::Cylinder { half_height: h * 0.5, radius: 0.07 + h * 0.05 }, Color::hex("#d8cfb8"));
                    stem.look = Look::Lit;
                    self.sim.spawn(Spawn::new("stem", at + o + Vec3::Y * h * 0.5).visual(stem));
                    let mut cap = Visual::new(
                        Shape::Cylinder { half_height: 0.06 + h * 0.04, radius: 0.2 + h * 0.25 },
                        Color::hex(&t.accent),
                    );
                    cap.look = Look::Unlit;
                    cap.emissive = 0.7;
                    self.sim.spawn(Spawn::new("cap", at + o + Vec3::Y * h).visual(cap));
                }
            }
            _ => {}
        }
    }

    /// The gate to the next depth.
    fn exit_gate(&mut self, at: Vec3) {
        let st = &mut self.sim.state;
        let stone = Color::hex(&self.plan.theme.pillar).scale(0.8);
        for k in 0..2 {
            let x = if k == 0 { -2.2 } else { 2.2 };
            st.statics.add(
                &mut st.physics,
                Block::new(at + Vec3::new(x - 0.35, 0.0, -0.35), at + Vec3::new(x + 0.35, 3.6, 0.35), stone)
                    .with_flags(block_flags::ROUNDED),
            );
        }
        st.statics.add(&mut st.physics, Block::new(at + Vec3::new(-2.7, 3.6, -0.45), at + Vec3::new(2.7, 4.1, 0.45), stone));
        let mut v = Visual::new(Shape::Cylinder { half_height: 0.03, radius: 1.6 }, Color::hex("#ffd27a"));
        v.look = Look::Unlit;
        v.emissive = 1.0;
        v.light = Some(Box::new(LightDef {
            color: "#ffc86a".into(),
            radius: 10.0,
            intensity: 2.0,
            pulse: 0.5,
            offset: Vec3::Y * 1.5,
            ..Default::default()
        }));
        v.particles = Some(Box::new(EmitterDef {
            preset: "magic".into(),
            color: "#ffe2a0".into(),
            area: Vec3::new(1.4, 0.05, 1.4),
            size: 1.3,
            ..Default::default()
        }));
        v.distortion =
            Some(Box::new(DistortDef { kind: "ripple".into(), radius: 1.8, strength: 0.3, period: 1.4, offset: Vec3::Y * 0.4 }));
        self.sim.spawn(Spawn::new("exit gate", at + Vec3::Y * 0.04).visual(v));
    }

    /// A ring of red runes over the gate while the level's boss lives.
    fn exit_seal(&mut self, at: Vec3) -> EntityId {
        let mut v = Visual::new(Shape::Cylinder { half_height: 0.05, radius: 1.9 }, Color::hex("#ff2a2a"));
        v.look = Look::Unlit;
        v.emissive = 1.4;
        v.light = Some(Box::new(LightDef {
            color: "#ff3a2a".into(),
            radius: 6.0,
            intensity: 1.6,
            pulse: 1.2,
            offset: Vec3::Y,
            ..Default::default()
        }));
        v.particles = Some(Box::new(EmitterDef {
            preset: "embers".into(),
            color: "#ff4a3a".into(),
            area: Vec3::new(1.6, 0.05, 1.6),
            rate: 20.0,
            ..Default::default()
        }));
        self.sim.spawn(Spawn::new("exit seal", at + Vec3::Y * 0.09).visual(v))
    }

    fn game(&mut self) -> &mut Game {
        self.game.as_mut().unwrap()
    }

    // ---------------------------------------------------------------------- mechanics

    fn feature(&mut self, kind: FeatureKind, pos: Vec3, room: usize, entity: Option<EntityId>, deco: Vec<EntityId>) -> usize {
        self.features.push(Feature { kind, pos, room, entity, deco });
        self.features.len() - 1
    }

    /// Places every mechanic of the plan, signature ones generously, mixed-in ones sparingly.
    fn mechanics(&mut self) {
        let plan = self.plan;
        let mid = self.middle_rooms();
        if mid.is_empty() {
            return;
        }
        // Ice and crumbling floors were built with the floor: remember where they are.
        for i in 0..self.layout.rooms.len() {
            let r = self.layout.rooms[i].rect;
            let c = Vec3::new(r.center().x, 0.0, r.center().y);
            if self.ice[i] {
                self.feature(FeatureKind::Ice { min: r.min, max: r.max }, c, i, None, Vec::new());
            }
            if self.crumble[i] {
                self.feature(FeatureKind::Crumble { min: r.min, max: r.max }, c, i, None, Vec::new());
            }
        }
        for c in 0..self.layout.corridors.len() {
            let cor = &self.layout.corridors[c];
            if self.ice[cor.rooms.0] && self.ice[cor.rooms.1] {
                let (r, room) = (cor.rect, cor.rooms.0);
                let p = Vec3::new(r.center().x, 0.0, r.center().y);
                self.feature(FeatureKind::Ice { min: r.min, max: r.max }, p, room, None, Vec::new());
            }
        }
        let all: Vec<usize> = (0..self.layout.rooms.len()).filter(|i| *i != self.layout.start).collect();
        let w = |m| plan.weight(m);
        if w(Mechanic::Shrines) > 0.0 && !mid.is_empty() {
            let n = ((mid.len() as f32 * 0.55 * w(Mechanic::Shrines)).round() as usize).max(1);
            for k in 0..n {
                let room = mid[(k * mid.len() / n + self.rng.below(2) as usize).min(mid.len() - 1)];
                if let Some(p) = self.spot(room, 2.2, 3.0) {
                    let boon = Boon::ALL[self.rng.below(Boon::ALL.len() as u32) as usize];
                    self.shrine(p, room, boon);
                }
            }
        }
        if w(Mechanic::RiftGates) > 0.0 && self.layout.path.len() >= 3 {
            // A shortcut from near the start to near the exit, and (signature levels) one from
            // a side room into the middle of the path.
            let path = self.layout.path.clone();
            let n = path.len();
            let mut pairs = vec![(path[1.min(n - 2)], path[n - 2].max(path[1]))];
            if pairs[0].0 == pairs[0].1 {
                pairs[0] = (path[0], path[n - 1]);
            }
            if w(Mechanic::RiftGates) >= 1.0 {
                let side: Vec<usize> =
                    (0..self.layout.rooms.len()).filter(|i| self.layout.rooms[*i].role == RoomRole::Side).collect();
                let a = if side.is_empty() { path[2.min(n - 1)] } else { side[self.rng.below(side.len() as u32) as usize] };
                let b = path[n / 2];
                if a != b {
                    pairs.push((a, b));
                }
            }
            for (a, b) in pairs {
                let pa = self.spot(a, 2.5, 3.5).or_else(|| self.spot(a, 1.6, 2.4));
                let pb = self.spot(b, 2.5, 3.5).or_else(|| self.spot(b, 1.6, 2.4));
                let (Some(pa), Some(pb)) = (pa, pb) else { continue };
                let ia = self.gate(pa, a);
                let ib = self.gate(pb, b);
                if let FeatureKind::Gate { to, .. } = &mut self.features[ia].kind {
                    *to = ib;
                }
                if let FeatureKind::Gate { to, .. } = &mut self.features[ib].kind {
                    *to = ia;
                }
            }
        }
        if w(Mechanic::Windways) > 0.0 {
            let n = self.layout.corridors.len();
            for c in 0..n {
                if !self.chance(0.95 * w(Mechanic::Windways)) {
                    continue;
                }
                let cor = self.layout.corridors[c].clone();
                // Mostly blowing toward the exit (the path's direction), now and then against it.
                let (a, b) = cor.rooms;
                let ca = self.layout.rooms[a].rect.center();
                let cb = self.layout.rooms[b].rect.center();
                let mut dir = (cb - ca).normalize_or_zero();
                if self.chance(0.25) {
                    dir = -dir;
                }
                // Stretch the strip into both rooms a little.
                let r = cor.rect;
                let ext = 3.0;
                let rect = if cor.along_x {
                    Rect { min: Vec2::new(r.min.x - ext, r.min.y), max: Vec2::new(r.max.x + ext, r.max.y) }
                } else {
                    Rect { min: Vec2::new(r.min.x, r.min.y - ext), max: Vec2::new(r.max.x, r.max.y + ext) }
                };
                self.wind(rect, Vec3::new(dir.x, 0.0, dir.y), a);
            }
            // A crosswind in a few rooms.
            for &room in &mid {
                if self.chance(0.3 * w(Mechanic::Windways)) {
                    let r = self.layout.rooms[room].rect;
                    let c = r.center();
                    let along_x = self.chance(0.5);
                    let rect = if along_x {
                        Rect { min: Vec2::new(r.min.x + 1.0, c.y - 2.0), max: Vec2::new(r.max.x - 1.0, c.y + 2.0) }
                    } else {
                        Rect { min: Vec2::new(c.x - 2.0, r.min.y + 1.0), max: Vec2::new(c.x + 2.0, r.max.y - 1.0) }
                    };
                    let s = if self.chance(0.5) { 1.0 } else { -1.0 };
                    let dir = if along_x { Vec3::X * s } else { Vec3::Z * s };
                    self.wind(rect, dir, room);
                }
            }
        }
        if w(Mechanic::Gauntlet) > 0.0 {
            let n = self.layout.corridors.len();
            for c in 0..n {
                if !self.chance(0.8 * w(Mechanic::Gauntlet)) {
                    continue;
                }
                let r = self.layout.corridors[c].rect;
                let room = self.layout.corridors[c].rooms.0;
                let phase = self.rng.range(0.0, 2.6);
                self.spikes(r.center(), room, phase);
            }
            for &room in &mid {
                if !self.chance(0.5 * w(Mechanic::Gauntlet)) {
                    continue;
                }
                // A row of plates firing one after another.
                let r = self.layout.rooms[room].rect;
                let c = r.center();
                let along_x = r.size().x >= r.size().y;
                let phase = self.rng.range(0.0, 2.6);
                for k in -1..=1 {
                    let p = if along_x { c + Vec2::new(k as f32 * 4.0, 0.0) } else { c + Vec2::new(0.0, k as f32 * 4.0) };
                    if self.taken[room].iter().all(|(q, qr)| (p - *q).length() > qr + 2.0) {
                        self.taken[room].push((p, 2.2));
                        self.spikes(p, room, phase + (k + 1) as f32 * 0.35);
                    }
                }
            }
        }
        if w(Mechanic::MoltenFloor) > 0.0 {
            for &room in &mid {
                if !self.chance(0.75 * w(Mechanic::MoltenFloor) + 0.15) {
                    continue;
                }
                let n = 1 + (w(Mechanic::MoltenFloor) >= 1.0) as u32 + self.rng.below(2);
                for _ in 0..n {
                    let radius = self.rng.range(1.8, 3.2);
                    if let Some(p) = self.spot(room, radius + 0.8, radius + 1.5) {
                        self.lava(p, room, radius);
                    }
                }
            }
        }
        if w(Mechanic::TimeRift) > 0.0 {
            for &room in &mid {
                if !self.chance(0.6 * w(Mechanic::TimeRift) + 0.1) {
                    continue;
                }
                let radius = self.rng.range(5.0, 6.5);
                let c = self.layout.rooms[room].rect.center();
                self.bubble(c, room, radius);
            }
        }
        if w(Mechanic::LightlessDeep) > 0.0 {
            for &room in &all {
                if room == self.layout.exit || !self.chance(0.75 * w(Mechanic::LightlessDeep) + 0.1) {
                    continue;
                }
                if let Some(p) = self.spot(room, 2.0, 3.0) {
                    self.well(p, room);
                }
            }
        }
        if w(Mechanic::CursedVaults) > 0.0 {
            let mut rooms: Vec<usize> = mid.clone();
            rooms.sort_by_key(|r| if self.layout.rooms[*r].role == RoomRole::Side { 0 } else { 1 });
            let n = if w(Mechanic::CursedVaults) >= 1.0 { (rooms.len() / 2).max(2) } else { 1 };
            for &room in rooms.iter().take(n) {
                if let Some(p) = self.spot(room, 4.0, 4.0) {
                    self.chest(p, room);
                }
            }
        }
    }

    fn shrine(&mut self, p: Vec2, room: usize, boon: Boon) {
        let at = Vec3::new(p.x, 0.0, p.y);
        let st = &mut self.sim.state;
        let stone = Color::hex(&self.plan.theme.pillar);
        st.statics.add(
            &mut st.physics,
            Block::new(at + Vec3::new(-0.9, 0.0, -0.9), at + Vec3::new(0.9, 0.25, 0.9), stone.scale(0.85))
                .with_flags(block_flags::GHOST | block_flags::ROUNDED),
        );
        let col = boon.color();
        let mut v = Visual::new(Shape::Box { half: Vec3::new(0.18, 0.18, 0.18) }, Color::hex(col));
        v.look = Look::Unlit;
        v.emissive = 2.4;
        v.light = Some(Box::new(LightDef { color: col.into(), radius: 7.0, intensity: 2.0, pulse: 0.7, ..Default::default() }));
        v.particles = Some(Box::new(EmitterDef {
            preset: "magic".into(),
            color: col.into(),
            area: Vec3::new(0.6, 0.6, 0.6),
            ..Default::default()
        }));
        let gem = self.sim.spawn(Spawn::new("shrine", at + Vec3::Y * 1.4).visual(v));
        let mut deco = Vec::new();
        for k in 0..3 {
            let a = k as f32 / 3.0 * std::f32::consts::TAU;
            let mut v = Visual::new(Shape::Box { half: Vec3::new(0.12, 0.7, 0.12) }, stone);
            v.look = Look::Lit;
            let mut s = Spawn::new("shrine stone", at + Vec3::new(a.cos() * 0.75, 0.7, a.sin() * 0.75)).visual(v);
            s.rot = Quat::from_rotation_y(-a) * Quat::from_rotation_z(0.18);
            deco.push(self.sim.spawn(s));
        }
        self.feature(FeatureKind::Shrine { boon, used: false }, at, room, Some(gem), deco);
    }

    fn gate(&mut self, p: Vec2, room: usize) -> usize {
        let at = Vec3::new(p.x, 0.0, p.y);
        let st = &mut self.sim.state;
        let stone = Color::hex("#3a3448");
        for k in 0..6 {
            let a = k as f32 / 6.0 * std::f32::consts::TAU;
            let c = at + Vec3::new(a.cos(), 0.0, a.sin()) * 1.5;
            st.statics.add(
                &mut st.physics,
                Block::new(c + Vec3::new(-0.18, 0.0, -0.18), c + Vec3::new(0.18, 1.1 + 0.3 * (k % 2) as f32, 0.18), stone)
                    .with_flags(block_flags::ROUNDED),
            );
        }
        let mut v = Visual::new(Shape::Cylinder { half_height: 0.03, radius: 1.1 }, Color::hex("#b07aff"));
        v.look = Look::Unlit;
        v.emissive = 1.1;
        v.light = Some(Box::new(LightDef {
            color: "#a070ff".into(),
            radius: 7.0,
            intensity: 1.8,
            pulse: 0.9,
            offset: Vec3::Y,
            ..Default::default()
        }));
        v.particles = Some(Box::new(EmitterDef {
            preset: "magic".into(),
            color: "#c9a0ff".into(),
            area: Vec3::new(0.9, 0.05, 0.9),
            ..Default::default()
        }));
        v.distortion =
            Some(Box::new(DistortDef { kind: "ripple".into(), radius: 1.3, strength: 0.4, period: 0.9, offset: Vec3::Y * 0.3 }));
        let e = self.sim.spawn(Spawn::new("rift gate", at + Vec3::Y * 0.04).visual(v));
        self.feature(FeatureKind::Gate { to: usize::MAX, cd: 0.0 }, at, room, Some(e), Vec::new())
    }

    fn wind(&mut self, rect: Rect, dir: Vec3, room: usize) {
        let facing = if dir.x > 0.5 {
            Facing::East
        } else if dir.x < -0.5 {
            Facing::West
        } else if dir.z > 0.5 {
            Facing::South
        } else {
            Facing::North
        };
        let zone = Zone {
            min: Vec3::new(rect.min.x, -0.5, rect.min.y),
            max: Vec3::new(rect.max.x, 2.5, rect.max.y),
            kind: ZoneKind::Conveyor,
            course: String::new(),
            index: 0,
            params: Default::default(),
            camera: None,
            label: String::new(),
            label_size: None,
            color: Some(Color::hex("#9fd8ff").scale(0.35)),
            facing: Some(facing),
            speed: 4.5,
            signal: String::new(),
            note: String::new(),
        };
        let c = rect.center();
        let st = &mut self.sim.state;
        st.statics.add_zone_to(RegionKey::chunk_of(Vec3::new(c.x, 0.0, c.y)), zone);
        // Chevrons streaming down the strip.
        let len = if dir.x.abs() > 0.5 { rect.size().x } else { rect.size().y };
        let n = (len / 3.0).floor().max(1.0) as usize;
        let mut chevrons = Vec::new();
        for k in 0..n {
            for sd in [-1.0f32, 1.0] {
                let mut v = Visual::new(Shape::Box { half: Vec3::new(0.04, 0.01, 0.45) }, Color::hex("#bfe4f8"));
                v.look = Look::Unlit;
                v.emissive = 0.25;
                let mut s = Spawn::new("~wind", Vec3::new(c.x, 0.03, c.y)).visual(v);
                s.rot = Quat::from_rotation_y(dir.x.atan2(dir.z) + sd * 0.7);
                let id = self.sim.spawn(s);
                chevrons.push(id);
                let _ = k;
            }
        }
        let mut v = Visual::new(Shape::Sphere { radius: 0.01 }, Color::hex("#000000"));
        v.look = Look::Unlit;
        v.particles = Some(Box::new(EmitterDef {
            preset: "dust".into(),
            color: "#dff2ff".into(),
            rate: 8.0,
            size: 0.5,
            area: Vec3::new(rect.size().x * 0.5, 0.6, rect.size().y * 0.5),
            ..Default::default()
        }));
        let air = self.sim.spawn(Spawn::new("~gust", Vec3::new(c.x, 0.8, c.y)).visual(v));
        self.feature(
            FeatureKind::Wind { min: rect.min, max: rect.max, dir, chevrons },
            Vec3::new(c.x, 0.0, c.y),
            room,
            Some(air),
            Vec::new(),
        );
    }

    fn spikes(&mut self, p: Vec2, room: usize, phase: f32) {
        let at = Vec3::new(p.x, 0.0, p.y);
        let half = 1.9;
        let st = &mut self.sim.state;
        st.statics.add(
            &mut st.physics,
            Block::new(at + Vec3::new(-half, 0.0, -half), at + Vec3::new(half, 0.03, half), Color::hex("#3a3634"))
                .with_flags(block_flags::GHOST),
        );
        let mut spikes = Vec::new();
        for i in 0..3 {
            for j in 0..3 {
                let o = Vec3::new((i as f32 - 1.0) * 1.2, -0.6, (j as f32 - 1.0) * 1.2);
                let mut v = Visual::new(Shape::Capsule { half_height: 0.32, radius: 0.09 }, Color::hex("#c8c4bc"));
                v.look = Look::Lit;
                spikes.push(self.sim.spawn(Spawn::new("~spike", at + o).visual(v)));
            }
        }
        self.feature(FeatureKind::Spikes { half, period: 2.8, phase, up: false, spikes }, at, room, None, Vec::new());
    }

    fn lava(&mut self, p: Vec2, room: usize, radius: f32) {
        let at = Vec3::new(p.x, 0.0, p.y);
        let mut crust = Visual::new(Shape::Cylinder { half_height: 0.02, radius: radius + 0.35 }, Color::hex("#1e1410"));
        crust.look = Look::Lit;
        let crust = self.sim.spawn(Spawn::new("lava crust", at + Vec3::Y * 0.015).visual(crust));
        let mut v = Visual::new(Shape::Cylinder { half_height: 0.02, radius }, Color::hex("#d8380a"));
        v.look = Look::Unlit;
        v.emissive = 0.75;
        v.light = Some(Box::new(LightDef {
            color: "#ff5a18".into(),
            radius: radius * 2.4,
            intensity: 1.5,
            flicker: 0.35,
            offset: Vec3::Y * 0.8,
            ..Default::default()
        }));
        v.particles = Some(Box::new(EmitterDef {
            preset: "embers".into(),
            area: Vec3::new(radius * 0.7, 0.05, radius * 0.7),
            rate: radius * 6.0,
            ..Default::default()
        }));
        v.distortion = Some(Box::new(DistortDef {
            kind: "haze".into(),
            radius: radius * 0.9,
            strength: 0.25,
            period: 1.0,
            offset: Vec3::Y * 0.6,
        }));
        let pool = self.sim.spawn(Spawn::new("lava", at + Vec3::Y * 0.04).visual(v));
        // Brighter seams and darker cooling crusts floating on it.
        let mut deco = vec![crust];
        for k in 0..3 {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let o = Vec3::new(a.cos(), 0.0, a.sin()) * radius * self.rng.range(0.15, 0.6);
            let r = radius * self.rng.range(0.18, 0.32);
            let (c, e) = if k == 0 { ("#ffb030", 1.1) } else { ("#3a1a10", 0.0) };
            let mut v = Visual::new(Shape::Cylinder { half_height: 0.02, radius: r }, Color::hex(c));
            v.look = if e > 0.0 { Look::Unlit } else { Look::Lit };
            v.emissive = e;
            deco.push(self.sim.spawn(Spawn::new("~lava crust", at + o + Vec3::Y * (0.06 + k as f32 * 0.004)).visual(v)));
        }
        self.feature(FeatureKind::Lava { radius, tick: 0.0 }, at, room, Some(pool), deco);
    }

    fn bubble(&mut self, p: Vec2, room: usize, radius: f32) {
        let at = Vec3::new(p.x, 0.0, p.y);
        self.taken[room].push((p, 1.0));
        let mut v = Visual::new(Shape::Cylinder { half_height: 0.01, radius }, Color::hex("#1c3c5c"));
        v.look = Look::Unlit;
        v.emissive = 0.08;
        v.light = Some(Box::new(LightDef {
            color: "#8fd0ff".into(),
            radius: radius * 1.6,
            intensity: 1.2,
            pulse: 0.25,
            offset: Vec3::Y * 2.0,
            ..Default::default()
        }));
        v.particles = Some(Box::new(EmitterDef {
            preset: "magic".into(),
            color: "#bfe6ff".into(),
            area: Vec3::new(radius * 0.7, 1.2, radius * 0.7),
            rate: 18.0,
            speed: 0.3,
            ..Default::default()
        }));
        v.distortion =
            Some(Box::new(DistortDef { kind: "ring".into(), radius, strength: 0.25, period: 3.0, offset: Vec3::Y * 0.5 }));
        let disc = self.sim.spawn(Spawn::new("time bubble", at + Vec3::Y * 0.035).visual(v));
        // Clock hands turning slowly in the middle.
        let mut hands = Vec::new();
        for (len, w) in [(radius * 0.55, 0.07), (radius * 0.35, 0.1)] {
            let mut v = Visual::new(Shape::Box { half: Vec3::new(w, 0.015, len * 0.5) }, Color::hex("#dff3ff"));
            v.look = Look::Unlit;
            v.emissive = 0.9;
            hands.push(self.sim.spawn(Spawn::new("~clock hand", at + Vec3::new(0.0, 0.06, len * 0.5)).visual(v)));
        }
        self.feature(FeatureKind::Bubble { radius, hands }, at, room, Some(disc), Vec::new());
    }

    fn well(&mut self, p: Vec2, room: usize) {
        let at = Vec3::new(p.x, 0.0, p.y);
        let st = &mut self.sim.state;
        let stone = Color::hex(&self.plan.theme.pillar).scale(0.75);
        st.statics.add(
            &mut st.physics,
            Block::new(at + Vec3::new(-0.75, 0.0, -0.75), at + Vec3::new(0.75, 0.7, 0.75), stone)
                .with_flags(block_flags::ROUNDED),
        );
        let mut v = Visual::new(Shape::Cylinder { half_height: 0.03, radius: 0.55 }, Color::hex("#5a2a14"));
        v.look = Look::Unlit;
        v.emissive = 0.8;
        v.light =
            Some(Box::new(LightDef { color: "#ff7a3a".into(), radius: 2.6, intensity: 0.8, flicker: 0.6, ..Default::default() }));
        let coals = self.sim.spawn(Spawn::new("well", at + Vec3::Y * 0.74).visual(v));
        self.feature(FeatureKind::Well { lit: false }, at, room, Some(coals), Vec::new());
    }

    fn chest(&mut self, p: Vec2, room: usize) {
        let at = Vec3::new(p.x, 0.0, p.y);
        let st = &mut self.sim.state;
        st.statics.add(
            &mut st.physics,
            Block::new(at + Vec3::new(-0.75, 0.0, -0.5), at + Vec3::new(0.75, 0.7, 0.5), Color::hex("#3a2a3e"))
                .with_flags(block_flags::ROUNDED),
        );
        let mut v = Visual::new(Shape::Box { half: Vec3::new(0.8, 0.12, 0.55) }, Color::hex("#8a4aff"));
        v.look = Look::Unlit;
        v.emissive = 0.9;
        v.light =
            Some(Box::new(LightDef { color: "#9a5aff".into(), radius: 6.0, intensity: 1.6, pulse: 0.6, ..Default::default() }));
        v.particles = Some(Box::new(EmitterDef {
            preset: "smoke".into(),
            color: "#4a2a6a".into(),
            area: Vec3::new(0.6, 0.1, 0.4),
            rate: 6.0,
            ..Default::default()
        }));
        let lid = self.sim.spawn(Spawn::new("cursed chest", at + Vec3::Y * 0.82).visual(v));
        let g = self.game();
        let spot = g.spots.len();
        g.spots.push(Spot {
            kind: SpotKind::Chest,
            name: "Cursed Chest".into(),
            pos: at,
            reach: 2.4,
            info: vec!["Open it and its keepers come, three waves of them.".into(), "Survive them for the treasure.".into()],
        });
        self.feature(
            FeatureKind::Chest { state: ChestState::Closed, wave: 0, keepers: Vec::new(), spot, t: 0.0 },
            at,
            room,
            Some(lid),
            Vec::new(),
        );
    }

    // ---------------------------------------------------------------------- monsters

    fn monsters(&mut self, st: &mut LevelState) {
        let plan = self.plan;
        let depth = plan.depth;
        let level = plan.monster_level;
        let magic = (0.12 + 0.012 * depth as f32).min(0.45);
        let rare = (0.05 + 0.006 * depth as f32).min(0.25);
        let rooms: Vec<usize> = (0..self.layout.rooms.len()).filter(|i| *i != self.layout.start).collect();
        let wk = plan.weight(Mechanic::PowderKeg);
        let wt = plan.weight(Mechanic::Totems);
        for room in rooms {
            let r = self.layout.rooms[room].rect;
            let area = r.size().x * r.size().y;
            let mut packs = 1 + (area / 190.0) as u32 + (depth >= 6) as u32;
            let is_exit = room == self.layout.exit;
            if is_exit {
                packs = 1;
            }
            for p in 0..packs.min(4) {
                let Some(at) = self.spot(room, 3.0, 3.0) else { continue };
                // The exit room of a boss-less level is guarded by a rare pack.
                let guard = is_exit && plan.boss.is_none() && p == 0;
                let rarity = if guard || self.chance(rare) {
                    Rarity::Rare
                } else if self.chance(magic) {
                    Rarity::Magic
                } else {
                    Rarity::Normal
                };
                let feet = Vec3::new(at.x, 0.1, at.y);
                self.pack(feet, level, rarity);
                // Kegs beside packs, totems among them.
                if wk > 0.0 && self.chance(0.6 * wk + 0.1) {
                    let n = 2 + self.rng.below(3);
                    for k in 0..n {
                        let a = self.rng.range(0.0, std::f32::consts::TAU);
                        let o = Vec2::new(a.cos(), a.sin()) * self.rng.range(2.0, 3.8);
                        let q = at + o;
                        if r.shrink(1.2).contains(q) && self.taken[room].iter().all(|(c, cr)| (q - *c).length() > cr * 0.6) {
                            self.keg(q, room, level);
                        }
                        let _ = k;
                    }
                }
                if wt > 0.0 && self.chance(0.55 * wt + 0.1) {
                    if let Some(q) = self.spot(room, 1.2, 2.5) {
                        if (q - at).length() < 12.0 {
                            self.totem(q, room, level);
                        }
                    }
                }
            }
        }
        // The boss waits at the exit.
        if let Some(key) = &plan.boss {
            let c = self.layout.rooms[self.layout.exit].rect.center();
            let at = Vec3::new(c.x, 0.1, c.y - 5.0);
            let g = self.game.as_mut().unwrap();
            if let Some(id) = g.spawn_boss(self.sim, key, level + 1, at) {
                if let Some(b) = g.actors.get_mut(&id).and_then(|a| a.brain.as_mut()) {
                    b.aggro = false;
                }
                st.boss = Some(id);
                st.boss_name = g.actors.get(&id).map(|a| a.name.clone()).unwrap_or_default();
            }
        }
    }

    fn pack(&mut self, feet: Vec3, level: u32, rarity: Rarity) {
        let info = PackInfo::of(self.plan);
        let g = self.game.as_mut().unwrap();
        spawn_pack(g, self.sim, &mut self.rng, &info, feet, level, rarity, false);
    }

    fn keg(&mut self, p: Vec2, room: usize, level: u32) {
        let at = Vec3::new(p.x, 0.0, p.y);
        self.taken[room].push((p, 0.6));
        let mut v = Visual::new(Shape::Cylinder { half_height: 0.42, radius: 0.36 }, Color::hex("#7a4a22"));
        v.look = Look::Lit;
        let id = self.sim.spawn(Spawn::new("powder keg", at + Vec3::Y * 0.42).visual(v).body(BodyKind::Fixed));
        let mut band = Visual::new(Shape::Cylinder { half_height: 0.05, radius: 0.38 }, Color::hex("#2a2624"));
        band.look = Look::Lit;
        let b1 = self.sim.spawn(Spawn::new("~keg band", at + Vec3::Y * 0.62).visual(band.clone()));
        let b2 = self.sim.spawn(Spawn::new("~keg band", at + Vec3::Y * 0.22).visual(band));
        let mut fuse = Visual::new(Shape::Sphere { radius: 0.07 }, Color::hex("#ff3a1a"));
        fuse.look = Look::Unlit;
        fuse.emissive = 2.0;
        let f = self.sim.spawn(Spawn::new("~keg mark", at + Vec3::Y * 0.9).visual(fuse));
        let g = self.game.as_mut().unwrap();
        let base = super::stats::Base { life: 1.0, mana: 0.0, life_regen: 0.0, mana_regen: 0.0, armor: 0.0, res: 0.0 };
        let mut a = super::combat::Actor::new(
            Team::Neutral,
            "Powder Keg",
            level,
            super::stats::Sheet::compute(base, &Default::default()),
        );
        a.base = base;
        a.radius = 0.4;
        a.immovable = true;
        a.life = a.sheet.life_max;
        g.actors.insert(id, a);
        self.feature(FeatureKind::Keg { fuse: 0.0 }, at, room, Some(id), vec![b1, b2, f]);
    }

    fn totem(&mut self, p: Vec2, room: usize, level: u32) {
        let at = Vec3::new(p.x, 0.0, p.y);
        let t = self.plan.theme.clone();
        let mut v = Visual::new(Shape::Box { half: Vec3::new(0.3, 1.05, 0.3) }, Color::hex(&t.wall).scale(0.7));
        v.look = Look::Lit;
        let mut s = Spawn::new("ward totem", at + Vec3::Y * 1.05).visual(v).body(BodyKind::Fixed);
        s.rot = Quat::from_rotation_y(0.785);
        let id = self.sim.spawn(s);
        let mut deco = Vec::new();
        for (k, y) in [0.7f32, 1.4].iter().enumerate() {
            let mut v = Visual::new(Shape::Box { half: Vec3::new(0.36, 0.06, 0.36) }, Color::hex(&t.accent));
            v.look = Look::Unlit;
            v.emissive = 1.2;
            let mut s = Spawn::new("~totem band", at + Vec3::Y * *y).visual(v);
            s.rot = Quat::from_rotation_y(0.785 + k as f32 * 0.3);
            deco.push(self.sim.spawn(s));
        }
        let mut eye = Visual::new(Shape::Sphere { radius: 0.2 }, Color::hex(&t.accent));
        eye.look = Look::Unlit;
        eye.emissive = 2.5;
        eye.light =
            Some(Box::new(LightDef { color: t.accent.clone(), radius: 8.0, intensity: 1.4, pulse: 0.8, ..Default::default() }));
        deco.push(self.sim.spawn(Spawn::new("~totem eye", at + Vec3::Y * 2.35).visual(eye)));
        let g = self.game.as_mut().unwrap();
        let life = super::combat::monster_life(level) * 3.0 * self.sim.config.difficulty.enemy_life;
        let base = super::stats::Base { life, mana: 0.0, life_regen: 0.0, mana_regen: 0.0, armor: level as f32 * 4.0, res: 20.0 };
        let mut a = super::combat::Actor::new(
            Team::Monster,
            "Ward Totem",
            level,
            super::stats::Sheet::compute(base, &Default::default()),
        );
        a.base = base;
        a.family = "totem".into();
        a.radius = 0.5;
        a.immovable = true;
        a.life = a.sheet.life_max;
        a.xp = super::combat::monster_xp(level) * 2.0;
        g.actors.insert(id, a);
        self.feature(FeatureKind::Totem, at, room, Some(id), deco);
    }
}
