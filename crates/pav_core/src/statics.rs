//! Static geometry, grouped into regions: terrain chunks, rooms and the pavilion hub. A region
//! is the unit of streaming: it can be active (colliders in the physics world) or dormant
//! (kept in memory with all changes, colliders removed).

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{IVec2, Quat, Vec3};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::physics::{PhysicsState, TAG_BLOCK};
use crate::shape::{Look, Shape};
use crate::terrain::TerrainPatch;
use crate::zones::{Label, Zone};

/// Terrain chunk edge length in metres.
pub const CHUNK_SIZE: f32 = 32.0;

pub mod block_flags {
    /// Can be destroyed by explosions.
    pub const DESTRUCTIBLE: u32 = 1;
    /// Climbable surface (ladders).
    pub const LADDER: u32 = 2;
    /// No collision (decoration).
    pub const GHOST: u32 = 4;
    /// Rendered with rounded edges.
    pub const ROUNDED: u32 = 8;
    /// About to crumble (drawn cracked).
    pub const CRACKED: u32 = 16;
    /// Crumbles only under the player (monsters fall into holes but don't make them).
    pub const PLAYER_CRUMBLE: u32 = 32;
}

/// Which region static content belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RegionKey {
    /// Procedural terrain chunk at chunk coordinates.
    Chunk(i32, i32),
    /// A showcase room (index into the world's room list).
    Room(u16),
    /// The pavilion hub: plaza and corridors.
    Hub,
}

impl RegionKey {
    pub fn chunk_of(p: Vec3) -> Self {
        RegionKey::Chunk((p.x / CHUNK_SIZE).floor() as i32, (p.z / CHUNK_SIZE).floor() as i32)
    }
    fn pack(self) -> (u128, u128, u128) {
        match self {
            RegionKey::Chunk(x, z) => (0, x as u32 as u128, z as u32 as u128),
            RegionKey::Room(id) => (1, id as u128, 0),
            RegionKey::Hub => (2, 0, 0),
        }
    }
    fn unpack(kind: u128, a: u128, b: u128) -> Option<Self> {
        match kind {
            0 => Some(RegionKey::Chunk(a as u32 as i32, b as u32 as i32)),
            1 => Some(RegionKey::Room(a as u16)),
            2 => Some(RegionKey::Hub),
            _ => None,
        }
    }
}

/// Compatibility alias: terrain chunk coordinates.
pub type ChunkKey = RegionKey;

pub fn chunk_origin(x: i32, z: i32) -> Vec3 {
    Vec3::new(x as f32 * CHUNK_SIZE, 0.0, z as f32 * CHUNK_SIZE)
}

pub fn chunk_ivec(p: Vec3) -> IVec2 {
    IVec2::new((p.x / CHUNK_SIZE).floor() as i32, (p.z / CHUNK_SIZE).floor() as i32)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Block {
    pub min: Vec3,
    pub max: Vec3,
    pub color: Color,
    #[serde(default)]
    pub look: Look,
    #[serde(default)]
    pub flags: u32,
    #[serde(default)]
    pub collider: Option<ColliderHandle>,
    #[serde(default = "yes")]
    pub alive: bool,
    #[serde(default)]
    pub crumble: f32,
    #[serde(default)]
    pub regrow: f32,
    #[serde(default)]
    pub strength: f32,
}

fn yes() -> bool {
    true
}

impl Block {
    pub fn new(min: Vec3, max: Vec3, color: Color) -> Self {
        Self {
            min: min.min(max),
            max: max.max(min),
            color,
            look: Look::Cel,
            flags: 0,
            collider: None,
            alive: true,
            crumble: 0.0,
            regrow: 0.0,
            strength: 0.0,
        }
    }
    pub fn with_flags(mut self, flags: u32) -> Self {
        self.flags |= flags;
        self
    }
    pub fn with_look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    pub fn half(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }
    pub fn has(&self, f: u32) -> bool {
        self.flags & f != 0
    }
}

/// Compass direction (north = -Z).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Facing {
    #[default]
    North,
    East,
    South,
    West,
}

