//! 🏰 Atelier de structures panels: prefab list and brush palette (left),
//! prefab details and tools (right). The prefab itself is edited directly in
//! the viewport ([`crate::editor::structure_studio`]).

use egui::RichText;

use crate::core::app_state::{EditorState, EditorUiState, Workspace};
use crate::core::project_io::ProjectLayout;
use crate::editor::structure_studio::{StructureStudio, StructureTool};
use crate::editor::theme;
use crate::voxel::blocks::{BlockId, BlockRegistry};
use crate::voxel::structures::{StructureDef, StructureLibrary, StructureRoom, ROOM_ORIGIN};
use crate::voxel::world::VoxelWorld;

/// Left panel: prefabs and the brush block palette.
pub fn show_list(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    state: &mut EditorUiState,
    library: &mut StructureLibrary,
    registry: &BlockRegistry,
    studio: &mut StructureStudio,
    project: &ProjectLayout,
) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("➕ New").clicked() {
            let mut index = library.structures.len() + 1;
            while library.find(&format!("Structure {index}")).is_some() {
                index += 1;
            }
            library.structures.push(StructureDef::new(&format!("Structure {index}"), [1, 1, 1]));
            library.selection = library.structures.len() - 1;
            library.revision = library.revision.wrapping_add(1);
            studio.dirty = true;
        }
        if ui.button("🗐 Duplicate").clicked() {
            if let Some(mut copy) = library.structures.get(library.selection).cloned() {
                copy.name = format!("{} copy", copy.name);
                library.structures.push(copy);
                library.selection = library.structures.len() - 1;
                library.revision = library.revision.wrapping_add(1);
                studio.dirty = true;
            }
        }
        if ui.add_enabled(library.structures.len() > 1, egui::Button::new("🗑")).on_hover_text("Remove from the list").clicked() {
            library.structures.remove(library.selection);
            library.selection = library.selection.min(library.structures.len().saturating_sub(1));
            library.revision = library.revision.wrapping_add(1);
        }
        if ui.button("📂").on_hover_text("Reload from disk").clicked() {
            match library.load_from_project(project) {
                Ok(_) => {
                    studio.dirty = false;
                    state.action("Structures reloaded.");
                }
                Err(error) => state.error(error),
            }
        }
    });
    theme::section_label(ui, "STRUCTURES");
    egui::ScrollArea::vertical().id_salt("prefab_list").max_height(ui.available_height() * 0.45).show(ui, |ui| {
        for index in 0..library.structures.len() {
            let structure = &library.structures[index];
            let label = format!(
                "{}  -  {}x{}x{}  -  {} blocks",
                structure.name, structure.size[0], structure.size[1], structure.size[2], structure.cells.len()
            );
            if ui.selectable_label(library.selection == index, label).clicked() {
                library.selection = index;
            }
        }
    });

    ui.separator();
    theme::section_label(ui, "BRUSH");
    let current = registry.get(editor.selected_block);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 3.0, egui::Color32::from_rgb(current.color[0], current.color[1], current.color[2]));
        ui.label(RichText::new(&current.label).strong());
    });
    egui::ScrollArea::vertical().id_salt("prefab_palette").show(ui, |ui| {
        let entries: Vec<(BlockId, String, [u8; 3])> =
            registry.blocks().map(|(id, def)| (id, def.label.clone(), def.color)).collect();
        ui.horizontal_wrapped(|ui| {
            for (id, label, color) in entries {
                let selected = editor.selected_block == id;
                let (rect, response) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
                ui.painter().rect_filled(rect, 4.0, egui::Color32::from_rgb(color[0], color[1], color[2]));
                if selected {
                    ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(2.0, egui::Color32::WHITE), egui::StrokeKind::Outside);
                }
                if response.on_hover_text(label).clicked() {
                    editor.selected_block = id;
                    studio.tool = StructureTool::Place;
                }
            }
        });
    });
}

/// Right panel: details, tools and saving.
#[allow(clippy::too_many_arguments)]
pub fn show_details(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    state: &mut EditorUiState,
    library: &mut StructureLibrary,
    room: &mut StructureRoom,
    world: &mut VoxelWorld,
    registry: &BlockRegistry,
    studio: &mut StructureStudio,
    project: &ProjectLayout,
) {
    let selection = library.selection;
    ui.horizontal(|ui| {
        if theme::primary_button(ui, "💾 Save", theme::ACCENT).clicked() {
            match library.save_one(project, selection) {
                Ok(path) => {
                    studio.dirty = false;
                    state.action(format!("Structure saved: {}", path.display()));
                }
                Err(error) => state.error(error),
            }
        }
        if studio.dirty {
            ui.label(RichText::new("● modified").size(10.0).color(theme::WARN));
        }
    });
    ui.separator();
    let Some(structure) = library.structures.get_mut(selection) else {
        return;
    };
    let mut changed = false;
    theme::section_label(ui, "STRUCTURE");
    egui::Grid::new("structure_fields").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Name");
        changed |= ui.text_edit_singleline(&mut structure.name).changed();
        ui.end_row();
        ui.label("Size");
        ui.label(format!("{} x {} x {}", structure.size[0], structure.size[1], structure.size[2]));
        ui.end_row();
        ui.label("Blocks");
        ui.label(format!("{}", structure.cells.len()));
        ui.end_row();
        ui.label("Anchors");
        ui.label(format!("{}", structure.anchors.len()));
        ui.end_row();
    });
    theme::tip(ui, "Without anchors, the blocks of layer 0 act as anchors: they must touch the ground for the generator to place the structure.");

    theme::section_label(ui, "TOOLS");
    ui.horizontal_wrapped(|ui| {
        for tool in StructureTool::ALL {
            if theme::toggle_button(ui, studio.tool == tool, tool.icon(), tool.label()).clicked() {
                studio.tool = tool;
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button("🎯 Frame").clicked() {
            studio.frame_request = true;
        }
        if ui.button("✖ Clear").clicked() {
            structure.cells.clear();
            structure.anchors.clear();
            structure.size = [1, 1, 1];
            changed = true;
        }
    });

    if changed {
        library.revision = library.revision.wrapping_add(1);
        studio.dirty = true;
    }

    ui.separator();
    theme::section_label(ui, "ADVANCED");
    let Some(structure) = library.structures.get(selection) else {
        return;
    };
    if ui
        .button("🌍 Edit in the world (isolated room)")
        .on_hover_text("Opens the structure in a room above the world, with the normal brush.")
        .clicked()
    {
        let snapshot = structure.clone();
        room.open(world, registry, &snapshot, selection);
        editor.workspace = Workspace::World;
        state.action(format!(
            "Structure '{}' opened in the room ({}, {}, {}).",
            snapshot.name, ROOM_ORIGIN.x, ROOM_ORIGIN.y, ROOM_ORIGIN.z
        ));
    }
    if let Some(editing) = room.editing {
        ui.horizontal(|ui| {
            if ui.button("📥 Capture the room").clicked() {
                if let Some(target) = library.structures.get_mut(editing) {
                    let count = room.capture(world, registry, target);
                    state.action(format!("{count} block(s) captured."));
                }
                library.revision = library.revision.wrapping_add(1);
                studio.dirty = true;
            }
            if ui.button("✖ Close the room").clicked() {
                room.clear(world, room.size);
                room.editing = None;
                room.anchor_mode = false;
                state.info("Room closed.");
            }
        });
    }
}
