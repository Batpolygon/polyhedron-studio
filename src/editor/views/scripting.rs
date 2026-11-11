//! Blockly-style visual scripting panel.
//!
//! The left column lists programs and actions; the right column edits the
//! selected item as a recursive block list. Every edit bumps the library
//! revision so the VM restarts on the next tick with the new graph.

use egui::{Color32, RichText, Stroke, vec2};

use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::editor::pickers::{self, Kind};
use crate::editor::theme;
use crate::scripting::library::{program_templates, ScriptLibrary, ScriptSelection};
use crate::scripting::model::{
    ActionDef, BlockCategory, CompareOp, EventHandler, GameEvent, IssueSeverity, MathOp, Operand,
    PlayerStat, Question, ScriptBlock, ScriptProgram, ScriptTarget, Sensor, ValidationIssue,
};
use crate::scripting::vm::{ProgramStatus, ProgramStatusView, ScriptRuntime};

pub fn show(
    ui: &mut egui::Ui,
    library: &mut ScriptLibrary,
    runtime: &mut ScriptRuntime,
    state: &mut EditorUiState,
    project: &ProjectLayout,
) {
    theme::section_label(ui, "VISUAL SCRIPTING");
    show_toolbar(ui, library, runtime, state, project);
    ui.separator();

    let available = ui.available_size();
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            vec2(190.0_f32.min(available.x * 0.3), available.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("polyhedron_script_library_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        show_library(ui, library, runtime, state);
                    });
            },
        );

        ui.separator();

        ui.vertical(|ui| {
            egui::ScrollArea::vertical()
                .id_salt("polyhedron_script_editor_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    show_editor(ui, library, runtime);
                });
        });
    });
}

