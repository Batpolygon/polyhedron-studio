//! Data-driven block definitions.
//!
//! A block owns the shape, colour and material response used by the mesher and
//! the renderer. Blocks are declared in `project/blocks/*.ron`; the builtin set
//! is only a fallback so the sample project is usable even with an empty folder.
//!
//! Voxel cells store a compact [`BlockId`] (an index in the registry) instead of
//! duplicating shape and colour per cell. World files persist the stable string
//! key, so editing a block file never invalidates a saved world.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::app_state::EditorUiState;
use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::voxel::shapes::{VoxelShape, MAX_SHAPE_PARTS};

/// Index of a block in the registry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u16);

/// Density of the quantised Voronoi normal map applied by the WGSL shader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VoronoiCells {
    None,
    Cells3,
    #[default]
    Cells7,
    Cells30,
    Cells200,
}

impl VoronoiCells {
    pub const ALL: [Self; 5] = [
        Self::None,
        Self::Cells3,
        Self::Cells7,
        Self::Cells30,
        Self::Cells200,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Cells3 => "3 Cells",
            Self::Cells7 => "7 Cells",
            Self::Cells30 => "30 Cells",
            Self::Cells200 => "200 Cells",
        }
    }

    /// Cell density per world unit fed to the shader.
    pub const fn density(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Cells3 => 1.2,
            Self::Cells7 => 2.0,
            Self::Cells30 => 4.5,
            Self::Cells200 => 11.0,
        }
    }
}

/// Low-poly Voronoi normal map: every Voronoi cell of the surface gets its own
/// tilted normal, which breaks flat faces into lit facets (a "chiselled"
/// look) without adding geometry. Exposed per block in the block forge.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoronoiNormal {
    /// Facets on/off.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Tilt of every facet, `0` = flat, `1` = very chiselled.
    #[serde(default = "default_normal_strength")]
    pub strength: f32,
    /// Number of facets per block edge (cell density).
    #[serde(default = "default_normal_cells")]
    pub cells: f32,
    /// Width of the bevel between facets, `0` = sharp edges.
    #[serde(default = "default_normal_bevel")]
    pub bevel: f32,
    /// Random seed offset so two blocks never share the same pattern.
    #[serde(default)]
    pub seed: f32,
}

const fn default_true() -> bool {
    true
}

const fn default_normal_strength() -> f32 {
    0.35
}

const fn default_normal_cells() -> f32 {
    3.0
}

const fn default_normal_bevel() -> f32 {
    0.12
}

impl Default for VoronoiNormal {
    fn default() -> Self {
        Self {
            enabled: true,
            strength: default_normal_strength(),
            cells: default_normal_cells(),
            bevel: default_normal_bevel(),
            seed: 0.0,
        }
    }
}

impl VoronoiNormal {
    pub const fn with(strength: f32, cells: f32, bevel: f32) -> Self {
        Self {
            enabled: true,
            strength,
            cells,
            bevel,
            seed: 0.0,
        }
    }
}

/// One box of a custom block model, in 1/16 block units (`0..=16`), like the
/// elements of the mob sculptor. An element without colour uses the block
/// colour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelElement {
    #[serde(default)]
    pub name: String,
    pub from: [f32; 3],
    pub to: [f32; 3],
    /// Own colour of the element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
}

impl ModelElement {
    pub fn new(name: &str, from: [f32; 3], to: [f32; 3]) -> Self {
        Self {
            name: name.to_string(),
            from,
            to,
            color: None,
        }
    }

    /// Normalised `[0, 1]` cell-space bounds (min, max).
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let a = Vec3::from_array(self.from) / 16.0;
        let b = Vec3::from_array(self.to) / 16.0;
        (a.min(b).clamp(Vec3::ZERO, Vec3::ONE), a.max(b).clamp(Vec3::ZERO, Vec3::ONE))
    }
}

/// Custom multi-box model replacing the shape of a block when present.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockModel {
    pub elements: Vec<ModelElement>,
}

impl BlockModel {
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
}

/// Physical surface family: drives footsteps, mining sounds and particles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SurfaceMaterial {
    #[default]
    Stone,
    Dirt,
    Grass,
    Sand,
    Wood,
    Metal,
    Glass,
    Ice,
}

impl SurfaceMaterial {
    pub const ALL: [Self; 8] = [
        Self::Stone,
        Self::Dirt,
        Self::Grass,
        Self::Sand,
        Self::Wood,
        Self::Metal,
        Self::Glass,
        Self::Ice,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Stone => "Stone",
            Self::Dirt => "Dirt",
            Self::Grass => "Grass",
            Self::Sand => "Sand",
            Self::Wood => "Wood",
            Self::Metal => "Metal",
            Self::Glass => "Glass",
            Self::Ice => "Ice",
        }
    }
}

/// Gameplay behaviour attached to a block (machines, movables, vehicles).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlockFunction {
    /// No behaviour. (Old `CableNode` blocks load as plain blocks: cables now
    /// connect machine ports directly.)
    #[default]
    #[serde(alias = "CableNode")]
    None,
    /// Block subject to gravity with a fixed rotation (the whole connected
    /// structure becomes one body).
    Movable,
    /// Turns the blocks attached to its top face (the whole block turns).
    Motor,
    /// Drives a movable from a distance (with the Remote Control).
    Controller,
    /// Seat subject to gravity, faces where it goes.
    Wagon,
    /// Burns 10 Floral or 10 Faunal into 1 Voltic of power for a small field.
    Combustor,
    /// Solaronel: daylight -> 1 Voltic every 9 s.
    Solar,
    /// Floral + Mineral / Floral + Crystalline -> Synthetic (by hand).
    Synthesizer,
    /// Infusions of two resources (Celliom, Aetech, Voltiom...).
    Infuser,
    /// Astral-Vacum: pulls the selected resources out of containers in a
    /// radius and stores them in the container it is connected to.
    Vacuum,
    /// Mnemo-Harvestor: bio-machine that gathers raw resource blocks and
    /// stores them in a container.
    Harvester,
    /// Energiser: spends Voltic to power every machine in its field.
    Energizer,
    /// Stores resources.
    Container,
    /// Light: emits light when powered.
    Lamp,
    /// Combines resources automatically (Synthetic, Technic, Conflux, Omnicore).
    Assembler,
    /// Sends Omnicore into the network.
    Omnichorus,
    /// Pushes the entities and movables around it (configurable).
    Repulsor,
    /// Runs its command lines one by one in a loop while powered.
    Terminal,
    /// Simplified command interface (combo boxes, no typing).
    Comandor,
    /// Logic gate (AND, OR, XOR, NOT, NAND, NOR) or resource decoder.
    Transmitter,
    /// Shows the output value of the machine plugged into it.
    Visualiser,
    /// Emits a set value (Boolean, Float, String).
    Emitter,
    /// Stores the value of a connected output into a variable.
    Receiver,
    /// Displays a colour from a signal (Boolean / Float / String).
    Screen,
    /// Conflux (or Garbage) + 100 Voltic -> Atomic.
    Atomiser,
    /// Collects / distributes the resources of the entities walking on it.
    Ressourcer,
    /// Attracts the targeted entities (AI) in a radius.
    Ringer,
    /// Line trace: hit (Boolean), length (Float) or name (String).
    Laser,
    /// Teleport / respawn point paid in Ethereal.
    Etheraltar,
    /// Pressure -> 1 Voltic every 12 s.
    Kineticster,
    /// Duplicates a resource fed 10 times, with Atomic and 1000 Voltic.
    Reconstructor,
    /// Generates the resources of its biome (10 Voltic, 4 to 8 units / 4 s).
    Excavator,
    /// Extracts Ethereal from the entities on it until they die.
    Ethereaper,
    /// Produces power from the Omnicore network.
    Omnireactor,
    /// Summons the entities of a discovered biome for Ethereal.
    Invocateral,
    /// Saves constructions / machines to a file and loads them back.
    Televerseur,
    /// Bio-machine attacking the entities walking on it.
    MnemoDigestor,
    /// Bio-machine making a Mnemo double of a killed entity.
    MnemoCloner,
    /// Modifies the player stats.
    AstralModifier,
    /// Container with room for 1 000 000 resources.
    AstralContainer,
    /// Storage and panel; its behaviour comes from its components.
    Custom,
}

