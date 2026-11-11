//! Studio plugin: egui context, dock layout, menu bar and status bar.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, ClearColorConfig};
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiPrimaryContextPass, PrimaryEguiContext,
};


use crate::core::app_state::{EditorMode, EditorState, EditorUiState, Workspace};
use crate::editor::mob_studio::MobStudio;
use crate::core::pie_manager::PieState;
use crate::core::project_io::{load_ron, ProjectLayout};
use crate::editor::dock_layout::{EditorLayout, EditorTab};
use crate::editor::scene_sync;
use crate::editor::theme;
use crate::editor::views;
use crate::editor::world_actions;
use crate::gameplay::mobs::library::MobLibrary;
use crate::gameplay::player_stats::PlayerStats;
use crate::scripting::library::ScriptLibrary;
use crate::scripting::model::IssueSeverity;
use crate::scripting::vm::ScriptRuntime;
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::picking::VoxelCursor;
use crate::voxel::world::VoxelWorld;
use crate::voxel::VoxelSet;
use bevy::ecs::system::SystemParam;
use crate::audio::AudioSettings;
use crate::gameplay::combat::CombatState;
use crate::gameplay::data_cables::CableNetwork;
use crate::gameplay::energy_grid::EnergyGrid;
use crate::gameplay::inventory::{HotbarItem, Inventory};
use crate::gameplay::mining::MiningState;
use crate::gameplay::world_rules::WorldRules;
use crate::ui::render::HudData;
use crate::ui::UiLibrary;
use crate::voxel::biomes::BiomeLibrary;
use crate::voxel::chunk_manager::WorldStreaming;
use crate::voxel::structures::{StructureLibrary, StructureRoom};

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        if let Some(mut settings) = app.world_mut().get_resource_mut::<EguiGlobalSettings>() {
            settings.auto_create_primary_context = false;
        }

        app.add_plugins((
            crate::editor::studio_common::StudioCommonPlugin,
            crate::editor::mob_studio::MobStudioPlugin,
            crate::editor::block_studio::BlockStudioPlugin,
            crate::editor::biome_studio::BiomeStudioPlugin,
            crate::editor::menus::MenuPlugin,
            crate::editor::structure_studio::StructureStudioPlugin,
            crate::editor::item_studio::ItemStudioPlugin,
            crate::editor::worlds::WorldsPlugin,
            crate::editor::game_ui::GameUiPlugin,
            crate::editor::undo::UndoPlugin,
        ))
            .init_resource::<EditorLayout>()
            .init_resource::<crate::editor::save_all::SavedRevisions>()
            .add_systems(Startup, crate::editor::save_all::load_layout.after(crate::core::project_io::initialize_project_system))
            .add_systems(Startup, spawn_ui_camera)
            .add_systems(Update, crate::editor::pickers::refresh_catalog)
            .add_systems(
                Startup,
                crate::editor::views::shader_editor::apply_startup_preset.after(crate::core::project_io::initialize_project_system),
            )
            .add_systems(Update, (tick_editor_frame, world_shortcuts))
            .add_systems(
                Update,
                scene_sync::refresh_scene_entries.in_set(VoxelSet::Sync),
            )
            .add_systems(
                EguiPrimaryContextPass,
                (apply_theme_system, draw_editor, crate::editor::save_all::save_all_system).chain(),
            );
    }
}

/// Ctrl+S saves everything, even while a text field has the keyboard. While
/// the game runs the keys belong to the game.
fn world_shortcuts(keyboard: Res<ButtonInput<KeyCode>>, pie: Res<PieState>, menu: Res<crate::editor::menus::MenuState>, mut ui: ResMut<EditorUiState>) {
    if pie.is_playing() || menu.covers_studio() {
        return;
    }
    let control = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if control && keyboard.just_pressed(KeyCode::KeyS) {
        ui.save_all_request = true;
    }
}

/// Egui needs its own camera so the 3D camera can be restricted to the viewport
/// rectangle without clipping the studio panels.
fn spawn_ui_camera(mut commands: Commands) {
    commands.spawn((
        PrimaryEguiContext,
        Camera2d,
        Camera {
            order: 10,
            clear_color: ClearColorConfig::None,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            ..default()
        },
        RenderLayers::none(),
    ));
}

fn tick_editor_frame(time: Res<Time>, mut editor: ResMut<EditorState>, mut ui: ResMut<EditorUiState>) {
    editor.frame = editor.frame.wrapping_add(1);

    let instant = (1.0 / time.delta_secs().max(0.000_1)).min(999.0);
    ui.fps = ui.fps * 0.9 + instant * 0.1;
}

