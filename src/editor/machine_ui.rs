//! Machine panels opened with `E` in game: status (power, fuel, storage),
//! the settings of the configurable machines (Terminal, Comandor, Emitter,
//! Transmitter, Repulsor...), the recipes of the crafting stations
//! (Synthesizer, Assembler, Infuser, Atomiser) and, for motors (on a block
//! or on a launched movable), speed / direction / on-off.

use crate::core::game_rules::game_rules;
use egui::{Color32, CornerRadius, RichText, vec2};

use crate::editor::game_ui::function_help;
use crate::editor::theme;
use crate::gameplay::energy_grid::EnergyGrid;
use crate::gameplay::inventory::Inventory;
use crate::gameplay::machines::motors::Motors;
use crate::gameplay::machines::{MachineCommand, MachinePanel};
use crate::voxel::blocks::{BlockFunction, BlockRegistry};
use crate::voxel::world::VoxelWorld;

/// Everything a machine panel reads or edits.
pub struct MachineUiData<'a> {
    pub panel: &'a mut MachinePanel,
    pub motors: &'a mut Motors,
    pub grid: &'a EnergyGrid,
    pub world: &'a VoxelWorld,
    pub registry: &'a BlockRegistry,
    pub inventory: &'a Inventory,
    pub items: &'a crate::gameplay::items::ItemLibrary,
    /// Omnicore stored in the world network.
    pub network: u32,
    pub configs: &'a mut crate::gameplay::machines::config::MachineConfigs,
    pub logic: &'a crate::gameplay::logic::LogicState,
    /// Biomes the Invocateral can summon from.
    pub summonable: Vec<String>,
    /// Entities killed at least once (Mnemo-Cloner).
    pub killed: Vec<String>,
    /// Every entity name (Ringer target).
    pub mob_names: Vec<String>,
}

fn status_chip(ui: &mut egui::Ui, on: bool, on_text: &str, off_text: &str) {
    let (fill, text) = if on { (theme::ACCENT_DIM, on_text) } else { (Color32::from_rgb(70, 36, 40), off_text) };
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(CornerRadius::same(255))
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(Color32::WHITE));
        });
}

fn row(ui: &mut egui::Ui, label: &str, value: String) {
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new(label).size(12.5).color(theme::TEXT_DIM)));
        ui.label(RichText::new(value).size(13.0).color(theme::TEXT));
    });
}

/// Panel of a motor mounted on a launched movable.
fn draw_body_motor_panel(ctx: &egui::Context, data: &mut MachineUiData) {
    let Some(edit) = data.panel.body_motor.as_mut() else {
        return;
    };
    let screen = ctx.viewport_rect();
    let rect = egui::Rect::from_min_size(egui::pos2(screen.center().x + 60.0, screen.center().y - 160.0), vec2(380.0, 320.0));
    let id = egui::Id::new("body_motor_panel");
    let mut ui = egui::Ui::new(
        ctx.clone(),
        id,
        egui::UiBuilder::new().layer_id(egui::LayerId::new(theme::WINDOW_ORDER, id)).max_rect(rect),
    );
    let mut close = false;
    theme::window_frame().show(&mut ui, |ui| {
        ui.set_width(rect.width() - 36.0);
        ui.horizontal(|ui| {
            theme::window_title(ui, "Motor", "On a movable");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::icon_button(ui, "X", "Close (E / Esc)").clicked() {
                    close = true;
                }
            });
        });
        ui.label(RichText::new("A running motor pushes the movable along its hub axis. Clockwise pushes forward, counter-clockwise backward.").size(12.0).color(theme::TEXT_DIM));
        ui.add_space(8.0);
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                let text = if edit.enabled { "Running" } else { "Stopped" };
                status_chip(ui, edit.enabled, text, text);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if edit.enabled { "Stop" } else { "Start" };
                    if theme::game_button(ui, label, !edit.enabled, 96.0).clicked() {
                        edit.enabled = !edit.enabled;
                        edit.dirty = true;
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Power").size(12.5).color(theme::TEXT_DIM)));
                if ui.add(egui::Slider::new(&mut edit.rpm, 0.0..=game_rules().vehicles.max_rpm).suffix(" rpm")).changed() {
                    edit.dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Direction").size(12.5).color(theme::TEXT_DIM)));
                if theme::game_tab(ui, edit.clockwise, "Forward").clicked() {
                    edit.clockwise = true;
                    edit.dirty = true;
                }
                if theme::game_tab(ui, !edit.clockwise, "Backward").clicked() {
                    edit.clockwise = false;
                    edit.dirty = true;
                }
            });
            if data.inventory.survival {
                theme::hint(ui, "Needs power: a Combustor on board or a generator nearby.");
            }
        });
    });
    let typing = ctx.memory(|m| m.focused().is_some());
    if !data.panel.just_opened && !typing && ctx.input(|i| i.key_pressed(egui::Key::E)) {
        close = true;
    }
    data.panel.just_opened = false;
    if close {
        data.panel.body_motor = None;
    }
}