impl BlockFunction {
    pub const ALL: [Self; 41] = [
        Self::None,
        Self::Movable,
        Self::Motor,
        Self::Controller,
        Self::Wagon,
        Self::Combustor,
        Self::Solar,
        Self::Synthesizer,
        Self::Infuser,
        Self::Vacuum,
        Self::Harvester,
        Self::Energizer,
        Self::Container,
        Self::Lamp,
        Self::Assembler,
        Self::Omnichorus,
        Self::Repulsor,
        Self::Terminal,
        Self::Comandor,
        Self::Transmitter,
        Self::Visualiser,
        Self::Emitter,
        Self::Receiver,
        Self::Screen,
        Self::Atomiser,
        Self::Ressourcer,
        Self::Ringer,
        Self::Laser,
        Self::Etheraltar,
        Self::Kineticster,
        Self::Reconstructor,
        Self::Excavator,
        Self::Ethereaper,
        Self::Omnireactor,
        Self::Invocateral,
        Self::Televerseur,
        Self::MnemoDigestor,
        Self::MnemoCloner,
        Self::AstralModifier,
        Self::AstralContainer,
        Self::Custom,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Movable => "Movable",
            Self::Motor => "Motor",
            Self::Controller => "Controller",
            Self::Wagon => "Wagon",
            Self::Combustor => "Combustor",
            Self::Solar => "Solaronel",
            Self::Synthesizer => "Synthesizer",
            Self::Infuser => "Infuser",
            Self::Vacuum => "Astral-Vacum",
            Self::Harvester => "Mnemo-Harvestor",
            Self::Energizer => "Energiser",
            Self::Container => "Container",
            Self::Lamp => "Light",
            Self::Assembler => "Assembler",
            Self::Omnichorus => "Omnichorus",
            Self::Repulsor => "Repulsor",
            Self::Terminal => "Terminal",
            Self::Comandor => "Comandor",
            Self::Transmitter => "Transmitter",
            Self::Visualiser => "Visualiser",
            Self::Emitter => "Emitter",
            Self::Receiver => "Receiver",
            Self::Screen => "Screen",
            Self::Atomiser => "Atomiser",
            Self::Ressourcer => "Ressourcer",
            Self::Ringer => "Ringer",
            Self::Laser => "Laser",
            Self::Etheraltar => "Etheraltar",
            Self::Kineticster => "Kineticster",
            Self::Reconstructor => "Reconstructor",
            Self::Excavator => "Excavator",
            Self::Ethereaper => "Ethereaper",
            Self::Omnireactor => "Omnireactor",
            Self::Invocateral => "Invocateral",
            Self::Televerseur => "Televerseur",
            Self::MnemoDigestor => "Mnemo-Digestor",
            Self::MnemoCloner => "Mnemo-Cloner",
            Self::AstralModifier => "Astral-Modifier",
            Self::AstralContainer => "Astral-Container",
            Self::Custom => "Custom machine",
        }
    }

    /// Sources of a power field (and of Energy cable outputs).
    pub const fn is_generator(self) -> bool {
        matches!(self, Self::Combustor | Self::Omnireactor)
    }

    /// Logic machines: their value travels on Signal cables.
    pub const fn is_logic(self) -> bool {
        matches!(
            self,
            Self::Transmitter | Self::Visualiser | Self::Emitter | Self::Receiver | Self::Screen | Self::Laser | Self::Terminal | Self::Comandor
        )
    }

    /// Machines that keep resources the player can open and move.
    pub const fn is_storage(self) -> bool {
        matches!(self, Self::Container | Self::AstralContainer | Self::Custom)
    }

    /// Room of a storage machine, in resource units.
    pub const fn capacity(self) -> u32 {
        match self {
            Self::AstralContainer => 1_000_000,
            _ => 27 * 999,
        }
    }

    /// Blocks with a runtime state in the energy grid.
    pub const fn is_machine(self) -> bool {
        !matches!(self, Self::None | Self::Movable | Self::Controller | Self::Wagon | Self::Assembler | Self::Omnichorus)
    }

    /// Machines that run without power.
    pub const fn needs_no_power(self) -> bool {
        matches!(
            self,
            Self::Motor
                | Self::Synthesizer
                | Self::Container
                | Self::AstralContainer
                | Self::Solar
                | Self::Kineticster
                | Self::Emitter
                | Self::Transmitter
                | Self::Visualiser
                | Self::Receiver
                | Self::Ressourcer
                | Self::Etheraltar
                | Self::Televerseur
                | Self::MnemoDigestor
                | Self::Harvester
                | Self::MnemoCloner
                | Self::Energizer
        )
    }

    /// Default ports of the function (used when a block declares none).
    pub fn default_ports(self) -> Vec<PortDef> {
        use PortDirection::{Input, Output};
        use PortKind::{Energy, Resource, Signal};
        let power = || PortDef::new(Energy, Input, "Power");
        match self {
            Self::Combustor => vec![PortDef::new(Energy, Output, "Power"), PortDef::new(Resource, Input, "Fuel in")],
            Self::Omnireactor => vec![PortDef::new(Energy, Output, "Power")],
            Self::Solar | Self::Kineticster => vec![PortDef::new(Resource, Output, "Voltic")],
            Self::Energizer => vec![PortDef::new(Energy, Input, "Charge"), PortDef::new(Resource, Input, "Voltic in")],
            Self::Lamp | Self::Infuser | Self::Repulsor | Self::Ringer | Self::Invocateral | Self::AstralModifier => {
                vec![power(), PortDef::new(Signal, Input, "Enable")]
            }
            Self::Motor => vec![power(), PortDef::new(Signal, Input, "Enable")],
            Self::Harvester | Self::Vacuum | Self::Excavator | Self::Ethereaper | Self::Ressourcer => {
                vec![power(), PortDef::new(Resource, Output, "Items")]
            }
            Self::Atomiser | Self::Reconstructor => {
                vec![power(), PortDef::new(Resource, Input, "Items in"), PortDef::new(Resource, Output, "Items out")]
            }
            Self::Container | Self::AstralContainer | Self::Custom => vec![PortDef::new(Resource, Input, "Items in"), PortDef::new(Resource, Output, "Items out")],
            Self::Terminal => vec![power(), PortDef::new(Signal, Output, "Result")],
            Self::Comandor => vec![power(), PortDef::new(Signal, Input, "Trigger")],
            Self::Transmitter => vec![
                PortDef::new(Signal, Input, "A"),
                PortDef::new(Signal, Input, "B"),
                PortDef::new(Resource, Input, "Decode"),
                PortDef::new(Signal, Output, "Out"),
            ],
            Self::Visualiser | Self::Receiver => vec![PortDef::new(Signal, Input, "Value")],
            Self::Emitter => vec![PortDef::new(Signal, Output, "Value")],
            Self::Screen => vec![power(), PortDef::new(Signal, Input, "Colour")],
            Self::Laser => vec![power(), PortDef::new(Signal, Output, "Hit")],
            Self::MnemoCloner => vec![PortDef::new(Resource, Input, "Ethereal in")],
            _ => Vec::new(),
        }
    }
}

/// What travels through a cable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortKind {
    /// Power: generators and energizers feed machines.
    #[default]
    Energy,
    /// Items moved between machine storages (one per second).
    Resource,
    /// Script signals (`Set signal` -> `On Signal`).
    Signal,
}