fn apply_theme_system(
    mut contexts: EguiContexts,
    mut ui: ResMut<EditorUiState>,
) -> Result {
    if ui.theme_applied {
        // The fonts set last frame are live now.
        theme::mark_pixel_ready();
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;
    theme::apply_theme(ctx);
    ui.theme_applied = true;
    Ok(())
}

fn draw_editor(
    mut contexts: EguiContexts,
    mut layout: ResMut<EditorLayout>,
    mut editor: ResMut<EditorState>,
    mut ui_state: ResMut<EditorUiState>,
    mut pie: ResMut<PieState>,
    mut world: ResMut<VoxelWorld>,
    cursor: Res<VoxelCursor>,
    project: Res<ProjectLayout>,
    mut registry: ResMut<BlockRegistry>,
    mut scripts: ResMut<ScriptLibrary>,
    mut script_runtime: ResMut<ScriptRuntime>,
    mut mobs: ResMut<MobLibrary>,
    mut panels: StudioPanels,
    mut studio: ResMut<MobStudio>,
    mut extras: StudioExtras,
    mut game: GameHud,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // The menu character is only drawn while the main menu shows it.
    game.menu_preview.rect = None;

    // Main menu: covers the studio, the world turns behind it.
    if extras.menu.covers_studio() && !pie.is_playing() {
        ui_state.viewport_rect = Some(ctx.viewport_rect());
        ui_state.viewport_hovered = false;
        ui_state.pixels_per_point = ctx.pixels_per_point();
        let biome_names: Vec<String> = panels.biomes.biomes.iter().map(|b| b.name.clone()).collect();
        let action = crate::editor::menus::draw_main_menu(
            ctx,
            extras.menu.as_mut(),
            extras.settings.as_mut(),
            extras.rebind.as_mut(),
            extras.browser.as_mut(),
            &biome_names,
            &project,
            ui_state.as_mut(),
            game.menu_preview.as_mut(),
            game.menu_character.as_mut(),
            &mobs.player_rig(),
        );
        crate::editor::menus::apply_action(action, extras.menu.as_mut(), editor.as_mut(), ui_state.as_mut(), pie.as_mut(), game.quit.as_mut());
        game.preview.rect = None;
        return Ok(());
    }

    // Play-In-Editor: the dock folds away and the viewport fills the window.
    if pie.is_playing() {
        ui_state.viewport_rect = Some(ctx.viewport_rect());
        ui_state.viewport_hovered = !extras.menu.is_open();
        ui_state.pixels_per_point = ctx.pixels_per_point();
        let mut hud = panels.hud_data(&registry, &world, &extras.items);
        hud.max_health = panels.stats.max_health;
        hud.variables = script_runtime.variables().clone();
        hud.lists = script_runtime.lists().clone();
        hud.variables.insert("respawn_cost".to_string(), crate::core::game_rules::game_rules().vitals.respawn_here_cost as f32);
        hud.altars = game.machines.death.altars.iter().map(|cell| format!("Etheraltar {} {} {}", cell.x, cell.y, cell.z)).collect();
        hud.resource_looks = crate::gameplay::crafting::book()
            .resources
            .iter()
            .map(|r| (r.id.clone(), (r.label.clone(), egui::Color32::from_rgb(r.color[0], r.color[1], r.color[2]))))
            .collect();
        hud.minimap = crate::editor::game_ui::minimap_texture(ctx, &mut game.minimap);
        let parts = panels.umg.hud().map(|layout| layout.parts).unwrap_or_default();
        if pie.started_this_frame {
            panels.screens.names.clear();
        }
        let screen = ctx.viewport_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("game_hud")));
        let spectator = panels.inventory.spectator;

        // Top-right card: 3D replica + minimap.
        let position = game.minimap.position;
        let biome = panels
            .streaming
            .generator
            .as_ref()
            .map(|generator| generator.biome_name(generator.biome_at(position.x as i32, position.z as i32)).to_string());
        let status = crate::editor::game_ui::StatusInfo {
            show_preview: extras.settings.show_player_preview,
            show_minimap: extras.settings.show_minimap,
            biome: biome.as_deref(),
            time_of_day: panels.rules.time_of_day,
        };
        let card_bottom = if !parts.status { screen.top() + 8.0 } else { crate::editor::game_ui::draw_status_panel(ctx, &painter, screen, &mut game.minimap, &mut game.preview, &status) };

        let environment = crate::editor::pie_overlay::PlayerView {
            underwater: extras.players.iter().next().and_then(|player| player.head_under),
            show_fps: extras.settings.show_fps,
            paused: extras.menu.is_open(),
            pause_key: extras.settings.bindings.describe(crate::core::settings::GameAction::Pause),
            creative: !panels.inventory.survival,
            fps_y: card_bottom,
        };
        let mut widgets = std::mem::take(&mut panels.screens.widgets);
        let mut actions = crate::editor::pie_overlay::show(ctx, &pie, &panels.stats, &ui_state, panels.umg.hud(), &hud, &environment, &mut widgets);
        if spectator {
            theme::hud_pill(&painter, egui::pos2(screen.center().x, screen.bottom() - 40.0), egui::Align2::CENTER_BOTTOM, "SPECTATOR  -  /gamemode creative to leave", 14.0, theme::TEXT);
        } else {
            if parts.hotbar {
                crate::editor::game_ui::draw_hotbar(&painter, screen, &panels.inventory, &registry, &extras.items, &panels.stats);
            }
        }

        // Crosshair with the aimed block and what can be done with it.
        if pie.cursor_captured && !extras.menu.is_open() && !spectator && parts.crosshair {
            let bindings = &extras.settings.bindings;
            let mut info = crate::editor::game_ui::CrosshairInfo {
                combo: hud.combo as u32,
                mining: hud.mining,
                attack_key: bindings.describe(crate::core::settings::GameAction::Attack),
                use_key: bindings.describe(crate::core::settings::GameAction::Use),
                interact_key: bindings.describe(crate::core::settings::GameAction::Interact),
                hitmarker: panels.combat.hitmarker / crate::gameplay::combat::HITMARKER_TIME,
                heavy_hit: panels.combat.heavy_hit,
                ..Default::default()
            };
            if let HotbarItem::Block(id) = panels.inventory.current() {
                if registry.get(*id).synthetic {
                    info.extra_hint = Some((bindings.describe(crate::core::settings::GameAction::Customize), "Customize".to_string()));
                }
            }
            if let Some(voxel) = game.aim.hit.and_then(|hit| world.get(hit.voxel)) {
                let block = voxel.sub.map(|sub| sub.block).unwrap_or(voxel.block);
                let def = registry.get(block);
                let tool_mines = game.held.mining_speed > 0.0;
                info.target = Some(def.label.clone());
                info.unbreakable = def.hardness >= 1000.0 && !def.is_liquid();
                info.breakable = !info.unbreakable && !def.is_liquid() && tool_mines;
                info.interact = crate::editor::game_ui::interact_verb(def.function, def.script.is_some()).map(str::to_string);
                info.can_place = match panels.inventory.current() {
                    HotbarItem::Block(id) => {
                        !panels.inventory.survival || panels.inventory.count(registry.get(*id).resource_id()) > 0
                    }
                    _ => false,
                };
            }
            // A creature in front of the aimed block wins the crosshair info.
            if let Some(mob) = &game.machines.aimed.target {
                let mood = if mob.tamed {
                    "  -  tamed"
                } else if mob.hostile {
                    "  -  hostile"
                } else {
                    ""
                };
                info.target = Some(format!("{}  {:.0}/{:.0} HP{mood}", mob.name, mob.health.max(0.0).ceil(), mob.max_health));
                info.breakable = false;
                info.unbreakable = false;
                info.can_place = false;
                info.interact = None;
            }
            crate::editor::game_ui::draw_crosshair(&painter, screen, &info);
        }

        if parts.chat || game.chat.open {
            crate::editor::game_ui::draw_chat(ctx, game.chat.as_mut(), screen);
        }

        if (game.panel.open.is_some() || game.panel.body_motor.is_some()) && !extras.menu.is_open() {
            crate::editor::machine_ui::draw_machine_panel(
                ctx,
                crate::editor::machine_ui::MachineUiData {
                    panel: game.panel.as_mut(),
                    motors: game.motors.as_mut(),
                    grid: &panels.grid,
                    world: &world,
                    registry: &registry,
                    inventory: &panels.inventory,
                    items: &extras.items,
                    network: game.network.stored,
                    configs: game.machines.configs.as_mut(),
                    logic: &game.machines.logic,
                    summonable: game.machines.bestiary.summonable(&mobs),
                    killed: {
                        let mut killed: Vec<String> = game.machines.bestiary.killed.iter().cloned().collect();
                        killed.sort();
                        killed
                    },
                    mob_names: mobs.mob_rigs().map(|rig| rig.name.clone()).collect(),
                },
            );
        }
        // Project screens: opened ones, then the ones replacing a built-in.
        let mut modal = false;
        for name in panels.screens.names.clone() {
            if let Some(layout) = panels.umg.find(&name) {
                modal |= layout.role == crate::ui::ScreenRole::Modal;
                actions.extend(crate::editor::pie_overlay::draw_screen(ctx, &pie, layout, &hud, &mut widgets));
            }
        }
        panels.screens.modal = modal || game.quests.dialogue.is_some();
        crate::gameplay::quests::draw(ctx, game.quests.as_mut(), parts.quests);
        if game.machines.death.open {
            if let Some(layout) = panels.umg.with_role(crate::ui::ScreenRole::Death) {
                actions.extend(crate::editor::pie_overlay::draw_screen(ctx, &pie, layout, &hud, &mut widgets));
                if let Some(error) = &game.machines.death.error {
                    painter.text(egui::pos2(screen.center().x, screen.bottom() - 60.0), egui::Align2::CENTER_CENTER, error, egui::FontId::proportional(14.0), theme::ERROR);
                }
            } else {
            crate::editor::game_ui::draw_death_screen(ctx, game.machines.death.as_mut(), panels.inventory.count("ethereal"), panels.inventory.survival);
            }
        }
        if extras.menu.inventory_open && !extras.menu.is_open() {
            let mob_names: Vec<String> = mobs.mob_rigs().map(|rig| rig.name.clone()).collect();
            let open = crate::editor::game_ui::draw_inventory(ctx, panels.inventory.as_mut(), &registry, &extras.items, &mob_names, ui_state.as_mut());
            if !open {
                extras.menu.inventory_open = false;
            }
        }
        if extras.menu.designer_open && !extras.menu.is_open() {
            let open = crate::editor::game_ui::draw_synthetic_designer(ctx, panels.inventory.as_mut(), registry.as_mut(), ui_state.as_mut());
            if !open {
                extras.menu.designer_open = false;
            }
        }
        let custom_pause = (extras.menu.screen == Some(crate::editor::menus::MenuScreen::Pause))
            .then(|| panels.umg.with_role(crate::ui::ScreenRole::Pause))
            .flatten();
        if let Some(layout) = custom_pause {
            actions.extend(crate::editor::pie_overlay::draw_screen(ctx, &pie, layout, &hud, &mut widgets));
        } else if extras.menu.is_open() {
            let saves_world = pie.persist_world.is_some();
            let action = crate::editor::menus::draw_pause_menu(
                ctx,
                extras.menu.as_mut(),
                extras.settings.as_mut(),
                extras.rebind.as_mut(),
                &project,
                ui_state.as_mut(),
                saves_world,
            );
            crate::editor::menus::apply_action(action, extras.menu.as_mut(), editor.as_mut(), ui_state.as_mut(), pie.as_mut(), game.quit.as_mut());
        }
        panels.screens.widgets = widgets;
        for (action, row) in actions {
            use crate::editor::menus::MenuAction;
            use crate::gameplay::death::RespawnChoice;
            let menu_action = match &action {
                crate::ui::ButtonAction::OpenStudio if crate::core::projects::game_only() => MenuAction::StopToMainMenu,
                crate::ui::ButtonAction::OpenStudio => MenuAction::StopToStudio,
                crate::ui::ButtonAction::QuitToMenu => MenuAction::StopToMainMenu,
                crate::ui::ButtonAction::QuitGame => MenuAction::Quit,
                _ => MenuAction::None,
            };
            if !matches!(menu_action, MenuAction::None) {
                crate::editor::menus::apply_action(menu_action, extras.menu.as_mut(), editor.as_mut(), ui_state.as_mut(), pie.as_mut(), game.quit.as_mut());
                continue;
            }
            match action {
                crate::ui::ButtonAction::OpenStudio | crate::ui::ButtonAction::QuitToMenu | crate::ui::ButtonAction::QuitGame => {}
                crate::ui::ButtonAction::OpenScreen(name) => panels.screens.open(name),
                crate::ui::ButtonAction::CloseScreen(name) => panels.screens.close(&name),
                crate::ui::ButtonAction::ToggleScreen(name) => panels.screens.toggle(name),
                crate::ui::ButtonAction::Event(key) => {
                    crate::scripting::context::emit_game_event(crate::scripting::context::GameEvent::UiAction, &key, row as f32, None);
                }
                crate::ui::ButtonAction::Resume => extras.menu.screen = None,
                crate::ui::ButtonAction::OpenSettings => extras.menu.open_settings(crate::editor::menus::MenuScreen::Pause),
                crate::ui::ButtonAction::RespawnAtSpawn => game.machines.death.choice = Some(RespawnChoice::WorldSpawn),
                crate::ui::ButtonAction::RespawnWhereDied => game.machines.death.choice = Some(RespawnChoice::WhereDied),
                crate::ui::ButtonAction::RespawnAtAltar => {
                    if let Some(altar) = game.machines.death.altars.get(row).copied() {
                        game.machines.death.choice = Some(RespawnChoice::Etheraltar(altar));
                    }
                }
                crate::ui::ButtonAction::None => {}
                crate::ui::ButtonAction::StopPie => pie.stop(editor.as_mut(), ui_state.as_mut()),
                crate::ui::ButtonAction::ToggleSurvival => {
                    panels.inventory.survival = !panels.inventory.survival;
                }
                crate::ui::ButtonAction::Signal(port) => {
                    script_runtime.fire_event(
                        crate::scripting::context::ScriptOwner::Global,
                        &crate::scripting::context::ScriptEvent::Signal { port, value: 1.0 },
                    );
                }
            }
        }
        return Ok(());
    }
    game.preview.rect = None;

    let mut root = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("polyhedron_root"),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    views::help::show(ctx, &mut ui_state.help_open);

    egui::Panel::top("polyhedron_top_bar")
        .exact_size(48.0)
        .frame(theme::bar_frame())
        .show(&mut root, |ui| {
            top_bar(
                ui,
                editor.as_mut(),
                ui_state.as_mut(),
                pie.as_mut(),
                layout.as_mut(),
                &project,
                world.as_mut(),
                &registry,
                &scripts,
                script_runtime.as_mut(),
                game.quit.as_mut(),
            );
        });

    egui::Panel::left("polyhedron_workspace_rail")
        .exact_size(64.0)
        .resizable(false)
        .frame(theme::bar_frame())
        .show(&mut root, |ui| workspace_rail(ui, editor.as_mut()));

    egui::Panel::bottom("polyhedron_status_bar")
        .exact_size(26.0)
        .frame(theme::bar_frame())
        .show(&mut root, |ui| {
            status_bar(ui, &editor, &ui_state, &world, &script_runtime);
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.inner_margin(egui::Margin::same(4)))
        .show(&mut root, |ui| {
            let style = dock_style(ui);
            let workspace = editor.workspace;
            let mut viewer = EditorTabViewer {
                quit: game.quit.as_mut(),
                editor: editor.as_mut(),
                ui: ui_state.as_mut(),
                pie: pie.as_mut(),
                world: world.as_mut(),
                cursor: &cursor,
                project: &project,
                registry: registry.as_mut(),
                scripts: scripts.as_mut(),
                script_runtime: script_runtime.as_mut(),
                mobs: mobs.as_mut(),
                studio: studio.as_mut(),
                biomes: panels.biomes.as_mut(),
                structures: panels.structures.as_mut(),
                room: panels.room.as_mut(),
                streaming: panels.streaming.as_mut(),
                umg: panels.umg.as_mut(),
                cables: panels.cables.as_mut(),
                inventory: panels.inventory.as_mut(),
                audio: panels.audio.as_mut(),
                rules: panels.rules.as_mut(),
                grid: &panels.grid,
                forge: extras.forge.as_mut(),
                terraform: extras.terraform.as_mut(),
                items: extras.items.as_mut(),
                item_studio: extras.item_studio.as_mut(),
                structure_studio: extras.structure_studio.as_mut(),
                fluids: &extras.fluids,
            };

            egui_dock::DockArea::new(layout.dock_mut(workspace))
                .id(egui::Id::new(("polyhedron_dock", workspace)))
                .style(style)
                .show_close_buttons(false)
                .show_add_buttons(false)
                .show_add_popup(false)
                .draggable_tabs(true)
                .tab_context_menus(false)
                .show_inside(ui, &mut viewer);
        });

    Ok(())
}

