//! Level dressing (asset plan SF-2): every room gets 3-6 library props from the sets its theme
//! names (`game/props.toml`), in the theme's colours through their roles (`$wall`, `$accent`).
//! Dressing runs last, from a random stream of its own. It keeps clear of everything already
//! placed (what `taken` holds, the monsters, the wind, the wall lights) and adds nothing that
//! anything else reads, so the sets can change without moving a mechanic or a monster. A prop
//! without particles is baked into the level's static regions (no entity); one with `fx` stays
//! a placed prop, since particles need an emitter's state.

use std::collections::{BTreeMap, BTreeSet};

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::Builder;
use crate::arpg::levelgen::Rect;
use crate::arpg::mechanics::FeatureKind;
use crate::prop_instance::PropInstance;
use crate::props::{self, Bounds};
use crate::rng::Rng;
use crate::statics::StaticProp;

/// Where a set's props stand in a room.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceMode {
    /// Back to a wall, facing into the room.
    #[default]
    Wall,
    /// Anywhere on the floor, at any turn.
    Floor,
    /// In a corner, facing out of it.
    Corner,
    /// Hung on a wall's inner face (an asset whose back is at z 0, with a `wall_mount` socket).
    WallMount,
}

/// A dressing set (`game/props.toml`); a theme's `props` lists set names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropSet {
    #[serde(default)]
    pub place: PlaceMode,
    pub items: Vec<SetItem>,
}

/// One asset of a set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetItem {
    /// An embedded asset (`LIB/core/x` or `BUILTIN/x`): the browser has nothing else.
    pub asset: String,
    /// How often it is picked against the set's other items.
    #[serde(default = "one")]
    pub w: f32,
    /// Its own place mode, in place of the set's.
    #[serde(default)]
    pub place: Option<PlaceMode>,
    /// A uniform scale, picked between the two.
    #[serde(default = "unit")]
    pub scale: [f32; 2],
    /// Whether its solid parts collide and block paths (false: drawn only, walked through).
    #[serde(default = "yes")]
    pub solid: bool,
}

fn one() -> f32 {
    1.0
}

fn unit() -> [f32; 2] {
    [1.0, 1.0]
}

fn yes() -> bool {
    true
}

/// The `game/props.toml` checks: every item names an embedded asset that compiles, with a
/// positive weight and a sane scale.
pub fn check_sets(sets: &BTreeMap<String, PropSet>) -> Result<(), String> {
    for (key, set) in sets {
        if set.items.is_empty() {
            return Err(format!("game/props.toml [{key}]: a set needs items"));
        }
        for (i, item) in set.items.iter().enumerate() {
            let at = format!("game/props.toml [{key}] items[{i}]");
            let name = props::canonical_placed(&item.asset).map_err(|e| format!("{at}: {e}"))?;
            if !props::store::embedded(props::split_preset(&name).0) {
                return Err(format!("{at}: {name} is not embedded (BUILTIN/ or LIB/core/): the browser build lacks it"));
            }
            props::resolve_placed(&name, &props::Params::new()).map_err(|e| format!("{at}: {e}"))?;
            let [lo, hi] = item.scale;
            if !(item.w > 0.0 && item.w.is_finite()) || !(0.1..=10.0).contains(&lo) || !(lo..=10.0).contains(&hi) {
                return Err(format!("{at}: w must be above 0 and scale [lo, hi] within 0.1..10"));
            }
        }
    }
    Ok(())
}

/// Seeds a prop's variant is picked from (each asset keeps 64 compiled).
const VARIANTS: u32 = 16;
/// Tries for a spot before a prop is skipped.
const TRIES: usize = 12;
/// Walking room left round a standing prop, beyond its footprint.
const CLEAR: f32 = 0.5;
/// How far a wall light reaches out from its wall.
const SCONCE: f32 = 0.5;