impl PortKind {
    pub const ALL: [Self; 3] = [Self::Energy, Self::Resource, Self::Signal];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Energy => "Energy",
            Self::Resource => "Resource",
            Self::Signal => "Signal",
        }
    }

    /// Display colour (sRGB).
    pub const fn rgb(self) -> [u8; 3] {
        match self {
            Self::Energy => [255, 200, 60],
            Self::Resource => [110, 230, 120],
            Self::Signal => [90, 170, 255],
        }
    }
}

/// Direction of a port.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortDirection {
    #[default]
    Input,
    Output,
}

impl PortDirection {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Input => "Input",
            Self::Output => "Output",
        }
    }
}

/// A cable connector of a block, shown while the cable tool is in hand.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PortDef {
    pub kind: PortKind,
    pub direction: PortDirection,
    #[serde(default)]
    pub name: String,
}

impl PortDef {
    pub fn new(kind: PortKind, direction: PortDirection, name: &str) -> Self {
        Self { kind, direction, name: name.to_string() }
    }
}

/// One authored block type.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockDef {
    /// Stable key stored in world files.
    pub id: String,
    pub label: String,
    pub shape: VoxelShape,
    pub color: [u8; 3],
    #[serde(default = "default_roughness")]
    pub roughness: f32,
    #[serde(default)]
    pub metallic: f32,
    #[serde(default)]
    pub emissive: f32,
    #[serde(default)]
    pub voronoi: VoronoiCells,
    #[serde(default)]
    pub material: SurfaceMaterial,
    /// Seconds of continuous mining with bare hands.
    #[serde(default = "default_hardness")]
    pub hardness: f32,
    /// Resource dropped when broken (`None` drops the block itself).
    #[serde(default)]
    pub drop: Option<String>,
    #[serde(default)]
    pub function: BlockFunction,
    /// What the machine does, whatever its function.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<crate::gameplay::machines::components::MachineComponent>,
    /// Runs without power.
    #[serde(default)]
    pub self_powered: bool,
    /// Sounds made in the Ambience workspace (empty = the material's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub break_sound: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place_sound: Option<String>,
    /// Blockly program bound to this block (interactions, signals, energy).
    #[serde(default)]
    pub script: Option<String>,
    /// Wireless power radius for generators, in voxels.
    #[serde(default = "default_power_radius")]
    pub power_radius: f32,
    /// Liquid: flows to its neighbours, can be swum in, has no collision.
    #[serde(default)]
    pub liquid: bool,
    /// Ticks between two flow steps of a liquid (water 4, lava 12).
    #[serde(default = "default_flow_ticks")]
    pub flow_ticks: u32,
    /// How far a liquid spreads horizontally from a source (1..=7).
    #[serde(default = "default_flow_range")]
    pub flow_range: u8,
    /// Opacity of a liquid surface.
    #[serde(default = "default_liquid_opacity")]
    pub opacity: f32,
    /// Low-poly Voronoi normal map.
    #[serde(default)]
    pub normal: VoronoiNormal,
    /// Optional multi-box model (block forge). Replaces `shape` when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<BlockModel>,
    /// Synthetic block: colour and Voronoi relief are chosen by the player
    /// before placing (one of the nine shape blocks).
    #[serde(default)]
    pub synthetic: bool,
    /// Customised synthetic variant: id of the base block it was made from
    /// (the variant drops and consumes the base block).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant_of: Option<String>,
    /// Colour of each face, by part label ("Top", "Front"...): the rest of
    /// the block keeps `color`. Forge blocks only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub face_colors: Vec<(String, [u8; 3])>,
    /// Flecks scattered over the surface (ore veins, pebbles, flowers...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<BlockAccent>,
    /// Resolved colours of the parts: `face_colors`, or the painted parts of
    /// a customised synthetic variant (those live in the variant key).
    #[serde(skip)]
    pub part_colors: PartColors,
    /// Wind sway of the vertices (0 = still, 1 = strong), e.g. leaves.
    #[serde(default)]
    pub wind: f32,
    /// How a placed block is oriented.
    #[serde(default)]
    pub placement: BlockPlacement,
    /// Cable ports. Empty = the defaults of its function (and a signal input /
    /// output for blocks with a script).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<PortDef>,
    /// Extra animated parts drawn on the block in game (motor hub, fan,
    /// floating crystal, lit window...). `None` in an older file = the
    /// engine defaults of the same block id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<BlockPart>>,
    /// Not offered in the inventories nor craftable (retired blocks).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

/// Flecks of a block surface: a share of the cells of a Voronoi pattern take
/// another colour, which may glow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockAccent {
    pub color: [u8; 3],
    /// Share of the cells (`0..=1`).
    pub amount: f32,
    /// Cells along a block edge (bigger = finer flecks).
    pub cells: f32,
    /// Emission of the flecks.
    #[serde(default)]
    pub glow: f32,
}

/// When an animated part of a block moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartMotion {
    /// Always.
    #[default]
    Always,
    /// While the machine is powered (or burning fuel).
    Powered,
    /// Follows the motor: its rotor angle in the world, its thrust on a movable.
    Motor,
}

impl PartMotion {
    pub const ALL: [Self; 3] = [Self::Always, Self::Powered, Self::Motor];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Always => "Always",
            Self::Powered => "While powered",
            Self::Motor => "With the motor",
        }
    }
}

/// One coloured, optionally animated box drawn on a block in game. Units are
/// 1/16 of a block (like the forge elements); rotation is Euler YXZ degrees.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlockPart {
    #[serde(default)]
    pub name: String,
    pub from: [f32; 3],
    pub to: [f32; 3],
    #[serde(default)]
    pub rotation: [f32; 3],
    pub color: [u8; 3],
    #[serde(default)]
    pub emissive: f32,
    #[serde(default)]
    pub metallic: f32,
    /// Colour while the block is powered (lit window).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub powered_color: Option<[u8; 3]>,
    #[serde(default)]
    pub powered_emissive: f32,
    /// Centre of the spin / bob (1/16 units).
    #[serde(default = "part_pivot")]
    pub pivot: [f32; 3],
    /// Spin speed in radians per second (0 = still).
    #[serde(default)]
    pub spin: f32,
    #[serde(default = "part_axis")]
    pub spin_axis: [f32; 3],
    /// Float amplitude in blocks (0 = still).
    #[serde(default)]
    pub bob: f32,
    /// Direction of the float (a piston along any axis).
    #[serde(default = "part_axis")]
    pub bob_axis: [f32; 3],
    /// Rocking amplitude in degrees around `spin_axis` (0 = still): hammers,
    /// doors, jaws, pendulums.
    #[serde(default)]
    pub swing: f32,
    /// Scale amplitude around the pivot (0 = still): flames, hearts, cores.
    #[serde(default)]
    pub pulse: f32,
    /// Speed of the float, the rocking and the pulse (radians per second).
    #[serde(default = "part_speed")]
    pub speed: f32,
    /// Phase of the float, the rocking and the pulse (radians). Parts of one
    /// mechanism share theirs; a block is offset by its position in the world.
    #[serde(default)]
    pub phase: f32,
    #[serde(default)]
    pub motion: PartMotion,
}

fn part_pivot() -> [f32; 3] {
    [8.0, 8.0, 8.0]
}

fn part_axis() -> [f32; 3] {
    [0.0, 1.0, 0.0]
}

fn part_speed() -> f32 {
    2.2
}

impl BlockPart {
    /// A part from a centre and a size in block units, centre of the block at
    /// the origin.
    pub fn centered(name: &str, center: [f32; 3], size: [f32; 3], color: [u8; 3]) -> Self {
        let from = [0, 1, 2].map(|i| ((center[i] - size[i] * 0.5 + 0.5) * 16.0 * 1000.0).round() / 1000.0);
        let to = [0, 1, 2].map(|i| ((center[i] + size[i] * 0.5 + 0.5) * 16.0 * 1000.0).round() / 1000.0);
        Self {
            name: name.to_string(),
            from,
            to,
            rotation: [0.0; 3],
            color,
            emissive: 0.0,
            metallic: 0.0,
            powered_color: None,
            powered_emissive: 0.0,
            pivot: part_pivot(),
            spin: 0.0,
            spin_axis: part_axis(),
            bob: 0.0,
            bob_axis: part_axis(),
            swing: 0.0,
            pulse: 0.0,
            speed: part_speed(),
            phase: 0.0,
            motion: PartMotion::Always,
        }
    }

