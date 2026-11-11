//! Data-driven biomes (`project/biomes/*.ron`).
//!
//! Each biome owns a point in climate space (temperature, humidity), its
//! multi-octave relief (base elevation, height scale, roughness, octaves,
//! continentalness influence), its block palette, an optional landform
//! feature (lake, canyon, volcano, ice spikes, alien craters, Mnemo
//! contamination) and the structures it may spawn with their rules.
//!
//! Global generation settings live in `project/biomes/_world.ron`.

use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::project_io::{load_ron, save_ron, ProjectLayout};

pub const WORLD_SETTINGS_FILE: &str = "_world.ron";

/// Special landform carved on top of the relief.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BiomeFeature {
    #[default]
    None,
    /// Depressions below sea level filled with water.
    Lake,
    /// Like `Lake`, but the surface freezes into ice.
    FrozenLake,
    /// Deep winding ravines.
    Canyon,
    /// Cone with a lava crater.
    Volcano,
    /// Tall ice spikes.
    IceSpikes,
    /// Alien mode: impact craters with crust rims.
    AlienCraters,
    /// Mnemo mode: glowing contamination patches.
    MnemoContamination,
}

impl BiomeFeature {
    pub const ALL: [Self; 8] = [
        Self::None,
        Self::Lake,
        Self::FrozenLake,
        Self::Canyon,
        Self::Volcano,
        Self::IceSpikes,
        Self::AlienCraters,
        Self::MnemoContamination,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Lake => "Lake",
            Self::FrozenLake => "Frozen lake",
            Self::Canyon => "Canyon",
            Self::Volcano => "Volcano",
            Self::IceSpikes => "Ice spikes",
            Self::AlienCraters => "Alien craters",
            Self::MnemoContamination => "Mnemo contamination",
        }
    }
}

impl BiomeFeature {
    /// This relief written as landforms, to start editing from.
    pub fn preset(self) -> Vec<Landform> {
        let form = |shape, scale, threshold, strength| Landform { shape, scale, threshold, strength, ..Landform::default() };
        let block = |name: &str| Some(name.to_string());
        match self {
            Self::None | Self::Lake => Vec::new(),
            Self::FrozenLake => vec![form(LandShape::Freeze, 0.0, 0.0, 0.0)],
            Self::Canyon => vec![form(LandShape::Ridge, 70.0, 0.09, 16.0)],
            Self::Volcano => vec![
                form(LandShape::Hills, 150.0, 0.25, 70.0),
                Landform { surface: block("lava"), subsurface: block("basalt"), ..form(LandShape::Pit, 150.0, 0.5, 120.0) },
            ],
            Self::IceSpikes => vec![Landform { surface: block("ice"), ..form(LandShape::Spikes, 6.0, 0.55, 45.0) }],
            Self::AlienCraters => vec![Landform {
                surface: block("alien_crust"),
                core: block("astral_ore"),
                chance: 0.6,
                core_chance: 0.35,
                ..form(LandShape::Craters, 28.0, 5.0, 6.0)
            }],
            Self::MnemoContamination => vec![
                Landform { surface: block("mnemo_rot"), ..form(LandShape::Patches, 22.0, 0.3, 0.0) },
                Landform { surface: block("mnemonical_ore"), chance: 0.08, ..form(LandShape::Patches, 22.0, 0.62, 0.0) },
            ],
        }
    }
}

/// How a landform changes the terrain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LandShape {
    /// Long ravines where the noise crosses zero.
    Ridge,
    /// Raises the ground where the noise passes the threshold.
    #[default]
    Hills,
    /// Lowers the ground where the noise passes the threshold.
    Pit,
    /// Columns rising above the surface.
    Spikes,
    /// Only changes the surface blocks.
    Patches,
    /// Round impacts: scale = cell size, threshold = smallest radius,
    /// strength = extra radius, chance = share of cells with a crater.
    Craters,
    /// The water of the biome freezes.
    Freeze,
}

