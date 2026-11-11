//! Liquid flow (water, lava, any block flagged `liquid`).
//!
//! Cellular, Minecraft-like rules evaluated on a sparse set of *active*
//! cells, so a still ocean costs nothing:
//!
//! - a source (level 0) feeds its neighbours; a flowing cell keeps the
//!   smallest neighbour level + 1 and dries up past the block's `flow_range`;
//! - liquid falls first: an empty cell below receives a full falling column
//!   ([`FLUID_FALLING`]) and nothing spreads sideways until the fall is blocked;
//! - two horizontal sources above solid ground create a new source (endless
//!   water), for liquids with the full range of 7;
//! - every block edit wakes the edited cell and its six neighbours.
//!
//! Each liquid steps every `flow_ticks` ticks of 50 ms (water 4, lava 12).
//! Bulk edits (streaming, world loads) never wake liquids, so generated seas
//! stay still until someone digs next to them.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::voxel::blocks::{BlockId, BlockRegistry};
use crate::voxel::world::{VoxelWorld, FLUID_FALLING};

/// Duration of one simulation tick.
const TICK: f32 = 0.05;
/// Upper bound of cells evaluated per tick (the rest waits for the next one).
const MAX_UPDATES_PER_TICK: usize = 6000;

const HORIZONTAL: [IVec3; 4] = [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z];
const NEIGHBOURS: [IVec3; 6] = [
    IVec3::X,
    IVec3::NEG_X,
    IVec3::Y,
    IVec3::NEG_Y,
    IVec3::Z,
    IVec3::NEG_Z,
];

#[derive(Resource, Debug)]
pub struct FluidSim {
    pub enabled: bool,
    active: HashSet<IVec3>,
    timer: f32,
    tick: u64,
    last_registry: Option<u64>,
    /// Cells evaluated during the last tick (debug / console).
    pub last_updates: usize,
}

impl Default for FluidSim {
    fn default() -> Self {
        Self {
            enabled: true,
            active: HashSet::new(),
            timer: 0.0,
            tick: 0,
            last_registry: None,
            last_updates: 0,
        }
    }
}

impl FluidSim {
    fn wake_around(&mut self, cell: IVec3) {
        self.active.insert(cell);
        for offset in NEIGHBOURS {
            self.active.insert(cell + offset);
        }
    }
}

/// Mirror `BlockDef::is_liquid` into the world so the picker, the player and
/// the simulation can test cells without the registry.
pub fn sync_liquid_mask(registry: Res<BlockRegistry>, mut world: ResMut<VoxelWorld>, mut sim: ResMut<FluidSim>) {
    if sim.last_registry == Some(registry.revision) {
        return;
    }
    sim.last_registry = Some(registry.revision);
    let mask = registry.blocks().map(|(_, def)| def.is_liquid()).collect();
    world.set_liquid_mask(mask);
}

/// Can a liquid move into this empty cell? The target chunk (or one of the
/// chunks just below it) must exist, otherwise the cell belongs to a column
/// that is not generated yet and the liquid would pour into the void.
fn loaded(world: &VoxelWorld, cell: IVec3) -> bool {
    let coord = VoxelWorld::chunk_coord(cell);
    (0..6).any(|depth| world.has_chunk(coord - IVec3::Y * depth))
}

fn solid(world: &VoxelWorld, cell: IVec3) -> bool {
    match world.get(cell) {
        Some(voxel) => !world.is_liquid_block(voxel.block) || voxel.sub.is_some(),
        None => false,
    }
}

pub fn simulate_fluids(
    time: Res<Time>,
    registry: Res<BlockRegistry>,
    mut world: ResMut<VoxelWorld>,
    mut sim: ResMut<FluidSim>,
) {
    if let Some(edits) = world.take_edits() {
        for cell in edits {
            sim.wake_around(cell);
        }
    }
    if !sim.enabled {
        return;
    }

    sim.timer += time.delta_secs().min(0.25);
    let mut steps = 0;
    while sim.timer >= TICK && steps < 3 {
        sim.timer -= TICK;
        steps += 1;
        sim.tick = sim.tick.wrapping_add(1);
        step(&registry, &mut world, &mut sim);
    }
    if steps == 3 {
        sim.timer = 0.0;
    }
}

/// Run the flow on a standalone world (forge preview) until it settles or
/// `max_ticks` ticks elapsed. Every cell written so far counts as an edit.
pub fn settle(registry: &BlockRegistry, world: &mut VoxelWorld, max_ticks: usize) {
    world.set_liquid_mask(registry.blocks().map(|(_, def)| def.is_liquid()).collect());
    let mut sim = FluidSim::default();
    if let Some(edits) = world.take_edits() {
        for cell in edits {
            sim.wake_around(cell);
        }
    }
    for _ in 0..max_ticks {
        sim.tick = sim.tick.wrapping_add(1);
        step(registry, world, &mut sim);
        if sim.active.is_empty() {
            break;
        }
    }
}

fn step(registry: &BlockRegistry, world: &mut VoxelWorld, sim: &mut FluidSim) {
    if sim.active.is_empty() {
        sim.last_updates = 0;
        return;
    }
    let mut cells: Vec<IVec3> = sim.active.drain().collect();
    // Bottom-up, deterministic order.
    cells.sort_by_key(|c| (c.y, c.x, c.z));
    let mut next: HashSet<IVec3> = HashSet::new();
    let mut updates = 0;

    for cell in cells {
        let Some(block) = world.liquid_at(cell) else {
            continue;
        };
        if updates >= MAX_UPDATES_PER_TICK {
            next.insert(cell);
            continue;
        }
        let def = registry.get(block);
        if sim.tick % u64::from(def.flow_ticks.clamp(1, 60)) != 0 {
            next.insert(cell);
            continue;
        }
        updates += 1;
        update_cell(world, &mut next, cell, block, def.flow_range.clamp(1, 7));
    }
    sim.last_updates = updates;
    sim.active = next;
}