    /// Centre and size in block units (centre of the block at the origin).
    pub fn center_size(&self) -> (Vec3, Vec3) {
        let a = Vec3::from_array(self.from) / 16.0;
        let b = Vec3::from_array(self.to) / 16.0;
        ((a + b) * 0.5 - Vec3::splat(0.5), (b - a).abs().max(Vec3::splat(0.001)))
    }

    /// Moves by itself (or with its motor).
    pub fn is_animated(&self) -> bool {
        self.spin != 0.0 || self.bob != 0.0 || self.swing != 0.0 || self.pulse != 0.0 || self.motion == PartMotion::Motor
    }

    pub fn pivot_point(&self) -> Vec3 {
        Vec3::from_array(self.pivot) / 16.0 - Vec3::splat(0.5)
    }

    pub fn rotation_quat(&self) -> Quat {
        Quat::from_euler(
            EulerRot::YXZ,
            self.rotation[1].to_radians(),
            self.rotation[0].to_radians(),
            self.rotation[2].to_radians(),
        )
    }
}

/// How a block placed by the player is oriented.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockPlacement {
    /// Upright (upside down when placed under a block), its front facing the
    /// player.
    #[default]
    FacePlayer,
    /// Up along the clicked face (logs, pillars, ramps on walls), its front
    /// facing the player.
    AlignToSurface,
}

impl BlockPlacement {
    pub const ALL: [Self; 2] = [Self::FacePlayer, Self::AlignToSurface];

    pub const fn label(self) -> &'static str {
        match self {
            Self::FacePlayer => "Face the player",
            Self::AlignToSurface => "Align to the clicked face",
        }
    }
}

const fn default_flow_ticks() -> u32 {
    4
}

const fn default_flow_range() -> u8 {
    7
}

const fn default_liquid_opacity() -> f32 {
    0.72
}

const fn default_roughness() -> f32 {
    0.72
}

const fn default_hardness() -> f32 {
    0.8
}

const fn default_power_radius() -> f32 {
    8.0
}

impl BlockDef {
    pub fn new(id: &str, label: &str, shape: VoxelShape, color: [u8; 3]) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            shape,
            color,
            roughness: default_roughness(),
            metallic: 0.0,
            emissive: 0.0,
            voronoi: VoronoiCells::default(),
            material: SurfaceMaterial::default(),
            hardness: default_hardness(),
            drop: None,
            function: BlockFunction::None,
            components: Vec::new(),
            self_powered: false,
            break_sound: None,
            place_sound: None,
            script: None,
            power_radius: default_power_radius(),
            liquid: false,
            flow_ticks: default_flow_ticks(),
            flow_range: default_flow_range(),
            opacity: default_liquid_opacity(),
            normal: VoronoiNormal::default(),
            model: None,
            synthetic: false,
            variant_of: None,
            face_colors: Vec::new(),
            accent: None,
            part_colors: PartColors::default(),
            wind: 0.0,
            placement: BlockPlacement::FacePlayer,
            ports: Vec::new(),
            parts: None,
            hidden: false,
        }
    }

    /// The cable ports of the block (its own, or the defaults of its function).
    pub fn ports(&self) -> Vec<PortDef> {
        if !self.ports.is_empty() {
            return self.ports.clone();
        }
        let mut ports = self.function.default_ports();
        if self.script.is_some() {
            ports.push(PortDef::new(PortKind::Signal, PortDirection::Input, "Signal in"));
            ports.push(PortDef::new(PortKind::Signal, PortDirection::Output, "Signal out"));
        }
        ports
    }

    /// Inventory resource of the block (a variant counts as its base block).
    pub fn resource_id(&self) -> &str {
        self.variant_of.as_deref().unwrap_or(&self.id)
    }

    pub fn with_drop(mut self, drop: &str) -> Self {
        self.drop = Some(drop.to_string());
        self
    }

    pub fn with_wind(mut self, wind: f32) -> Self {
        self.wind = wind;
        self
    }

    pub fn as_synthetic(mut self, label: &str) -> Self {
        self.synthetic = true;
        self.label = label.to_string();
        self
    }

    /// Liquids flow and can be swum in. Older `water.ron` files predate the
    /// `liquid` flag, so the `water` key is always treated as a liquid.
    pub fn is_liquid(&self) -> bool {
        self.liquid || self.id == "water"
    }

    /// Custom model with at least one element.
    pub fn custom_model(&self) -> Option<&BlockModel> {
        self.model.as_ref().filter(|model| !model.is_empty())
    }

    /// Turn `face_colors` into the part colours of the shape.
    pub fn resolve_face_colors(&mut self) {
        if self.variant_of.is_some() {
            return;
        }
        self.part_colors = PartColors::default();
        if self.custom_model().is_some() {
            return;
        }
        for (label, color) in &self.face_colors {
            if let Some(part) = crate::voxel::shapes::shape_parts(self.shape).iter().position(|part| part.label == *label) {
                self.part_colors.set(part, *color);
            }
        }
    }

    /// Colours come from the vertices (painted parts or coloured elements):
    /// the material stays white, see `block_material`.
    pub fn has_vertex_colors(&self) -> bool {
        !self.part_colors.is_empty()
            || self.custom_model().is_some_and(|model| model.elements.iter().any(|element| element.color.is_some()))
    }

    /// Renders as a plain full cube (greedy meshing, full face cover).
    pub fn is_plain_cube(&self) -> bool {
        self.shape == VoxelShape::Cube && self.custom_model().is_none() && !self.is_liquid()
    }

    pub fn with_liquid(mut self, flow_ticks: u32, flow_range: u8, opacity: f32) -> Self {
        self.liquid = true;
        self.flow_ticks = flow_ticks;
        self.flow_range = flow_range;
        self.opacity = opacity;
        self
    }

    pub fn with_normal(mut self, normal: VoronoiNormal) -> Self {
        self.normal = normal;
        self
    }

    pub fn with_surface(mut self, material: SurfaceMaterial, hardness: f32) -> Self {
        self.material = material;
        self.hardness = hardness;
        self
    }

    pub fn with_function(mut self, function: BlockFunction) -> Self {
        self.function = function;
        self
    }

    pub fn with_voronoi(mut self, voronoi: VoronoiCells) -> Self {
        self.voronoi = voronoi;
        self
    }

    pub fn color_vec(&self) -> Vec3 {
        Vec3::new(
            self.color[0] as f32 / 255.0,
            self.color[1] as f32 / 255.0,
            self.color[2] as f32 / 255.0,
        )
    }

    pub fn with_material(mut self, roughness: f32, metallic: f32) -> Self {
        self.roughness = roughness;
        self.metallic = metallic;
        self
    }

    pub fn with_emissive(mut self, emissive: f32) -> Self {
        self.emissive = emissive;
        self
    }
}

/// Test blocks created by earlier versions (their file is removed on load).
pub const RETIRED_BLOCKS: [&str; 10] = [
    "nouveau_bloc_2",
    "beacon",
    "forge_table",
    "plinth",
    "plinth_trim",
    "tower",
    "cable_node",
    "coal_ore",
    "iron_ore",
    "gold_ore",
];

/// Result of scanning the block folder.
#[derive(Clone, Debug, Default)]
pub struct BlockLoadReport {
    pub loaded: usize,
    pub warnings: Vec<String>,
}

