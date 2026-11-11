//! Voxel world serialization.
//!
//! Worlds are plain `.ron` documents under `project/worlds/`. Each chunk stores
//! the non-empty cells as `(index, block key)` pairs, which keeps the file
//! readable and diff-friendly while staying compact for sparse terrain.
//!
//! Block keys are resolved through the registry on load, so renaming a block
//! file is a data change, not a world-format break.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::project_io::{load_ron, save_ron};
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::shapes::Orientation;
use crate::voxel::world::{SubVoxel, Voxel, VoxelChunk, VoxelWorld, CHUNK_SIZE, CHUNK_VOLUME};

/// Version written into every world file.
pub const WORLD_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldDocument {
    pub format_version: u32,
    pub name: String,
    /// Stable block keys referenced by the chunks, sorted for readability.
    pub blocks: Vec<String>,
    pub chunks: Vec<ChunkDocument>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkDocument {
    pub coord: [i32; 3],
    pub voxels: Vec<VoxelDocument>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoxelDocument {
    /// Linear cell index inside the chunk, in `0..CHUNK_VOLUME`.
    pub index: u16,
    pub block: String,
    /// Packed facing * 4 + roll. Omitted for the identity orientation.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rot: u8,
    /// Fused second half-shape: `(block key, packed orientation)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<(String, u8)>,
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

/// Summary returned after a successful save.
#[derive(Clone, Debug)]
pub struct SavedWorld {
    pub chunks: usize,
    pub voxels: usize,
    pub path: PathBuf,
}

/// Summary returned after a successful load.
#[derive(Clone, Debug)]
pub struct LoadedWorld {
    pub name: String,
    pub chunks: usize,
    pub voxels: usize,
    pub skipped: usize,
}

/// Serialize the current world and write it as pretty RON.
pub fn save_to_path(
    world: &VoxelWorld,
    registry: &BlockRegistry,
    path: &Path,
    name: &str,
) -> Result<SavedWorld, String> {
    let mut chunks: Vec<&VoxelChunk> = world.chunks().collect();
    chunks.sort_by_key(|chunk| (chunk.coord.x, chunk.coord.y, chunk.coord.z));

    let mut block_keys = BTreeSet::new();
    let mut documents = Vec::with_capacity(chunks.len());
    let mut voxel_count = 0;

    for chunk in chunks {
        let mut voxels = Vec::with_capacity(chunk.filled());

        for y in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    let local = UVec3::new(x as u32, y as u32, z as u32);
                    let Some(voxel) = chunk.get(local) else {
                        continue;
                    };
                    // Flowing liquid is derived state: only sources are saved.
                    if world.fluid_level(chunk.origin() + local.as_ivec3()) != 0 {
                        continue;
                    }

                    let def = registry.get(voxel.block);
                    block_keys.insert(def.id.clone());
                    let sub = voxel.sub.map(|sub| {
                        let key = registry.get(sub.block).id.clone();
                        block_keys.insert(key.clone());
                        (key, sub.orientation.0)
                    });
                    voxels.push(VoxelDocument {
                        index: VoxelChunk::local_index(local) as u16,
                        block: def.id.clone(),
                        rot: voxel.orientation.0,
                        sub,
                    });
                    voxel_count += 1;
                }
            }
        }

        documents.push(ChunkDocument {
            coord: [chunk.coord.x, chunk.coord.y, chunk.coord.z],
            voxels,
        });
    }

    let document = WorldDocument {
        format_version: WORLD_FORMAT_VERSION,
        name: name.to_string(),
        blocks: block_keys.into_iter().collect(),
        chunks: documents,
    };

    save_ron(path, &document)?;

    Ok(SavedWorld {
        chunks: document.chunks.len(),
        voxels: voxel_count,
        path: path.to_path_buf(),
    })
}

/// Register the customised synthetic blocks (`base~look` keys) used by a
/// world file, so [`load_from_path`] finds them.
pub fn preload_variants(registry: &mut BlockRegistry, path: &Path) {
    let Ok(document) = load_ron::<WorldDocument>(path) else {
        return;
    };
    for key in document.blocks.iter().filter(|key| key.contains('~')) {
        registry.ensure_variant(key);
    }
    for chunk in &document.chunks {
        for cell in &chunk.voxels {
            if cell.block.contains('~') {
                registry.ensure_variant(&cell.block);
            }
            if let Some((key, _)) = &cell.sub {
                if key.contains('~') {
                    registry.ensure_variant(key);
                }
            }
        }
    }
}

