//! Inspector panel.
//!
//! Inspects the world root or a chunk selected in the hierarchy, then exposes the
//! live brush target and the data-driven block brush.

use egui::{RichText, Sense, Stroke, vec2};

use crate::core::app_state::{EditorState, EditorUiState};
use crate::editor::theme;
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::picking::VoxelCursor;
use crate::voxel::world::{VoxelWorld, CHUNK_SIZE, CHUNK_VOLUME};

pub fn show(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    state: &mut EditorUiState,
    world: &VoxelWorld,
    cursor: &VoxelCursor,
    registry: &BlockRegistry,
) {
    let Some(entry) = editor.selected_entry() else {
        ui.label(RichText::new("Nothing selected").color(theme::TEXT_DIM));
        return;
    };

    let name = entry.name.clone();
    let kind = entry.kind;
    let color = entry.color;
    let coord = entry.coord;
    let voxels = entry.voxels;
    let is_world = entry.is_world();

    theme::section_label(ui, "SELECTION");
    ui.label(RichText::new(name).size(15.0).strong().color(theme::TEXT));
    ui.label(RichText::new(kind).size(10.5).color(theme::ACCENT));

    ui.separator();

    egui::Grid::new("polyhedron_inspector_properties")
        .num_columns(2)
        .spacing(vec2(14.0, 7.0))
        .show(ui, |ui| {
            ui.label(RichText::new("Index").size(10.5).color(theme::TEXT_DIM));
            ui.label(
                RichText::new(format!("{}", editor.selected_scene))
                    .size(10.5)
                    .color(theme::TEXT),
            );
            ui.end_row();

            ui.label(RichText::new("Color").size(10.5).color(theme::TEXT_DIM));
            let (swatch, _) = ui.allocate_exact_size(vec2(34.0, 14.0), Sense::hover());
            ui.painter().rect_filled(swatch, 2.0, color);
            ui.painter().rect_stroke(
                swatch,
                2.0,
                Stroke::new(1.0, theme::BORDER),
                egui::StrokeKind::Inside,
            );
            ui.end_row();

            if is_world {
                let stats = world.stats();
                ui.label(RichText::new("Chunks").size(10.5).color(theme::TEXT_DIM));
                ui.label(
                    RichText::new(format!("{}", stats.chunks))
                        .size(10.5)
                        .color(theme::TEXT),
                );
                ui.end_row();

                ui.label(RichText::new("Voxels").size(10.5).color(theme::TEXT_DIM));
                ui.label(
                    RichText::new(format!("{}", stats.voxels))
                        .size(10.5)
                        .color(theme::TEXT),
                );
                ui.end_row();

                ui.label(RichText::new("Dirty").size(10.5).color(theme::TEXT_DIM));
                ui.label(
                    RichText::new(format!("{} chunk(s)", stats.dirty))
                        .size(10.5)
                        .color(theme::TEXT),
                );
                ui.end_row();

                ui.label(RichText::new("Revision").size(10.5).color(theme::TEXT_DIM));
                ui.label(
                    RichText::new(format!("{}", stats.revision))
                        .size(10.5)
                        .color(theme::TEXT),
                );
                ui.end_row();
            } else if let Some(coord) = coord {
                let origin = coord * CHUNK_SIZE as i32;
                ui.label(RichText::new("Coord").size(10.5).color(theme::TEXT_DIM));
                ui.label(
                    RichText::new(format!("[{}, {}, {}]", coord.x, coord.y, coord.z))
                        .size(10.5)
                        .color(theme::TEXT),
                );
                ui.end_row();

                ui.label(RichText::new("Origin").size(10.5).color(theme::TEXT_DIM));
                ui.label(
                    RichText::new(format!("[{}, {}, {}]", origin.x, origin.y, origin.z))
                        .size(10.5)
                        .color(theme::TEXT),
                );
                ui.end_row();

                ui.label(RichText::new("Voxels").size(10.5).color(theme::TEXT_DIM));
                let fill = voxels as f32 / CHUNK_VOLUME as f32 * 100.0;
                ui.label(
                    RichText::new(format!("{}  ({fill:.1}%)", voxels))
                        .size(10.5)
                        .color(theme::TEXT),
                );
                ui.end_row();
            }

            ui.label(RichText::new("Fusion").size(10.5).color(theme::TEXT_DIM));
            ui.label(
                RichText::new(if state.greedy_meshing { "enabled" } else { "disabled" })
                    .size(10.5)
                    .color(theme::TEXT),
            );
            ui.end_row();

            ui.label(RichText::new("World").size(10.5).color(theme::TEXT_DIM));
            ui.label(
                RichText::new(format!(
                    "{} ({})",
                    state.world_name,
                    if world.revision == state.last_saved_revision {
                        "saved"
                    } else {
                        "modified"
                    }
                ))
                .size(10.5)
                .color(theme::TEXT),
            );
            ui.end_row();

            ui.label(RichText::new("Blocks").size(10.5).color(theme::TEXT_DIM));
            ui.label(
                RichText::new(format!("{} registered", registry.len()))
                    .size(10.5)
                    .color(theme::TEXT),
            );
            ui.end_row();
        });

    ui.separator();
    theme::section_label(ui, "PICK TARGET");

    egui::Grid::new("polyhedron_inspector_cursor")
        .num_columns(2)
        .spacing(vec2(14.0, 7.0))
        .show(ui, |ui| {
            let target = cursor.target.map(|hit| registry.get(hit.block));

            ui.label(RichText::new("Block").size(10.5).color(theme::TEXT_DIM));
            ui.label(
                RichText::new(target.map(|def| def.label.as_str()).unwrap_or("-"))
                    .size(10.5)
                    .color(theme::TEXT),
            );
            ui.end_row();

            ui.label(RichText::new("Shape").size(10.5).color(theme::TEXT_DIM));
            ui.label(
                RichText::new(
                    target
                        .map(|def| def.shape.label())
                        .unwrap_or("-"),
                )
                .size(10.5)
                .color(theme::TEXT),
            );
            ui.end_row();

            ui.label(RichText::new("Voxel").size(10.5).color(theme::TEXT_DIM));
            ui.label(
                RichText::new(
                    cursor
                        .target
                        .map(|hit| format!("[{}, {}, {}]", hit.voxel.x, hit.voxel.y, hit.voxel.z))
                        .unwrap_or_else(|| "-".to_string()),
                )
                .size(10.5)
                .color(theme::TEXT),
            );
            ui.end_row();

            ui.label(RichText::new("Placement").size(10.5).color(theme::TEXT_DIM));
            ui.label(
                RichText::new(
                    cursor
                        .placement
                        .map(|cell| format!("[{}, {}, {}]", cell.x, cell.y, cell.z))
                        .unwrap_or_else(|| "-".to_string()),
                )
                .size(10.5)
                .color(theme::TEXT),
            );
            ui.end_row();

            ui.label(RichText::new("Distance").size(10.5).color(theme::TEXT_DIM));
            ui.label(
                RichText::new(
                    cursor
                        .target
                        .map(|hit| format!("{:.2} m", hit.distance))
                        .unwrap_or_else(|| "-".to_string()),
                )
                .size(10.5)
                .color(theme::TEXT),
            );
            ui.end_row();
        });

    ui.separator();
    theme::section_label(ui, "VOXEL BRUSH");

    egui::ComboBox::from_id_salt("polyhedron_block_combo")
        .selected_text(registry.get(editor.selected_block).label.as_str())
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for (block, def) in registry.blocks() {
                ui.selectable_value(&mut editor.selected_block, block, def.label.as_str());
            }
        });

    let selected = registry.get(editor.selected_block);
    ui.label(
        RichText::new(format!(
            "{}  •  {}  •  rough {:.2}  •  metal {:.2}  •  emissive {:.2}",
            selected.id,
            selected.shape.label(),
            selected.roughness,
            selected.metallic,
            selected.emissive
        ))
        .size(9.5)
        .color(theme::TEXT_DIM),
    );

    ui.checkbox(&mut state.greedy_meshing, "Greedy meshing");
    ui.horizontal(|ui| {
        ui.checkbox(&mut state.snap_to_grid, "Snap to grid");
        ui.add(
            egui::DragValue::new(&mut state.grid_size)
                .range(0.25..=4.0)
                .speed(0.05)
                .fixed_decimals(2)
                .suffix(" m"),
        );
    });

    theme::tip(ui, "LMB place  •  Shift+LMB or X erase");
}
