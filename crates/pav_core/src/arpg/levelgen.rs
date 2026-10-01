//! Level layouts: rooms on a grid joined by corridors. A random walk lays the main path from
//! the start room to the exit, side branches hang off it, every room gets a size and a shape,
//! corridors run straight between neighbouring rooms. Pure data from a seed: the world builder
//! turns it into blocks, the `levelmap` tool draws it.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::rng::Rng;

/// Size of a grid cell (m): one room per cell.
pub const CELL: f32 = 32.0;
pub const CORRIDOR: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect {
    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }
    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }
    pub fn contains(&self, p: Vec2) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }
    pub fn shrink(&self, m: f32) -> Rect {
        Rect { min: self.min + Vec2::splat(m), max: self.max - Vec2::splat(m) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomShape {
    Plain,
    /// Columns in two rows.
    Pillars,
    /// A block in the middle to circle around.
    Ring,
    /// A wall across with two gaps.
    Split,
    /// Blocks in the four corners.
    Cross,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomRole {
    Start,
    Main,
    Side,
    Exit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Room {
    pub cell: (i32, i32),
    pub rect: Rect,
    pub shape: RoomShape,
    pub role: RoomRole,
    /// Steps from the start along the main path (side rooms: their branch point's).
    pub depth: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Corridor {
    pub rect: Rect,
    /// Runs along x (else along z).
    pub along_x: bool,
    pub rooms: (usize, usize),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layout {
    pub rooms: Vec<Room>,
    pub corridors: Vec<Corridor>,
    pub start: usize,
    pub exit: usize,
    /// Room indices from start to exit.
    pub path: Vec<usize>,
}

const DIRS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

/// Snaps to the 4 m floor grid (tiles line up across rooms and corridors).
fn snap(v: f32) -> f32 {
    (v / 4.0).round() * 4.0
}

impl Layout {
    /// Lays out a level: `main` rooms on the path (start and exit included), a few side rooms.
    pub fn generate(seed: u64, main: usize) -> Layout {
        let mut rng = Rng::new(seed ^ 0x1a70);
        let main = main.max(2);
        // Main path: a random walk that prefers going on straight.
        let mut cells: Vec<(i32, i32)> = vec![(0, 0)];
        let mut dir = rng.below(4) as usize;
        let mut tries = 0;
        while cells.len() < main && tries < 200 {
            tries += 1;
            let last = *cells.last().unwrap();
            let mut options: Vec<usize> = (0..4).filter(|d| !cells.contains(&(last.0 + DIRS[*d].0, last.1 + DIRS[*d].1))).collect();
            if options.is_empty() {
                // Dead end: start over from a different direction.
                cells.truncate(1);
                dir = rng.below(4) as usize;
                continue;
            }
            if options.contains(&dir) && rng.f32() < 0.55 {
                options = vec![dir];
            }
            dir = options[rng.below(options.len() as u32) as usize];
            cells.push((last.0 + DIRS[dir].0, last.1 + DIRS[dir].1));
        }
        let n_main = cells.len();
        let mut edges: Vec<(usize, usize)> = (1..n_main).map(|i| (i - 1, i)).collect();
        let mut roles: Vec<RoomRole> = (0..n_main)
            .map(|i| if i == 0 { RoomRole::Start } else if i + 1 == n_main { RoomRole::Exit } else { RoomRole::Main })
            .collect();
        let mut depth: Vec<usize> = (0..n_main).collect();
        // Side branches off the middle of the path.
        let branches = 1 + rng.below(3);
        for _ in 0..branches {
            if n_main < 3 {
                break;
            }
            let from = 1 + rng.below((n_main - 2) as u32) as usize;
            let mut at = from;
            for _ in 0..(1 + rng.below(2)) {
                let c = cells[at];
                let free: Vec<(i32, i32)> =
                    DIRS.iter().map(|d| (c.0 + d.0, c.1 + d.1)).filter(|p| !cells.contains(p)).collect();
                if free.is_empty() {
                    break;
                }
                let p = free[rng.below(free.len() as u32) as usize];
                cells.push(p);
                roles.push(RoomRole::Side);
                depth.push(depth[from]);
                edges.push((at, cells.len() - 1));
                at = cells.len() - 1;
            }
        }
        // Rooms: a size and a shape each, inside their cell.
        let shapes = [RoomShape::Plain, RoomShape::Pillars, RoomShape::Ring, RoomShape::Split, RoomShape::Cross];
        let rooms: Vec<Room> = cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let w = snap(rng.range(16.0, 24.0));
                let h = snap(rng.range(16.0, 24.0));
                let jx = snap(rng.range(-2.0, 2.0));
                let jz = snap(rng.range(-2.0, 2.0));
                let center = Vec2::new(c.0 as f32 * CELL + jx, c.1 as f32 * CELL + jz);
                let rect = Rect { min: center - Vec2::new(w, h) * 0.5, max: center + Vec2::new(w, h) * 0.5 };
                let shape = match roles[i] {
                    RoomRole::Start | RoomRole::Exit => RoomShape::Plain,
                    _ => shapes[rng.below(shapes.len() as u32) as usize],
                };
                Room { cell: *c, rect, shape, role: roles[i], depth: depth[i] }
            })
            .collect();
        // Corridors between neighbours.
        let corridors = edges
            .iter()
            .map(|(a, b)| {
                let (ra, rb) = (&rooms[*a].rect, &rooms[*b].rect);
                let along_x = rooms[*a].cell.1 == rooms[*b].cell.1;
                let rect = if along_x {
                    let (l, r) = if ra.max.x < rb.min.x { (ra, rb) } else { (rb, ra) };
                    let lo = l.min.y.max(r.min.y) + 2.0;
                    let hi = l.max.y.min(r.max.y) - 2.0;
                    let z = snap((lo + hi) * 0.5 - CORRIDOR * 0.5);
                    Rect { min: Vec2::new(l.max.x, z), max: Vec2::new(r.min.x, z + CORRIDOR) }
                } else {
                    let (l, r) = if ra.max.y < rb.min.y { (ra, rb) } else { (rb, ra) };
                    let lo = l.min.x.max(r.min.x) + 2.0;
                    let hi = l.max.x.min(r.max.x) - 2.0;
                    let x = snap((lo + hi) * 0.5 - CORRIDOR * 0.5);
                    Rect { min: Vec2::new(x, l.max.y), max: Vec2::new(x + CORRIDOR, r.min.y) }
                };
                Corridor { rect, along_x, rooms: (*a, *b) }
            })
            .collect();
        Layout { rooms, corridors, start: 0, exit: n_main - 1, path: (0..n_main).collect() }
    }

    /// Bounds of everything (for maps).
    pub fn bounds(&self) -> Rect {
        let mut min = Vec2::splat(f32::MAX);
        let mut max = Vec2::splat(f32::MIN);
        for r in self.rooms.iter().map(|r| r.rect).chain(self.corridors.iter().map(|c| c.rect)) {
            min = min.min(r.min);
            max = max.max(r.max);
        }
        Rect { min, max }
    }

    /// Door gaps on a room's sides: (side 0 +x, 1 +z, 2 -x, 3 -z; from, to along the side).
    pub fn doors(&self, room: usize) -> Vec<(usize, f32, f32)> {
        let r = &self.rooms[room].rect;
        self.corridors
            .iter()
            .filter(|c| c.rooms.0 == room || c.rooms.1 == room)
            .map(|c| {
                if c.along_x {
                    let side = if (c.rect.min.x - r.max.x).abs() < 0.5 { 0 } else { 2 };
                    (side, c.rect.min.y, c.rect.max.y)
                } else {
                    let side = if (c.rect.min.y - r.max.y).abs() < 0.5 { 1 } else { 3 };
                    (side, c.rect.min.x, c.rect.max.x)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_connect_and_never_overlap() {
        for seed in 0..200u64 {
            let l = Layout::generate(seed, 6 + (seed % 7) as usize);
            assert!(l.rooms.len() >= 6, "seed {seed}: {} rooms", l.rooms.len());
            assert_eq!(l.rooms[l.exit].role, RoomRole::Exit);
            for (i, a) in l.rooms.iter().enumerate() {
                for b in &l.rooms[i + 1..] {
                    let overlap = a.rect.min.x < b.rect.max.x && b.rect.min.x < a.rect.max.x && a.rect.min.y < b.rect.max.y && b.rect.min.y < a.rect.max.y;
                    assert!(!overlap, "seed {seed}: rooms overlap");
                }
            }
            for c in &l.corridors {
                let s = c.rect.size();
                assert!(s.x > 0.0 && s.y > 0.0, "seed {seed}: corridor {:?}", c.rect);
                assert!((if c.along_x { s.y } else { s.x } - CORRIDOR).abs() < 0.01);
            }
            // Every room is reachable from the start.
            let mut seen = vec![false; l.rooms.len()];
            seen[0] = true;
            for _ in 0..l.rooms.len() {
                for c in &l.corridors {
                    if seen[c.rooms.0] || seen[c.rooms.1] {
                        seen[c.rooms.0] = true;
                        seen[c.rooms.1] = true;
                    }
                }
            }
            assert!(seen.iter().all(|s| *s), "seed {seed}");
        }
    }
}