impl Facing {
    /// Unit vector pointing in this direction on the ground plane.
    pub fn dir(self) -> Vec3 {
        match self {
            Facing::North => Vec3::NEG_Z,
            Facing::East => Vec3::X,
            Facing::South => Vec3::Z,
            Facing::West => Vec3::NEG_X,
        }
    }
    /// Rotated clockwise (seen from above) by `quarters` quarter turns.
    pub fn rotated(self, quarters: u8) -> Facing {
        const ORDER: [Facing; 4] = [Facing::North, Facing::East, Facing::South, Facing::West];
        let i = ORDER.iter().position(|f| *f == self).unwrap_or(0);
        ORDER[(i + quarters as usize) % 4]
    }
    pub fn opposite(self) -> Facing {
        self.rotated(2)
    }
}

/// A climbable zone. `facing` points from the climber toward the wall.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ladder {
    pub min: Vec3,
    pub max: Vec3,
    pub facing: Facing,
    #[serde(default)]
    pub color: Color,
}

impl Ladder {
    pub fn top(&self) -> f32 {
        self.max.y
    }
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    /// True when a vertical capsule (feet position, radius, height) overlaps the zone.
    pub fn overlaps(&self, feet: Vec3, radius: f32, height: f32) -> bool {
        let r = radius * 0.9;
        feet.x + r > self.min.x
            && feet.x - r < self.max.x
            && feet.z + r > self.min.z
            && feet.z - r < self.max.z
            && feet.y + height > self.min.y
            && feet.y < self.max.y + 0.05
    }
}

/// A static decorative shape (trees, rocks, lamps), optionally solid.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Decor {
    pub shape: Shape,
    pub pos: Vec3,
    #[serde(default = "quat_id")]
    pub rot: Quat,
    pub color: Color,
    #[serde(default)]
    pub look: Look,
    #[serde(default)]
    pub emissive: f32,
    #[serde(default)]
    pub solid: bool,
    #[serde(default)]
    pub collider: Option<ColliderHandle>,
}

fn quat_id() -> Quat {
    Quat::IDENTITY
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StaticChunk {
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub ladders: Vec<Ladder>,
    #[serde(default)]
    pub decor: Vec<Decor>,
    #[serde(default)]
    pub terrain: Option<Arc<TerrainPatch>>,
    #[serde(default)]
    pub terrain_collider: Option<ColliderHandle>,
    /// Trigger zones (courses, checkpoints, pits, water, pads, camera cues).
    #[serde(default)]
    pub zones: Vec<Zone>,
    /// Text in the world.
    #[serde(default)]
    pub labels: Vec<Label>,
    /// Bounding box of everything in the region (for spatial queries).
    pub min: Vec3,
    pub max: Vec3,
    /// Bumped on every change; the renderer caches per (region, version).
    pub version: u64,
    /// Changed since generation (terrain chunks that are unmodified can be dropped and
    /// regenerated from the seed instead of being kept).
    #[serde(default)]
    pub modified: bool,
}

impl StaticChunk {
    fn grow(&mut self, min: Vec3, max: Vec3) {
        if self.blocks.is_empty()
            && self.decor.is_empty()
            && self.ladders.is_empty()
            && self.terrain.is_none()
            && self.zones.is_empty()
            && self.labels.is_empty()
        {
            self.min = min;
            self.max = max;
        } else {
            self.min = self.min.min(min);
            self.max = self.max.max(max);
        }
    }
    pub fn intersects(&self, min: Vec3, max: Vec3) -> bool {
        self.min.x <= max.x
            && self.max.x >= min.x
            && self.min.y <= max.y
            && self.max.y >= min.y
            && self.min.z <= max.z
            && self.max.z >= min.z
    }
}

/// Where a zone lives: region + index inside the region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ZoneRef {
    pub region: RegionKey,
    pub index: u32,
}