impl LandShape {
    pub const ALL: [Self; 7] = [Self::Ridge, Self::Hills, Self::Pit, Self::Spikes, Self::Patches, Self::Craters, Self::Freeze];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Ridge => "Ravines",
            Self::Hills => "Hills",
            Self::Pit => "Pits",
            Self::Spikes => "Spikes",
            Self::Patches => "Surface patches",
            Self::Craters => "Craters",
            Self::Freeze => "Frozen water",
        }
    }
}

/// One noise-driven relief of a biome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Landform {
    pub shape: LandShape,
    /// Size of the pattern, in voxels.
    pub scale: f32,
    pub threshold: f32,
    pub strength: f32,
    /// Blocks put on the affected ground (spikes use `surface`).
    #[serde(default)]
    pub surface: Option<String>,
    #[serde(default)]
    pub subsurface: Option<String>,
    /// Centre of a crater.
    #[serde(default)]
    pub core: Option<String>,
    /// Share of the cells affected.
    #[serde(default = "full")]
    pub chance: f32,
    #[serde(default = "full")]
    pub core_chance: f32,
}

fn full() -> f32 {
    1.0
}

impl Default for Landform {
    fn default() -> Self {
        Self { shape: LandShape::Hills, scale: 60.0, threshold: 0.2, strength: 12.0, surface: None, subsurface: None, core: None, chance: 1.0, core_chance: 1.0 }
    }
}

/// A kind of ore in the rock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OreVein {
    pub block: String,
    pub min_y: i32,
    pub max_y: i32,
    /// Share of the clumps of rock that hold this ore.
    pub chance: f32,
    /// Size of a clump, in voxels (1 to 8).
    pub size: u32,
    /// Above 0: a long winding vein of this thickness instead of clumps;
    /// `chance` is then the share of the vein filled with ore.
    #[serde(default)]
    pub width: f32,
}

impl Default for OreVein {
    fn default() -> Self {
        Self { block: "mineral_ore".to_string(), min_y: -96, max_y: 40, chance: 0.01, size: 2, width: 0.0 }
    }
}

/// Rock replaced by another below a height.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RockLayer {
    pub block: String,
    pub below_y: i32,
    /// Blocks of irregular border.
    pub dither: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaveSettings {
    /// Lower = bigger caverns.
    pub cavern_rarity: f32,
    pub cavern_scale: f32,
    pub tunnel_width: f32,
    pub tunnel_scale: f32,
    pub noodle_width: f32,
    /// Lower = more openings to the surface.
    pub entrance_rarity: f32,
    /// Fills the caves near the bottom of the world.
    pub fluid: String,
    pub fluid_depth: i32,
    /// Cave floors of humid biomes (empty = none).
    pub floor: String,
    /// Grows on the floor (empty = none).
    pub tuft: String,
    pub tuft_chance: f32,
    pub floor_humidity: f32,
    /// Great halls: lower = more of them.
    pub hall_rarity: f32,
    pub hall_scale: f32,
    /// Height of the middle of the halls, and how far they reach above and below.
    pub hall_y: i32,
    pub hall_height: i32,
    /// Stone columns of the halls: lower = more of them.
    pub pillar_rarity: f32,
    /// Spacing of the columns, in voxels.
    pub pillar_scale: f32,
    /// Longest stalactite, in voxels (0 = none).
    pub spike_length: i32,
    /// Lower = more stalactites.
    pub spike_rarity: f32,
    /// Underground lakes fill every cave below this height.
    pub lake_fluid: String,
    pub lake_level: i32,
    /// Light carried by the player underground, in lumens (0 = none).
    pub explorer_light: f32,
    /// How far that light reaches, in blocks.
    pub explorer_range: f32,
    pub explorer_color: [f32; 3],
}

impl Default for CaveSettings {
    fn default() -> Self {
        Self {
            cavern_rarity: 0.62,
            cavern_scale: 1.0,
            tunnel_width: 0.075,
            tunnel_scale: 1.0,
            noodle_width: 0.045,
            entrance_rarity: 0.32,
            fluid: "molten_lava".to_string(),
            fluid_depth: 7,
            floor: "moss".to_string(),
            tuft: "glow_moss".to_string(),
            tuft_chance: 0.07,
            floor_humidity: 0.15,
            hall_rarity: 0.14,
            hall_scale: 1.0,
            hall_y: -50,
            hall_height: 40,
            pillar_rarity: 0.42,
            pillar_scale: 22.0,
            spike_length: 12,
            spike_rarity: 0.3,
            lake_fluid: "water".to_string(),
            lake_level: -76,
            explorer_light: 1_400_000.0,
            explorer_range: 64.0,
            explorer_color: [1.0, 0.9, 0.78],
        }
    }
}