fn show_toolbar(
    ui: &mut egui::Ui,
    library: &mut ScriptLibrary,
    runtime: &mut ScriptRuntime,
    state: &mut EditorUiState,
    project: &ProjectLayout,
) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("+ Program")
            .on_hover_text("Create a new On Start / On Tick program.")
            .clicked()
        {
            let index = library.add_program();
            let name = library.programs[index].name.clone();
            state.action(format!("Created script program '{name}'."));
        }

        if ui
            .button("+ Action")
            .on_hover_text("Create a reusable action callable from programs.")
            .clicked()
        {
            let index = library.add_action();
            let name = library.actions[index].name.clone();
            state.action(format!("Created script action '{name}'."));
        }

        if ui
            .button("Save all")
            .on_hover_text("Write project/scripts and project/actions.")
            .clicked()
        {
            match library.save_all(project) {
                Ok(report) => {
                    let removed = if report.removed > 0 {
                        format!(", removed {} stale file(s)", report.removed)
                    } else {
                        String::new()
                    };
                    state.action(format!(
                        "Saved {} program(s) and {} action(s){removed}.",
                        report.programs, report.actions
                    ));
                }
                Err(error) => state.error(error),
            }
        }

        if ui
            .button("Reload")
            .on_hover_text("Re-read project/scripts and project/actions from disk.")
            .clicked()
        {
            match library.load_from_project(project) {
                Ok(report) => {
                    state.action(format!(
                        "Reloaded {} program(s) and {} action(s).",
                        report.programs, report.actions
                    ));
                    for warning in report.warnings {
                        state.warn(warning);
                    }
                }
                Err(error) => state.error(error),
            }
        }

        if ui
            .button("Validate")
            .on_hover_text("Report unknown actions, recursion and unsafe values.")
            .clicked()
        {
            let issues = library.validate();
            let errors = issues
                .iter()
                .filter(|issue| issue.severity == IssueSeverity::Error)
                .count();
            let warnings = issues.len() - errors;
            if errors == 0 && warnings == 0 {
                state.action("Script validation: no issues.");
            } else {
                state.warn(format!(
                    "Script validation: {errors} error(s), {warnings} warning(s)."
                ));
                for issue in issues.iter().take(6) {
                    match issue.severity {
                        IssueSeverity::Error => {
                            state.error(format!("{} - {}", issue.location, issue.message));
                        }
                        IssueSeverity::Warning => {
                            state.warn(format!("{} - {}", issue.location, issue.message));
                        }
                    }
                }
            }
        }

        ui.separator();

        if ui
            .add_enabled(!runtime.is_running(), egui::Button::new("Run"))
            .on_hover_text("Run every enabled program outside PIE.")
            .clicked()
        {
            runtime.start(library, false);
            state.action(format!(
                "Script VM started with {} program(s).",
                runtime.program_count()
            ));
        }

        if ui
            .add_enabled(runtime.is_running(), egui::Button::new("Stop"))
            .on_hover_text("Halt the VM and clear its variables.")
            .clicked()
        {
            runtime.stop();
            state.action("Script VM stopped.");
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

fn show_library(
    ui: &mut egui::Ui,
    library: &mut ScriptLibrary,
    runtime: &mut ScriptRuntime,
    state: &mut EditorUiState,
) {
    let statuses = runtime.statuses();

    theme::section_label(ui, "PROGRAMS");
    ui.horizontal(|ui| {
        if ui.small_button("New").clicked() {
            let index = library.add_program();
            let name = library.programs[index].name.clone();
            state.action(format!("Created script program '{name}'."));
        }
        ui.menu_button("Template", |ui| {
            for template in program_templates() {
                let description = template.description.clone();
                if ui.button(template.name.as_str()).on_hover_text(description).clicked() {
                    let index = library.add_template(template);
                    let name = library.programs[index].name.clone();
                    state.action(format!("Created '{name}' from a template."));
                    ui.close();
                }
            }
        });
        if ui.small_button("Copy").clicked() {
            if let ScriptSelection::Program(index) = library.selection {
                if let Some(name) = library.duplicate_program(index) {
                    state.action(format!("Duplicated program as '{name}'."));
                }
            }
        }
        if ui.small_button("Delete").clicked() {
            if let ScriptSelection::Program(index) = library.selection {
                if let Some(name) = library.remove_program(index) {
                    state.action(format!("Deleted program '{name}'."));
                }
            }
        }
    });

    if library.programs.is_empty() {
        ui.label(RichText::new("No programs.").size(9.5).color(theme::TEXT_DIM));
    }

    let filter_id = ui.id().with("program_search");
    let mut filter: String = ui.ctx().data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
    ui.add(egui::TextEdit::singleline(&mut filter).hint_text("🔍 search").desired_width(150.0));
    ui.ctx().data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    let needle = filter.to_lowercase();

    for index in 0..library.programs.len() {
        if !needle.is_empty() && !library.programs[index].name.to_lowercase().contains(&needle) {
            continue;
        }
        let selected = library.selection == ScriptSelection::Program(index);
        let name = library.programs[index].name.clone();
        let enabled = library.programs[index].enabled;
        let status = statuses.iter().find(|status| status.name == name);
        let marker = if enabled { "*" } else { "-" };
        let response = ui.selectable_label(
            selected,
            RichText::new(format!("{marker} {name}"))
                .size(10.0)
                .color(status_color(status.map(|status| status.status))),
        );
        if response.clicked() {
            library.selection = ScriptSelection::Program(index);
        }
        if let Some(status) = status {
            ui.label(
                RichText::new(format!("   {}  •  {} steps", status.status.label(), status.steps))
                    .size(8.5)
                    .color(theme::TEXT_DIM),
            );
        }
    }

    ui.separator();
    theme::section_label(ui, "ACTIONS");
    ui.horizontal(|ui| {
        if ui.small_button("New").clicked() {
            let index = library.add_action();
            let name = library.actions[index].name.clone();
            state.action(format!("Created script action '{name}'."));
        }
        if ui.small_button("Copy").clicked() {
            if let ScriptSelection::Action(index) = library.selection {
                if let Some(name) = library.duplicate_action(index) {
                    state.action(format!("Duplicated action as '{name}'."));
                }
            }
        }
        if ui.small_button("Delete").clicked() {
            if let ScriptSelection::Action(index) = library.selection {
                if let Some(name) = library.remove_action(index) {
                    state.action(format!("Deleted action '{name}'."));
                }
            }
        }
    });

    if library.actions.is_empty() {
        ui.label(RichText::new("No actions.").size(9.5).color(theme::TEXT_DIM));
    }

    for index in 0..library.actions.len() {
        let selected = library.selection == ScriptSelection::Action(index);
        let name = library.actions[index].name.clone();
        let blocks = library.actions[index].blocks.len();
        let response = ui.selectable_label(
            selected,
            RichText::new(format!("{name}  ({blocks})"))
                .size(10.0)
                .color(theme::TEXT),
        );
        if response.clicked() {
            library.selection = ScriptSelection::Action(index);
        }
    }

    ui.separator();
    show_runtime_summary(ui, runtime);
}

fn show_runtime_summary(ui: &mut egui::Ui, runtime: &mut ScriptRuntime) {
    theme::section_label(ui, "VM STATE");
    let (label, color) = if runtime.is_running() {
        ("RUNNING", theme::ACCENT)
    } else {
        ("STOPPED", theme::TEXT_DIM)
    };
    ui.label(RichText::new(label).size(10.0).strong().color(color));
    ui.horizontal(|ui| {
        if runtime.is_paused() {
            if ui.small_button("▶ Resume").clicked() {
                runtime.set_paused(false);
            }
            if ui.small_button("⏭ Step").on_hover_text("Run one tick").clicked() {
                runtime.step();
            }
        } else if ui.small_button("⏸ Pause").clicked() {
            runtime.set_paused(true);
        }
    });
    ui.label(
        RichText::new(format!(
            "tick {}  •  {} instr  •  {} signal(s)",
            runtime.tick_count(),
            runtime.instructions(),
            runtime.signal_count()
        ))
        .size(8.5)
        .color(theme::TEXT_DIM),
    );

    let (energy, capacity) = runtime.energy();
    ui.label(
        RichText::new(format!("energy {energy:.1} / {capacity:.1}"))
            .size(8.5)
            .color(theme::TEXT_DIM),
    );

    if let Some(error) = runtime.last_error() {
        ui.label(RichText::new(error).size(8.5).color(theme::ERROR));
    }

    if !runtime.variables().is_empty() {
        egui::CollapsingHeader::new(RichText::new("Variables").size(9.0))
            .id_salt("polyhedron_script_variables")
            .default_open(false)
            .show(ui, |ui| {
                for (name, value) in runtime.variables() {
                    ui.label(
                        RichText::new(format!("{name} = {value:.3}"))
                            .monospace()
                            .size(8.5)
                            .color(theme::TEXT),
                    );
                }
            });
    }

    let instances = runtime.instance_variables();
    if !instances.is_empty() {
        egui::CollapsingHeader::new(RichText::new("Instances").size(9.0))
            .id_salt("polyhedron_script_instances")
            .default_open(false)
            .show(ui, |ui| {
                for (name, variables) in instances {
                    ui.label(RichText::new(name).size(8.5).color(theme::ACCENT_DIM));
                    for (variable, value) in variables {
                        ui.label(RichText::new(format!("  {variable} = {value:.3}")).monospace().size(8.5).color(theme::TEXT));
                    }
                }
            });
    }
    if !runtime.lists().is_empty() {
        egui::CollapsingHeader::new(RichText::new("Lists").size(9.0))
            .id_salt("polyhedron_script_lists")
            .default_open(false)
            .show(ui, |ui| {
                for (name, list) in runtime.lists() {
                    ui.label(RichText::new(format!("{name}: {list:?}")).monospace().size(8.5).color(theme::TEXT));
                }
            });
    }
}

fn show_editor(ui: &mut egui::Ui, library: &mut ScriptLibrary, runtime: &ScriptRuntime) {
    let action_names = library.action_names();
    let statuses = runtime.statuses();
    let mut changed = false;

    match library.selection {
        ScriptSelection::Program(index) => {
            if let Some(program) = library.programs.get_mut(index) {
                changed = show_program_editor(ui, program, &action_names, &statuses);
            } else {
                ui.label(
                    RichText::new("No program selected. Create one with '+ Program'.")
                        .size(10.0)
                        .color(theme::TEXT_DIM),
                );
            }
        }
        ScriptSelection::Action(index) => {
            if let Some(action) = library.actions.get_mut(index) {
                changed = show_action_editor(ui, action, &action_names);
            } else {
                ui.label(
                    RichText::new("No action selected. Create one with '+ Action'.")
                        .size(10.0)
                        .color(theme::TEXT_DIM),
                );
            }
        }
    }

    if changed {
        library.touch();
    }

    ui.separator();
    let issues = library.validate();
    show_validation(ui, &issues);
}

fn show_program_editor(
    ui: &mut egui::Ui,
    program: &mut ScriptProgram,
    action_names: &[String],
    statuses: &[ProgramStatusView],
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label(RichText::new("Name").size(9.5).color(theme::TEXT_DIM));
        changed |= ui
            .add(egui::TextEdit::singleline(&mut program.name).desired_width(180.0))
            .changed();
        changed |= ui.checkbox(&mut program.enabled, "Enabled").changed();
    });

    ui.horizontal(|ui| {
        ui.label(RichText::new("Description").size(9.5).color(theme::TEXT_DIM));
        changed |= ui
            .add(egui::TextEdit::singleline(&mut program.description))
            .changed();
    });

    if let Some(status) = statuses.iter().find(|status| status.name == program.name) {
        ui.label(
            RichText::new(format!(
                "VM: {}  •  {} steps{}",
                status.status.label(),
                status.steps,
                status
                    .error
                    .as_deref()
                    .map(|error| format!("  •  {error}"))
                    .unwrap_or_default()
            ))
            .size(9.0)
            .color(status_color(Some(status.status))),
        );
    }

    ui.separator();
    theme::section_label(ui, "ON START");
    changed |= show_block_list(ui, &mut program.on_start, action_names, "start", 0);

    ui.separator();
    theme::section_label(ui, "ON TICK");
    changed |= show_block_list(ui, &mut program.on_tick, action_names, "tick", 0);

    ui.separator();
    ui.horizontal(|ui| {
        changed |= ui
            .checkbox(&mut program.bound, "Bound")
            .on_hover_text(
                "Bound programs run once per block / entity that references them \
                 (block `script` field, mob `script`), with local variables. \
                 Prefix a variable with `g.` to share it.",
            )
            .changed();
    });

    let handlers: [(&str, &str); 4] = [
        ("ON INTERACT", "interact"),
        ("ON HIT  (local: damage)", "hit"),
        ("ON SIGNAL  (locals: signal, port_<name>)", "signal"),
        ("ON ENERGY  (local: powered)", "energy"),
    ];
    for (title, key) in handlers {
        let blocks = match key {
            "interact" => &mut program.on_interact,
            "hit" => &mut program.on_hit,
            "signal" => &mut program.on_signal,
            _ => &mut program.on_energy,
        };
        let count = blocks.len();
        egui::CollapsingHeader::new(
            RichText::new(format!("{title}  •  {count} block(s)"))
                .size(9.5)
                .color(theme::ACCENT_DIM),
        )
        .id_salt(format!("handler_{key}"))
        .default_open(count > 0)
        .show(ui, |ui| {
            if key == "signal" {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("port filter").size(9.0).color(theme::TEXT_DIM));
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut program.signal_port)
                                .hint_text("any")
                                .desired_width(100.0),
                        )
                        .changed();
                });
            }
            let blocks = match key {
                "interact" => &mut program.on_interact,
                "hit" => &mut program.on_hit,
                "signal" => &mut program.on_signal,
                _ => &mut program.on_energy,
            };
            changed |= show_block_list(ui, blocks, action_names, key, 0);
        });
    }

    ui.separator();
    ui.horizontal(|ui| {
        theme::section_label(ui, "ON GAME EVENT");
        ui.menu_button("+ Add event", |ui| {
            for event in GameEvent::ALL {
                if ui.button(event.label()).clicked() {
                    program.events.push(EventHandler { event, ..EventHandler::default() });
                    changed = true;
                    ui.close();
                }
            }
        });
    });
    theme::tip(ui, "Locals: event_value, event_x, event_y, event_z");
    let mut remove = None;
    for (index, handler) in program.events.iter_mut().enumerate() {
        let salt = format!("event_{index}");
        ui.push_id(&salt, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(handler.event.label()).size(9.5).strong().color(theme::ACCENT_DIM));
                let hint = handler.event.key_hint();
                if !hint.is_empty() {
                    let kinds: &[Kind] = match handler.event {
                        GameEvent::MobDied | GameEvent::MobKilled => &[Kind::Mob],
                        GameEvent::BlockPlaced | GameEvent::BlockBroken => &[Kind::Block],
                        GameEvent::ItemUsed => &[Kind::Item],
                        GameEvent::ItemCollected | GameEvent::Crafted => &[Kind::Resource, Kind::Item, Kind::Block],
                        _ => &[],
                    };
                    if kinds.is_empty() {
                        changed |= ui.add(egui::TextEdit::singleline(&mut handler.filter).hint_text(hint).desired_width(110.0)).changed();
                    } else {
                        changed |= pickers::pick(ui, &salt, &mut handler.filter, kinds, Some("any"));
                    }
                }
                if ui.small_button("Del").clicked() {
                    remove = Some(index);
                }
            });
            ui.indent(&salt, |ui| {
                changed |= show_block_list(ui, &mut handler.blocks, action_names, &salt, 0);
            });
        });
    }
    if let Some(index) = remove {
        program.events.remove(index);
        changed = true;
    }

    changed
}

