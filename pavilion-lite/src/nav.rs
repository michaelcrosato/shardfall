//! Grid pathfinding for walkers: which cells of a level a character fits in, and A* paths
//! between them (smoothed so they don't zig-zag). Build it once in `setup` and keep it in the
//! game struct; it only reads the level, so it never goes stale unless walls move.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use glam::Vec3;

use crate::world::World;

#[derive(Clone, Debug)]
pub struct NavGrid {
    /// World position of cell (0, 0)'s corner; `y` is the floor height the grid was built at.
    pub origin: Vec3,
    pub cell: f32,
    pub cols: usize,
    pub rows: usize,
    pub walk: Vec<bool>,
}

impl NavGrid {
    /// Samples the box between the corners `min` and `max` (x and z; `min.y` is the floor
    /// height) in square cells of `cell` metres. A cell is walkable when there is floor under
    /// its centre and nothing solid within `radius` (use the walker's capsule radius, e.g. 0.32)
    /// at body height. Static and kinematic entities count as solid; props and characters
    /// don't, so it can be built at any time.
    pub fn build(w: &World, min: Vec3, max: Vec3, cell: f32, radius: f32) -> NavGrid {
        let cell = cell.max(0.1);
        let cols = ((max.x - min.x) / cell).ceil().max(1.0) as usize;
        let rows = ((max.z - min.z) / cell).ceil().max(1.0) as usize;
        let mut walk = vec![false; cols * rows];
        for r in 0..rows {
            for c in 0..cols {
                let x = min.x + (c as f32 + 0.5) * cell;
                let z = min.z + (r as f32 + 0.5) * cell;
                let floor = w.ground_at(x, z, min.y + 0.6).is_some_and(|y| (y - min.y).abs() < 0.45);
                let body = Vec3::new(x, min.y + 0.95, z);
                let blocked = w.solid_box(body, Vec3::new(radius, 0.6, radius)).is_some();
                walk[r * cols + c] = floor && !blocked;
            }
        }
        NavGrid { origin: min, cell, cols, rows, walk }
    }

    fn index(&self, p: Vec3) -> Option<(usize, usize)> {
        let (c, r) = ((p.x - self.origin.x) / self.cell, (p.z - self.origin.z) / self.cell);
        (c >= 0.0 && r >= 0.0 && (c as usize) < self.cols && (r as usize) < self.rows).then_some((c as usize, r as usize))
    }

    fn center(&self, c: usize, r: usize) -> Vec3 {
        Vec3::new(self.origin.x + (c as f32 + 0.5) * self.cell, self.origin.y, self.origin.z + (r as f32 + 0.5) * self.cell)
    }

    pub fn walkable(&self, p: Vec3) -> bool {
        self.index(p).is_some_and(|(c, r)| self.walk[r * self.cols + c])
    }

    /// True if a straight walk from `a` to `b` stays on walkable cells.
    pub fn clear(&self, a: Vec3, b: Vec3) -> bool {
        let d = Vec3::new(b.x - a.x, 0.0, b.z - a.z);
        let steps = (d.length() / (self.cell * 0.4)).ceil().max(1.0) as usize;
        (0..=steps).all(|i| self.walkable(a + d * (i as f32 / steps as f32)))
    }

