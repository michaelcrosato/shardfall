//! Action moves as data (anim/moves.toml): attacks, casts and gestures described by numbers.
//! A move is an arc the striking hand (or foot) sweeps around the shoulders, with timing and
//! whole-body motion: wind-up from wherever the arm was, a strike with lunge, lean, twist, hop or
//! spin, then a recovery that holds, follows through and eases back. `frame` turns a move and its
//! progress into a `MoveFrame` (pure: rewind and replays see exactly the same motion), and the
//! puppet turns that into hand and foot targets for its IK limbs.

use std::collections::HashMap;
use std::f32::consts::{PI, TAU};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What strikes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Hand {
    #[default]
    Right,
    Left,
    /// A two-handed grip: the left hand holds below the right.
    Both,
    /// Both hands, mirrored (a cast, a whirlwind, arms flung wide).
    Pair,
    /// The right foot.
    Kick,
}

/// The arc's plane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plane {
    /// Level, around the shoulders (0 = straight ahead, + = the right side).
    #[default]
    Ground,
    /// Up and down in front (0 = straight ahead, + = up).
    Side,
}

/// One number, or `[start, end]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span(pub [f32; 2]);

impl Span {
    pub fn at(self, k: f32) -> f32 {
        self.0[0] + (self.0[1] - self.0[0]) * k
    }
}

impl Serialize for Span {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.0[0] == self.0[1] { self.0[0].serialize(s) } else { self.0.serialize(s) }
    }
}

impl<'de> Deserialize<'de> for Span {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OneOrTwo {
            One(f32),
            Two([f32; 2]),
        }
        Ok(match OneOrTwo::deserialize(d)? {
            OneOrTwo::One(v) => Span([v, v]),
            OneOrTwo::Two(v) => Span(v),
        })
    }
}

/// One move (see anim/moves.toml for what every field means).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MoveDef {
    pub name: String,
    pub about: String,
    pub hand: Hand,
    pub plane: Plane,
    /// Degrees along the arc.
    pub from: f32,
    pub to: f32,
    /// Above the shoulders in arm lengths (kicks: shares of hip height above the ground).
    pub height: Span,
    /// Arm lengths from the shoulders (kicks: leg lengths).
    pub reach: Span,
    /// Seconds: the move's shape; the skill or caller decides the real time.
    pub wind: f32,
    pub active: f32,
    pub recover: f32,
    /// Share of `active` where the hit lands.
    pub hit: f32,
    /// Share of `recover` spent holding the end pose.
    pub hold: f32,
    /// Metres (on a 1.8 m figure, scaled with the puppet).
    pub lunge: f32,
    pub hop: f32,
    pub crouch: f32,
    /// Radians of forward lean through the strike (negative leans back).
    pub lean: f32,
    /// How much the torso turns with the swing angle.
    pub twist: f32,
    pub spin: bool,
    pub trail: bool,
    /// The move played on alternate swings.
    pub alt: String,
}

