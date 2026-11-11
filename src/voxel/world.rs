//! Chunked voxel storage.
//!
//! The world is a sparse map of dense 16^3 chunks. Every chunk owns its cells so
//! the mesher can walk a contiguous array, while the world itself only keeps the
//! chunks that contain at least one voxel.
//!
//! Coordinates are world-space voxel coordinates: the voxel at `(x, y, z)`
//! occupies the unit cube `[x, x + 1) x [y, y + 1) x [z, z + 1)`.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::voxel::blocks::BlockId;
use crate::voxel::shapes::Orientation;

/// Edge length of a chunk, in voxels.
pub const CHUNK_SIZE: usize = 16;

/// Number of cells in a single chunk.
pub const CHUNK_VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

/// Second half-shape fused into the same cell (sub-voxel fusion).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SubVoxel {
    pub block: BlockId,
    pub orientation: Orientation,
}

/// A single authored voxel. The shape and material live in the block registry,
/// so a cell stores a 16-bit block index, a 5-bit orientation and an optional
/// second half-shape with its own block (and therefore its own material).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Voxel {
    pub block: BlockId,
    pub orientation: Orientation,
    pub sub: Option<SubVoxel>,
}

impl Voxel {
    pub const fn new(block: BlockId) -> Self {
        Self {
            block,
            orientation: Orientation::IDENTITY,
            sub: None,
        }
    }

    pub const fn oriented(block: BlockId, orientation: Orientation) -> Self {
        Self {
            block,
            orientation,
            sub: None,
        }
    }

    pub const fn with_sub(mut self, sub: SubVoxel) -> Self {
        self.sub = Some(sub);
        self
    }

    /// Every `(block, orientation)` part stored in the cell.
    pub fn parts(&self) -> impl Iterator<Item = (BlockId, Orientation)> {
        std::iter::once((self.block, self.orientation))
            .chain(self.sub.map(|sub| (sub.block, sub.orientation)))
    }
}

/// Dense storage for one chunk.
#[derive(Clone, Debug)]
pub struct VoxelChunk {
    pub coord: IVec3,
    cells: Vec<Option<Voxel>>,
}

impl VoxelChunk {
    pub fn new(coord: IVec3) -> Self {
        Self {
            coord,
            cells: vec![None; CHUNK_VOLUME],
        }
    }

    pub const fn local_index(local: UVec3) -> usize {
        (local.y as usize * CHUNK_SIZE + local.z as usize) * CHUNK_SIZE + local.x as usize
    }

    /// Inverse of [`Self::local_index`].
    pub const fn local_from_index(index: usize) -> UVec3 {
        let x = index % CHUNK_SIZE;
        let z = (index / CHUNK_SIZE) % CHUNK_SIZE;
        let y = index / (CHUNK_SIZE * CHUNK_SIZE);
        UVec3::new(x as u32, y as u32, z as u32)
    }

    pub fn get(&self, local: UVec3) -> Option<Voxel> {
        self.cells.get(Self::local_index(local)).copied().flatten()
    }

    /// Write a cell. Returns `true` when the stored value actually changed.
    pub fn set(&mut self, local: UVec3, voxel: Option<Voxel>) -> bool {
        let index = Self::local_index(local);
        if self.cells[index] == voxel {
            return false;
        }
        self.cells[index] = voxel;
        true
    }

    pub fn filled(&self) -> usize {
        self.cells.iter().filter(|cell| cell.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|cell| cell.is_none())
    }

    /// World-space minimum corner of this chunk.
    pub fn origin(&self) -> IVec3 {
        self.coord * CHUNK_SIZE as i32
    }
}

/// Aggregate counters exposed to the editor panels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldStats {
    pub chunks: usize,
    pub voxels: usize,
    pub dirty: usize,
    pub revision: u64,
}

/// Flow level of a liquid cell that is not a source (sources store nothing).
/// `1..=7` = spreading water, lower = fuller; [`FLUID_FALLING`] = falling column.
pub const FLUID_FALLING: u8 = 8;

/// Edits kept for the fluid simulation per frame; past this the frame is a
/// bulk load (streaming, world load) and does not wake liquids.
const MAX_TRACKED_EDITS: usize = 2048;

