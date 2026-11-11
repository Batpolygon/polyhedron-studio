//! Mob and action editor.
//!
//! Left column: rig list, box hierarchy and sockets. Right column: the selected
//! box inspector, socket inspector and the action clip list. The keyframe
//! timeline lives in the dock's Timeline tab and shares the same library.

use egui::{RichText, vec2};

use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::editor::theme;
use crate::gameplay::mobs::library::{MobIssue, MobLibrary, MobSeverity};

pub fn show(
    ui: &mut egui::Ui,
    library: &mut MobLibrary,
    state: &mut EditorUiState,
    project: &ProjectLayout,
) {
    theme::section_label(ui, "MOB & ACTION EDITOR");
    toolbar(ui, library, state, project);
    ui.separator();

    if library.rigs.is_empty() {
        ui.label(
            RichText::new("No mob in the project. Use '+ Mob' to create one.")
                .size(9.5)
                .color(theme::TEXT_DIM),
        );
        return;
    }

    let available = ui.available_size();
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            vec2(198.0_f32.min(available.x * 0.32), available.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("polyhedron_mob_tree_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        rig_list(ui, library, state);
                        ui.separator();
                        box_tree(ui, library, state);
                        ui.separator();
                        socket_list(ui, library, state);
                    });
            },
        );

        ui.separator();

        ui.vertical(|ui| {
            egui::ScrollArea::vertical()
                .id_salt("polyhedron_mob_inspector_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    box_inspector(ui, library, state);
                    ui.separator();
                    socket_inspector(ui, library, state);
                    ui.separator();
                    clip_list(ui, library, state);
                    ui.separator();
                    show_validation(ui, library);
                });
        });
    });
}

fn toolbar(
    ui: &mut egui::Ui,
    library: &mut MobLibrary,
    state: &mut EditorUiState,
    project: &ProjectLayout,
) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("+ Mob")
            .on_hover_text("Create a rig with a root box and an Idle clip.")
            .clicked()
        {
            let index = library.add_rig();
            let name = library.rigs[index].name.clone();
            state.action(format!("Created mob '{name}'."));
        }

        if ui
            .add_enabled(
                !library.rigs.is_empty(),
                egui::Button::new("Copy"),
            )
            .clicked()
        {
            if let Some(name) = library.duplicate_rig(library.selection) {
                state.action(format!("Duplicated mob as '{name}'."));
            }
        }

        if ui
            .add_enabled(!library.rigs.is_empty(), egui::Button::new("Delete"))
            .clicked()
        {
            if let Some(name) = library.remove_rig(library.selection) {
                state.action(format!("Deleted mob '{name}'."));
            }
        }

        ui.separator();

        if ui
            .button("Save")
            .on_hover_text("Write project/entities/mobs.")
            .clicked()
        {
            match library.save_all(project) {
                Ok(report) => {
                    let removed = if report.removed > 0 {
                        format!(", removed {} stale file(s)", report.removed)
                    } else {
                        String::new()
                    };
                    state.action(format!("Saved {} mob(s){removed}.", report.rigs));
                }
                Err(error) => state.error(error),
            }
        }

        if ui
            .button("Reload")
            .on_hover_text("Re-read project/entities/mobs from disk.")
            .clicked()
        {
            match library.load_from_project(project) {
                Ok(report) => {
                    state.action(format!("Reloaded {} mob(s).", report.rigs));
                    for warning in report.warnings {
                        state.warn(warning);
                    }
                }
                Err(error) => state.error(error),
            }
        }

        if ui
            .button("Validate")
            .on_hover_text("Check hierarchy cycles, sockets and keyframes.")
            .clicked()
        {
            report_validation(library, state);
        }

        ui.separator();
        let dirty = if library.is_dirty() {
            RichText::new("UNSAVED").color(theme::WARN)
        } else {
            RichText::new("SAVED").color(theme::ACCENT_DIM)
        };
        ui.label(dirty.size(9.0).strong());
    });
}