fn show_action_editor(
    ui: &mut egui::Ui,
    action: &mut ActionDef,
    action_names: &[String],
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label(RichText::new("Name").size(9.5).color(theme::TEXT_DIM));
        changed |= ui
            .add(egui::TextEdit::singleline(&mut action.name).desired_width(180.0))
            .changed();
    });

    ui.horizontal(|ui| {
        ui.label(RichText::new("Description").size(9.5).color(theme::TEXT_DIM));
        changed |= ui
            .add(egui::TextEdit::singleline(&mut action.description))
            .changed();
    });

    ui.separator();
    theme::section_label(ui, "ACTION BLOCKS");
    changed |= show_block_list(ui, &mut action.blocks, action_names, "action", 0);

    changed
}

fn show_block_list(
    ui: &mut egui::Ui,
    blocks: &mut Vec<ScriptBlock>,
    action_names: &[String],
    path: &str,
    depth: usize,
) -> bool {
    let mut changed = false;
    let mut remove = None;
    let mut move_up = None;
    let mut move_down = None;

    if blocks.is_empty() {
        ui.label(
            RichText::new("(empty)")
                .size(9.0)
                .italics()
                .color(theme::TEXT_DIM),
        );
    }

    for index in 0..blocks.len() {
        let child_path = format!("{path}/{index}");
        ui.push_id(&child_path, |ui| {
            egui::Frame::NONE
                .fill(theme::BG_ELEVATED)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(3)
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(blocks[index].kind_label())
                                .size(10.0)
                                .strong()
                                .color(category_color(blocks[index].category())),
                        )
                        .on_hover_text(blocks[index].help());
                        ui.label(
                            RichText::new(blocks[index].summary())
                                .size(8.5)
                                .color(theme::TEXT_DIM),
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui.small_button("Del").clicked() {
                                    remove = Some(index);
                                }
                                if ui.small_button("Down").clicked() {
                                    move_down = Some(index);
                                }
                                if ui.small_button("Up").clicked() {
                                    move_up = Some(index);
                                }
                            },
                        );
                    });

                    changed |= show_block_fields(ui, &mut blocks[index], action_names, &child_path, depth);
                });
        });
    }

    if let Some(index) = remove {
        blocks.remove(index);
        changed = true;
    } else if let Some(index) = move_up {
        if index > 0 {
            blocks.swap(index, index - 1);
            changed = true;
        }
    } else if let Some(index) = move_down {
        if index + 1 < blocks.len() {
            blocks.swap(index, index + 1);
            changed = true;
        }
    }

    if add_block_menu(ui, blocks, action_names) {
        changed = true;
    }

    changed
}