/// Slot crafting: drag resources from the inventory into the slots, the
/// matching recipe of the station shows its result and crafts on the button.
fn slot_station(ui: &mut egui::Ui, data: &mut MachineUiData, cell: bevy::prelude::IVec3, station: crate::gameplay::crafting::Station, slots: usize, verb: &str, enabled: bool) {
    use crate::editor::game_ui::{empty_tile, resource_tile, Drag};
    if data.panel.slots_cell != Some(cell) || data.panel.slots.len() != slots {
        data.panel.slots_cell = Some(cell);
        data.panel.slots = vec![None; slots];
    }
    let filled: Vec<String> = data.panel.slots.iter().flatten().cloned().collect();
    let mut recipe = crate::gameplay::crafting::match_slots(station, &filled);
    // A failed infusion still runs: it gives Garbage.
    if recipe.is_none() && station == crate::gameplay::crafting::Station::Infuser && filled.len() == slots {
        recipe = crate::gameplay::crafting::failed_infusion(&filled);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 6.0);
        for index in 0..slots {
            let key = data.panel.slots[index].clone();
            let (_, dropped) = ui.dnd_drop_zone::<Drag, ()>(egui::Frame::NONE, |ui| match &key {
                Some(key) => {
                    let owned = if data.inventory.survival { Some(data.inventory.count(key)) } else { None };
                    if resource_tile(ui, data.registry, data.items, key, owned, None).on_hover_text("Click to empty the slot").clicked() {
                        data.panel.slots[index] = None;
                    }
                }
                None => {
                    empty_tile(ui, true);
                }
            });
            if let Some(payload) = dropped {
                match payload.as_ref() {
                    Drag::Entry(item) => data.panel.slots[index] = item.resource_key(data.registry),
                    Drag::Grid(cell) => data.panel.slots[index] = data.inventory.grid.get(*cell).cloned().flatten(),
                    Drag::Slot(slot) => data.panel.slots[index] = data.inventory.hotbar.get(*slot).and_then(|item| item.resource_key(data.registry)),
                    Drag::Stored(_) => {}
                }
            }
            if index + 1 < slots {
                ui.label(RichText::new("+").size(22.0).color(theme::TEXT_DIM));
            }
        }
        ui.label(RichText::new("=").size(22.0).color(theme::TEXT_DIM));
        match &recipe {
            Some(recipe) => {
                resource_tile(ui, data.registry, data.items, &recipe.output, Some(recipe.count), None);
            }
            None => {
                empty_tile(ui, false);
            }
        }
        ui.vertical(|ui| {
            let affordable = recipe.as_ref().is_some_and(|recipe| !data.inventory.survival || crate::gameplay::crafting::can_afford(data.inventory, recipe));
            if theme::game_button(ui, verb, true, 120.0).clicked() && enabled && affordable {
                if let Some(recipe) = recipe.clone() {
                    data.panel.commands.push(MachineCommand::CraftRecipe(cell, recipe, 1));
                }
            }
            if theme::game_small_button(ui, "x5", enabled && affordable).clicked() {
                if let Some(recipe) = recipe.clone() {
                    data.panel.commands.push(MachineCommand::CraftRecipe(cell, recipe, 5));
                }
            }
        });
    });
    match &recipe {
        Some(recipe) => {
            let inputs: Vec<String> = recipe
                .inputs
                .iter()
                .map(|(key, amount)| format!("{amount} {}", crate::gameplay::crafting::display_name(key, data.registry, data.items)))
                .collect();
            theme::hint(ui, &format!("Uses {}.", inputs.join(" + ")));
        }
        None if filled.is_empty() => theme::hint(ui, "Drag resources from your inventory into the slots."),
        None => theme::hint(ui, "No recipe uses these resources here."),
    }
    if !enabled {
        theme::hint(ui, "Not powered.");
    }
}

/// Container: its slots in their own order (drag a stack onto another slot
/// to reorganise, onto the inventory to take it) and a drop zone for the
/// inventory stacks.
fn container_slots(ui: &mut egui::Ui, data: &mut MachineUiData, cell: bevy::prelude::IVec3, machine: Option<&crate::gameplay::energy_grid::MachineState>) {
    use crate::editor::game_ui::{empty_tile, resource_tile, Drag};
    let layout: Vec<Option<(String, u32)>> = machine.map(|m| m.slots()).unwrap_or_default();
    let total: u32 = machine.map(|m| m.storage.values().sum()).unwrap_or(0);
    let capacity = machine.map(|m| m.function.capacity()).unwrap_or(CONTAINER_SLOTS as u32 * game_rules().inventory.stack_size);
    ui.label(RichText::new(format!("STORAGE  -  {total} / {capacity}")).size(11.0).color(theme::TEXT_DIM));
    let shown = (layout.len() + 1).clamp(CONTAINER_SLOTS, 54).div_ceil(9) * 9;
    let mut moves: Vec<MachineCommand> = Vec::new();
    let (_, dropped) = ui.dnd_drop_zone::<Drag, ()>(egui::Frame::NONE, |ui| {
        ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
        for row in (0..shown).collect::<Vec<_>>().chunks(9) {
            ui.horizontal(|ui| {
                for &index in row {
                    let tile = layout.get(index).cloned().flatten();
                    let (_, dropped) = ui.dnd_drop_zone::<Drag, ()>(egui::Frame::NONE, |ui| match &tile {
                        Some((key, count)) => {
                            resource_tile(ui, data.registry, data.items, key, Some(*count), Some(Drag::Stored(key.clone())));
                        }
                        None => {
                            empty_tile(ui, false);
                        }
                    });
                    if let Some(payload) = dropped {
                        match payload.as_ref() {
                            Drag::Stored(key) => moves.push(MachineCommand::MoveStored(cell, key.clone(), index)),
                            Drag::Entry(item) => {
                                if let Some(key) = item.resource_key(data.registry) {
                                    moves.push(MachineCommand::Deposit(cell, key.clone()));
                                    moves.push(MachineCommand::MoveStored(cell, key, index));
                                }
                            }
                            Drag::Grid(slot) => {
                                if let Some(key) = data.inventory.grid.get(*slot).cloned().flatten() {
                                    moves.push(MachineCommand::Deposit(cell, key.clone()));
                                    moves.push(MachineCommand::MoveStored(cell, key, index));
                                }
                            }
                            Drag::Slot(_) => {}
                        }
                    }
                }
            });
        }
    });
    if let Some(payload) = dropped {
        let key = match payload.as_ref() {
            Drag::Entry(item) => item.resource_key(data.registry),
            Drag::Grid(slot) => data.inventory.grid.get(*slot).cloned().flatten(),
            Drag::Slot(slot) => data.inventory.hotbar.get(*slot).and_then(|item| item.resource_key(data.registry)),
            Drag::Stored(_) => None,
        };
        if let Some(key) = key {
            if moves.is_empty() {
                moves.push(MachineCommand::Deposit(cell, key));
            }
        }
    }
    data.panel.commands.extend(moves);
}