/// Where a block lives: region + index inside the region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockRef {
    pub region: RegionKey,
    pub index: u32,
}

impl BlockRef {
    /// Collider user data: tag (16 bits) | region kind (8) | a (32) | b (32) | index (32).
    pub fn tag(self) -> u128 {
        let (k, a, b) = self.region.pack();
        TAG_BLOCK | k << 104 | a << 72 | b << 40 | self.index as u128
    }
    pub fn from_tag(tag: u128) -> Option<Self> {
        if tag & crate::physics::TAG_MASK != TAG_BLOCK {
            return None;
        }
        let region = RegionKey::unpack((tag >> 104) & 0xFF, (tag >> 72) & 0xFFFF_FFFF, (tag >> 40) & 0xFFFF_FFFF)?;
        Some(BlockRef { region, index: tag as u32 })
    }
}

fn block_collider(b: &Block, tag: u128) -> ColliderBuilder {
    let h = b.half();
    let c = b.center();
    let mut cb = ColliderBuilder::cuboid(h.x as Real, h.y as Real, h.z as Real)
        .translation(Vector::new(c.x as Real, c.y as Real, c.z as Real))
        .friction(0.8)
        .user_data(tag);
    if b.strength > 0.0 {
        // Breakable: report hard impacts.
        cb = cb.active_events(ActiveEvents::CONTACT_FORCE_EVENTS).contact_force_event_threshold(b.strength as Real);
    }
    cb
}

