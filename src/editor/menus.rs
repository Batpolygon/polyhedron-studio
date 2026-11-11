//! Main menu, world selection / creation, pause menu, inventory / chat /
//! machine panel toggles and the settings screen (game, sound, controls), in
//! the studio style (dark, modern, rounded).
//!
//! - The main menu opens at launch (can be disabled in the settings) over a
//!   live panorama generated with the parameters being chosen.
//! - *Play* opens the world list: play a saved world or create one (name,
//!   seed, type, game mode, single biome, sea level, caves). *Studio* opens
//!   the editor.
//! - In game, `Esc` (remappable) closes the chat / machine panel / inventory
//!   first, otherwise pauses: the virtual clock stops, the mouse is freed and
//!   the pause menu offers Resume / Settings / Studio / Menu / Quit.
//!   `Tab` opens the inventory, `T` the chat and `/` a command, without pausing.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use egui::{Align2, Color32, CornerRadius, RichText, Stroke, pos2, vec2};

use crate::core::app_state::{EditorState, EditorUiState, Workspace};
use crate::core::editor_camera::EditorCamera;
use crate::core::pie_manager::{PieSet, PieState};
use crate::core::project_io::ProjectLayout;
use crate::core::settings::{GameAction, GameInput, GameSettings, KeyBindings, RebindRequest};
use crate::editor::theme;
use crate::editor::worlds::{GameMode, WorldBrowser, WorldKind, WorldMeta};
use crate::gameplay::chat::Chat;
use crate::gameplay::machines::MachinePanel;
use crate::voxel::chunk_manager::WorldStreaming;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuScreen {
    Main,
    Worlds,
    CreateWorld,
    Customize,
    Pause,
    Settings,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsTab {
    #[default]
    Game,
    Interface,
    Sound,
    Controls,
}

#[derive(Resource, Debug, Default)]
pub struct MenuState {
    pub screen: Option<MenuScreen>,
    /// Screen the settings return to.
    pub settings_return: Option<MenuScreen>,
    pub tab: SettingsTab,
    /// In-game inventory (does not pause the game).
    pub inventory_open: bool,
    /// Synthetic block designer (Customize key, does not pause the game).
    pub designer_open: bool,
}

impl MenuState {
    pub fn is_open(&self) -> bool {
        self.screen.is_some()
    }

    /// Screens that cover the whole studio (main menu family).
    pub fn covers_studio(&self) -> bool {
        match self.screen {
            Some(MenuScreen::Main | MenuScreen::Worlds | MenuScreen::CreateWorld | MenuScreen::Customize) => true,
            Some(MenuScreen::Settings) => self.settings_return == Some(MenuScreen::Main),
            _ => false,
        }
    }

    /// The mouse must be free (menu or inventory).
    pub fn wants_cursor(&self) -> bool {
        self.is_open() || self.inventory_open || self.designer_open
    }

    pub fn open_settings(&mut self, from: MenuScreen) {
        self.settings_return = Some(from);
        self.screen = Some(MenuScreen::Settings);
    }
}

/// Quit requested by a menu: play is stopped first (so the world is saved),
/// then the window closes a few frames later.
#[derive(Resource, Debug, Default)]
pub struct QuitRequest {
    pub frames: Option<u8>,
}

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuState>()
            .init_resource::<QuitRequest>()
            .add_systems(Startup, open_main_menu.after(crate::core::settings::load_settings_system))
            .add_systems(
                Update,
                (
                    pause_input.after(PieSet::Transition).before(crate::core::settings::capture_rebind_system),
                    apply_camera_settings,
                    menu_backdrop,
                    quit_app,
                ),
            );
    }
}

fn open_main_menu(settings: Res<GameSettings>, mut menu: ResMut<MenuState>) {
    if settings.show_main_menu || crate::core::projects::game_only() {
        menu.screen = Some(MenuScreen::Main);
    }
}

/// Close the primary window once a quit was requested: the app exits when
/// its primary window closes.
fn quit_app(mut commands: Commands, mut quit: ResMut<QuitRequest>, windows: Query<Entity, With<PrimaryWindow>>) {
    let Some(frames) = quit.frames else {
        return;
    };
    if frames > 0 {
        quit.frames = Some(frames - 1);
        return;
    }
    quit.frames = None;
    for window in &windows {
        commands.entity(window).despawn();
    }
}