/// Button of the workspace rail: icon with its name below.
fn rail_button(ui: &mut egui::Ui, workspace: Workspace, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(60.0, 46.0), egui::Sense::click());
    let hovered = response.hovered();
    let fill = match (selected, hovered) {
        (true, _) => theme::ACCENT_DIM,
        (false, true) => theme::BG_ELEVATED,
        _ => egui::Color32::TRANSPARENT,
    };
    let painter = ui.painter();
    painter.rect_filled(rect.shrink2(egui::vec2(3.0, 1.0)), 9, fill);
    let color = if selected { egui::Color32::WHITE } else if hovered { theme::TEXT } else { theme::TEXT_DIM };
    painter.text(rect.center_top() + egui::vec2(0.0, 15.0), egui::Align2::CENTER_CENTER, workspace.icon(), egui::FontId::proportional(19.0), color);
    painter.text(rect.center_top() + egui::vec2(0.0, 35.0), egui::Align2::CENTER_CENTER, workspace.label(), egui::FontId::proportional(9.5), color);
    response.on_hover_text(workspace.hint()).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Left rail: every workspace, by family.
fn workspace_rail(ui: &mut egui::Ui, editor: &mut EditorState) {
    egui::ScrollArea::vertical().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for (index, group) in Workspace::RAIL.iter().enumerate() {
            if index > 0 {
                ui.add_space(3.0);
                ui.separator();
                ui.add_space(3.0);
            }
            for workspace in group.iter().copied() {
                if rail_button(ui, workspace, editor.workspace == workspace).clicked() {
                    editor.workspace = workspace;
                }
            }
        }
    });
}