impl Default for MoveDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            about: String::new(),
            hand: Hand::Right,
            plane: Plane::Ground,
            from: 0.0,
            to: 0.0,
            height: Span([-0.76, -0.76]),
            reach: Span([0.82, 0.82]),
            wind: 0.06,
            active: 0.1,
            recover: 0.18,
            hit: 0.35,
            hold: 0.3,
            lunge: 0.064,
            hop: 0.0,
            crouch: 0.15,
            lean: 0.3,
            twist: 1.0,
            spin: false,
            trail: true,
            alt: String::new(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MovesFile {
    #[serde(rename = "move")]
    moves: Vec<MoveDef>,
}

/// Every move, indexed: the index is what animation state stores (0 = none).
#[derive(Clone, Debug)]
pub struct MoveTable {
    /// `[0]` is the empty "none" move.
    pub moves: Vec<MoveDef>,
    index: HashMap<String, u8>,
}

impl MoveTable {
    /// Parses a moves file. Names already in `keep` keep their index, so animation state that
    /// was saved (snapshots, a running game during live editing) stays meaningful.
    pub fn parse(text: &str, keep: Option<&MoveTable>) -> Result<Self, String> {
        let file: MovesFile = toml::from_str(text).map_err(|e| format!("moves.toml: {e}"))?;
        let mut moves = vec![MoveDef { name: "none".into(), ..Default::default() }];
        let mut fresh = Vec::new();
        if let Some(k) = keep {
            moves.resize(k.moves.len(), MoveDef::default());
        }
        let mut seen = HashMap::new();
        for m in file.moves {
            if m.name.is_empty() || m.name == "none" {
                return Err(format!("moves.toml: a move needs a name other than \"none\" (after {:?})", seen.keys().last()));
            }
            if seen.insert(m.name.clone(), ()).is_some() {
                return Err(format!("moves.toml: \"{}\" is defined twice", m.name));
            }
            if m.wind < 0.0 || m.active <= 0.0 || m.recover < 0.0 {
                return Err(format!("moves.toml: \"{}\" needs active > 0 and no negative times", m.name));
            }
            match keep.and_then(|k| k.index.get(&m.name)) {
                Some(&i) => moves[i as usize] = m,
                None => fresh.push(m),
            }
        }
        moves.extend(fresh);
        // Moves dropped from the file leave an inert gap (their index stays reserved).
        if moves.len() > 255 {
            return Err("moves.toml: at most 255 moves".into());
        }
        let index = moves
            .iter()
            .enumerate()
            .filter(|(i, m)| *i > 0 && !m.name.is_empty())
            .map(|(i, m)| (m.name.clone(), i as u8))
            .collect();
        let t = MoveTable { moves, index };
        for m in t.moves.iter().filter(|m| !m.alt.is_empty()) {
            if !t.index.contains_key(&m.alt) {
                return Err(format!("moves.toml: \"{}\" has alt \"{}\", which is not a move", m.name, m.alt));
            }
        }
        Ok(t)
    }

    pub fn get(&self, id: u8) -> Option<&MoveDef> {
        (id > 0).then(|| self.moves.get(id as usize)).flatten().filter(|m| !m.name.is_empty())
    }

    pub fn id(&self, name: &str) -> Option<u8> {
        self.index.get(name).copied()
    }

    /// Move names in table order.
    pub fn names(&self) -> Vec<&str> {
        self.moves.iter().skip(1).filter(|m| !m.name.is_empty()).map(|m| m.name.as_str()).collect()
    }
}

static TABLE: RwLock<Option<Arc<MoveTable>>> = RwLock::new(None);

/// The moves (the embedded file the first time).
pub fn table() -> Arc<MoveTable> {
    if let Some(t) = TABLE.read().unwrap().as_ref() {
        return t.clone();
    }
    let text = crate::anim::source("moves.toml").unwrap_or_default();
    let t = Arc::new(MoveTable::parse(&text, None).unwrap_or_else(|e| panic!("{e}")));
    *TABLE.write().unwrap() = Some(t.clone());
    t
}

/// Re-reads moves.toml (see `crate::anim::reload`). Returns how many moves there are.
pub fn reload() -> Result<usize, String> {
    let text = crate::anim::source("moves.toml").ok_or("anim/moves.toml is missing")?;
    let old = table();
    let t = MoveTable::parse(&text, Some(&old))?;
    let n = t.names().len();
    *TABLE.write().unwrap() = Some(Arc::new(t));
    Ok(n)
}

/// A move, stored as its index in animation state and written as its name in data files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MoveId(pub u8);

impl MoveId {
    pub const NONE: MoveId = MoveId(0);

    /// The move called `name`, if there is one.
    pub fn named(name: &str) -> Option<MoveId> {
        if name.is_empty() || name == "none" {
            return Some(MoveId::NONE);
        }
        table().id(name).map(MoveId)
    }

    /// The move called `name`, or none.
    pub fn of(name: &str) -> MoveId {
        Self::named(name).unwrap_or(MoveId::NONE)
    }

    pub fn index(self) -> u8 {
        self.0
    }

    pub fn name(self) -> String {
        table().get(self.0).map(|m| m.name.clone()).unwrap_or_else(|| "none".into())
    }
}

impl Serialize for MoveId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.name().serialize(s)
    }
}

impl<'de> Deserialize<'de> for MoveId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        MoveId::named(&name).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown move \"{name}\" (anim/moves.toml has: {})", table().names().join(", ")))
        })
    }
}