/// `Pause` closes the chat / machine panel / inventory or toggles the pause
/// menu; `Inventory`, `Chat` and `Command` open their screens. The virtual
/// clock follows the pause menu.
#[allow(clippy::too_many_arguments)]
fn pause_input(
    input: GameInput,
    rebind: Res<RebindRequest>,
    pie: Res<PieState>,
    mut menu: ResMut<MenuState>,
    mut chat: ResMut<Chat>,
    mut panel: ResMut<MachinePanel>,
    mut time: ResMut<Time<Virtual>>,
    inventory: Res<crate::gameplay::inventory::Inventory>,
    registry: Res<crate::voxel::blocks::BlockRegistry>,
) {
    if pie.started_this_frame {
        menu.screen = None;
        menu.inventory_open = false;
        menu.designer_open = false;
    }
    if !pie.is_playing() {
        menu.inventory_open = false;
        menu.designer_open = false;
        if matches!(menu.screen, Some(MenuScreen::Pause))
            || (menu.screen == Some(MenuScreen::Settings) && menu.settings_return == Some(MenuScreen::Pause))
        {
            menu.screen = None;
        }
        if time.is_paused() {
            time.unpause();
        }
        return;
    }
    if rebind.pending.is_none() && input.just_pressed(GameAction::Pause) {
        if chat.open {
            chat.close();
        } else if panel.open.is_some() || panel.body_motor.is_some() {
            panel.open = None;
            panel.body_motor = None;
        } else if menu.designer_open {
            menu.designer_open = false;
        } else if menu.inventory_open {
            menu.inventory_open = false;
        } else {
            menu.screen = match menu.screen {
                None => Some(MenuScreen::Pause),
                Some(MenuScreen::Settings) => Some(MenuScreen::Pause),
                Some(_) => None,
            };
        }
    } else if menu.screen.is_none() && !chat.open && panel.open.is_none() && panel.body_motor.is_none() {
        let holds_synthetic = match inventory.current() {
            crate::gameplay::inventory::HotbarItem::Block(id) => registry.get(*id).synthetic,
            crate::gameplay::inventory::HotbarItem::Item(id) => id == "brush",
            _ => false,
        };
        if input.just_pressed(GameAction::Customize) && !menu.inventory_open && !inventory.spectator {
            if menu.designer_open {
                menu.designer_open = false;
            } else if holds_synthetic {
                menu.designer_open = true;
            }
        } else if menu.designer_open {
            // The designer keeps the keyboard.
        } else if !menu.inventory_open && input.just_pressed(GameAction::Command) {
            chat.open_with("/");
        } else if !menu.inventory_open && input.just_pressed(GameAction::Chat) {
            chat.open_with("");
        } else if input.just_pressed(GameAction::Inventory) {
            menu.inventory_open = !menu.inventory_open;
        }
    }
    let paused = menu.is_open();
    if paused && !time.is_paused() {
        time.pause();
    } else if !paused && time.is_paused() {
        time.unpause();
    }
}

/// FOV in game, streaming radius from the view distance.
fn apply_camera_settings(
    pie: Res<PieState>,
    settings: Res<GameSettings>,
    menu: Res<MenuState>,
    mut streaming: ResMut<WorldStreaming>,
    mut projections: Query<&mut Projection, With<EditorCamera>>,
    mut last: Local<Option<u64>>,
) {
    let fov = if pie.is_playing() || menu.covers_studio() {
        settings.fov_degrees.clamp(40.0, 120.0).to_radians()
    } else {
        std::f32::consts::FRAC_PI_4
    };
    for mut projection in &mut projections {
        if let Projection::Perspective(perspective) = projection.as_mut() {
            if (perspective.fov - fov).abs() > 1e-4 {
                perspective.fov = fov;
            }
        }
    }
    if *last != Some(settings.revision) && !menu.covers_studio() {
        *last = Some(settings.revision);
        streaming.radius = settings.view_distance.clamp(1, 16) as _;
    }
}

/// The main menu slowly pans the panorama behind it.
fn menu_backdrop(
    time: Res<Time<Real>>,
    menu: Res<MenuState>,
    pie: Res<PieState>,
    mut cameras: Query<(&mut Transform, &mut EditorCamera)>,
) {
    if !menu.covers_studio() || pie.is_playing() {
        return;
    }
    if let Ok((mut transform, mut camera)) = cameras.single_mut() {
        camera.yaw += time.delta_secs() * 0.035;
        transform.rotation = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch.clamp(-0.6, -0.1), 0.0);
    }
}

/// What the menus asked the editor to do this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    None,
    Studio,
    StopToStudio,
    StopToMainMenu,
    Quit,
}

/// A foreground UI covering `rect`.
fn overlay_ui(ctx: &egui::Context, name: &str, rect: egui::Rect) -> egui::Ui {
    let id = egui::Id::new(name);
    egui::Ui::new(
        ctx.clone(),
        id,
        egui::UiBuilder::new().layer_id(egui::LayerId::new(theme::WINDOW_ORDER, id)).max_rect(rect),
    )
}