/// Registry shared by the mesher, the renderer, the brush and the editor.
#[derive(Resource, Debug, Clone)]
pub struct BlockRegistry {
    blocks: Vec<BlockDef>,
    lookup: HashMap<String, BlockId>,
    /// Bumped whenever the registry content changes.
    pub revision: u64,
}

impl Default for BlockRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl BlockRegistry {
    /// Deterministic fallback set used when the project folder has no files.
    pub fn builtin() -> Self {
        let blocks = builtin_defs();
        let lookup = blocks
            .iter()
            .enumerate()
            .map(|(index, def)| (def.id.clone(), BlockId(index as u16)))
            .collect();

        Self {
            blocks,
            lookup,
            revision: 0,
        }
    }

    pub fn blocks(&self) -> impl Iterator<Item = (BlockId, &BlockDef)> {
        self.blocks
            .iter()
            .enumerate()
            .map(|(index, def)| (BlockId(index as u16), def))
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn get(&self, id: BlockId) -> &BlockDef {
        self.blocks
            .get(id.0 as usize)
            .unwrap_or(&self.blocks[0])
    }

    pub fn find(&self, key: &str) -> Option<BlockId> {
        self.lookup.get(key).copied()
    }

    /// Look up a key, falling back to the first builtin block.
    pub fn id_or_default(&self, key: &str) -> BlockId {
        self.find(key).unwrap_or(BlockId(0))
    }

    /// Persist one block to `project/blocks/<id>.ron`.
    pub fn save_block(&self, dir: &Path, id: BlockId) -> Result<PathBuf, String> {
        let def = self.get(id);
        let path = dir.join(format!("{}.ron", def.id));
        save_ron(&path, def)?;
        Ok(path)
    }

    /// Insert a block or replace the definition sharing the same key.
    pub fn upsert(&mut self, mut def: BlockDef) -> BlockId {
        def.resolve_face_colors();
        if let Some(id) = self.lookup.get(&def.id).copied() {
            self.blocks[id.0 as usize] = def;
            self.revision = self.revision.wrapping_add(1);
            id
        } else {
            let id = BlockId(self.blocks.len() as u16);
            self.lookup.insert(def.id.clone(), id);
            self.blocks.push(def);
            self.revision = self.revision.wrapping_add(1);
            id
        }
    }

    /// Write one `.ron` file per builtin block that is not on disk yet.
    pub fn write_samples(&self, dir: &Path) -> Result<usize, String> {
        fs::create_dir_all(dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;

        let mut written = 0;
        for def in &self.blocks {
            let path = dir.join(format!("{}.ron", def.id));
            if path.exists() {
                continue;
            }
            save_ron(&path, def)?;
            written += 1;
        }

        Ok(written)
    }

    /// Load every `.ron` block file, overriding builtins with the same key.
    pub fn load_from_dir(&mut self, dir: &Path) -> Result<BlockLoadReport, String> {
        fs::create_dir_all(dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;

        let mut paths: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|error| format!("cannot read {}: {error}", dir.display()))?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .map(|extension| extension.eq_ignore_ascii_case("ron"))
                    .unwrap_or(false)
            })
            .collect();
        paths.sort();

        let mut report = BlockLoadReport::default();
        for path in paths {
            // Test blocks of earlier versions are removed.
            let stem = path.file_stem().map(|stem| stem.to_string_lossy().to_string()).unwrap_or_default();
            if RETIRED_BLOCKS.contains(&stem.as_str()) {
                let _ = fs::remove_file(&path);
                continue;
            }
            match load_ron::<BlockDef>(&path) {
                Ok(def) if def.id.trim().is_empty() => {
                    report
                        .warnings
                        .push(format!("{}: block id is empty", path.display()));
                }
                Ok(def) => {
                    // A block is exactly its file: no part or model is added
                    // by the code (they are made in the Forge).
                    self.upsert(def);
                    report.loaded += 1;
                }
                Err(error) => report.warnings.push(error),
            }
        }

        Ok(report)
    }
}

/// Startup system: create sample files, then load the project block library.
/// Colours painted on the parts of a shape (see `shapes::shape_parts`). A
/// part without colour keeps the block colour.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PartColors {
    painted: u16,
    colors: [[u8; 3]; MAX_SHAPE_PARTS],
}

impl PartColors {
    pub fn is_empty(&self) -> bool {
        self.painted == 0
    }

    pub fn get(&self, part: usize) -> Option<[u8; 3]> {
        (part < MAX_SHAPE_PARTS && self.painted & (1 << part) != 0).then(|| self.colors[part])
    }

    pub fn set(&mut self, part: usize, color: [u8; 3]) {
        if part < MAX_SHAPE_PARTS {
            self.painted |= 1 << part;
            self.colors[part] = color;
        }
    }

    pub fn clear(&mut self, part: usize) {
        if part < MAX_SHAPE_PARTS {
            self.painted &= !(1 << part);
            self.colors[part] = [0; 3];
        }
    }

    /// Seven hex digits per painted part: its index, then its colour.
    fn encode(&self) -> String {
        (0..MAX_SHAPE_PARTS)
            .filter_map(|part| self.get(part).map(|c| format!("{part:x}{:02x}{:02x}{:02x}", c[0], c[1], c[2])))
            .collect()
    }

    fn decode(text: &str) -> Option<Self> {
        if !text.is_ascii() || text.len() % 7 != 0 {
            return None;
        }
        let mut parts = Self::default();
        for entry in text.as_bytes().chunks(7) {
            let entry = std::str::from_utf8(entry).ok()?;
            let byte = |i: usize| u8::from_str_radix(&entry[i..i + 2], 16).ok();
            let part = usize::from_str_radix(&entry[..1], 16).ok()?;
            parts.set(part, [byte(1)?, byte(3)?, byte(5)?]);
        }
        Some(parts)
    }
}

/// Look chosen for a customised synthetic block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyntheticLook {
    pub color: [u8; 3],
    pub voronoi: VoronoiCells,
    pub normal: VoronoiNormal,
    /// Parts painted in another colour than `color`.
    pub parts: PartColors,
}

impl SyntheticLook {
    pub fn of(def: &BlockDef) -> Self {
        Self { color: def.color, voronoi: def.voronoi, normal: def.normal, parts: def.part_colors }
    }

    /// Stable suffix of the variant key
    /// (`rrggbb~cells~strength~size~bevel~seed~on`, then `~parts` when some
    /// parts are painted).
    fn encode(&self) -> String {
        let cells = VoronoiCells::ALL.iter().position(|c| *c == self.voronoi).unwrap_or(2);
        let parts = if self.parts.is_empty() { String::new() } else { format!("~{}", self.parts.encode()) };
        format!(
            "{:02x}{:02x}{:02x}~{}~{}~{}~{}~{}~{}{}",
            self.color[0],
            self.color[1],
            self.color[2],
            cells,
            (self.normal.strength * 100.0).round() as i32,
            (self.normal.cells * 100.0).round() as i32,
            (self.normal.bevel * 100.0).round() as i32,
            self.normal.seed.round() as i32,
            u8::from(self.normal.enabled),
            parts,
        )
    }

    fn decode(text: &str) -> Option<Self> {
        let parts: Vec<&str> = text.split('~').collect();
        if !(7..=8).contains(&parts.len()) || parts[0].len() != 6 {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&parts[0][i..i + 2], 16).ok();
        let number = |i: usize| parts[i].parse::<i32>().ok();
        Some(Self {
            color: [byte(0)?, byte(2)?, byte(4)?],
            voronoi: *VoronoiCells::ALL.get(number(1)? as usize)?,
            normal: VoronoiNormal {
                enabled: number(6)? != 0,
                strength: number(2)? as f32 / 100.0,
                cells: number(3)? as f32 / 100.0,
                bevel: number(4)? as f32 / 100.0,
                seed: number(5)? as f32,
            },
            parts: match parts.get(7) {
                Some(painted) => PartColors::decode(painted)?,
                None => PartColors::default(),
            },
        })
    }
}