impl Builder<'_> {
    /// Dresses every room (see the module notes). `seed` is the level's.
    pub(super) fn dress(&mut self, seed: u64) {
        let d = crate::arpg::data::data();
        let theme = self.plan.theme.clone();
        if theme.props.is_empty() {
            return;
        }
        let mut rng = Rng::new(seed ^ 0xd2e5_5a11);
        let palette = theme.palette();
        let (actors, lights, wind) = self.occupied();
        let mut regions = BTreeSet::new();
        for room in 0..self.layout.rooms.len() {
            let rect = self.layout.rooms[room].rect;
            // What a standing prop keeps clear of: all that is placed, the actors, the others.
            let mut near = self.taken[room].clone();
            near.extend(actors.iter().filter(|(p, _)| rect.contains(*p)));
            let mut hung: Vec<Vec2> = lights.iter().filter(|p| rect.shrink(-1.0).contains(**p)).copied().collect();
            // The wall lights too: a column must not stand through a sconce.
            near.extend(hung.iter().map(|p| (*p, SCONCE)));
            // 3-6 props, from up to three picks each (a crowded room takes fewer).
            let want = 3 + rng.below(4) as usize;
            let mut placed = 0;
            for _ in 0..want * 3 {
                if placed == want {
                    break;
                }
                let Some(set) = d.props.get(&theme.props[rng.below(theme.props.len() as u32) as usize]) else { continue };
                let Some(item) = pick(&set.items, &mut rng) else { continue };
                let place = item.place.unwrap_or(set.place);
                let variant = rng.below(VARIANTS);
                let scale = rng.range(item.scale[0], item.scale[1]);
                let Ok((name, def, params)) = props::resolve_placed(&item.asset, &props::Params::new()) else { continue };
                let parts = def.variant_with(&params, variant);
                let bounds = parts.bounds_at(scale);
                let spot = match place {
                    PlaceMode::WallMount => self.mount_spot(room, &bounds, &mut rng, &hung),
                    _ => self.stand_spot(room, place, &bounds, &mut rng, &near, &wind),
                };
                let Some((p, yaw)) = spot else { continue };
                let (pos, rot) = (Vec3::new(p.x, 0.0, p.y), Quat::from_rotation_y(yaw));
                let solid = item.solid && parts.parts.iter().any(|part| part.solid);
                if parts.fx.is_empty() {
                    let st = &mut self.sim.state;
                    let prop = StaticProp::new(&item.asset, pos, rot, scale, variant, solid);
                    let Ok(key) = st.statics.add_prop(&mut st.physics, prop) else { continue };
                    regions.insert(key);
                } else {
                    let copy = PropInstance::new(&name, def.clone(), scale, solid)
                        .and_then(|c| c.with_seed(variant).with_params(&params))
                        .and_then(|c| c.with_roles(palette.clone()));
                    let Ok(copy) = copy else { continue };
                    if self.sim.place_prop(copy, pos, rot, None).is_err() {
                        continue;
                    }
                }
                match place {
                    PlaceMode::WallMount => hung.push(p),
                    _ => near.push((p, footprint(&bounds))),
                }
                placed += 1;
            }
        }
        let st = &mut self.sim.state;
        for key in regions {
            st.statics.set_palette(key, palette.clone());
        }
    }

    /// What dressing keeps clear of beyond `taken`: the actors (monsters, the boss, kegs,
    /// totems: where they stand and how wide they are), the wall lights, the wind's strips.
    fn occupied(&self) -> (Vec<(Vec2, f32)>, Vec<Vec2>, Vec<Rect>) {
        let st = &self.sim.state;
        let flat = |p: Vec3| Vec2::new(p.x, p.z);
        let mut actors = Vec::new();
        let mut wind = Vec::new();
        if let Some(g) = st.game.as_ref() {
            for (id, a) in &g.actors {
                if let Some(e) = st.entities.get(*id) {
                    actors.push((flat(e.pos), a.radius.max(0.5) + 0.5));
                }
            }
            for f in g.level.iter().flat_map(|l| &l.features) {
                if let FeatureKind::Wind { min, max, .. } = f.kind {
                    wind.push(Rect { min, max });
                }
            }
        }
        let lights = st.entities.iter().filter(|e| e.name == "sconce").map(|e| flat(e.pos)).collect();
        (actors, lights, wind)
    }

    /// A spot for a prop standing on the floor (wall, corner or floor), facing into the room:
    /// its footprint plus `CLEAR` from everything near.
    fn stand_spot(
        &self,
        room: usize,
        place: PlaceMode,
        bounds: &Bounds,
        rng: &mut Rng,
        near: &[(Vec2, f32)],
        wind: &[Rect],
    ) -> Option<(Vec2, f32)> {
        let r = self.layout.rooms[room].rect;
        let foot = footprint(bounds);
        for _ in 0..TRIES {
            let turn = rng.range(-0.3, 0.3);
            let (p, yaw) = match place {
                PlaceMode::Floor => {
                    let s = r.shrink(foot + CLEAR);
                    if s.max.x <= s.min.x || s.max.y <= s.min.y {
                        return None;
                    }
                    (Vec2::new(rng.range(s.min.x, s.max.x), rng.range(s.min.y, s.max.y)), rng.range(0.0, std::f32::consts::TAU))
                }
                PlaceMode::Corner => {
                    let k = rng.below(4);
                    let inward = Vec2::new(if k & 1 == 0 { 1.0 } else { -1.0 }, if k & 2 == 0 { 1.0 } else { -1.0 });
                    let corner =
                        Vec2::new(if k & 1 == 0 { r.min.x } else { r.max.x }, if k & 2 == 0 { r.min.y } else { r.max.y });
                    (corner + inward * (foot + rng.range(0.1, 0.4)), yaw_toward(inward) + turn)
                }
                _ => {
                    let side = rng.below(4);
                    let inset = foot + rng.range(0.05, 0.3);
                    let u = rng.f32();
                    let (p, inward) = along_wall(r, side, foot + 0.3, u)?;
                    (p + inward * inset, yaw_toward(inward) + turn)
                }
            };
            let clear = near.iter().all(|(q, qr)| (p - *q).length() > qr + foot + CLEAR);
            let calm = wind.iter().all(|w| !w.shrink(-foot).contains(p));
            if r.contains(p) && clear && calm {
                return Some((p, yaw));
            }
        }
        None
    }

    /// A spot on a wall's inner face for a hung prop, clear of the doorways, the wall lights
    /// and the other hung props.
    fn mount_spot(&self, room: usize, bounds: &Bounds, rng: &mut Rng, hung: &[Vec2]) -> Option<(Vec2, f32)> {
        let r = self.layout.rooms[room].rect;
        let half = bounds.min.x.abs().max(bounds.max.x.abs());
        let doors = self.layout.doors(room);
        for _ in 0..TRIES {
            // Not the south wall: the game camera looks north, over its back.
            let side = [0, 2, 3][rng.below(3) as usize];
            let u = rng.f32();
            let Some((p, inward)) = along_wall(r, side, half + 0.6, u) else { continue };
            let t = if side.is_multiple_of(2) { p.y } else { p.x };
            let doorway = doors.iter().any(|(s, a, b)| *s == side as usize && t + half + 0.4 > *a && t - half - 0.4 < *b);
            if !doorway && hung.iter().all(|q| (p - *q).length() > half + 1.2) {
                return Some((p, yaw_toward(inward)));
            }
        }
        None
    }
}