/// Dot-matrix glyphs of the title.
fn title_glyph(c: char) -> [&'static str; 7] {
    match c {
        'P' => ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."],
        'O' => [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
        'L' => ["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
        'Y' => ["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."],
        'H' => ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
        'E' => ["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
        'D' => ["####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."],
        'R' => ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
        'N' => ["#...#", "##..#", "#.#.#", "#.#.#", "#..##", "#...#", "#...#"],
        _ => [".....", ".....", ".....", ".....", ".....", ".....", "....."],
    }
}

fn draw_title(painter: &egui::Painter, center: egui::Pos2, text: &str, dot: f32) {
    let letters: Vec<char> = text.chars().collect();
    let pitch = dot * 1.25;
    let width = letters.len() as f32 * 6.0 * pitch - pitch;
    let origin = center - vec2(width * 0.5, 3.5 * pitch);
    for (index, letter) in letters.iter().enumerate() {
        for (row, bits) in title_glyph(*letter).iter().enumerate() {
            for (column, bit) in bits.chars().enumerate() {
                if bit == '#' {
                    let p = origin + vec2((index as f32 * 6.0 + column as f32) * pitch, row as f32 * pitch);
                    let color = if row < 3 { Color32::WHITE } else { theme::ACCENT.lerp_to_gamma(Color32::WHITE, 0.55) };
                    painter.rect_filled(egui::Rect::from_min_size(p, vec2(dot, dot)), CornerRadius::same((dot * 0.3) as u8), color);
                }
            }
        }
    }
}

/// Title band and footer shared by the main menu screens.
fn frame_bars(painter: &egui::Painter, screen: egui::Rect) {
    let top = egui::Rect::from_min_size(screen.min, vec2(screen.width(), 210.0));
    painter.rect_filled(top, 0.0, Color32::from_rgba_premultiplied(8, 12, 17, 200));
    painter.line_segment([top.left_bottom(), top.right_bottom()], Stroke::new(1.0, theme::BORDER));
    let dot = (screen.width() / 130.0).clamp(6.0, 13.0);
    draw_title(painter, top.center() - vec2(0.0, 6.0), "POLYHEDRON", dot);
    painter.text(top.center_bottom() - vec2(0.0, 22.0), Align2::CENTER_CENTER, "A VOXEL SANDBOX", egui::FontId::proportional(12.0), theme::TEXT_DIM);
    let bottom = egui::Rect::from_min_max(pos2(screen.left(), screen.bottom() - 40.0), screen.max);
    painter.rect_filled(bottom, 0.0, Color32::from_rgba_premultiplied(8, 12, 17, 200));
    painter.text(bottom.left_center() + vec2(18.0, 0.0), Align2::LEFT_CENTER, "By Batpolygon", egui::FontId::proportional(13.0), theme::TEXT);
    painter.text(
        bottom.right_center() - vec2(18.0, 0.0),
        Align2::RIGHT_CENTER,
        format!("v{}", env!("CARGO_PKG_VERSION")),
        egui::FontId::proportional(13.0),
        theme::TEXT_DIM,
    );
}

fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(13.5).color(theme::TEXT_DIM));
}

/// Full-screen main menu family (main, worlds, creation, customise, settings).
#[allow(clippy::too_many_arguments)]
pub fn draw_main_menu(
    ctx: &egui::Context,
    menu: &mut MenuState,
    settings: &mut GameSettings,
    rebind: &mut RebindRequest,
    browser: &mut WorldBrowser,
    biome_names: &[String],
    project: &ProjectLayout,
    log: &mut EditorUiState,
    preview: &mut crate::gameplay::player_preview::MenuPreview,
    character: &mut crate::gameplay::avatar::MenuCharacter,
    player_rig: &crate::gameplay::mobs::model::MobRig,
) -> MenuAction {
    let screen = ctx.viewport_rect();
    preview.rect = None;
    preview.pixels_per_point = ctx.pixels_per_point();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("menu_backdrop")));
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha(50));
    frame_bars(&painter, screen);

    if browser.is_launching() {
        let center = screen.center() + vec2(0.0, 60.0);
        let box_rect = egui::Rect::from_center_size(center, vec2(420.0, 80.0));
        painter.rect_filled(box_rect, CornerRadius::same(16), theme::WINDOW_FILL);
        painter.rect_stroke(box_rect, CornerRadius::same(16), Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);
        let dots = ".".repeat(1 + (ctx.input(|i| i.time) * 2.0) as usize % 3);
        painter.text(box_rect.center(), Align2::CENTER_CENTER, format!("Generating the world{dots}"), egui::FontId::proportional(18.0), Color32::WHITE);
        ctx.request_repaint();
        return MenuAction::None;
    }

    let mut action = MenuAction::None;
    let content = egui::Rect::from_min_max(pos2(screen.left() + 60.0, screen.top() + 250.0), pos2(screen.right() - 60.0, screen.bottom() - 60.0));
    match menu.screen {
        Some(MenuScreen::Settings) => settings_window(ctx, menu, settings, rebind, project, log),
        Some(MenuScreen::Worlds) => worlds_screen(ctx, content, menu, browser, project, log),
        Some(MenuScreen::CreateWorld) => create_screen(ctx, content, menu, browser, biome_names),
        Some(MenuScreen::Customize) => customize_screen(ctx, content, menu, settings, project, log, preview, character, player_rig),
        _ => {
            let mut ui = overlay_ui(ctx, "main_menu_buttons", egui::Rect::from_min_size(content.min + vec2(0.0, 10.0), vec2(320.0, 360.0)));
            theme::window_frame().show(&mut ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                if theme::game_button(ui, "Play", true, 260.0).clicked() {
                    browser.refresh(project);
                    menu.screen = Some(MenuScreen::Worlds);
                }
                if theme::game_button(ui, "Settings", false, 260.0).clicked() {
                    menu.open_settings(MenuScreen::Main);
                }
                if !crate::core::projects::game_only() && theme::game_button(ui, "Studio", false, 260.0).clicked() {
                    action = MenuAction::Studio;
                }
                if theme::game_button(ui, "Quit", false, 260.0).clicked() {
                    action = MenuAction::Quit;
                }
            });

            // Player card on the right.
            let card = egui::Rect::from_min_size(pos2(content.right() - 260.0, content.top() + 10.0), vec2(240.0, 380.0));
            let mut ui = overlay_ui(ctx, "main_menu_player", card);
            theme::window_frame().show(&mut ui, |ui| {
                ui.set_width(card.width() - 36.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("PLAYER").size(12.0).color(theme::TEXT_DIM));
                    let (figure, _) = ui.allocate_exact_size(vec2(170.0, 250.0), egui::Sense::hover());
                    show_character(ctx, ui.painter(), figure, preview, character);
                    if theme::game_button(ui, "Customize", false, 180.0).clicked() {
                        menu.screen = Some(MenuScreen::Customize);
                    }
                });
            });
        }
    }
    action
}