/// The ☰ menu: rare actions only, by theme.
#[allow(clippy::too_many_arguments)]
fn main_menu(
    ui: &mut egui::Ui,
    editor: &EditorState,
    ui_state: &mut EditorUiState,
    layout: &mut EditorLayout,
    project: &ProjectLayout,
    world: &mut VoxelWorld,
    registry: &BlockRegistry,
    scripts: &ScriptLibrary,
    script_runtime: &mut ScriptRuntime,
    quit: &mut crate::editor::menus::QuitRequest,
) {
    ui.menu_button("🌍 World", |ui| {
        if ui.button("📂 Reload").clicked() {
            let name = ui_state.world_name.clone();
            world_actions::load_named(world, registry, project, ui_state, &name);
            ui.close();
        }
        if ui.button("✨ New demo world").clicked() {
            crate::voxel::generator::generate_demo_world(world, registry);
            ui_state.action("Demo world regenerated.");
            ui.close();
        }
    });
    ui.menu_button("🕸 Scripts", |ui| {
        if ui.button("▶ Run all").clicked() {
            script_runtime.start(scripts, false);
            ui.close();
        }
        if ui.add_enabled(script_runtime.is_running(), egui::Button::new("⏹ Stop all")).clicked() {
            script_runtime.stop();
            ui.close();
        }
        if ui.button("✔ Check").clicked() {
            let issues = scripts.validate();
            let errors = issues.iter().filter(|i| i.severity == IssueSeverity::Error).count();
            if issues.is_empty() {
                ui_state.action("No problem in the scripts.");
            } else {
                ui_state.warn(format!("{errors} error(s), {} warning(s).", issues.len() - errors));
            }
            ui.close();
        }
    });
    ui.menu_button("🖥 Panels", |ui| {
        if ui.button("↺ Reset this workspace").clicked() {
            layout.reset(editor.workspace);
            ui.close();
        }
        if ui.button("📂 Back to the saved layout").clicked() {
            match load_ron(&crate::editor::save_all::layout_path(project)) {
                Ok(docks) => layout.docks = docks,
                Err(error) => ui_state.error(error),
            }
            ui.close();
        }
    });
    ui.menu_button("👁 Display", |ui| {
        ui.checkbox(&mut ui_state.show_grid, "📐 Grid");
        ui.checkbox(&mut ui_state.show_gizmos, "🎯 Block cursor");
        ui.checkbox(&mut ui_state.show_stats, "📊 Statistics");
    });
    ui.separator();
    if ui.button("ℹ About").clicked() {
        ui_state.info(format!("Polyhedron Studio {} - Bevy 0.20 - egui 0.36 - Rapier 0.37", env!("CARGO_PKG_VERSION")));
        ui.close();
    }
    if ui.button("🚪 Quit").clicked() {
        quit.frames = Some(1);
    }
}