/// Blocks of the world that belong to no biome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldBlocks {
    pub water: String,
    pub ice: String,
    pub beach: String,
    /// Rock of a world with no biome.
    pub rock: String,
}

impl Default for WorldBlocks {
    fn default() -> Self {
        Self { water: "water".to_string(), ice: "ice".to_string(), beach: "sand".to_string(), rock: "basalt".to_string() }
    }
}

fn default_ores() -> Vec<OreVein> {
    let vein = |block: &str, min_y, max_y, chance| OreVein { block: block.to_string(), min_y, max_y, chance, size: 2, width: 0.0 };
    let lode = |block: &str, min_y, max_y, width| OreVein { block: block.to_string(), min_y, max_y, chance: 0.8, size: 2, width };
    vec![
        lode("crystal_ore", -96, -20, 0.05),
        lode("mineral_ore", -96, 30, 0.07),
        lode("mineral_ore", -40, 200, 0.05),
        vein("crystal_ore", -96, -28, 0.006),
        vein("mineral_ore", -96, 0, 0.007),
        vein("mineral_ore", -96, 26, 0.011),
        vein("mineral_ore", -96, 200, 0.027),
    ]
}

fn default_layers() -> Vec<RockLayer> {
    vec![RockLayer { block: "deepslate".to_string(), below_y: -6, dither: 4 }]
}

fn default_freeze() -> f32 {
    -0.65
}

fn yes() -> bool {
    true
}

/// Great ranges rising from the biomes that allow them (see `BiomeDef::peaks`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MountainSettings {
    /// Height of the tallest summits above the biome, in voxels.
    pub height: f32,
    /// Spacing of the ridges, in voxels.
    pub scale: f32,
    /// Spacing of the ranges, in voxels.
    pub range_scale: f32,
    /// Above 1: thin summits and wide valleys.
    pub sharpness: f32,
    /// Bare rock above this height.
    pub rock_line: i32,
    /// Snow above this height.
    pub snow_line: i32,
    /// Steeper than this (voxels per voxel), the slope is bare rock.
    pub cliff: f32,
    /// Height of the crags that break up the slopes, in voxels.
    pub crag: f32,
    pub rock: String,
    pub snow: String,
}

impl Default for MountainSettings {
    fn default() -> Self {
        Self {
            height: 175.0,
            scale: 300.0,
            range_scale: 900.0,
            sharpness: 1.35,
            rock_line: 72,
            snow_line: 112,
            cliff: 1.3,
            crag: 12.0,
            rock: "stone".to_string(),
            snow: "snow".to_string(),
        }
    }
}

/// A structure found in the caves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaveStructureRule {
    pub structure: String,
    /// Probability of one attempt per chunk column, in `[0, 1]`.
    pub chance_per_chunk: f32,
    #[serde(default = "one")]
    pub attempts: u32,
    pub min_y: i32,
    pub max_y: i32,
    /// Hangs from the ceiling instead of standing on the floor.
    #[serde(default)]
    pub ceiling: bool,
    /// Biomes above the cave (empty = all).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub biomes: Vec<String>,
}

impl Default for CaveStructureRule {
    fn default() -> Self {
        Self { structure: String::new(), chance_per_chunk: 0.1, attempts: 1, min_y: -80, max_y: -10, ceiling: false, biomes: Vec::new() }
    }
}

/// A choice among the structures of a rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeightedStructure {
    pub structure: String,
    pub weight: f32,
}

