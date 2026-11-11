//! Cables between machine ports.
//!
//! Every block declares typed **ports** (`BlockDef::ports`, by default the
//! ports of its function): Energy, Resource or Signal, each an input or an
//! output. Holding an item of kind `Cable` (the Cable tool) shows the ports
//! of the nearby blocks; `Use` (RMB) on a port then on another port connects
//! an output to an input of the same kind (either order), `Attack` (LMB) on a
//! port removes its cables. In the studio the World panel's cable tool does
//! the same on whole blocks (kind chosen in the panel).
//!
//! - **Energy**: a generator or a charged Energizer powers the machine at the
//!   other end (see `energy_grid`).
//! - **Resource**: one item per second moves from the storage of the source
//!   machine (Vacuum, Harvester, Container...) to the target's storage.
//! - **Signal**: a script `Set signal` on a block fires `On Signal` on every
//!   block its signal outputs are connected to (0 tick).
//!
//! Cables of the studio world are saved to `project/worlds/cables.ron`; the
//! cables of a world played from the main menu are saved in its `.meta`.

use std::collections::HashSet;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::app_state::{EditorMode, EditorState, EditorUiState};
use crate::core::pie_manager::{PieSet, PieState};
use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::core::settings::{GameAction, GameInput};
use crate::gameplay::energy_grid::EnergyGrid;
use crate::scripting::context::{ScriptEvent, ScriptOwner};
use crate::scripting::vm::ScriptRuntime;
use crate::voxel::blocks::{BlockRegistry, PortDef, PortDirection, PortKind};
use crate::voxel::picking::VoxelCursor;
use crate::voxel::world::VoxelWorld;

/// Radius of a port marker (picking and drawing).
pub const PORT_RADIUS: f32 = 0.16;
/// Ports are shown on the blocks within this distance of the player.
const PORT_VIEW_RADIUS: i32 = 9;

/// One cable: from an output port to an input port of the same kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cable {
    pub from: [i32; 3],
    #[serde(default)]
    pub from_port: u8,
    pub to: [i32; 3],
    #[serde(default)]
    pub to_port: u8,
    pub kind: PortKind,
}

impl Cable {
    pub fn touches(&self, cell: IVec3) -> bool {
        IVec3::from_array(self.from) == cell || IVec3::from_array(self.to) == cell
    }

    pub fn touches_port(&self, cell: IVec3, port: u8) -> bool {
        (IVec3::from_array(self.from) == cell && self.from_port == port) || (IVec3::from_array(self.to) == cell && self.to_port == port)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CableDocument {
    #[serde(default)]
    pub cables: Vec<Cable>,
}

/// A port of a block in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortRef {
    pub cell: IVec3,
    pub index: u8,
    pub kind: PortKind,
    pub direction: PortDirection,
}

/// Cables in play plus the authoring state.
#[derive(Resource, Debug, Default)]
pub struct CableNetwork {
    /// Cables of the world being shown or played.
    pub cables: Vec<Cable>,
    /// Cables of the studio world (restored when play stops).
    pub studio: Vec<Cable>,
    /// First port picked, waiting for the second one.
    pub pending: Option<PortRef>,
    /// Kind used by the studio cable tool.
    pub kind: PortKind,
    /// Last value seen on each block (inspector / HUD bindings).
    pub values: std::collections::HashMap<IVec3, f32>,
    pub stream_timer: f32,
    pub loaded: bool,
    pub transmitted: u64,
    /// Changed during play (the played world's meta saves them).
    pub dirty: bool,
}

impl CableNetwork {
    pub fn path(project: &ProjectLayout) -> std::path::PathBuf {
        project.worlds_path().join("cables.ron")
    }

    /// Save the studio cables.
    pub fn save(&self, project: &ProjectLayout) -> Result<(), String> {
        save_ron(&Self::path(project), &CableDocument { cables: self.studio.clone() })
    }

