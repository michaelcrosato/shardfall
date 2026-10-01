//! The passive tree: generated from the pieces in game/tree.toml (sectors, notables,
//! keystones, masteries, skill branches) into a web of a few hundred nodes, then the endless
//! Astral rings beyond. Layout is part of generation (positions for the UI, nudged apart by a
//! small relaxation so clusters never overlap), and so are the links. Allocation rules: a node
//! needs an allocated neighbour; refunding must keep everything connected to the start.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::powers::Power;
use super::skills::{Tweak, TweakField};
use super::stats::{Mods, Stat};

// ------------------------------------------------------------------------------ data file

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Layout {
    /// Radius of the first road level, spacing between levels, lanes' angle off the road.
    pub base: f32,
    pub step: f32,
    pub lane: f32,
    pub wheel_radius: f32,
    pub endless_start: f32,
    pub endless_step: f32,
    pub endless_spacing: f32,
    pub endless_rings: u32,
    pub endless_growth: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            base: 3.0,
            step: 1.6,
            lane: 18.0,
            wheel_radius: 1.15,
            endless_start: 19.5,
            endless_step: 1.3,
            endless_spacing: 1.6,
            endless_rings: 30,
            endless_growth: 0.1,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SectorDef {
    pub name: String,
    pub angle: f32,
    pub color: String,
    pub minor: Vec<BTreeMap<String, f32>>,
    pub notables: Vec<String>,
    pub keystone: String,
    pub bridge_keystone: String,
    pub mastery: String,
    pub skills: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NodeDef {
    pub name: String,
    pub stats: BTreeMap<String, f32>,
    pub power: Option<Power>,
    pub lore: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MasteryOption {
    pub name: String,
    pub stats: BTreeMap<String, f32>,
    pub power: Option<Power>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MasteryDef {
    pub name: String,
    pub options: Vec<MasteryOption>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BranchNode {
    pub name: String,
    pub tweaks: BTreeMap<String, f32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BranchDef {
    pub nodes: Vec<BranchNode>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TreeFile {
    pub layout: Layout,
    pub sector: BTreeMap<String, SectorDef>,
    pub notable: BTreeMap<String, NodeDef>,
    pub keystone: BTreeMap<String, NodeDef>,
    pub mastery: BTreeMap<String, MasteryDef>,
    pub skill: BTreeMap<String, BranchDef>,
}

// -------------------------------------------------------------------------------- runtime

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Start,
    Minor,
    Notable,
    Keystone,
    Mastery,
    /// A tweak for one skill.
    Skill,
    /// The endless rings.
    Astral,
}

#[derive(Clone, Debug, Serialize)]
pub struct Node {
    pub id: u32,
    pub key: String,
    pub kind: NodeKind,
    pub name: String,
    /// Tree units, y down (the UI scales it).
    #[serde(skip)]
    pub pos: Vec2,
    /// Sector index (for colour).
    pub sector: u8,
    pub stats: Vec<(Stat, f32)>,
    pub power: Option<Power>,
    pub tweaks: Vec<Tweak>,
    /// Mastery nodes: which mastery's options they offer.
    pub mastery: String,
    pub lore: String,
    pub links: Vec<u32>,
    /// Astral ring index (+1), 0 for the main tree.
    pub ring: u32,
}

#[derive(Clone, Debug)]
pub struct SectorInfo {
    pub key: String,
    pub name: String,
    pub color: [f32; 3],
    pub angle: f32,
}

/// A mastery's options with resolved stats.
#[derive(Clone, Debug)]
pub struct Mastery {
    pub name: String,
    pub options: Vec<(String, Vec<(Stat, f32)>, Option<Power>)>,
}

#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub index: BTreeMap<u32, usize>,
    pub sectors: Vec<SectorInfo>,
    pub masteries: BTreeMap<String, Mastery>,
}

/// FNV-1a: node ids are hashes of their layout keys (stable when other parts change).
pub fn node_id(key: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in key.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

pub fn start_id() -> u32 {
    node_id("start")
}

/// Polar position: radius, angle in degrees (0 = up, clockwise).
fn polar(r: f32, deg: f32) -> Vec2 {
    let a = deg.to_radians();
    Vec2::new(r * a.sin(), -r * a.cos())
}

fn stats_of(owner: &str, m: &BTreeMap<String, f32>, k: f32) -> Result<Vec<(Stat, f32)>, String> {
    m.iter()
        .map(|(s, v)| {
            let st = Stat::from_key(s).ok_or_else(|| format!("tree {owner}: unknown stat '{s}'"))?;
            let v = v * k;
            Ok((st, if v.abs() >= 5.0 { v.round() } else { (v * 10.0).round() / 10.0 }))
        })
        .collect()
}

struct Builder {
    tree: Tree,
    fixed: BTreeSet<u32>,
}

impl Builder {
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        key: String,
        kind: NodeKind,
        name: String,
        pos: Vec2,
        sector: u8,
        stats: Vec<(Stat, f32)>,
    ) -> Result<u32, String> {
        let id = node_id(&key);
        if self.tree.index.contains_key(&id) {
            return Err(format!("tree: node id collision for '{key}'"));
        }
        self.tree.index.insert(id, self.tree.nodes.len());
        self.tree.nodes.push(Node {
            id,
            key,
            kind,
            name,
            pos,
            sector,
            stats,
            power: None,
            tweaks: Vec::new(),
            mastery: String::new(),
            lore: String::new(),
            links: Vec::new(),
            ring: 0,
        });
        Ok(id)
    }

    fn node(&mut self, id: u32) -> &mut Node {
        let i = self.tree.index[&id];
        &mut self.tree.nodes[i]
    }

    fn link(&mut self, a: u32, b: u32) {
        if a == b {
            return;
        }
        if !self.node(a).links.contains(&b) {
            self.node(a).links.push(b);
        }
        if !self.node(b).links.contains(&a) {
            self.node(b).links.push(a);
        }
    }
}

impl Tree {
    pub fn node(&self, id: u32) -> Option<&Node> {
        self.index.get(&id).map(|i| &self.nodes[*i])
    }

    /// Generates the tree. `skill_name` validates skill keys and gives their names.
    pub fn build(f: &TreeFile, skill_name: &dyn Fn(&str) -> Option<String>) -> Result<Tree, String> {
        let l = &f.layout;
        let mut b = Builder { tree: Tree::default(), fixed: BTreeSet::new() };
        for (k, m) in &f.mastery {
            let mut options = Vec::new();
            for o in &m.options {
                options.push((o.name.clone(), stats_of(&format!("mastery '{k}'"), &o.stats, 1.0)?, o.power));
            }
            b.tree.masteries.insert(k.clone(), Mastery { name: m.name.clone(), options });
        }
        let start = b.add("start".into(), NodeKind::Start, "The Wanderer's Start".into(), Vec2::ZERO, 0, Vec::new())?;
        b.fixed.insert(start);
        let mut sectors: Vec<(&String, &SectorDef)> = f.sector.iter().collect();
        sectors.sort_by(|a, b| a.1.angle.total_cmp(&b.1.angle));
        for (k, s) in &sectors {
            b.tree.sectors.push(SectorInfo {
                key: (*k).clone(),
                name: s.name.clone(),
                color: crate::color::Color::try_hex(&s.color).map(|c| c.0).unwrap_or([0.8; 3]),
                angle: s.angle,
            });
        }
        let pool = |s: &SectorDef, i: usize, owner: &str, k: f32| -> Result<Vec<(Stat, f32)>, String> {
            if s.minor.is_empty() {
                return Err(format!("tree sector '{owner}' has no minor stats"));
            }
            stats_of(&format!("sector '{owner}'"), &s.minor[i % s.minor.len()], k)
        };
        let notable = |key: &str| f.notable.get(key).ok_or_else(|| format!("tree: unknown notable '{key}'"));
        let keystone = |key: &str| f.keystone.get(key).ok_or_else(|| format!("tree: unknown keystone '{key}'"));
        // Each sector is a road out from the start with a lane either side (wheels and skill
        // branches) and a gutter on each border where bridges cross to the neighbours:
        //   level 0-1   right: skill branch 1      left: skill branch 2
        //   level 2     bridges (inner)
        //   level 3-4   right: wheel 1             left: wheel 2
        //   level 5     bridges (outer, with a keystone out along the gutter)
        //   level 6-7   right: wheel 3             left: skill branch 3
        //   level 8-9   the keystone path
        let r_at = |level: f32| l.base + level * l.step;
        let lane = l.lane;
        let mut roads: Vec<Vec<u32>> = Vec::new();
        let mut gates: Vec<(u32, f32)> = Vec::new();
        for (si, (sk, s)) in sectors.iter().enumerate() {
            let a = s.angle;
            let sec = si as u8;
            if s.notables.len() < 5 {
                return Err(format!("tree sector '{sk}' needs 5 notables"));
            }
            let add_notable = |b: &mut Builder, key: String, nk: &str, pos: Vec2| -> Result<u32, String> {
                let n = notable(nk)?;
                let id = b.add(key, NodeKind::Notable, n.name.clone(), pos, sec, stats_of(nk, &n.stats, 1.0)?)?;
                b.node(id).power = n.power;
                Ok(id)
            };
            // The road: levels 0..7, notables at 3 and 7.
            let mut road = Vec::new();
            let mut prev = start;
            for lv in 0..8usize {
                let key = format!("{sk}.road.{lv}");
                let pos = polar(r_at(lv as f32), a);
                let id = match lv {
                    3 => add_notable(&mut b, key, &s.notables[3], pos)?,
                    7 => add_notable(&mut b, key, &s.notables[4], pos)?,
                    _ => b.add(key, NodeKind::Minor, s.name.clone(), pos, sec, pool(s, lv + si, sk, 1.0)?)?,
                };
                b.fixed.insert(id);
                b.link(prev, id);
                road.push(id);
                prev = id;
            }
            // The keystone at the end of the road.
            let k1 = b.add(
                format!("{sk}.key.path"),
                NodeKind::Minor,
                s.name.clone(),
                polar(r_at(8.0), a),
                sec,
                pool(s, si, sk, 1.5)?,
            )?;
            b.link(road[7], k1);
            let kd = keystone(&s.keystone)?;
            let kid = b.add(
                format!("{sk}.keystone"),
                NodeKind::Keystone,
                kd.name.clone(),
                polar(r_at(9.0), a),
                sec,
                stats_of(&s.keystone, &kd.stats, 1.0)?,
            )?;
            b.node(kid).power = kd.power;
            b.node(kid).lore = kd.lore.clone();
            b.link(k1, kid);
            gates.push((k1, a));
            // Wheels: five around a mastery, in a lane between two levels.
            for (w, (level, side)) in [(3.5f32, 1.0f32), (3.5, -1.0), (6.5, 1.0)].into_iter().enumerate() {
                let anchor_level = level.floor() as usize;
                let anchor = road[anchor_level];
                let apos = polar(r_at(anchor_level as f32), a);
                let center = polar(r_at(level), a + side * lane);
                let to_anchor = (apos - center).normalize_or(Vec2::Y);
                let base = to_anchor.y.atan2(to_anchor.x);
                let mut ids = Vec::new();
                for j in 0..5 {
                    let ang = base + side * j as f32 * std::f32::consts::TAU / 5.0;
                    let pos = center + Vec2::new(ang.cos(), ang.sin()) * l.wheel_radius;
                    let key = format!("{sk}.wheel{w}.{j}");
                    let id = if j == 2 {
                        add_notable(&mut b, key, &s.notables[w], pos)?
                    } else {
                        b.add(key, NodeKind::Minor, s.name.clone(), pos, sec, pool(s, w * 3 + j + 1, sk, 1.0)?)?
                    };
                    ids.push(id);
                }
                b.link(anchor, ids[0]);
                for j in 0..5 {
                    b.link(ids[j], ids[(j + 1) % 5]);
                }
                if !s.mastery.is_empty() {
                    let m = b
                        .tree
                        .masteries
                        .get(&s.mastery)
                        .ok_or_else(|| format!("tree: unknown mastery '{}'", s.mastery))?
                        .name
                        .clone();
                    let mid = b.add(format!("{sk}.wheel{w}.mastery"), NodeKind::Mastery, m, center, sec, Vec::new())?;
                    b.node(mid).mastery = s.mastery.clone();
                    b.link(ids[2], mid);
                }
            }
            // Skill branches: three tweaks running outward along a lane.
            for (bi, skill) in s.skills.iter().enumerate() {
                let (level, side) = [(0.0f32, 1.0f32), (0.0, -1.0), (6.0, -1.0)][bi.min(2)];
                let sname = skill_name(skill).ok_or_else(|| format!("tree sector '{sk}': unknown skill '{skill}'"))?;
                let branch = f.skill.get(skill).ok_or_else(|| format!("tree: skill '{skill}' has no [skill.{skill}] branch"))?;
                let mut prev = road[level as usize];
                for (k, n) in branch.nodes.iter().enumerate() {
                    let pos = polar(r_at(level + 0.15 + k as f32 * 0.62), a + side * lane);
                    let id = b.add(
                        format!("{sk}.skill.{skill}.{k}"),
                        NodeKind::Skill,
                        format!("{sname}: {}", n.name),
                        pos,
                        sec,
                        Vec::new(),
                    )?;
                    for (field, v) in &n.tweaks {
                        let field = TweakField::from_key(field)
                            .ok_or_else(|| format!("tree skill '{skill}': unknown tweak '{field}'"))?;
                        b.node(id).tweaks.push(Tweak { skill: skill.clone(), field, value: *v });
                    }
                    b.link(prev, id);
                    prev = id;
                }
            }
            roads.push(road);
        }
        // Bridges across the gutters: road -> right lane -> gutter -> neighbour's left lane ->
        // its road, at levels 2 and 5; the outer one sends a keystone out along the gutter.
        let n = sectors.len();
        for si in 0..n {
            let sj = (si + 1) % n;
            if n < 2 || (n == 2 && si == 1) {
                break;
            }
            let (ka, sa) = sectors[si];
            let (kb, sb) = sectors[sj];
            let delta = (sb.angle - sa.angle).rem_euclid(360.0);
            for (level, outer) in [(2usize, false), (5, true)] {
                let r = r_at(level as f32);
                let angles = [sa.angle + lane, sa.angle + delta * 0.5, sa.angle + delta - lane];
                let mut prev = roads[si][level];
                let mut mid = 0;
                for (k, ang) in angles.into_iter().enumerate() {
                    let pos = polar(r + 0.25 * l.step, ang);
                    let key = format!("bridge.{ka}.{kb}.{level}.{k}");
                    let id = if k == 1 {
                        // Half of each side, a little stronger.
                        let mut st = pool(sa, level, ka, 1.25)?;
                        st.extend(pool(sb, level + 1, kb, 1.25)?);
                        b.add(key, NodeKind::Minor, format!("{} & {}", sa.name, sb.name), pos, si as u8, st)?
                    } else {
                        let (s, sk, sec) = if k == 0 { (sa, ka, si) } else { (sb, kb, sj) };
                        b.add(key, NodeKind::Minor, s.name.clone(), pos, sec as u8, pool(s, level + k, sk, 1.0)?)?
                    };
                    b.link(prev, id);
                    if k == 1 {
                        mid = id;
                    }
                    prev = id;
                }
                b.link(prev, roads[sj][level]);
                if outer && !sa.bridge_keystone.is_empty() {
                    let ma = sa.angle + delta * 0.5;
                    let p = b.add(
                        format!("bridge.{ka}.{kb}.key.path"),
                        NodeKind::Minor,
                        sa.name.clone(),
                        polar(r_at(6.4), ma),
                        si as u8,
                        pool(sa, 0, ka, 1.5)?,
                    )?;
                    b.link(mid, p);
                    let kd = keystone(&sa.bridge_keystone)?;
                    let kid = b.add(
                        format!("bridge.{ka}.{kb}.keystone"),
                        NodeKind::Keystone,
                        kd.name.clone(),
                        polar(r_at(7.6), ma),
                        si as u8,
                        stats_of(&sa.bridge_keystone, &kd.stats, 1.0)?,
                    )?;
                    b.node(kid).power = kd.power;
                    b.node(kid).lore = kd.lore.clone();
                    b.link(p, kid);
                }
            }
        }
        relax(&mut b, 0.8, 200);
        // The Astral rings, forever outward (as many as the layout asks for).
        let mut prev_ring: Vec<(u32, f32)> = Vec::new();
        for k in 0..l.endless_rings {
            let radius = l.endless_start + k as f32 * l.endless_step;
            let count = ((std::f32::consts::TAU * radius / l.endless_spacing / 6.0).round() as usize * 6).max(12);
            let grow = 1.0 + l.endless_growth * (k + 1) as f32;
            let mut ring: Vec<(u32, f32)> = Vec::new();
            for i in 0..count {
                let ang = (i as f32 + if k % 2 == 1 { 0.5 } else { 0.0 }) * 360.0 / count as f32;
                // The sector whose direction this is.
                let si = sectors
                    .iter()
                    .enumerate()
                    .min_by(|x, y| ang_dist(x.1.1.angle, ang).total_cmp(&ang_dist(y.1.1.angle, ang)))
                    .map(|x| x.0)
                    .unwrap_or(0);
                let (sk, s) = sectors[si];
                let star = i % 6 == 0;
                let mut st = pool(s, i / 6 + k as usize, sk, grow * if star { 1.6 } else { 1.0 })?;
                if star {
                    st.extend(pool(s, i / 6 + k as usize + 2, sk, grow * 1.6)?);
                }
                let name = if star { format!("Astral {} Star", s.name) } else { format!("Astral {}", s.name) };
                let id = b.add(format!("astral.{k}.{i}"), NodeKind::Astral, name, polar(radius, ang), si as u8, st)?;
                b.node(id).ring = k + 1;
                if star {
                    b.node(id).kind = NodeKind::Astral;
                    b.node(id).lore = "star".into();
                }
                ring.push((id, ang));
            }
            for i in 0..ring.len() {
                b.link(ring[i].0, ring[(i + 1) % ring.len()].0);
            }
            if k == 0 {
                // Gates from the end of each road.
                for (g, a) in &gates {
                    let near = ring.iter().min_by(|x, y| ang_dist(x.1, *a).total_cmp(&ang_dist(y.1, *a))).unwrap().0;
                    b.link(*g, near);
                }
            } else {
                // Spokes from the previous ring's stars.
                for (pid, pa) in prev_ring.iter().enumerate().filter(|(i, _)| i % 6 == 0).map(|(_, x)| *x) {
                    let near = ring.iter().min_by(|x, y| ang_dist(x.1, pa).total_cmp(&ang_dist(y.1, pa))).unwrap().0;
                    b.link(pid, near);
                }
            }
            prev_ring = ring;
        }
        Ok(b.tree)
    }

    /// Allocated nodes count as reached; the start always is.
    fn reached(&self, alloc: &BTreeSet<u32>, id: u32) -> bool {
        id == start_id() || alloc.contains(&id)
    }

    pub fn can_allocate(&self, alloc: &BTreeSet<u32>, id: u32) -> bool {
        let Some(n) = self.node(id) else { return false };
        n.kind != NodeKind::Start && !alloc.contains(&id) && n.links.iter().any(|l| self.reached(alloc, *l))
    }

    /// Can this node be taken back without cutting anything off from the start?
    pub fn can_refund(&self, alloc: &BTreeSet<u32>, id: u32) -> bool {
        if !alloc.contains(&id) {
            return false;
        }
        let mut rest = alloc.clone();
        rest.remove(&id);
        let mut seen = BTreeSet::new();
        let mut q = VecDeque::from([start_id()]);
        while let Some(x) = q.pop_front() {
            let Some(n) = self.node(x) else { continue };
            for l in &n.links {
                if rest.contains(l) && seen.insert(*l) {
                    q.push_back(*l);
                }
            }
        }
        seen.len() == rest.len()
    }

    /// Shortest run of unallocated nodes from what is allocated to `target` (inclusive).
    pub fn path_to(&self, alloc: &BTreeSet<u32>, target: u32) -> Option<Vec<u32>> {
        self.node(target)?;
        if alloc.contains(&target) {
            return Some(Vec::new());
        }
        let mut prev: BTreeMap<u32, u32> = BTreeMap::new();
        let mut q: VecDeque<u32> = alloc.iter().copied().chain([start_id()]).collect();
        let mut seen: BTreeSet<u32> = q.iter().copied().collect();
        while let Some(x) = q.pop_front() {
            if x == target {
                let mut path = vec![x];
                let mut c = x;
                while let Some(p) = prev.get(&c) {
                    if self.reached(alloc, *p) {
                        break;
                    }
                    path.push(*p);
                    c = *p;
                }
                path.reverse();
                return Some(path);
            }
            for l in &self.node(x)?.links {
                if seen.insert(*l) {
                    prev.insert(*l, x);
                    q.push_back(*l);
                }
            }
        }
        None
    }

    /// What the allocated nodes add up to.
    pub fn bonus(&self, alloc: &BTreeSet<u32>, masteries: &BTreeMap<u32, u8>) -> TreeBonus {
        let mut out = TreeBonus::default();
        for id in alloc {
            let Some(n) = self.node(*id) else { continue };
            for (s, v) in &n.stats {
                out.mods.add(*s, *v);
            }
            if let Some(p) = n.power {
                out.powers.push(p);
            }
            out.tweaks.extend(n.tweaks.iter().cloned());
            if n.kind == NodeKind::Mastery {
                if let Some((_, stats, power)) =
                    masteries.get(id).and_then(|o| self.masteries.get(&n.mastery).and_then(|m| m.options.get(*o as usize)))
                {
                    for (s, v) in stats {
                        out.mods.add(*s, *v);
                    }
                    if let Some(p) = power {
                        out.powers.push(*p);
                    }
                }
            }
        }
        out
    }

    /// Lines describing a node (stats, power, tweaks).
    pub fn describe(&self, n: &Node) -> Vec<String> {
        let d = super::data::data();
        let mut v: Vec<String> = n.stats.iter().map(|(s, x)| super::stats::describe(*s, *x)).collect();
        if let Some(p) = &n.power {
            v.push(p.describe());
        }
        for t in &n.tweaks {
            let name = d.skill_id(&t.skill).map(|i| d.skill(i).name.clone()).unwrap_or_else(|| t.skill.clone());
            v.push(t.describe(&name));
        }
        v
    }
}

/// What a set of allocated nodes gives.
#[derive(Clone, Debug, Default)]
pub struct TreeBonus {
    pub mods: Mods,
    pub powers: Vec<Power>,
    pub tweaks: Vec<Tweak>,
}

fn ang_dist(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// Pushes overlapping nodes apart (roads and the start stay put), pulls stretched links in.
fn relax(b: &mut Builder, min: f32, iters: u32) {
    let n = b.tree.nodes.len();
    let fixed: Vec<bool> = b.tree.nodes.iter().map(|x| b.fixed.contains(&x.id)).collect();
    let links: Vec<(usize, usize)> = b
        .tree
        .nodes
        .iter()
        .enumerate()
        .flat_map(|(i, x)| x.links.iter().map(move |l| (i, *l)))
        .filter_map(|(i, l)| b.tree.index.get(&l).map(|j| (i, *j)))
        .filter(|(i, j)| i < j)
        .collect();
    for _ in 0..iters {
        let mut moved = false;
        let mut push = vec![Vec2::ZERO; n];
        for i in 0..n {
            for j in (i + 1)..n {
                let d = b.tree.nodes[j].pos - b.tree.nodes[i].pos;
                let len = d.length();
                if len < min {
                    let dir = if len > 1e-4 { d / len } else { Vec2::new(((i * 7 + j) % 5) as f32 - 2.0, 1.0).normalize() };
                    let amount = (min - len) * 0.5;
                    push[i] -= dir * amount;
                    push[j] += dir * amount;
                    moved = true;
                }
            }
        }
        for (i, j) in &links {
            let d = b.tree.nodes[*j].pos - b.tree.nodes[*i].pos;
            let len = d.length();
            let max = min * 1.9;
            if len > max {
                let dir = d / len;
                push[*i] += dir * (len - max) * 0.25;
                push[*j] -= dir * (len - max) * 0.25;
            }
        }
        for (i, p) in push.into_iter().enumerate() {
            if !fixed[i] {
                b.tree.nodes[i].pos += p * 0.5;
            }
        }
        if !moved {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> std::sync::Arc<super::super::data::Data> {
        super::super::data::data()
    }

    #[test]
    fn the_tree_is_big_connected_and_spread_out() {
        let d = tree();
        let t = &d.tree;
        let main = t.nodes.iter().filter(|n| n.ring == 0).count();
        assert!(main > 250, "main tree nodes: {main}");
        assert!(t.nodes.len() > 2000, "with the astral rings: {}", t.nodes.len());
        assert_eq!(t.nodes.iter().filter(|n| n.kind == NodeKind::Keystone).count(), 12);
        assert_eq!(t.nodes.iter().filter(|n| n.kind == NodeKind::Skill).count(), 48);
        // Everything reachable from the start.
        let mut seen = BTreeSet::from([start_id()]);
        let mut q = VecDeque::from([start_id()]);
        while let Some(x) = q.pop_front() {
            for l in &t.node(x).unwrap().links {
                if seen.insert(*l) {
                    q.push_back(*l);
                }
            }
        }
        assert_eq!(seen.len(), t.nodes.len());
        // No two main-tree nodes on top of each other.
        let mains: Vec<&Node> = t.nodes.iter().filter(|n| n.ring == 0).collect();
        for (i, a) in mains.iter().enumerate() {
            for b in &mains[i + 1..] {
                assert!((a.pos - b.pos).length() > 0.6, "{} and {} overlap", a.key, b.key);
            }
        }
    }

    #[test]
    fn allocation_rules() {
        let d = tree();
        let t = &d.tree;
        let mut alloc = BTreeSet::new();
        let first = node_id("might.road.0");
        let second = node_id("might.road.1");
        assert!(!t.can_allocate(&alloc, second), "not adjacent yet");
        assert!(t.can_allocate(&alloc, first));
        alloc.insert(first);
        alloc.insert(second);
        assert!(!t.can_refund(&alloc, first), "would cut road.2 off");
        assert!(t.can_refund(&alloc, second));
        let key = node_id("might.keystone");
        let path = t.path_to(&alloc, key).unwrap();
        assert_eq!(path.last(), Some(&key));
        assert!(path.len() >= 6, "{path:?}");
        for p in &path {
            alloc.insert(*p);
        }
        let b = t.bonus(&alloc, &BTreeMap::new());
        assert!(b.powers.iter().any(|p| p.kind == super::super::powers::PowerKind::StandFirm));
        assert!(b.mods.get(Stat::Strength) > 0.0 || b.mods.get(Stat::Life) > 0.0);
    }
}