/// Sparse voxel world shared by the mesher, the picker and the editor.
#[derive(Resource, Clone, Debug, Default)]
pub struct VoxelWorld {
    chunks: HashMap<IVec3, VoxelChunk>,
    /// Chunks whose surface must be rebuilt, including neighbours of a border edit.
    dirty: HashSet<IVec3>,
    pub revision: u64,
    /// Flow level of non-source liquid cells (see [`FLUID_FALLING`]).
    fluid: HashMap<IVec3, u8>,
    /// `liquid[block]` mirrors `BlockDef::is_liquid`, synced from the registry.
    liquid: Vec<bool>,
    /// Cells written since the fluid simulation last looked.
    edits: Vec<IVec3>,
    /// Cells written one by one since the cable network last looked.
    changed: Vec<IVec3>,
    edits_overflow: bool,
    /// Chunk columns (x, z) touched by single edits since generation: the
    /// streamer keeps them in memory when it unloads them.
    modified_columns: HashSet<IVec2>,
    /// Cells built by a player (game or editor placement, settled movables):
    /// a launched movable always carries the ones connected to it.
    placed: HashSet<IVec3>,
    /// While true, every single edit is logged (studio undo, see
    /// `editor::undo`): (cell, before, after).
    pub recording: bool,
    pub undo_log: Vec<(IVec3, Option<Voxel>, Option<Voxel>)>,
}

impl VoxelWorld {
    pub fn new() -> Self {
        Self::default()
    }

    /// Chunk containing a world-space voxel coordinate. Uses Euclidean division
    /// so negative coordinates map to the correct chunk and local cell.
    pub fn chunk_coord(world: IVec3) -> IVec3 {
        IVec3::new(
            world.x.div_euclid(CHUNK_SIZE as i32),
            world.y.div_euclid(CHUNK_SIZE as i32),
            world.z.div_euclid(CHUNK_SIZE as i32),
        )
    }

    pub fn local_coord(world: IVec3) -> UVec3 {
        UVec3::new(
            world.x.rem_euclid(CHUNK_SIZE as i32) as u32,
            world.y.rem_euclid(CHUNK_SIZE as i32) as u32,
            world.z.rem_euclid(CHUNK_SIZE as i32) as u32,
        )
    }

    pub fn chunk(&self, coord: IVec3) -> Option<&VoxelChunk> {
        self.chunks.get(&coord)
    }

    pub fn chunks(&self) -> impl Iterator<Item = &VoxelChunk> {
        self.chunks.values()
    }

    pub fn get(&self, world: IVec3) -> Option<Voxel> {
        self.chunks
            .get(&Self::chunk_coord(world))
            .and_then(|chunk| chunk.get(Self::local_coord(world)))
    }

    /// Write a world-space voxel. Empty chunks are removed automatically so the
    /// hierarchy and the renderer never keep ghost chunks.
    pub fn set(&mut self, world: IVec3, voxel: Option<Voxel>) -> bool {
        let coord = Self::chunk_coord(world);
        let local = Self::local_coord(world);
        if self.recording {
            let before = self.get(world);
            if before != voxel {
                self.undo_log.push((world, before, voxel));
            }
        }

        match voxel {
            Some(voxel) => {
                let chunk = self
                    .chunks
                    .entry(coord)
                    .or_insert_with(|| VoxelChunk::new(coord));
                if chunk.set(local, Some(voxel)) {
                    self.revision = self.revision.wrapping_add(1);
                    self.mark_dirty(world);
                    self.fluid.remove(&world);
                    self.track_edit(world);
                    true
                } else {
                    false
                }
            }
            None => {
                let Some(chunk) = self.chunks.get_mut(&coord) else {
                    return false;
                };

                let changed = chunk.set(local, None);
                let empty = chunk.is_empty();
                self.placed.remove(&world);
                if changed {
                    self.revision = self.revision.wrapping_add(1);
                    self.mark_dirty(world);
                    self.fluid.remove(&world);
                    self.track_edit(world);
                }
                if empty {
                    self.chunks.remove(&coord);
                }
                changed
            }
        }
    }