    /// Connect two ports (an output and an input of the same kind, in any
    /// order).
    pub fn connect(&mut self, a: PortRef, b: PortRef) -> Result<PortKind, String> {
        if a.cell == b.cell {
            return Err("Connect two different blocks.".to_string());
        }
        if a.kind != b.kind {
            return Err(format!("A {} port only connects to a {} port.", a.kind.label(), a.kind.label()));
        }
        let (from, to) = match (a.direction, b.direction) {
            (PortDirection::Output, PortDirection::Input) => (a, b),
            (PortDirection::Input, PortDirection::Output) => (b, a),
            _ => return Err("Connect an output to an input.".to_string()),
        };
        let cable = Cable { from: from.cell.to_array(), from_port: from.index, to: to.cell.to_array(), to_port: to.index, kind: a.kind };
        if self.cables.contains(&cable) {
            return Err("These ports are already connected.".to_string());
        }
        self.cables.push(cable);
        Ok(a.kind)
    }

    /// Remove the cables of one port; returns how many.
    pub fn remove_port(&mut self, cell: IVec3, port: u8) -> usize {
        let before = self.cables.len();
        self.cables.retain(|cable| !cable.touches_port(cell, port));
        before - self.cables.len()
    }

    /// Remove every cable of a block; returns how many.
    pub fn remove_at(&mut self, cell: IVec3) -> usize {
        let before = self.cables.len();
        self.cables.retain(|cable| !cable.touches(cell));
        before - self.cables.len()
    }
}

/// How far the port pegs (and the cable ends) stand out of the face.
const PORT_OUTSET: f32 = 0.04;

/// Side face of port `index` and its rank on that face: inputs take the -X,
/// -Z, +Z, +X faces in that order, outputs +X, +Z, -Z, -X; a face shared by
/// several ports stacks them vertically.
fn port_face(ports: &[PortDef], index: usize) -> (Vec3, usize, usize) {
    const INPUT_FACES: [Vec3; 4] = [Vec3::NEG_X, Vec3::NEG_Z, Vec3::Z, Vec3::X];
    const OUTPUT_FACES: [Vec3; 4] = [Vec3::X, Vec3::Z, Vec3::NEG_Z, Vec3::NEG_X];
    let mut faces: Vec<Vec3> = Vec::with_capacity(ports.len());
    let mut used: Vec<Vec3> = Vec::new();
    // Inputs choose first, then outputs.
    let order: Vec<usize> = (0..ports.len())
        .filter(|i| ports[*i].direction == PortDirection::Input)
        .chain((0..ports.len()).filter(|i| ports[*i].direction != PortDirection::Input))
        .collect();
    faces.resize(ports.len(), Vec3::X);
    for i in order {
        let prefs = if ports[i].direction == PortDirection::Input { INPUT_FACES } else { OUTPUT_FACES };
        let face = prefs.iter().copied().find(|face| !used.contains(face)).unwrap_or(prefs[0]);
        used.push(face);
        faces[i] = face;
    }
    let face = faces.get(index).copied().unwrap_or(Vec3::X);
    let sharing: Vec<usize> = (0..faces.len()).filter(|i| faces[*i] == face).collect();
    let rank = sharing.iter().position(|i| *i == index).unwrap_or(0);
    (face, rank, sharing.len().max(1))
}

/// Outward normal of the side face carrying port `index`.
pub fn port_normal(ports: &[PortDef], index: usize) -> Vec3 {
    if index >= ports.len() {
        return Vec3::Y;
    }
    port_face(ports, index).0
}

/// World position of port `index` of a block: the centre of a side face
/// (see [`port_face`]), just outside the block.
pub fn port_position(cell: IVec3, ports: &[PortDef], index: usize) -> Vec3 {
    let center = cell.as_vec3() + Vec3::splat(0.5);
    if index >= ports.len() {
        return center + Vec3::Y * (0.5 + PORT_OUTSET);
    }
    let (face, rank, count) = port_face(ports, index);
    // Several ports on one face: spread from top to bottom around the centre.
    let lift = if count > 1 { 0.25 - 0.5 * rank as f32 / (count - 1) as f32 } else { 0.0 };
    center + face * (0.5 + PORT_OUTSET) + Vec3::Y * lift
}

/// Ports of the block in `cell` (empty for air).
pub fn ports_at(world: &VoxelWorld, registry: &BlockRegistry, cell: IVec3) -> Vec<PortDef> {
    world.get(cell).map(|voxel| registry.get(voxel.block).ports()).unwrap_or_default()
}

/// Signals raised by scripts this frame: (emitting block, port, value).
#[derive(Resource, Debug, Default)]
pub struct SignalRequests {
    pub pending: Vec<(IVec3, String, f32)>,
}

pub struct DataCablePlugin;

impl Plugin for DataCablePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CableNetwork>()
            .init_resource::<SignalRequests>()
            .init_resource::<AimedPort>()
            .add_systems(Startup, load_cables.after(crate::core::project_io::initialize_project_system))
            .add_systems(
                Update,
                (sync_with_play, prune_cables, studio_cable_tool, play_cable_tool, propagate_signals, stream_resources, draw_cables)
                    .chain()
                    .after(PieSet::Transition)
                    .after(crate::gameplay::mining::MiningSet),
            );
    }
}

