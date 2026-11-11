//! Biome-driven terrain generator.
//!
//! Pure and thread-safe: a [`TerrainGenerator`] is built from the biome and
//! structure libraries (block keys resolved once), shared behind an `Arc`
//! and evaluated column by column on worker threads by the chunk manager.
//!
//! Per column:
//! 1. climate noise (temperature, humidity) selects the nearest biomes in
//!    climate space; their heights are blended over `blend` climate units so
//!    borders never form cliffs;
//! 2. continental + multi-octave relief noise give the surface height, and
//!    ridged noise raises mountain ranges in the biomes that have `peaks`,
//!    with bare rock on cliffs and snow on the summits;
//! 3. the dominant biome's landforms (ravines, hills, pits, spikes, patches,
//!    craters) reshape the column;
//! 4. layers: surface / subsurface / stone down to the bedrock, water (or ice)
//!    up to sea level;
//! 5. caves (when enabled): great halls with stone columns, stalactites and
//!    lakes, cheese caverns, spaghetti tunnels and noodle passages carved from
//!    3D noise sampled on a coarse lattice, a fluid near the floor, rock
//!    layers, ore clumps and veins and lush floors, all from the world settings.
//!
//! Structures are seeded per chunk column from a deterministic hash, checked
//! against their biome rule (chance, slope, ground block) and their anchors
//! (each anchor must rest on solid ground), then stamped into every chunk
//! they overlap, so streaming order never changes the result. Cave
//! structures fit inside their chunk column and stand on its carved floors.

use std::collections::HashMap;

use bevy::prelude::*;
use noise::{NoiseFn, Perlin};

use crate::voxel::biomes::{BiomeDef, BiomeLibrary, CaveStructureRule, LandShape, Landform, OreVein, WorldGenSettings};
use crate::voxel::blocks::{BlockId, BlockRegistry};
use crate::voxel::structures::{StructureDef, StructureLibrary};
use crate::voxel::world::{Voxel, CHUNK_SIZE};

/// Largest structure footprint the generator accepts (one chunk margin).
const MAX_STRUCTURE_FOOTPRINT: i32 = CHUNK_SIZE as i32;

#[derive(Clone, Debug)]
struct ResolvedBiome {
    def: BiomeDef,
    surface: BlockId,
    subsurface: BlockId,
    stone: BlockId,
    ground_blocks: Vec<Vec<BlockId>>,
    forms: Vec<ResolvedForm>,
    ores: Vec<ResolvedOre>,
}

#[derive(Clone, Debug)]
struct ResolvedForm {
    def: Landform,
    surface: Option<BlockId>,
    subsurface: Option<BlockId>,
    core: Option<BlockId>,
}

#[derive(Clone, Copy, Debug)]
struct ResolvedOre {
    block: BlockId,
    min_y: i32,
    max_y: i32,
    chance: f32,
    /// A clump is `1 << shift` voxels wide.
    shift: u32,
    /// Thickness of a vein (0 = clumps).
    width: f32,
    /// Which lines of the vein noise this ore follows.
    line: [f32; 2],
}

fn resolve_ores(ores: &[OreVein], registry: &BlockRegistry) -> Vec<ResolvedOre> {
    ores.iter()
        .filter_map(|ore| {
            Some(ResolvedOre {
                block: registry.find(&ore.block)?,
                min_y: ore.min_y,
                max_y: ore.max_y,
                chance: ore.chance,
                shift: ore.size.clamp(1, 8).ilog2(),
                width: ore.width,
                line: {
                    let hash = ore.block.bytes().fold(ore.min_y as u32, |hash, byte| hash.wrapping_mul(31).wrapping_add(byte as u32));
                    [unit(hash3(hash, 1, 2, 3)) * 0.8 - 0.4, unit(hash3(hash, 4, 5, 6)) * 0.8 - 0.4]
                },
            })
        })
        .collect()
}

#[derive(Clone, Debug)]
struct ResolvedStructure {
    size: IVec3,
    cells: Vec<(IVec3, Voxel)>,
    anchors: Vec<IVec3>,
}

/// Result of sampling one column.
#[derive(Clone, Copy, Debug)]
pub struct ColumnInfo {
    pub height: i32,
    pub biome: usize,
    pub temperature: f32,
    pub humidity: f32,
    pub surface: BlockId,
    pub subsurface: BlockId,
    pub stone: BlockId,
    pub subsurface_depth: i32,
    /// Extra ice spike above the surface.
    pub spike: i32,
    /// Block of the spike.
    pub spike_block: BlockId,
    /// Water surface frozen.
    pub frozen: bool,
}

pub struct TerrainGenerator {
    pub settings: WorldGenSettings,
    biomes: Vec<ResolvedBiome>,
    structures: HashMap<String, ResolvedStructure>,
    temperature: Perlin,
    humidity: Perlin,
    continental: Perlin,
    relief: Perlin,
    detail: Perlin,
    ranges: Perlin,
    ridges: Perlin,
    mountain_rock: BlockId,
    mountain_snow: BlockId,
    cave_rules: Vec<(CaveStructureRule, Vec<usize>)>,
    water: BlockId,
    ice: BlockId,
    rock: BlockId,
    beach: BlockId,
    layers: Vec<(BlockId, i32, u32)>,
    ores: Vec<ResolvedOre>,
    caves: CaveNoise,
}

/// Noise fields and blocks of the cave system.
struct CaveNoise {
    cheese: Perlin,
    tunnel_a: Perlin,
    tunnel_b: Perlin,
    noodle_a: Perlin,
    noodle_b: Perlin,
    entrance: Perlin,
    hall: Perlin,
    pillar: Perlin,
    spike: Perlin,
    vein_a: Perlin,
    vein_b: Perlin,
    fluid: BlockId,
    lake: BlockId,
    floor: Option<BlockId>,
    tuft: Option<BlockId>,
}

/// Lattice step of the cave noise (sampled coarsely, then interpolated).
const CAVE_STEP: i32 = 4;

/// Cave noise of one chunk column, sampled on a coarse lattice.
struct CaveField {
    base_y: i32,
    layers: i32,
    /// `[cheese, tunnel_a, tunnel_b, noodle_a, noodle_b, hall, vein_a, vein_b]`
    /// per lattice point.
    values: Vec<[f32; 8]>,
}

impl CaveField {
    const SIDE: i32 = CHUNK_SIZE as i32 / CAVE_STEP + 1;

    fn index(&self, ix: i32, iy: i32, iz: i32) -> usize {
        ((iy * Self::SIDE + iz) * Self::SIDE + ix) as usize
    }