fn show_block_fields(
    ui: &mut egui::Ui,
    block: &mut ScriptBlock,
    action_names: &[String],
    path: &str,
    depth: usize,
) -> bool {
    let mut changed = false;

    match block {
        ScriptBlock::Move { distance } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("distance").size(9.0).color(theme::TEXT_DIM));
                changed |= ui
                    .add(
                        egui::DragValue::new(distance)
                            .range(-64.0..=64.0)
                            .speed(0.1)
                            .fixed_decimals(2)
                            .suffix(" m"),
                    )
                    .changed();
            });
        }
        ScriptBlock::Rotate { degrees } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("degrees").size(9.0).color(theme::TEXT_DIM));
                changed |= ui
                    .add(
                        egui::DragValue::new(degrees)
                            .range(-360.0..=360.0)
                            .speed(1.0)
                            .fixed_decimals(1)
                            .suffix(" deg"),
                    )
                    .changed();
            });
        }
        ScriptBlock::Wait { seconds } => {
            ui.horizontal(|ui| changed |= labeled_operand(ui, "seconds", seconds));
        }
        ScriptBlock::Emit { signal } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("signal").size(9.0).color(theme::TEXT_DIM));
                changed |= ui.text_edit_singleline(signal).changed();
            });
        }
        ScriptBlock::Log { message } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("message").size(9.0).color(theme::TEXT_DIM));
                changed |= ui.text_edit_singleline(message).changed();
            });
        }
        ScriptBlock::SetVar { name, value } | ScriptBlock::AddVar { name, value } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("variable").size(9.0).color(theme::TEXT_DIM));
                changed |= ui
                    .add(egui::TextEdit::singleline(name).desired_width(110.0))
                    .changed();
                changed |= labeled_operand(ui, "value", value);
            });
        }
        ScriptBlock::AddEnergy { amount } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("amount").size(9.0).color(theme::TEXT_DIM));
                changed |= ui
                    .add(
                        egui::DragValue::new(amount)
                            .range(-1_000.0..=1_000.0)
                            .speed(0.5)
                            .fixed_decimals(1),
                    )
                    .changed();
            });
        }
        ScriptBlock::Repeat { times, body } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("times").size(9.0).color(theme::TEXT_DIM));
                changed |= ui
                    .add(
                        egui::DragValue::new(times)
                            .range(0..=10_000)
                            .speed(1.0),
                    )
                    .changed();
            });
            ui.indent(format!("{path}/repeat"), |ui| {
                changed |= show_block_list(ui, body, action_names, &format!("{path}/repeat"), depth + 1);
            });
        }
        ScriptBlock::If {
            variable,
            op,
            value,
            then_body,
            else_body,
        } => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("if").size(9.0).color(theme::TEXT_DIM));
                changed |= ui
                    .add(egui::TextEdit::singleline(variable).desired_width(100.0))
                    .changed();
                compare_op_combo(ui, &format!("{path}/if_op"), op, &mut changed);
                changed |= operand_field(ui, value);
            });
            ui.indent(format!("{path}/then"), |ui| {
                ui.label(RichText::new("then").size(8.5).color(theme::ACCENT_DIM));
                changed |= show_block_list(ui, then_body, action_names, &format!("{path}/then"), depth + 1);
            });
            ui.indent(format!("{path}/else"), |ui| {
                ui.label(RichText::new("else").size(8.5).color(theme::TEXT_DIM));
                changed |= show_block_list(ui, else_body, action_names, &format!("{path}/else"), depth + 1);
            });
        }
        ScriptBlock::While {
            variable,
            op,
            value,
            max_iterations,
            body,
        } => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("while").size(9.0).color(theme::TEXT_DIM));
                changed |= ui
                    .add(egui::TextEdit::singleline(variable).desired_width(100.0))
                    .changed();
                compare_op_combo(ui, &format!("{path}/while_op"), op, &mut changed);
                changed |= operand_field(ui, value);
                ui.label(RichText::new("max").size(9.0).color(theme::TEXT_DIM));
                changed |= ui
                    .add(
                        egui::DragValue::new(max_iterations)
                            .range(1..=10_000)
                            .speed(1.0),
                    )
                    .changed();
            });
            ui.indent(format!("{path}/while"), |ui| {
                changed |= show_block_list(ui, body, action_names, &format!("{path}/while"), depth + 1);
            });
        }
        ScriptBlock::CallAction { action, args } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("action").size(9.0).color(theme::TEXT_DIM));
                if action_names.is_empty() {
                    changed |= ui
                        .add(egui::TextEdit::singleline(action).desired_width(140.0))
                        .changed();
                } else {
                    egui::ComboBox::from_id_salt(format!("{path}/action"))
                        .selected_text(action.as_str())
                        .width(160.0)
                        .show_ui(ui, |ui| {
                            for name in action_names {
                                if ui
                                    .selectable_value(action, name.clone(), name.as_str())
                                    .clicked()
                                {
                                    changed = true;
                                }
                            }
                        });
                }
                if ui.small_button("+ parameter").on_hover_text("A variable set right before the call").clicked() {
                    args.push(("param".to_string(), 0.0.into()));
                    changed = true;
                }
            });
            let mut remove = None;
            for (index, (name, value)) in args.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::TextEdit::singleline(name).desired_width(80.0)).changed();
                    ui.label("=");
                    changed |= operand_field(ui, value);
                    if ui.small_button("🗙").clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                args.remove(index);
                changed = true;
            }
        }
        ScriptBlock::Math { name, op, value } => {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(name).desired_width(80.0)).changed();
                math_op_combo(ui, &format!("{path}/mathop"), op, &mut changed);
                changed |= operand_field(ui, value);
            });
        }
        ScriptBlock::Compute { name, left, op, right } => {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(name).desired_width(80.0)).changed();
                ui.label("=");
                changed |= operand_field(ui, left);
                math_op_combo(ui, &format!("{path}/computeop"), op, &mut changed);
                changed |= operand_field(ui, right);
            });
        }
        ScriptBlock::RandomRange { name, min, max } => {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(name).desired_width(80.0)).changed();
                changed |= labeled_operand(ui, "from", min);
                changed |= labeled_operand(ui, "to", max);
            });
        }
        ScriptBlock::Ask { name, question } => {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(name).desired_width(80.0)).changed();
                ui.label(RichText::new("<-").size(9.0).color(theme::TEXT_DIM));
                question_fields(ui, path, question, &mut changed);
            });
        }
        ScriptBlock::ListPush { list, value } => {
            ui.horizontal(|ui| {
                changed |= name_field(ui, "list", list);
                changed |= labeled_operand(ui, "value", value);
            });
        }
        ScriptBlock::ListGet { name, list, index } => {
            ui.horizontal(|ui| {
                changed |= name_field(ui, "into", name);
                changed |= name_field(ui, "list", list);
                changed |= labeled_operand(ui, "index", index);
            });
        }
        ScriptBlock::ListSet { list, index, value } => {
            ui.horizontal(|ui| {
                changed |= name_field(ui, "list", list);
                changed |= labeled_operand(ui, "index", index);
                changed |= labeled_operand(ui, "value", value);
            });
        }
        ScriptBlock::ListLength { name, list } => {
            ui.horizontal(|ui| {
                changed |= name_field(ui, "into", name);
                changed |= name_field(ui, "list", list);
            });
        }
        ScriptBlock::ListClear { list } => {
            ui.horizontal(|ui| changed |= name_field(ui, "list", list));
        }
        ScriptBlock::SaveValue { key, value } => {
            ui.horizontal(|ui| {
                changed |= name_field(ui, "key", key);
                changed |= labeled_operand(ui, "value", value);
            });
        }
        ScriptBlock::Shoot { damage, range } => {
            ui.horizontal(|ui| {
                changed |= labeled_operand(ui, "dmg", damage);
                changed |= labeled_operand(ui, "range", range);
            });
        }
        ScriptBlock::MoveTo { x, y, z, speed } => {
            ui.horizontal(|ui| {
                changed |= operand_field(ui, x);
                changed |= operand_field(ui, y);
                changed |= operand_field(ui, z);
                changed |= ui.add(egui::DragValue::new(speed).range(0.0..=30.0).speed(0.1).suffix(" m/s")).changed();
            });
        }
        ScriptBlock::Broadcast { message, value } => {
            ui.horizontal(|ui| {
                changed |= name_field(ui, "message", message);
                changed |= labeled_operand(ui, "value", value);
            });
        }
        ScriptBlock::GiveItem { key, amount } => {
            ui.horizontal(|ui| {
                changed |= pickers::pick(ui, &format!("{path}/give"), key, &[Kind::Resource, Kind::Item, Kind::Block], None);
                changed |= labeled_operand(ui, "x", amount);
            });
        }
        ScriptBlock::TakeItem { key, amount, result } => {
            ui.horizontal(|ui| {
                changed |= pickers::pick(ui, &format!("{path}/take"), key, &[Kind::Resource, Kind::Item, Kind::Block], None);
                changed |= labeled_operand(ui, "x", amount);
                changed |= name_field(ui, "result", result);
            });
        }
        ScriptBlock::SetPlayerStat { stat, value, add } => {
            ui.horizontal(|ui| {
                stat_combo(ui, &format!("{path}/stat"), stat, &mut changed);
                changed |= ui.checkbox(add, "add").changed();
                changed |= operand_field(ui, value);
            });
        }
        ScriptBlock::DamagePlayer { amount } => {
            ui.horizontal(|ui| changed |= labeled_operand(ui, "damage", amount));
        }
        ScriptBlock::TeleportPlayer { x, y, z } => {
            ui.horizontal(|ui| {
                changed |= operand_field(ui, x);
                changed |= operand_field(ui, y);
                changed |= operand_field(ui, z);
            });
        }
        ScriptBlock::ShowMessage { text } => {
            changed |= ui.add(egui::TextEdit::singleline(text).hint_text("Hello {name}")).changed();
        }
        ScriptBlock::OpenScreen { screen } | ScriptBlock::CloseScreen { screen } => {
            ui.horizontal(|ui| changed |= name_field(ui, "screen", screen));
        }
        ScriptBlock::SetWeather { weather } => {
            ui.horizontal(|ui| changed |= name_field(ui, "weather", weather));
        }
        ScriptBlock::StartQuest { quest } => {
            ui.horizontal(|ui| changed |= name_field(ui, "quest", quest));
        }
        ScriptBlock::ShowDialogue { dialogue } => {
            ui.horizontal(|ui| changed |= name_field(ui, "dialogue", dialogue));
        }
        ScriptBlock::Breakpoint => {}
        ScriptBlock::Sense { name, sensor } => {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(name).desired_width(80.0)).changed();
                ui.label(RichText::new("<-").size(9.0).color(theme::TEXT_DIM));
                egui::ComboBox::from_id_salt(format!("{path}/sensor"))
                    .selected_text(sensor.label())
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for candidate in Sensor::ALL {
                            if ui.selectable_value(sensor, candidate, candidate.label()).clicked() {
                                changed = true;
                            }
                        }
                    });
            });
        }
        ScriptBlock::SetBlock { offset, block, rot } => {
            ui.horizontal(|ui| {
                changed |= offset_fields(ui, offset);
                changed |= pickers::pick(ui, &format!("{path}/block"), block, &[Kind::Block], None);
                ui.label(RichText::new("rot").size(9.0).color(theme::TEXT_DIM));
                changed |= ui.add(egui::DragValue::new(rot).range(0..=23)).changed();
            });
        }
        ScriptBlock::BreakBlock { offset, drop } => {
            ui.horizontal(|ui| {
                changed |= offset_fields(ui, offset);
                changed |= ui.checkbox(drop, "drop").changed();
            });
        }
        ScriptBlock::SpawnParticle { kind, count } => {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(kind).desired_width(90.0)).changed();
                changed |= ui.add(egui::DragValue::new(count).range(1..=256)).changed();
            });
        }
        ScriptBlock::DropResource { resource, amount } => {
            ui.horizontal(|ui| {
                changed |= pickers::pick(ui, &format!("{path}/resource"), resource, &[Kind::Resource, Kind::Item, Kind::Block], None);
                changed |= ui.add(egui::DragValue::new(amount).range(1..=999)).changed();
            });
        }
        ScriptBlock::PlaySound { sound } => {
            egui::ComboBox::from_id_salt(format!("{path}/sound"))
                .selected_text(sound.as_str())
                .width(150.0)
                .show_ui(ui, |ui| {
                    for name in crate::audio::sound_events::sound_names() {
                        if ui.selectable_value(sound, name.clone(), name.as_str()).clicked() {
                            changed = true;
                        }
                    }
                });
        }
        ScriptBlock::SetTimeOfDay { value } => {
            changed |= ui
                .add(egui::Slider::new(value, -1.0..=1.0).text("(-1 = var `time`)"))
                .changed();
        }
        ScriptBlock::SpawnMob { mob } => {
            changed |= pickers::pick(ui, &format!("{path}/mob"), mob, &[Kind::Mob], None);
        }
        ScriptBlock::PlayAction { action, .. } => {
            changed |= ui.add(egui::TextEdit::singleline(action).desired_width(140.0)).changed();
        }
        ScriptBlock::MoveToward { target, speed } => {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(format!("{path}/target"))
                    .selected_text(target.label())
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for candidate in ScriptTarget::ALL {
                            if ui.selectable_value(target, candidate, candidate.label()).clicked() {
                                changed = true;
                            }
                        }
                    });
                changed |= ui.add(egui::DragValue::new(speed).range(0.0..=30.0).speed(0.1)).changed();
            });
        }
        ScriptBlock::ApplyImpulse { impulse } => {
            ui.horizontal(|ui| {
                for value in impulse.iter_mut() {
                    changed |= ui.add(egui::DragValue::new(value).speed(0.1)).changed();
                }
            });
        }
        ScriptBlock::SetHealth { amount } => {
            ui.horizontal(|ui| changed |= operand_field(ui, amount));
        }
        ScriptBlock::Attack { damage, range } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("dmg").size(9.0).color(theme::TEXT_DIM));
                changed |= ui.add(egui::DragValue::new(damage).range(0.0..=100.0).speed(0.1)).changed();
                ui.label(RichText::new("range").size(9.0).color(theme::TEXT_DIM));
                changed |= ui.add(egui::DragValue::new(range).range(0.5..=16.0).speed(0.05)).changed();
            });
        }
        ScriptBlock::TameEntity => {}
        ScriptBlock::SetSignal { port, value } => {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(port).desired_width(80.0)).changed();
                changed |= operand_field(ui, value);
            });
        }
    }

    changed
}