/// Where and how often a structure appears in a biome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StructureRule {
    pub structure: String,
    /// Probability of one attempt per chunk column, in `[0, 1]`.
    pub chance_per_chunk: f32,
    /// Attempts per chunk column (each rolls `chance_per_chunk`).
    #[serde(default = "one")]
    pub attempts: u32,
    /// Maximum height difference between neighbouring columns at the spot.
    pub max_slope: i32,
    /// Surface blocks the structure may stand on (empty = any).
    #[serde(default)]
    pub ground: Vec<String>,
    /// Instead of `structure`: one of these, by weight.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<WeightedStructure>,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BiomeDef {
    pub name: String,
    /// Climate point, both in `[-1, 1]`.
    pub temperature: f32,
    pub humidity: f32,
    /// Mean surface height in voxels.
    pub base_height: f32,
    /// Amplitude of the relief noise.
    pub height_scale: f32,
    /// Persistence of the octaves (0 = smooth, 1 = very rough).
    pub roughness: f32,
    pub octaves: u32,
    /// Share of the great mountain ranges this biome receives (0 = none, 1 = all).
    #[serde(default)]
    pub peaks: f32,
    /// How strongly the continental noise lifts / sinks this biome.
    #[serde(default = "default_continentalness")]
    pub continentalness: f32,
    pub surface_block: String,
    pub subsurface_block: String,
    pub stone_block: String,
    #[serde(default = "default_subsurface_depth")]
    pub subsurface_depth: i32,
    #[serde(default)]
    pub feature: BiomeFeature,
    pub sky_color: [f32; 3],
    #[serde(default = "default_fog")]
    pub fog_density: f32,
    #[serde(default)]
    pub structures: Vec<StructureRule>,
    /// Reliefs of the biome; empty = the preset `feature`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub landforms: Vec<Landform>,
    /// Ores of this biome, on top of those of the world.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ores: Vec<OreVein>,
    /// Sand on the shores.
    #[serde(default = "yes")]
    pub beaches: bool,
}

impl BiomeDef {
    pub fn forms(&self) -> Vec<Landform> {
        if self.landforms.is_empty() {
            self.feature.preset()
        } else {
            self.landforms.clone()
        }
    }
}

fn default_continentalness() -> f32 {
    0.5
}

fn default_subsurface_depth() -> i32 {
    3
}

fn default_fog() -> f32 {
    0.015
}

/// Global noise settings shared by all biomes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldGenSettings {
    pub seed: u32,
    /// Climate noise wavelength, in voxels.
    pub climate_scale: f32,
    /// Continental noise wavelength, in voxels.
    pub continental_scale: f32,
    /// Relief noise wavelength, in voxels.
    pub relief_scale: f32,
    pub sea_level: i32,
    /// Lowest generated layer.
    pub bedrock: i32,
    /// Blend width between biomes, in climate units.
    pub blend: f32,
    /// Carve caves (caverns, tunnels, lava lakes, ores, lush floors).
    #[serde(default = "default_caves")]
    pub caves: bool,
    #[serde(default)]
    pub cave: CaveSettings,
    #[serde(default = "default_ores")]
    pub ores: Vec<OreVein>,
    #[serde(default = "default_layers")]
    pub layers: Vec<RockLayer>,
    #[serde(default)]
    pub blocks: WorldBlocks,
    /// Water freezes below this temperature.
    #[serde(default = "default_freeze")]
    pub freeze_below: f32,
    #[serde(default)]
    pub mountains: MountainSettings,
    /// Spacing of the ore veins, in voxels.
    #[serde(default = "default_vein_scale")]
    pub vein_scale: f32,
    /// Structures of the caves.
    #[serde(default)]
    pub cave_structures: Vec<CaveStructureRule>,
}

fn default_vein_scale() -> f32 {
    34.0
}

fn default_caves() -> bool {
    true
}