fn wake(next: &mut HashSet<IVec3>, cell: IVec3) {
    next.insert(cell);
    for offset in NEIGHBOURS {
        next.insert(cell + offset);
    }
}

fn update_cell(world: &mut VoxelWorld, next: &mut HashSet<IVec3>, cell: IVec3, block: BlockId, range: u8) {
    let mut level = world.fluid_level(cell);

    // 1. A flowing cell follows its feeders (or dries up).
    if level != 0 {
        let fed_from_above = world.liquid_at(cell + IVec3::Y) == Some(block);
        let wanted = if fed_from_above {
            Some(FLUID_FALLING)
        } else {
            let mut best = u8::MAX;
            let mut sources = 0;
            for offset in HORIZONTAL {
                let neighbour = cell + offset;
                if world.liquid_at(neighbour) != Some(block) {
                    continue;
                }
                let neighbour_level = world.fluid_level(neighbour);
                if neighbour_level == 0 {
                    sources += 1;
                }
                // A falling column feeds its sides like a source.
                let effective = if neighbour_level == FLUID_FALLING { 0 } else { neighbour_level };
                best = best.min(effective.saturating_add(1));
            }
            if sources >= 2 && range >= 7 && solid(world, cell - IVec3::Y) {
                Some(0)
            } else if best <= range {
                Some(best)
            } else {
                None
            }
        };
        match wanted {
            None => {
                world.clear_flowing(cell);
                wake(next, cell);
                return;
            }
            Some(wanted) if wanted != level => {
                world.set_fluid_level(cell, wanted);
                level = wanted;
                wake(next, cell);
            }
            Some(_) => {}
        }
    }

    // 2. Fall.
    let below = cell - IVec3::Y;
    match world.get(below) {
        None if loaded(world, below) => {
            world.set_flowing(below, block, FLUID_FALLING);
            wake(next, below);
            return;
        }
        Some(voxel) if voxel.block == block && voxel.sub.is_none() => {
            let below_level = world.fluid_level(below);
            if below_level != 0 && below_level != FLUID_FALLING {
                world.set_fluid_level(below, FLUID_FALLING);
                wake(next, below);
            }
            // Landing in the same liquid: no sideways spread.
            return;
        }
        _ => {}
    }

    // 3. Spread sideways on solid ground.
    let spread = if level == 0 || level == FLUID_FALLING { 1 } else { level + 1 };
    if spread > range {
        return;
    }
    for offset in HORIZONTAL {
        let target = cell + offset;
        match world.get(target) {
            None if loaded(world, target) => {
                world.set_flowing(target, block, spread);
                wake(next, target);
            }
            Some(voxel) if voxel.block == block && voxel.sub.is_none() => {
                let current = world.fluid_level(target);
                if current != 0 && current != FLUID_FALLING && current > spread {
                    world.set_fluid_level(target, spread);
                    wake(next, target);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::world::Voxel;

    fn setup() -> (BlockRegistry, VoxelWorld, FluidSim, BlockId) {
        let registry = BlockRegistry::builtin();
        let mut world = VoxelWorld::new();
        world.set_liquid_mask(registry.blocks().map(|(_, def)| def.is_liquid()).collect());
        let stone = registry.id_or_default("stone");
        for x in -10..=10 {
            for z in -10..=10 {
                world.set(IVec3::new(x, 0, z), Some(Voxel::new(stone)));
            }
        }
        world.take_edits();
        let water = registry.id_or_default("water");
        (registry, world, FluidSim::default(), water)
    }

    #[test]
    fn settle_spreads_a_source_offline() {
        let (registry, mut world, _, water) = setup();
        world.set(IVec3::new(0, 1, 0), Some(Voxel::new(water)));
        settle(&registry, &mut world, 400);
        assert_eq!(world.liquid_at(IVec3::new(2, 1, 0)), Some(water));
    }

    fn run(registry: &BlockRegistry, world: &mut VoxelWorld, sim: &mut FluidSim, ticks: usize) {
        if let Some(edits) = world.take_edits() {
            for cell in edits {
                sim.wake_around(cell);
            }
        }
        for _ in 0..ticks {
            sim.tick += 1;
            step(registry, world, sim);
        }
    }

    #[test]
    fn a_source_spreads_up_to_its_range_and_falls_first() {
        let (registry, mut world, mut sim, water) = setup();
        world.set(IVec3::new(0, 3, 0), Some(Voxel::new(water)));
        run(&registry, &mut world, &mut sim, 200);
        // Fell down to the floor...
        assert_eq!(world.liquid_at(IVec3::new(0, 1, 0)), Some(water));
        assert_eq!(world.fluid_level(IVec3::new(0, 2, 0)), FLUID_FALLING);
        // ...then spread 7 cells around the landing point.
        assert_eq!(world.liquid_at(IVec3::new(7, 1, 0)), Some(water));
        assert_eq!(world.liquid_at(IVec3::new(9, 1, 0)), None);
    }

    #[test]
    fn removing_the_source_drains_the_flow() {
        let (registry, mut world, mut sim, water) = setup();
        let source = IVec3::new(0, 1, 0);
        world.set(source, Some(Voxel::new(water)));
        run(&registry, &mut world, &mut sim, 200);
        assert_eq!(world.liquid_at(IVec3::new(3, 1, 0)), Some(water));
        world.set(source, None);
        run(&registry, &mut world, &mut sim, 400);
        assert_eq!(world.liquid_at(IVec3::new(3, 1, 0)), None);
        assert_eq!(world.liquid_at(IVec3::new(1, 1, 0)), None);
    }
}
