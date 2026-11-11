//! 🎥 3D viewport panel.
//!
//! The tab body is transparent: the Bevy camera renders directly behind it and
//! the camera viewport follows the rectangle published here. Overlays are kept
//! minimal: a few rounded chips and, in the 🐾 Mobs workspace, a floating tool
//! bar (mode, gizmo, snapping, framing).

use egui::{Align2, Color32, FontId, Rect, Sense, pos2, vec2};

use crate::core::app_state::{EditorState, EditorUiState, Workspace};
use crate::core::pie_manager::PieState;
use crate::editor::biome_studio::BiomeStudio;
use crate::editor::block_studio::{BlockStudio, ForgeGizmo, ForgePreview};
use crate::editor::mob_studio::{GizmoMode, MobEditMode, MobStudio};
use crate::voxel::biomes::BiomeLibrary;
use crate::editor::theme;
use crate::gameplay::mobs::library::MobLibrary;
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::picking::VoxelCursor;
use crate::voxel::world::VoxelWorld;

/// Rounded translucent chip with text, returns its rect.
fn chip(painter: &egui::Painter, anchor: egui::Pos2, align: Align2, text: &str, color: Color32) -> Rect {
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(12.0), color);
    let size = galley.size() + vec2(18.0, 10.0);
    let rect = align.anchor_size(anchor, size);
    painter.rect_filled(rect, 12.0, Color32::from_black_alpha(140));
    painter.galley(rect.min + vec2(9.0, 5.0), galley, color);
    rect
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    state: &mut EditorUiState,
    editor: &EditorState,
    pie: &PieState,
    world: &VoxelWorld,
    cursor: &VoxelCursor,
    registry: &BlockRegistry,
    mobs: &mut MobLibrary,
    studio: &mut MobStudio,
    forge: &mut BlockStudio,
    terraform: &mut BiomeStudio,
    biomes: &BiomeLibrary,
    structure_studio: &mut crate::editor::structure_studio::StructureStudio,
    item_studio: &mut crate::editor::item_studio::ItemStudio,
) {
    let rect = ui.max_rect();
    state.viewport_rect = Some(rect);
    state.pixels_per_point = ui.ctx().pixels_per_point();

    let response = ui.interact(rect, ui.id().with("viewport_canvas"), Sense::click_and_drag());
    state.viewport_hovered =
        response.hovered() || studio.is_dragging() || forge.is_dragging() || item_studio.is_dragging();

    match editor.workspace {
        Workspace::Mobs => mob_overlay(ui, rect, mobs, studio, state),
        Workspace::Blocks => forge_overlay(ui, rect, forge, state),
        Workspace::Biomes => terraform_overlay(ui, rect, terraform, biomes, state),
        Workspace::Structures => structure_overlay(ui, rect, structure_studio, registry, editor, state),
        Workspace::Items => item_overlay(ui, rect, item_studio, state),
        _ => world_overlay(ui, rect, state, editor, pie, world, cursor, registry),
    }
}