    /// Trilinear interpolation at local `(lx, y, lz)`.
    fn sample(&self, lx: i32, y: i32, lz: i32) -> [f32; 8] {
        let fy = (y - self.base_y) as f32 / CAVE_STEP as f32;
        let fx = lx as f32 / CAVE_STEP as f32;
        let fz = lz as f32 / CAVE_STEP as f32;
        let (ix, iy, iz) = (fx.floor() as i32, fy.floor() as i32, fz.floor() as i32);
        let ix = ix.clamp(0, Self::SIDE - 2);
        let iz = iz.clamp(0, Self::SIDE - 2);
        let iy = iy.clamp(0, self.layers - 2);
        let (tx, ty, tz) = (fx - ix as f32, fy - iy as f32, fz - iz as f32);
        let mut out = [0.0; 8];
        for (corner, weight) in [
            ((0, 0, 0), (1.0 - tx) * (1.0 - ty) * (1.0 - tz)),
            ((1, 0, 0), tx * (1.0 - ty) * (1.0 - tz)),
            ((0, 1, 0), (1.0 - tx) * ty * (1.0 - tz)),
            ((1, 1, 0), tx * ty * (1.0 - tz)),
            ((0, 0, 1), (1.0 - tx) * (1.0 - ty) * tz),
            ((1, 0, 1), tx * (1.0 - ty) * tz),
            ((0, 1, 1), (1.0 - tx) * ty * tz),
            ((1, 1, 1), tx * ty * tz),
        ] {
            let v = &self.values[self.index(ix + corner.0, iy + corner.1, iz + corner.2)];
            for (o, value) in out.iter_mut().zip(v) {
                *o += value * weight;
            }
        }
        out
    }
}

fn fbm(noise: &Perlin, x: f64, z: f64, octaves: u32, persistence: f64) -> f64 {
    let mut total = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut norm = 0.0;
    for _ in 0..octaves.clamp(1, 8) {
        total += noise.get([x * frequency, z * frequency]) * amplitude;
        norm += amplitude;
        amplitude *= persistence;
        frequency *= 2.0;
    }
    total / norm.max(1e-6)
}

fn hash3(seed: u32, x: i32, z: i32, salt: u32) -> u32 {
    let mut h = seed ^ salt.wrapping_mul(0x9E37_79B9);
    h ^= (x as u32).wrapping_mul(0x85EB_CA6B);
    h = h.rotate_left(13) ^ (z as u32).wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^ (h >> 16)
}

/// The structure a rule places, for a roll in `[0, 1]`.
fn pick_variant(rule: &crate::voxel::biomes::StructureRule, roll: f32) -> &str {
    let total: f32 = rule.variants.iter().map(|variant| variant.weight.max(0.0)).sum();
    let mut left = roll * total;
    for variant in &rule.variants {
        left -= variant.weight.max(0.0);
        if left <= 0.0 && variant.weight > 0.0 {
            return &variant.structure;
        }
    }
    &rule.structure
}

fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low).max(1e-5)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn unit(hash: u32) -> f32 {
    (hash & 0xFF_FFFF) as f32 / 0xFF_FFFF as f32
}

impl TerrainGenerator {
    pub fn new(library: &BiomeLibrary, structures: &StructureLibrary, registry: &BlockRegistry) -> Self {
        let settings = library.settings.clone();
        let seed = settings.seed;
        let biomes = library
            .biomes
            .iter()
            .map(|def| ResolvedBiome {
                surface: registry.id_or_default(&def.surface_block),
                subsurface: registry.id_or_default(&def.subsurface_block),
                stone: registry.id_or_default(&def.stone_block),
                ground_blocks: def
                    .structures
                    .iter()
                    .map(|rule| rule.ground.iter().filter_map(|g| registry.find(g)).collect())
                    .collect(),
                forms: def
                    .forms()
                    .into_iter()
                    .map(|form| ResolvedForm {
                        surface: form.surface.as_deref().and_then(|name| registry.find(name)),
                        subsurface: form.subsurface.as_deref().and_then(|name| registry.find(name)),
                        core: form.core.as_deref().and_then(|name| registry.find(name)),
                        def: form,
                    })
                    .collect(),
                ores: resolve_ores(&def.ores, registry),
                def: def.clone(),
            })
            .collect();
        let structures = structures
            .structures
            .iter()
            .filter(|s| s.size[0] <= MAX_STRUCTURE_FOOTPRINT && s.size[2] <= MAX_STRUCTURE_FOOTPRINT)
            .map(|s: &StructureDef| {
                (
                    s.name.clone(),
                    ResolvedStructure {
                        size: IVec3::from_array(s.size),
                        cells: s
                            .cells
                            .iter()
                            .filter_map(|cell| {
                                StructureDef::voxel(cell, registry).map(|v| (IVec3::from_array(cell.pos), v))
                            })
                            .collect(),
                        anchors: s.effective_anchors(),
                    },
                )
            })
            .collect();

        Self {
            settings: settings.clone(),
            biomes,
            structures,
            temperature: Perlin::new(seed),
            humidity: Perlin::new(seed.wrapping_add(101)),
            continental: Perlin::new(seed.wrapping_add(202)),
            relief: Perlin::new(seed.wrapping_add(303)),
            detail: Perlin::new(seed.wrapping_add(404)),
            ranges: Perlin::new(seed.wrapping_add(1111)),
            ridges: Perlin::new(seed.wrapping_add(1212)),
            mountain_rock: registry.id_or_default(&settings.mountains.rock),
            mountain_snow: registry.id_or_default(&settings.mountains.snow),
            cave_rules: settings
                .cave_structures
                .iter()
                .map(|rule| {
                    let biomes = library.biomes.iter().enumerate().filter(|(_, biome)| rule.biomes.contains(&biome.name)).map(|(index, _)| index).collect();
                    (rule.clone(), biomes)
                })
                .collect(),
            water: registry.id_or_default(&settings.blocks.water),
            ice: registry.id_or_default(&settings.blocks.ice),
            rock: registry.id_or_default(&settings.blocks.rock),
            beach: registry.id_or_default(&settings.blocks.beach),
            layers: settings.layers.iter().filter_map(|layer| Some((registry.find(&layer.block)?, layer.below_y, layer.dither))).collect(),
            ores: resolve_ores(&settings.ores, registry),
            caves: CaveNoise {
                cheese: Perlin::new(seed.wrapping_add(505)),
                tunnel_a: Perlin::new(seed.wrapping_add(606)),
                tunnel_b: Perlin::new(seed.wrapping_add(707)),
                noodle_a: Perlin::new(seed.wrapping_add(808)),
                noodle_b: Perlin::new(seed.wrapping_add(909)),
                entrance: Perlin::new(seed.wrapping_add(1010)),
                hall: Perlin::new(seed.wrapping_add(1313)),
                pillar: Perlin::new(seed.wrapping_add(1414)),
                spike: Perlin::new(seed.wrapping_add(1515)),
                vein_a: Perlin::new(seed.wrapping_add(1616)),
                vein_b: Perlin::new(seed.wrapping_add(1717)),
                fluid: registry.id_or_default(&settings.cave.fluid),
                lake: registry.id_or_default(&settings.cave.lake_fluid),
                floor: registry.find(&settings.cave.floor),
                tuft: registry.find(&settings.cave.tuft),
            },
        }
    }