/// Current workspace, Play in the middle, the common actions on the right.
#[allow(clippy::too_many_arguments)]
fn top_bar(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    ui_state: &mut EditorUiState,
    pie: &mut PieState,
    layout: &mut EditorLayout,
    project: &ProjectLayout,
    world: &mut VoxelWorld,
    registry: &BlockRegistry,
    scripts: &ScriptLibrary,
    script_runtime: &mut ScriptRuntime,
    quit: &mut crate::editor::menus::QuitRequest,
) {
    let bar = ui.max_rect();
    let playing = editor.mode == EditorMode::Playing;
    let play = egui::Button::new(egui::RichText::new(if playing { "⏹  Stop" } else { "▶  Play" }).size(13.0).strong().color(theme::BG_DEEP))
        .fill(if playing { theme::WARN } else { theme::ACCENT })
        .corner_radius(14);
    let mut play_ui = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_center_size(bar.center(), egui::vec2(104.0, 30.0))));
    if play_ui
        .add_sized([104.0, 30.0], play)
        .on_hover_text("Test the game (F7). Everything is restored when it stops.")
        .clicked()
    {
        pie.toggle(editor, ui_state);
    }

    ui.horizontal_centered(|ui| {
        ui.label(egui::RichText::new("💠").size(20.0).color(theme::ACCENT));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("{}  {}", editor.workspace.icon(), editor.workspace.label())).size(14.0).strong().color(theme::TEXT));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(egui::RichText::new("☰").size(18.0), |ui| {
                main_menu(ui, editor, ui_state, layout, project, world, registry, scripts, script_runtime, quit);
            })
            .response
            .on_hover_text("Menu");
            if theme::icon_button(ui, "❓", "Help").clicked() {
                ui_state.help_open = !ui_state.help_open;
            }
            ui.add_space(6.0);
            if theme::icon_button(ui, "↪", "Redo (Ctrl+Y)").clicked() {
                ui_state.undo_request = Some(false);
            }
            if theme::icon_button(ui, "↩", "Undo (Ctrl+Z)").clicked() {
                ui_state.undo_request = Some(true);
            }
            ui.add_space(6.0);
            let unsaved = !ui_state.unsaved.is_empty();
            let text = if unsaved { "💾  Save all  ●" } else { "💾  Save all" };
            let tooltip = if unsaved { format!("Save everything (Ctrl+S)\nChanged: {}", ui_state.unsaved.join(", ")) } else { "Save everything (Ctrl+S)".to_string() };
            let button = egui::Button::new(egui::RichText::new(text).size(12.5).strong().color(if unsaved { theme::BG_DEEP } else { theme::TEXT }))
                .fill(if unsaved { theme::WARN } else { theme::BG_ELEVATED })
                .corner_radius(14)
                .min_size(egui::vec2(0.0, 28.0));
            if ui.add_enabled(!playing, button).on_hover_text(tooltip).on_disabled_hover_text("Stop the game first").clicked() {
                ui_state.save_all_request = true;
            }
        });
    });
}