fn rig_list(ui: &mut egui::Ui, library: &mut MobLibrary, state: &mut EditorUiState) {
    theme::section_label(ui, "RIGS");
    for index in 0..library.rigs.len() {
        let selected = library.selection == index;
        let name = library.rigs[index].name.clone();
        let boxes = library.rigs[index].box_count();
        let clips = library.rigs[index].clips.len();

        if ui
            .selectable_label(
                selected,
                RichText::new(format!("{name}"))
                    .size(10.0)
                    .color(if selected { theme::ACCENT } else { theme::TEXT }),
            )
            .clicked()
        {
            library.selection = index;
            library.clamp_selection();
            library.preview.time = 0.0;
            library.preview.playing = true;
            state.action(format!("Selected mob '{name}'."));
        }

        ui.label(
            RichText::new(format!("   {boxes} box(es)  •  {clips} clip(s)"))
                .size(8.5)
                .color(theme::TEXT_DIM),
        );
    }
}

fn box_tree(ui: &mut egui::Ui, library: &mut MobLibrary, state: &mut EditorUiState) {
    theme::section_label(ui, "BOXES");

    let selection = library.bone_selection.clone();

    ui.horizontal_wrapped(|ui| {
        if ui.small_button("+ Root").clicked() {
            if let Some(rig) = library.current_mut() {
                let index = rig.add_box(None);
                let name = rig.boxes[index].name.clone();
                library.bone_selection = Some(name.clone());
                state.action(format!("Added root box '{name}'."));
            }
            library.touch();
        }

        let can_add_child = selection.is_some();
        if ui
            .add_enabled(can_add_child, egui::Button::new("+ Child").small())
            .on_hover_text("Add a box parented to the selection.")
            .clicked()
        {
            if let Some(parent) = selection.clone() {
                if let Some(rig) = library.current_mut() {
                    let index = rig.add_box(Some(parent.clone()));
                    let name = rig.boxes[index].name.clone();
                    library.bone_selection = Some(name.clone());
                    state.action(format!("Added box '{name}' under '{parent}'."));
                }
                library.touch();
            }
        }

        let can_delete = selection.is_some();
        if ui
            .add_enabled(can_delete, egui::Button::new("Del").small())
            .clicked()
        {
            if let Some(name) = selection.clone() {
                if let Some(rig) = library.current_mut() {
                    rig.remove_box(&name);
                }
                library.clamp_selection();
                library.touch();
                state.action(format!("Deleted box '{name}'."));
            }
        }
    });

    let rows: Vec<(String, usize, bool)> = library
        .current()
        .map(|rig| {
            rig.hierarchy_order()
                .into_iter()
                .map(|index| {
                    let bone = &rig.boxes[index];
                    let depth = rig.depth_of(&bone.name);
                    let has_children = rig
                        .boxes
                        .iter()
                        .any(|candidate| candidate.parent.as_deref() == Some(&bone.name));
                    (bone.name.clone(), depth, has_children)
                })
                .collect()
        })
        .unwrap_or_default();

    if rows.is_empty() {
        ui.label(RichText::new("No box.").size(9.0).color(theme::TEXT_DIM));
    }

    for (name, depth, has_children) in rows {
        let selected = library.bone_selection.as_deref() == Some(name.as_str());
        let marker = if has_children { "🔽" } else { "•" };
        let label = format!("{}{} {}", "   ".repeat(depth), marker, name);

        if ui
            .selectable_label(
                selected,
                RichText::new(label)
                    .size(9.5)
                    .color(if selected { theme::ACCENT } else { theme::TEXT }),
            )
            .clicked()
        {
            library.bone_selection = Some(name.clone());
            library.socket_selection = None;
        }
    }
}

fn socket_list(ui: &mut egui::Ui, library: &mut MobLibrary, state: &mut EditorUiState) {
    theme::section_label(ui, "SOCKETS");

    let bone = library.bone_selection.clone();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(bone.is_some(), egui::Button::new("+ Socket").small())
            .on_hover_text("Attach an anchor point to the selected box.")
            .clicked()
        {
            if let Some(bone) = bone.clone() {
                if let Some(rig) = library.current_mut() {
                    let index = rig.add_socket(bone.clone());
                    let name = rig.sockets[index].name.clone();
                    library.socket_selection = Some(name.clone());
                    state.action(format!("Added socket '{name}' on '{bone}'."));
                }
                library.touch();
            }
        }

        if ui
            .add_enabled(
                library.socket_selection.is_some(),
                egui::Button::new("Del").small(),
            )
            .clicked()
        {
            if let Some(name) = library.socket_selection.clone() {
                if let Some(rig) = library.current_mut() {
                    rig.remove_socket(&name);
                }
                library.socket_selection = None;
                library.touch();
                state.action(format!("Deleted socket '{name}'."));
            }
        }
    });

    let sockets: Vec<(String, String)> = library
        .current()
        .map(|rig| {
            rig.sockets
                .iter()
                .map(|socket| (socket.name.clone(), socket.bone.clone()))
                .collect()
        })
        .unwrap_or_default();

    if sockets.is_empty() {
        ui.label(
            RichText::new("No socket.")
                .size(9.0)
                .color(theme::TEXT_DIM),
        );
    }

    for (name, bone) in sockets {
        let selected = library.socket_selection.as_deref() == Some(name.as_str());
        if ui
            .selectable_label(
                selected,
                RichText::new(format!("{name}  ->  {bone}"))
                    .size(9.5)
                    .color(if selected { theme::WARN } else { theme::TEXT }),
            )
            .clicked()
        {
            library.socket_selection = Some(name.clone());
            library.bone_selection = Some(bone.clone());
        }
    }
}

