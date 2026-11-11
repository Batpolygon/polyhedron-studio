//! Console panel with a tiny command line.

use egui::{RichText, vec2};

use crate::core::app_state::{EditorState, EditorUiState};
use crate::core::pie_manager::PieState;
use crate::core::project_io::ProjectLayout;
use crate::editor::theme;
use crate::editor::world_actions;
use crate::gameplay::mobs::library::{MobLibrary, MobSeverity};
use crate::scripting::library::ScriptLibrary;
use crate::scripting::model::IssueSeverity;
use crate::scripting::vm::ScriptRuntime;
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::generator;
use crate::voxel::world::VoxelWorld;

pub fn show(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    state: &mut EditorUiState,
    pie: &mut PieState,
    world: &mut VoxelWorld,
    project: &ProjectLayout,
    registry: &BlockRegistry,
    library: &mut ScriptLibrary,
    runtime: &mut ScriptRuntime,
    mobs: &mut MobLibrary,
) {
    theme::section_label(ui, "CONSOLE");

    let history_height = (ui.available_height() - 38.0).max(60.0);
    egui::ScrollArea::vertical()
        .id_salt("polyhedron_console_history")
        .max_height(history_height)
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for line in &state.console {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(
                        RichText::new(format!("[{}]", line.level.label()))
                            .monospace()
                            .size(9.0)
                            .color(line.level.color()),
                    );
                    ui.label(
                        RichText::new(&line.message)
                            .size(11.0)
                            .color(theme::TEXT),
                    );
                });
            }
        });

    ui.separator();

    let mut submit = false;
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 52.0).max(80.0);
        let response = ui.add_sized(
            vec2(width, 24.0),
            egui::TextEdit::singleline(&mut state.console_input)
                .hint_text(
                    "help, stats, world, chunks, blocks, save, load, worlds, reset, pie, grid, gizmos, fusion, shading, toon, voronoi, rim, outline, bands, outlinewidth, outlinecolor, shaders, scripts, actions, script, clear",
                )
                .font(egui::FontId::monospace(11.0)),
        );

        if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            submit = true;
        }

        if ui.button("Run").clicked() {
            submit = true;
        }
    });

    if submit {
        let command = std::mem::take(&mut state.console_input);
        execute_command(
            command.trim(),
            editor,
            state,
            pie,
            world,
            project,
            registry,
            library,
            runtime,
            mobs,
        );
    }
}