/// Where a move is in its three phases.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Wind,
    Active,
    Recover,
}

/// What a move does at one moment.
#[derive(Clone, Copy, Debug, Default)]
pub struct MoveFrame {
    pub hand: Hand,
    pub side_plane: bool,
    /// The swing: angle along the arc (radians), height and reach (see `MoveDef`).
    pub theta: f32,
    pub height: f32,
    pub reach: f32,
    /// How much the move holds the arms (or the kicking leg) over the walk, 0..1.
    pub w: f32,
    /// Progress of the wind-up's blend from where the arms were (1 once it is done).
    pub wind_k: f32,
    pub lean: f32,
    /// Metres.
    pub lunge: f32,
    pub hop: f32,
    pub crouch: f32,
    /// Torso turn (radians, + turns the right shoulder back).
    pub twist: f32,
    /// Whole-body turn (radians).
    pub spin: f32,
    pub phase: Phase,
    /// Progress through the phase, 0..1.
    pub u: f32,
    /// The weapon draws a trail now.
    pub trail: bool,
}

fn out_quad(u: f32) -> f32 {
    1.0 - (1.0 - u) * (1.0 - u)
}

fn out_cubic(u: f32) -> f32 {
    1.0 - (1.0 - u).powi(3)
}

fn in_out(u: f32) -> f32 {
    if u < 0.5 { 2.0 * u * u } else { 1.0 - (-2.0 * u + 2.0).powi(2) / 2.0 }
}

impl MoveDef {
    /// The move's own seconds at progress `t` (0..1) when the hit lands at `hit` (0..1): the
    /// wind-up fills the time before the hit, the rest after it.
    pub fn time_at(&self, t: f32, hit: f32) -> f32 {
        let total = self.wind + self.active + self.recover;
        let t_hit = self.wind + self.hit.clamp(0.0, 1.0) * self.active;
        let h = hit.clamp(0.02, 0.98);
        let t = t.clamp(0.0, 1.0);
        if t < h { t / h * t_hit } else { t_hit + (t - h) / (1.0 - h) * (total - t_hit) }
    }

    /// The frame at the move's own time `mt` (seconds).
    pub fn frame_at(&self, mt: f32) -> MoveFrame {
        let side_plane = self.plane == Plane::Side;
        let kick = self.hand == Hand::Kick;
        let rad = PI / 180.0;
        let (a0, a1) = (self.from * rad, self.to * rad);
        let rest = if side_plane {
            -0.9
        } else if self.hand == Hand::Left {
            -1.1
        } else {
            1.1
        };
        let (h0, h1) = (self.height.0[0], self.height.0[1]);
        let (r0, r1) = (self.reach.0[0], self.reach.0[1]);
        let lean_back = -self.lean.abs() * 0.45;
        let mut f = MoveFrame { hand: self.hand, side_plane, trail: self.trail, ..Default::default() };
        if mt < self.wind {
            // Anticipation: the arc's start is reached from the rest angle; the body coils back.
            let u = (mt / self.wind.max(1e-4)).clamp(0.0, 1.0);
            let k = out_quad(u);
            f.phase = Phase::Wind;
            f.u = u;
            f.theta = rest + (a0 - rest) * k;
            let h_start = if kick || !self.trail { h0 } else { -0.76 };
            f.height = h_start + (h0 - h_start) * u;
            f.reach = r0 * (0.75 + 0.25 * k);
            f.w = 1.0;
            f.wind_k = k;
            f.lean = lean_back * k;
            f.lunge = -self.lunge * 0.3 * k;
            f.crouch = self.crouch * k;
        } else if mt < self.wind + self.active {
            let u = ((mt - self.wind) / self.active.max(1e-4)).clamp(0.0, 1.0);
            let k = out_cubic(u);
            f.phase = Phase::Active;
            f.u = u;
            f.theta = if self.spin { a0 } else { a0 + (a1 - a0) * k };
            f.height = h0 + (h1 - h0) * k;
            f.reach = r0 + (r1 - r0) * k;
            f.w = 1.0;
            f.wind_k = 1.0;
            f.lean = lean_back + (self.lean - lean_back) * out_quad((u * 2.5).min(1.0));
            f.lunge = -self.lunge * 0.3 + self.lunge * 1.3 * k;
            f.hop = self.hop * (k * PI).sin();
            f.crouch = self.crouch * (1.0 - k);
            if self.spin {
                f.spin = -TAU * k;
            }
        } else {
            let u = ((mt - self.wind - self.active) / self.recover.max(1e-4)).clamp(0.0, 1.0);
            let hold = self.hold.clamp(0.0, 0.95);
            let k = out_quad((u / hold.max(1e-4)).min(1.0));
            let span = if self.spin { 0.0 } else { a1 - a0 };
            f.phase = Phase::Recover;
            f.u = u;
            f.theta = if self.spin { a0 } else { a1 } + span * 0.08 * k;
            f.height = h1;
            f.reach = r1;
            let arm = if u < hold { 1.0 } else { 1.0 - in_out((u - hold) / (1.0 - hold)) };
            f.w = arm;
            f.wind_k = 1.0;
            f.lean = self.lean * arm;
            f.lunge = self.lunge * arm;
        }
        if self.hand != Hand::Pair {
            let side = if self.hand == Hand::Left { -1.0 } else { 1.0 };
            f.twist = (f.theta * if side_plane { 0.08 } else { 0.32 } * self.twist * side).clamp(-0.7, 0.7);
        }
        f.trail = self.trail && (f.phase == Phase::Active || (f.phase == Phase::Recover && f.u < 0.12));
        f
    }
}