fn worlds_screen(ctx: &egui::Context, content: egui::Rect, menu: &mut MenuState, browser: &mut WorldBrowser, project: &ProjectLayout, log: &mut EditorUiState) {
    let rect = egui::Rect::from_min_size(content.min, vec2(640.0_f32.min(content.width()), content.height()));
    let mut ui = overlay_ui(ctx, "worlds_screen", rect);
    theme::window_frame().show(&mut ui, |ui| {
        ui.set_width(rect.width() - 40.0);
        theme::window_title(ui, "Worlds", "");
        ui.add_space(8.0);
        let list_height = (rect.height() - 170.0).max(80.0);
        theme::wheel_scroll(ui, "worlds_list", list_height, |ui| {
            if browser.worlds.is_empty() {
                theme::empty_state(ui, "+", "No world yet: create one!");
            }
            let mut select = None;
            for (index, meta) in browser.worlds.iter().enumerate() {
                let selected = browser.selected == Some(index);
                let response = egui::Frame::NONE
                    .fill(if selected { Color32::from_rgb(22, 46, 48) } else { theme::BG_ELEVATED })
                    .stroke(Stroke::new(1.0, if selected { theme::ACCENT } else { theme::BORDER }))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(&meta.name).size(15.0).strong().color(Color32::WHITE));
                        ui.label(
                            RichText::new(format!(
                                "{}  -  {}  -  seed {}{}",
                                meta.kind.label(),
                                meta.mode.label(),
                                meta.seed,
                                if meta.caves && meta.kind == WorldKind::Infinite { "  -  caves" } else { "" }
                            ))
                            .size(12.0)
                            .color(theme::TEXT_DIM),
                        );
                    })
                    .response
                    .interact(egui::Sense::click());
                if response.clicked() {
                    select = Some(index);
                }
                if response.double_clicked() {
                    browser.launch = Some((meta.clone(), false));
                }
                ui.add_space(4.0);
            }
            if let Some(index) = select {
                browser.selected = Some(index);
                browser.confirm_delete = false;
            }
        });
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            let has = browser.selected.is_some();
            if ui.add_enabled_ui(has, |ui| theme::game_button(ui, "Play", true, 120.0)).inner.clicked() {
                if let Some(meta) = browser.selected.and_then(|i| browser.worlds.get(i)).cloned() {
                    browser.launch = Some((meta, false));
                }
            }
            if theme::game_small_button(ui, "+ Create a world", true).clicked() {
                browser.draft = WorldMeta {
                    name: format!("World {}", browser.worlds.len() + 1),
                    seed: WorldMeta::seed_from_text(&format!("{:.3}", ctx.input(|i| i.time))),
                    ..WorldMeta::default()
                };
                browser.seed_text = browser.draft.seed.to_string();
                menu.screen = Some(MenuScreen::CreateWorld);
            }
            if browser.confirm_delete {
                if theme::game_small_button(ui, "Confirm delete", has).clicked() {
                    if let Some(meta) = browser.selected.and_then(|i| browser.worlds.get(i)).cloned() {
                        meta.delete(project);
                    }
                    browser.confirm_delete = false;
                    browser.refresh(project);
                }
            } else if theme::game_small_button(ui, "Delete", has).clicked() {
                browser.confirm_delete = true;
            }
            if theme::game_small_button(ui, "Export", has).clicked() {
                if let Some(meta) = browser.selected.and_then(|i| browser.worlds.get(i)).cloned() {
                    match crate::editor::presets::export_world(project, &meta.stem()) {
                        Ok(path) => log.action(format!("World exported to {}.", path.display())),
                        Err(error) => log.warn(error),
                    }
                }
            }
            let importable = crate::editor::presets::importable_worlds(project);
            if !importable.is_empty() {
                egui::ComboBox::from_id_salt("import_world").selected_text("Import...").show_ui(ui, |ui| {
                    for stem in importable {
                        if ui.selectable_label(false, &stem).clicked() {
                            match crate::editor::presets::import_world(project, &stem) {
                                Ok(name) => log.action(format!("World '{name}' imported.")),
                                Err(error) => log.error(error),
                            }
                            browser.refresh(project);
                        }
                    }
                });
            }
            if theme::game_small_button(ui, "Back", true).clicked() {
                menu.screen = Some(MenuScreen::Main);
            }
        });
    });
}