fn load_cables(project: Res<ProjectLayout>, mut network: ResMut<CableNetwork>, mut ui: ResMut<EditorUiState>) {
    let path = CableNetwork::path(&project);
    if path.is_file() {
        match load_ron::<CableDocument>(&path) {
            Ok(document) => {
                network.studio = document.cables;
                network.cables = network.studio.clone();
                ui.info(format!("Loaded {} cable(s).", network.cables.len()));
            }
            // Cables of the old node system cannot be converted.
            Err(_) => ui.warn("cables.ron uses the old cable-node format: it is ignored (connect machine ports with the Cable tool)."),
        }
    }
    network.loaded = true;
}

/// Studio play uses the studio cables; stopping play restores them.
fn sync_with_play(pie: Res<PieState>, mut network: ResMut<CableNetwork>) {
    if pie.started_this_frame && pie.persist_world.is_none() {
        network.cables = network.studio.clone();
        network.pending = None;
    }
    if pie.stopped_this_frame {
        network.cables = network.studio.clone();
        network.pending = None;
        network.dirty = false;
    }
}

/// Studio: click two blocks to connect their ports of the chosen kind, `X`
/// on a block removes its cables.
#[allow(clippy::too_many_arguments)]
fn studio_cable_tool(
    editor: Res<EditorState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    cursor: Res<VoxelCursor>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    project: Res<ProjectLayout>,
    mut network: ResMut<CableNetwork>,
    mut ui: ResMut<EditorUiState>,
) {
    if editor.mode != EditorMode::Editing || !ui.cable_tool || !ui.viewport_hovered {
        if editor.mode == EditorMode::Editing {
            network.pending = None;
        }
        return;
    }
    let Some(hit) = cursor.target else {
        return;
    };
    let cell = hit.voxel;
    if keyboard.just_pressed(KeyCode::KeyX) {
        let removed = network.remove_at(cell);
        if removed > 0 {
            network.studio = network.cables.clone();
            ui.action(format!("Removed {removed} cable(s)."));
            if let Err(error) = network.save(&project) {
                ui.error(error);
            }
        }
        return;
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let kind = network.kind;
    let ports = ports_at(&world, &registry, cell);
    let find = |direction: PortDirection| {
        ports
            .iter()
            .position(|port| port.kind == kind && port.direction == direction)
            .map(|index| PortRef { cell, index: index as u8, kind, direction })
    };
    match network.pending.take() {
        None => match find(PortDirection::Output).or_else(|| find(PortDirection::Input)) {
            Some(port) => {
                network.pending = Some(port);
                ui.info(format!("{} {} picked; click a second block.", kind.label(), port.direction.label().to_lowercase()));
            }
            None => ui.warn(format!("This block has no {} port.", kind.label())),
        },
        Some(first) => {
            let wanted = if first.direction == PortDirection::Output { PortDirection::Input } else { PortDirection::Output };
            let Some(second) = find(wanted) else {
                ui.warn(format!("This block has no {} {}.", kind.label(), wanted.label().to_lowercase()));
                return;
            };
            match network.connect(first, second) {
                Ok(kind) => {
                    network.studio = network.cables.clone();
                    ui.action(format!("{} cable connected.", kind.label()));
                    if let Err(error) = network.save(&project) {
                        ui.error(error);
                    }
                }
                Err(error) => ui.warn(error),
            }
        }
    }
}

/// Port under the aiming ray (nearest), if any.
fn aimed_port(aim: &crate::gameplay::mining::PlayerAim, world: &VoxelWorld, registry: &BlockRegistry, center: IVec3) -> Option<(PortRef, Vec3)> {
    if aim.ray_direction == Vec3::ZERO {
        return None;
    }
    let mut best: Option<(f32, PortRef, Vec3)> = None;
    let r = PORT_VIEW_RADIUS;
    for dy in -r..=r {
        for dz in -r..=r {
            for dx in -r..=r {
                let cell = center + IVec3::new(dx, dy, dz);
                let ports = ports_at(world, registry, cell);
                for (index, port) in ports.iter().enumerate() {
                    let position = port_position(cell, &ports, index);
                    let to = position - aim.origin;
                    let t = to.dot(aim.ray_direction);
                    if t < 0.0 || t > aim.reach + 1.0 {
                        continue;
                    }
                    let miss = (to - aim.ray_direction * t).length();
                    if miss > PORT_RADIUS * 1.4 {
                        continue;
                    }
                    if best.as_ref().is_none_or(|(d, ..)| t < *d) {
                        let port_ref = PortRef { cell, index: index as u8, kind: port.kind, direction: port.direction };
                        best = Some((t, port_ref, position));
                    }
                }
            }
        }
    }
    best.map(|(_, port, position)| (port, position))
}

/// Index of the cable closest to the aiming ray (its sagging curve).
fn aimed_cable(aim: &crate::gameplay::mining::PlayerAim, network: &CableNetwork, world: &VoxelWorld, registry: &BlockRegistry) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for (index, cable) in network.cables.iter().enumerate() {
        let from = IVec3::from_array(cable.from);
        let to = IVec3::from_array(cable.to);
        let a = port_position(from, &ports_at(world, registry, from), cable.from_port as usize);
        let b = port_position(to, &ports_at(world, registry, to), cable.to_port as usize);
        for point in crate::gameplay::cable_visuals::cable_curve(a, b) {
            let delta = point - aim.origin;
            let t = delta.dot(aim.ray_direction);
            if t < 0.0 || t > aim.reach + 2.0 {
                continue;
            }
            let miss = (delta - aim.ray_direction * t).length();
            if miss < 0.22 && best.is_none_or(|(d, _)| t < d) {
                best = Some((t, index));
            }
        }
    }
    best.map(|(_, index)| index)
}

