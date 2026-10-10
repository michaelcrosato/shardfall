//! Navigation: a walkable grid over the ground, built from what is actually there (floor
//! blocks to stand on, walls, furniture and fixed props in the way, grown by a walker's
//! radius), with A* paths (smoothed by line of sight) and flow fields (every cell's distance
//! to a goal, so a whole pack can follow one field downhill). The game's bot plans its way
//! through levels with paths; monsters chase the hero around walls with a flow field. Both are
//! pure functions of the grid and the goal, so replays and rewind stay exact.

use std::collections::BinaryHeap;

use glam::{Vec2, Vec3};

use crate::entity::BodyKind;
use crate::sim::Sim;
use crate::statics::block_flags;

/// Walkable cells on a regular grid.
#[derive(Clone, Debug, PartialEq)]
pub struct NavGrid {
    pub origin: Vec2,
    pub cell: f32,
    pub w: usize,
    pub h: usize,
    /// true = can't stand there (wall, furniture, no floor).
    pub blocked: Vec<bool>,
}

const DIRS: [(i32, i32, f32); 8] = [
    (1, 0, 1.0),
    (-1, 0, 1.0),
    (0, 1, 1.0),
    (0, -1, 1.0),
    (1, 1, std::f32::consts::SQRT_2),
    (1, -1, std::f32::consts::SQRT_2),
    (-1, 1, std::f32::consts::SQRT_2),
    (-1, -1, std::f32::consts::SQRT_2),
];

impl NavGrid {
    /// Builds the grid over `min..max` (ground plane): floor is any block whose top is near
    /// height 0; obstacles are solid blocks and fixed props between knee and head height,
    /// grown by `radius`.
    /// `skip` leaves out entities that come and go (the game's kegs and totems), so the grid
    /// is the same whenever it is built; crumbled tiles count as floor for the same reason.
    pub fn build(
        sim: &Sim,
        min: Vec2,
        max: Vec2,
        cell: f32,
        radius: f32,
        skip: &dyn Fn(crate::entity::EntityId) -> bool,
    ) -> NavGrid {
        let w = ((max.x - min.x) / cell).ceil().max(1.0) as usize;
        let h = ((max.y - min.y) / cell).ceil().max(1.0) as usize;
        let mut floor = vec![false; w * h];
        let mut solid = vec![false; w * h];
        let cells = |lo: Vec2, hi: Vec2| {
            let x0 = (((lo.x - min.x) / cell).floor().max(0.0) as usize).min(w);
            let x1 = (((hi.x - min.x) / cell).ceil().max(0.0) as usize).min(w);
            let y0 = (((lo.y - min.y) / cell).floor().max(0.0) as usize).min(h);
            let y1 = (((hi.y - min.y) / cell).ceil().max(0.0) as usize).min(h);
            (x0, x1, y0, y1)
        };
        let mark = |grid: &mut Vec<bool>, lo: Vec2, hi: Vec2, centers_only: bool| {
            let (x0, x1, y0, y1) = cells(lo, hi);
            for y in y0..y1 {
                for x in x0..x1 {
                    let c = min + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * cell;
                    if !centers_only || (c.cmpge(lo).all() && c.cmple(hi).all()) {
                        grid[y * w + x] = true;
                    }
                }
            }
        };
        for chunk in sim.state.statics.chunks.values() {
            for b in &chunk.blocks {
                if b.flags & block_flags::GHOST != 0 {
                    continue;
                }
                let (lo, hi) = (Vec2::new(b.min.x, b.min.z), Vec2::new(b.max.x, b.max.z));
                if b.max.y > -0.15 && b.max.y < 0.15 {
                    // Floor: the cell's centre must be on it.
                    mark(&mut floor, lo, hi, true);
                } else if b.max.y >= 0.3 && b.min.y < 1.6 {
                    mark(&mut solid, lo - Vec2::splat(radius), hi + Vec2::splat(radius), true);
                }
            }
        }
        for e in sim.state.entities.iter() {
            if e.body_kind != BodyKind::Fixed || e.character.is_some() || skip(e.id) {
                continue;
            }
            if let Some(prop) = &e.prop {
                if prop.collide {
                    for part in prop.definition.parts.values().filter(|p| p.solid) {
                        let b = crate::prop_instance::transform_bounds(part.bounds(prop.scale), e.pos, e.rot);
                        if b.min.y <= 1.6 && b.max.y >= 0.3 {
                            mark(
                                &mut solid,
                                Vec2::new(b.min.x, b.min.z) - Vec2::splat(radius),
                                Vec2::new(b.max.x, b.max.z) + Vec2::splat(radius),
                                true,
                            );
                        }
                    }
                }
                continue;
            }
            let Some(v) = &e.visual else { continue };
            let he = v.shape.half_extents();
            if e.pos.y - he.y > 1.6 || e.pos.y + he.y < 0.3 {
                continue;
            }
            let r = he.x.max(he.z) + radius;
            mark(&mut solid, Vec2::new(e.pos.x - r, e.pos.z - r), Vec2::new(e.pos.x + r, e.pos.z + r), true);
        }
        let blocked = floor.iter().zip(&solid).map(|(f, s)| !f || *s).collect();
        NavGrid { origin: min, cell, w, h, blocked }
    }