fn create_screen(ctx: &egui::Context, content: egui::Rect, menu: &mut MenuState, browser: &mut WorldBrowser, biome_names: &[String]) {
    let rect = egui::Rect::from_min_size(content.min, vec2(640.0_f32.min(content.width()), content.height()));
    let mut ui = overlay_ui(ctx, "create_screen", rect);
    theme::window_frame().show(&mut ui, |ui| {
        ui.set_width(rect.width() - 40.0);
        theme::window_title(ui, "Create a world", "");
        theme::hint(ui, "The panorama behind shows the world live.");
        ui.add_space(8.0);
        let draft = &mut browser.draft;
        egui::Grid::new("create_world").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            field_label(ui, "Name");
            ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(300.0));
            ui.end_row();

            field_label(ui, "Seed");
            ui.horizontal(|ui| {
                if ui.add(egui::TextEdit::singleline(&mut browser.seed_text).desired_width(220.0)).changed() {
                    draft.seed = WorldMeta::seed_from_text(&browser.seed_text);
                }
                if theme::icon_button(ui, "?", "Random seed").clicked() {
                    draft.seed = WorldMeta::seed_from_text(&format!("{:.4}", ctx.input(|i| i.time)));
                    browser.seed_text = draft.seed.to_string();
                }
            });
            ui.end_row();

            field_label(ui, "Type");
            ui.horizontal(|ui| {
                for kind in WorldKind::ALL {
                    if theme::game_tab(ui, draft.kind == kind, kind.label()).clicked() {
                        draft.kind = kind;
                    }
                }
            });
            ui.end_row();

            field_label(ui, "Game mode");
            ui.horizontal(|ui| {
                for mode in [GameMode::Creative, GameMode::Survival] {
                    if theme::game_tab(ui, draft.mode == mode, mode.label()).clicked() {
                        draft.mode = mode;
                    }
                }
            });
            ui.end_row();

            if draft.kind == WorldKind::Infinite {
                field_label(ui, "Biomes");
                egui::ComboBox::from_id_salt("create_biome")
                    .selected_text(draft.biome.clone().unwrap_or_else(|| "All (climate)".to_string()))
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(draft.biome.is_none(), "All (climate)").clicked() {
                            draft.biome = None;
                        }
                        for name in biome_names {
                            if ui.selectable_label(draft.biome.as_deref() == Some(name), name).clicked() {
                                draft.biome = Some(name.clone());
                            }
                        }
                    });
                ui.end_row();
                field_label(ui, "Sea level");
                ui.add(egui::Slider::new(&mut draft.sea_level, -8..=32));
                ui.end_row();
                field_label(ui, "Caves");
                ui.checkbox(&mut draft.caves, "Caverns, tunnels, ores and lava lakes");
                ui.end_row();
            }
        });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            let valid = !browser.draft.name.trim().is_empty();
            if ui.add_enabled_ui(valid, |ui| theme::game_button(ui, "Create and play", true, 180.0)).inner.clicked() {
                browser.launch = Some((browser.draft.clone(), true));
            }
            if theme::game_small_button(ui, "Back", true).clicked() {
                menu.screen = Some(MenuScreen::Worlds);
            }
        });
    });
}

/// Live 3D character in `rect`; its head follows the mouse.
fn show_character(
    ctx: &egui::Context,
    painter: &egui::Painter,
    rect: egui::Rect,
    preview: &mut crate::gameplay::player_preview::MenuPreview,
    character: &mut crate::gameplay::avatar::MenuCharacter,
) {
    crate::gameplay::player_preview::paint_preview(painter, rect, preview.texture);
    painter.rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Outside);
    preview.rect = Some(rect);
    let screen = ctx.viewport_rect();
    character.look = ctx.input(|i| i.pointer.hover_pos()).map(|pointer| {
        let half = (screen.size() * 0.5).max(vec2(1.0, 1.0));
        let offset = pointer - rect.center();
        egui::Vec2::new(offset.x / half.x, offset.y / half.y).clamp(vec2(-1.0, -1.0), vec2(1.0, 1.0))
    }).map(|v| bevy::math::Vec2::new(v.x, v.y));
}