/// A number, or the name of a variable: the small button switches.
fn operand_field(ui: &mut egui::Ui, value: &mut Operand) -> bool {
    let mut changed = false;
    match value {
        Operand::Number(number) => {
            changed |= ui.add(egui::DragValue::new(number).speed(0.1).max_decimals(3)).changed();
            if ui.small_button("x").on_hover_text("Use a variable").clicked() {
                *value = Operand::Var("value".to_string());
                changed = true;
            }
        }
        Operand::Var(name) => {
            changed |= ui.add(egui::TextEdit::singleline(name).hint_text("variable").desired_width(80.0)).changed();
            if ui.small_button("#").on_hover_text("Use a number").clicked() {
                *value = Operand::Number(0.0);
                changed = true;
            }
        }
    }
    changed
}

fn name_field(ui: &mut egui::Ui, label: &str, value: &mut String) -> bool {
    ui.label(RichText::new(label).size(9.0).color(theme::TEXT_DIM));
    ui.add(egui::TextEdit::singleline(value).desired_width(90.0)).changed()
}

fn labeled_operand(ui: &mut egui::Ui, label: &str, value: &mut Operand) -> bool {
    ui.label(RichText::new(label).size(9.0).color(theme::TEXT_DIM));
    operand_field(ui, value)
}