fn execute_command(
    command: &str,
    editor: &mut EditorState,
    state: &mut EditorUiState,
    pie: &mut PieState,
    world: &mut VoxelWorld,
    project: &ProjectLayout,
    registry: &BlockRegistry,
    library: &mut ScriptLibrary,
    runtime: &mut ScriptRuntime,
    mobs: &mut MobLibrary,
) {
    if command.is_empty() {
        return;
    }

    state.action(format!("> {command}"));

    let mut parts = command.split_whitespace();
    let verb = parts.next().unwrap_or_default().to_ascii_lowercase();
    let args: Vec<&str> = parts.collect();
    let argument = args.first().copied().unwrap_or_default();

    match verb.as_str() {
        "help" => state.info(
            "Commands: help, stats, world, chunks, blocks, save [name], load [name], worlds, reset, pie, grid, gizmos, fusion, shading, toon, voronoi, rim, outline, bands <2-12>, outlinewidth <m>, outlinecolor <r> <g> <b>, shaders [reload|reset], mobs, mob [play|pause|stop|next|prev|scrub <s>|clip <name>|save|reload|validate|new] [name], scripts, actions, script [run|stop|reload|save|validate|signals|list] [name], clear",
        ),
        "clear" => state.console.clear(),
        "stats" => {
            let stats = world.stats();
            state.info(format!(
                "mode={} world={} chunks={} voxels={} dirty={} block={} grid={:.2}m snap={} fusion={} fps={:.0}",
                editor.mode.label(),
                state.world_name,
                stats.chunks,
                stats.voxels,
                stats.dirty,
                registry.get(editor.selected_block).label,
                state.grid_size,
                state.snap_to_grid,
                state.greedy_meshing,
                state.fps,
            ));
        }
        "world" => {
            let stats = world.stats();
            let status = if world.revision == state.last_saved_revision {
                "saved"
            } else {
                "modified"
            };
            state.info(format!(
                "World '{}' ({status}): {} chunks, {} voxels, {} dirty, revision {}.",
                state.world_name, stats.chunks, stats.voxels, stats.dirty, stats.revision
            ));
        }
        "chunks" => {
            let mut chunks: Vec<_> = world
                .chunks()
                .map(|chunk| (chunk.coord, chunk.filled()))
                .collect();
            chunks.sort_by_key(|(coord, _)| (coord.x, coord.y, coord.z));

            if chunks.is_empty() {
                state.warn("The world contains no chunks.");
            } else {
                let listing = chunks
                    .iter()
                    .take(12)
                    .map(|(coord, count)| {
                        format!("[{}, {}, {}]: {count}", coord.x, coord.y, coord.z)
                    })
                    .collect::<Vec<_>>()
                    .join("  ");
                let suffix = if chunks.len() > 12 {
                    format!("  (+{} more)", chunks.len() - 12)
                } else {
                    String::new()
                };
                state.info(format!("Chunks: {listing}{suffix}"));
            }
        }
        "blocks" => {
            let listing = registry
                .blocks()
                .take(24)
                .map(|(_, def)| format!("{} ({})", def.id, def.shape.label()))
                .collect::<Vec<_>>()
                .join("  ");
            let suffix = if registry.len() > 24 {
                format!("  (+{} more)", registry.len() - 24)
            } else {
                String::new()
            };
            state.info(format!(
                "{} blocks: {listing}{suffix}",
                registry.len()
            ));
        }
        "save" => {
            if !argument.is_empty() {
                state.world_name = argument.to_string();
            }
            world_actions::save_current(world, registry, project, state);
        }
        "load" => {
            let name = if argument.is_empty() {
                state.world_name.clone()
            } else {
                argument.to_string()
            };
            world_actions::load_named(world, registry, project, state, &name);
        }
        "worlds" => {
            let worlds = world_actions::list_worlds(project);
            if worlds.is_empty() {
                state.warn("No saved worlds yet. Use 'save <name>' or Ctrl+S.");
            } else {
                state.info(format!("Worlds: {}", worlds.join("  ")));
            }
        }
        "reset" => {
            generator::generate_demo_world(world, registry);
            state.action("Demo world regenerated.");
        }
        "pie" => pie.toggle(editor, state),
        "grid" => {
            state.show_grid = !state.show_grid;
            state.info(format!("Grid {}", on_off(state.show_grid)));
        }
        "gizmos" => {
            state.show_gizmos = !state.show_gizmos;
            state.info(format!("Gizmos {}", on_off(state.show_gizmos)));
        }
        "fusion" => {
            state.greedy_meshing = !state.greedy_meshing;
            state.info(format!("Greedy meshing {}", on_off(state.greedy_meshing)));
        }
        "shading" => state.info(format!("Shading: {}", state.shading.summary())),
        "toon" => {
            state.shading.toon = !state.shading.toon;
            state.shading.touch();
            state.info(format!("Toon bands {}", on_off(state.shading.toon)));
        }
        "voronoi" => {
            state.shading.voronoi = !state.shading.voronoi;
            state.shading.touch();
            state.info(format!("Voronoi tint {}", on_off(state.shading.voronoi)));
        }
        "rim" => {
            state.shading.rim = !state.shading.rim;
            state.shading.touch();
            state.info(format!("Rim light {}", on_off(state.shading.rim)));
        }
        "outline" => {
            state.shading.outline = !state.shading.outline;
            state.shading.touch();
            state.info(format!("Inverted-hull outline {}", on_off(state.shading.outline)));
        }
        "bands" => match argument.parse::<u32>() {
            Ok(bands) => {
                state.shading.bands = bands.clamp(2, 12);
                state.shading.touch();
                state.info(format!("Toon bands: {}", state.shading.bands));
            }
            Err(_) => state.warn("Usage: bands <2-12>"),
        },
        "outlinewidth" => match argument.parse::<f32>() {
            Ok(width) => {
                state.shading.outline_width = width.clamp(0.0, 0.05);
                state.shading.touch();
                state.info(format!(
                    "Outline width: {:.4} m per metre of distance",
                    state.shading.outline_width
                ));
            }
            Err(_) => state.warn("Usage: outlinewidth <0.0-0.05>"),
        },
        "outlinecolor" => {
            if args.len() < 3 {
                state.warn("Usage: outlinecolor <r> <g> <b>  (0-255 or 0-1)");
            } else {
                match parse_rgb(&args[0..3]) {
                    Some(color) => {
                        state.shading.outline_color = color;
                        state.shading.touch();
                        state.info(format!(
                            "Outline colour: {}  (r {:.3}  g {:.3}  b {:.3})",
                            rgb_hex(color),
                            color[0],
                            color[1],
                            color[2]
                        ));
                    }
                    None => state.warn(
                        "Usage: outlinecolor <r> <g> <b>  (0-255 or 0-1, finite numbers)",
                    ),
                }
            }
        }
        "mobs" => {
            if mobs.rigs.is_empty() {
                state.warn("No mob. Use 'mob new' or the Mob panel.");
            } else {
                let listing = mobs
                    .rigs
                    .iter()
                    .enumerate()
                    .map(|(index, rig)| {
                        format!(
                            "{}{} ({} box, {} clip)",
                            if index == mobs.selection { "* " } else { "  " },
                            rig.name,
                            rig.boxes.len(),
                            rig.clips.len()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("  ");
                state.info(format!("Mobs ({}): {listing}", mobs.rigs.len()));
            }
        }
        "mob" => match argument.to_ascii_lowercase().as_str() {
            "" | "list" => {
                let listing = mobs
                    .current()
                    .map(|rig| {
                        format!(
                            "{}: {} box(es), {} clip(s), default '{}'",
                            rig.name,
                            rig.boxes.len(),
                            rig.clips.len(),
                            rig.default_clip
                        )
                    })
                    .unwrap_or_else(|| "no mob selected".to_string());
                state.info(format!(
                    "{listing}  •  preview {:.2}s {}",
                    mobs.preview.time,
                    if mobs.preview.playing { "PLAYING" } else { "PAUSED" }
                ));
            }
            "save" => match mobs.save_all(project) {
                Ok(report) => state.action(format!("Saved {} mob(s).", report.rigs)),
                Err(error) => state.error(error),
            },
            "reload" => match mobs.load_from_project(project) {
                Ok(report) => {
                    state.action(format!("Reloaded {} mob(s).", report.rigs));
                    for warning in report.warnings {
                        state.warn(warning);
                    }
                }
                Err(error) => state.error(error),
            },
            "validate" => {
                let issues = mobs.validate();
                let errors = issues
                    .iter()
                    .filter(|issue| issue.severity == MobSeverity::Error)
                    .count();
                let warnings = issues.len() - errors;
                if issues.is_empty() {
                    state.action("Mob validation: no issues.");
                } else {
                    state.warn(format!(
                        "Mob validation: {errors} error(s), {warnings} warning(s)."
                    ));
                    for issue in issues.iter().take(12) {
                        match issue.severity {
                            MobSeverity::Error => state.error(format!(
                                "{} - {}",
                                issue.location, issue.message
                            )),
                            MobSeverity::Warning => state.warn(format!(
                                "{} - {}",
                                issue.location, issue.message
                            )),
                        }
                    }
                }
            }
            "new" => {
                let index = mobs.add_rig();
                let name = mobs.rigs[index].name.clone();
                state.action(format!("Created mob '{name}'."));
            }
            "play" => {
                mobs.preview.playing = true;
                state.action("Mob preview playing.");
            }
            "pause" => {
                mobs.preview.playing = false;
                state.action("Mob preview paused.");
            }
            "stop" => {
                mobs.preview.playing = false;
                mobs.preview.time = 0.0;
                state.action("Mob preview stopped.");
            }
            "scrub" => match args.get(1).and_then(|value| value.parse::<f32>().ok()) {
                Some(time) if time.is_finite() => {
                    let duration = mobs.clip_duration();
                    mobs.preview.scrub(time, duration);
                    mobs.preview.playing = false;
                    state.info(format!("Preview scrubbed to {:.3}s.", mobs.preview.time));
                }
                _ => state.warn("Usage: mob scrub <seconds>"),
            },
            "next" | "prev" => {
                if let Some(rig) = mobs.current() {
                    let clips: Vec<String> =
                        rig.clips.iter().map(|clip| clip.name.clone()).collect();
                    if clips.is_empty() {
                        state.warn("The selected mob has no clip.");
                    } else {
                        let current = mobs.clip_selection.clone();
                        let index = clips
                            .iter()
                            .position(|name| name == &current)
                            .unwrap_or(0);
                        let step = if argument.eq_ignore_ascii_case("next") {
                            1
                        } else {
                            clips.len() - 1
                        };
                        let next = (index + step) % clips.len();
                        mobs.clip_selection = clips[next].clone();
                        mobs.preview.time = 0.0;
                        mobs.preview.playing = true;
                        state.action(format!("Clip '{}'.", clips[next]));
                    }
                }
            }
            "clip" => {
                let target = args.get(1..).map(|rest| rest.join(" ")).unwrap_or_default();
                if target.is_empty() {
                    state.warn("Usage: mob clip <name>");
                } else if let Some(rig) = mobs.current() {
                    if rig.clip(&target).is_some() {
                        mobs.clip_selection = target.clone();
                        mobs.preview.time = 0.0;
                        mobs.preview.playing = true;
                        state.action(format!("Clip '{target}'."));
                    } else {
                        state.error(format!("Unknown clip '{target}'."));
                    }
                }
            }
            other => {
                let target = args.join(" ");
                let _ = other;
                let index = mobs.rigs.iter().position(|rig| {
                    rig.name.eq_ignore_ascii_case(&target)
                });
                match index {
                    Some(index) => {
                        mobs.selection = index;
                        mobs.clamp_selection();
                        mobs.preview.time = 0.0;
                        let name = mobs.rigs[index].name.clone();
                        state.action(format!("Selected mob '{name}'."));
                    }
                    None => state.warn(format!(
                        "Unknown mob '{target}'. Use 'mobs' to list rigs."
                    )),
                }
            }
        },
        "scripts" => {
            if library.programs.is_empty() {
                state.warn("No script programs. Use 'script reload' or create one in the Scripting panel.");
            } else {
                let statuses = runtime.statuses();
                let listing = library
                    .programs
                    .iter()
                    .map(|program| {
                        let status = statuses
                            .iter()
                            .find(|status| status.name == program.name)
                            .map(|status| status.status.label())
                            .unwrap_or("IDLE");
                        format!(
                            "{}{} [{}]",
                            if program.enabled { "" } else { "(off) " },
                            program.name,
                            status
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("  ");
                state.info(format!(
                    "Scripts ({}): {listing}",
                    library.programs.len()
                ));
            }
        }
        "actions" => {
            if library.actions.is_empty() {
                state.warn("No script actions.");
            } else {
                let listing = library
                    .actions
                    .iter()
                    .map(|action| format!("{} ({} blocks)", action.name, action.blocks.len()))
                    .collect::<Vec<_>>()
                    .join("  ");
                state.info(format!(
                    "Actions ({}): {listing}",
                    library.actions.len()
                ));
            }
        }
        "script" => match argument.to_ascii_lowercase().as_str() {
            "" | "list" => {
                let statuses = runtime.statuses();
                if library.programs.is_empty() {
                    state.warn("No script programs.");
                } else {
                    let listing = library
                        .programs
                        .iter()
                        .map(|program| {
                            let status = statuses
                                .iter()
                                .find(|status| status.name == program.name)
                                .map(|status| status.status.label())
                                .unwrap_or("IDLE");
                            format!("{} [{}]", program.name, status)
                        })
                        .collect::<Vec<_>>()
                        .join("  ");
                    state.info(format!(
                        "Scripts: {listing}  •  VM {}",
                        if runtime.is_running() { "RUNNING" } else { "STOPPED" }
                    ));
                }
            }
            "run" => {
                let target = args.get(1).copied();
                match target {
                    Some(name) => {
                        if runtime.start_named(library, name, false) {
                            state.action(format!("Script VM running only '{name}'."));
                        } else {
                            state.error(format!("Unknown script program '{name}'."));
                        }
                    }
                    None => {
                        runtime.start(library, false);
                        state.action(format!(
                            "Script VM started with {} program(s).",
                            runtime.program_count()
                        ));
                    }
                }
            }
            "stop" => {
                runtime.stop();
                state.action("Script VM stopped.");
            }
            "reload" => match library.load_from_project(project) {
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
            },
            "save" => match library.save_all(project) {
                Ok(report) => state.action(format!(
                    "Saved {} program(s) and {} action(s){}.",
                    report.programs,
                    report.actions,
                    if report.removed > 0 {
                        format!(", removed {} stale file(s)", report.removed)
                    } else {
                        String::new()
                    }
                )),
                Err(error) => state.error(error),
            },
            "validate" => {
                let issues = library.validate();
                let errors = issues
                    .iter()
                    .filter(|issue| issue.severity == IssueSeverity::Error)
                    .count();
                let warnings = issues.len() - errors;
                if issues.is_empty() {
                    state.action("Script validation: no issues.");
                } else {
                    state.warn(format!(
                        "Script validation: {errors} error(s), {warnings} warning(s)."
                    ));
                    for issue in issues.iter().take(12) {
                        match issue.severity {
                            IssueSeverity::Error => state.error(format!(
                                "{} - {}",
                                issue.location, issue.message
                            )),
                            IssueSeverity::Warning => state.warn(format!(
                                "{} - {}",
                                issue.location, issue.message
                            )),
                        }
                    }
                }
            }
            "signals" => {
                let signals = runtime.drain_signals();
                if signals.is_empty() {
                    state.info("No pending script signals.");
                } else {
                    let listing = signals
                        .iter()
                        .take(24)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("  ");
                    state.info(format!("Signals ({}): {listing}", signals.len()));
                }
            }
            other => state.warn(format!(
                "Unknown script action '{other}'. Use 'script run|stop|reload|save|validate|signals|list'."
            )),
        },
        "shaders" => match argument.to_ascii_lowercase().as_str() {
            "" | "reload" => {
                state.reload_shader_sources = true;
                state.info("Reloading project/shaders/*.wesl...");
            }
            "reset" => {
                state.reset_shader_sources = true;
                state.info("Restoring the embedded shader sources...");
            }
            other => state.warn(format!(
                "Unknown shader action '{other}'. Use 'shaders reload' or 'shaders reset'."
            )),
        },
        other => state.warn(format!("Unknown command '{other}'. Type 'help'.")),
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "enabled" } else { "disabled" }
}

fn parse_rgb(values: &[&str]) -> Option<[f32; 3]> {
    if values.len() < 3 {
        return None;
    }

    let mut parsed = [0.0f32; 3];
    for (index, value) in values.iter().take(3).enumerate() {
        let number = value.parse::<f32>().ok()?;
        if !number.is_finite() {
            return None;
        }
        parsed[index] = number;
    }

    let scale = if parsed.iter().any(|value| *value > 1.0) {
        255.0
    } else {
        1.0
    };
    Some([
        (parsed[0] / scale).clamp(0.0, 1.0),
        (parsed[1] / scale).clamp(0.0, 1.0),
        (parsed[2] / scale).clamp(0.0, 1.0),
    ])
}

fn rgb_hex(color: [f32; 3]) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        (color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}