impl BlockRegistry {
    /// Key of the variant of `base` with `look`.
    pub fn variant_key(base: &str, look: &SyntheticLook) -> String {
        format!("{base}~{}", look.encode())
    }

    /// Definition of a variant (not registered).
    pub fn variant_def(&self, base: BlockId, look: &SyntheticLook) -> BlockDef {
        let source = self.get(base);
        let root = source.variant_of.clone().unwrap_or_else(|| source.id.clone());
        let mut def = self.find(&root).map(|id| self.get(id).clone()).unwrap_or_else(|| source.clone());
        def.id = Self::variant_key(&root, look);
        def.variant_of = Some(root);
        def.color = look.color;
        def.voronoi = look.voronoi;
        def.normal = look.normal;
        // Custom models have no shape parts to paint.
        def.part_colors = if def.custom_model().is_some() { PartColors::default() } else { look.parts };
        def.synthetic = true;
        def
    }

    /// Find or register the variant behind `key` (`base~look`). Adding a
    /// variant never bumps the revision: existing meshes stay valid.
    pub fn ensure_variant(&mut self, key: &str) -> Option<BlockId> {
        if let Some(id) = self.find(key) {
            return Some(id);
        }
        let (base, look) = key.split_once('~')?;
        let base_id = self.find(base)?;
        if !self.get(base_id).synthetic {
            return None;
        }
        let look = SyntheticLook::decode(look)?;
        let def = self.variant_def(base_id, &look);
        let id = BlockId(self.blocks.len() as u16);
        self.lookup.insert(def.id.clone(), id);
        self.blocks.push(def);
        Some(id)
    }
}

pub fn load_block_registry(
    project: Res<ProjectLayout>,
    mut registry: ResMut<BlockRegistry>,
    mut ui: ResMut<EditorUiState>,
) {
    let dir = project.module_path("blocks");

    match registry.write_samples(&dir) {
        Ok(written) if written > 0 => {
            ui.info(format!("Wrote {written} sample block file(s) to {}", dir.display()));
        }
        Ok(_) => {}
        Err(error) => ui.warn(format!("Could not write sample blocks: {error}")),
    }

    match registry.load_from_dir(&dir) {
        Ok(report) => {
            for warning in report.warnings {
                ui.warn(warning);
            }
            ui.info(format!(
                "Block registry ready: {} blocks from {} files.",
                registry.len(),
                report.loaded
            ));
        }
        Err(error) => ui.error(format!("Block registry failed: {error}")),
    }
}