fn math_op_combo(ui: &mut egui::Ui, id: &str, op: &mut MathOp, changed: &mut bool) {
    egui::ComboBox::from_id_salt(id).selected_text(op.symbol()).width(40.0).show_ui(ui, |ui| {
        for candidate in MathOp::ALL {
            if ui.selectable_value(op, candidate, candidate.symbol()).clicked() {
                *changed = true;
            }
        }
    });
}

fn stat_combo(ui: &mut egui::Ui, id: &str, stat: &mut PlayerStat, changed: &mut bool) {
    egui::ComboBox::from_id_salt(id).selected_text(stat.label()).width(100.0).show_ui(ui, |ui| {
        for candidate in PlayerStat::ALL {
            if ui.selectable_value(stat, candidate, candidate.label()).clicked() {
                *changed = true;
            }
        }
    });
}

/// The kind of question, then its own fields.
fn question_fields(ui: &mut egui::Ui, path: &str, question: &mut Question, changed: &mut bool) {
    let kinds = [
        ("Item count", Question::ItemCount { key: String::new() }),
        ("Nearest mob", Question::NearestMob { rig: String::new() }),
        ("Mob count", Question::MobCount { rig: String::new() }),
        ("Block at", Question::BlockAt { offset: [0, 1, 0], block: String::new() }),
        ("Player stat", Question::Stat { stat: PlayerStat::Health }),
        ("Saved value", Question::Saved { key: "score".to_string() }),
    ];
    let current = kinds.iter().find(|(_, kind)| std::mem::discriminant(kind) == std::mem::discriminant(question)).map_or("", |(label, _)| label);
    egui::ComboBox::from_id_salt(format!("{path}/question")).selected_text(current).width(100.0).show_ui(ui, |ui| {
        for (label, kind) in &kinds {
            if ui.selectable_label(*label == current, *label).clicked() {
                *question = kind.clone();
                *changed = true;
            }
        }
    });
    match question {
        Question::ItemCount { key } => *changed |= pickers::pick(ui, &format!("{path}/key"), key, &[Kind::Resource, Kind::Item, Kind::Block], None),
        Question::NearestMob { rig } | Question::MobCount { rig } => {
            *changed |= pickers::pick(ui, &format!("{path}/rig"), rig, &[Kind::Mob], Some("any mob"));
        }
        Question::BlockAt { offset, block } => {
            *changed |= offset_fields(ui, offset);
            *changed |= pickers::pick(ui, &format!("{path}/block"), block, &[Kind::Block], Some("any block"));
        }
        Question::Stat { stat } => stat_combo(ui, &format!("{path}/stat"), stat, changed),
        Question::Saved { key } => *changed |= ui.add(egui::TextEdit::singleline(key).desired_width(90.0)).changed(),
    }
}

