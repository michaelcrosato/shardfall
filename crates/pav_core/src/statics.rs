//! Static level geometry: axis-aligned blocks stored per chunk. Blocks are the "tiles with
//! heights" of the engine: floors, walls, stairs, slabs of upper floors, crates that never move.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{IVec2, Vec3};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::physics::{PhysicsState, TAG_BLOCK};
use crate::shape::Look;

/// Chunk edge length in metres.
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChunkKey(pub i32, pub i32);

impl ChunkKey {
    pub fn of(p: Vec3) -> Self {
        ChunkKey((p.x / CHUNK_SIZE).floor() as i32, (p.z / CHUNK_SIZE).floor() as i32)
    }
    pub fn origin(self) -> Vec3 {
        Vec3::new(self.0 as f32 * CHUNK_SIZE, 0.0, self.1 as f32 * CHUNK_SIZE)
    }
    pub fn ivec(self) -> IVec2 {
        IVec2::new(self.0, self.1)
    }
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
    #[serde(skip)]
    pub collider: Option<ColliderHandle>,
    #[serde(default = "yes")]
    pub alive: bool,
}

fn yes() -> bool {
    true
}

impl Block {
    pub fn new(min: Vec3, max: Vec3, color: Color) -> Self {
        Self { min: min.min(max), max: max.max(min), color, look: Look::Cel, flags: 0, collider: None, alive: true }
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StaticChunk {
    pub blocks: Vec<Block>,
    /// Bumped on every change; the renderer caches per (chunk, version).
    pub version: u64,
}

/// Where a block lives: chunk + index inside the chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockRef {
    pub chunk: ChunkKey,
    pub index: u32,
}

impl BlockRef {
    /// Collider user data: tag in the top 16 bits, then chunk x, chunk z, index (32 bits each).
    pub fn tag(self) -> u128 {
        TAG_BLOCK | (self.chunk.0 as u32 as u128) << 64 | (self.chunk.1 as u32 as u128) << 32 | self.index as u128
    }
    pub fn from_tag(tag: u128) -> Option<Self> {
        if tag & crate::physics::TAG_MASK != TAG_BLOCK {
            return None;
        }
        let index = tag as u32;
        let cz = (tag >> 32) as u32 as i32;
        let cx = (tag >> 64) as u32 as i32;
        Some(BlockRef { chunk: ChunkKey(cx, cz), index })
    }
}

#[derive(Clone, Debug, Default)]
pub struct StaticWorld {
    pub chunks: BTreeMap<ChunkKey, Arc<StaticChunk>>,
}

impl StaticWorld {
    /// Adds a block (creating its collider) and returns where it went.
    pub fn add(&mut self, physics: &mut PhysicsState, mut block: Block) -> BlockRef {
        let key = ChunkKey::of(block.center());
        let chunk = Arc::make_mut(self.chunks.entry(key).or_default());
        let index = chunk.blocks.len() as u32;
        let r = BlockRef { chunk: key, index };
        if !block.has(block_flags::GHOST) {
            let h = block.half();
            let c = block.center();
            let col = ColliderBuilder::cuboid(h.x as Real, h.y as Real, h.z as Real)
                .translation(Vector::new(c.x as Real, c.y as Real, c.z as Real))
                .friction(0.8)
                .user_data(r.tag());
            block.collider = Some(physics.insert_static(col));
        }
        chunk.blocks.push(block);
        chunk.version += 1;
        r
    }

    pub fn get(&self, r: BlockRef) -> Option<&Block> {
        self.chunks.get(&r.chunk)?.blocks.get(r.index as usize)
    }

    /// Removes a block's collider and marks it dead. Returns the block if it was alive.
    pub fn destroy(&mut self, physics: &mut PhysicsState, r: BlockRef) -> Option<Block> {
        let chunk = Arc::make_mut(self.chunks.get_mut(&r.chunk)?);
        let b = chunk.blocks.get_mut(r.index as usize)?;
        if !b.alive {
            return None;
        }
        b.alive = false;
        if let Some(h) = b.collider.take() {
            physics.remove_collider(h);
        }
        chunk.version += 1;
        Some(b.clone())
    }

    /// Removes a whole chunk and its colliders.
    pub fn remove_chunk(&mut self, physics: &mut PhysicsState, key: ChunkKey) {
        if let Some(chunk) = self.chunks.remove(&key) {
            for b in &chunk.blocks {
                if let Some(h) = b.collider {
                    physics.remove_collider(h);
                }
            }
        }
    }

    pub fn iter_alive(&self) -> impl Iterator<Item = (BlockRef, &Block)> {
        self.chunks.iter().flat_map(|(k, c)| {
            c.blocks.iter().enumerate().filter(|(_, b)| b.alive).map(move |(i, b)| (BlockRef { chunk: *k, index: i as u32 }, b))
        })
    }

    pub fn block_count(&self) -> usize {
        self.chunks.values().map(|c| c.blocks.iter().filter(|b| b.alive).count()).sum()
    }
}