fn builtin_defs() -> Vec<BlockDef> {
    use BlockFunction as F;
    use SurfaceMaterial as M;
    vec![
        // --- Synthetic blocks: the nine customisable shapes -----------------
        BlockDef::new("cube_teal", "Cube", VoxelShape::Cube, [78, 210, 190]).as_synthetic("Synthetic Cube"),
        BlockDef::new("wedge_blue", "Ramp", VoxelShape::Ramp, [102, 178, 255]).as_synthetic("Synthetic Ramp"),
        BlockDef::new("inner_violet", "Ramp In Corner", VoxelShape::RampInCorner, [148, 140, 255])
            .as_synthetic("Synthetic Ramp In Corner"),
        BlockDef::new("outer_magenta", "Ramp Out Corner", VoxelShape::RampOutCorner, [216, 128, 255])
            .as_synthetic("Synthetic Ramp Out Corner"),
        BlockDef::new("cylinder_pink", "Stairs In Corner", VoxelShape::StairsInCorner, [255, 132, 186])
            .as_synthetic("Synthetic Stairs In Corner"),
        BlockDef::new("half_cylinder_peach", "H Half Cube", VoxelShape::HHalfCube, [255, 160, 112])
            .as_synthetic("Synthetic Slab"),
        BlockDef::new("sphere_gold", "Stairs Out Corner", VoxelShape::StairsOutCorner, [248, 205, 96])
            .as_synthetic("Synthetic Stairs Out Corner"),
        BlockDef::new("arch_lime", "Stairs", VoxelShape::Stairs, [164, 224, 104]).as_synthetic("Synthetic Stairs"),
        BlockDef::new("pillar_green", "V Half Cube", VoxelShape::VHalfCube, [90, 206, 132]).as_synthetic("Synthetic Wall"),
        BlockDef::new("grass", "Grass", VoxelShape::Cube, [44, 196, 52]).with_surface(M::Grass, 0.6),
        BlockDef::new("grass_dark", "Dark Grass", VoxelShape::Cube, [34, 168, 44]).with_surface(M::Grass, 0.6),
        BlockDef::new("dirt", "Dirt", VoxelShape::Cube, [124, 92, 66])
            .with_material(0.92, 0.0)
            .with_surface(M::Dirt, 0.5),
        BlockDef::new("stone", "Stone", VoxelShape::Cube, [92, 102, 118])
            .with_material(0.88, 0.02)
            .with_surface(M::Stone, 1.5),
        BlockDef::new("sand", "Sand", VoxelShape::Cube, [214, 190, 128])
            .with_material(0.94, 0.0)
            .with_surface(M::Sand, 0.4),
        // --- Terrain palette used by the biome generator -----------------
        BlockDef::new("slab_stone", "Stone Slab", VoxelShape::HHalfCube, [104, 114, 130])
            .with_surface(M::Stone, 1.2),
        BlockDef::new("snow", "Snow", VoxelShape::Cube, [232, 240, 248]).with_surface(M::Sand, 0.3),
        BlockDef::new("ice", "Ice", VoxelShape::Cube, [160, 210, 240])
            .with_material(0.12, 0.0)
            .with_surface(M::Ice, 0.7)
            .with_voronoi(VoronoiCells::Cells3)
            .with_normal(VoronoiNormal::with(0.5, 2.0, 0.05)),
        BlockDef::new("mud", "Mud", VoxelShape::Cube, [92, 70, 52]).with_surface(M::Dirt, 0.4),
        BlockDef::new("red_sand", "Red Sand", VoxelShape::Cube, [196, 112, 72]).with_surface(M::Sand, 0.5),
        BlockDef::new("basalt", "Basalt", VoxelShape::Cube, [46, 44, 52]).with_surface(M::Stone, 2.2),
        BlockDef::new("lava", "Lava Rock", VoxelShape::Cube, [240, 96, 40])
            .with_emissive(1.6)
            .with_surface(M::Stone, 3.0),
        BlockDef::new("water", "Water", VoxelShape::Cube, [64, 132, 210])
            .with_material(0.08, 0.0)
            .with_surface(M::Glass, 9999.0)
            .with_voronoi(VoronoiCells::Cells30)
            .with_liquid(4, 7, 0.68)
            .with_normal(VoronoiNormal::with(0.18, 2.0, 0.4)),
        BlockDef::new("log", "Log", VoxelShape::Cube, [214, 110, 46]).with_surface(M::Wood, 1.2),
        BlockDef::new("leaves", "Leaves", VoxelShape::Cube, [60, 214, 70])
            .with_surface(M::Grass, 0.2)
            .with_voronoi(VoronoiCells::Cells30)
            .with_wind(0.7),
        BlockDef::new("pine_leaves", "Pine Leaves", VoxelShape::Cube, [30, 150, 64])
            .with_surface(M::Grass, 0.2)
            .with_voronoi(VoronoiCells::Cells30)
            .with_wind(0.45),
        BlockDef::new("alien_crust", "Alien Crust", VoxelShape::Cube, [120, 60, 180])
            .with_emissive(0.4)
            .with_surface(M::Stone, 1.8)
            .with_voronoi(VoronoiCells::Cells200),
        BlockDef::new("mnemo_rot", "Mnemo Contamination", VoxelShape::Cube, [190, 44, 60])
            .with_emissive(0.8)
            .with_surface(M::Dirt, 1.0)
            .with_voronoi(VoronoiCells::Cells200),
        // --- Machines, movables and vehicles ------------------------------
        BlockDef::new("frame", "Movable", VoxelShape::Cube, [150, 156, 168])
            .with_material(0.4, 0.6)
            .with_surface(M::Metal, 1.0)
            .with_function(F::Movable),
        BlockDef::new("motor", "Motor", VoxelShape::Cube, [220, 120, 60])
            .with_material(0.35, 0.7)
            .with_surface(M::Metal, 1.2)
            .with_function(F::Motor),
        BlockDef::new("controller", "Controller", VoxelShape::Cube, [70, 180, 240])
            .with_material(0.3, 0.5)
            .with_surface(M::Metal, 1.0)
            .with_function(F::Controller),
        BlockDef::new("wagon_seat", "Wagon", VoxelShape::Stairs, [180, 70, 70])
            .with_surface(M::Wood, 0.8)
            .with_function(F::Wagon),
        BlockDef::new("combustor", "Combustor", VoxelShape::Cube, [200, 80, 50])
            .with_emissive(0.6)
            .with_material(0.5, 0.5)
            .with_surface(M::Metal, 2.0)
            .with_function(F::Combustor),
        BlockDef::new("solar", "Solaronel", VoxelShape::HHalfCube, [40, 60, 110])
            .with_material(0.15, 0.8)
            .with_surface(M::Glass, 1.0)
            .with_function(F::Solar),
        BlockDef::new("synthesizer", "Synthesizer", VoxelShape::Cube, [120, 220, 200])
            .with_material(0.3, 0.6)
            .with_surface(M::Metal, 2.0)
            .with_function(F::Synthesizer),
        BlockDef::new("infuser", "Infuser", VoxelShape::Cube, [200, 140, 240])
            .with_material(0.3, 0.6)
            .with_surface(M::Metal, 2.0)
            .with_function(F::Infuser),
        BlockDef::new("vacuum", "Astral-Vacum", VoxelShape::Cube, [110, 110, 120])
            .with_material(0.3, 0.6)
            .with_surface(M::Metal, 2.0)
            .with_function(F::Vacuum),
        BlockDef::new("harvester", "Mnemo-Harvestor", VoxelShape::Cube, [210, 190, 80])
            .with_material(0.3, 0.6)
            .with_surface(M::Metal, 2.0)
            .with_function(F::Harvester),
        BlockDef::new("energizer", "Energiser", VoxelShape::Cube, [70, 120, 230])
            .with_emissive(0.3)
            .with_material(0.3, 0.7)
            .with_surface(M::Metal, 1.5)
            .with_function(F::Energizer),
        BlockDef::new("container", "Container", VoxelShape::Cube, [150, 110, 70])
            .with_material(0.6, 0.2)
            .with_surface(M::Wood, 1.2)
            .with_function(F::Container),
        BlockDef::new("lamp", "Light", VoxelShape::Cube, [255, 236, 170])
            .with_emissive(0.2)
            .with_surface(M::Glass, 0.6)
            .with_function(F::Lamp),
        // --- Liquids and forge samples -------------------------------------
        BlockDef::new("molten_lava", "Molten Lava", VoxelShape::Cube, [255, 110, 30])
            .with_emissive(2.2)
            .with_material(0.4, 0.0)
            .with_surface(M::Stone, 9999.0)
            .with_liquid(12, 4, 0.95)
            .with_normal(VoronoiNormal::with(0.3, 2.5, 0.3)),
        // --- Caves (deep layers, ores, lush floors) -----------------------
        BlockDef::new("deepslate", "Deepslate", VoxelShape::Cube, [62, 64, 76])
            .with_material(0.9, 0.02)
            .with_surface(M::Stone, 2.4),
        BlockDef::new("crystal_ore", "Crystal Ore", VoxelShape::Cube, [110, 230, 240])
            .with_emissive(1.4)
            .with_material(0.2, 0.3)
            .with_surface(M::Glass, 3.5)
            .with_voronoi(VoronoiCells::Cells3)
            .with_drop("crystalline"),
        BlockDef::new("moss", "Moss", VoxelShape::Cube, [70, 160, 60]).with_surface(M::Grass, 0.4),
        BlockDef::new("glow_moss", "Glow Moss", VoxelShape::HHalfCube, [150, 240, 120])
            .with_emissive(1.2)
            .with_surface(M::Grass, 0.2)
            .with_wind(0.3),
        // --- Crafting --------------------------------------------------------
        BlockDef::new("assembler", "Assembler", VoxelShape::Cube, [96, 120, 150])
            .with_material(0.35, 0.6)
            .with_surface(M::Metal, 2.0)
            .with_function(F::Assembler),
        BlockDef::new("omnichorus", "Omnichorus", VoxelShape::Cube, [235, 240, 255])
            .with_emissive(0.9)
            .with_material(0.2, 0.7)
            .with_surface(M::Metal, 3.0)
            .with_function(F::Omnichorus),
        // --- Spec terrain: resource nodes and new biomes ---------------------
        BlockDef::new("mineral_ore", "Mineral Vein", VoxelShape::Cube, [128, 134, 150])
            .with_surface(M::Stone, 2.0)
            .with_voronoi(VoronoiCells::Cells30)
            .with_drop("mineral"),
        BlockDef::new("astral_ore", "Astral Ore", VoxelShape::Cube, [255, 236, 150])
            .with_emissive(2.0)
            .with_material(0.2, 0.6)
            .with_surface(M::Glass, 5.0)
            .with_voronoi(VoronoiCells::Cells3)
            .with_drop("astral"),
        BlockDef::new("mnemonical_ore", "Mnemonical Ore", VoxelShape::Cube, [255, 70, 90])
            .with_emissive(2.2)
            .with_material(0.2, 0.3)
            .with_surface(M::Glass, 5.0)
            .with_voronoi(VoronoiCells::Cells3)
            .with_drop("mnemonical"),
        BlockDef::new("burnt_ground", "Burnt Ground", VoxelShape::Cube, [52, 44, 40]).with_surface(M::Dirt, 0.6),
        BlockDef::new("burnt_log", "Burnt Log", VoxelShape::Cube, [40, 32, 30]).with_surface(M::Wood, 1.0),
        BlockDef::new("sandstone", "Sandstone", VoxelShape::Cube, [222, 196, 136]).with_surface(M::Stone, 1.4),
        BlockDef::new("temple_brick", "Temple Brick", VoxelShape::Cube, [150, 140, 104])
            .with_material(0.7, 0.05)
            .with_surface(M::Stone, 2.5),
        BlockDef::new("palm_leaves", "Palm Leaves", VoxelShape::Cube, [96, 200, 80])
            .with_surface(M::Grass, 0.2)
            .with_voronoi(VoronoiCells::Cells30)
            .with_wind(0.6),
        // --- Spec machines ----------------------------------------------------
        machine("repulsor", "Repulsor", [90, 200, 255], F::Repulsor),
        machine("terminal", "Terminal", [40, 48, 60], F::Terminal),
        machine("comandor", "Comandor", [60, 70, 96], F::Comandor),
        machine("transmitter", "Transmitter", [110, 130, 230], F::Transmitter),
        machine("visualiser", "Visualiser", [30, 34, 40], F::Visualiser),
        machine("emitter", "Emitter", [90, 170, 255], F::Emitter),
        machine("receiver", "Receiver", [80, 140, 210], F::Receiver),
        machine("screen", "Screen", [20, 22, 26], F::Screen),
        machine("atomiser", "Atomiser", [240, 240, 255], F::Atomiser).with_emissive(0.6),
        machine("ressourcer", "Ressourcer", [120, 200, 120], F::Ressourcer),
        machine("ringer", "Ringer", [240, 200, 80], F::Ringer),
        machine("laser", "Laser", [230, 60, 60], F::Laser),
        machine("etheraltar", "Etheraltar", [200, 150, 255], F::Etheraltar).with_emissive(0.9),
        machine("kineticster", "Kineticster", [120, 120, 136], F::Kineticster),
        machine("reconstructor", "Reconstructor", [180, 230, 255], F::Reconstructor).with_emissive(0.4),
        machine("excavator", "Excavator", [200, 150, 60], F::Excavator),
        machine("ethereaper", "Ethereaper", [150, 90, 220], F::Ethereaper).with_emissive(0.5),
        machine("omnireactor", "Omnireactor", [255, 255, 255], F::Omnireactor).with_emissive(1.2),
        machine("invocateral", "Invocateral", [170, 110, 240], F::Invocateral).with_emissive(0.7),
        machine("televerseur", "Televerseur", [90, 220, 210], F::Televerseur),
        machine("mnemo_digestor", "Mnemo-Digestor", [190, 50, 66], F::MnemoDigestor).with_emissive(0.6),
        machine("mnemo_cloner", "Mnemo-Cloner", [220, 60, 80], F::MnemoCloner).with_emissive(0.8),
        machine("astral_modifier", "Astral-Modifier", [255, 220, 120], F::AstralModifier).with_emissive(0.8),
        machine("astral_container", "Astral-Container", [255, 236, 170], F::AstralContainer).with_emissive(0.5),
    ]
}