fn offset_fields(ui: &mut egui::Ui, offset: &mut [i32; 3]) -> bool {
    let mut changed = false;
    for value in offset.iter_mut() {
        changed |= ui.add(egui::DragValue::new(value).range(-32..=32)).changed();
    }
    changed
}

fn add_block_menu(ui: &mut egui::Ui, blocks: &mut Vec<ScriptBlock>, action_names: &[String]) -> bool {
    let mut added = false;
    let filter_id = egui::Id::new("weave_block_search");

    ui.menu_button("+ Add block", |ui| {
        let mut filter: String = ui.ctx().data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
        ui.add(egui::TextEdit::singleline(&mut filter).hint_text("🔍 search a block").desired_width(180.0));
        ui.ctx().data_mut(|d| d.insert_temp(filter_id, filter.clone()));
        let needle = filter.to_lowercase();

        let palette = ScriptBlock::palette();
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            for (category, title) in [
                (BlockCategory::Flow, "CONTROL"),
                (BlockCategory::Data, "DATA & MATH"),
                (BlockCategory::Player, "PLAYER & ITEMS"),
                (BlockCategory::World, "VOXEL WORLD"),
                (BlockCategory::Entity, "ENTITIES & MOVEMENT"),
                (BlockCategory::Signal, "SIGNALS"),
            ] {
                let matching: Vec<&ScriptBlock> = palette
                    .iter()
                    .filter(|(entry, block)| {
                        *entry == category
                            && (needle.is_empty()
                                || block.kind_label().to_lowercase().contains(&needle)
                                || block.help().to_lowercase().contains(&needle))
                    })
                    .map(|(_, block)| block)
                    .collect();
                if matching.is_empty() {
                    continue;
                }
                ui.label(RichText::new(title).size(8.5).color(category_color(category)));
                for block in matching {
                    if ui.button(block.kind_label()).on_hover_text(block.help()).clicked() {
                        let mut block = block.clone();
                        if let (ScriptBlock::CallAction { action, .. }, Some(first)) = (&mut block, action_names.first()) {
                            action.clone_from(first);
                        }
                        blocks.push(block);
                        added = true;
                        ui.close();
                    }
                }
                ui.separator();
            }
        });
    });

    added
}