/// Floating rounded tool bar centred at the top of the viewport.
fn tool_bar(ui: &mut egui::Ui, rect: Rect, width: f32, state: &mut EditorUiState, add: impl FnOnce(&mut egui::Ui)) {
    let bar = Rect::from_center_size(rect.center_top() + vec2(0.0, 28.0), vec2(width, 40.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
    egui::Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(15, 21, 29, 230))
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(16)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(&mut child, |ui| {
            ui.horizontal(add);
        });
    if ui.rect_contains_pointer(bar) {
        state.viewport_hovered = false;
    }
}

fn forge_overlay(ui: &mut egui::Ui, rect: Rect, forge: &mut BlockStudio, state: &mut EditorUiState) {
    tool_bar(ui, rect, 470.0, state, |ui| {
        for mode in ForgeGizmo::ALL {
            if theme::toggle_button(ui, forge.gizmo == mode, mode.icon(), mode.label()).clicked() {
                forge.gizmo = mode;
            }
        }
        ui.separator();
        for preview in ForgePreview::ALL {
            if theme::toggle_button(ui, forge.preview == preview, preview.icon(), preview.label()).clicked() {
                forge.preview = preview;
                forge.frame_request = true;
            }
        }
        ui.separator();
        if theme::toggle_button(ui, forge.snap, "⌖", "Snap 1/16 (Ctrl to disable)").clicked() {
            forge.snap = !forge.snap;
        }
        if theme::toggle_button(ui, forge.show_grid, "📐", "16x16 grid").clicked() {
            forge.show_grid = !forge.show_grid;
        }
        if theme::icon_button(ui, "➕", "Add an element to the model").clicked() {
            forge.add_element();
        }
        if theme::icon_button(ui, "🎯", "Frame (F)").clicked() {
            forge.frame_request = true;
        }
    });

    let painter = ui.painter_at(rect);
    let def = &forge.draft;
    let kind = if def.is_liquid() {
        "💧 liquid".to_string()
    } else if let Some(model) = def.custom_model() {
        format!("📦 model - {} element(s)", model.elements.len())
    } else {
        def.shape.label().to_string()
    };
    chip(&painter, rect.left_top() + vec2(12.0, 12.0), Align2::LEFT_TOP, &format!("🔨 {}  -  {kind}", def.label), theme::TEXT);
    let facets = if def.normal.enabled {
        format!("💠 facets {:.2} - {:.1}/block", def.normal.strength, def.normal.cells)
    } else {
        "💠 facets off".to_string()
    };
    chip(&painter, rect.left_top() + vec2(12.0, 44.0), Align2::LEFT_TOP, &facets, theme::ACCENT);
    if forge.dirty {
        chip(&painter, rect.right_top() + vec2(-12.0, 12.0), Align2::RIGHT_TOP, "● not applied", theme::WARN);
    }
    if let Some(element) = forge.element.and_then(|index| forge.elements().get(index)) {
        chip(
            &painter,
            rect.right_top() + vec2(-12.0, 44.0),
            Align2::RIGHT_TOP,
            &format!(
                "📦 {}  [{:.1}, {:.1}, {:.1}] -> [{:.1}, {:.1}, {:.1}]",
                element.name, element.from[0], element.from[1], element.from[2], element.to[0], element.to[1], element.to[2]
            ),
            theme::WARN,
        );
    }
    chip(
        &painter,
        rect.left_bottom() + vec2(12.0, -12.0),
        Align2::LEFT_BOTTOM,
        "🖱 Click: pick an element   G/S: gizmo   Del: erase   Right click: orbit   Wheel: zoom",
        theme::TEXT_DIM,
    );
}

fn structure_overlay(
    ui: &mut egui::Ui,
    rect: Rect,
    studio: &mut crate::editor::structure_studio::StructureStudio,
    registry: &BlockRegistry,
    editor: &EditorState,
    state: &mut EditorUiState,
) {
    use crate::editor::structure_studio::StructureTool;
    tool_bar(ui, rect, 300.0, state, |ui| {
        for tool in StructureTool::ALL {
            if theme::toggle_button(ui, studio.tool == tool, tool.icon(), tool.label()).clicked() {
                studio.tool = tool;
            }
        }
        ui.separator();
        if theme::icon_button(ui, "🎯", "Frame (F)").clicked() {
            studio.frame_request = true;
        }
    });
    let painter = ui.painter_at(rect);
    let brush = registry.get(editor.selected_block);
    chip(&painter, rect.left_top() + vec2(12.0, 12.0), Align2::LEFT_TOP, &format!("🔨 Brush: {}", brush.label), theme::TEXT);
    if studio.dirty {
        chip(&painter, rect.right_top() + vec2(-12.0, 12.0), Align2::RIGHT_TOP, "● not saved", theme::WARN);
    }
    chip(
        &painter,
        rect.left_bottom() + vec2(12.0, -12.0),
        Align2::LEFT_BOTTOM,
        "🖱 Click: tool   Shift+click: erase   B/X/A/I: tools   Right click: orbit   Wheel: zoom",
        theme::TEXT_DIM,
    );
}

fn item_overlay(ui: &mut egui::Ui, rect: Rect, studio: &mut crate::editor::item_studio::ItemStudio, state: &mut EditorUiState) {
    tool_bar(ui, rect, 220.0, state, |ui| {
        for gizmo in ForgeGizmo::ALL {
            if theme::toggle_button(ui, studio.gizmo == gizmo, gizmo.icon(), gizmo.label()).clicked() {
                studio.gizmo = gizmo;
            }
        }
        if theme::toggle_button(ui, studio.snap, "⌖", "Snap 1/16").clicked() {
            studio.snap = !studio.snap;
        }
        if theme::icon_button(ui, "🎯", "Frame (F)").clicked() {
            studio.frame_request = true;
        }
    });
    let painter = ui.painter_at(rect);
    chip(
        &painter,
        rect.left_bottom() + vec2(12.0, -12.0),
        Align2::LEFT_BOTTOM,
        "🖱 Click: pick a box   G/S: gizmo   Del: erase   Right click: orbit   Wheel: zoom",
        theme::TEXT_DIM,
    );
}

fn terraform_overlay(
    ui: &mut egui::Ui,
    rect: Rect,
    terraform: &mut BiomeStudio,
    biomes: &BiomeLibrary,
    state: &mut EditorUiState,
) {
    tool_bar(ui, rect, 430.0, state, |ui| {
        ui.label(egui::RichText::new("Size").size(12.0).color(theme::TEXT_DIM));
        for size in [2, 3, 4, 6] {
            let label = format!("{}x{}", size * 16, size * 16);
            if theme::toggle_button(ui, terraform.size == size, &label, "Preview size in blocks").clicked() {
                terraform.size = size;
                terraform.frame_request = true;
            }
        }
        ui.separator();
        if theme::icon_button(ui, "🎲", "Another place in the biome").clicked() {
            terraform.reroll();
        }
        if theme::icon_button(ui, "⟳", "Regenerate").clicked() {
            terraform.rebuild_request = true;
        }
        if theme::icon_button(ui, "🎯", "Frame (F)").clicked() {
            terraform.frame_request = true;
        }
    });

    let painter = ui.painter_at(rect);
    if let Some(biome) = biomes.biomes.get(biomes.selection) {
        chip(&painter, rect.left_top() + vec2(12.0, 12.0), Align2::LEFT_TOP, &format!("🌲 {}", biome.name), theme::TEXT);
        chip(
            &painter,
            rect.left_top() + vec2(12.0, 44.0),
            Align2::LEFT_TOP,
            &format!(
                "{} - T {:.2} - H {:.2} - relief {:.0}±{:.0}",
                biome.feature.label(),
                biome.temperature,
                biome.humidity,
                biome.base_height,
                biome.height_scale
            ),
            theme::ACCENT,
        );
    }
    chip(
        &painter,
        rect.right_top() + vec2(-12.0, 12.0),
        Align2::RIGHT_TOP,
        &format!(
            "🏰 {} structure(s)  -  {} voxels  -  {:.0} ms",
            terraform.last_structures, terraform.last_voxels, terraform.last_millis
        ),
        theme::WARN,
    );
    chip(
        &painter,
        rect.left_bottom() + vec2(12.0, -12.0),
        Align2::LEFT_BOTTOM,
        "🖱 Right click: orbit   Middle click: pan   Wheel: zoom   - the preview follows your settings",
        theme::TEXT_DIM,
    );
}

#[allow(clippy::too_many_arguments)]
fn world_overlay(
    ui: &mut egui::Ui,
    rect: Rect,
    state: &EditorUiState,
    editor: &EditorState,
    pie: &PieState,
    world: &VoxelWorld,
    cursor: &VoxelCursor,
    registry: &BlockRegistry,
) {
    let painter = ui.painter_at(rect);
    let top_left = rect.left_top() + vec2(12.0, 12.0);

    // What the brush will place, and what is under the cursor.
    let selected = registry.get(editor.selected_block);
    let mut y = chip(
        &painter,
        top_left,
        Align2::LEFT_TOP,
        &format!("🔨 {}  -  {}", selected.label, selected.shape.label()),
        theme::TEXT,
    )
    .bottom()
        + 6.0;
    if let Some(hit) = cursor.target {
        let def = registry.get(hit.block);
        y = chip(
            &painter,
            pos2(top_left.x, y),
            Align2::LEFT_TOP,
            &format!("🎯 {}  ({}, {}, {})", def.label, hit.voxel.x, hit.voxel.y, hit.voxel.z),
            theme::WARN,
        )
        .bottom()
            + 6.0;
    }
    if state.cable_tool {
        chip(&painter, pos2(top_left.x, y), Align2::LEFT_TOP, "🔌 Cable tool active", Color32::from_rgb(255, 210, 120));
    }
    if pie.is_playing() {
        chip(&painter, rect.center_top() + vec2(0.0, 12.0), Align2::CENTER_TOP, "▶ In game", theme::WARN);
    }

    // Saved state + FPS.
    let saved = world.revision == state.last_saved_revision;
    chip(
        &painter,
        rect.right_top() + vec2(-12.0, 12.0),
        Align2::RIGHT_TOP,
        &format!("{}  {}  -  {:.0} fps", if saved { "✔" } else { "●" }, state.world_name, state.fps),
        if saved { theme::ACCENT } else { theme::WARN },
    );

    if state.show_stats {
        let stats = world.stats();
        chip(
            &painter,
            rect.right_bottom() + vec2(-12.0, -12.0),
            Align2::RIGHT_BOTTOM,
            &format!("📦 {} chunks  -  {} voxels", stats.chunks, stats.voxels),
            theme::TEXT_DIM,
        );
    }

    if ui.rect_contains_pointer(rect) {
        chip(
            &painter,
            rect.left_bottom() + vec2(12.0, -12.0),
            Align2::LEFT_BOTTOM,
            "🖱 Click: place   Shift+click: erase   Right click + WASD: fly   R/T: orient",
            theme::TEXT_DIM,
        );
    }
}

fn mob_overlay(ui: &mut egui::Ui, rect: Rect, mobs: &mut MobLibrary, studio: &mut MobStudio, state: &mut EditorUiState) {
    // Floating tool bar, top centre.
    let bar = Rect::from_center_size(rect.center_top() + vec2(0.0, 28.0), vec2(520.0, 40.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
    egui::Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(15, 21, 29, 230))
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(16)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(&mut child, |ui| {
            ui.horizontal(|ui| {
                if theme::toggle_button(ui, studio.edit == MobEditMode::Model, "📦 Model", "Edit the cubes (Tab)").clicked() {
                    studio.edit = MobEditMode::Model;
                }
                if theme::toggle_button(ui, studio.edit == MobEditMode::Animate, "🎬 Animation", "Place keys on the timeline (Tab)").clicked() {
                    studio.edit = MobEditMode::Animate;
                }
                ui.separator();
                for mode in GizmoMode::ALL {
                    if studio.edit == MobEditMode::Animate && mode == GizmoMode::Pivot {
                        continue;
                    }
                    if theme::toggle_button(ui, studio.gizmo == mode, mode.icon(), mode.label()).clicked() {
                        studio.gizmo = mode;
                    }
                }
                ui.separator();
                if theme::toggle_button(ui, studio.snap, "⌖", "Snap 1/16 block and 15 deg (Ctrl to disable)").clicked() {
                    studio.snap = !studio.snap;
                }
                if theme::toggle_button(ui, studio.show_grid, "📐", "Ground grid").clicked() {
                    studio.show_grid = !studio.show_grid;
                }
                if theme::icon_button(ui, "🎯", "Frame the mob (F)").clicked() {
                    studio.frame_request = true;
                }
            });
        });
    // The bar must not count as viewport hover (no gizmo / selection under it).
    if ui.rect_contains_pointer(bar) {
        state.viewport_hovered = false;
    }

    let painter = ui.painter_at(rect);
    let name = mobs.current().map(|rig| rig.name.clone()).unwrap_or_else(|| "-".into());
    let mode = match studio.edit {
        MobEditMode::Model => "📦 Model".to_string(),
        MobEditMode::Animate => format!("🎬 {}  -  {:.2} s", mobs.clip_name(), mobs.preview.time),
    };
    chip(&painter, rect.left_top() + vec2(12.0, 12.0), Align2::LEFT_TOP, &format!("🐾 {name}"), theme::TEXT);
    chip(&painter, rect.left_top() + vec2(12.0, 44.0), Align2::LEFT_TOP, &mode, theme::ACCENT);
    if let Some(bone) = &mobs.bone_selection {
        chip(&painter, rect.right_top() + vec2(-12.0, 12.0), Align2::RIGHT_TOP, &format!("📦 {bone}"), theme::WARN);
    }
    chip(
        &painter,
        rect.left_bottom() + vec2(12.0, -12.0),
        Align2::LEFT_BOTTOM,
        "🖱 Click: select / gizmo   Right click: orbit   Wheel: zoom   Middle click: pan",
        theme::TEXT_DIM,
    );
}