/// One-line status: mode, save state, last message, FPS.
fn status_bar(
    ui: &mut egui::Ui,
    editor: &EditorState,
    ui_state: &EditorUiState,
    world: &VoxelWorld,
    script_runtime: &ScriptRuntime,
) {
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let (text, color) = if editor.mode.is_playing() {
            ("▶ PLAYING", theme::WARN)
        } else {
            ("✏ EDITING", theme::ACCENT_DIM)
        };
        egui::Frame::NONE
            .fill(color)
            .corner_radius(9)
            .inner_margin(egui::Margin::symmetric(8, 1))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(text).size(10.0).strong().color(theme::BG_DEEP));
            });
        ui.label(
            egui::RichText::new(format!("{} {}", editor.workspace.icon(), editor.workspace.label()))
                .size(11.0)
                .color(theme::TEXT),
        );
        let saved = world.revision == ui_state.last_saved_revision;
        ui.label(
            egui::RichText::new(if saved { "✔ saved" } else { "● modified" })
                .size(11.0)
                .color(if saved { theme::ACCENT_DIM } else { theme::WARN }),
        );
        if script_runtime.is_running() {
            ui.label(egui::RichText::new("🕸 scripts running").size(11.0).color(theme::ACCENT));
        }
        if let Some(line) = ui_state.console.last() {
            ui.label(egui::RichText::new(&line.message).size(11.0).color(line.level.color()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(format!("{:.0} fps", ui_state.fps)).size(10.5).color(theme::TEXT_DIM));
        });
    });
}

fn dock_style(ui: &egui::Ui) -> egui_dock::Style {
    let mut style = egui_dock::Style::from_egui(ui.style());

    style.dock_area_padding = Some(egui::Margin::ZERO);
    style.main_surface_border_stroke = egui::Stroke::new(1.0, theme::BORDER);
    style.main_surface_border_rounding = egui::CornerRadius::same(10);

    style.tab_bar.bg_fill = theme::BG_DEEP;
    style.tab_bar.height = 32.0;
    style.tab_bar.hline_color = theme::BORDER;
    style.tab_bar.corner_radius = egui::CornerRadius::same(10);
    style.tab_bar.fill_tab_bar = false;

    style.tab.active.bg_fill = theme::BG_PANEL;
    style.tab.active.text_color = theme::TEXT;
    style.tab.active.outline_color = theme::BORDER;
    style.tab.active.corner_radius = egui::CornerRadius::same(8);

    style.tab.inactive.bg_fill = theme::BG_DEEP;
    style.tab.inactive.text_color = theme::TEXT_DIM;
    style.tab.inactive.outline_color = theme::BG_DEEP;
    style.tab.inactive.corner_radius = egui::CornerRadius::same(8);

    style.tab.hovered.bg_fill = theme::BG_ELEVATED;
    style.tab.hovered.text_color = theme::TEXT;
    style.tab.hovered.outline_color = theme::BORDER;
    style.tab.hovered.corner_radius = egui::CornerRadius::same(8);

    style.tab.focused.bg_fill = theme::BG_PANEL;
    style.tab.focused.text_color = theme::ACCENT;
    style.tab.focused.outline_color = theme::ACCENT_DIM;
    style.tab.focused.corner_radius = egui::CornerRadius::same(8);

    style.tab.tab_body.bg_fill = theme::BG_PANEL;
    style.tab.tab_body.stroke = egui::Stroke::NONE;
    style.tab.tab_body.inner_margin = egui::Margin::same(10);
    style.tab.tab_body.corner_radius = egui::CornerRadius::same(10);

    style.separator.width = 1.0;
    style.separator.color_idle = theme::BORDER;
    style.separator.color_hovered = theme::ACCENT_DIM;
    style.separator.color_dragged = theme::ACCENT;

    style.buttons.close_tab_color = theme::TEXT_DIM;
    style.buttons.close_tab_active_color = theme::TEXT;
    style.buttons.add_tab_color = theme::TEXT_DIM;

    style.overlay.selection_color = egui::Color32::from_rgba_unmultiplied(78, 210, 190, 80);

    style
}

struct EditorTabViewer<'a> {
    editor: &'a mut EditorState,
    ui: &'a mut EditorUiState,
    pie: &'a mut PieState,
    world: &'a mut VoxelWorld,
    cursor: &'a VoxelCursor,
    project: &'a ProjectLayout,
    registry: &'a mut BlockRegistry,
    scripts: &'a mut ScriptLibrary,
    script_runtime: &'a mut ScriptRuntime,
    mobs: &'a mut MobLibrary,
    studio: &'a mut MobStudio,
    biomes: &'a mut BiomeLibrary,
    structures: &'a mut StructureLibrary,
    room: &'a mut StructureRoom,
    streaming: &'a mut WorldStreaming,
    umg: &'a mut UiLibrary,
    cables: &'a mut CableNetwork,
    inventory: &'a mut Inventory,
    audio: &'a mut AudioSettings,
    rules: &'a mut WorldRules,
    grid: &'a EnergyGrid,
    forge: &'a mut crate::editor::block_studio::BlockStudio,
    terraform: &'a mut crate::editor::biome_studio::BiomeStudio,
    items: &'a mut crate::gameplay::items::ItemLibrary,
    item_studio: &'a mut crate::editor::item_studio::ItemStudio,
    structure_studio: &'a mut crate::editor::structure_studio::StructureStudio,
    fluids: &'a crate::voxel::fluids::FluidSim,
    quit: &'a mut crate::editor::menus::QuitRequest,
}