fn decor_collider(d: &Decor) -> ColliderBuilder {
    d.shape.collider().position(Pose::from_parts(d.pos, d.rot)).friction(0.8)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StaticWorld {
    /// Active regions (colliders present).
    pub chunks: BTreeMap<RegionKey, Arc<StaticChunk>>,
    /// Dormant regions (no colliders), kept with their changes.
    #[serde(default)]
    pub dormant: BTreeMap<RegionKey, Arc<StaticChunk>>,
}

impl StaticWorld {
    fn chunk_mut(&mut self, key: RegionKey) -> &mut StaticChunk {
        Arc::make_mut(self.chunks.entry(key).or_default())
    }

    /// Adds a block to an active region (creating its collider) and returns where it went.
    pub fn add_to(&mut self, physics: &mut PhysicsState, key: RegionKey, mut block: Block) -> BlockRef {
        let chunk = self.chunk_mut(key);
        let index = chunk.blocks.len() as u32;
        let r = BlockRef { region: key, index };
        if !block.has(block_flags::GHOST) {
            block.collider = Some(physics.insert_static(block_collider(&block, r.tag())));
        }
        chunk.grow(block.min, block.max);
        chunk.blocks.push(block);
        chunk.version += 1;
        r
    }

    /// Adds a block to the terrain chunk containing its centre (scenes and tests).
    pub fn add(&mut self, physics: &mut PhysicsState, block: Block) -> BlockRef {
        let key = RegionKey::chunk_of(block.center());
        self.add_to(physics, key, block)
    }

    pub fn add_ladder_to(&mut self, key: RegionKey, ladder: Ladder) {
        let chunk = self.chunk_mut(key);
        chunk.grow(ladder.min, ladder.max);
        chunk.ladders.push(ladder);
        chunk.version += 1;
    }

    pub fn add_ladder(&mut self, ladder: Ladder) {
        let key = RegionKey::chunk_of(ladder.center());
        self.add_ladder_to(key, ladder);
    }

    pub fn add_zone_to(&mut self, key: RegionKey, zone: Zone) {
        let chunk = self.chunk_mut(key);
        chunk.grow(zone.min, zone.max);
        chunk.zones.push(zone);
        chunk.version += 1;
    }

    pub fn add_label_to(&mut self, key: RegionKey, label: Label) {
        let chunk = self.chunk_mut(key);
        let h = Vec3::splat(label.size);
        chunk.grow(label.pos - h, label.pos + h);
        chunk.labels.push(label);
        chunk.version += 1;
    }

    /// Zones containing `p` (active regions only), with their region and index.
    pub fn zones_at(&self, p: Vec3) -> impl Iterator<Item = (ZoneRef, &Zone)> {
        self.regions_in(p, p).flat_map(move |(k, c)| {
            c.zones
                .iter()
                .enumerate()
                .filter(move |(_, z)| z.contains(p))
                .map(move |(i, z)| (ZoneRef { region: *k, index: i as u32 }, z))
        })
    }

    pub fn zone(&self, r: ZoneRef) -> Option<&Zone> {
        self.chunks.get(&r.region)?.zones.get(r.index as usize)
    }

    /// All zones of a region.
    pub fn region_zones(&self, key: RegionKey) -> &[Zone] {
        self.chunks.get(&key).map(|c| c.zones.as_slice()).unwrap_or(&[])
    }

    pub fn add_decor(&mut self, physics: &mut PhysicsState, key: RegionKey, mut d: Decor) {
        if d.solid {
            d.collider = Some(physics.insert_static(decor_collider(&d)));
        }
        let chunk = self.chunk_mut(key);
        let h = d.shape.half_extents().length();
        chunk.grow(d.pos - Vec3::splat(h), d.pos + Vec3::splat(h));
        chunk.decor.push(d);
        chunk.version += 1;
    }

    pub fn set_terrain(&mut self, physics: &mut PhysicsState, key: RegionKey, patch: Arc<TerrainPatch>) {
        let col = physics.insert_static(patch.collider());
        let chunk = self.chunk_mut(key);
        if let Some(old) = chunk.terrain_collider.replace(col) {
            physics.remove_collider(old);
        }
        chunk.grow(patch.min, patch.max);
        chunk.terrain = Some(patch);
        chunk.version += 1;
    }

    pub fn get(&self, r: BlockRef) -> Option<&Block> {
        self.chunks.get(&r.region)?.blocks.get(r.index as usize)
    }

    /// Brings a destroyed block back (crumbling/breakable tiles regrow).
    pub fn restore(&mut self, physics: &mut PhysicsState, r: BlockRef) -> bool {
        let Some(chunk) = self.chunks.get_mut(&r.region).map(Arc::make_mut) else { return false };
        let Some(b) = chunk.blocks.get_mut(r.index as usize) else { return false };
        if b.alive {
            return false;
        }
        b.alive = true;
        b.flags &= !block_flags::CRACKED;
        if !b.has(block_flags::GHOST) {
            b.collider = Some(physics.insert_static(block_collider(b, r.tag())));
        }
        chunk.version += 1;
        true
    }

    /// Sets or clears flags on a block (e.g. CRACKED); bumps the region version.
    pub fn set_block_flags(&mut self, r: BlockRef, set: u32, clear: u32) {
        if let Some(chunk) = self.chunks.get_mut(&r.region).map(Arc::make_mut) {
            if let Some(b) = chunk.blocks.get_mut(r.index as usize) {
                b.flags = (b.flags | set) & !clear;
                chunk.version += 1;
            }
        }
    }

    /// Removes a block's collider and marks it dead. Returns the block if it was alive.
    pub fn destroy(&mut self, physics: &mut PhysicsState, r: BlockRef) -> Option<Block> {
        let chunk = Arc::make_mut(self.chunks.get_mut(&r.region)?);
        let b = chunk.blocks.get_mut(r.index as usize)?;
        if !b.alive {
            return None;
        }
        b.alive = false;
        if let Some(h) = b.collider.take() {
            physics.remove_collider(h);
        }
        chunk.version += 1;
        chunk.modified = true;
        Some(b.clone())
    }

    fn remove_colliders(physics: &mut PhysicsState, chunk: &mut StaticChunk) {
        for b in &mut chunk.blocks {
            if let Some(h) = b.collider.take() {
                physics.remove_collider(h);
            }
        }
        for d in &mut chunk.decor {
            if let Some(h) = d.collider.take() {
                physics.remove_collider(h);
            }
        }
        if let Some(h) = chunk.terrain_collider.take() {
            physics.remove_collider(h);
        }
    }

    /// Makes a region dormant: colliders are removed, content is kept.
    pub fn deactivate(&mut self, physics: &mut PhysicsState, key: RegionKey) -> bool {
        let Some(mut arc) = self.chunks.remove(&key) else { return false };
        let chunk = Arc::make_mut(&mut arc);
        Self::remove_colliders(physics, chunk);
        chunk.version += 1;
        self.dormant.insert(key, arc);
        true
    }

    /// Wakes a dormant region (recreates its colliders). False if there is none.
    pub fn activate(&mut self, physics: &mut PhysicsState, key: RegionKey) -> bool {
        let Some(mut arc) = self.dormant.remove(&key) else { return false };
        let chunk = Arc::make_mut(&mut arc);
        for (i, b) in chunk.blocks.iter_mut().enumerate() {
            if b.alive && !b.has(block_flags::GHOST) {
                let tag = BlockRef { region: key, index: i as u32 }.tag();
                b.collider = Some(physics.insert_static(block_collider(b, tag)));
            }
        }
        for d in &mut chunk.decor {
            if d.solid {
                d.collider = Some(physics.insert_static(decor_collider(d)));
            }
        }
        if let Some(t) = &chunk.terrain {
            chunk.terrain_collider = Some(physics.insert_static(t.collider()));
        }
        chunk.version += 1;
        self.chunks.insert(key, arc);
        true
    }

    /// Removes a region entirely (active or dormant).
    pub fn remove_region(&mut self, physics: &mut PhysicsState, key: RegionKey) {
        if let Some(mut arc) = self.chunks.remove(&key) {
            Self::remove_colliders(physics, Arc::make_mut(&mut arc));
        }
        self.dormant.remove(&key);
    }

    pub fn is_active(&self, key: RegionKey) -> bool {
        self.chunks.contains_key(&key)
    }

    /// Active regions whose bounds intersect a box.
    pub fn regions_in(&self, min: Vec3, max: Vec3) -> impl Iterator<Item = (&RegionKey, &Arc<StaticChunk>)> {
        self.chunks.iter().filter(move |(_, c)| c.intersects(min, max))
    }

    /// Ladders near `p`.
    pub fn ladders_near(&self, p: Vec3) -> impl Iterator<Item = &Ladder> {
        let r = Vec3::splat(2.0);
        self.regions_in(p - r, p + r).flat_map(|(_, c)| c.ladders.iter())
    }

    /// Alive blocks intersecting a sphere.
    pub fn blocks_in_sphere(&self, center: Vec3, radius: f32) -> Vec<BlockRef> {
        let r = Vec3::splat(radius);
        let mut out = Vec::new();
        for (key, chunk) in self.regions_in(center - r, center + r) {
            for (i, b) in chunk.blocks.iter().enumerate() {
                if b.alive && center.clamp(b.min, b.max).distance_squared(center) < radius * radius {
                    out.push(BlockRef { region: *key, index: i as u32 });
                }
            }
        }
        out
    }

    pub fn iter_alive(&self) -> impl Iterator<Item = (BlockRef, &Block)> {
        self.chunks.iter().flat_map(|(k, c)| {
            c.blocks.iter().enumerate().filter(|(_, b)| b.alive).map(move |(i, b)| (BlockRef { region: *k, index: i as u32 }, b))
        })
    }

    pub fn block_count(&self) -> usize {
        self.chunks.values().map(|c| c.blocks.iter().filter(|b| b.alive).count()).sum()
    }
}