/// Read a world file and replace the in-memory world with its content.
pub fn load_from_path(
    world: &mut VoxelWorld,
    registry: &BlockRegistry,
    path: &Path,
) -> Result<LoadedWorld, String> {
    let document: WorldDocument = load_ron(path)?;

    if document.format_version != WORLD_FORMAT_VERSION {
        return Err(format!(
            "unsupported world format v{} (expected v{WORLD_FORMAT_VERSION})",
            document.format_version
        ));
    }

    world.clear();

    let mut voxels = 0;
    let mut skipped = 0;

    for chunk in &document.chunks {
        let origin = IVec3::new(chunk.coord[0], chunk.coord[1], chunk.coord[2])
            * CHUNK_SIZE as i32;

        for cell in &chunk.voxels {
            let index = cell.index as usize;
            if index >= CHUNK_VOLUME {
                skipped += 1;
                continue;
            }

            // Unknown customised synthetic blocks fall back on their base block.
            let resolve = |key: &str| registry.find(key).or_else(|| key.split_once('~').and_then(|(base, _)| registry.find(base)));
            let Some(block) = resolve(&cell.block) else {
                skipped += 1;
                continue;
            };

            let local = VoxelChunk::local_from_index(index).as_ivec3();
            let mut voxel = Voxel::oriented(block, Orientation(cell.rot % 24));
            if let Some((key, rot)) = &cell.sub {
                if let Some(sub_block) = resolve(key) {
                    voxel = voxel.with_sub(SubVoxel {
                        block: sub_block,
                        orientation: Orientation(*rot % 24),
                    });
                }
            }
            if world.set(origin + local, Some(voxel)) {
                voxels += 1;
            } else {
                skipped += 1;
            }
        }
    }

    Ok(LoadedWorld {
        name: document.name,
        chunks: document.chunks.len(),
        voxels,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "polyhedron_{name}_{}.ron",
            std::process::id()
        ))
    }

    #[test]
    fn world_document_round_trips_through_ron() {
        let registry = BlockRegistry::builtin();
        let grass = registry.id_or_default("grass");
        let stone = registry.id_or_default("stone");

        let mut world = VoxelWorld::new();
        world.set(IVec3::new(0, 0, 0), Some(Voxel::new(grass)));
        world.set(IVec3::new(-1, 17, 3), Some(Voxel::new(stone)));

        let path = temporary_path("round_trip");
        let saved = save_to_path(&world, &registry, &path, "round_trip").expect("save world");
        assert_eq!(saved.voxels, 2);
        assert_eq!(saved.chunks, 2);

        let mut loaded = VoxelWorld::new();
        let info = load_from_path(&mut loaded, &registry, &path).expect("load world");
        assert_eq!(info.name, "round_trip");
        assert_eq!(info.voxels, 2);
        assert_eq!(info.skipped, 0);
        assert_eq!(loaded.get(IVec3::new(0, 0, 0)), Some(Voxel::new(grass)));
        assert_eq!(loaded.get(IVec3::new(-1, 17, 3)), Some(Voxel::new(stone)));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn unknown_blocks_and_invalid_indices_are_skipped() {
        let registry = BlockRegistry::builtin();
        let path = temporary_path("unknown_block");
        let source = "(\n    format_version: 1,\n    name: \"broken\",\n    blocks: [],\n    chunks: [\n        (\n            coord: (0, 0, 0),\n            voxels: [\n                (index: 0, block: \"missing\"),\n                (index: 5000, block: \"stone\"),\n            ],\n        ),\n    ],\n)\n";
        std::fs::write(&path, source).expect("write fixture");

        let mut world = VoxelWorld::new();
        let info = load_from_path(&mut world, &registry, &path).expect("load world");
        assert_eq!(info.voxels, 0);
        assert_eq!(info.skipped, 2);
        assert_eq!(world.stats().voxels, 0);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn saved_document_lists_used_block_keys() {
        let registry = BlockRegistry::builtin();
        let grass = registry.id_or_default("grass");
        let stone = registry.id_or_default("stone");

        let mut world = VoxelWorld::new();
        world.set(IVec3::ZERO, Some(Voxel::new(grass)));
        world.set(IVec3::X, Some(Voxel::new(stone)));

        let path = temporary_path("keys");
        save_to_path(&world, &registry, &path, "keys").expect("save world");
        let document: WorldDocument = load_ron(&path).expect("read document");

        assert_eq!(document.blocks, vec!["grass".to_string(), "stone".to_string()]);

        let _ = std::fs::remove_file(path);
    }
}
