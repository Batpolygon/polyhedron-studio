//! Scene hierarchy panel.

use egui::{RichText, Sense, vec2};

use crate::core::app_state::{EditorState, EditorUiState};
use crate::editor::theme;

pub fn show(ui: &mut egui::Ui, editor: &mut EditorState, state: &mut EditorUiState) {
    ui.horizontal(|ui| {
        theme::section_label(ui, "SCENE");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} OBJECTS", editor.scene_entries.len()))
                    .size(9.5)
                    .color(theme::TEXT_DIM),
            );
        });
    });

    ui.add(
        egui::TextEdit::singleline(&mut state.hierarchy_filter)
            .hint_text("Filter objects")
            .desired_width(f32::INFINITY),
    );

    ui.add_space(2.0);

    let filter = state.hierarchy_filter.trim().to_lowercase();
    let mut clicked = None;

    egui::ScrollArea::vertical()
        .id_salt("polyhedron_hierarchy_tree")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, entry) in editor.scene_entries.iter().enumerate() {
                if !filter.is_empty() && !entry.name.to_lowercase().contains(&filter) {
                    continue;
                }

                let selected = index == editor.selected_scene;
                let response = ui
                    .horizontal(|ui| {
                        let (dot_rect, _) =
                            ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(dot_rect.center(), 3.0, entry.color);

                        let label = ui.selectable_label(
                            selected,
                            RichText::new(entry.name.as_str())
                                .size(12.0)
                                .color(if selected { theme::TEXT } else { theme::TEXT_DIM }),
                        );

                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    RichText::new(format!("{} vx", entry.voxels))
                                        .size(9.0)
                                        .color(theme::TEXT_DIM),
                                );
                            },
                        );

                        label
                    })
                    .inner;

                if response.clicked() {
                    clicked = Some(index);
                }
            }
        });

    if let Some(index) = clicked {
        editor.selected_scene = index;
        state.action(format!("Selected '{}'", editor.selected_name()));
    }

    ui.separator();
    theme::tip(ui, "Select a chunk to frame its bounds in the viewport.");
}