#[allow(clippy::too_many_arguments)]
fn customize_screen(
    ctx: &egui::Context,
    content: egui::Rect,
    menu: &mut MenuState,
    settings: &mut GameSettings,
    project: &ProjectLayout,
    log: &mut EditorUiState,
    preview: &mut crate::gameplay::player_preview::MenuPreview,
    character: &mut crate::gameplay::avatar::MenuCharacter,
    player_rig: &crate::gameplay::mobs::model::MobRig,
) {
    use crate::core::settings::{part_color_name, PART_PALETTE};
    let rect = egui::Rect::from_min_size(content.min, vec2(680.0_f32.min(content.width()), content.height().min(600.0)));
    let mut ui = overlay_ui(ctx, "customize_screen", rect);
    // Every box of the player model (studio, Mobs workspace, rig "Player"),
    // in tree order with its depth: torso, head, eyes, arm, hand...
    let boxes = rig_tree(player_rig);
    theme::window_frame().show(&mut ui, |ui| {
        ui.set_width(rect.width() - 40.0);
        theme::window_title(ui, "Customize", "Colour of each box of the player model");
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            let (figure, _) = ui.allocate_exact_size(vec2(200.0, 330.0), egui::Sense::hover());
            show_character(ctx, ui.painter(), figure, preview, character);
            ui.add_space(12.0);
            ui.vertical(|ui| {
                // One scroll for the colours, the buttons, the presets and the imports.
                theme::wheel_scroll(ui, "customize_scroll", rect.height() - 150.0, |ui| {
                    egui::Grid::new("part_colors").num_columns(3).spacing([10.0, 6.0]).show(ui, |ui| {
                        for (depth, bone) in &boxes {
                            let name = bone.name.clone();
                            ui.horizontal(|ui| {
                                ui.add_space(*depth as f32 * 14.0);
                                field_label(ui, &name);
                            });
                            let current = settings.part_colors.get(&name).copied();
                            let model = [
                                (bone.color[0].clamp(0.0, 1.0) * 255.0) as u8,
                                (bone.color[1].clamp(0.0, 1.0) * 255.0) as u8,
                                (bone.color[2].clamp(0.0, 1.0) * 255.0) as u8,
                            ];
                            let mut changed: Option<Option<[u8; 3]>> = None;
                            egui::ComboBox::from_id_salt(("part_color", name.as_str()))
                                .width(130.0)
                                .selected_text(part_color_name(current))
                                .show_ui(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let (swatch, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                                        ui.painter().rect_filled(swatch, CornerRadius::same(4), Color32::from_rgb(model[0], model[1], model[2]));
                                        if ui.selectable_label(current.is_none(), "Default").clicked() {
                                            changed = Some(None);
                                        }
                                    });
                                    for (label, rgb) in PART_PALETTE {
                                        ui.horizontal(|ui| {
                                            let (swatch, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                                            ui.painter().rect_filled(swatch, CornerRadius::same(4), Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                                            if ui.selectable_label(current == Some(rgb), label).clicked() {
                                                changed = Some(Some(rgb));
                                            }
                                        });
                                    }
                                });
                            // Free colour next to the list.
                            let mut rgb = current.unwrap_or(model);
                            if ui.color_edit_button_srgb(&mut rgb).changed() {
                                changed = Some(Some(rgb));
                            }
                            if let Some(color) = changed {
                                match color {
                                    Some(rgb) => {
                                        settings.part_colors.insert(name.clone(), rgb);
                                    }
                                    None => {
                                        settings.part_colors.remove(&name);
                                    }
                                }
                                settings.touch();
                            }
                            ui.end_row();
                        }
                    });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if theme::game_small_button(ui, "All default", true).clicked() {
                        settings.part_colors.clear();
                        settings.touch();
                    }
                    let first = boxes.first().map(|(_, bone)| bone.name.clone());
                    let base = first.as_ref().and_then(|name| settings.part_colors.get(name).copied());
                    if theme::game_small_button(ui, "Body like the root box", base.is_some()).clicked() {
                        if let Some(rgb) = base {
                            // Every box keeps its model shade relation except dark details (eyes...).
                            for (_, bone) in &boxes {
                                let shade = (bone.color[0] + bone.color[1] + bone.color[2]) / 3.0;
                                if shade > 0.2 {
                                    settings.part_colors.insert(bone.name.clone(), rgb);
                                }
                            }
                            settings.touch();
                        }
                    }
                });
                theme::hint(ui, "Default keeps the colour of the player model; its boxes are edited in the studio (Mobs workspace, rig \"Player\").");
                ui.add_space(8.0);
                presets_section(ui, settings, project, log);
                });
            });
        });
        ui.add_space(10.0);
        if theme::game_button(ui, "Save and back", true, 180.0).clicked() {
            match settings.save(project) {
                Ok(()) => log.action("Character saved."),
                Err(error) => log.error(error),
            }
            menu.screen = Some(MenuScreen::Main);
        }
    });
}

/// Customisation presets (save / load / delete) and import / export.
fn presets_section(ui: &mut egui::Ui, settings: &mut GameSettings, project: &ProjectLayout, log: &mut EditorUiState) {
    use crate::editor::presets;
    theme::section_label(ui, "PRESETS");
    let name_id = egui::Id::new("preset_name");
    let mut name: String = ui.ctx().data_mut(|d| d.get_temp::<String>(name_id)).unwrap_or_else(|| "My character".to_string());
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut name).desired_width(150.0));
        if theme::game_small_button(ui, "Save preset", true).clicked() {
            match presets::save_preset(project, &name, &settings.part_colors) {
                Ok(stem) => log.action(format!("Preset '{stem}' saved.")),
                Err(error) => log.warn(error),
            }
        }
        if theme::game_small_button(ui, "Export", true).clicked() {
            match presets::export_custom(project, &name, &settings.part_colors) {
                Ok(path) => log.action(format!("Customisation exported to {}.", path.display())),
                Err(error) => log.error(error),
            }
        }
        // Import the export named in the field (the list below has them all).
        if theme::game_small_button(ui, "Import", true).clicked() {
            let stem = crate::voxel::biomes::file_stem(&name);
            match presets::import_custom(project, &stem) {
                Ok(colors) => {
                    settings.part_colors = colors;
                    settings.touch();
                    log.action(format!("Customisation '{stem}' imported."));
                }
                Err(_) => log.warn(format!("No export named '{stem}' in {}.", presets::exports_dir(project).display())),
            }
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(name_id, name));
    for preset in presets::preset_names(project) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&preset).size(13.0).color(theme::TEXT));
            if theme::game_small_button(ui, "Load", true).clicked() {
                match presets::load_preset(project, &preset) {
                    Ok(colors) => {
                        settings.part_colors = colors;
                        settings.touch();
                    }
                    Err(error) => log.error(error),
                }
            }
            if theme::game_small_button(ui, "Delete", true).clicked() {
                presets::delete_preset(project, &preset);
            }
        });
    }
    let imports = presets::importable_customs(project);
    theme::section_label(ui, "IMPORT");
    if imports.is_empty() {
        theme::hint(
            ui,
            &format!(
                "No customisation to import: copy a .custom.ron file (made with Export) into {}.",
                presets::exports_dir(project).display()
            ),
        );
    }
    {
        for import in imports {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&import).size(13.0).color(theme::TEXT));
                if theme::game_small_button(ui, "Import", true).clicked() {
                    match presets::import_custom(project, &import) {
                        Ok(colors) => {
                            settings.part_colors = colors;
                            settings.touch();
                            log.action(format!("Customisation '{import}' imported."));
                        }
                        Err(error) => log.error(error),
                    }
                }
            });
        }
    }
}

