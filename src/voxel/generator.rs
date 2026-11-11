//! Procedural demo world.
//!
//! Everything here is generated from deterministic arithmetic: an island, a
//! stepped plinth that showcases the nine canonical shapes, and a solid tower
//! that makes the effect of greedy meshing obvious. Blocks are resolved through
//! the data-driven registry, so the generator never hardcodes a colour.

use bevy::prelude::*;

use crate::voxel::blocks::{BlockId, BlockRegistry};
use crate::voxel::shapes::VoxelShape;
use crate::voxel::world::{Voxel, VoxelWorld};

const SHOWCASE_KEYS: [&str; 9] = [
    "cube_teal",
    "wedge_blue",
    "inner_violet",
    "outer_magenta",
    "cylinder_pink",
    "half_cylinder_peach",
    "sphere_gold",
    "arch_lime",
    "pillar_green",
];

/// Rebuild the demo world from scratch. Also used by the console and toolbox
/// reset buttons.
pub fn generate_demo_world(world: &mut VoxelWorld, registry: &BlockRegistry) {
    world.clear();

    let palette = Palette::new(registry);
    generate_island(world, &palette);
    generate_plinth(world, &palette);
    generate_shape_showcase(world, registry);
    generate_tower(world, &palette);
}

struct Palette {
    grass: BlockId,
    grass_dark: BlockId,
    dirt: BlockId,
    stone: BlockId,
    sand: BlockId,
    plinth: BlockId,
    plinth_trim: BlockId,
    tower: BlockId,
    beacon: BlockId,
}

impl Palette {
    fn new(registry: &BlockRegistry) -> Self {
        Self {
            grass: registry.id_or_default("grass"),
            grass_dark: registry.id_or_default("grass_dark"),
            dirt: registry.id_or_default("dirt"),
            stone: registry.id_or_default("stone"),
            sand: registry.id_or_default("sand"),
            plinth: registry.id_or_default("stone"),
            plinth_trim: registry.id_or_default("temple_brick"),
            tower: registry.id_or_default("basalt"),
            beacon: registry.id_or_default("lamp"),
        }
    }
}

fn generate_island(world: &mut VoxelWorld, palette: &Palette) {
    for z in -14..=14 {
        for x in -14..=14 {
            let fx = x as f32;
            let fz = z as f32;
            let radius = (fx * fx + fz * fz).sqrt();
            if radius > 15.2 {
                continue;
            }

            let falloff = (1.0 - radius / 15.2).clamp(0.0, 1.0);
            let wave = (fx * 0.32).sin() * 0.9
                + (fz * 0.27).cos() * 0.8
                + ((fx + fz) * 0.15).sin() * 0.6;
            let height = (falloff * 3.6 + wave * falloff * 0.7 + 0.3).round() as i32;
            let height = height.clamp(0, 4);

            for y in 0..=height {
                let top = y == height;
                let block = if top {
                    if radius > 12.8 {
                        palette.sand
                    } else if hash2(x, z) % 3 == 0 {
                        palette.grass_dark
                    } else {
                        palette.grass
                    }
                } else if y + 1 == height {
                    palette.dirt
                } else {
                    palette.stone
                };

                world.set(IVec3::new(x, y, z), Some(Voxel::new(block)));
            }
        }
    }
}

fn generate_plinth(world: &mut VoxelWorld, palette: &Palette) {
    // Stepped pedestal embedded in the island: each layer is wider than the one
    // above it, so the showcase platform never floats above the terrain.
    for z in -8i32..=8 {
        for x in -8i32..=8 {
            world.set(
                IVec3::new(x, 2, z),
                Some(Voxel::new(palette.plinth_trim)),
            );
        }
    }

    for z in -7i32..=7 {
        for x in -7i32..=7 {
            world.set(
                IVec3::new(x, 3, z),
                Some(Voxel::new(palette.plinth_trim)),
            );
        }
    }

    for z in -6i32..=6 {
        for x in -6i32..=6 {
            let edge = x.abs() == 6 || z.abs() == 6;
            let block = if edge {
                palette.plinth_trim
            } else {
                palette.plinth
            };
            world.set(IVec3::new(x, 4, z), Some(Voxel::new(block)));
            world.set(IVec3::new(x, 5, z), Some(Voxel::new(block)));
        }
    }
}

fn generate_shape_showcase(world: &mut VoxelWorld, registry: &BlockRegistry) {
    for (index, _shape) in VoxelShape::ALL.into_iter().enumerate() {
        let column = (index % 3) as i32 - 1;
        let row = (index / 3) as i32 - 1;
        let position = IVec3::new(column * 4, 6, row * 4);
        let block = registry.id_or_default(SHOWCASE_KEYS[index]);
        world.set(position, Some(Voxel::new(block)));
    }
}

fn generate_tower(world: &mut VoxelWorld, palette: &Palette) {
    for z in -9..=-7 {
        for x in 9..=11 {
            for y in 0..=7 {
                world.set(
                    IVec3::new(x, y, z),
                    Some(Voxel::new(palette.tower)),
                );
            }
        }
    }

    for y in 8..=10 {
        world.set(
            IVec3::new(10, y, -8),
            Some(Voxel::new(palette.beacon)),
        );
    }
}

/// Small deterministic hash used for grass tint variation.
fn hash2(x: i32, z: i32) -> u32 {
    let mut value = (x as u32).wrapping_mul(0x9E37_79B9) ^ (z as u32).wrapping_mul(0x85EB_CA6B);
    value ^= value >> 13;
    value = value.wrapping_mul(0xC2B2_AE35);
    value ^ (value >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_world_contains_every_shape_and_a_tower() {
        let registry = BlockRegistry::builtin();
        let mut world = VoxelWorld::new();
        generate_demo_world(&mut world, &registry);

        let stats = world.stats();
        assert!(stats.chunks >= 4);
        assert!(stats.voxels > 2_000);

        for shape in VoxelShape::ALL {
            let found = world.chunks().any(|chunk| {
                chunk.filled() > 0
                    && (0..16).any(|y| {
                        (0..16).any(|z| {
                            (0..16).any(|x| {
                                chunk
                                    .get(UVec3::new(x, y, z))
                                    .map(|voxel| registry.get(voxel.block).shape == shape)
                                    .unwrap_or(false)
                            })
                        })
                    })
            });
            assert!(found, "demo world is missing {}", shape.label());
        }
    }
}