/// Slots of a container.
pub const CONTAINER_SLOTS: usize = 27;

/// Recipes of a crafting station inside its panel.
fn station_recipes(ui: &mut egui::Ui, data: &mut MachineUiData, cell: bevy::prelude::IVec3, station: crate::gameplay::crafting::Station, enabled: bool) {
    let clicked = crate::editor::game_ui::recipe_list(ui, station, data.inventory, data.registry, data.items, enabled, 330.0);
    if let Some((recipe, times)) = clicked {
        data.panel.commands.push(MachineCommand::CraftRecipe(cell, recipe, times));
    }
}

/// Draw the open machine panel (if any).
pub fn draw_machine_panel(ctx: &egui::Context, mut data: MachineUiData) {
    if data.panel.body_motor.is_some() {
        draw_body_motor_panel(ctx, &mut data);
        return;
    }
    let Some(cell) = data.panel.open else {
        return;
    };
    let Some(voxel) = data.world.get(cell) else {
        data.panel.open = None;
        return;
    };
    let def = data.registry.get(voxel.block);
    let function = def.function;
    let machine = data.grid.machine(cell);

    let screen = ctx.viewport_rect();
    // Stations and containers show the player inventory under them.
    let with_inventory = matches!(
        function,
        BlockFunction::Synthesizer
            | BlockFunction::Assembler
            | BlockFunction::Infuser
            | BlockFunction::Container
            | BlockFunction::AstralContainer
            | BlockFunction::Custom
            | BlockFunction::Atomiser
            | BlockFunction::Reconstructor
            | BlockFunction::Excavator
            | BlockFunction::Energizer
            | BlockFunction::Combustor
            | BlockFunction::MnemoCloner
            | BlockFunction::Ressourcer
            | BlockFunction::Vacuum
            | BlockFunction::Harvester
            | BlockFunction::Ethereaper
            | BlockFunction::Solar
            | BlockFunction::Kineticster
            | BlockFunction::Omnichorus
    );
    let size = vec2(if with_inventory { 620.0 } else { 380.0 }, 0.0);
    let height = if with_inventory { 760.0_f32.min(screen.height() - 30.0) } else { 420.0 };
    let rect = egui::Rect::from_min_size(
        egui::pos2((screen.center().x - size.x * 0.5).max(10.0), (screen.center().y - height * 0.5).max(10.0)),
        vec2(size.x, height),
    );
    let id = egui::Id::new("machine_panel");
    let mut ui = egui::Ui::new(
        ctx.clone(),
        id,
        egui::UiBuilder::new().layer_id(egui::LayerId::new(theme::WINDOW_ORDER, id)).max_rect(rect),
    );
    let mut close = false;
    theme::window_frame().show(&mut ui, |ui| {
        ui.set_width(size.x - 36.0);
        ui.horizontal(|ui| {
            theme::window_title(ui, &def.label, function.label());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::icon_button(ui, "X", "Close (E / Esc)").clicked() {
                    close = true;
                }
            });
        });
        ui.label(RichText::new(function_help(function)).size(12.0).color(theme::TEXT_DIM));
        ui.add_space(8.0);

        // Power line (motors do not need it, generators produce it).
        if let Some(machine) = machine {
            ui.horizontal(|ui| {
                if function.is_generator() {
                    status_chip(ui, machine.powered, "Producing power", "Idle");
                } else if !function.needs_no_power() {
                    status_chip(ui, machine.powered, "Powered", "No power");
                }
                let mut toggled = false;
                if let Some(config) = data.configs.map.get_mut(&cell) {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if config.enabled { "ON" } else { "OFF" };
                        if theme::game_tab(ui, config.enabled, label).on_hover_text("Switch the machine on / off").clicked() {
                            config.enabled = !config.enabled;
                            toggled = true;
                        }
                    });
                }
                if toggled {
                    data.configs.dirty = true;
                }
            });
            if !machine.powered && !function.is_generator() && !function.needs_no_power() {
                theme::hint(ui, "Needs power: an Energiser fed with Voltic, a Combustor or an Omnireactor nearby (or an Energy cable).");
            }
            ui.add_space(6.0);
        }

        theme::card(ui, |ui| match function {
            BlockFunction::Motor => {
                let config = data.motors.config_mut(cell);
                ui.horizontal(|ui| {
                    let text = if config.enabled { "Running" } else { "Stopped" };
                    status_chip(ui, config.enabled, text, text);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if config.enabled { "Stop" } else { "Start" };
                        if theme::game_button(ui, label, !config.enabled, 96.0).clicked() {
                            config.enabled = !config.enabled;
                        }
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Speed").size(12.5).color(theme::TEXT_DIM)));
                    ui.add(egui::Slider::new(&mut config.rpm, 0.0..=game_rules().vehicles.max_rpm).suffix(" rpm"));
                });
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Direction").size(12.5).color(theme::TEXT_DIM)));
                    if theme::game_tab(ui, config.clockwise, "Clockwise").clicked() {
                        config.clockwise = true;
                    }
                    if theme::game_tab(ui, !config.clockwise, "Counter-clockwise").clicked() {
                        config.clockwise = false;
                    }
                });
                ui.add_space(4.0);
                row(ui, "Status", config.status.clone());
                theme::hint(ui, "Blocks touching the top face (where the hub is) turn with it. Up to 1024 blocks.");
            }
            BlockFunction::Combustor => {
                let fuel = machine.map(|m| m.fuel).unwrap_or(0.0);
                row(ui, "Burning", format!("{fuel:.0} s"));
                let bar = (fuel / 120.0).clamp(0.0, 1.0);
                ui.add(egui::ProgressBar::new(bar).desired_height(8.0).fill(theme::WARN));
                row(ui, "Field", format!("{:.0} blocks", def.power_radius));
                if theme::game_small_button(ui, "Add 10 Floral / Faunal", true).clicked() {
                    data.panel.commands.push(MachineCommand::AddFuel(cell));
                }
                container_slots(ui, &mut data, cell, machine);
                theme::hint(ui, "10 Floral (or 10 Faunal) burn into 1 Voltic of power: 30 s of field.");
            }
            BlockFunction::Solar | BlockFunction::Kineticster => {
                let voltic = machine.and_then(|m| m.storage.get("voltic").copied()).unwrap_or(0);
                row(ui, "Voltic stored", voltic.to_string());
                let period = if function == BlockFunction::Solar { 9.0 } else { 12.0 };
                let progress = machine.map(|m| m.timer / period).unwrap_or(0.0).clamp(0.0, 1.0);
                ui.add(egui::ProgressBar::new(progress).desired_height(8.0).fill(theme::ACCENT));
                theme::hint(
                    ui,
                    if function == BlockFunction::Solar { "Daylight: 1 Voltic every 9 s." } else { "Stand (or let an entity stand) on it: 1 Voltic every 12 s." },
                );
                if theme::game_small_button(ui, "Collect", voltic > 0).clicked() {
                    data.panel.commands.push(MachineCommand::Collect(cell));
                }
            }
            BlockFunction::Synthesizer => {
                theme::hint(ui, "1 Floral + 1 Mineral (or 1 Floral + 1 Crystalline) = 1 Synthetic, the universal material of most recipes.");
                slot_station(ui, &mut data, cell, crate::gameplay::crafting::Station::Synthesizer, 2, "Synthesize", true);
            }
            BlockFunction::Assembler => {
                let powered = machine.is_none_or(|m| m.powered);
                theme::hint(ui, "Synthetic, Technic, Conflux and the end-game Omnicore.");
                station_recipes(ui, &mut data, cell, crate::gameplay::crafting::Station::Assembler, powered);
            }
            BlockFunction::Atomiser => {
                let powered = machine.is_some_and(|m| m.powered);
                theme::hint(ui, "Conflux (or 8 Garbage) + 100 Voltic = 1 Atomic. Fed by cables, it also works on its own storage.");
                station_recipes(ui, &mut data, cell, crate::gameplay::crafting::Station::Atomiser, powered);
                container_slots(ui, &mut data, cell, machine);
            }
            BlockFunction::Omnichorus => {
                row(ui, "Network", format!("{} Omnicore", data.network));
                row(ui, "Inventory", format!("{} Omnicore", data.inventory.count("omnicore")));
                theme::hint(ui, "Sends your Omnicore into the network. In survival every chat command spends 1 Omnicore from it.");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let owned = data.inventory.count("omnicore") > 0;
                    if theme::game_small_button(ui, "Send 1", owned).clicked() {
                        data.panel.commands.push(MachineCommand::TransferOmnicore(cell, false));
                    }
                    if theme::game_small_button(ui, "Send all", owned).clicked() {
                        data.panel.commands.push(MachineCommand::TransferOmnicore(cell, true));
                    }
                });
            }
            BlockFunction::Omnireactor => {
                row(ui, "Network", format!("{} Omnicore", data.network));
                row(ui, "Field", format!("{:.0} blocks", def.power_radius));
                theme::hint(ui, "Powers the machines of its field while the Omnicore network holds Omnicore (1 every 60 s).");
            }
            BlockFunction::Vacuum => {
                let config = data.configs.get_or_default(cell, function);
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Radius").size(12.5).color(theme::TEXT_DIM)));
                    ui.add(egui::Slider::new(&mut config.radius, 1.0..=64.0).suffix(" blocks"));
                });
                ui.horizontal(|ui| {
                    if theme::game_tab(ui, config.mode == 0, "Everything").clicked() {
                        config.mode = 0;
                    }
                    if theme::game_tab(ui, config.mode == 1, "Only").clicked() {
                        config.mode = 1;
                    }
                    if config.mode == 1 {
                        let shown = if config.target.is_empty() { "choose...".to_string() } else { crate::gameplay::crafting::resource_label(&config.target) };
                        egui::ComboBox::from_id_salt("vacuum_filter").selected_text(shown).show_ui(ui, |ui| {
                            for id in crate::gameplay::crafting::resource_ids() {
                                let label = crate::gameplay::crafting::resource_label(&id);
                                ui.selectable_value(&mut config.target, id, label);
                            }
                        });
                    }
                });
                data.configs.dirty = true;
                theme::hint(ui, "Every second, pulls the chosen resources out of the containers in range into the container it is wired to (or next to).");
                container_slots(ui, &mut data, cell, machine);
            }
            BlockFunction::Harvester | BlockFunction::Ethereaper | BlockFunction::Excavator | BlockFunction::Reconstructor => {
                let help = match function {
                    BlockFunction::Harvester => "Bio-machine: gathers the raw resource blocks around it (logs, leaves, ores) into the container it is wired to (or next to).",
                    BlockFunction::Ethereaper => "Every second takes 1 Ethereal from each entity standing on it (x2 here) until it dies.",
                    BlockFunction::Excavator => "Every 4 s spends 10 Voltic for 4 to 8 resources of its biome. Put Voltic in it or wire it.",
                    _ => "Feed it 10 units of a resource to teach it, then each copy costs 2 Atomic + 1000 Voltic (every 3 s).",
                };
                theme::hint(ui, help);
                if function == BlockFunction::Reconstructor {
                    let learnt = machine
                        .and_then(|m| m.storage.iter().find(|(k, c)| !matches!(k.as_str(), "atomic" | "voltic") && **c >= game_rules().machines.reconstructor_learn).map(|(k, _)| k.clone()));
                    row(ui, "Learnt", learnt.map(|k| crate::gameplay::crafting::resource_label(&k)).unwrap_or_else(|| "nothing yet".to_string()));
                }
                container_slots(ui, &mut data, cell, machine);
                if theme::game_small_button(ui, "Take everything", true).clicked() {
                    data.panel.commands.push(MachineCommand::Collect(cell));
                }
            }
            BlockFunction::Infuser => {
                let powered = machine.is_some_and(|m| m.powered);
                ui.label(RichText::new("INFUSION").size(11.0).color(theme::TEXT_DIM));
                slot_station(ui, &mut data, cell, crate::gameplay::crafting::Station::Infuser, 2, "Infuse", powered);
                theme::hint(ui, "Two resources that match no recipe make Garbage (weak Atomiser fuel).");
            }
            BlockFunction::Lamp => {
                row(ui, "Light", if machine.is_some_and(|m| m.powered) { "On".to_string() } else { "Off (no power)".to_string() });
            }
            BlockFunction::Controller => {
                theme::hint(ui, "Connect it with the Remote Control (E with the remote in hand) to drive its movable from afar: Sprint + ZQSD. E again disconnects.");
            }
            BlockFunction::Energizer => {
                let charge = machine.map(|m| m.charge).unwrap_or(0.0);
                let capacity = game_rules().energy.energizer_capacity;
                row(ui, "Charge", format!("{charge:.0} / {capacity:.0}"));
                ui.add(egui::ProgressBar::new((charge / capacity).clamp(0.0, 1.0)).desired_height(8.0).fill(theme::ACCENT));
                row(ui, "Field", format!("{:.0} blocks", def.power_radius));
                theme::hint(ui, "Drop Voltic (or Voltiom) here or wire a Solaronel / Kineticster: 1 Voltic = 20 charge. While charged its field powers every machine inside it.");
                container_slots(ui, &mut data, cell, machine);
            }
            BlockFunction::Container | BlockFunction::AstralContainer | BlockFunction::Custom => {
                container_slots(ui, &mut data, cell, machine);
                ui.horizontal(|ui| {
                    if theme::game_small_button(ui, "Take everything", true).clicked() {
                        data.panel.commands.push(MachineCommand::Collect(cell));
                    }
                });
                theme::hint(ui, "Drag stacks between the storage and your inventory. Resource cables fill it.");
            }
            BlockFunction::Repulsor => {
                let config = data.configs.get_or_default(cell, function);
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Strength").size(12.5).color(theme::TEXT_DIM)));
                    ui.add(egui::Slider::new(&mut config.number, -50.0..=50.0));
                });
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Radius").size(12.5).color(theme::TEXT_DIM)));
                    ui.add(egui::Slider::new(&mut config.radius, 0.5..=32.0).suffix(" blocks"));
                });
                data.configs.dirty = true;
                theme::hint(ui, "Pushes the entities, the player and the movables around it (a negative strength pulls).");
            }
            BlockFunction::Ringer => {
                let config = data.configs.get_or_default(cell, function);
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Radius").size(12.5).color(theme::TEXT_DIM)));
                    ui.add(egui::Slider::new(&mut config.radius, 1.0..=64.0).suffix(" blocks"));
                });
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Target").size(12.5).color(theme::TEXT_DIM)));
                    let shown = if config.target.is_empty() { "Every entity".to_string() } else { config.target.clone() };
                    egui::ComboBox::from_id_salt("ringer_target").selected_text(shown).show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.target, String::new(), "Every entity");
                        for name in &data.mob_names {
                            ui.selectable_value(&mut config.target, name.clone(), name);
                        }
                    });
                });
                data.configs.dirty = true;
                theme::hint(ui, "Attracts the targeted entities in its radius while powered.");
            }
            BlockFunction::Terminal => {
                let output = data.logic.outputs.get(&cell).map(|v| v.as_text()).unwrap_or_else(|| "-".to_string());
                let config = data.configs.get_or_default(cell, function);
                theme::hint(ui, "One command per line (lines starting with # are comments). While powered, runs them one by one in a loop (every 0.5 s).");
                if ui.add(egui::TextEdit::multiline(&mut config.text).desired_rows(8).desired_width(f32::INFINITY).code_editor()).changed() {
                    data.configs.dirty = true;
                }
                row(ui, "Last result", output);
            }
            BlockFunction::Comandor => {
                let output = data.logic.outputs.get(&cell).map(|v| v.as_text()).unwrap_or_else(|| "-".to_string());
                let config = data.configs.get_or_default(cell, function);
                comandor_editor(ui, config, data.registry, &data.mob_names);
                ui.horizontal(|ui| {
                    if theme::game_button(ui, "Run", true, 120.0).clicked() {
                        config.trigger = true;
                    }
                    ui.label(RichText::new(config.text.clone()).size(12.0).monospace().color(theme::TEXT));
                });
                data.configs.dirty = true;
                row(ui, "Last result", output);
                theme::hint(ui, "Runs when you press Run or when its Trigger input turns true (needs power).");
            }
            BlockFunction::Transmitter => {
                let output = data.logic.outputs.get(&cell).map(|v| format!("{} ({})", v.as_text(), v.kind())).unwrap_or_else(|| "-".to_string());
                let config = data.configs.get_or_default(cell, function);
                ui.horizontal_wrapped(|ui| {
                    for (index, gate) in crate::gameplay::machines::config::GATES.iter().enumerate() {
                        if theme::game_tab(ui, config.mode as usize == index, gate).clicked() {
                            config.mode = index as u8;
                        }
                    }
                });
                if config.mode == crate::gameplay::machines::config::DECODER {
                    ui.horizontal(|ui| {
                        for (key, label) in [("bool", "Boolean (received?)"), ("float", "Float (count)"), ("string", "String (name)")] {
                            let selected = config.target == key || (key == "float" && config.target.is_empty());
                            if theme::game_tab(ui, selected, label).clicked() {
                                config.target = key.to_string();
                            }
                        }
                    });
                    theme::hint(ui, "Decoder: wire a Resource cable into its Decode input; what it receives becomes a value.");
                } else {
                    theme::hint(ui, "Logic gate on its inputs A and B (NOT uses A).");
                }
                data.configs.dirty = true;
                row(ui, "Output", output);
            }
            BlockFunction::Visualiser | BlockFunction::Receiver | BlockFunction::Screen => {
                let value = data.logic.inputs.get(&cell).cloned();
                if function == BlockFunction::Receiver {
                    let config = data.configs.get_or_default(cell, function);
                    ui.horizontal(|ui| {
                        ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Variable").size(12.5).color(theme::TEXT_DIM)));
                        ui.text_edit_singleline(&mut config.text);
                    });
                    data.configs.dirty = true;
                }
                match &value {
                    Some(value) => {
                        ui.label(RichText::new(value.as_text()).size(28.0).strong().color(theme::TEXT));
                        row(ui, "Type", value.kind().to_string());
                        if function == BlockFunction::Screen {
                            let color = value.color().to_srgba();
                            let (rect, _) = ui.allocate_exact_size(vec2(120.0, 40.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect, CornerRadius::same(8), Color32::from_rgb((color.red * 255.0) as u8, (color.green * 255.0) as u8, (color.blue * 255.0) as u8));
                        }
                    }
                    None => theme::hint(ui, "Nothing received: wire a Signal output into it."),
                }
            }
            BlockFunction::Emitter => {
                let config = data.configs.get_or_default(cell, function);
                ui.horizontal(|ui| {
                    for (index, label) in ["Boolean", "Float", "String"].iter().enumerate() {
                        if theme::game_tab(ui, config.mode as usize == index, label).clicked() {
                            config.mode = index as u8;
                        }
                    }
                });
                match config.mode {
                    0 => {
                        ui.checkbox(&mut config.enabled, "true");
                    }
                    1 => {
                        ui.add(egui::DragValue::new(&mut config.number).speed(0.1));
                    }
                    _ => {
                        ui.text_edit_singleline(&mut config.text);
                    }
                }
                data.configs.dirty = true;
                theme::hint(ui, "Emits this value on its Signal output, all the time.");
            }
            BlockFunction::Laser => {
                let output = data.logic.outputs.get(&cell).map(|v| v.as_text()).unwrap_or_else(|| "-".to_string());
                let config = data.configs.get_or_default(cell, function);
                ui.horizontal(|ui| {
                    for (index, label) in ["Hit (Boolean)", "Length (Float)", "Name (String)"].iter().enumerate() {
                        if theme::game_tab(ui, config.mode as usize == index, label).clicked() {
                            config.mode = index as u8;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Length").size(12.5).color(theme::TEXT_DIM)));
                    ui.add(egui::Slider::new(&mut config.number, 1.0..=128.0).suffix(" blocks"));
                });
                data.configs.dirty = true;
                row(ui, "Output", output);
                theme::hint(ui, "Line trace from its front face (turn it with the Wrench).");
            }
            BlockFunction::Ressourcer => {
                let config = data.configs.get_or_default(cell, function);
                ui.horizontal(|ui| {
                    if theme::game_tab(ui, config.mode == 0, "Collect").clicked() {
                        config.mode = 0;
                    }
                    if theme::game_tab(ui, config.mode == 1, "Distribute").clicked() {
                        config.mode = 1;
                    }
                });
                data.configs.dirty = true;
                theme::hint(ui, "Collect: takes the resources of the entities (and yours) walking on it. Distribute: gives its storage to the player on it.");
                container_slots(ui, &mut data, cell, machine);
            }
            BlockFunction::Etheraltar => {
                let config = data.configs.get_or_default(cell, function);
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Name").size(12.5).color(theme::TEXT_DIM)));
                    ui.text_edit_singleline(&mut config.text);
                });
                data.configs.dirty = true;
                theme::hint(ui, &format!("Teleport to another Etheraltar for {} Ethereal. The death screen can respawn you here.", game_rules().vitals.respawn_altar_cost));
                let mut altars: Vec<bevy::prelude::IVec3> =
                    data.grid.machines.iter().filter(|(c, m)| m.function == BlockFunction::Etheraltar && **c != cell).map(|(c, _)| *c).collect();
                altars.sort_by_key(|c| (c.x, c.y, c.z));
                if altars.is_empty() {
                    theme::hint(ui, "No other Etheraltar placed.");
                }
                for altar in altars {
                    let name = data.configs.get(altar).map(|c| c.text.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| format!("{} {} {}", altar.x, altar.y, altar.z));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).size(13.0).color(theme::TEXT));
                        if theme::game_small_button(ui, "Teleport", true).clicked() {
                            data.panel.commands.push(MachineCommand::Teleport(altar));
                        }
                    });
                }
            }
            BlockFunction::Invocateral => {
                theme::hint(ui, &format!("Summons an entity of a biome you discovered and whose entities you all killed once, for {} Ethereal.", game_rules().machines.summon_cost));
                if data.summonable.is_empty() {
                    theme::hint(ui, "No biome available yet.");
                }
                for biome in data.summonable.clone() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&biome).size(13.0).color(theme::TEXT));
                        if theme::game_small_button(ui, "Summon", machine.is_some_and(|m| m.powered)).clicked() {
                            data.panel.commands.push(MachineCommand::Summon(cell, biome.clone()));
                        }
                    });
                }
            }
            BlockFunction::Televerseur => {
                let config = data.configs.get_or_default(cell, function);
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("File name").size(12.5).color(theme::TEXT_DIM)));
                    ui.text_edit_singleline(&mut config.text);
                });
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 20.0), egui::Label::new(RichText::new("Box size").size(12.5).color(theme::TEXT_DIM)));
                    for axis in 0..3 {
                        ui.add(egui::DragValue::new(&mut config.size[axis]).range(1..=64));
                    }
                });
                data.configs.dirty = true;
                theme::hint(ui, "The box sits on top of the Televerseur. Export writes project/exports/<name>.ron; Import places that file in the box.");
                ui.horizontal(|ui| {
                    if theme::game_small_button(ui, "Export", true).clicked() {
                        data.panel.commands.push(MachineCommand::Export(cell));
                    }
                    if theme::game_small_button(ui, "Import", true).clicked() {
                        data.panel.commands.push(MachineCommand::Import(cell));
                    }
                });
            }
            BlockFunction::MnemoDigestor => {
                theme::hint(ui, "Bio-machine: hurts every entity walking on it (2 damage per second). Bound entities are spared.");
            }
            BlockFunction::MnemoCloner => {
                theme::hint(ui, &format!("Makes a Mnemo double of an entity you killed for {} Mnemonical. The clone then eats 1 Ethereal per second (and 1 per damage taken) from this storage; it dissolves without Ethereal or when it goes too far.", game_rules().machines.clone_cost));
                container_slots(ui, &mut data, cell, machine);
                for name in data.killed.clone() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&name).size(13.0).color(theme::TEXT));
                        if theme::game_small_button(ui, "Clone", true).clicked() {
                            data.panel.commands.push(MachineCommand::Clone(cell, name.clone()));
                        }
                    });
                }
            }
            BlockFunction::AstralModifier => {
                let powered = machine.is_some_and(|m| m.powered);
                row(ui, "Astral", data.inventory.count("astral").to_string());
                for (index, (label, cost)) in crate::gameplay::machines::advanced::boosts().iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{label}  -  {cost} Astral")).size(13.0).color(theme::TEXT));
                        if theme::game_small_button(ui, "Buy", powered).clicked() {
                            data.panel.commands.push(MachineCommand::Boost(cell, index));
                        }
                    });
                }
                theme::hint(ui, "Permanent boosts of the player stats for this play session.");
            }
            _ => {}
        });
        if with_inventory {
            ui.add_space(8.0);
            if let Some(crate::editor::game_ui::Drag::Stored(key)) =
                crate::editor::game_ui::inventory_strip(ui, data.inventory, data.registry, data.items, 200.0)
            {
                data.panel.commands.push(MachineCommand::Withdraw(cell, key));
            }
        }
    });
    // The dragged tile follows the pointer.
    if let (Some(payload), Some(pointer)) = (egui::DragAndDrop::payload::<crate::editor::game_ui::Drag>(ctx), ctx.pointer_interact_pos()) {
        let key = match payload.as_ref() {
            crate::editor::game_ui::Drag::Entry(item) => item.resource_key(data.registry),
            crate::editor::game_ui::Drag::Stored(key) => Some(key.clone()),
            crate::editor::game_ui::Drag::Slot(_) => None,
            crate::editor::game_ui::Drag::Grid(cell) => data.inventory.grid.get(*cell).cloned().flatten(),
        };
        if let Some(key) = key {
            crate::editor::game_ui::paint_drag_ghost(ctx, pointer, &key, data.registry, data.items);
        }
    }
    // `E` closes the panel (not on the frame that opened it).
    let typing = ctx.memory(|m| m.focused().is_some());
    if !data.panel.just_opened && !typing && ctx.input(|i| i.key_pressed(egui::Key::E)) {
        close = true;
    }
    data.panel.just_opened = false;
    if close {
        data.panel.open = None;
    }
}