/// Pause menu drawn over the game. Returns the chosen action.
pub fn draw_pause_menu(
    ctx: &egui::Context,
    menu: &mut MenuState,
    settings: &mut GameSettings,
    rebind: &mut RebindRequest,
    project: &ProjectLayout,
    log: &mut EditorUiState,
    saves_world: bool,
) -> MenuAction {
    let screen = ctx.viewport_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("pause_dim")));
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha(150));

    let mut action = MenuAction::None;
    if menu.screen == Some(MenuScreen::Settings) {
        settings_window(ctx, menu, settings, rebind, project, log);
        return action;
    }
    let mut ui = overlay_ui(ctx, "pause_menu", egui::Rect::from_center_size(screen.center(), vec2(330.0, 420.0)));
    theme::window_frame().show(&mut ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Paused").size(24.0).strong().color(Color32::WHITE));
        });
        ui.add_space(10.0);
        ui.spacing_mut().item_spacing.y = 10.0;
        if theme::game_button(ui, "Resume", true, 290.0).clicked() {
            menu.screen = None;
        }
        if theme::game_button(ui, "Settings", false, 290.0).clicked() {
            menu.open_settings(MenuScreen::Pause);
        }
        let studio = if saves_world { "Save and open the studio" } else { "Open the studio" };
        if !crate::core::projects::game_only() && theme::game_button(ui, studio, false, 290.0).clicked() {
            action = MenuAction::StopToStudio;
        }
        let main = if saves_world { "Save and quit to menu" } else { "Quit to menu" };
        if theme::game_button(ui, main, false, 290.0).clicked() {
            action = MenuAction::StopToMainMenu;
        }
        if theme::game_button(ui, if saves_world { "Save and quit the game" } else { "Quit the game" }, false, 290.0).clicked() {
            action = MenuAction::Quit;
        }
    });
    action
}

fn settings_window(
    ctx: &egui::Context,
    menu: &mut MenuState,
    settings: &mut GameSettings,
    rebind: &mut RebindRequest,
    project: &ProjectLayout,
    log: &mut EditorUiState,
) {
    let screen = ctx.viewport_rect();
    let size = vec2(660.0_f32.min(screen.width() - 40.0), 560.0_f32.min(screen.height() - 40.0));
    let mut ui = overlay_ui(ctx, "settings_window", egui::Rect::from_center_size(screen.center(), size + vec2(40.0, 40.0)));
    theme::window_frame().show(&mut ui, |ui| {
        ui.set_width(size.x);
        ui.set_height(size.y);
        ui.horizontal(|ui| {
            theme::window_title(ui, "Settings", "");
            ui.add_space(16.0);
            for (tab, label) in [
                (SettingsTab::Game, "Game"),
                (SettingsTab::Interface, "Interface"),
                (SettingsTab::Sound, "Sound"),
                (SettingsTab::Controls, "Controls"),
            ] {
                if theme::game_tab(ui, menu.tab == tab, label).clicked() {
                    menu.tab = tab;
                }
            }
        });
        ui.separator();
        let footer = 50.0;
        let height = ui.available_height() - footer;
        theme::wheel_scroll(ui, "settings_scroll", height, |ui| {
            let changed = match menu.tab {
                SettingsTab::Game => game_tab(ui, settings),
                SettingsTab::Interface => interface_tab(ui, settings),
                SettingsTab::Sound => sound_tab(ui, settings),
                SettingsTab::Controls => controls_tab(ui, settings, rebind),
            };
            if changed {
                settings.touch();
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if theme::game_button(ui, "Save and back", true, 170.0).clicked() {
                match settings.save(project) {
                    Ok(()) => log.action("Settings saved."),
                    Err(error) => log.error(error),
                }
                rebind.pending = None;
                menu.screen = menu.settings_return.or(Some(MenuScreen::Main));
            }
            if theme::game_small_button(ui, "Reset to defaults", true).clicked() {
                let keep_menu = settings.show_main_menu;
                let revision = settings.revision;
                *settings = GameSettings {
                    show_main_menu: keep_menu,
                    revision,
                    ..GameSettings::default()
                };
                settings.touch();
            }
        });
    });
}

fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized(vec2(240.0, 24.0), egui::Label::new(RichText::new(label).size(14.0).color(theme::TEXT)));
        changed = add(ui);
    });
    changed
}