/// A plain metal machine block.
fn machine(id: &str, label: &str, color: [u8; 3], function: BlockFunction) -> BlockDef {
    BlockDef::new(id, label, VoxelShape::Cube, color)
        .with_material(0.35, 0.6)
        .with_surface(SurfaceMaterial::Metal, 2.0)
        .with_function(function)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILTIN_COUNT: usize = 79;

    /// The blocks shipped in `project/` all parse, and the machines moved.
    #[test]
    fn shipped_blocks_load_without_warning() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("project/blocks");
        let mut registry = BlockRegistry::builtin();
        let report = registry.load_from_dir(&directory).expect("block folder");
        assert!(report.warnings.is_empty(), "{:#?}", report.warnings);
        assert!(report.loaded >= BUILTIN_COUNT, "{} blocks loaded", report.loaded);
        let animated = registry
            .blocks()
            .filter(|(_, def)| def.parts.iter().flatten().any(BlockPart::is_animated))
            .count();
        assert!(animated >= 30, "only {animated} blocks are animated");
    }

    #[test]
    fn builtin_registry_exposes_every_shape() {
        let registry = BlockRegistry::builtin();
        assert_eq!(registry.len(), BUILTIN_COUNT);

        for shape in VoxelShape::ALL {
            assert!(
                registry.blocks().any(|(_, def)| def.shape == shape),
                "no builtin block for {}",
                shape.label()
            );
        }
    }

    #[test]
    fn synthetic_variants_round_trip_through_their_key() {
        let mut registry = BlockRegistry::builtin();
        let base = registry.find("cube_teal").expect("synthetic cube");
        let look = SyntheticLook {
            color: [12, 200, 99],
            voronoi: VoronoiCells::Cells30,
            normal: VoronoiNormal { enabled: true, strength: 0.6, cells: 4.5, bevel: 0.2, seed: 7.0 },
            parts: PartColors::default(),
        };
        let key = BlockRegistry::variant_key("cube_teal", &look);
        let revision = registry.revision;
        let id = registry.ensure_variant(&key).expect("variant");
        assert_ne!(id, base);
        assert_eq!(registry.revision, revision);
        let def = registry.get(id);
        assert_eq!(def.color, look.color);
        assert_eq!(def.resource_id(), "cube_teal");
        assert_eq!(SyntheticLook::of(def), look);
        assert_eq!(registry.ensure_variant(&key), Some(id));
        // Only synthetic blocks have variants.
        assert!(registry.ensure_variant("stone~ffffff~2~35~300~12~0~1").is_none());
    }

    #[test]
    fn face_colours_of_a_block_file_reach_its_parts() {
        let mut registry = BlockRegistry::builtin();
        let mut def = BlockDef::new("test_turf", "Turf", VoxelShape::Cube, [120, 90, 60]);
        def.face_colors = vec![("Top".to_string(), [40, 200, 50]), ("Nowhere".to_string(), [1, 1, 1])];
        let id = registry.upsert(def);
        let def = registry.get(id);
        assert!(def.has_vertex_colors());
        let top = crate::voxel::shapes::cube_part_facing(Vec3::Y);
        assert_eq!(def.part_colors.get(top), Some([40, 200, 50]));
        let text = ron::to_string(def).expect("written");
        let reloaded: BlockDef = ron::from_str(&text).expect("reloaded");
        assert_eq!(reloaded.face_colors, def.face_colors);
    }

    #[test]
    fn element_colours_survive_the_block_file() {
        let source = "(\n    id: \"test\",\n    label: \"Test\",\n    shape: Cube,\n    color: (10, 20, 30),\n    model: Some((elements: [(name: \"Old\", from: (0.0, 0.0, 0.0), to: (16.0, 8.0, 16.0))])),\n)\n";
        let mut def: BlockDef = ron::from_str(source).expect("older block file");
        assert!(!def.has_vertex_colors());
        def.model.as_mut().expect("model").elements[0].color = Some([255, 0, 0]);
        assert!(def.has_vertex_colors());
        let reloaded: BlockDef = ron::from_str(&ron::to_string(&def).expect("written")).expect("reloaded");
        assert_eq!(reloaded.model, def.model);
    }

    #[test]
    fn painted_parts_round_trip_through_the_variant_key() {
        let mut registry = BlockRegistry::builtin();
        let mut look = SyntheticLook::of(registry.get(registry.find("arch_lime").expect("stairs")));
        let plain = BlockRegistry::variant_key("arch_lime", &look);
        look.parts.set(0, [255, 0, 16]);
        look.parts.set(11, [1, 2, 3]);
        let key = BlockRegistry::variant_key("arch_lime", &look);
        assert_ne!(key, plain);
        // Keys saved before parts existed still load.
        assert!(registry.ensure_variant(&plain).is_some());
        let id = registry.ensure_variant(&key).expect("variant");
        assert_eq!(registry.get(id).part_colors, look.parts);
        assert_eq!(SyntheticLook::of(registry.get(id)), look);
        assert!(registry.ensure_variant(&format!("{key}zz")).is_none());
        look.parts.clear(0);
        look.parts.clear(11);
        assert!(look.parts.is_empty());
        assert_eq!(BlockRegistry::variant_key("arch_lime", &look), plain);
    }

    #[test]
    fn block_definition_parses_from_ron() {
        let source = "(\n    id: \"test\",\n    label: \"Test\",\n    shape: Wedge,\n    color: (10, 20, 30),\n)\n";
        let def: BlockDef = ron::from_str(source).expect("valid block definition");
        assert_eq!(def.id, "test");
        assert_eq!(def.shape, VoxelShape::Ramp);
        assert_eq!(def.color, [10, 20, 30]);
        assert!((def.roughness - default_roughness()).abs() < f32::EPSILON);
    }

    #[test]
    fn upsert_replaces_known_keys_and_appends_new_ones() {
        let mut registry = BlockRegistry::builtin();
        let stone = registry.find("stone").expect("builtin stone");
        let replacement = BlockDef::new("stone", "Basalt", VoxelShape::Cube, [20, 20, 24]);

        let id = registry.upsert(replacement);
        assert_eq!(id, stone);
        assert_eq!(registry.get(stone).label, "Basalt");
        assert_eq!(registry.len(), BUILTIN_COUNT);

        let added = registry.upsert(BlockDef::new(
            "glass",
            "Glass",
            VoxelShape::Cube,
            [200, 230, 255],
        ));
        assert_eq!(registry.len(), BUILTIN_COUNT + 1);
        assert_eq!(registry.find("glass"), Some(added));
        assert_eq!(registry.get(added).label, "Glass");
    }
}
