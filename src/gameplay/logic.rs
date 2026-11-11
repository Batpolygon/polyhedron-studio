//! Logic machines: typed values (Boolean / Float / String) travelling on
//! Signal cables.
//!
//! - **Emitter**: emits its configured value.
//! - **Transmitter**: logic gate on its inputs A and B (AND, OR, XOR, NOT,
//!   NAND, NOR) or **Decoder**: the resources it receives on its Resource
//!   input become a Boolean (anything received), a Float (count) or a
//!   String (resource name).
//! - **Visualiser**: shows the value of the output plugged into it.
//! - **Receiver**: stores the value it receives into a command variable.
//! - **Screen**: shows a colour from its input (false red / true green, a
//!   Float as a hue, a String as a colour name or `#rrggbb`).
//! - **Laser**: line trace from its front: hit (Boolean), length (Float) or
//!   the name of the block / entity hit (String).
//! - **Terminal**: runs its command lines one by one in a loop while powered
//!   (output: success of the last command). **Comandor**: runs its command
//!   when its button is pressed or its Trigger input turns true.
//!
//! Values are evaluated once per frame from the previous frame's outputs, so
//! loops are allowed and settle in a few frames.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::core::pie_manager::{PieOnly, PieSet, PieState};
use crate::gameplay::data_cables::CableNetwork;
use crate::gameplay::energy_grid::EnergyGrid;
use crate::gameplay::machines::config::{MachineConfigs, DECODER};
use crate::voxel::blocks::{BlockFunction, BlockRegistry, PortKind};
use crate::voxel::world::VoxelWorld;

/// A value carried by a Signal cable.
#[derive(Clone, Debug, PartialEq)]
pub enum SignalValue {
    Bool(bool),
    Float(f32),
    Text(String),
}

impl Default for SignalValue {
    fn default() -> Self {
        Self::Bool(false)
    }
}

impl SignalValue {
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(value) => *value,
            Self::Float(value) => value.abs() > f32::EPSILON,
            Self::Text(text) => !text.is_empty() && !text.eq_ignore_ascii_case("false") && text != "0",
        }
    }

    pub fn as_f32(&self) -> f32 {
        match self {
            Self::Bool(value) => f32::from(u8::from(*value)),
            Self::Float(value) => *value,
            Self::Text(text) => text.trim().parse().unwrap_or(0.0),
        }
    }

    pub fn as_text(&self) -> String {
        match self {
            Self::Bool(value) => value.to_string(),
            Self::Float(value) => format!("{value}"),
            Self::Text(text) => text.clone(),
        }
    }

    /// Parse a command argument: `true` / `false`, a number, else a string.
    pub fn parse(text: &str) -> Self {
        match text.to_ascii_lowercase().as_str() {
            "true" | "on" => Self::Bool(true),
            "false" | "off" => Self::Bool(false),
            _ => text.parse::<f32>().map(Self::Float).unwrap_or_else(|_| Self::Text(text.to_string())),
        }
    }

    /// Kind name shown by the Visualiser.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Bool(_) => "Boolean",
            Self::Float(_) => "Float",
            Self::Text(_) => "String",
        }
    }

    /// Colour shown by a Screen.
    pub fn color(&self) -> Color {
        match self {
            Self::Bool(true) => Color::srgb(0.2, 0.9, 0.35),
            Self::Bool(false) => Color::srgb(0.9, 0.2, 0.2),
            Self::Float(value) => Color::hsl((value.rem_euclid(1.0)) * 360.0, 0.85, 0.55),
            Self::Text(text) => named_color(text).unwrap_or(Color::srgb(0.85, 0.85, 0.9)),
        }
    }
}

/// `red`, `green`, ... or `#rrggbb`.
pub fn named_color(text: &str) -> Option<Color> {
    let text = text.trim().to_ascii_lowercase();
    if let Some(hex) = text.strip_prefix('#') {
        if hex.len() == 6 {
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            return Some(Color::srgb_u8(byte(0)?, byte(2)?, byte(4)?));
        }
        return None;
    }
    let rgb = match text.as_str() {
        "red" => [230, 50, 50],
        "green" => [60, 220, 90],
        "blue" => [60, 110, 240],
        "yellow" => [240, 220, 60],
        "orange" => [245, 150, 50],
        "purple" | "violet" => [170, 90, 240],
        "pink" => [240, 120, 190],
        "cyan" => [80, 220, 240],
        "white" => [245, 245, 245],
        "black" => [15, 15, 18],
        "grey" | "gray" => [130, 130, 140],
        _ => return None,
    };
    Some(Color::srgb_u8(rgb[0], rgb[1], rgb[2]))
}

/// Output of every logic machine and the command variables.
#[derive(Resource, Debug, Default)]
pub struct LogicState {
    pub outputs: HashMap<IVec3, SignalValue>,
    /// Last input value seen by each logic machine (Visualiser, Screen).
    pub inputs: HashMap<IVec3, SignalValue>,
    /// Variables of the command language (`/assignvar`, Receivers).
    pub variables: HashMap<String, SignalValue>,
    /// Laser beams to draw: (from, to, hit).
    pub beams: Vec<(Vec3, Vec3, bool)>,
}