/// Studios, menus and settings added after the first studio pass.
#[derive(SystemParam)]
struct StudioExtras<'w, 's> {
    forge: ResMut<'w, crate::editor::block_studio::BlockStudio>,
    terraform: ResMut<'w, crate::editor::biome_studio::BiomeStudio>,
    menu: ResMut<'w, crate::editor::menus::MenuState>,
    settings: ResMut<'w, crate::core::settings::GameSettings>,
    rebind: ResMut<'w, crate::core::settings::RebindRequest>,
    players: Query<'w, 's, &'static crate::physics::player_controller::Player>,
    browser: ResMut<'w, crate::editor::worlds::WorldBrowser>,
    items: ResMut<'w, crate::gameplay::items::ItemLibrary>,
    item_studio: ResMut<'w, crate::editor::item_studio::ItemStudio>,
    structure_studio: ResMut<'w, crate::editor::structure_studio::StructureStudio>,
    fluids: Res<'w, crate::voxel::fluids::FluidSim>,
}

/// In-game HUD resources (chat, machine panels, replica, minimap, quit).
#[derive(SystemParam)]
struct GameHud<'w> {
    quests: ResMut<'w, crate::gameplay::quests::QuestLog>,
    chat: ResMut<'w, crate::gameplay::chat::Chat>,
    panel: ResMut<'w, crate::gameplay::machines::MachinePanel>,
    motors: ResMut<'w, crate::gameplay::machines::motors::Motors>,
    machines: MachineHud<'w>,
    aim: Res<'w, crate::gameplay::mining::PlayerAim>,
    held: Res<'w, crate::gameplay::items::HeldTool>,
    preview: ResMut<'w, crate::gameplay::player_preview::PlayerPreview>,
    minimap: ResMut<'w, crate::editor::game_ui::Minimap>,
    quit: ResMut<'w, crate::editor::menus::QuitRequest>,
    menu_preview: ResMut<'w, crate::gameplay::player_preview::MenuPreview>,
    menu_character: ResMut<'w, crate::gameplay::avatar::MenuCharacter>,
    network: Res<'w, crate::gameplay::crafting::OmnicoreNetwork>,
}

/// Machine settings, logic values, bestiary and death screen for the HUD.
#[derive(SystemParam)]
struct MachineHud<'w> {
    configs: ResMut<'w, crate::gameplay::machines::config::MachineConfigs>,
    logic: Res<'w, crate::gameplay::logic::LogicState>,
    bestiary: Res<'w, crate::gameplay::mob_ai::Bestiary>,
    death: ResMut<'w, crate::gameplay::death::DeathState>,
    aimed: Res<'w, crate::gameplay::mining::AimedEntity>,
}

/// Resources of the stage-7+ panels and the in-game HUD.
#[derive(SystemParam)]
struct StudioPanels<'w> {
    stats: Res<'w, PlayerStats>,
    biomes: ResMut<'w, BiomeLibrary>,
    structures: ResMut<'w, StructureLibrary>,
    room: ResMut<'w, StructureRoom>,
    streaming: ResMut<'w, WorldStreaming>,
    umg: ResMut<'w, UiLibrary>,
    screens: ResMut<'w, crate::ui::OpenScreens>,
    cables: ResMut<'w, CableNetwork>,
    inventory: ResMut<'w, Inventory>,
    audio: ResMut<'w, AudioSettings>,
    rules: ResMut<'w, WorldRules>,
    grid: Res<'w, EnergyGrid>,
    mining: Res<'w, MiningState>,
    combat: Res<'w, CombatState>,
}

impl StudioPanels<'_> {
    fn hud_data(&self, registry: &BlockRegistry, world: &VoxelWorld, items: &crate::gameplay::items::ItemLibrary) -> HudData {
        let hotbar = self
            .inventory
            .hotbar
            .iter()
            .map(|item| {
                let color = match item {
                    HotbarItem::Block(id) => {
                        let c = registry.get(*id).color;
                        egui::Color32::from_rgb(c[0], c[1], c[2])
                    }
                    HotbarItem::Empty => egui::Color32::from_black_alpha(0),
                    HotbarItem::Item(id) => {
                        let c = items.find(id).map(|i| i.color()).unwrap_or([200, 200, 210]);
                        egui::Color32::from_rgb(c[0], c[1], c[2])
                    }
                    HotbarItem::SpawnEgg(_) => egui::Color32::from_rgb(236, 224, 200),
                    HotbarItem::Resource(id) => {
                        let c = crate::gameplay::crafting::resource_info(id).map(|info| info.color).unwrap_or([200, 200, 210]);
                        egui::Color32::from_rgb(c[0], c[1], c[2])
                    }
                };
                (item.label(registry, items), color)
            })
            .collect();
        HudData {
            health: self.stats.health,
            stamina: self.stats.stamina,
            max_stamina: self.stats.max_stamina,
            mining: self.mining.fraction(registry, world),
            time_of_day: self.rules.time_of_day,
            kills: self.stats.kills,
            deaths: self.stats.deaths,
            combo: self.combat.combo,
            powered: self.grid.powered_count(),
            resources: self.inventory.resources.clone(),
            hotbar,
            selected: self.inventory.selected,
            ..HudData::default()
        }
    }
}