    pub fn cell_of(&self, p: Vec3) -> Option<usize> {
        let x = ((p.x - self.origin.x) / self.cell).floor();
        let y = ((p.z - self.origin.y) / self.cell).floor();
        if x < 0.0 || y < 0.0 || x >= self.w as f32 || y >= self.h as f32 {
            return None;
        }
        Some(y as usize * self.w + x as usize)
    }

    pub fn center(&self, i: usize) -> Vec3 {
        let (x, y) = (i % self.w, i / self.w);
        let c = self.origin + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * self.cell;
        Vec3::new(c.x, 0.0, c.y)
    }

    pub fn walkable(&self, p: Vec3) -> bool {
        self.cell_of(p).is_some_and(|i| !self.blocked[i])
    }

    /// The nearest walkable cell to `p` (within a few cells), for starts and goals that sit
    /// on an obstacle's margin.
    pub fn nearest_open(&self, p: Vec3) -> Option<usize> {
        let i = self.cell_of(p)?;
        if !self.blocked[i] {
            return Some(i);
        }
        let (cx, cy) = ((i % self.w) as i32, (i / self.w) as i32);
        for r in 1..6i32 {
            let mut best: Option<(f32, usize)> = None;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let Some(j) = self.at(cx + dx, cy + dy) else { continue };
                    if !self.blocked[j] {
                        let d = (self.center(j) - Vec3::new(p.x, 0.0, p.z)).length();
                        if best.is_none_or(|b| d < b.0) {
                            best = Some((d, j));
                        }
                    }
                }
            }
            if let Some((_, j)) = best {
                return Some(j);
            }
        }
        None
    }

    fn at(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h).then(|| y as usize * self.w + x as usize)
    }

    /// Neighbours of a cell with step costs (no cutting corners past obstacles).
    fn neighbours(&self, i: usize) -> impl Iterator<Item = (usize, f32)> + '_ {
        let (x, y) = ((i % self.w) as i32, (i / self.w) as i32);
        DIRS.iter().filter_map(move |(dx, dy, c)| {
            let j = self.at(x + dx, y + dy)?;
            if self.blocked[j] {
                return None;
            }
            if *dx != 0
                && *dy != 0
                && (self.at(x + dx, y).is_none_or(|k| self.blocked[k]) || self.at(x, y + dy).is_none_or(|k| self.blocked[k]))
            {
                return None;
            }
            Some((j, *c))
        })
    }

    /// Whether a straight walk from `a` to `b` stays on open cells.
    pub fn clear(&self, a: Vec3, b: Vec3) -> bool {
        let d = Vec3::new(b.x - a.x, 0.0, b.z - a.z);
        let n = (d.length() / (self.cell * 0.5)).ceil().max(1.0) as usize;
        (0..=n).all(|k| self.walkable(a + d * (k as f32 / n as f32)))
    }

    /// A* from `from` to `to`: waypoints (smoothed by line of sight), ending at `to`'s cell.
    pub fn path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let s = self.nearest_open(from)?;
        let g = self.nearest_open(to)?;
        let goal = self.center(g);
        let hcost = |i: usize| (self.center(i) - goal).length() / self.cell;
        let mut cost = vec![f32::INFINITY; self.blocked.len()];
        let mut prev = vec![usize::MAX; self.blocked.len()];
        let mut open = BinaryHeap::new();
        cost[s] = 0.0;
        open.push(Node { f: hcost(s), i: s });
        while let Some(Node { i, .. }) = open.pop() {
            if i == g {
                break;
            }
            for (j, c) in self.neighbours(i) {
                let nc = cost[i] + c;
                if nc < cost[j] {
                    cost[j] = nc;
                    prev[j] = i;
                    open.push(Node { f: nc + hcost(j), i: j });
                }
            }
        }
        if !cost[g].is_finite() {
            return None;
        }
        let mut cells = vec![g];
        let mut at = g;
        while prev[at] != usize::MAX {
            at = prev[at];
            cells.push(at);
        }
        cells.reverse();
        // String-pulling: skip every waypoint that can be walked past in a straight line.
        let pts: Vec<Vec3> = cells.iter().map(|i| self.center(*i)).collect();
        let mut out = Vec::new();
        let mut anchor = Vec3::new(from.x, 0.0, from.z);
        let mut k = 0;
        while k + 1 < pts.len() {
            let mut far = k + 1;
            while far + 1 < pts.len() && self.clear(anchor, pts[far + 1]) {
                far += 1;
            }
            out.push(pts[far]);
            anchor = pts[far];
            k = far;
        }
        if out.is_empty() {
            out.push(goal);
        }
        Some(out)
    }

    /// Distances (in metres) from every reachable cell within `reach` metres to `goal`.
    pub fn flow(&self, goal: Vec3, reach: f32) -> Option<FlowField> {
        let g = self.nearest_open(goal)?;
        let mut dist = vec![f32::INFINITY; self.blocked.len()];
        let mut open = BinaryHeap::new();
        dist[g] = 0.0;
        open.push(Node { f: 0.0, i: g });
        let limit = reach / self.cell;
        while let Some(Node { f, i }) = open.pop() {
            if f > dist[i] || f > limit {
                continue;
            }
            for (j, c) in self.neighbours(i) {
                let nd = dist[i] + c;
                if nd < dist[j] {
                    dist[j] = nd;
                    open.push(Node { f: nd, i: j });
                }
            }
        }
        Some(FlowField { goal: g, dist })
    }
}