impl Default for WorldGenSettings {
    fn default() -> Self {
        Self {
            seed: 1337,
            climate_scale: 420.0,
            continental_scale: 900.0,
            relief_scale: 96.0,
            sea_level: 6,
            bedrock: -96,
            blend: 0.35,
            caves: true,
            cave: CaveSettings::default(),
            ores: default_ores(),
            layers: default_layers(),
            blocks: WorldBlocks::default(),
            freeze_below: default_freeze(),
            mountains: MountainSettings::default(),
            vein_scale: default_vein_scale(),
            cave_structures: Vec::new(),
        }
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct BiomeLibrary {
    pub biomes: Vec<BiomeDef>,
    pub settings: WorldGenSettings,
    pub selection: usize,
    pub revision: u64,
    pub last_saved_revision: u64,
}

impl BiomeLibrary {
    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.last_saved_revision
    }

    pub fn dir(project: &ProjectLayout) -> PathBuf {
        project.module_path("biomes")
    }

    pub fn load_from_project(&mut self, project: &ProjectLayout) -> Result<Vec<String>, String> {
        let dir = Self::dir(project);
        fs::create_dir_all(&dir).map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        let mut warnings = Vec::new();

        let settings_path = dir.join(WORLD_SETTINGS_FILE);
        if settings_path.is_file() {
            match load_ron::<WorldGenSettings>(&settings_path) {
                Ok(settings) => self.settings = settings,
                Err(error) => warnings.push(error),
            }
        }

        let mut biomes = Vec::new();
        for path in ron_files(&dir)? {
            if path.file_name().map(|name| name == WORLD_SETTINGS_FILE).unwrap_or(false) {
                continue;
            }
            match load_ron::<BiomeDef>(&path) {
                Ok(biome) => biomes.push(biome),
                Err(error) => warnings.push(error),
            }
        }

        let mut created = false;
        if biomes.is_empty() {
            biomes = default_biomes();
            created = true;
        }
        // Spec names, retired examples and the biomes added since.
        for (old, new) in RENAMED_BIOMES {
            if !biomes.iter().any(|b| b.name == new) {
                if let Some(biome) = biomes.iter_mut().find(|b| b.name == old) {
                    biome.name = new.to_string();
                    created = true;
                }
            }
        }
        let before = biomes.len();
        biomes.retain(|b| !RETIRED_BIOMES.contains(&b.name.as_str()) && !RENAMED_BIOMES.iter().any(|(old, _)| *old == b.name));
        created |= biomes.len() != before;
        for biome in default_biomes() {
            if !biomes.iter().any(|b| b.name == biome.name) {
                biomes.push(biome);
                created = true;
            }
        }
        biomes.sort_by(|a, b| a.name.cmp(&b.name));
        self.biomes = biomes;
        self.selection = self.selection.min(self.biomes.len().saturating_sub(1));
        self.touch();
        self.last_saved_revision = self.revision;
        if created || !settings_path.is_file() {
            self.save_all(project)?;
        }
        Ok(warnings)
    }

    pub fn save_all(&mut self, project: &ProjectLayout) -> Result<usize, String> {
        let dir = Self::dir(project);
        save_ron(&dir.join(WORLD_SETTINGS_FILE), &self.settings)?;
        for biome in &self.biomes {
            save_ron(&dir.join(format!("{}.ron", file_stem(&biome.name))), biome)?;
        }
        // Remove files of deleted biomes.
        let keep: Vec<String> = self
            .biomes
            .iter()
            .map(|biome| format!("{}.ron", file_stem(&biome.name)))
            .collect();
        for path in ron_files(&dir)? {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name != WORLD_SETTINGS_FILE && !keep.contains(&name) {
                let _ = fs::remove_file(&path);
            }
        }
        self.last_saved_revision = self.revision;
        Ok(self.biomes.len())
    }

    /// Add a copy of the selected biome under a fresh name.
    pub fn duplicate_selected(&mut self) -> Option<String> {
        let mut biome = self.biomes.get(self.selection)?.clone();
        let mut index = 2;
        let base = biome.name.clone();
        while self.biomes.iter().any(|b| b.name == format!("{base} {index}")) {
            index += 1;
        }
        biome.name = format!("{base} {index}");
        let name = biome.name.clone();
        self.biomes.push(biome);
        self.selection = self.biomes.len() - 1;
        self.touch();
        Some(name)
    }
}

pub fn file_stem(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

pub fn ron_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().map(|e| e.eq_ignore_ascii_case("ron")).unwrap_or(false))
        .collect();
    paths.sort();
    Ok(paths)
}

