//! Energy network: cables and wireless spheres.
//!
//! **Cables** (cable tool, see `data_cables`): a generator's Energy output
//! connected to an Energy input powers that machine.
//!
//! **Voltic** is the energy resource: Solaronels (daylight, 1 every 9 s) and
//! Kineticsters (pressure, 1 every 12 s) produce it into their storage, and
//! Resource cables carry it to the machines.
//!
//! **Fields**: Combustors, Omnireactors and charged **Energisers** project a
//! sphere of `power_radius` voxels (0 turns it off); every machine inside an
//! active sphere is powered. An Energiser turns the Voltic it receives (or a
//! charging Energy cable) into charge and spends it while its field feeds
//! machines (1 Voltic = 20 charge, 1 Voltiom = 200). A Combustor burns 10
//! Floral or 10 Faunal for one Voltic of power (30 s) in a small radius. An
//! Omnireactor is active while the Omnicore network holds Omnicore.
//!
//! The grid rescans the voxel world when its revision changes, tracks the
//! powered state of every machine and fires the Blockly `On Energy` handler
//! (and a power-on / power-off sound) on every transition. Lamps get a real
//! point light while powered.

use crate::core::game_rules::game_rules;
use std::collections::{BTreeMap, HashMap};

use bevy::prelude::*;

use crate::audio::dsp_engine::SoundKind;
use crate::audio::sound_events::{SoundQueue, SoundRequest};
use crate::core::pie_manager::{PieOnly, PieSet, PieState};
use crate::gameplay::world_rules::WorldRules;
use crate::scripting::context::{ScriptEvent, ScriptOwner};
use crate::scripting::vm::ScriptRuntime;
use crate::voxel::blocks::{BlockFunction, BlockRegistry};
use crate::voxel::world::{VoxelWorld, CHUNK_SIZE};


#[derive(Clone, Debug)]
pub struct Generator {
    pub cell: IVec3,
    pub function: BlockFunction,
    pub radius: f32,
    pub active: bool,
}

/// Runtime state of one machine block.
#[derive(Clone, Debug, Default)]
pub struct MachineState {
    pub function: BlockFunction,
    pub powered: bool,
    pub timer: f32,
    /// One timer per component of the block.
    pub timers: Vec<f32>,
    /// The block runs without power.
    pub self_powered: bool,
    /// Internal buffer (harvester output, synthesizer input...).
    pub storage: BTreeMap<String, u32>,
    /// Remaining fuel seconds (combustors).
    pub fuel: f32,
    /// Stored energy (energizers).
    pub charge: f32,
    /// Order of the storage slots shown in the panel (one key per slot).
    pub layout: Vec<Option<String>>,
}

impl MachineState {
    pub fn add(&mut self, resource: &str, amount: u32) {
        if amount == 0 {
            return;
        }
        *self.storage.entry(resource.to_string()).or_insert(0) += amount;
        if !self.layout.iter().any(|slot| slot.as_deref() == Some(resource)) {
            match self.layout.iter().position(Option::is_none) {
                Some(free) => self.layout[free] = Some(resource.to_string()),
                None => self.layout.push(Some(resource.to_string())),
            }
        }
    }

    /// The storage in slot order: one entry per slot (`None` = empty slot).
    pub fn slots(&self) -> Vec<Option<(String, u32)>> {
        let mut slots: Vec<Option<(String, u32)>> = self
            .layout
            .iter()
            .map(|slot| slot.as_ref().and_then(|key| self.storage.get(key).map(|count| (key.clone(), *count))))
            .collect();
        // Keys stored without a slot yet (older states) come last.
        for (key, count) in &self.storage {
            if !self.layout.iter().any(|slot| slot.as_deref() == Some(key.as_str())) {
                slots.push(Some((key.clone(), *count)));
            }
        }
        slots
    }

    /// Put the stack of `resource` in slot `index` (swapping with what is there).
    pub fn move_to(&mut self, resource: &str, index: usize) {
        if !self.storage.contains_key(resource) {
            return;
        }
        if index >= self.layout.len() {
            self.layout.resize(index + 1, None);
        }
        let from = self.layout.iter().position(|slot| slot.as_deref() == Some(resource));
        match from {
            Some(from) => self.layout.swap(from, index),
            None => {
                let previous = self.layout[index].replace(resource.to_string());
                if let Some(previous) = previous {
                    if let Some(free) = self.layout.iter().position(Option::is_none) {
                        self.layout[free] = Some(previous);
                    } else {
                        self.layout.push(Some(previous));
                    }
                }
            }
        }
    }