impl LogicState {
    /// Value seen on an output port: a logic machine, else a script signal.
    fn value_of(&self, cell: IVec3, network: &CableNetwork) -> Option<SignalValue> {
        self.outputs.get(&cell).cloned().or_else(|| network.values.get(&cell).map(|value| SignalValue::Float(*value)))
    }
}

/// Coloured face of a Screen.
#[derive(Component)]
pub struct ScreenFace {
    pub cell: IVec3,
}

/// Logic evaluation (the command runner reads its queue after it).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LogicSet;

pub struct LogicPlugin;

impl Plugin for LogicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LogicState>().add_systems(
            Update,
            (evaluate_logic, sync_screens, draw_beams).chain().in_set(LogicSet).after(PieSet::Transition).after(crate::gameplay::energy_grid::GridSet),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn evaluate_logic(
    time: Res<Time>,
    pie: Res<PieState>,
    grid: Res<EnergyGrid>,
    network: Res<CableNetwork>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    mut configs: ResMut<MachineConfigs>,
    mut state: ResMut<LogicState>,
    mut queue: ResMut<crate::gameplay::commands::CommandQueue>,
    mobs: Query<(&crate::gameplay::mobs::runtime::MobInstance, &Transform)>,
) {
    if pie.started_this_frame || pie.stopped_this_frame {
        let variables = std::mem::take(&mut state.variables);
        *state = LogicState::default();
        if pie.persist_world.is_some() {
            state.variables = variables;
        }
    }
    if !pie.is_playing() {
        return;
    }
    let delta = time.delta_secs();
    state.beams.clear();
    let signal_cables: Vec<(IVec3, IVec3, u8)> = network
        .cables
        .iter()
        .filter(|cable| cable.kind == PortKind::Signal)
        .map(|cable| (IVec3::from_array(cable.from), IVec3::from_array(cable.to), cable.to_port))
        .collect();
    let mut next: HashMap<IVec3, SignalValue> = HashMap::new();
    let cells: Vec<(IVec3, BlockFunction, bool)> = grid
        .machines
        .iter()
        .filter(|(_, machine)| machine.function.is_logic())
        .map(|(cell, machine)| (*cell, machine.function, machine.powered))
        .collect();

    for (cell, function, powered) in cells {
        // Inputs by port index.
        let mut inputs: HashMap<u8, SignalValue> = HashMap::new();
        for (from, to, port) in &signal_cables {
            if *to == cell {
                if let Some(value) = state.value_of(*from, &network) {
                    inputs.insert(*port, value);
                }
            }
        }
        let first = inputs.values().next().cloned();
        let config = configs.get_or_default(cell, function);
        match function {
            BlockFunction::Emitter => {
                let value = match config.mode {
                    0 => SignalValue::Bool(config.enabled),
                    1 => SignalValue::Float(config.number),
                    _ => SignalValue::Text(config.text.clone()),
                };
                next.insert(cell, value);
            }
            BlockFunction::Transmitter => {
                let value = if config.mode == DECODER {
                    let storage = grid.machine(cell).map(|machine| machine.storage.clone()).unwrap_or_default();
                    let count: u32 = storage.values().sum();
                    match config.target.as_str() {
                        "bool" => SignalValue::Bool(count > 0),
                        "string" => SignalValue::Text(storage.keys().next().cloned().unwrap_or_default()),
                        _ => SignalValue::Float(count as f32),
                    }
                } else {
                    let a = inputs.get(&0).map(SignalValue::as_bool).unwrap_or(false);
                    let b = inputs.get(&1).map(SignalValue::as_bool).unwrap_or(false);
                    SignalValue::Bool(match config.mode {
                        0 => a && b,
                        1 => a || b,
                        2 => a ^ b,
                        3 => !a,
                        4 => !(a && b),
                        _ => !(a || b),
                    })
                };
                next.insert(cell, value);
            }
            BlockFunction::Visualiser | BlockFunction::Screen => {
                if let Some(value) = first {
                    state.inputs.insert(cell, value.clone());
                    next.insert(cell, value);
                }
            }
            BlockFunction::Receiver => {
                if let Some(value) = first {
                    let name = if config.text.trim().is_empty() { "value".to_string() } else { config.text.trim().to_string() };
                    state.inputs.insert(cell, value.clone());
                    state.variables.insert(name, value);
                }
            }
            BlockFunction::Laser => {
                if !powered {
                    next.insert(cell, SignalValue::Bool(false));
                    continue;
                }
                let orientation = world.get(cell).map(|voxel| voxel.orientation).unwrap_or(crate::voxel::shapes::Orientation::IDENTITY);
                let direction = orientation.apply_direction(Vec3::NEG_Z).normalize_or_zero();
                let length = if config.number > 0.0 { config.number.min(128.0) } else { 32.0 };
                let origin = cell.as_vec3() + Vec3::splat(0.5) + direction * 0.51;
                let block_hit = crate::voxel::picking::raycast(&world, origin, direction, length);
                let mut distance = block_hit.map(|hit| hit.distance).unwrap_or(length);
                let mut name = block_hit.map(|hit| registry.get(hit.block).label.clone());
                for (mob, transform) in &mobs {
                    let to = transform.translation - origin;
                    let along = to.dot(direction);
                    if along > 0.0 && along < distance && (origin + direction * along).distance(transform.translation) < mob.half_height.max(0.5) {
                        distance = along;
                        name = Some(mob.rig.clone());
                    }
                }
                let hit = name.is_some();
                state.beams.push((origin, origin + direction * distance, hit));
                let value = match config.mode {
                    0 => SignalValue::Bool(hit),
                    1 => SignalValue::Float(if hit { distance } else { 0.0 }),
                    _ => SignalValue::Text(name.unwrap_or_default()),
                };
                next.insert(cell, value);
            }
            BlockFunction::Terminal => {
                if powered {
                    config.timer += delta;
                    if config.timer >= 0.5 {
                        config.timer = 0.0;
                        let lines: Vec<String> = config
                            .text
                            .lines()
                            .map(|line| line.trim().to_string())
                            .filter(|line| !line.is_empty() && !line.starts_with('#'))
                            .collect();
                        if !lines.is_empty() {
                            let line = lines[config.cursor % lines.len()].clone();
                            config.cursor = (config.cursor + 1) % lines.len();
                            queue.push_block(&line, cell);
                        }
                    }
                }
                if let Some(value) = state.outputs.get(&cell).cloned() {
                    next.insert(cell, value);
                }
            }
            BlockFunction::Comandor => {
                let rising = inputs.get(&1).map(SignalValue::as_bool).unwrap_or(false);
                let was = state.inputs.get(&cell).map(SignalValue::as_bool).unwrap_or(false);
                state.inputs.insert(cell, SignalValue::Bool(rising));
                if (config.trigger || (rising && !was)) && powered {
                    config.trigger = false;
                    let command = config.text.clone();
                    queue.push_block(&command, cell);
                }
                config.trigger = false;
                if let Some(value) = state.outputs.get(&cell).cloned() {
                    next.insert(cell, value);
                }
            }
            _ => {}
        }
    }
    state.outputs = next;
}

/// One coloured quad on top of every Screen.
fn sync_screens(
    mut commands: Commands,
    pie: Res<PieState>,
    grid: Res<EnergyGrid>,
    state: Res<LogicState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    faces: Query<(Entity, &ScreenFace, &MeshMaterial3d<StandardMaterial>)>,
    mut mesh: Local<Option<Handle<Mesh>>>,
) {
    if !pie.is_playing() {
        return;
    }
    let screens: Vec<(IVec3, bool)> = grid
        .machines
        .iter()
        .filter(|(_, machine)| machine.function == BlockFunction::Screen)
        .map(|(cell, machine)| (*cell, machine.powered))
        .collect();
    for (entity, face, material) in &faces {
        match screens.iter().find(|(cell, _)| *cell == face.cell) {
            None => commands.entity(entity).despawn(),
            Some((cell, powered)) => {
                let color = if *powered { state.inputs.get(cell).map(SignalValue::color).unwrap_or(Color::srgb(0.08, 0.08, 0.1)) } else { Color::srgb(0.02, 0.02, 0.03) };
                if let Some(mut material) = materials.get_mut(&material.0) {
                    material.base_color = color;
                    material.emissive = color.to_linear() * 2.0;
                }
            }
        }
    }
    let quad = mesh.get_or_insert_with(|| meshes.add(Cuboid::new(0.86, 0.02, 0.86))).clone();
    for (cell, _) in screens {
        if faces.iter().any(|(_, face, _)| face.cell == cell) {
            continue;
        }
        let material = materials.add(StandardMaterial { base_color: Color::BLACK, unlit: true, ..default() });
        commands.spawn((
            Mesh3d(quad.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(cell.as_vec3() + Vec3::new(0.5, 1.01, 0.5)),
            ScreenFace { cell },
            PieOnly,
        ));
    }
}

fn draw_beams(mut gizmos: Gizmos, pie: Res<PieState>, state: Res<LogicState>) {
    if !pie.is_playing() {
        return;
    }
    for (from, to, hit) in &state.beams {
        let color = if *hit { Color::srgb(1.0, 0.2, 0.2) } else { Color::srgb(1.0, 0.5, 0.5) };
        gizmos.line(*from, *to, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_convert_between_kinds() {
        assert!(SignalValue::parse("true").as_bool());
        assert_eq!(SignalValue::parse("2.5").as_f32(), 2.5);
        assert_eq!(SignalValue::parse("hello").as_text(), "hello");
        assert!(!SignalValue::Text("false".into()).as_bool());
        assert!(named_color("#ff0000").is_some());
        assert!(named_color("green").is_some());
    }
}