fn rule(structure: &str, chance: f32, attempts: u32, max_slope: i32, ground: &[&str]) -> StructureRule {
    StructureRule {
        structure: structure.to_string(),
        chance_per_chunk: chance,
        attempts,
        max_slope,
        ground: ground.iter().map(|g| g.to_string()).collect(),
        variants: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn biome(
    name: &str,
    temperature: f32,
    humidity: f32,
    base_height: f32,
    height_scale: f32,
    roughness: f32,
    palette: (&str, &str, &str),
    feature: BiomeFeature,
    sky: [f32; 3],
    structures: Vec<StructureRule>,
) -> BiomeDef {
    BiomeDef {
        name: name.to_string(),
        temperature,
        humidity,
        base_height,
        height_scale,
        roughness,
        octaves: 4,
        peaks: if name == "Mountain" { 1.0 } else { 0.0 },
        continentalness: default_continentalness(),
        surface_block: palette.0.to_string(),
        subsurface_block: palette.1.to_string(),
        stone_block: palette.2.to_string(),
        subsurface_depth: default_subsurface_depth(),
        feature,
        sky_color: sky,
        fog_density: default_fog(),
        structures,
        landforms: Vec::new(),
        ores: Vec::new(),
        beaches: feature != BiomeFeature::Volcano,
    }
}

/// Biomes of earlier versions renamed to the spec names: (old, new).
pub const RENAMED_BIOMES: [(&str, &str); 4] = [("Alien Crater", "Crater"), ("Mnemo Zone", "Mnemo Plain"), ("Mud Plain", "Mud"), ("Ice Pike", "Ice Floe")];
/// Biomes created as examples by earlier versions (removed on load).
pub const RETIRED_BIOMES: [&str; 2] = ["Test", "Lake"];

/// The biomes of the game.
pub fn default_biomes() -> Vec<BiomeDef> {
    use BiomeFeature as F;
    let grass = ("grass", "dirt", "stone");
    let sand = ("sand", "sand", "sandstone");
    vec![
        biome("Plains", 0.2, 0.0, 10.0, 4.0, 0.35, grass, F::None, [0.50, 0.70, 0.90],
            vec![rule("Oak Tree", 0.25, 1, 2, &["grass"]), rule("Ruin", 0.03, 1, 1, &["grass"])]),
        biome("Forest", 0.3, 0.45, 11.0, 6.0, 0.45, ("grass_dark", "dirt", "stone"), F::None, [0.44, 0.64, 0.82],
            vec![rule("Oak Tree", 0.9, 3, 3, &["grass_dark"])]),
        biome("Great Forest", 0.45, 0.8, 13.0, 9.0, 0.5, ("grass_dark", "dirt", "stone"), F::None, [0.38, 0.58, 0.74],
            vec![rule("Giant Tree", 0.5, 2, 2, &["grass_dark"]), rule("Oak Tree", 0.8, 2, 3, &["grass_dark"])]),
        biome("Pine Forest", -0.35, 0.35, 14.0, 9.0, 0.5, ("grass_dark", "dirt", "stone"), F::None, [0.46, 0.60, 0.78],
            vec![rule("Pine Tree", 0.9, 3, 3, &["grass_dark", "snow"])]),
        biome("Mud", 0.4, 0.95, 7.0, 2.5, 0.3, ("mud", "mud", "stone"), F::Lake, [0.48, 0.56, 0.58],
            vec![rule("Hut", 0.04, 1, 1, &["mud"])]),
        biome("Swamp", 0.65, 0.85, 6.0, 2.0, 0.3, ("grass_dark", "mud", "stone"), F::Lake, [0.42, 0.52, 0.46],
            vec![rule("Oak Tree", 0.5, 2, 2, &["grass_dark"])]),
        biome("Mountain", -0.1, -0.2, 26.0, 30.0, 0.62, ("stone", "stone", "stone"), F::None, [0.56, 0.68, 0.86],
            vec![rule("Crystal Spire", 0.12, 1, 6, &["stone"])]),
        biome("Canyon", 0.75, -0.6, 18.0, 6.0, 0.4, ("red_sand", "red_sand", "stone"), F::Canyon, [0.86, 0.66, 0.48],
            vec![rule("Ruin", 0.05, 1, 2, &["red_sand"])]),
        biome("Desert", 0.9, -0.55, 9.0, 3.0, 0.3, sand, F::None, [0.92, 0.80, 0.58], Vec::new()),
        biome("Oasis", 0.95, -0.2, 7.0, 2.0, 0.3, sand, F::Lake, [0.70, 0.82, 0.86],
            vec![rule("Palm Tree", 0.8, 2, 2, &["sand"])]),
        biome("Pyramid and Temple", 0.7, -0.95, 9.0, 2.5, 0.25, sand, F::None, [0.94, 0.78, 0.52],
            vec![rule("Pyramid", 0.12, 1, 2, &["sand"]), rule("Temple", 0.1, 1, 2, &["sand"])]),
        biome("Volcano", 0.95, -0.9, 16.0, 14.0, 0.55, ("basalt", "basalt", "basalt"), F::Volcano, [0.40, 0.30, 0.30],
            Vec::new()),
        biome("Frozen Lake", -0.8, 0.7, 2.0, 3.0, 0.3, ("snow", "ice", "stone"), F::FrozenLake, [0.76, 0.84, 0.94],
            Vec::new()),
        biome("Ice Floe", -0.9, -0.2, 14.0, 8.0, 0.45, ("snow", "ice", "stone"), F::IceSpikes, [0.78, 0.86, 0.96],
            Vec::new()),
        biome("Crater", 0.6, 0.3, 12.0, 5.0, 0.4, ("alien_crust", "basalt", "basalt"), F::AlienCraters, [0.42, 0.26, 0.56],
            vec![rule("Alien Pod", 0.3, 1, 2, &["alien_crust"])]),
        biome("Burnt Forest", 0.7, 0.12, 11.0, 4.0, 0.4, ("burnt_ground", "dirt", "stone"), F::None, [0.46, 0.38, 0.36],
            vec![rule("Burnt Tree", 0.8, 3, 3, &["burnt_ground"])]),
        biome("Mnemo Plain", -0.3, -0.85, 10.0, 4.0, 0.35, ("mnemo_rot", "mnemo_rot", "stone"), F::MnemoContamination, [0.50, 0.26, 0.28],
            vec![rule("Crystal Spire", 0.08, 1, 3, &[])]),
    ]
}

pub struct BiomePlugin;

impl Plugin for BiomePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BiomeLibrary>().add_systems(
            Startup,
            load_biomes.after(crate::core::project_io::initialize_project_system),
        );
    }
}

