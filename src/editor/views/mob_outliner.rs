//! 💀 Squelette: mob selector and cube hierarchy (Blockbench outliner).

use egui::{Color32, RichText, Sense, vec2};

use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::editor::mob_studio::MobStudio;
use crate::editor::theme;
use crate::gameplay::mobs::library::{MobLibrary, MobSeverity};

pub fn show(
    ui: &mut egui::Ui,
    library: &mut MobLibrary,
    studio: &mut MobStudio,
    state: &mut EditorUiState,
    project: &ProjectLayout,
) {
    library.clamp_selection();

    // --- Mob selector -----------------------------------------------------------
    ui.horizontal(|ui| {
        ui.label(RichText::new("🐾").size(16.0));
        let current = library.current().map(|rig| rig.name.clone()).unwrap_or_else(|| "No mob".into());
        egui::ComboBox::from_id_salt("mob_select")
            .selected_text(RichText::new(current).strong())
            .width(ui.available_width() - 4.0)
            .show_ui(ui, |ui| {
                for index in 0..library.rigs.len() {
                    let rig = &library.rigs[index];
                    let name = match rig.role {
                        crate::gameplay::mobs::model::RigRole::Mob => rig.name.clone(),
                        role => format!("{}  -  {}", rig.name, role.label()),
                    };
                    if ui.selectable_label(library.selection == index, name).clicked() {
                        library.selection = index;
                        library.bone_selection = None;
                        library.clamp_selection();
                        studio.frame_request = true;
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        if theme::icon_button(ui, "➕", "New mob").clicked() {
            let index = library.add_rig();
            state.action(format!("Mob '{}' created.", library.rigs[index].name));
            studio.frame_request = true;
        }
        if theme::icon_button(ui, "🗐", "Duplicate the mob").clicked() {
            let selection = library.selection;
            if let Some(name) = library.duplicate_rig(selection) {
                state.action(format!("Mob duplicated: {name}"));
            }
        }
        if theme::icon_button(ui, "🗑", "Delete the mob").clicked() {
            let selection = library.selection;
            match library.remove_rig(selection) {
                Some(name) => state.warn(format!("Mob '{name}' deleted (save to confirm).")),
                None => state.warn("The player body and hand can be edited but not deleted."),
            }
        }
        ui.separator();
        let dirty = library.is_dirty();
        if theme::icon_button(ui, if dirty { "💾•" } else { "💾" }, "Save every mob").clicked() {
            match library.save_all(project) {
                Ok(report) if report.removed > 0 => state.action(format!(
                    "{} mob(s) saved, {} old file(s) removed.",
                    report.rigs, report.removed
                )),
                Ok(report) => state.action(format!("{} mob(s) saved.", report.rigs)),
                Err(error) => state.error(error),
            }
        }
        if theme::icon_button(ui, "⟲", "Reload from disk").clicked() {
            match library.load_from_project(project) {
                Ok(report) => state.action(format!("{} mob(s) reloaded.", report.rigs)),
                Err(error) => state.error(error),
            }
        }
    });

    if library.current().is_none() {
        theme::empty_state(ui, "🐣", "Click ➕ to create your first mob");
        return;
    }
    match library.current().map(|rig| rig.role) {
        Some(crate::gameplay::mobs::model::RigRole::Player) => theme::hint(
            ui,
            "Player model: the bones Head, Arm L, Arm R, Leg L and Leg R are animated in game (their children follow), the socket Hand holds the item. Customised colours replace each part's colour.",
        ),
        Some(crate::gameplay::mobs::model::RigRole::PlayerHand) => theme::hint(
            ui,
            "First-person hand, in camera space (X right, Y up, -Z forward). The bone Arm is swung by the animations, the socket Hand holds the item.",
        ),
        _ => {}
    }

    // --- Cube hierarchy -----------------------------------------------------------
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let count = library.current().map(|rig| rig.box_count()).unwrap_or(0);
        theme::header(ui, "📦", &format!("Cubes ({count})"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let selected = library.bone_selection.clone();
            let all = library.selected_bones();
            if theme::icon_button(ui, "🗑", "Delete the selected cubes").clicked() && !all.is_empty() {
                if let Some(rig) = library.current_mut() {
                    for name in &all {
                        rig.remove_box(name);
                    }
                }
                library.select_bone(None, false);
                library.clamp_selection();
                library.touch();
            }
            if theme::icon_button(ui, "🗐", "Duplicate the selected cubes").clicked() && !all.is_empty() {
                let mut created = Vec::new();
                if let Some(rig) = library.current_mut() {
                    for name in &all {
                        if let Some(source) = rig.bone(name).cloned() {
                            let index = rig.add_box(source.parent.clone());
                            let new_name = rig.boxes[index].name.clone();
                            rig.boxes[index] = source;
                            rig.boxes[index].name = new_name.clone();
                            rig.boxes[index].offset[0] += 0.25;
                            created.push(new_name);
                        }
                    }
                }
                if !created.is_empty() {
                    library.select_bone(None, false);
                    for (index, name) in created.into_iter().enumerate() {
                        library.select_bone(Some(name), index > 0);
                    }
                    library.touch();
                }
            }
            if theme::icon_button(ui, "➕", "Add a cube (child of the selected cube)").clicked() {
                if let Some(rig) = library.current_mut() {
                    let parent = selected.filter(|name| rig.bone(name).is_some());
                    let index = rig.add_box(parent);
                    let name = rig.boxes[index].name.clone();
                    library.bone_selection = Some(name);
                    library.touch();
                }
            }
        });
    });

    let rows: Vec<(String, usize, [f32; 3], bool, bool)> = {
        let rig = library.current().expect("checked above");
        rig.hierarchy_order()
            .into_iter()
            .map(|index| {
                let bone = &rig.boxes[index];
                let has_children = rig.boxes.iter().any(|b| b.parent.as_deref() == Some(bone.name.as_str()));
                (bone.name.clone(), rig.depth_of(&bone.name), bone.color, bone.visible, has_children)
            })
            .collect()
    };

    egui::ScrollArea::vertical()
        .id_salt("mob_outliner_tree")
        .max_height((ui.available_height() - 150.0).max(120.0))
        .show(ui, |ui| {
            for (name, depth, color, visible, has_children) in rows {
                let selected = library.bone_selection.as_deref() == Some(name.as_str()) || library.bone_extra.contains(&name);
                egui::Frame::NONE
                    .fill(if selected { theme::ACCENT_DIM } else { Color32::TRANSPARENT })
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(4, 2))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(depth as f32 * 14.0);
                            ui.label(RichText::new(if has_children { "🔽" } else { "-" }).color(theme::TEXT_DIM));
                            let eye = if visible { "👁" } else { "👻" };
                            if ui
                                .add(egui::Button::new(eye).frame(false))
                                .on_hover_text("Show / hide")
                                .clicked()
                            {
                                if let Some(bone) = library.current_mut().and_then(|r| r.bone_mut(&name)) {
                                    bone.visible = !bone.visible;
                                }
                                library.touch();
                            }
                            let (swatch, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                            ui.painter().rect_filled(
                                swatch,
                                3.0,
                                Color32::from_rgb((color[0] * 255.0) as u8, (color[1] * 255.0) as u8, (color[2] * 255.0) as u8),
                            );
                            let label = ui.add(
                                egui::Label::new(RichText::new(&name).color(if selected { Color32::WHITE } else { theme::TEXT }))
                                    .sense(Sense::click()),
                            );
                            if label.clicked() {
                                let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
                                library.select_bone(Some(name.clone()), add);
                                library.socket_selection = None;
                            }
                            if label.double_clicked() {
                                studio.frame_request = true;
                            }
                        });
                    });
            }
        });

    // --- Sockets ----------------------------------------------------------------------
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        theme::header(ui, "📌", "Sockets");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::icon_button(ui, "➕", "Add a socket on the selected cube").clicked() {
                if let Some(bone) = library.bone_selection.clone() {
                    if let Some(rig) = library.current_mut() {
                        let index = rig.add_socket(bone);
                        let name = rig.sockets[index].name.clone();
                        library.socket_selection = Some(name);
                        library.touch();
                    }
                } else {
                    state.warn("Select a cube first.");
                }
            }
        });
    });
    let sockets: Vec<(String, String)> = library
        .current()
        .map(|rig| rig.sockets.iter().map(|s| (s.name.clone(), s.bone.clone())).collect())
        .unwrap_or_default();
    if sockets.is_empty() {
        theme::hint(ui, "A hand, a head... to hold weapons and hats.");
    }
    for (name, bone) in sockets {
        let selected = library.socket_selection.as_deref() == Some(name.as_str());
        ui.horizontal(|ui| {
            if ui.selectable_label(selected, format!("📌 {name}")).clicked() {
                library.socket_selection = Some(name.clone());
            }
            ui.label(RichText::new(format!("-> {bone}")).size(10.0).color(theme::TEXT_DIM));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                    if let Some(rig) = library.current_mut() {
                        rig.remove_socket(&name);
                    }
                    library.socket_selection = None;
                    library.touch();
                }
            });
        });
    }

    // --- Health check -------------------------------------------------------------------
    let issues = library.validate();
    let errors = issues.iter().filter(|i| i.severity == MobSeverity::Error).count();
    ui.add_space(4.0);
    if issues.is_empty() {
        ui.label(RichText::new("✔ Mob is valid").color(theme::ACCENT).size(11.0));
    } else {
        ui.collapsing(
            RichText::new(format!("{} {} problem(s)", if errors > 0 { "❌" } else { "⚠" }, issues.len()))
                .color(if errors > 0 { theme::ERROR } else { theme::WARN }),
            |ui| {
                for issue in issues.iter().take(12) {
                    ui.label(RichText::new(format!("{} - {}", issue.location, issue.message)).size(10.0));
                }
            },
        );
    }
}