/// Cables whose block (or port) is gone are removed with it.
fn prune_cables(pie: Res<PieState>, mut world: ResMut<VoxelWorld>, registry: Res<BlockRegistry>, mut network: ResMut<CableNetwork>) {
    let changed = world.take_changed();
    if changed.is_empty() || network.cables.is_empty() {
        return;
    }
    let before = network.cables.len();
    let changed: HashSet<IVec3> = changed.into_iter().collect();
    network.cables.retain(|cable| {
        let alive = |cell: [i32; 3], port: u8| {
            let cell = IVec3::from_array(cell);
            !changed.contains(&cell) || ports_at(&world, &registry, cell).len() > port as usize
        };
        alive(cable.from, cable.from_port) && alive(cable.to, cable.to_port)
    });
    let removed = before - network.cables.len();
    if removed > 0 {
        network.dirty = true;
        // In the studio the edited world is the studio world.
        if !pie.is_playing() {
            network.studio = network.cables.clone();
        }
    }
}

/// Port aimed with the Cable tool this frame (drawn highlighted).
#[derive(Resource, Debug, Default)]
pub struct AimedPort(pub Option<(PortRef, Vec3)>);

/// Play: the Cable tool connects ports (`Use`) and removes cables (`Attack`).
#[allow(clippy::too_many_arguments)]
fn play_cable_tool(
    pie: Res<PieState>,
    input: GameInput,
    held: Res<crate::gameplay::items::HeldTool>,
    aim: Res<crate::gameplay::mining::PlayerAim>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    mut network: ResMut<CableNetwork>,
    mut ui: ResMut<EditorUiState>,
    mut sounds: ResMut<crate::audio::sound_events::SoundQueue>,
    mut aimed_state: ResMut<AimedPort>,
) {
    aimed_state.0 = None;
    if !pie.is_playing() || !held.cable {
        if pie.is_playing() {
            network.pending = None;
        }
        return;
    }
    let center = aim.origin.floor().as_ivec3();
    let aimed = aimed_port(&aim, &world, &registry, center);
    aimed_state.0 = aimed;
    if !pie.cursor_captured {
        return;
    }
    if input.just_pressed(GameAction::Attack) {
        // A port: all its cables. Else the cable under the crosshair. Else
        // the aimed block: all of its cables.
        let removed = if let Some((port, _)) = aimed {
            network.remove_port(port.cell, port.index)
        } else if let Some(index) = aimed_cable(&aim, &network, &world, &registry) {
            network.cables.remove(index);
            1
        } else if let Some(hit) = aim.hit {
            network.remove_at(hit.voxel)
        } else {
            0
        };
        network.pending = None;
        if removed > 0 {
            network.dirty = true;
            ui.action(format!("Removed {removed} cable(s)."));
            sounds.play_kind(crate::audio::dsp_engine::SoundKind::UiClick);
        }
        return;
    }
    if !input.just_pressed(GameAction::Use) {
        return;
    }
    let Some((port, _)) = aimed else {
        if network.pending.take().is_some() {
            ui.info("Cable cancelled.");
        }
        return;
    };
    match network.pending.take() {
        None => {
            network.pending = Some(port);
            ui.info(format!("{} {} picked: Use on a {} {}.", port.kind.label(), port.direction.label().to_lowercase(), port.kind.label(), if port.direction == PortDirection::Output { "input" } else { "output" }));
            sounds.play_kind(crate::audio::dsp_engine::SoundKind::UiClick);
        }
        Some(first) if first == port => {
            ui.info("Cable cancelled.");
        }
        Some(first) => match network.connect(first, port) {
            Ok(kind) => {
                network.dirty = true;
                ui.action(format!("{} cable connected.", kind.label()));
                sounds.play_kind(crate::audio::dsp_engine::SoundKind::BlockPlace);
            }
            Err(error) => {
                ui.warn(error);
                network.pending = Some(port);
            }
        },
    }
}