impl egui_dock::TabViewer for EditorTabViewer<'_> {
    type Tab = EditorTab;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        egui::RichText::new(format!("{}  {}", tab.icon(), tab.title())).size(12.0).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            EditorTab::Viewport => views::viewport::show(
                ui,
                self.ui,
                self.editor,
                self.pie,
                self.world,
                self.cursor,
                self.registry,
                self.mobs,
                self.studio,
                self.forge,
                self.terraform,
                self.biomes,
                self.structure_studio,
                self.item_studio,
            ),
            EditorTab::Hierarchy => views::hierarchy::show(ui, self.editor, self.ui),
            EditorTab::Project => {
                egui::ScrollArea::vertical().id_salt("project_panel").show(ui, |ui| {
                    views::project::show(ui, self.project, self.ui, self.world, self.registry);
                    ui.separator();
                    let libraries = views::project_tools::Libraries { biomes: self.biomes, structures: self.structures, mobs: self.mobs, scripts: self.scripts };
                    views::project_tools::show(ui, self.ui, self.project, libraries, self.quit);
                });
            }
            EditorTab::Toolbox => {
                views::toolbox::show(ui, self.editor, self.ui, self.world, self.registry)
            }
            EditorTab::Inspector => views::inspector::show(
                ui,
                self.editor,
                self.ui,
                self.world,
                self.cursor,
                self.registry,
            ),
            EditorTab::Console => views::console::show(
                ui,
                self.editor,
                self.ui,
                self.pie,
                self.world,
                self.project,
                self.registry,
                self.scripts,
                self.script_runtime,
                self.mobs,
            ),
            EditorTab::Timeline => views::timeline::show(ui, self.mobs, self.studio, self.ui),
            EditorTab::Scripting => views::scripting::show(
                ui,
                self.scripts,
                self.script_runtime,
                self.ui,
                self.project,
            ),
            EditorTab::MobOutliner => {
                views::mob_outliner::show(ui, self.mobs, self.studio, self.ui, self.project)
            }
            EditorTab::MobDetails => {
                let programs: Vec<String> = self
                    .scripts
                    .programs
                    .iter()
                    .filter(|program| program.bound)
                    .map(|program| program.name.clone())
                    .collect();
                views::mob_details::show(ui, self.mobs, self.studio, self.ui, &programs);
            }
            EditorTab::Blocks => {
                let programs: Vec<String> = self
                    .scripts
                    .programs
                    .iter()
                    .filter(|program| program.bound)
                    .map(|program| program.name.clone())
                    .collect();
                views::block_editor::show_details(
                    ui,
                    self.editor,
                    self.ui,
                    self.registry,
                    self.project,
                    &programs,
                    self.forge,
                );
            }
            EditorTab::BlockLibrary => {
                views::block_editor::show_library(ui, self.editor, self.ui, self.registry, self.forge)
            }
            EditorTab::Biomes => views::biome_editor::show_details(ui, self.biomes, self.structures, self.registry),
            EditorTab::BiomeList => views::biome_editor::show_list(
                ui,
                self.ui,
                self.biomes,
                self.structures,
                self.registry,
                self.streaming,
                self.world,
                self.project,
            ),
            EditorTab::Structures => views::structure_editor::show_details(
                ui,
                self.editor,
                self.ui,
                self.structures,
                self.room,
                self.world,
                self.registry,
                self.structure_studio,
                self.project,
            ),
            EditorTab::StructureList => views::structure_editor::show_list(
                ui,
                self.editor,
                self.ui,
                self.structures,
                self.registry,
                self.structure_studio,
                self.project,
            ),
            EditorTab::ItemList => views::item_editor::show_list(ui, self.ui, self.items, self.item_studio, self.project),
            EditorTab::ItemDetails => {
                let programs: Vec<String> = self
                    .scripts
                    .programs
                    .iter()
                    .filter(|program| program.bound)
                    .map(|program| program.name.clone())
                    .collect();
                views::item_editor::show_details(ui, self.items, self.item_studio, &programs);
            }
            EditorTab::UiHierarchy => views::umg_designer::show_hierarchy(ui, self.ui, self.umg, self.project),
            EditorTab::UiProperties => views::umg_designer::show_properties(ui, self.umg),
            EditorTab::World => views::world_editor::show(
                ui,
                views::world_editor::Generation {
                    world: self.world,
                    streaming: self.streaming,
                    biomes: self.biomes,
                    structures: self.structures,
                    registry: self.registry,
                    fluids: self.fluids,
                },
                self.ui,
                self.cables,
                self.inventory,
                self.audio,
                self.rules,
                self.grid,
                self.script_runtime,
                self.project,
            ),
            EditorTab::UiDesigner => views::umg_designer::show_canvas(ui, self.umg),
            EditorTab::Shader => views::shader_editor::show(ui, self.ui, self.project),
            EditorTab::GameRules => views::game_rules::show(ui, self.ui, self.project),
            EditorTab::Crafting => views::crafting_editor::show(ui, self.ui, self.project),
            EditorTab::Audio => views::ambience::show_audio(ui, self.ui, self.project),
            EditorTab::Particles => views::ambience::show_particles(ui, self.ui, self.project),
            EditorTab::Sky => views::ambience::show_sky(ui, self.ui, self.project),
            EditorTab::Story => views::story::show(ui, self.ui, self.project),
        }
    }

    fn clear_background(&self, tab: &Self::Tab) -> bool {
        !tab.is_viewport()
    }

    fn scroll_bars(&self, tab: &Self::Tab) -> [bool; 2] {
        if tab.is_viewport() {
            [false, false]
        } else {
            [false, true]
        }
    }

    fn is_closeable(&self, tab: &Self::Tab) -> bool {
        !tab.is_viewport()
    }

    fn allowed_in_windows(&self, tab: &mut Self::Tab) -> bool {
        !tab.is_viewport()
    }

    fn tab_style_override(
        &self,
        tab: &Self::Tab,
        global_style: &egui_dock::TabStyle,
    ) -> Option<egui_dock::TabStyle> {
        if tab.is_viewport() {
            let mut style = global_style.clone();
            style.tab_body.inner_margin = egui::Margin::ZERO;
            style.tab_body.stroke = egui::Stroke::NONE;
            style.tab_body.bg_fill = egui::Color32::TRANSPARENT;
            Some(style)
        } else {
            None
        }
    }
}