fn compare_op_combo(ui: &mut egui::Ui, id: &str, op: &mut CompareOp, changed: &mut bool) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(op.symbol())
        .width(46.0)
        .show_ui(ui, |ui| {
            for candidate in CompareOp::ALL {
                if ui
                    .selectable_value(op, candidate, candidate.symbol())
                    .clicked()
                {
                    *changed = true;
                }
            }
        });
}

fn show_validation(ui: &mut egui::Ui, issues: &[ValidationIssue]) {
    let errors = issues
        .iter()
        .filter(|issue| issue.severity == IssueSeverity::Error)
        .count();
    let warnings = issues.len() - errors;

    egui::CollapsingHeader::new(
        RichText::new(format!(
            "VALIDATION  •  {errors} error(s)  •  {warnings} warning(s)"
        ))
        .size(9.5)
        .color(if errors > 0 { theme::ERROR } else { theme::TEXT_DIM }),
    )
    .id_salt("polyhedron_script_validation")
    .default_open(errors > 0)
    .show(ui, |ui| {
        if issues.is_empty() {
            ui.label(
                RichText::new("No issues. The library is ready to run.")
                    .size(9.0)
                    .color(theme::ACCENT_DIM),
            );
            return;
        }

        for issue in issues.iter().take(40) {
            let color = match issue.severity {
                IssueSeverity::Error => theme::ERROR,
                IssueSeverity::Warning => theme::WARN,
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
    });
}

fn status_color(status: Option<ProgramStatus>) -> Color32 {
    match status {
        Some(ProgramStatus::Running) => theme::ACCENT,
        Some(ProgramStatus::Waiting) => theme::WARN,
        Some(ProgramStatus::Completed) => theme::ACCENT_DIM,
        Some(ProgramStatus::Error) => theme::ERROR,
        Some(ProgramStatus::Disabled) => theme::TEXT_DIM,
        Some(ProgramStatus::Idle) | None => theme::TEXT_DIM,
    }
}

/// Blockly-style colour per palette family.
fn category_color(category: BlockCategory) -> Color32 {
    match category {
        BlockCategory::Flow => Color32::from_rgb(240, 184, 92),
        BlockCategory::Data => Color32::from_rgb(148, 140, 255),
        BlockCategory::World => Color32::from_rgb(110, 200, 120),
        BlockCategory::Entity => theme::ACCENT,
        BlockCategory::Signal => Color32::from_rgb(255, 132, 186),
        BlockCategory::Player => Color32::from_rgb(120, 214, 224),
    }
}