    pub fn take(&mut self, resource: &str, amount: u32) -> bool {
        match self.storage.get_mut(resource) {
            Some(count) if *count >= amount => {
                *count -= amount;
                if *count == 0 {
                    self.storage.remove(resource);
                    for slot in self.layout.iter_mut() {
                        if slot.as_deref() == Some(resource) {
                            *slot = None;
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Remove and return one arbitrary unit.
    pub fn pop_any(&mut self) -> Option<String> {
        let key = self.storage.keys().next()?.clone();
        self.take(&key, 1);
        Some(key)
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;

    #[test]
    fn storage_slots_can_be_reordered() {
        let mut machine = MachineState::default();
        machine.add("floral", 3);
        machine.add("mineral", 2);
        assert_eq!(machine.slots()[0].as_ref().map(|s| s.0.as_str()), Some("floral"));
        machine.move_to("mineral", 0);
        assert_eq!(machine.slots()[0].as_ref().map(|s| s.0.as_str()), Some("mineral"));
        machine.move_to("floral", 5);
        assert_eq!(machine.slots()[5].as_ref().map(|s| s.0.as_str()), Some("floral"));
        assert!(machine.take("floral", 3));
        assert!(machine.slots().iter().all(|slot| slot.as_ref().is_none_or(|s| s.0 != "floral")));
    }
}

#[derive(Resource, Debug, Default)]
pub struct EnergyGrid {
    pub generators: Vec<Generator>,
    pub machines: HashMap<IVec3, MachineState>,
    scanned_revision: Option<u64>,
}

impl EnergyGrid {
    /// Is a world-space point inside an active generator sphere?
    pub fn is_powered_at(&self, point: Vec3) -> bool {
        self.generators.iter().any(|generator| {
            generator.active
                && (generator.cell.as_vec3() + Vec3::splat(0.5)).distance(point) <= generator.radius
        })
    }

    /// Same, ignoring the field of the source in `except` (an energizer does
    /// not charge from its own field).
    pub fn is_powered_at_except(&self, point: Vec3, except: IVec3) -> bool {
        self.generators.iter().any(|generator| {
            generator.cell != except
                && generator.active
                && (generator.cell.as_vec3() + Vec3::splat(0.5)).distance(point) <= generator.radius
        })
    }

    pub fn machine(&self, cell: IVec3) -> Option<&MachineState> {
        self.machines.get(&cell)
    }

    pub fn machine_mut(&mut self, cell: IVec3) -> Option<&mut MachineState> {
        self.machines.get_mut(&cell)
    }

    pub fn powered_count(&self) -> usize {
        self.machines.values().filter(|machine| machine.powered).count()
    }

    /// Rescan generators and machines, keeping the state of cells that still
    /// hold the same function.
    fn rescan(&mut self, world: &VoxelWorld, registry: &BlockRegistry) {
        let mut previous = std::mem::take(&mut self.machines);
        self.generators.clear();

        for chunk in world.chunks() {
            let origin = chunk.origin();
            for index in 0..CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE {
                let local = crate::voxel::world::VoxelChunk::local_from_index(index);
                let Some(voxel) = chunk.get(local) else {
                    continue;
                };
                let def = registry.get(voxel.block);
                if !def.function.is_machine() {
                    continue;
                }
                let cell = origin + local.as_ivec3();
                let mut state = previous.remove(&cell).filter(|state| state.function == def.function)
                    .unwrap_or_else(|| MachineState {
                        function: def.function,
                        fuel: if def.function == BlockFunction::Combustor { game_rules().energy.starting_fuel } else { 0.0 },
                        ..Default::default()
                    });
                state.function = def.function;
                state.self_powered = def.self_powered;
                if def.function.is_generator() || def.function == BlockFunction::Energizer {
                    self.generators.push(Generator {
                        cell,
                        function: def.function,
                        radius: def.power_radius.max(0.0),
                        active: false,
                    });
                }
                self.machines.insert(cell, state);
            }
        }
    }
}

/// Point light attached to a powered lamp.
#[derive(Component)]
pub struct LampLight {
    pub cell: IVec3,
}

/// The energy grid update (machines read the powered states after it).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridSet;

pub struct EnergyGridPlugin;

impl Plugin for EnergyGridPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EnergyGrid>()
            .add_systems(Update, (update_grid, update_lamps, draw_fields).chain().in_set(GridSet).after(PieSet::Transition));
    }
}

#[allow(clippy::too_many_arguments)]
fn update_grid(
    time: Res<Time>,
    pie: Res<PieState>,
    world: Res<VoxelWorld>,
    registry: Res<BlockRegistry>,
    rules: Res<WorldRules>,
    network: Res<crate::gameplay::crafting::OmnicoreNetwork>,
    cables: Res<crate::gameplay::data_cables::CableNetwork>,
    mut grid: ResMut<EnergyGrid>,
    mut runtime: ResMut<ScriptRuntime>,
    mut sounds: ResMut<SoundQueue>,
) {
    if pie.started_this_frame || pie.stopped_this_frame {
        *grid = EnergyGrid::default();
    }
    if !pie.is_playing() {
        return;
    }
    if grid.scanned_revision != Some(world.revision) {
        grid.rescan(&world, &registry);
        grid.scanned_revision = Some(world.revision);
    }

    let delta = time.delta_secs();
    let _daylight = rules.daylight();

    // Fuel: Combustors burn 10 Floral / Faunal per Voltic of power, Energisers
    // turn their Voltic (or Voltiom) into charge.
    for machine in grid.machines.values_mut() {
        match machine.function {
            BlockFunction::Combustor if machine.fuel <= 0.0 => {
                let batch = game_rules().energy.fuel_batch;
                let fuel: Option<String> = machine
                    .storage
                    .iter()
                    .find(|(key, count)| **count >= batch && (crate::gameplay::crafting::accepts("@floral", key) || key.as_str() == "faunal"))
                    .map(|(key, _)| key.clone());
                if let Some(key) = fuel {
                    machine.take(&key, batch);
                    machine.fuel += game_rules().energy.fuel_per_unit;
                }
            }
            BlockFunction::Energizer if machine.charge <= game_rules().energy.energizer_capacity - game_rules().energy.voltic_charge => {
                if machine.take("voltic", 1) {
                    machine.charge += game_rules().energy.voltic_charge;
                } else if machine.charge <= game_rules().energy.energizer_capacity - game_rules().energy.voltic_charge * 10.0 && machine.take("voltiom", 1) {
                    machine.charge += game_rules().energy.voltic_charge * 10.0;
                }
            }
            _ => {}
        }
    }

    // Generators: burn fuel / read the sun.
    let mut activity = Vec::with_capacity(grid.generators.len());
    for generator in &grid.generators {
        let active = match generator.function {
            BlockFunction::Combustor => grid
                .machines
                .get(&generator.cell)
                .map(|state| state.fuel > 0.0)
                .unwrap_or(false),
            BlockFunction::Omnireactor => network.stored > 0,
            BlockFunction::Energizer => grid.machines.get(&generator.cell).is_some_and(|state| state.charge > 0.0),
            _ => false,
        };
        activity.push(active);
    }
    for (generator, active) in grid.generators.iter_mut().zip(activity) {
        generator.active = active;
    }
    for machine in grid.machines.values_mut() {
        if machine.function == BlockFunction::Combustor && machine.fuel > 0.0 {
            machine.fuel = (machine.fuel - delta).max(0.0);
        }
    }

    // Energy cables: what each output supplies, then who is fed.
    let energy_cables: Vec<(IVec3, IVec3)> = cables
        .cables
        .iter()
        .filter(|cable| cable.kind == crate::voxel::blocks::PortKind::Energy)
        .map(|cable| (IVec3::from_array(cable.from), IVec3::from_array(cable.to)))
        .collect();
    let supplies = |grid: &EnergyGrid, cell: IVec3| -> bool {
        match grid.machines.get(&cell) {
            Some(state) if state.function.is_generator() => grid.generators.iter().any(|g| g.cell == cell && g.active),
            _ => false,
        }
    };
    let mut fed: std::collections::HashSet<IVec3> = std::collections::HashSet::new();
    for (from, to) in &energy_cables {
        if supplies(&grid, *from) {
            fed.insert(*to);
        }
    }
    // Energizer fields: each one spends energy for the machines inside it.
    let fields: Vec<(IVec3, f32)> = grid
        .generators
        .iter()
        .filter(|generator| generator.function == BlockFunction::Energizer && generator.active)
        .map(|generator| (generator.cell, generator.radius))
        .collect();
    for (cell, radius) in fields {
        let center = cell.as_vec3() + Vec3::splat(0.5);
        let users = grid
            .machines
            .iter()
            .filter(|(other, state)| {
                **other != cell
                    && !state.function.is_generator()
                    && state.function != BlockFunction::Energizer
                    && (other.as_vec3() + Vec3::splat(0.5)).distance(center) <= radius
            })
            .count();
        if let Some(state) = grid.machines.get_mut(&cell) {
            state.charge = (state.charge - delta * (0.25 + 0.5 * users as f32)).max(0.0);
        }
    }

    // Machines: powered state transitions.
    let cells: Vec<IVec3> = grid.machines.keys().copied().collect();
    for cell in cells {
        let center = cell.as_vec3() + Vec3::splat(0.5);
        let powered = {
            let state = &grid.machines[&cell];
            if state.function.is_generator() {
                grid.generators.iter().any(|g| g.cell == cell && g.active)
            } else if state.function == BlockFunction::Energizer {
                // Charging: a cable, or the field of another source.
                fed.contains(&cell) || grid.is_powered_at_except(center, cell)
            } else if state.function.needs_no_power() || state.self_powered {
                true
            } else {
                // Cables, or the field of a generator / energizer.
                fed.contains(&cell) || grid.is_powered_at(center)
            }
        };
        if let Some(state) = grid.machines.get_mut(&cell) {
            if state.function == BlockFunction::Energizer && powered {
                state.charge = (state.charge + game_rules().energy.energizer_charge_rate * delta).min(game_rules().energy.energizer_capacity);
            }
        }
        let Some(state) = grid.machines.get_mut(&cell) else {
            continue;
        };
        if state.powered != powered {
            state.powered = powered;
            runtime.fire_event(ScriptOwner::Block(cell.to_array()), &ScriptEvent::Energy { powered });
            sounds.play(
                SoundRequest::sfx(if powered { SoundKind::PowerOn } else { SoundKind::PowerOff })
                    .with_volume(0.35),
            );
        }
    }
}

fn update_lamps(
    mut commands: Commands,
    pie: Res<PieState>,
    grid: Res<EnergyGrid>,
    lights: Query<(Entity, &LampLight)>,
) {
    if !pie.is_playing() {
        return;
    }
    let wanted: Vec<IVec3> = grid
        .machines
        .iter()
        .filter(|(_, state)| state.function == BlockFunction::Lamp && state.powered)
        .map(|(cell, _)| *cell)
        .collect();

    for (entity, light) in &lights {
        if !wanted.contains(&light.cell) {
            commands.entity(entity).despawn();
        }
    }
    for cell in wanted {
        if lights.iter().any(|(_, light)| light.cell == cell) {
            continue;
        }
        commands.spawn((
            PointLight {
                color: Color::srgb(1.0, 0.86, 0.62),
                intensity: 60_000.0,
                range: 14.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(cell.as_vec3() + Vec3::new(0.5, 1.2, 0.5)),
            LampLight { cell },
            PieOnly,
        ));
    }
}

/// Energizer force fields: three slowly turning rings around each active one.
fn draw_fields(mut gizmos: Gizmos, time: Res<Time>, pie: Res<PieState>, grid: Res<EnergyGrid>) {
    if !pie.is_playing() {
        return;
    }
    let t = time.elapsed_secs();
    for generator in grid.generators.iter().filter(|g| g.active && g.function == BlockFunction::Energizer && g.radius > 0.0) {
        let center = generator.cell.as_vec3() + Vec3::splat(0.5);
        let charge = grid.machine(generator.cell).map(|m| m.charge / game_rules().energy.energizer_capacity).unwrap_or(0.0).clamp(0.0, 1.0);
        let color = Color::srgba(0.45, 0.7, 1.0, 0.25 + 0.5 * charge);
        for (index, tilt) in [0.0_f32, 1.05, 2.1].into_iter().enumerate() {
            let rotation = Quat::from_rotation_y(t * 0.35 + tilt) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 + index as f32 * 0.5);
            gizmos.circle(Isometry3d::new(center, rotation), generator.radius, color);
        }
        gizmos.circle(Isometry3d::new(center, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), generator.radius, color);
    }
}