/// A point on side `side` of a room's walls (0 east, 1 south, 2 west, 3 north, as the walls
/// are built), `margin` in from its ends, `u` of the way along; and the way into the room.
fn along_wall(r: Rect, side: u32, margin: f32, u: f32) -> Option<(Vec2, Vec2)> {
    let (lo, hi) = if side.is_multiple_of(2) { (r.min.y, r.max.y) } else { (r.min.x, r.max.x) };
    if hi - lo <= 2.0 * margin {
        return None;
    }
    let t = lo + margin + u * (hi - lo - 2.0 * margin);
    Some(match side {
        0 => (Vec2::new(r.max.x, t), Vec2::NEG_X),
        1 => (Vec2::new(t, r.max.y), Vec2::NEG_Y),
        2 => (Vec2::new(r.min.x, t), Vec2::X),
        _ => (Vec2::new(t, r.min.y), Vec2::Y),
    })
}

/// The turn about +y that points an asset's front (+z) along `dir` (x, z).
fn yaw_toward(dir: Vec2) -> f32 {
    dir.x.atan2(dir.y)
}

/// The radius round its origin that a prop covers on the floor at any turn.
fn footprint(b: &Bounds) -> f32 {
    b.min.x.abs().max(b.max.x.abs()).hypot(b.min.z.abs().max(b.max.z.abs()))
}

/// An item by weight.
fn pick<'a>(items: &'a [SetItem], rng: &mut Rng) -> Option<&'a SetItem> {
    let total: f32 = items.iter().map(|i| i.w).sum();
    let mut x = rng.f32() * total;
    for item in items {
        if x < item.w {
            return Some(item);
        }
        x -= item.w;
    }
    items.last()
}