impl MoveFrame {
    /// The point on the arc at angle `theta` relative to the centre of the shoulders, in arm
    /// lengths (character space: x right, y up, z forward); `mirror` gives the left hand of a pair.
    fn arc(&self, theta: f32, mirror: bool) -> glam::Vec3 {
        let v = if self.side_plane {
            glam::Vec3::new(0.087, theta.sin() * self.reach, theta.cos() * self.reach)
        } else {
            glam::Vec3::new(theta.sin() * self.reach, self.height, theta.cos() * self.reach)
        };
        if mirror { glam::Vec3::new(-v.x, v.y, v.z) } else { v }
    }

    /// Which way a held weapon points (unit, character space).
    pub fn weapon_dir(&self) -> glam::Vec3 {
        let (s, c) = self.theta.sin_cos();
        if self.side_plane { glam::Vec3::new(0.05, s, c).normalize() } else { glam::Vec3::new(s, -0.12, c).normalize() }
    }

    /// Each hand's target `[left, right]` relative to the centre of the shoulders, in arm
    /// lengths, and how firmly the move holds it (times `w`). The hand that isn't striking guards
    /// the chin (`fists`) or pulls back; a kick throws both arms up for balance.
    pub fn hands(&self, fists: bool) -> [(glam::Vec3, f32); 2] {
        use glam::Vec3;
        let guard = [Vec3::new(-0.109, 0.043, 0.413), Vec3::new(0.163, -0.065, 0.25)];
        let back = |s: f32| Vec3::new(s * 0.174, -0.174, 0.174);
        let other = |i: usize| if fists { (guard[i], 0.85) } else { (back(if i == 0 { -1.0 } else { 1.0 }), 0.85) };
        match self.hand {
            Hand::Kick => [(Vec3::new(-0.24, 0.065, 0.2), 0.85), (Vec3::new(0.24, -0.095, 0.2), 0.85)],
            Hand::Pair => [(self.arc(-self.theta, true), 1.0), (self.arc(self.theta, false), 1.0)],
            Hand::Both => {
                let grip = self.arc(self.theta, false);
                [(grip - self.weapon_dir() * 0.18, 1.0), (grip, 1.0)]
            }
            Hand::Left => [(self.arc(self.theta, false), 1.0), other(1)],
            Hand::Right => [other(0), (self.arc(self.theta, false), 1.0)],
        }
    }

    /// The kicking foot's target relative to the feet (character space), for a leg `leg` long
    /// and a standing hip height `hip`. None unless this is a kick.
    pub fn foot(&self, leg: f32, hip: f32) -> Option<glam::Vec3> {
        (self.hand == Hand::Kick).then(|| {
            let r = self.reach * leg;
            glam::Vec3::new(self.theta.sin() * r, self.height * hip, self.theta.cos() * r)
        })
    }
}