    fn track_edit(&mut self, world: IVec3) {
        if self.changed.len() < MAX_TRACKED_EDITS {
            self.changed.push(world);
        }
        let coord = Self::chunk_coord(world);
        self.modified_columns.insert(IVec2::new(coord.x, coord.z));
        if self.edits.len() < MAX_TRACKED_EDITS {
            self.edits.push(world);
        } else {
            self.edits_overflow = true;
        }
    }

    /// Remember that a player built the block in `world` (see `placed`).
    pub fn mark_placed(&mut self, world: IVec3) {
        if self.get(world).is_some() {
            self.placed.insert(world);
        }
    }

    /// Was the block in `world` built by a player?
    pub fn is_placed(&self, world: IVec3) -> bool {
        self.placed.contains(&world)
    }

    /// Write many generated cells at once: one dirty mark per chunk, no edit
    /// tracking (streaming, world generation).
    pub fn set_bulk(&mut self, cells: Vec<(IVec3, Voxel)>) {
        let mut touched: HashSet<IVec3> = HashSet::new();
        for (world, voxel) in cells {
            let coord = Self::chunk_coord(world);
            let local = Self::local_coord(world);
            let chunk = self.chunks.entry(coord).or_insert_with(|| VoxelChunk::new(coord));
            if chunk.set(local, Some(voxel)) {
                touched.insert(coord);
            }
        }
        if touched.is_empty() {
            return;
        }
        self.revision = self.revision.wrapping_add(1);
        for coord in touched {
            self.dirty.insert(coord);
            for offset in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                if self.chunks.contains_key(&(coord + offset)) {
                    self.dirty.insert(coord + offset);
                }
            }
        }
    }

    /// Was this chunk column edited by hand since it was generated?
    pub fn column_modified(&self, column: IVec2) -> bool {
        self.modified_columns.contains(&column)
    }

    /// A copy holding only the edited columns: the others come back from the seed.
    pub fn edited_columns(&self) -> Self {
        let mut copy = Self::new();
        let edited = self.chunks.values().filter(|chunk| self.column_modified(IVec2::new(chunk.coord.x, chunk.coord.z)));
        copy.chunks = edited.map(|chunk| (chunk.coord, chunk.clone())).collect();
        copy.fluid = self.fluid.clone();
        copy
    }

    /// Remove a whole chunk column and return its chunks (flowing liquid is
    /// dropped: only sources are kept, like on save).
    pub fn take_column(&mut self, column: IVec2, low: i32, high: i32) -> Vec<VoxelChunk> {
        let mut taken = Vec::new();
        for cy in low..=high {
            let coord = IVec3::new(column.x, cy, column.y);
            if let Some(mut chunk) = self.chunks.remove(&coord) {
                let origin = chunk.origin();
                for index in 0..CHUNK_VOLUME {
                    let local = VoxelChunk::local_from_index(index);
                    if self.fluid.remove(&(origin + local.as_ivec3())).is_some() {
                        chunk.set(local, None);
                    }
                }
                self.dirty.insert(coord);
                taken.push(chunk);
            }
        }
        self.modified_columns.remove(&column);
        if !taken.is_empty() {
            self.revision = self.revision.wrapping_add(1);
        }
        taken
    }

    /// Put back chunks stored by [`Self::take_column`].
    pub fn restore_column(&mut self, column: IVec2, chunks: Vec<VoxelChunk>) {
        for chunk in chunks {
            let coord = chunk.coord;
            self.dirty.insert(coord);
            for offset in [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z, IVec3::Y, IVec3::NEG_Y] {
                if self.chunks.contains_key(&(coord + offset)) {
                    self.dirty.insert(coord + offset);
                }
            }
            self.chunks.insert(coord, chunk);
        }
        self.modified_columns.insert(column);
        self.revision = self.revision.wrapping_add(1);
    }

    /// Cells written one by one since the last call (cables of a removed or
    /// replaced block are dropped with it).
    pub fn take_changed(&mut self) -> Vec<IVec3> {
        std::mem::take(&mut self.changed)
    }

    /// Cells edited since the last call. `None` when too many cells changed
    /// at once (bulk generation or loading), which must not wake liquids.
    pub fn take_edits(&mut self) -> Option<Vec<IVec3>> {
        let edits = std::mem::take(&mut self.edits);
        if std::mem::take(&mut self.edits_overflow) {
            None
        } else {
            Some(edits)
        }
    }

    /// Refresh the liquid lookup from the block registry.
    pub fn set_liquid_mask(&mut self, mask: Vec<bool>) {
        self.liquid = mask;
    }

    pub fn is_liquid_block(&self, block: BlockId) -> bool {
        self.liquid.get(block.0 as usize).copied().unwrap_or(false)
    }

    /// Liquid block stored in a cell, if any.
    pub fn liquid_at(&self, world: IVec3) -> Option<BlockId> {
        self.get(world)
            .filter(|voxel| voxel.sub.is_none() && self.is_liquid_block(voxel.block))
            .map(|voxel| voxel.block)
    }

    /// Flow level of a liquid cell: `0` = source.
    pub fn fluid_level(&self, world: IVec3) -> u8 {
        self.fluid.get(&world).copied().unwrap_or(0)
    }

    /// Change the flow level of an existing liquid cell (remeshes on change).
    pub fn set_fluid_level(&mut self, world: IVec3, level: u8) {
        let previous = self.fluid_level(world);
        if previous == level {
            return;
        }
        if level == 0 {
            self.fluid.remove(&world);
        } else {
            self.fluid.insert(world, level);
        }
        self.revision = self.revision.wrapping_add(1);
        self.mark_dirty(world);
        // Neighbouring liquid surfaces are smoothed with this cell's height.
        for offset in [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
            self.dirty.insert(Self::chunk_coord(world + offset));
        }
    }

    /// Place a flowing liquid (used by the fluid simulation, never tracked as
    /// an edit so the simulation does not wake itself twice).
    pub fn set_flowing(&mut self, world: IVec3, block: BlockId, level: u8) -> bool {
        let coord = Self::chunk_coord(world);
        let local = Self::local_coord(world);
        let chunk = self
            .chunks
            .entry(coord)
            .or_insert_with(|| VoxelChunk::new(coord));
        let changed = chunk.set(local, Some(Voxel::new(block)));
        if level == 0 {
            self.fluid.remove(&world);
        } else {
            self.fluid.insert(world, level);
        }
        self.revision = self.revision.wrapping_add(1);
        self.mark_dirty(world);
        for offset in [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
            self.dirty.insert(Self::chunk_coord(world + offset));
        }
        changed
    }

    /// Remove a liquid cell without tracking it as an edit.
    pub fn clear_flowing(&mut self, world: IVec3) -> bool {
        let coord = Self::chunk_coord(world);
        let local = Self::local_coord(world);
        let Some(chunk) = self.chunks.get_mut(&coord) else {
            return false;
        };
        let changed = chunk.set(local, None);
        let empty = chunk.is_empty();
        self.fluid.remove(&world);
        if changed {
            self.revision = self.revision.wrapping_add(1);
            self.mark_dirty(world);
            for offset in [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
                self.dirty.insert(Self::chunk_coord(world + offset));
            }
        }
        if empty {
            self.chunks.remove(&coord);
        }
        changed
    }

    /// Mark the edited chunk and any chunk sharing the touched face as dirty.
    fn mark_dirty(&mut self, world: IVec3) {
        let coord = Self::chunk_coord(world);
        let local = Self::local_coord(world);
        let last = CHUNK_SIZE as u32 - 1;

        self.dirty.insert(coord);
        if local.x == 0 {
            self.dirty.insert(coord - IVec3::X);
        } else if local.x == last {
            self.dirty.insert(coord + IVec3::X);
        }
        if local.y == 0 {
            self.dirty.insert(coord - IVec3::Y);
        } else if local.y == last {
            self.dirty.insert(coord + IVec3::Y);
        }
        if local.z == 0 {
            self.dirty.insert(coord - IVec3::Z);
        } else if local.z == last {
            self.dirty.insert(coord + IVec3::Z);
        }
    }

    /// Drain the chunks waiting for a remesh, in deterministic order.
    pub fn take_dirty(&mut self) -> Vec<IVec3> {
        let mut coords: Vec<IVec3> = self.dirty.drain().collect();
        coords.sort_by_key(|coord| (coord.x, coord.y, coord.z));
        coords
    }

    /// Replace the whole content with another world (PIE rollback). Every chunk
    /// present before or after is marked for a remesh.
    pub fn restore_from(&mut self, other: &VoxelWorld) {
        let mut dirty: HashSet<IVec3> = self.chunks.keys().copied().collect();
        dirty.extend(other.chunks.keys().copied());
        self.chunks = other.chunks.clone();
        self.fluid = other.fluid.clone();
        self.modified_columns = other.modified_columns.clone();
        self.placed = other.placed.clone();
        self.undo_log.clear();
        self.edits.clear();
        self.edits_overflow = false;
        self.dirty.extend(dirty);
        self.revision = self.revision.wrapping_add(1);
    }

    /// Does a chunk exist at this chunk coordinate?
    pub fn has_chunk(&self, coord: IVec3) -> bool {
        self.chunks.contains_key(&coord)
    }

    pub fn clear(&mut self) {
        self.dirty.extend(self.chunks.keys().copied());
        self.chunks.clear();
        self.fluid.clear();
        self.modified_columns.clear();
        self.placed.clear();
        self.edits.clear();
        self.edits_overflow = true;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn stats(&self) -> WorldStats {
        WorldStats {
            chunks: self.chunks.len(),
            voxels: self.chunks.values().map(VoxelChunk::filled).sum(),
            dirty: self.dirty.len(),
            revision: self.revision,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_coordinates_wrap_into_the_right_chunk() {
        assert_eq!(VoxelWorld::chunk_coord(IVec3::new(-1, -17, 16)), IVec3::new(-1, -2, 1));
        assert_eq!(VoxelWorld::local_coord(IVec3::new(-1, -17, 16)), UVec3::new(15, 15, 0));
    }

    #[test]
    fn only_edited_columns_are_kept_for_a_streamed_world() {
        let mut world = VoxelWorld::new();
        let voxel = Voxel::new(BlockId(0));
        world.set(IVec3::new(1, 1, 1), Some(voxel));
        world.set(IVec3::new(40, 1, 1), Some(voxel));
        world.modified_columns.remove(&IVec2::new(2, 0));

        let saved = world.edited_columns();

        assert_eq!(saved.get(IVec3::new(1, 1, 1)), Some(voxel));
        assert_eq!(saved.get(IVec3::new(40, 1, 1)), None);
    }

    #[test]
    fn writing_and_erasing_updates_the_revision_and_drops_empty_chunks() {
        let mut world = VoxelWorld::new();
        let voxel = Voxel::new(BlockId(0));

        assert!(world.set(IVec3::new(3, 4, 5), Some(voxel)));
        assert_eq!(world.get(IVec3::new(3, 4, 5)), Some(voxel));
        assert_eq!(world.stats().chunks, 1);
        assert_eq!(world.stats().voxels, 1);

        let revision = world.revision;
        assert!(world.set(IVec3::new(3, 4, 5), None));
        assert!(world.revision > revision);
        assert_eq!(world.stats().chunks, 0);
        assert_eq!(world.get(IVec3::new(3, 4, 5)), None);
    }

    #[test]
    fn border_edits_dirty_the_touching_neighbour_chunk() {
        let mut world = VoxelWorld::new();
        world.set(IVec3::new(15, 0, 0), Some(Voxel::new(BlockId(0))));

        let dirty = world.take_dirty();
        assert!(dirty.contains(&IVec3::ZERO));
        assert!(dirty.contains(&IVec3::new(1, 0, 0)));
        assert!(world.take_dirty().is_empty());
    }

    #[test]
    fn clear_marks_every_existing_chunk_for_removal() {
        let mut world = VoxelWorld::new();
        world.set(IVec3::new(0, 0, 0), Some(Voxel::new(BlockId(0))));
        world.set(IVec3::new(40, 0, -40), Some(Voxel::new(BlockId(0))));
        world.take_dirty();

        world.clear();
        let dirty = world.take_dirty();
        assert_eq!(dirty.len(), 2);
        assert!(dirty.contains(&IVec3::ZERO));
        assert!(dirty.contains(&IVec3::new(2, 0, -3)));
    }

    #[test]
    fn local_index_round_trips() {
        for index in [0usize, 15, 16, 255, 256, 4095] {
            let local = VoxelChunk::local_from_index(index);
            assert_eq!(VoxelChunk::local_index(local), index);
        }
    }
}