    /// Waypoints from `from` to `to` (ending at `to`), or None if `to` can't be reached.
    /// Walk to the first waypoint, drop it when within about half a metre, then the next; plan
    /// again every few tenths of a second (not every tick) as the target moves.
    pub fn path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let start = self.index(from)?;
        let goal = self.index(to)?;
        let idx = |(c, r): (usize, usize)| r * self.cols + c;
        if !self.walk[idx(goal)] {
            return None;
        }
        let start = if self.walk[idx(start)] {
            start
        } else {
            // Standing against a wall: start from the nearest walkable cell (up to 2 away).
            let here = Vec3::new(from.x, self.origin.y, from.z);
            let mut best: Option<((usize, usize), f32)> = None;
            for dr in -2i32..=2 {
                for dc in -2i32..=2 {
                    let (c, r) = (start.0 as i32 + dc, start.1 as i32 + dr);
                    if c < 0 || r < 0 || c as usize >= self.cols || r as usize >= self.rows {
                        continue;
                    }
                    let (c, r) = (c as usize, r as usize);
                    let d = self.center(c, r).distance(here);
                    if self.walk[r * self.cols + c] && best.is_none_or(|b| d < b.1) {
                        best = Some(((c, r), d));
                    }
                }
            }
            best?.0
        };
        // A* on 8 neighbours (no corner cutting), costs in 1/10 cells.
        let n = self.cols * self.rows;
        let mut cost = vec![u32::MAX; n];
        let mut came = vec![usize::MAX; n];
        let mut open = BinaryHeap::new();
        let h = |(c, r): (usize, usize)| {
            let (dx, dz) = (c.abs_diff(goal.0) as u32, r.abs_diff(goal.1) as u32);
            10 * dx.max(dz) + 4 * dx.min(dz)
        };
        cost[idx(start)] = 0;
        open.push(Reverse((h(start), idx(start))));
        while let Some(Reverse((_, i))) = open.pop() {
            if i == idx(goal) {
                break;
            }
            let (c, r) = (i % self.cols, i / self.cols);
            for (dc, dr, step) in
                [(1, 0, 10), (-1, 0, 10), (0, 1, 10), (0, -1, 10), (1, 1, 14), (1, -1, 14), (-1, 1, 14), (-1, -1, 14)]
            {
                let (nc, nr) = (c as i32 + dc, r as i32 + dr);
                if nc < 0 || nr < 0 || nc as usize >= self.cols || nr as usize >= self.rows {
                    continue;
                }
                let j = nr as usize * self.cols + nc as usize;
                if !self.walk[j]
                    || (dc != 0
                        && dr != 0
                        && (!self.walk[r * self.cols + nc as usize] || !self.walk[nr as usize * self.cols + c]))
                {
                    continue;
                }
                let g = cost[i] + step;
                if g < cost[j] {
                    cost[j] = g;
                    came[j] = i;
                    open.push(Reverse((g + h((nc as usize, nr as usize)), j)));
                }
            }
        }
        if cost[idx(goal)] == u32::MAX {
            return None;
        }
        let mut cells = vec![idx(goal)];
        while let Some(&last) = cells.last() {
            if last == idx(start) {
                break;
            }
            cells.push(came[last]);
        }
        cells.reverse();
        let pts: Vec<Vec3> = cells.iter().map(|&i| self.center(i % self.cols, i / self.cols)).collect();
        // Smooth: from each kept point, jump to the farthest point still in a straight line.
        let mut out = Vec::new();
        let mut at = from;
        let mut k = 0;
        while k < pts.len() {
            let mut far = k;
            for j in (k..pts.len()).rev() {
                if self.clear(at, pts[j]) {
                    far = j;
                    break;
                }
            }
            at = pts[far];
            out.push(at);
            k = far + 1;
        }
        if let Some(last) = out.last_mut() {
            *last = Vec3::new(to.x, self.origin.y, to.z);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_go_around_walls() {
        let mut w = World::new(1);
        w.load_level(
            r##"
[[layer]]
map = """
#########
#.......#
#.#.#.#.#
#.#.#.#.#
#...#...#
#########
"""
[legend]
"#" = { block = { y0 = 0, y1 = 2 } }
"." = { block = { y0 = -0.5, y1 = 0 } }
"##,
        )
        .unwrap();
        let nav = NavGrid::build(&w, Vec3::ZERO, Vec3::new(9.0, 0.0, 6.0), 0.5, 0.2);
        let (a, b) = (Vec3::new(3.5, 0.0, 3.5), Vec3::new(5.5, 0.0, 3.5));
        assert!(nav.walkable(a) && nav.walkable(b));
        assert!(!nav.clear(a, b), "the wall between them blocks the straight line");
        let path = nav.path(a, b).expect("a way round");
        assert!(path.len() >= 2);
        let mut prev = a;
        for p in &path {
            assert!(nav.clear(prev, *p), "{prev} -> {p}");
            prev = *p;
        }
        assert!(path.last().unwrap().distance(b) < 1e-4);
        assert!(nav.path(a, Vec3::new(4.5, 0.0, 2.5)).is_none(), "inside a wall");
    }

    #[test]
    fn characters_and_props_are_not_holes() {
        let mut w = World::new(1);
        w.load_level(
            r##"
[[layer]]
map = """
#####
#...#
#####
"""
[legend]
"#" = { block = { y0 = 0, y1 = 2 } }
"." = { block = { y0 = -0.5, y1 = 0 } }
"##,
        )
        .unwrap();
        w.spawn(crate::entity::Spawn::character("npc", Vec3::new(2.5, 0.0, 1.5)));
        w.spawn(crate::entity::Spawn::new("crate", Vec3::new(1.5, 0.5, 1.5)).body(crate::entity::Body::Dynamic));
        let nav = NavGrid::build(&w, Vec3::ZERO, Vec3::new(5.0, 0.0, 3.0), 0.5, 0.2);
        assert!(nav.walkable(Vec3::new(2.5, 0.0, 1.5)), "the character's cell");
        assert!(nav.walkable(Vec3::new(1.5, 0.0, 1.5)), "the crate's cell");
        assert!(nav.path(Vec3::new(1.25, 0.0, 1.5), Vec3::new(3.75, 0.0, 1.5)).is_some());
    }
}