fn load_biomes(
    mut library: ResMut<BiomeLibrary>,
    project: Res<ProjectLayout>,
    mut ui: ResMut<crate::core::app_state::EditorUiState>,
) {
    match library.load_from_project(&project) {
        Ok(warnings) => {
            ui.info(format!("Biome library: {} biome(s).", library.biomes.len()));
            for warning in warnings {
                ui.warn(warning);
            }
        }
        Err(error) => ui.error(format!("Biome library failed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_biomes_cover_the_spec() {
        let biomes = default_biomes();
        for name in [
            "Plains", "Mud", "Great Forest", "Mountain", "Volcano", "Desert", "Crater", "Canyon", "Forest", "Frozen Lake",
            "Ice Floe", "Swamp", "Oasis", "Burnt Forest", "Pyramid and Temple", "Mnemo Plain",
        ] {
            assert!(biomes.iter().any(|b| b.name == name), "missing {name}");
        }
        for name in RETIRED_BIOMES {
            assert!(!biomes.iter().any(|b| b.name == name), "{name} should not exist");
        }
        // Every structure a biome uses is a default prefab.
        let structures = crate::voxel::structures::default_structures();
        for biome in &biomes {
            for rule in &biome.structures {
                assert!(structures.iter().any(|s| s.name == rule.structure), "{} uses unknown {}", biome.name, rule.structure);
            }
        }
    }

    #[test]
    fn biome_round_trips_through_ron() {
        let biome = default_biomes().remove(0);
        let text = ron::ser::to_string(&biome).expect("serialize");
        let back: BiomeDef = ron::from_str(&text).expect("parse");
        assert_eq!(biome, back);
    }
}