/// 0-tick propagation of logic signals along Signal cables.
fn propagate_signals(
    pie: Res<PieState>,
    mut requests: ResMut<SignalRequests>,
    mut network: ResMut<CableNetwork>,
    mut runtime: ResMut<ScriptRuntime>,
) {
    let pending: Vec<(IVec3, String, f32)> = requests.pending.drain(..).collect();
    if !pie.is_playing() {
        return;
    }
    for (source, port, value) in pending {
        let mut delivered: HashSet<IVec3> = HashSet::new();
        let targets: Vec<IVec3> = network
            .cables
            .iter()
            .filter(|cable| cable.kind == PortKind::Signal && IVec3::from_array(cable.from) == source)
            .map(|cable| IVec3::from_array(cable.to))
            .collect();
        for receiver in targets {
            if !delivered.insert(receiver) {
                continue;
            }
            network.values.insert(receiver, value);
            runtime.fire_event(ScriptOwner::Block(receiver.to_array()), &ScriptEvent::Signal { port: port.clone(), value });
        }
        network.transmitted += 1;
        // World-rule programs see every signal as well.
        runtime.fire_event(ScriptOwner::Global, &ScriptEvent::Signal { port, value });
    }
}

/// Resource cables: one item per second from the source machine's storage
/// to the target machine's storage.
fn stream_resources(time: Res<Time>, pie: Res<PieState>, mut network: ResMut<CableNetwork>, mut grid: ResMut<EnergyGrid>) {
    if !pie.is_playing() {
        return;
    }
    network.stream_timer += time.delta_secs();
    if network.stream_timer < 1.0 {
        return;
    }
    network.stream_timer = 0.0;
    let streams: Vec<(IVec3, IVec3)> = network
        .cables
        .iter()
        .filter(|cable| cable.kind == PortKind::Resource)
        .map(|cable| (IVec3::from_array(cable.from), IVec3::from_array(cable.to)))
        .collect();
    for (from, to) in streams {
        if grid.machine(to).is_none() {
            continue;
        }
        let item = grid.machine_mut(from).and_then(|machine| machine.pop_any());
        if let Some(item) = item {
            if let Some(target) = grid.machine_mut(to) {
                target.add(&item, 1);
                network.transmitted += 1;
            }
        }
    }
}

fn kind_color(kind: PortKind) -> Color {
    let [r, g, b] = kind.rgb();
    Color::srgb_u8(r, g, b)
}

