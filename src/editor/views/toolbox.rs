//! 🔧 Outils: visual block palette and placement options.
//!
//! Blocks are shown as coloured tiles with their shape glyph; a search box and
//! category chips filter them. Selection is stored in [`EditorState`] so the
//! brush, the inspector and the viewport chip stay in sync.

use egui::{Align2, Color32, FontId, RichText, Sense, Stroke, vec2};

use crate::core::app_state::{EditorState, EditorUiState};
use crate::editor::theme;
use crate::voxel::blocks::{BlockDef, BlockFunction, BlockRegistry};
use crate::voxel::generator;
use crate::voxel::shapes::{Orientation, VoxelShape};
use crate::voxel::world::VoxelWorld;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Category {
    #[default]
    All,
    Terrain,
    Shapes,
    Machines,
}

impl Category {
    const ALL: [Self; 4] = [Self::All, Self::Terrain, Self::Shapes, Self::Machines];

    fn label(self) -> &'static str {
        match self {
            Self::All => "✳ All",
            Self::Terrain => "🌱 Terrain",
            Self::Shapes => "🔷 Shapes",
            Self::Machines => "⚙ Machines",
        }
    }

    fn accepts(self, def: &BlockDef) -> bool {
        match self {
            Self::All => true,
            Self::Terrain => def.function == BlockFunction::None && def.shape == VoxelShape::Cube,
            Self::Shapes => def.shape != VoxelShape::Cube && def.function == BlockFunction::None,
            Self::Machines => def.function != BlockFunction::None,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct PaletteFilter {
    search: String,
    category: Category,
}

/// Glyph drawn on a tile for each shape.
pub fn shape_glyph(shape: VoxelShape) -> &'static str {
    match shape {
        VoxelShape::Cube => "■",
        VoxelShape::HHalfCube => "▄",
        VoxelShape::VHalfCube => "▌",
        VoxelShape::Stairs => "▟",
        VoxelShape::StairsInCorner => "▙",
        VoxelShape::StairsOutCorner => "▗",
        VoxelShape::Ramp => "◢",
        VoxelShape::RampInCorner => "◣",
        VoxelShape::RampOutCorner => "◤",
    }
}

pub fn show(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    state: &mut EditorUiState,
    world: &mut VoxelWorld,
    registry: &BlockRegistry,
) {
    let id = egui::Id::new("polyhedron_palette_filter");
    let mut filter: PaletteFilter = ui.ctx().data_mut(|d| d.get_temp(id)).unwrap_or_default();

    egui::ScrollArea::vertical()
        .id_salt("polyhedron_toolbox_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Search + categories.
            ui.add(
                egui::TextEdit::singleline(&mut filter.search)
                    .hint_text("🔍 Search a block...")
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal_wrapped(|ui| {
                for category in Category::ALL {
                    if theme::toggle_button(ui, filter.category == category, category.label(), "Filter").clicked() {
                        filter.category = category;
                    }
                }
            });
            ui.add_space(4.0);

            // Tile grid.
            let search = filter.search.to_lowercase();
            let tile = 58.0;
            let columns = ((ui.available_width() + 6.0) / (tile + 6.0)).floor().max(1.0) as usize;
            let blocks: Vec<_> = registry
                .blocks()
                .filter(|(_, def)| def.variant_of.is_none())
                .filter(|(_, def)| filter.category.accepts(def))
                .filter(|(_, def)| search.is_empty() || def.label.to_lowercase().contains(&search) || def.id.contains(&search))
                .collect();
            if blocks.is_empty() {
                theme::empty_state(ui, "🔍", "No block matches");
            }
            for row in blocks.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (block, def) in row {
                        let selected = *block == editor.selected_block;
                        let (rect, response) = ui.allocate_exact_size(vec2(tile, tile + 14.0), Sense::click());
                        let painter = ui.painter_at(rect);
                        let swatch = egui::Rect::from_min_size(rect.min, vec2(tile, tile));
                        let color = Color32::from_rgb(def.color[0], def.color[1], def.color[2]);
                        painter.rect_filled(swatch, 10.0, if response.hovered() { theme::BG_HOVER } else { theme::BG_ELEVATED });
                        painter.rect_filled(swatch.shrink(8.0), 7.0, color);
                        painter.text(
                            swatch.center(),
                            Align2::CENTER_CENTER,
                            shape_glyph(def.shape),
                            FontId::proportional(20.0),
                            Color32::from_black_alpha(110),
                        );
                        if def.function != BlockFunction::None {
                            painter.text(swatch.right_top() + vec2(-4.0, 3.0), Align2::RIGHT_TOP, "⚙", FontId::proportional(11.0), Color32::WHITE);
                        }
                        if def.emissive > 0.0 {
                            painter.text(swatch.left_top() + vec2(4.0, 3.0), Align2::LEFT_TOP, "💡", FontId::proportional(10.0), Color32::WHITE);
                        }
                        if selected {
                            painter.rect_stroke(swatch, 10.0, Stroke::new(2.5, theme::ACCENT), egui::StrokeKind::Inside);
                        }
                        let label: String = def.label.chars().take(9).collect();
                        painter.text(
                            rect.center_bottom(),
                            Align2::CENTER_BOTTOM,
                            label,
                            FontId::proportional(10.0),
                            if selected { theme::ACCENT } else { theme::TEXT_DIM },
                        );
                        if response
                            .on_hover_text(format!("{}\n{} - {}", def.label, def.shape.label(), def.function.label()))
                            .clicked()
                        {
                            editor.selected_block = *block;
                        }
                    }
                });
            }

            // Placement.
            ui.add_space(6.0);
            theme::card(ui, |ui| {
                theme::header(ui, "🚩", "Placement");
                ui.horizontal(|ui| {
                    if theme::toggle_button(ui, state.auto_orient, "🔄 Auto", "Automatic orientation: top = clicked face, back = view direction").clicked() {
                        state.auto_orient = !state.auto_orient;
                    }
                    if theme::icon_button(ui, "⟳", "Rotate by 90 deg (R)").clicked() {
                        state.manual_orientation = state.manual_orientation.next_roll();
                    }
                    if theme::icon_button(ui, "↕", "Change the top face (T)").clicked() {
                        state.manual_orientation = state.manual_orientation.next_facing();
                    }
                    if theme::icon_button(ui, "↺", "Default orientation").clicked() {
                        state.manual_orientation = Orientation::IDENTITY;
                    }
                    ui.label(RichText::new(state.manual_orientation.label()).size(10.5).color(theme::TEXT_DIM));
                });
                ui.horizontal_wrapped(|ui| {
                    if theme::toggle_button(ui, state.sub_voxel_fusion, "🔀 Fusion", "Two complementary half blocks share a cell").clicked() {
                        state.sub_voxel_fusion = !state.sub_voxel_fusion;
                    }
                    if theme::toggle_button(ui, state.greedy_meshing, "⚡ Greedy", "Merge faces to lighten the mesh").clicked() {
                        state.greedy_meshing = !state.greedy_meshing;
                    }
                    if theme::toggle_button(ui, state.snap_to_grid, "⌖ Grid", "Snap to the grid").clicked() {
                        state.snap_to_grid = !state.snap_to_grid;
                    }
                });
            });

            theme::card(ui, |ui| {
                theme::header(ui, "🌍", "Scene");
                let stats = world.stats();
                theme::hint(ui, &format!("📦 {} chunks - {} voxels", stats.chunks, stats.voxels));
                if ui.button("✨ Regenerate the demo world").clicked() {
                    generator::generate_demo_world(world, registry);
                    state.action("Demo world regenerated.");
                }
            });
        });

    ui.ctx().data_mut(|d| d.insert_temp(id, filter));
}