/// Comandor: the command and its arguments with combo boxes (no typing for
/// the known kinds).
fn comandor_editor(ui: &mut egui::Ui, config: &mut crate::gameplay::machines::config::MachineConfig, registry: &BlockRegistry, mobs: &[String]) {
    use crate::gameplay::commands::{spec, Arg, SPECS};
    let mut words: Vec<String> = config.text.trim().trim_start_matches('/').split_whitespace().map(str::to_string).collect();
    if words.is_empty() {
        words.push("settime".to_string());
    }
    let mut name = words[0].clone();
    egui::ComboBox::from_id_salt("comandor_command").selected_text(format!("/{name}")).width(260.0).show_ui(ui, |ui| {
        for spec in SPECS {
            ui.selectable_value(&mut name, spec.name.to_string(), format!("/{}", spec.name)).on_hover_text(spec.help);
        }
    });
    let Some(spec) = spec(&name) else { return };
    if name != words[0] {
        words = vec![name.clone()];
    }
    ui.label(RichText::new(spec.format).size(11.5).color(theme::TEXT_DIM));
    let mut args: Vec<String> = words.iter().skip(1).cloned().collect();
    args.resize(spec.args.len().max(args.len()), String::new());
    let blocks: Vec<String> = registry.blocks().filter(|(_, d)| d.variant_of.is_none() && !d.hidden).map(|(_, d)| d.id.clone()).collect();
    egui::Grid::new("comandor_args").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        for (index, kind) in spec.args.iter().enumerate() {
            ui.label(RichText::new(format!("{:?}", kind)).size(12.0).color(theme::TEXT_DIM));
            let value = &mut args[index];
            let options: Vec<String> = match kind {
                Arg::Block => blocks.clone(),
                Arg::Entity => ["@p", "@s", "@n"].iter().map(|s| s.to_string()).chain(mobs.iter().map(|m| m.replace(' ', "_"))).collect(),
                Arg::Mob => mobs.iter().map(|m| m.replace(' ', "_")).collect(),
                Arg::Resource => crate::gameplay::crafting::resource_ids(),
                Arg::Stat => ["health", "maxhealth", "stamina", "speed", "damage", "armor", "hostile"].iter().map(|s| s.to_string()).collect(),
                Arg::Time => ["day", "noon", "sunset", "night", "midnight", "sunrise"].iter().map(|s| s.to_string()).collect(),
                Arg::Kind => ["energy", "resource", "signal"].iter().map(|s| s.to_string()).collect(),
                Arg::Mode => ["creative", "survival", "spectator"].iter().map(|s| s.to_string()).collect(),
                Arg::Command => SPECS.iter().map(|s| s.name.to_string()).collect(),
                _ => Vec::new(),
            };
            if options.is_empty() {
                ui.add(egui::TextEdit::singleline(value).desired_width(180.0));
            } else {
                egui::ComboBox::from_id_salt(("comandor_arg", index)).selected_text(if value.is_empty() { "..." } else { value.as_str() }).width(180.0).show_ui(ui, |ui| {
                    for option in options {
                        let label = option.clone();
                        ui.selectable_value(value, option, label);
                    }
                });
            }
            ui.end_row();
        }
    });
    let mut text = format!("/{name}");
    for arg in args.iter().filter(|a| !a.trim().is_empty()) {
        text.push(' ');
        text.push_str(arg.trim());
    }
    config.text = text;
}