/// Every cell's distance to one goal: walk downhill to get there.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowField {
    pub goal: usize,
    pub dist: Vec<f32>,
}

impl FlowField {
    /// Which way to go from `p` (None if `p` can't reach the goal): toward the lowest cell a
    /// few steps downhill, so the way bends smoothly round corners.
    pub fn dir(&self, grid: &NavGrid, p: Vec3) -> Option<Vec3> {
        let mut i = grid.nearest_open(p)?;
        if !self.dist[i].is_finite() {
            return None;
        }
        for _ in 0..3 {
            let next = grid.neighbours(i).min_by(|a, b| self.dist[a.0].total_cmp(&self.dist[b.0]))?;
            if self.dist[next.0] >= self.dist[i] {
                break;
            }
            i = next.0;
        }
        let d = grid.center(i) - Vec3::new(p.x, 0.0, p.z);
        (d.length() > 1e-3).then(|| d.normalize())
    }

    /// Path length (m) from `p` to the goal, if reachable.
    pub fn distance(&self, grid: &NavGrid, p: Vec3) -> Option<f32> {
        let i = grid.cell_of(p)?;
        self.dist[i].is_finite().then(|| self.dist[i] * grid.cell)
    }
}

#[derive(PartialEq)]
struct Node {
    f: f32,
    i: usize,
}

impl Eq for Node {}

impl Ord for Node {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        // Min-heap on f; ties by index so the order never depends on anything else.
        o.f.total_cmp(&self.f).then(o.i.cmp(&self.i))
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::statics::Block;

    #[test]
    fn paths_go_round_walls_and_flow_points_the_way() {
        let mut sim = Sim::new("empty", 1).unwrap();
        let st = &mut sim.state;
        // A wall across x = 0 from z = -6 to 6: the only ways round are past its ends.
        st.statics.add(&mut st.physics, Block::new(Vec3::new(-0.5, 0.0, -6.0), Vec3::new(0.5, 2.0, 6.0), Color::hex("#808080")));
        let grid = NavGrid::build(&sim, Vec2::splat(-15.0), Vec2::splat(15.0), 0.5, 0.4, &|_| false);
        let from = Vec3::new(-5.0, 0.0, 0.0);
        let to = Vec3::new(5.0, 0.0, 0.0);
        assert!(!grid.clear(from, to));
        let p = grid.path(from, to).expect("a way round");
        assert!(p.iter().any(|q| q.z.abs() > 6.0), "goes past an end of the wall: {p:?}");
        let mut at = from;
        for q in &p {
            assert!(grid.clear(at, *q), "each leg is walkable");
            at = *q;
        }
        let f = grid.flow(to, 40.0).unwrap();
        let d = f.dir(&grid, from).unwrap();
        assert!(d.z.abs() > 0.3, "flow heads for an end of the wall, not into it: {d}");
        assert!(f.distance(&grid, from).unwrap() > 12.0);
    }
}