/// The frame of move `id` at progress `t` (0..1) of an action whose hit lands at `hit`; `side`
/// below zero plays the move's alternate. None for no move.
pub fn frame(table: &MoveTable, id: u8, t: f32, hit: f32, side: f32) -> Option<MoveFrame> {
    let m = table.get(id)?;
    let m = if side < 0.0 && !m.alt.is_empty() { table.id(&m.alt).and_then(|i| table.get(i)).unwrap_or(m) } else { m };
    Some(m.frame_at(m.time_at(t, hit)))
}

/// The move definition actually played (its alternate on `side < 0`).
pub fn played(table: &MoveTable, id: u8, side: f32) -> Option<&MoveDef> {
    let m = table.get(id)?;
    Some(if side < 0.0 && !m.alt.is_empty() { table.id(&m.alt).and_then(|i| table.get(i)).unwrap_or(m) } else { m })
}

/// The move a puppet plays for swing `combo` of skill `skill` in place of the skill's own
/// (`attack_moves`), and the side to swing it on: a list names every swing itself, so it plays
/// each as written; a single move keeps alternating like the skill's own (`side`).
pub fn attack_move(def: &crate::puppet::PuppetDef, skill: &str, combo: u32, side: f32) -> Option<(MoveId, f32)> {
    let named = def.attack_moves.get(skill)?;
    let id = MoveId::named(named.swing(combo)).filter(|m| *m != MoveId::NONE)?;
    Some((id, if named.per_swing() { 1.0 } else { side }))
}

/// Every move a puppet names that the table doesn't have (data checks).
pub fn missing(def: &crate::puppet::PuppetDef) -> Vec<String> {
    let t = table();
    def.attack_moves.values().flat_map(|s| s.names()).filter(|n| t.id(n).is_none()).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_moves_load_in_their_original_order() {
        let t = table();
        let first: Vec<&str> = t.names().into_iter().take(9).collect();
        assert_eq!(first, ["slash", "overhead", "thrust", "spin", "cast", "throw", "roar", "leap", "lunge"]);
        assert!(t.names().len() >= 30, "{}", t.names().len());
        for name in t.names() {
            let m = t.get(t.id(name).unwrap()).unwrap();
            assert!(!m.about.is_empty(), "{name} needs an about line");
        }
    }

    #[test]
    fn a_swing_sweeps_its_arc_and_comes_back() {
        let t = table();
        let slash = t.id("slash").unwrap();
        let at = |p: f32| frame(&t, slash, p, 0.5, 1.0).unwrap();
        // Wind-up ends at the arc's start (the right side), the strike sweeps across.
        let wound = at(0.33);
        assert_eq!(wound.phase, Phase::Wind);
        assert!(wound.theta > 1.3, "wound up to the right: {}", wound.theta);
        let mid = at(0.56);
        assert_eq!(mid.phase, Phase::Active);
        assert!(mid.theta < wound.theta, "swinging across: {}", mid.theta);
        assert!(at(1.0).w < 0.01, "back to the walk at the end");
        // Alternate swings play the backslash: from the left side.
        let back = frame(&t, slash, 0.33, 0.5, -1.0).unwrap();
        assert!(back.theta < -1.0, "backslash winds up on the left: {}", back.theta);
        // The hit lands where the caller says.
        let m = t.get(slash).unwrap();
        let at_hit = m.time_at(0.5, 0.5);
        assert!((at_hit - (m.wind + m.hit * m.active)).abs() < 1e-5);
    }

    #[test]
    fn moves_parse_keeps_known_indices() {
        let old = MoveTable::parse("[[move]]\nname = \"a\"\n[[move]]\nname = \"b\"\n", None).unwrap();
        let new = MoveTable::parse("[[move]]\nname = \"c\"\n[[move]]\nname = \"b\"\n", Some(&old)).unwrap();
        assert_eq!(new.id("b"), old.id("b"));
        assert!(new.get(old.id("a").unwrap()).is_none(), "a dropped move leaves a gap");
        assert_eq!(new.id("c"), Some(3));
        assert!(MoveTable::parse("[[move]]\nname = \"a\"\nalt = \"zz\"\n", None).is_err());
    }
}