fn game_tab(ui: &mut egui::Ui, settings: &mut GameSettings) -> bool {
    let mut changed = false;
    changed |= row(ui, "Mouse sensitivity", |ui| ui.add(egui::Slider::new(&mut settings.mouse_sensitivity, 0.1..=4.0).logarithmic(true)).changed());
    changed |= row(ui, "Invert vertical look", |ui| ui.checkbox(&mut settings.invert_y, "").changed());
    changed |= row(ui, "Field of view", |ui| ui.add(egui::Slider::new(&mut settings.fov_degrees, 50.0..=110.0).suffix(" deg")).changed());
    changed |= row(ui, "View distance (chunks)", |ui| ui.add(egui::Slider::new(&mut settings.view_distance, 2..=16)).changed());
    changed |= row(ui, "View bobbing", |ui| ui.checkbox(&mut settings.head_bob, "").changed());
    changed |= row(ui, "Main menu at startup", |ui| ui.checkbox(&mut settings.show_main_menu, "").changed());
    changed
}

fn interface_tab(ui: &mut egui::Ui, settings: &mut GameSettings) -> bool {
    let mut changed = false;
    changed |= row(ui, "Player preview (top right)", |ui| ui.checkbox(&mut settings.show_player_preview, "").changed());
    changed |= row(ui, "Minimap (top right)", |ui| ui.checkbox(&mut settings.show_minimap, "").changed());
    changed |= row(ui, "Show FPS", |ui| ui.checkbox(&mut settings.show_fps, "").changed());
    changed
}

fn sound_tab(ui: &mut egui::Ui, settings: &mut GameSettings) -> bool {
    let mut changed = false;
    changed |= row(ui, "Master volume", |ui| ui.add(egui::Slider::new(&mut settings.master_volume, 0.0..=1.0)).changed());
    changed |= row(ui, "Sound effects", |ui| ui.add(egui::Slider::new(&mut settings.sfx_volume, 0.0..=1.0)).changed());
    changed
}

fn controls_tab(ui: &mut egui::Ui, settings: &mut GameSettings, rebind: &mut RebindRequest) -> bool {
    let mut changed = false;
    theme::hint(ui, "Click a key, then press the new one (Esc cancels, Backspace clears).");
    ui.add_space(6.0);
    egui::Grid::new("bindings_grid").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label(RichText::new("Action").strong());
        ui.label(RichText::new("Primary").strong());
        ui.label(RichText::new("Secondary").strong());
        ui.end_row();
        for action in GameAction::ALL {
            ui.label(RichText::new(action.label()).size(14.0));
            let binding = settings.bindings.get(action).copied();
            for slot in 0..2 {
                let current = binding.and_then(|b| if slot == 0 { b.primary } else { b.secondary });
                let waiting = rebind.pending == Some((action, slot));
                let text = if waiting { "press a key...".to_string() } else { current.map(|b| b.display()).unwrap_or_else(|| "-".to_string()) };
                let button = egui::Button::new(RichText::new(text).size(13.0).color(if waiting { theme::WARN } else { theme::TEXT }))
                    .min_size(vec2(130.0, 28.0))
                    .corner_radius(CornerRadius::same(10));
                if ui.add(button).clicked() {
                    rebind.pending = Some((action, slot));
                }
            }
            ui.end_row();
        }
    });
    ui.add_space(8.0);
    if theme::game_small_button(ui, "Default keys", true).clicked() {
        settings.bindings = KeyBindings::default();
        changed = true;
    }
    theme::hint(
        ui,
        &format!(
            "Also: 1-9 hotbar, {} pause, F7 leaves the game. Creative: double-tap Jump to fly.",
            settings.bindings.describe(GameAction::Pause)
        ),
    );
    changed
}

/// Apply a menu action to the editor state.
pub fn apply_action(
    action: MenuAction,
    menu: &mut MenuState,
    editor: &mut EditorState,
    ui: &mut EditorUiState,
    pie: &mut PieState,
    quit: &mut QuitRequest,
) {
    match action {
        MenuAction::None => {}
        MenuAction::Studio => {
            menu.screen = None;
            editor.workspace = Workspace::World;
        }
        MenuAction::StopToStudio => {
            menu.screen = None;
            pie.stop(editor, ui);
        }
        MenuAction::StopToMainMenu => {
            pie.stop(editor, ui);
            menu.screen = Some(MenuScreen::Main);
        }
        MenuAction::Quit => {
            // Stop play first so the world is saved, then close.
            if pie.is_playing() {
                pie.stop(editor, ui);
            }
            menu.screen = None;
            quit.frames = Some(3);
        }
    }
}

/// Boxes of `rig` in tree order (parents first), with their depth.
fn rig_tree(rig: &crate::gameplay::mobs::model::MobRig) -> Vec<(usize, &crate::gameplay::mobs::model::MobBox)> {
    fn visit<'a>(
        rig: &'a crate::gameplay::mobs::model::MobRig,
        parent: Option<&str>,
        depth: usize,
        out: &mut Vec<(usize, &'a crate::gameplay::mobs::model::MobBox)>,
    ) {
        for bone in rig.boxes.iter().filter(|bone| bone.parent.as_deref() == parent) {
            if out.iter().any(|(_, b)| b.name == bone.name) || depth > 32 {
                continue;
            }
            out.push((depth, bone));
            visit(rig, Some(&bone.name), depth + 1, out);
        }
    }
    let mut out = Vec::new();
    visit(rig, None, 0, &mut out);
    // Boxes whose parent is missing still get a row.
    for bone in &rig.boxes {
        if !out.iter().any(|(_, b)| b.name == bone.name) {
            out.push((0, bone));
        }
    }
    out
}