fn box_inspector(ui: &mut egui::Ui, library: &mut MobLibrary, state: &mut EditorUiState) {
    theme::section_label(ui, "BOX INSPECTOR");

    let Some(name) = library.bone_selection.clone() else {
        ui.label(
            RichText::new("Select a box in the tree.")
                .size(9.5)
                .color(theme::TEXT_DIM),
        );
        return;
    };

    let Some(rig) = library.current() else {
        return;
    };
    let Some(original) = rig.bone(&name).cloned() else {
        ui.label(
            RichText::new("The selected box no longer exists.")
                .size(9.5)
                .color(theme::TEXT_DIM),
        );
        return;
    };

    let parent_options: Vec<String> = rig
        .boxes
        .iter()
        .filter(|bone| bone.name != name && !rig.is_ancestor(&name, &bone.name))
        .map(|bone| bone.name.clone())
        .collect();

    let mut bone = original.clone();
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label(RichText::new("Name").size(9.0).color(theme::TEXT_DIM));
        changed |= ui
            .add(egui::TextEdit::singleline(&mut bone.name).desired_width(150.0))
            .changed();
        changed |= ui.checkbox(&mut bone.visible, "Visible").changed();
    });

    ui.horizontal(|ui| {
        ui.label(RichText::new("Parent").size(9.0).color(theme::TEXT_DIM));
        let current = bone.parent.clone().unwrap_or_else(|| "- none -".to_string());
        egui::ComboBox::from_id_salt("polyhedron_mob_parent")
            .selected_text(current)
            .width(150.0)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(bone.parent.is_none(), "- none -")
                    .clicked()
                {
                    bone.parent = None;
                    changed = true;
                }
                for candidate in &parent_options {
                    let selected = bone.parent.as_deref() == Some(candidate.as_str());
                    if ui.selectable_label(selected, candidate).clicked() {
                        bone.parent = Some(candidate.clone());
                        changed = true;
                    }
                }
            });
    });

    ui.add_space(4.0);
    changed |= vec3_row(ui, "Size", &mut bone.size, 0.01, 0.01..=16.0);
    changed |= vec3_row(ui, "Pivot", &mut bone.pivot, 0.01, -16.0..=16.0);
    changed |= vec3_row(ui, "Offset", &mut bone.offset, 0.01, -16.0..=16.0);
    changed |= vec3_row(ui, "Rotation", &mut bone.rotation, 1.0, -360.0..=360.0);

    if let Some((pose, time)) = crate::gameplay::mobs::preview_pose(library) {
        let sampled = pose.get(&name);
        ui.label(
            RichText::new(format!(
                "POSE @ {time:.2}s   t ({:.2}, {:.2}, {:.2})   r ({:.1}, {:.1}, {:.1})   s ({:.2}, {:.2}, {:.2})",
                sampled.translation.x,
                sampled.translation.y,
                sampled.translation.z,
                sampled.rotation.x,
                sampled.rotation.y,
                sampled.rotation.z,
                sampled.scale.x,
                sampled.scale.y,
                sampled.scale.z
            ))
            .monospace()
            .size(8.5)
            .color(theme::TEXT_DIM),
        );
    }

    ui.horizontal(|ui| {
        ui.label(RichText::new("Colour").size(9.0).color(theme::TEXT_DIM));
        changed |= ui.color_edit_button_rgb(&mut bone.color).changed();
        ui.label(
            RichText::new(format!(
                "#{:02X}{:02X}{:02X}",
                (bone.color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                (bone.color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                (bone.color[2].clamp(0.0, 1.0) * 255.0).round() as u8
            ))
            .monospace()
            .size(8.5)
            .color(theme::TEXT_DIM),
        );
    });

    ui.horizontal(|ui| {
        if ui.small_button("Reset transform").clicked() {
            bone.offset = [0.0; 3];
            bone.rotation = [0.0; 3];
            bone.pivot = [0.0; 3];
            changed = true;
        }
    });

    if !changed {
        return;
    }

    let Some(rig) = library.current_mut() else {
        return;
    };

    let mut target_name = name.clone();
    let renamed = bone.name.trim().to_string();
    if renamed != name {
        match rig.rename_box(&name, &renamed) {
            Ok(()) => target_name = renamed,
            Err(error) => {
                state.error(error);
                bone.name = name.clone();
            }
        }
    }

    if bone.parent != original.parent {
        if let Err(error) = rig.reparent(&target_name, bone.parent.clone()) {
            state.error(error);
            bone.parent = original.parent.clone();
        }
    }

    if let Some(target) = rig.bone_mut(&target_name) {
        let parent = target.parent.clone();
        *target = bone;
        target.parent = parent;
    }

    library.bone_selection = Some(target_name);
    library.touch();
}

fn socket_inspector(ui: &mut egui::Ui, library: &mut MobLibrary, state: &mut EditorUiState) {
    theme::section_label(ui, "SOCKET INSPECTOR");

    let Some(name) = library.socket_selection.clone() else {
        ui.label(
            RichText::new("Select a socket to edit its anchor.")
                .size(9.5)
                .color(theme::TEXT_DIM),
        );
        return;
    };

    let bones: Vec<String> = library
        .current()
        .map(|rig| rig.boxes.iter().map(|bone| bone.name.clone()).collect())
        .unwrap_or_default();

    let Some(rig) = library.current_mut() else {
        return;
    };
    let Some(index) = rig.sockets.iter().position(|socket| socket.name == name) else {
        return;
    };

    let mut changed = false;
    {
        let socket = &mut rig.sockets[index];
        ui.horizontal(|ui| {
            ui.label(RichText::new("Name").size(9.0).color(theme::TEXT_DIM));
            changed |= ui
                .add(egui::TextEdit::singleline(&mut socket.name).desired_width(150.0))
                .changed();
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Box").size(9.0).color(theme::TEXT_DIM));
            egui::ComboBox::from_id_salt("polyhedron_mob_socket_bone")
                .selected_text(socket.bone.clone())
                .width(150.0)
                .show_ui(ui, |ui| {
                    for candidate in &bones {
                        if ui
                            .selectable_value(
                                &mut socket.bone,
                                candidate.clone(),
                                candidate.as_str(),
                            )
                            .clicked()
                        {
                            changed = true;
                        }
                    }
                });
        });

        changed |= vec3_row(ui, "Offset", &mut socket.offset, 0.01, -16.0..=16.0);
        changed |= vec3_row(ui, "Rotation", &mut socket.rotation, 1.0, -360.0..=360.0);
    }

    if changed {
        library.socket_selection = Some(rig.sockets[index].name.clone());
        library.touch();
        state.action("Socket updated.");
    }
}

fn clip_list(ui: &mut egui::Ui, library: &mut MobLibrary, state: &mut EditorUiState) {
    theme::section_label(ui, "ACTION CLIPS");

    let clips: Vec<(String, f32, bool, usize, usize)> = library
        .current()
        .map(|rig| {
            rig.clips
                .iter()
                .map(|clip| {
                    (
                        clip.name.clone(),
                        clip.duration_or(1.0),
                        clip.looping,
                        clip.tracks.len(),
                        clip.key_count(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let default_clip = library
        .current()
        .map(|rig| rig.default_clip.clone())
        .unwrap_or_default();

    ui.horizontal_wrapped(|ui| {
        if ui.small_button("+ Clip").clicked() {
            if let Some(name) = library.add_clip() {
                state.action(format!("Created clip '{name}'."));
            }
        }

        if ui
            .add_enabled(
                !library.clip_selection.is_empty(),
                egui::Button::new("Copy").small(),
            )
            .clicked()
        {
            let name = library.clip_selection.clone();
            if let Some(rig) = library.current_mut() {
                rig.duplicate_clip(&name);
            }
            library.touch();
            state.action(format!("Duplicated clip '{name}'."));
        }

        if ui
            .add_enabled(
                !library.clip_selection.is_empty(),
                egui::Button::new("Del").small(),
            )
            .clicked()
        {
            let name = library.clip_selection.clone();
            if let Some(rig) = library.current_mut() {
                rig.remove_clip(&name);
            }
            library.clamp_selection();
            library.touch();
            state.action(format!("Deleted clip '{name}'."));
        }
    });

    if clips.is_empty() {
        ui.label(RichText::new("No clip.").size(9.0).color(theme::TEXT_DIM));
    }

    for (name, duration, looping, tracks, keys) in clips {
        let selected = library.clip_selection == name;
        ui.horizontal(|ui| {
            if ui
                .selectable_label(
                    selected,
                    RichText::new(format!(
                        "{name}   {duration:.2}s   {tracks} track(s)   {keys} key(s)"
                    ))
                    .size(9.5)
                    .color(if selected { theme::ACCENT } else { theme::TEXT }),
                )
                .clicked()
            {
                library.clip_selection = name.clone();
                library.preview.time = 0.0;
                library.preview.playing = true;
                library.key_bone = None;
                library.key_time = None;
            }

            if default_clip == name {
                ui.label(
                    RichText::new("DEFAULT")
                        .size(8.0)
                        .strong()
                        .color(theme::ACCENT_DIM),
                );
            } else if ui
                .small_button("default")
                .on_hover_text("Use this clip when the mob plays in game.")
                .clicked()
            {
                if let Some(rig) = library.current_mut() {
                    rig.default_clip = name.clone();
                }
                library.touch();
            }

            if !looping {
                ui.label(
                    RichText::new("ONCE")
                        .size(8.0)
                        .color(theme::TEXT_DIM),
                );
            }
        });
    }
}

fn show_validation(ui: &mut egui::Ui, library: &MobLibrary) {
    let issues = library.validate();
    let errors = issues
        .iter()
        .filter(|issue| issue.severity == MobSeverity::Error)
        .count();
    let warnings = issues.len() - errors;

    egui::CollapsingHeader::new(
        RichText::new(format!(
            "VALIDATION  •  {errors} error(s)  •  {warnings} warning(s)"
        ))
        .size(9.5)
        .color(if errors > 0 { theme::ERROR } else { theme::TEXT_DIM }),
    )
    .id_salt("polyhedron_mob_validation")
    .default_open(errors > 0)
    .show(ui, |ui| {
        show_issue_rows(ui, &issues);
    });
}

fn show_issue_rows(ui: &mut egui::Ui, issues: &[MobIssue]) {
    if issues.is_empty() {
        ui.label(
            RichText::new("No issues. The rig is ready to animate.")
                .size(9.0)
                .color(theme::ACCENT_DIM),
        );
        return;
    }

    for issue in issues.iter().take(40) {
        let color = match issue.severity {
            MobSeverity::Error => theme::ERROR,
            MobSeverity::Warning => theme::WARN,
        };
        ui.label(
            RichText::new(format!("{}  {}", issue.severity.label(), issue.message))
                .size(9.0)
                .color(color),
        );
        ui.label(
            RichText::new(&issue.location)
                .size(8.0)
                .color(theme::TEXT_DIM),
        );
    }

    if issues.len() > 40 {
        ui.label(
            RichText::new(format!("... and {} more issue(s).", issues.len() - 40))
                .size(8.5)
                .color(theme::TEXT_DIM),
        );
    }
}

fn report_validation(library: &MobLibrary, state: &mut EditorUiState) {
    let issues = library.validate();
    let errors = issues
        .iter()
        .filter(|issue| issue.severity == MobSeverity::Error)
        .count();
    let warnings = issues.len() - errors;

    if issues.is_empty() {
        state.action("Mob validation: no issues.");
        return;
    }

    state.warn(format!(
        "Mob validation: {errors} error(s), {warnings} warning(s)."
    ));
    for issue in issues.iter().take(10) {
        match issue.severity {
            MobSeverity::Error => {
                state.error(format!("{} - {}", issue.location, issue.message));
            }
            MobSeverity::Warning => {
                state.warn(format!("{} - {}", issue.location, issue.message));
            }
        }
    }
}

fn vec3_row(
    ui: &mut egui::Ui,
    label: &str,
    values: &mut [f32; 3],
    speed: f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(9.0).color(theme::TEXT_DIM));
        for value in values.iter_mut() {
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .range(range.clone())
                        .speed(speed)
                        .fixed_decimals(2),
                )
                .changed();
        }
    });
    changed
}