    pub fn biome_name(&self, index: usize) -> &str {
        self.biomes.get(index).map(|b| b.def.name.as_str()).unwrap_or("?")
    }

    /// Dominant biome of a column from the climate alone (cheap).
    pub fn biome_at(&self, x: i32, z: i32) -> usize {
        let (temperature, humidity) = self.climate(x, z);
        self.biomes
            .iter()
            .enumerate()
            .map(|(index, biome)| {
                (index, Vec2::new(biome.def.temperature - temperature, biome.def.humidity - humidity).length())
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    pub fn biome_sky(&self, index: usize) -> Vec3 {
        self.biomes
            .get(index)
            .map(|b| Vec3::from_array(b.def.sky_color))
            .unwrap_or(Vec3::new(0.5, 0.7, 0.9))
    }

    /// Climate at a world column, both in roughly `[-1, 1]`.
    pub fn climate(&self, x: i32, z: i32) -> (f32, f32) {
        let scale = self.settings.climate_scale.max(16.0) as f64;
        let t = fbm(&self.temperature, x as f64 / scale, z as f64 / scale, 3, 0.5) * 1.6;
        let h = fbm(&self.humidity, x as f64 / scale + 31.7, z as f64 / scale - 12.3, 3, 0.5) * 1.6;
        (t.clamp(-1.0, 1.0) as f32, h.clamp(-1.0, 1.0) as f32)
    }

    /// Mountain range at a column, in `[0, 1]`: ridged noise, bent so the
    /// ridges never line up, and lower between the ranges.
    fn range_at(&self, x: f32, z: f32) -> f32 {
        let mountains = &self.settings.mountains;
        let scale = mountains.scale.max(16.0) as f64;
        let (fx, fz) = (x as f64 / scale, z as f64 / scale);
        let bent_x = fx + self.ranges.get([fz * 0.8 + 11.3, fx * 0.8]) * 0.4;
        let bent_z = fz + self.ranges.get([fx * 0.8 - 4.7, fz * 0.8]) * 0.4;
        let (mut total, mut amplitude, mut frequency, mut weight) = (0.0f32, 0.55f32, 1.0f64, 1.0f32);
        for _ in 0..5 {
            let ridge = 1.0 - self.ridges.get([bent_x * frequency, bent_z * frequency]).abs() as f32;
            let ridge = ridge * ridge;
            total += ridge * amplitude * weight;
            weight = (ridge * 1.8).clamp(0.0, 1.0);
            amplitude *= 0.5;
            frequency *= 2.03;
        }
        let spread = mountains.range_scale.max(32.0) as f64;
        let range = fbm(&self.ranges, x as f64 / spread + 3.1, z as f64 / spread, 2, 0.5) as f32;
        let shape = total.clamp(0.0, 1.0).powf(mountains.sharpness.max(0.2)) * (0.35 + 0.65 * smoothstep(-0.3, 0.35, range));
        // Crags: nothing in the valleys, full strength from mid slope.
        let crags = fbm(&self.ridges, x as f64 / 34.0 + 7.7, z as f64 / 34.0, 3, 0.55) as f32;
        (shape + smoothstep(0.02, 0.3, shape) * crags * mountains.crag / mountains.height.max(1.0)).max(0.0)
    }

    /// Bare rock on cliffs and high ground, snow on the summits.
    fn dress_mountain(&self, x: i32, z: i32, rise: f32, info: &mut ColumnInfo) {
        let mountains = &self.settings.mountains;
        let (fx, fz) = (x as f32, z as f32);
        let east = self.range_at(fx + 2.0, fz) - self.range_at(fx - 2.0, fz);
        let south = self.range_at(fx, fz + 2.0) - self.range_at(fx, fz - 2.0);
        let slope = east.hypot(south) * rise / 4.0;
        let wobble = (hash3(self.settings.seed, x, z, 93) % 9) as i32 - 4;
        let altitude = info.height + wobble;
        let cliff = slope > mountains.cliff;
        if altitude >= mountains.snow_line && slope < mountains.cliff * 1.5 {
            info.surface = self.mountain_snow;
            info.subsurface = self.mountain_rock;
        } else if altitude >= mountains.rock_line || cliff {
            info.surface = self.mountain_rock;
            info.subsurface = self.mountain_rock;
        }
    }

    fn raw_height(&self, biome: &BiomeDef, x: i32, z: i32, continental: f32) -> f32 {
        let scale = self.settings.relief_scale.max(8.0) as f64;
        let relief = fbm(
            &self.relief,
            x as f64 / scale,
            z as f64 / scale,
            biome.octaves,
            biome.roughness.clamp(0.05, 0.95) as f64,
        ) as f32;
        biome.base_height + relief * biome.height_scale + continental * biome.continentalness * 10.0
    }

    pub fn column(&self, x: i32, z: i32) -> ColumnInfo {
        let (temperature, humidity) = self.climate(x, z);
        let fallback = ColumnInfo {
            height: self.settings.sea_level,
            biome: 0,
            temperature,
            humidity,
            surface: self.beach,
            subsurface: self.beach,
            stone: self.rock,
            subsurface_depth: 3,
            spike: 0,
            spike_block: self.ice,
            frozen: false,
        };
        if self.biomes.is_empty() {
            return fallback;
        }

        let cscale = self.settings.continental_scale.max(32.0) as f64;
        let continental = fbm(&self.continental, x as f64 / cscale, z as f64 / cscale, 2, 0.5) as f32;

        // Distance of every biome in climate space.
        let mut distances: Vec<(usize, f32)> = self
            .biomes
            .iter()
            .enumerate()
            .map(|(index, biome)| {
                let d = Vec2::new(biome.def.temperature - temperature, biome.def.humidity - humidity).length();
                (index, d)
            })
            .collect();
        distances.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (dominant, nearest) = distances[0];
        let blend = self.settings.blend.max(0.01);

        let mut weight_sum = 0.0;
        let mut height = 0.0;
        let mut peaks = 0.0;
        for (index, distance) in distances.iter().take(4) {
            let weight = (1.0 - (distance - nearest) / blend).max(0.0);
            if weight <= 0.0 {
                continue;
            }
            let weight = weight * weight;
            height += self.raw_height(&self.biomes[*index].def, x, z, continental) * weight;
            peaks += self.biomes[*index].def.peaks * weight;
            weight_sum += weight;
        }
        let mut height = height / weight_sum.max(1e-5);
        // Blended like the relief, so a range fades out at the biome border.
        let rise = peaks / weight_sum.max(1e-5) * self.settings.mountains.height;
        if rise > 0.5 {
            height += self.range_at(x as f32, z as f32) * rise;
        }

        let biome = &self.biomes[dominant];
        let mut info = ColumnInfo {
            height: 0,
            biome: dominant,
            temperature,
            humidity,
            surface: biome.surface,
            subsurface: biome.subsurface,
            stone: biome.stone,
            subsurface_depth: biome.def.subsurface_depth.max(1),
            spike: 0,
            spike_block: self.ice,
            frozen: temperature < self.settings.freeze_below,
        };

        for form in &biome.forms {
            self.apply_landform(form, x, z, &mut height, &mut info);
        }

        // Beaches.
        let sea = self.settings.sea_level;
        info.height = height.round() as i32;
        if info.height <= sea + 1 && info.height >= sea - 2 && biome.def.beaches {
            if !info.frozen {
                info.surface = self.beach;
                info.subsurface = self.beach;
            }
        }
        if rise > 0.5 && info.height > sea + 1 {
            self.dress_mountain(x, z, rise, &mut info);
        }
        info
    }

    /// One landform on one column.
    fn apply_landform(&self, form: &ResolvedForm, x: i32, z: i32, height: &mut f32, info: &mut ColumnInfo) {
        let def = &form.def;
        let (fx, fz) = (x as f64, z as f64);
        let scale = def.scale.max(1.0) as f64;
        let seed = self.settings.seed;
        let threshold = def.threshold;
        match def.shape {
            LandShape::Freeze => info.frozen = true,
            LandShape::Ridge => {
                let ridge = self.detail.get([fx / scale, fz / scale]).abs() as f32;
                if ridge < threshold {
                    *height -= (threshold - ridge) / threshold.max(1e-4) * def.strength;
                }
            }
            LandShape::Hills | LandShape::Pit => {
                let cone = fbm(&self.detail, fx / scale + 7.0, fz / scale, 2, 0.5) as f32;
                if cone > threshold {
                    let amount = (cone - threshold) * def.strength;
                    *height += if def.shape == LandShape::Pit { -amount } else { amount };
                    info.surface = form.surface.unwrap_or(info.surface);
                    info.subsurface = form.subsurface.unwrap_or(info.subsurface);
                }
            }
            LandShape::Spikes => {
                let spike = self.detail.get([fx / scale, fz / scale]) as f32;
                if spike > threshold {
                    info.spike = ((spike - threshold) * def.strength) as i32;
                    info.spike_block = form.surface.unwrap_or(self.ice);
                }
            }
            LandShape::Patches => {
                let patch = self.detail.get([fx / scale, fz / scale]) as f32;
                if patch > threshold && (def.chance >= 1.0 || unit(hash3(seed, x, z, 92)) < def.chance) {
                    info.surface = form.surface.unwrap_or(info.surface);
                    info.subsurface = form.subsurface.unwrap_or(info.subsurface);
                }
            }
            LandShape::Craters => {
                let size = def.scale.max(4.0) as i32;
                let cell = IVec2::new(x.div_euclid(size), z.div_euclid(size));
                let mut depress = 0.0f32;
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let c = cell + IVec2::new(dx, dz);
                        let h = hash3(seed, c.x, c.y, 77);
                        if unit(h) < 1.0 - def.chance {
                            continue;
                        }
                        let center = Vec2::new(
                            (c.x * size) as f32 + unit(h.rotate_left(8)) * size as f32,
                            (c.y * size) as f32 + unit(h.rotate_left(16)) * size as f32,
                        );
                        let radius = threshold + unit(h.rotate_left(24)) * def.strength;
                        let d = Vec2::new(x as f32, z as f32).distance(center) / radius.max(0.5);
                        if d < 0.18 && unit(hash3(seed, x, z, 91)) < def.core_chance {
                            info.surface = form.core.unwrap_or(info.surface);
                        }
                        if d < 1.0 {
                            depress = depress.max((1.0 - d * d) * radius * 0.7);
                        } else if d < 1.25 {
                            depress = depress.min(-(1.25 - d) * 8.0);
                            info.surface = form.surface.unwrap_or(info.surface);
                        }
                    }
                }
                *height -= depress;
            }
        }
    }

    /// Block of a column at a height, or `None` for air.
    fn block_at(&self, info: &ColumnInfo, y: i32) -> Option<BlockId> {
        if y < self.settings.bedrock {
            return None;
        }
        let h = info.height;
        if y <= h {
            return Some(if y == h {
                info.surface
            } else if y > h - info.subsurface_depth {
                info.subsurface
            } else {
                info.stone
            });
        }
        if info.spike > 0 && y <= h + info.spike {
            return Some(info.spike_block);
        }
        let sea = self.settings.sea_level;
        if y <= sea {
            return Some(if info.frozen && y == sea { self.ice } else { self.water });
        }
        None
    }

    /// Highest Y of the generated column range.
    pub fn top(&self) -> i32 {
        let max_biome = self
            .biomes
            .iter()
            .map(|b| b.def.base_height + b.def.height_scale.abs() + 12.0 * b.def.continentalness + b.def.peaks.max(0.0) * self.settings.mountains.height)
            .fold(self.settings.sea_level as f32, f32::max);
        max_biome as i32 + 64
    }

    /// Generate every voxel of a chunk column (`coord` in chunk units).
    pub fn generate_column(&self, coord: IVec2) -> Vec<(IVec3, Voxel)> {
        let size = CHUNK_SIZE as i32;
        let origin = coord * size;
        let mut cells = Vec::with_capacity((size * size * 24) as usize);

        let mut infos = Vec::with_capacity((size * size) as usize);
        for lz in 0..size {
            for lx in 0..size {
                infos.push(self.column(origin.x + lx, origin.y + lz));
            }
        }
        let field = self.settings.caves.then(|| self.cave_field(origin, &infos));
        let bedrock = self.settings.bedrock;
        let mut column: Vec<Option<BlockId>> = Vec::new();
        // Kept for the cave structures, which stand on what was carved.
        let mut carved_columns = Vec::with_capacity(if self.cave_rules.is_empty() { 0 } else { (size * size) as usize });
        for lz in 0..size {
            for lx in 0..size {
                let info = &infos[(lz * size + lx) as usize];
                let top = info.height.max(self.settings.sea_level) + info.spike;
                let (x, z) = (origin.x + lx, origin.y + lz);
                column.clear();
                column.extend((bedrock..=top).map(|y| self.block_at(info, y)));
                if let Some(field) = &field {
                    self.carve_column(field, info, lx, lz, x, z, &mut column);
                }
                for (offset, block) in column.iter().enumerate() {
                    if let Some(block) = block {
                        cells.push((IVec3::new(x, bedrock + offset as i32, z), Voxel::new(*block)));
                    }
                }
                if field.is_some() && !self.cave_rules.is_empty() {
                    carved_columns.push(column.clone());
                }
            }
        }

        // Structures seeded in this column and its neighbours.
        let min = IVec2::new(origin.x, origin.y);
        let max = min + IVec2::splat(size);
        for dz in -1..=1 {
            for dx in -1..=1 {
                for (base, structure) in self.structures_in_column(coord + IVec2::new(dx, dz)) {
                    for (offset, voxel) in &structure.cells {
                        let cell = base + *offset;
                        if cell.x >= min.x && cell.x < max.x && cell.z >= min.y && cell.z < max.y {
                            cells.push((cell, *voxel));
                        }
                    }
                }
            }
        }
        if !carved_columns.is_empty() {
            for (base, structure) in self.structures_in_caves(coord, &infos, &carved_columns) {
                cells.extend(structure.cells.iter().map(|(offset, voxel)| (base + *offset, *voxel)));
            }
        }
        cells
    }

    /// Cave structures of a chunk column: a rule rolls a spot, the nearest
    /// floor (or ceiling) below it takes the structure if there is room.
    fn structures_in_caves(&self, coord: IVec2, infos: &[ColumnInfo], columns: &[Vec<Option<BlockId>>]) -> Vec<(IVec3, &ResolvedStructure)> {
        let size = CHUNK_SIZE as i32;
        let bedrock = self.settings.bedrock;
        let cell = |lx: i32, lz: i32, y: i32| -> Option<BlockId> {
            let column = &columns[(lz * size + lx) as usize];
            usize::try_from(y - bedrock).ok().and_then(|offset| column.get(offset).copied().flatten())
        };
        let open = |lx: i32, lz: i32, y: i32| -> bool {
            let column = &columns[(lz * size + lx) as usize];
            usize::try_from(y - bedrock).is_ok_and(|offset| offset < column.len() && column[offset].is_none())
        };
        let mut placements = Vec::new();
        let biome = infos[(size / 2 * size + size / 2) as usize].biome;
        for (index, (rule, biomes)) in self.cave_rules.iter().enumerate() {
            if !rule.biomes.is_empty() && !biomes.contains(&biome) {
                continue;
            }
            let Some(structure) = self.structures.get(&rule.structure) else {
                continue;
            };
            for attempt in 0..rule.attempts.clamp(1, 8) {
                let h = hash3(self.settings.seed ^ 0xCA7E, coord.x, coord.y, index as u32 * 37 + attempt);
                if unit(h) > rule.chance_per_chunk {
                    continue;
                }
                let lx = (unit(h.rotate_left(7)) * (size - structure.size.x + 1).max(1) as f32) as i32;
                let lz = (unit(h.rotate_left(13)) * (size - structure.size.z + 1).max(1) as f32) as i32;
                let (mx, mz) = (lx + structure.size.x / 2, lz + structure.size.z / 2);
                let start = rule.min_y + (unit(h.rotate_left(21)) * (rule.max_y - rule.min_y).max(0) as f32) as i32;
                // Walk down to the first open cell, then to what bounds it.
                let Some(top) = (rule.min_y..=start).rev().find(|y| open(mx, mz, *y)) else {
                    continue;
                };
                let base_y = if rule.ceiling {
                    let Some(roof) = (top + 1..top + 48).find(|y| !open(mx, mz, *y)) else {
                        continue;
                    };
                    if cell(mx, mz, roof).is_none() {
                        continue;
                    }
                    roof - structure.size.y
                } else {
                    let Some(floor) = (rule.min_y - 1..top).rev().find(|y| !open(mx, mz, *y)) else {
                        continue;
                    };
                    if cell(mx, mz, floor).is_none_or(|block| block == self.water || block == self.caves.lake || block == self.caves.fluid) {
                        continue;
                    }
                    floor + 1
                };
                // Room for the whole structure, and ground under its corners.
                let (hx, hz) = (lx + structure.size.x - 1, lz + structure.size.z - 1);
                let corners = [(lx, lz), (hx, lz), (lx, hz), (hx, hz), (mx, mz)];
                let roomy = corners.iter().all(|&(cx, cz)| (0..structure.size.y).step_by(2).all(|dy| open(cx.min(size - 1), cz.min(size - 1), base_y + dy)));
                let rest_y = if rule.ceiling { base_y + structure.size.y } else { base_y - 1 };
                let resting = corners.iter().filter(|&&(cx, cz)| (-2..=2).any(|dy| cell(cx.min(size - 1), cz.min(size - 1), rest_y + dy).is_some())).count() >= 4;
                if roomy && resting {
                    placements.push((IVec3::new(coord.x * size + lx, base_y, coord.y * size + lz), structure));
                }
            }
        }
        placements
    }

    /// Sample the cave noise lattice of a chunk column.
    fn cave_field(&self, origin: IVec2, infos: &[ColumnInfo]) -> CaveField {
        let bedrock = self.settings.bedrock;
        let max_height = infos.iter().map(|info| info.height).max().unwrap_or(bedrock) + CAVE_STEP;
        let base_y = bedrock.div_euclid(CAVE_STEP) * CAVE_STEP;
        let layers = ((max_height - base_y) / CAVE_STEP + 2).max(2);
        let side = CaveField::SIDE;
        let noise = &self.caves;
        let cavern = self.settings.cave.cavern_scale.max(0.1) as f64;
        let tunnel = self.settings.cave.tunnel_scale.max(0.1) as f64;
        let hall = self.settings.cave.hall_scale.max(0.1) as f64;
        let vein = self.settings.vein_scale.max(4.0) as f64;
        let mut values = Vec::with_capacity((side * side * layers) as usize);
        for iy in 0..layers {
            for iz in 0..side {
                for ix in 0..side {
                    let x = (origin.x + ix * CAVE_STEP) as f64;
                    let y = (base_y + iy * CAVE_STEP) as f64;
                    let z = (origin.y + iz * CAVE_STEP) as f64;
                    values.push([
                        // Cheese: big caverns, squashed vertically.
                        (noise.cheese.get([x / (70.0 * cavern), y / (38.0 * cavern), z / (70.0 * cavern)])
                            + noise.cheese.get([x / (23.0 * cavern) + 5.1, y / (15.0 * cavern), z / (23.0 * cavern)]) * 0.3) as f32,
                        // Spaghetti: tunnels where two fields cross zero.
                        noise.tunnel_a.get([x / (64.0 * tunnel), y / (40.0 * tunnel), z / (64.0 * tunnel)]) as f32,
                        noise.tunnel_b.get([x / (64.0 * tunnel), y / (40.0 * tunnel), z / (64.0 * tunnel)]) as f32,
                        // Noodles: thin winding passages.
                        noise.noodle_a.get([x / 30.0, y / 26.0, z / 30.0]) as f32,
                        noise.noodle_b.get([x / 30.0, y / 26.0, z / 30.0]) as f32,
                        // Great halls: wide and low, with a rougher edge.
                        (noise.hall.get([x / (140.0 * hall), y / (62.0 * hall), z / (140.0 * hall)])
                            + noise.hall.get([x / (46.0 * hall) - 7.3, y / (28.0 * hall), z / (46.0 * hall)]) * 0.32) as f32,
                        // Ores follow the lines where these two fields hold one value.
                        noise.vein_a.get([x / vein, y / vein, z / vein]) as f32,
                        noise.vein_b.get([x / vein, y / vein, z / vein]) as f32,
                    ]);
                }
            }
        }
        CaveField { base_y, layers, values }
    }

    /// Carve caves into one column and decorate them (lava lakes near the
    /// floor, deepslate, ores in the walls, moss on humid cave floors).
    #[allow(clippy::too_many_arguments)]
    fn carve_column(
        &self,
        field: &CaveField,
        info: &ColumnInfo,
        lx: i32,
        lz: i32,
        x: i32,
        z: i32,
        column: &mut [Option<BlockId>],
    ) {
        let bedrock = self.settings.bedrock;
        let sea = self.settings.sea_level;
        let surface = info.height;
        let underwater = surface < sea;
        let entrance = self.caves.entrance.get([x as f64 / 90.0, z as f64 / 90.0]) as f32;
        let pillar_scale = self.settings.cave.pillar_scale.max(2.0) as f64;
        let pillar = self.caves.pillar.get([x as f64 / pillar_scale, z as f64 / pillar_scale]) as f32;
        let seed = self.settings.seed;
        let cave = &self.settings.cave;
        let mut carved = vec![false; column.len()];

        for (offset, block) in column.iter_mut().enumerate() {
            let y = bedrock + offset as i32;
            if y <= bedrock + 1 || y > surface || block.is_none() {
                continue;
            }
            let depth = surface - y;
            // Never open the sea floor (water would pour in).
            if underwater && depth < 6 {
                continue;
            }
            let [cheese, ta, tb, na, nb, hall, ..] = field.sample(lx, y, lz);
            let deep = ((-(y - sea) as f32) / 40.0).clamp(0.0, 1.0);
            let mut hollow = false;
            if depth > 7 + (y - sea).max(0) / 3 {
                hollow |= cheese > cave.cavern_rarity - 0.14 * deep;
            }
            // A hall fades out towards the top and bottom of its band; its
            // columns are thinner halfway up.
            let band = 1.0 - ((y - cave.hall_y) as f32 / cave.hall_height.max(1) as f32).powi(2);
            if band > 0.0 && depth > 12 && pillar <= cave.pillar_rarity + 0.14 * band {
                hollow |= hall - (1.0 - band) * 0.55 > cave.hall_rarity;
            }
            let near_surface_ok = depth > 3 || entrance > cave.entrance_rarity;
            if near_surface_ok {
                let width = cave.tunnel_width + 0.02 * deep;
                hollow |= ta.abs() < width && tb.abs() < width;
                hollow |= depth > 2 && na.abs() < cave.noodle_width && nb.abs() < cave.noodle_width;
            }
            if hollow {
                carved[offset] = true;
                *block = if y <= bedrock + cave.fluid_depth {
                    Some(self.caves.fluid)
                } else if y <= cave.lake_level {
                    Some(self.caves.lake)
                } else {
                    None
                };
            }
        }

        // Stalagmites and stalactites of the tall spaces: taller at the noise
        // peaks, so they come out as cones. They are rock again, for the pass below.
        let spike_at = |offset: f64| {
            let noise = self.caves.spike.get([x as f64 / 5.0 + offset, z as f64 / 5.0]) as f32;
            ((noise - cave.spike_rarity) / (1.0 - cave.spike_rarity).max(0.05) * 1.6 * cave.spike_length as f32).max(0.0) as usize
        };
        let (rising, hanging) = (spike_at(0.0), spike_at(37.0));
        let mut offset = 0;
        while (rising > 0 || hanging > 0) && offset < column.len() {
            let start = offset;
            while offset < column.len() && carved[offset] && column[offset].is_none() {
                offset += 1;
            }
            let length = offset - start;
            if length >= 10 && bedrock + (offset as i32) < surface - 2 {
                for cell in start..start + rising.min(length / 2) {
                    carved[cell] = false;
                    column[cell] = Some(info.stone);
                }
                for cell in offset - hanging.min(length / 2)..offset {
                    carved[cell] = false;
                    column[cell] = Some(info.stone);
                }
            }
            offset = offset.max(start + 1);
        }

        let humid = info.humidity > cave.floor_humidity;
        let biome_ores = self.biomes.get(info.biome).map_or(&[][..], |biome| &biome.ores[..]);
        let veins = self.ores.iter().chain(biome_ores).any(|ore| ore.width > 0.0);
        for offset in 0..column.len() {
            let y = bedrock + offset as i32;
            let Some(block) = column[offset] else {
                continue;
            };
            if carved[offset] || y > surface - info.subsurface_depth {
                continue;
            }
            let is_stone = block == info.stone;
            if !is_stone {
                continue;
            }
            // Rock layers, with an irregular border.
            let mut stone = block;
            for &(layer, below_y, dither) in &self.layers {
                let wobble = (hash3(seed, x, z, (y as u32).wrapping_mul(31)) % dither.max(1)) as i32;
                if y < below_y + wobble {
                    stone = layer;
                    break;
                }
            }

            // Long veins and clumps, a little irregular.
            let fine = unit(hash3(seed ^ 0xA11, x, z, (y as u32).wrapping_mul(0x9E37)));
            let lines = veins.then(|| field.sample(lx, y, lz));
            for (index, ore) in self.ores.iter().chain(biome_ores).enumerate() {
                if y < ore.min_y || y > ore.max_y {
                    continue;
                }
                if ore.width > 0.0 {
                    let [.., a, b] = lines.unwrap_or_default();
                    if (a - ore.line[0]).abs() < ore.width && (b - ore.line[1]).abs() < ore.width && fine < ore.chance {
                        stone = ore.block;
                        break;
                    }
                    continue;
                }
                let salt = 0x51ED ^ (index as u32).wrapping_mul(0x1F3D_5B79);
                let roll = unit(hash3(seed ^ salt, x >> ore.shift, z >> ore.shift, ((y >> ore.shift) as u32).wrapping_mul(0x2545_F491)));
                if fine < 0.55 && roll < ore.chance {
                    stone = ore.block;
                    break;
                }
            }
            column[offset] = Some(stone);

            // Lush cave floors, with tufts on top.
            let above_open = offset + 1 < column.len() && carved[offset + 1] && column[offset + 1].is_none();
            if humid && above_open && y > bedrock + cave.fluid_depth {
                if let Some(floor) = self.caves.floor {
                    column[offset] = Some(floor);
                }
                if fine > 1.0 - cave.tuft_chance {
                    if let Some(tuft) = self.caves.tuft {
                        column[offset + 1] = Some(tuft);
                    }
                }
            }
        }
    }

    /// Structures seeded by one chunk column (biome studio statistics).
    pub fn structure_count(&self, coord: IVec2) -> usize {
        self.structures_in_column(coord).len()
    }

    /// Placements seeded by one chunk column, after every rule check.
    fn structures_in_column(&self, coord: IVec2) -> Vec<(IVec3, &ResolvedStructure)> {
        let size = CHUNK_SIZE as i32;
        let center = coord * size + IVec2::splat(size / 2);
        let info = self.column(center.x, center.y);
        let Some(biome) = self.biomes.get(info.biome) else {
            return Vec::new();
        };
        let mut placements = Vec::new();

        for (rule_index, rule) in biome.def.structures.iter().enumerate() {
            for attempt in 0..rule.attempts.clamp(1, 8) {
                let h = hash3(self.settings.seed, coord.x, coord.y, (rule_index as u32) * 31 + attempt);
                if unit(h) > rule.chance_per_chunk {
                    continue;
                }
                let name = pick_variant(rule, unit(h.rotate_left(27)));
                let Some(structure) = self.structures.get(name) else {
                    continue;
                };
                let x = coord.x * size + (unit(h.rotate_left(7)) * size as f32) as i32;
                let z = coord.y * size + (unit(h.rotate_left(19)) * size as f32) as i32;
                let ground = self.column(x, z);
                if ground.biome != info.biome || ground.height <= self.settings.sea_level || ground.spike > 0 {
                    continue;
                }
                let allowed = &biome.ground_blocks[rule_index];
                if !allowed.is_empty() && !allowed.contains(&ground.surface) {
                    continue;
                }
                // Slope over the footprint corners.
                let footprint = [
                    IVec2::ZERO,
                    IVec2::new(structure.size.x - 1, 0),
                    IVec2::new(0, structure.size.z - 1),
                    IVec2::new(structure.size.x - 1, structure.size.z - 1),
                ];
                let heights: Vec<i32> = footprint
                    .iter()
                    .map(|offset| self.column(x + offset.x, z + offset.y).height)
                    .collect();
                let (lo, hi) = (heights.iter().min().copied().unwrap_or(0), heights.iter().max().copied().unwrap_or(0));
                if hi - lo > rule.max_slope.max(0) {
                    continue;
                }
                // Anchors must rest on solid ground.
                let base = IVec3::new(x, ground.height + 1, z);
                let anchored = structure.anchors.iter().all(|anchor| {
                    let column = self.column(base.x + anchor.x, base.z + anchor.z);
                    column.height >= base.y + anchor.y - 1 && column.height <= base.y + anchor.y
                });
                if anchored {
                    placements.push((base, structure));
                }
            }
        }
        placements
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::biomes::BiomeLibrary;
    use crate::voxel::structures::{default_structures, StructureLibrary};

    fn generator() -> TerrainGenerator {
        let registry = BlockRegistry::builtin();
        let biomes = BiomeLibrary {
            biomes: crate::voxel::biomes::default_biomes(),
            ..Default::default()
        };
        let structures = StructureLibrary {
            structures: default_structures(),
            ..Default::default()
        };
        TerrainGenerator::new(&biomes, &structures, &registry)
    }

    #[test]
    fn generation_is_deterministic() {
        let a = generator().generate_column(IVec2::new(3, -2));
        let b = generator().generate_column(IVec2::new(3, -2));
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }

    #[test]
    fn caves_carve_air_below_the_surface() {
        let generator = generator();
        let mut carved = 0;
        for cx in -3..3 {
            for cz in -3..3 {
                let cells = generator.generate_column(IVec2::new(cx, cz));
                let solid: std::collections::HashSet<IVec3> = cells.iter().map(|(cell, _)| *cell).collect();
                for lx in 0..CHUNK_SIZE as i32 {
                    for lz in 0..CHUNK_SIZE as i32 {
                        let (x, z) = (cx * CHUNK_SIZE as i32 + lx, cz * CHUNK_SIZE as i32 + lz);
                        let info = generator.column(x, z);
                        for y in generator.settings.bedrock + 2..info.height - 8 {
                            if !solid.contains(&IVec3::new(x, y, z)) {
                                carved += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(carved > 500, "only {carved} carved cells");
    }

    #[test]
    fn data_ores_and_layers_reach_the_world() {
        let registry = BlockRegistry::builtin();
        let mut biomes = BiomeLibrary { biomes: crate::voxel::biomes::default_biomes(), ..Default::default() };
        biomes.settings.ores = vec![crate::voxel::biomes::OreVein { block: "gold_block".to_string(), min_y: -96, max_y: 200, chance: 0.5, size: 1, width: 0.0 }];
        biomes.settings.layers = vec![crate::voxel::biomes::RockLayer { block: "sand".to_string(), below_y: 200, dither: 1 }];
        let block = registry.find("gold_block").or(registry.find("stone")).unwrap();
        biomes.settings.ores[0].block = registry.get(block).id.clone();
        let structures = StructureLibrary::default();
        let generator = TerrainGenerator::new(&biomes, &structures, &registry);
        let cells = generator.generate_column(IVec2::ZERO);
        let sand = registry.find("sand").unwrap();
        assert!(cells.iter().any(|(_, voxel)| voxel.block == block));
        assert!(cells.iter().filter(|(_, voxel)| voxel.block == sand).count() > 500);
    }

    #[test]
    fn presets_become_landforms_and_weights_pick_a_variant() {
        use crate::voxel::biomes::{BiomeFeature, StructureRule, WeightedStructure};
        assert_eq!(BiomeFeature::Volcano.preset().len(), 2);
        let weighted = |name: &str, weight| WeightedStructure { structure: name.to_string(), weight };
        let rule = StructureRule {
            structure: "base".to_string(),
            chance_per_chunk: 1.0,
            attempts: 1,
            max_slope: 2,
            ground: Vec::new(),
            variants: vec![weighted("a", 1.0), weighted("b", 3.0)],
        };
        assert_eq!(pick_variant(&rule, 0.1), "a");
        assert_eq!(pick_variant(&rule, 0.9), "b");
    }

    /// A generator of one mountain biome, with the given caves.
    fn mountain() -> TerrainGenerator {
        let mut biomes = crate::voxel::biomes::default_biomes();
        biomes.retain(|biome| biome.name == "Mountain");
        let library = BiomeLibrary { biomes, ..Default::default() };
        TerrainGenerator::new(&library, &StructureLibrary::default(), &BlockRegistry::builtin())
    }

    #[test]
    fn mountains_rise_high_and_end_in_snow() {
        let generator = mountain();
        let snow = generator.mountain_snow;
        let (mut highest, mut snowy) = (i32::MIN, 0);
        for x in (-1500..1500).step_by(23) {
            for z in (-1500..1500).step_by(23) {
                let info = generator.column(x, z);
                highest = highest.max(info.height);
                snowy += usize::from(info.surface == snow);
            }
        }
        assert!(highest > 120, "highest summit {highest}");
        assert!(highest < generator.top(), "{highest} above the top of the world");
        assert!(snowy > 0);
    }

    #[test]
    fn great_halls_are_tall_and_wide() {
        let generator = mountain();
        let (mut longest, mut air) = (0, 0);
        for coord in (-6..6).flat_map(|x| (-6..6).map(move |z| IVec2::new(x * 3, z * 3))) {
            let cells = generator.generate_column(coord);
            let solid: std::collections::HashSet<IVec3> = cells.iter().map(|(cell, _)| *cell).collect();
            for (lx, lz) in [(4, 4), (11, 11)] {
                let (x, z) = (coord.x * 16 + lx, coord.y * 16 + lz);
                let mut run = 0;
                for y in -90..-10 {
                    if solid.contains(&IVec3::new(x, y, z)) {
                        run = 0;
                    } else {
                        run += 1;
                        air += 1;
                        longest = longest.max(run);
                    }
                }
            }
        }
        assert!(longest >= 30, "tallest hall {longest}");
        assert!(air > 1000, "only {air} open cells");
    }

    #[test]
    fn veins_are_long_lines_of_ore() {
        let registry = BlockRegistry::builtin();
        let mut biomes = BiomeLibrary { biomes: crate::voxel::biomes::default_biomes(), ..Default::default() };
        let block = registry.get(registry.find("mineral_ore").or(registry.find("stone")).unwrap()).id.clone();
        biomes.settings.ores = vec![OreVein { block: block.clone(), min_y: -96, max_y: 200, chance: 1.0, size: 1, width: 0.08 }];
        let generator = TerrainGenerator::new(&biomes, &StructureLibrary::default(), &registry);
        let id = registry.find(&block).unwrap();
        let ore: Vec<IVec3> = (-2..2)
            .flat_map(|x| (-2..2).map(move |z| IVec2::new(x, z)))
            .flat_map(|coord| generator.generate_column(coord))
            .filter(|(_, voxel)| voxel.block == id)
            .map(|(cell, _)| cell)
            .collect();
        assert!(ore.len() > 200, "only {} cells of ore", ore.len());
        let span = ore.iter().map(|cell| cell.x).max().unwrap() - ore.iter().map(|cell| cell.x).min().unwrap();
        assert!(span > 32, "vein spans {span} cells");
    }

    #[test]
    fn cave_structures_stand_in_the_caves() {
        use crate::voxel::biomes::CaveStructureRule;
        use crate::voxel::structures::StructureDef;
        let registry = BlockRegistry::builtin();
        let mut biomes = crate::voxel::biomes::default_biomes();
        biomes.retain(|biome| biome.name == "Mountain");
        let mut library = BiomeLibrary { biomes, ..Default::default() };
        let rule = CaveStructureRule { structure: "Marker".to_string(), chance_per_chunk: 1.0, attempts: 4, min_y: -80, max_y: -12, ..Default::default() };
        library.settings.cave_structures = vec![rule];
        let mut marker = StructureDef::new("Marker", [1, 2, 1]);
        marker.put(0, 0, 0, "stone");
        marker.put(0, 1, 0, "stone");
        let structures = StructureLibrary { structures: vec![marker], ..Default::default() };
        let generator = TerrainGenerator::new(&library, &structures, &registry);
        let found: usize = (-6..6)
            .flat_map(|x| (-6..6).map(move |z| IVec2::new(x, z)))
            .map(|coord| {
                let plain = TerrainGenerator::new(&BiomeLibrary { settings: Default::default(), biomes: library.biomes.clone(), ..Default::default() }, &StructureLibrary::default(), &registry);
                generator.generate_column(coord).len().saturating_sub(plain.generate_column(coord).len())
            })
            .sum();
        assert!(found > 0, "no cave structure placed");
    }

    /// The shipped data: every structure parses, uses real blocks and fits
    /// the generator, and every rule names a real structure.
    #[test]
    fn project_structures_and_rules_are_complete() {
        use crate::voxel::biomes::WORLD_SETTINGS_FILE;
        use crate::voxel::structures::StructureDef;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("project");
        let texts = |folder: &str| -> Vec<(String, String)> {
            std::fs::read_dir(root.join(folder)).unwrap().flatten().map(|file| (file.file_name().to_string_lossy().to_string(), std::fs::read_to_string(file.path()).unwrap())).collect()
        };
        let blocks: std::collections::HashSet<String> = texts("blocks").iter().map(|(name, _)| name.trim_end_matches(".ron").to_string()).collect();
        let structures: Vec<StructureDef> = texts("structures").iter().map(|(name, text)| ron::from_str(text).unwrap_or_else(|error| panic!("{name}: {error}"))).collect();
        let names: std::collections::HashSet<&str> = structures.iter().map(|s| s.name.as_str()).collect();
        for structure in &structures {
            assert!(structure.size[0] <= MAX_STRUCTURE_FOOTPRINT && structure.size[2] <= MAX_STRUCTURE_FOOTPRINT, "{} too wide", structure.name);
            assert!(!structure.cells.is_empty(), "{} is empty", structure.name);
            for cell in &structure.cells {
                assert!(blocks.contains(&cell.block), "{} uses unknown block {}", structure.name, cell.block);
            }
        }
        for (name, text) in texts("biomes") {
            if name == WORLD_SETTINGS_FILE {
                let settings: WorldGenSettings = ron::from_str(&text).unwrap();
                assert!(!settings.cave_structures.is_empty());
                for rule in &settings.cave_structures {
                    assert!(names.contains(rule.structure.as_str()), "unknown {}", rule.structure);
                }
            } else {
                let biome: BiomeDef = ron::from_str(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
                for rule in &biome.structures {
                    assert!(names.contains(rule.structure.as_str()), "{name}: unknown {}", rule.structure);
                    for variant in &rule.variants {
                        assert!(names.contains(variant.structure.as_str()), "{name}: unknown {}", variant.structure);
                    }
                }
            }
        }
    }

    #[test]
    fn every_column_has_a_surface_above_bedrock() {
        let generator = generator();
        for x in (-200..200).step_by(37) {
            for z in (-200..200).step_by(41) {
                let info = generator.column(x, z);
                assert!(info.height > generator.settings.bedrock);
            }
        }
    }
}
