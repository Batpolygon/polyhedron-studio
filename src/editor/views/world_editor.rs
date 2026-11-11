//! World & gameplay panel: data cable tool, play rules, mixer and a live
//! readout of the runtime systems (energy grid, vehicles, VM instances).

use egui::RichText;

use crate::audio::AudioSettings;
use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::editor::theme;
use crate::gameplay::data_cables::CableNetwork;
use crate::voxel::blocks::PortKind;
use crate::gameplay::energy_grid::EnergyGrid;
use crate::gameplay::inventory::Inventory;
use crate::gameplay::world_rules::WorldRules;
use crate::scripting::vm::ScriptRuntime;

#[allow(clippy::too_many_arguments)]
/// What the procedural generation section edits.
pub struct Generation<'a> {
    pub world: &'a mut crate::voxel::world::VoxelWorld,
    pub streaming: &'a mut crate::voxel::chunk_manager::WorldStreaming,
    pub biomes: &'a crate::voxel::biomes::BiomeLibrary,
    pub structures: &'a crate::voxel::structures::StructureLibrary,
    pub registry: &'a crate::voxel::blocks::BlockRegistry,
    pub fluids: &'a crate::voxel::fluids::FluidSim,
}

/// Generate a procedural world directly from the world workspace.
fn generation_section(ui: &mut egui::Ui, state: &mut EditorUiState, generation: Generation) {
    use crate::editor::worlds::{generate, WorldKind, WorldMeta};
    theme::section_label(ui, "PROCEDURAL GENERATION");
    let id = egui::Id::new("world_generation_params");
    let mut meta: WorldMeta = ui.ctx().data_mut(|d| d.get_temp(id)).unwrap_or_default();
    egui::Grid::new("world_generation").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Type");
        egui::ComboBox::from_id_salt("gen_kind")
            .selected_text(meta.kind.label())
            .show_ui(ui, |ui| {
                for kind in WorldKind::ALL {
                    ui.selectable_value(&mut meta.kind, kind, kind.label());
                }
            });
        ui.end_row();
        if meta.kind == WorldKind::Infinite {
            ui.label("Seed");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut meta.seed));
                if ui.small_button("🎲").clicked() {
                    meta.seed = meta.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                }
            });
            ui.end_row();
            ui.label("Biomes");
            egui::ComboBox::from_id_salt("gen_biome")
                .selected_text(meta.biome.clone().unwrap_or_else(|| "All (climate)".into()))
                .show_ui(ui, |ui| {
                    if ui.selectable_label(meta.biome.is_none(), "All (climate)").clicked() {
                        meta.biome = None;
                    }
                    for biome in &generation.biomes.biomes {
                        if ui.selectable_label(meta.biome.as_deref() == Some(&biome.name), &biome.name).clicked() {
                            meta.biome = Some(biome.name.clone());
                        }
                    }
                });
            ui.end_row();
            ui.label("Sea level");
            ui.add(egui::Slider::new(&mut meta.sea_level, -8..=32));
            ui.end_row();
        }
    });
    if ui
        .button("✨ Generate the world")
        .on_hover_text("Replaces the current world (save it first if needed).")
        .clicked()
    {
        generate(&meta, generation.world, generation.streaming, generation.biomes, generation.structures, generation.registry);
        state.world_name = format!("monde_{}", meta.seed);
        state.action(format!("World '{}' generated.", meta.kind.label()));
    }
    let streaming = generation.streaming;
    ui.horizontal(|ui| {
        ui.checkbox(&mut streaming.enabled, "Streaming");
        ui.add(egui::Slider::new(&mut streaming.radius, 1..=16).text("radius"));
        ui.checkbox(&mut streaming.unload, "Unload far away");
    });
    ui.label(
        RichText::new(format!(
            "{} column(s) loaded - {} pending - {} edited in memory - {} active liquid(s)",
            streaming.generated_count(),
            streaming.pending(),
            streaming.stored_count(),
            generation.fluids.last_updates
        ))
        .size(9.0)
        .color(theme::TEXT_DIM),
    );
    ui.ctx().data_mut(|d| d.insert_temp(id, meta));
    ui.separator();
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    generation: Generation,
    state: &mut EditorUiState,
    cables: &mut CableNetwork,
    inventory: &mut Inventory,
    audio: &mut AudioSettings,
    rules: &mut WorldRules,
    grid: &EnergyGrid,
    runtime: &ScriptRuntime,
    project: &ProjectLayout,
) {
    generation_section(ui, state, generation);
    theme::section_label(ui, "CABLES");
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut state.cable_tool, "Cable tool")
            .on_hover_text("LMB on two machines connects an output to an input of the chosen kind. X on a block removes its cables. In game, hold the Cable item.")
            .changed()
        {
            cables.pending = None;
        }
        egui::ComboBox::from_id_salt("cable_kind")
            .selected_text(cables.kind.label())
            .show_ui(ui, |ui| {
                for kind in PortKind::ALL {
                    ui.selectable_value(&mut cables.kind, kind, kind.label());
                }
            });
    });
    ui.label(
        RichText::new(format!(
            "{} cable(s) - {} transmission(s) this session{}",
            cables.cables.len(),
            cables.transmitted,
            if cables.pending.is_some() { " - waiting for the 2nd block" } else { "" }
        ))
        .size(9.0)
        .color(theme::TEXT_DIM),
    );
    ui.horizontal(|ui| {
        if ui.button("Save cables").clicked() {
            match cables.save(project) {
                Ok(()) => state.action("Cables saved."),
                Err(error) => state.error(error),
            }
        }
        if ui.button("Clear all").clicked() {
            cables.cables.clear();
            cables.studio.clear();
            state.warn("All cables removed (save to persist).");
        }
    });

    ui.separator();
    theme::section_label(ui, "PLAY RULES");
    ui.checkbox(&mut inventory.survival, "Survival (placing consumes inventory)");
    ui.checkbox(&mut rules.auto_cycle, "Automatic day cycle (when no script drives it)");
    ui.add(egui::Slider::new(&mut rules.day_length, 30.0..=1200.0).text("day length (s)"));
    ui.add(egui::Slider::new(&mut rules.time_of_day, 0.0..=1.0).text("time of day"));

    ui.separator();
    theme::section_label(ui, "PROCEDURAL AUDIO");
    ui.add(egui::Slider::new(&mut audio.master_volume, 0.0..=1.0).text("master"));
    ui.add(egui::Slider::new(&mut audio.sfx_volume, 0.0..=1.0).text("sfx"));
    ui.add(egui::Slider::new(&mut audio.ui_volume, 0.0..=1.0).text("ui"));
    ui.add(egui::Slider::new(&mut audio.music_volume, 0.0..=1.0).text("ambience"));
    ui.checkbox(&mut audio.footsteps, "Footsteps");

    ui.separator();
    theme::section_label(ui, "RUNTIME");
    let generators = grid.generators.iter().filter(|g| g.active).count();
    ui.label(
        RichText::new(format!(
            "Energy: {} / {} generator(s) active - {} / {} machine(s) powered",
            generators,
            grid.generators.len(),
            grid.powered_count(),
            grid.machines.len()
        ))
        .size(9.5),
    );
    ui.label(
        RichText::new(format!(
            "Weave VM: {} - {} global program(s) - {} bound instance(s)",
            if runtime.is_running() { "running" } else { "stopped" },
            runtime.program_count().saturating_sub(runtime.instance_count()),
            runtime.instance_count()
        ))
        .size(9.5),
    );
    let templates = runtime.template_names();
    if !templates.is_empty() {
        ui.label(
            RichText::new(format!("Bound templates: {}", templates.join(", ")))
                .size(9.0)
                .color(theme::TEXT_DIM),
        );
    }
    if !inventory.resources.is_empty() {
        let items: Vec<String> = inventory
            .resources
            .iter()
            .map(|(name, count)| format!("{name} x{count}"))
            .collect();
        ui.label(RichText::new(format!("Inventory: {}", items.join(", "))).size(9.0));
    }
    ui.label(
        RichText::new(
            "PIE controls: ZQSD/WASD move · Space jump · Shift sprint · Ctrl dash · F5 camera · \
             1-9 hotbar · LMB mine / attack · RMB place / parry · E interact · Shift+Remote drive",
        )
        .size(8.5)
        .color(theme::TEXT_DIM),
    );
}