/// Cables and ports are meshes (`cable_visuals`); only the authoring hints
/// are drawn here: the aimed port and the cable being plugged.
fn draw_cables(
    mut gizmos: Gizmos,
    network: Res<CableNetwork>,
    pie: Res<PieState>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    held: Res<crate::gameplay::items::HeldTool>,
    aim: Res<crate::gameplay::mining::PlayerAim>,
    aimed: Res<AimedPort>,
) {
    let show_ports = pie.is_playing() && held.cable;
    if show_ports {
        if let Some((port, position)) = aimed.0 {
            let color = kind_color(port.kind);
            let ring = Isometry3d::new(position + Vec3::Y * 0.09, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
            gizmos.circle(ring, PORT_RADIUS * 1.3, color);
            gizmos.circle(ring, PORT_RADIUS * 1.5, Color::WHITE);
        }
    }
    if let Some(pending) = network.pending {
        let ports = ports_at(&world, &registry, pending.cell);
        let position = port_position(pending.cell, &ports, pending.index as usize);
        let color = kind_color(pending.kind);
        gizmos.circle(
            Isometry3d::new(position + Vec3::Y * 0.09, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            PORT_RADIUS * 1.6,
            color,
        );
        if show_ports && aim.ray_direction != Vec3::ZERO {
            // Cable following the aim until it is plugged.
            let end = aim.origin + aim.ray_direction * 2.5;
            let points = crate::gameplay::cable_visuals::cable_curve(position, end);
            gizmos.linestrip(points, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(cell: IVec3, index: u8, kind: PortKind, direction: PortDirection) -> PortRef {
        PortRef { cell, index, kind, direction }
    }

    #[test]
    fn cables_join_an_output_to_an_input_of_the_same_kind() {
        let mut network = CableNetwork::default();
        let solar = port(IVec3::ZERO, 0, PortKind::Energy, PortDirection::Output);
        let energizer_in = port(IVec3::X * 3, 0, PortKind::Energy, PortDirection::Input);
        let container_in = port(IVec3::Z * 3, 0, PortKind::Resource, PortDirection::Input);
        // Either order works, the cable always runs output -> input.
        assert!(network.connect(energizer_in, solar).is_ok());
        assert_eq!(network.cables[0].from, [0, 0, 0]);
        assert!(network.connect(solar, energizer_in).is_err());
        assert!(network.connect(solar, container_in).is_err());
        assert!(network.connect(solar, solar).is_err());
        assert_eq!(network.remove_port(IVec3::ZERO, 0), 1);
        assert!(network.cables.is_empty());
    }

    #[test]
    fn machines_expose_their_default_ports() {
        let registry = BlockRegistry::builtin();
        // The Solaronel makes Voltic (a resource); the Combustor makes power.
        let solar = registry.get(registry.find("solar").expect("solar")).ports();
        assert!(solar.iter().any(|p| p.kind == PortKind::Resource && p.direction == PortDirection::Output));
        let combustor = registry.get(registry.find("combustor").expect("combustor")).ports();
        assert!(combustor.iter().any(|p| p.kind == PortKind::Energy && p.direction == PortDirection::Output));
        let energizer = registry.get(registry.find("vacuum").expect("vacuum")).ports();
        assert_eq!(energizer.len(), 2);
        let container = registry.get(registry.find("container").expect("container")).ports();
        assert!(container.iter().any(|p| p.kind == PortKind::Resource && p.direction == PortDirection::Input));
        let positions: Vec<Vec3> = (0..energizer.len()).map(|i| port_position(IVec3::ZERO, &energizer, i)).collect();
        assert!(positions[0].distance(positions[1]) > 0.3);
    }

    #[test]
    fn ports_sit_on_the_centre_of_the_side_faces() {
        use crate::voxel::blocks::BlockFunction;
        let ports = BlockFunction::Transmitter.default_ports();
        let positions: Vec<Vec3> = (0..ports.len()).map(|i| port_position(IVec3::ZERO, &ports, i)).collect();
        for (index, position) in positions.iter().enumerate() {
            // Half way up a side face, never on the top.
            assert!((position.y - 0.5).abs() < 1e-4, "{index}: {position:?}");
            let normal = port_normal(&ports, index);
            assert_eq!(normal.y, 0.0);
            let on_face = Vec3::splat(0.5) + normal * 0.5;
            assert!(position.distance(on_face) < 0.1, "{index}: {position:?}");
        }
        // Four ports, four different faces; the output faces +X.
        let output = ports.iter().position(|port| port.direction == PortDirection::Output).expect("output");
        assert_eq!(port_normal(&ports, output), Vec3::X);
    }
}
