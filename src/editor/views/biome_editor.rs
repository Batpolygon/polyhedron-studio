//! 🌲 Terraformeur panels.
//!
//! - [`show_list`] (left): biome list, creation, world generation settings and
//!   the top-down climate map of the whole generator;
//! - [`show_details`] (right): relief, climate point, palette, landform
//!   feature and structure rules of the selected biome. The 3D preview of
//!   the biome lives in the viewport ([`crate::editor::biome_studio`]).

use std::sync::Arc;

use egui::{Color32, RichText, Sense, vec2};

use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::editor::theme;
use crate::editor::pickers::{self, Kind};
use crate::voxel::biomes::{BiomeFeature, BiomeLibrary, LandShape, Landform, OreVein, RockLayer, StructureRule, WeightedStructure};
use crate::voxel::blocks::BlockRegistry;
use crate::voxel::chunk_manager::WorldStreaming;
use crate::voxel::structures::StructureLibrary;
use crate::voxel::world::VoxelWorld;
use crate::voxel::world_gen::TerrainGenerator;

const PREVIEW: usize = 110;

#[derive(Clone, Default)]
struct PreviewCache {
    revision: Option<(u64, u64)>,
    span: f32,
    texture: Option<egui::TextureHandle>,
    legend: Vec<(String, Color32)>,
    generator: Option<Arc<TerrainGenerator>>,
    mode: u8,
}

fn biome_color(index: usize) -> Color32 {
    const PALETTE: [Color32; 13] = [
        Color32::from_rgb(120, 190, 100),
        Color32::from_rgb(60, 140, 70),
        Color32::from_rgb(30, 100, 50),
        Color32::from_rgb(50, 110, 90),
        Color32::from_rgb(120, 95, 70),
        Color32::from_rgb(150, 150, 160),
        Color32::from_rgb(210, 120, 70),
        Color32::from_rgb(120, 40, 40),
        Color32::from_rgb(210, 230, 250),
        Color32::from_rgb(70, 130, 210),
        Color32::from_rgb(160, 200, 230),
        Color32::from_rgb(140, 70, 190),
        Color32::from_rgb(90, 210, 140),
    ];
    PALETTE[index % PALETTE.len()]
}

fn number(ui: &mut egui::Ui, label: &str, value: &mut i32) -> bool {
    ui.label(label);
    ui.add(egui::DragValue::new(value).range(-256..=256)).changed()
}

/// A list of ores, for the world or for a biome.
fn ore_list(ui: &mut egui::Ui, ores: &mut Vec<OreVein>) -> bool {
    let mut changed = false;
    let mut remove = None;
    for (index, ore) in ores.iter_mut().enumerate() {
        ui.push_id(("ore", index), |ui| {
            ui.horizontal(|ui| {
                changed |= pickers::pick(ui, "ore_block", &mut ore.block, &[Kind::Block], None);
                if ui.small_button("🗑").clicked() {
                    remove = Some(index);
                }
            });
            ui.horizontal(|ui| {
                changed |= number(ui, "from y", &mut ore.min_y);
                changed |= number(ui, "to y", &mut ore.max_y);
            });
            ui.horizontal(|ui| {
                changed |= ui.add(egui::Slider::new(&mut ore.chance, 0.0..=0.2).text("frequency")).changed();
                changed |= ui.add(egui::DragValue::new(&mut ore.size).range(1..=8).prefix("size ")).changed();
            });
            changed |= ui
                .add(egui::Slider::new(&mut ore.width, 0.0..=0.2).text("vein"))
                .on_hover_text("0 = clumps. Above 0: long winding veins of this thickness; frequency is then how full the vein is.")
                .changed();
        });
    }
    if let Some(index) = remove {
        ores.remove(index);
        changed = true;
    }
    if ui.small_button("➕ Ore").clicked() {
        ores.push(OreVein::default());
        changed = true;
    }
    changed
}

/// Caves, ores and rock layers of the world.
fn underground(ui: &mut egui::Ui, settings: &mut crate::voxel::biomes::WorldGenSettings, structures: &StructureLibrary) -> bool {
    let mut changed = false;
    egui::CollapsingHeader::new("Mountains").id_salt("mountain_settings").show(ui, |ui| {
        let mountains = &mut settings.mountains;
        changed |= ui.add(egui::Slider::new(&mut mountains.height, 0.0..=400.0).text("Summit height")).changed();
        changed |= ui.add(egui::Slider::new(&mut mountains.scale, 64.0..=1000.0).text("Ridge spacing")).changed();
        changed |= ui.add(egui::Slider::new(&mut mountains.range_scale, 128.0..=3000.0).text("Range spacing")).changed();
        changed |= ui.add(egui::Slider::new(&mut mountains.sharpness, 0.4..=3.0).text("Sharpness")).changed();
        ui.horizontal(|ui| {
            changed |= number(ui, "rock above", &mut mountains.rock_line);
            changed |= number(ui, "snow above", &mut mountains.snow_line);
        });
        changed |= ui.add(egui::Slider::new(&mut mountains.cliff, 0.2..=4.0).text("Cliff slope")).changed();
        changed |= ui.add(egui::Slider::new(&mut mountains.crag, 0.0..=40.0).text("Crags")).changed();
        ui.horizontal(|ui| {
            ui.label("Rock");
            changed |= pickers::pick(ui, "mountain_rock", &mut mountains.rock, &[Kind::Block], None);
            ui.label("Snow");
            changed |= pickers::pick(ui, "mountain_snow", &mut mountains.snow, &[Kind::Block], None);
        });
    });
    egui::CollapsingHeader::new("Caves").id_salt("cave_settings").show(ui, |ui| {
        let cave = &mut settings.cave;
        changed |= ui.add(egui::Slider::new(&mut cave.cavern_rarity, 0.2..=1.2).text("Cavern rarity")).changed();
        changed |= ui.add(egui::Slider::new(&mut cave.cavern_scale, 0.3..=4.0).text("Cavern size")).changed();
        changed |= ui.add(egui::Slider::new(&mut cave.tunnel_width, 0.0..=0.3).text("Tunnel width")).changed();
        changed |= ui.add(egui::Slider::new(&mut cave.tunnel_scale, 0.3..=4.0).text("Tunnel length")).changed();
        changed |= ui.add(egui::Slider::new(&mut cave.noodle_width, 0.0..=0.2).text("Passage width")).changed();
        changed |= ui.add(egui::Slider::new(&mut cave.entrance_rarity, -0.5..=1.0).text("Entrance rarity")).changed();
        ui.horizontal(|ui| {
            ui.label("Fluid");
            changed |= pickers::pick(ui, "cave_fluid", &mut cave.fluid, &[Kind::Block], None);
        });
        changed |= ui.add(egui::Slider::new(&mut cave.fluid_depth, 0..=40).text("Fluid depth")).changed();
        ui.horizontal(|ui| {
            ui.label("Floor");
            changed |= pickers::pick(ui, "cave_floor", &mut cave.floor, &[Kind::Block], Some("none"));
            ui.label("Tuft");
            changed |= pickers::pick(ui, "cave_tuft", &mut cave.tuft, &[Kind::Block], Some("none"));
        });
        changed |= ui.add(egui::Slider::new(&mut cave.tuft_chance, 0.0..=1.0).text("Tuft chance")).changed();
        changed |= ui.add(egui::Slider::new(&mut cave.floor_humidity, -1.0..=1.0).text("Floor from humidity")).changed();
        ui.separator();
        changed |= ui.add(egui::Slider::new(&mut cave.hall_rarity, -0.4..=0.8).text("Hall rarity")).changed();
        changed |= ui.add(egui::Slider::new(&mut cave.hall_scale, 0.3..=3.0).text("Hall size")).changed();
        ui.horizontal(|ui| {
            changed |= number(ui, "hall y", &mut cave.hall_y);
            changed |= number(ui, "reach", &mut cave.hall_height);
        });
        changed |= ui.add(egui::Slider::new(&mut cave.pillar_rarity, 0.0..=1.0).text("Column rarity")).changed();
        changed |= ui.add(egui::Slider::new(&mut cave.pillar_scale, 4.0..=80.0).text("Column spacing")).changed();
        changed |= ui.add(egui::Slider::new(&mut cave.spike_length, 0..=32).text("Stalactite length")).changed();
        changed |= ui.add(egui::Slider::new(&mut cave.spike_rarity, 0.0..=0.9).text("Stalactite rarity")).changed();
        ui.horizontal(|ui| {
            ui.label("Lake");
            changed |= pickers::pick(ui, "cave_lake", &mut cave.lake_fluid, &[Kind::Block], None);
            changed |= number(ui, "below y", &mut cave.lake_level);
        });
        ui.horizontal(|ui| {
            ui.label("Cave light");
            changed |= ui.color_edit_button_rgb(&mut cave.explorer_color).changed();
            changed |= ui.add(egui::DragValue::new(&mut cave.explorer_range).range(0.0..=128.0).suffix(" blocks")).changed();
        });
        changed |= ui.add(egui::Slider::new(&mut cave.explorer_light, 0.0..=8_000_000.0).text("Cave light power")).changed();
    });
    egui::CollapsingHeader::new("Cave structures").id_salt("cave_structures").show(ui, |ui| {
        let mut remove = None;
        for (index, rule) in settings.cave_structures.iter_mut().enumerate() {
            ui.push_id(("cave_structure", index), |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("structure").selected_text(rule.structure.as_str()).show_ui(ui, |ui| {
                        for name in structures.names() {
                            changed |= ui.selectable_value(&mut rule.structure, name.clone(), name).changed();
                        }
                    });
                    if ui.small_button("🗑").clicked() {
                        remove = Some(index);
                    }
                });
                changed |= ui.add(egui::Slider::new(&mut rule.chance_per_chunk, 0.0..=1.0).text("chance per chunk")).changed();
                ui.horizontal(|ui| {
                    changed |= number(ui, "from y", &mut rule.min_y);
                    changed |= number(ui, "to y", &mut rule.max_y);
                    changed |= ui.checkbox(&mut rule.ceiling, "ceiling").changed();
                });
            });
        }
        if let Some(index) = remove {
            settings.cave_structures.remove(index);
            changed = true;
        }
        if ui.small_button("➕ Cave structure").clicked() {
            let structure = structures.names().into_iter().next().unwrap_or_default();
            settings.cave_structures.push(crate::voxel::biomes::CaveStructureRule { structure, ..Default::default() });
            changed = true;
        }
    });
    egui::CollapsingHeader::new("Ores").id_salt("world_ores").show(ui, |ui| changed |= ore_list(ui, &mut settings.ores));
    changed |= ui.add(egui::Slider::new(&mut settings.vein_scale, 8.0..=120.0).text("Vein spacing")).changed();
    egui::CollapsingHeader::new("Rock layers").id_salt("rock_layers").show(ui, |ui| {
        let mut remove = None;
        for (index, layer) in settings.layers.iter_mut().enumerate() {
            ui.push_id(("layer", index), |ui| {
                ui.horizontal(|ui| {
                    changed |= pickers::pick(ui, "layer_block", &mut layer.block, &[Kind::Block], None);
                    changed |= number(ui, "below y", &mut layer.below_y);
                    changed |= ui.add(egui::DragValue::new(&mut layer.dither).range(1..=16).prefix("border ")).changed();
                    if ui.small_button("🗑").clicked() {
                        remove = Some(index);
                    }
                });
            });
        }
        if let Some(index) = remove {
            settings.layers.remove(index);
            changed = true;
        }
        if ui.small_button("➕ Layer").clicked() {
            settings.layers.push(RockLayer { block: "deepslate".to_string(), below_y: -6, dither: 4 });
            changed = true;
        }
    });
    egui::CollapsingHeader::new("World blocks").id_salt("world_blocks").show(ui, |ui| {
        let blocks = &mut settings.blocks;
        for (label, value) in [("Water", &mut blocks.water), ("Ice", &mut blocks.ice), ("Beach", &mut blocks.beach), ("Rock", &mut blocks.rock)] {
            ui.horizontal(|ui| {
                ui.label(label);
                changed |= pickers::pick(ui, label, value, &[Kind::Block], None);
            });
        }
    });
    changed
}

/// The reliefs of a biome.
fn landforms(ui: &mut egui::Ui, biome: &mut crate::voxel::biomes::BiomeDef) -> bool {
    let mut changed = false;
    if biome.landforms.is_empty() && biome.feature != BiomeFeature::None {
        ui.label(RichText::new(format!("Using the preset {}.", biome.feature.label())).size(9.5).color(theme::TEXT_DIM));
        if ui.small_button("Edit it as landforms").clicked() {
            biome.landforms = biome.feature.preset();
            changed = true;
        }
    }
    let mut remove = None;
    for (index, form) in biome.landforms.iter_mut().enumerate() {
        ui.push_id(("landform", index), |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("shape").selected_text(form.shape.label()).show_ui(ui, |ui| {
                    for shape in LandShape::ALL {
                        changed |= ui.selectable_value(&mut form.shape, shape, shape.label()).changed();
                    }
                });
                if ui.small_button("🗑").clicked() {
                    remove = Some(index);
                }
            });
            if form.shape == LandShape::Freeze {
                return;
            }
            changed |= ui.add(egui::Slider::new(&mut form.scale, 2.0..=400.0).logarithmic(true).text("Scale")).changed();
            changed |= ui.add(egui::Slider::new(&mut form.threshold, -1.0..=12.0).text("Threshold")).changed();
            changed |= ui.add(egui::Slider::new(&mut form.strength, 0.0..=160.0).text("Strength")).changed();
            changed |= ui.add(egui::Slider::new(&mut form.chance, 0.0..=1.0).text("Chance")).changed();
            ui.horizontal(|ui| {
                ui.label("Surface");
                changed |= pickers::pick_option(ui, "form_surface", &mut form.surface, &[Kind::Block], "unchanged");
                ui.label("Subsoil");
                changed |= pickers::pick_option(ui, "form_subsoil", &mut form.subsurface, &[Kind::Block], "unchanged");
            });
            if form.shape == LandShape::Craters {
                ui.horizontal(|ui| {
                    ui.label("Core");
                    changed |= pickers::pick_option(ui, "form_core", &mut form.core, &[Kind::Block], "unchanged");
                    changed |= ui.add(egui::Slider::new(&mut form.core_chance, 0.0..=1.0).text("Core chance")).changed();
                });
            }
            ui.separator();
        });
    }
    if let Some(index) = remove {
        biome.landforms.remove(index);
        changed = true;
    }
    if ui.small_button("➕ Landform").clicked() {
        biome.landforms.push(Landform::default());
        changed = true;
    }
    changed
}

/// Left panel of the terraformer.
#[allow(clippy::too_many_arguments)]
pub fn show_list(
    ui: &mut egui::Ui,
    state: &mut EditorUiState,
    library: &mut BiomeLibrary,
    structures: &StructureLibrary,
    registry: &BlockRegistry,
    streaming: &mut WorldStreaming,
    world: &mut VoxelWorld,
    project: &ProjectLayout,
) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("➕ New").on_hover_text("New biome from a neutral template").clicked() {
            let mut biome = crate::voxel::biomes::default_biomes().remove(0);
            let mut index = library.biomes.len() + 1;
            while library.biomes.iter().any(|b| b.name == format!("Biome {index}")) {
                index += 1;
            }
            biome.name = format!("Biome {index}");
            biome.structures.clear();
            library.biomes.push(biome);
            library.selection = library.biomes.len() - 1;
            library.touch();
            state.action("New biome created: tune it on the right, the preview follows.");
        }
        if ui.button("🗐 Duplicate").clicked() {
            if let Some(name) = library.duplicate_selected() {
                state.action(format!("Biome '{name}' created."));
            }
        }
        if ui
            .add_enabled(library.biomes.len() > 1, egui::Button::new("🗑 Delete"))
            .clicked()
        {
            let removed = library.biomes.remove(library.selection);
            library.selection = library.selection.min(library.biomes.len() - 1);
            library.touch();
            state.warn(format!("Biome '{}' deleted (save to remove its file).", removed.name));
        }
    });
    ui.horizontal(|ui| {
        if ui.button("💾 Save").clicked() {
            match library.save_all(project) {
                Ok(count) => state.action(format!("{count} biome(s) and world settings saved.")),
                Err(error) => state.error(error),
            }
        }
        if ui.button("📂 Reload").clicked() {
            match library.load_from_project(project) {
                Ok(_) => state.action("Biomes reloaded."),
                Err(error) => state.error(error),
            }
        }
        if library.is_dirty() {
            ui.label(RichText::new("● modified").size(9.5).color(theme::WARN));
        }
    });
    theme::section_label(ui, "BIOMES");
    let mut select = None;
    for (index, biome) in library.biomes.iter().enumerate() {
        ui.horizontal(|ui| {
            let (swatch, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().rect_filled(swatch, 2.0, biome_color(index));
            let sky = Color32::from_rgb(
                (biome.sky_color[0] * 255.0) as u8,
                (biome.sky_color[1] * 255.0) as u8,
                (biome.sky_color[2] * 255.0) as u8,
            );
            let (sky_rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(sky_rect.center(), 4.0, sky);
            if ui
                .selectable_label(library.selection == index, &biome.name)
                .on_hover_text(format!(
                    "T {:.2} - H {:.2} - {} - {} structure(s)",
                    biome.temperature,
                    biome.humidity,
                    biome.feature.label(),
                    biome.structures.len()
                ))
                .clicked()
            {
                select = Some(index);
            }
        });
    }
    if let Some(index) = select {
        library.selection = index;
    }
    ui.separator();

    // --- World settings + streaming ---------------------------------------
    egui::CollapsingHeader::new(RichText::new("WORLD GENERATION").size(9.5).color(theme::ACCENT_DIM))
        .default_open(false)
        .show(ui, |ui| {
            let settings = &mut library.settings;
            let mut changed = false;
            egui::Grid::new("world_gen").num_columns(2).show(ui, |ui| {
                ui.label("Seed");
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::DragValue::new(&mut settings.seed)).changed();
                    if ui.small_button("🎲").on_hover_text("Random seed").clicked() {
                        settings.seed = settings.seed.wrapping_mul(2_654_435_761).wrapping_add(ui.input(|input| input.time.to_bits()) as u32) >> 1;
                        changed = true;
                    }
                });
                ui.end_row();
                ui.label("Climate scale");
                changed |= ui.add(egui::Slider::new(&mut settings.climate_scale, 64.0..=2000.0)).changed();
                ui.end_row();
                ui.label("Continental scale");
                changed |= ui.add(egui::Slider::new(&mut settings.continental_scale, 128.0..=4000.0)).changed();
                ui.end_row();
                ui.label("Relief scale");
                changed |= ui.add(egui::Slider::new(&mut settings.relief_scale, 16.0..=400.0)).changed();
                ui.end_row();
                ui.label("Sea level");
                changed |= ui.add(egui::Slider::new(&mut settings.sea_level, -8..=40)).changed();
                ui.end_row();
                ui.label("Biome blend");
                changed |= ui.add(egui::Slider::new(&mut settings.blend, 0.02..=1.0)).changed();
                ui.end_row();
                ui.label("Bedrock (lowest layer)");
                changed |= ui.add(egui::Slider::new(&mut settings.bedrock, -96..=-8)).changed();
                ui.end_row();
                ui.label("Caves");
                changed |= ui.checkbox(&mut settings.caves, "Caverns, tunnels, ores, lava lakes").changed();
                ui.end_row();
                ui.label("Water freezes below");
                changed |= ui.add(egui::Slider::new(&mut settings.freeze_below, -1.0..=1.0)).changed();
                ui.end_row();
            });
            changed |= underground(ui, settings, structures);
            if changed {
                library.touch();
            }

            ui.horizontal(|ui| {
                if ui
                    .button("New infinite world")
                    .on_hover_text("Clears the current world and streams terrain from these biomes.")
                    .clicked()
                {
                    streaming.start_new_world(world, library, structures, registry);
                    state.world_name = "biome_world".to_string();
                    state.action("Streaming a new biome world around the camera.");
                }
                if ui.button("Apply to streaming").clicked() {
                    streaming.rebuild(library, structures, registry);
                    state.action("Generator rebuilt; new columns use the edited biomes.");
                }
            });
            ui.horizontal(|ui| {
                if ui.checkbox(&mut streaming.enabled, "Streaming").changed() && streaming.enabled {
                    if streaming.generator.is_none() {
                        streaming.rebuild(library, structures, registry);
                    }
                    streaming.adopt_existing(world);
                }
                ui.add(egui::Slider::new(&mut streaming.radius, 1..=16).text("radius"));
                ui.checkbox(&mut streaming.unload, "Unload far");
            });
            ui.label(
                RichText::new(format!(
                    "{} column(s) generated - {} pending - biome here: {}",
                    streaming.generated_count(),
                    streaming.pending(),
                    streaming.focus_biome.as_deref().unwrap_or("-")
                ))
                .size(9.0)
                .color(theme::TEXT_DIM),
            );
        });

    // --- Preview ---------------------------------------------------------------
    let cache_id = egui::Id::new("polyhedron_biome_preview");
    let mut cache: PreviewCache = ui.ctx().data_mut(|d| d.get_temp(cache_id)).unwrap_or_default();
    if cache.span <= 0.0 {
        cache.span = 1024.0;
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("WORLD MAP").size(9.5).color(theme::ACCENT_DIM));
        let mut span = cache.span;
        if ui.add(egui::Slider::new(&mut span, 128.0..=8192.0).logarithmic(true).text("m")).changed() {
            cache.span = span;
            cache.revision = None;
        }
        for (mode, label) in [(0, "Biomes"), (1, "Height"), (2, "Blocks")] {
            if ui.selectable_label(cache.mode == mode, label).clicked() {
                cache.mode = mode;
                cache.revision = None;
            }
        }
    });
    let key = (library.revision, structures.revision);
    if cache.revision != Some(key) {
        let generator = Arc::new(TerrainGenerator::new(library, structures, registry));
        let mut pixels = Vec::with_capacity(PREVIEW * PREVIEW);
        let step = cache.span / PREVIEW as f32;
        let sea = library.settings.sea_level;
        for py in 0..PREVIEW {
            for px in 0..PREVIEW {
                let x = ((px as f32 - PREVIEW as f32 / 2.0) * step) as i32;
                let z = ((py as f32 - PREVIEW as f32 / 2.0) * step) as i32;
                let info = generator.column(x, z);
                let color = if cache.mode == 2 {
                    let block = if info.height <= sea { registry.find(&library.settings.blocks.water).unwrap_or(info.surface) } else { info.surface };
                    let [r, g, b] = registry.get(block).color;
                    let shade = 0.72 + ((info.height - sea) as f32 / 90.0).clamp(0.0, 0.5);
                    Color32::from_rgb((r as f32 * shade).min(255.0) as u8, (g as f32 * shade).min(255.0) as u8, (b as f32 * shade).min(255.0) as u8)
                } else if cache.mode == 1 {
                    let t = ((info.height - sea) as f32 / 48.0).clamp(-1.0, 1.0);
                    if info.height <= sea {
                        Color32::from_rgb(30, 60, (140.0 + t * 60.0) as u8)
                    } else {
                        let v = (90.0 + t * 160.0) as u8;
                        Color32::from_rgb(v, v, v)
                    }
                } else if info.height <= sea {
                    biome_color(info.biome).lerp_to_gamma(Color32::from_rgb(40, 90, 170), 0.55)
                } else {
                    biome_color(info.biome)
                };
                pixels.push(color);
            }
        }
        cache.legend = library
            .biomes
            .iter()
            .enumerate()
            .map(|(index, biome)| (biome.name.clone(), biome_color(index)))
            .collect();
        let image = egui::ColorImage::new([PREVIEW, PREVIEW], pixels);
        cache.texture = Some(ui.ctx().load_texture("biome_world_map", image, egui::TextureOptions::NEAREST));
        cache.generator = Some(generator);
        cache.revision = Some(key);
    }
    ui.horizontal(|ui| {
        let size = 220.0;
        let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
        if let Some(texture) = &cache.texture {
            ui.painter_at(rect).image(texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        if let (Some(pos), Some(generator)) = (response.hover_pos(), cache.generator.as_ref()) {
            let local = (pos - rect.min) / size - vec2(0.5, 0.5);
            let x = (local.x * cache.span) as i32;
            let z = (local.y * cache.span) as i32;
            let info = generator.column(x, z);
            response.on_hover_text(format!(
                "({x}, {z})  {}  h={}  T={:.2} H={:.2}",
                generator.biome_name(info.biome),
                info.height,
                info.temperature,
                info.humidity
            ));
        }
        ui.vertical(|ui| {
            for (index, (name, color)) in cache.legend.iter().enumerate() {
                ui.horizontal(|ui| {
                    let (swatch, _) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::hover());
                    ui.painter().rect_filled(swatch, 1.0, *color);
                    if ui
                        .selectable_label(library.selection == index, RichText::new(name).size(9.5))
                        .clicked()
                    {
                        library.selection = index;
                    }
                });
            }
        });
    });
    ui.ctx().data_mut(|d| d.insert_temp(cache_id, cache));
}

/// Right panel of the terraformer: the selected biome.
pub fn show_details(
    ui: &mut egui::Ui,
    library: &mut BiomeLibrary,
    structures: &StructureLibrary,
    registry: &BlockRegistry,
) {
    let structure_names = structures.names();
    let block_keys: Vec<String> = registry.blocks().map(|(_, def)| def.id.clone()).collect();
    let selection = library.selection;
    let Some(biome) = library.biomes.get_mut(selection) else {
        return;
    };
    let mut changed = false;
    theme::section_label(ui, "SELECTED BIOME");
    egui::Grid::new("biome_fields").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
        ui.label("Name");
        changed |= ui.text_edit_singleline(&mut biome.name).changed();
        ui.end_row();
        ui.label("Temperature");
        changed |= ui.add(egui::Slider::new(&mut biome.temperature, -1.0..=1.0)).changed();
        ui.end_row();
        ui.label("Humidity");
        changed |= ui.add(egui::Slider::new(&mut biome.humidity, -1.0..=1.0)).changed();
        ui.end_row();
        ui.label("Base height");
        changed |= ui.add(egui::Slider::new(&mut biome.base_height, -16.0..=96.0)).changed();
        ui.end_row();
        ui.label("Relief amplitude");
        changed |= ui.add(egui::Slider::new(&mut biome.height_scale, 0.0..=80.0)).changed();
        ui.end_row();
        ui.label("Roughness");
        changed |= ui.add(egui::Slider::new(&mut biome.roughness, 0.05..=0.95)).changed();
        ui.end_row();
        ui.label("Octaves");
        changed |= ui.add(egui::Slider::new(&mut biome.octaves, 1..=8)).changed();
        ui.end_row();
        ui.label("Continentalness");
        changed |= ui.add(egui::Slider::new(&mut biome.continentalness, 0.0..=2.0)).changed();
        ui.end_row();
        ui.label("Mountains");
        changed |= ui
            .add(egui::Slider::new(&mut biome.peaks, 0.0..=1.0))
            .on_hover_text("Share of the great mountain ranges in this biome (settings of the world).")
            .changed();
        ui.end_row();
        for (label, value) in [
            ("Surface", &mut biome.surface_block),
            ("Subsoil", &mut biome.subsurface_block),
            ("Rock", &mut biome.stone_block),
        ] {
            ui.label(label);
            egui::ComboBox::from_id_salt(format!("biome_block_{label}"))
                .selected_text(value.as_str())
                .show_ui(ui, |ui| {
                    for key in &block_keys {
                        if ui.selectable_label(value == key, key).clicked() {
                            *value = key.clone();
                            changed = true;
                        }
                    }
                });
            ui.end_row();
        }
        ui.label("Subsoil depth");
        changed |= ui.add(egui::Slider::new(&mut biome.subsurface_depth, 1..=12)).changed();
        ui.end_row();
        ui.label("Special relief");
        egui::ComboBox::from_id_salt("biome_feature")
            .selected_text(biome.feature.label())
            .show_ui(ui, |ui| {
                for feature in BiomeFeature::ALL {
                    changed |= ui.selectable_value(&mut biome.feature, feature, feature.label()).changed();
                }
            });
        ui.end_row();
        ui.label("Sky colour");
        changed |= ui.color_edit_button_rgb(&mut biome.sky_color).changed();
        ui.end_row();
        ui.label("Fog");
        changed |= ui.add(egui::Slider::new(&mut biome.fog_density, 0.0..=0.1)).changed();
        ui.end_row();
        ui.label("Beaches");
        changed |= ui.checkbox(&mut biome.beaches, "Sand on the shores").changed();
        ui.end_row();
    });

    theme::section_label(ui, "LANDFORMS");
    changed |= landforms(ui, biome);
    theme::section_label(ui, "ORES OF THIS BIOME");
    changed |= ore_list(ui, &mut biome.ores);

    theme::section_label(ui, "NATURAL STRUCTURES");
    theme::tip(ui, "They appear in the 3D preview following their rules.");
    let mut remove = None;
    for (index, rule) in biome.structures.iter_mut().enumerate() {
        ui.push_id(("rule", index), |ui| {
            egui::Frame::NONE
                .fill(theme::BG_ELEVATED)
                .corner_radius(3)
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("structure")
                            .selected_text(rule.structure.as_str())
                            .show_ui(ui, |ui| {
                                for name in &structure_names {
                                    if ui.selectable_label(&rule.structure == name, name).clicked() {
                                        rule.structure = name.clone();
                                        changed = true;
                                    }
                                }
                            });
                        if ui.small_button("🗑").clicked() {
                            remove = Some(index);
                        }
                    });
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(egui::Slider::new(&mut rule.chance_per_chunk, 0.0..=1.0).text("chance / chunk"))
                            .changed();
                        changed |= ui.add(egui::DragValue::new(&mut rule.attempts).range(1..=8).prefix("x")).changed();
                        changed |= ui
                            .add(egui::DragValue::new(&mut rule.max_slope).range(0..=16).prefix("slope <= "))
                            .changed();
                    });
                    let mut drop = None;
                    for (index, variant) in rule.variants.iter_mut().enumerate() {
                        ui.push_id(("variant", index), |ui| {
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_salt("variant").selected_text(variant.structure.as_str()).show_ui(ui, |ui| {
                                    for name in &structure_names {
                                        if ui.selectable_label(&variant.structure == name, name).clicked() {
                                            variant.structure = name.clone();
                                            changed = true;
                                        }
                                    }
                                });
                                changed |= ui.add(egui::DragValue::new(&mut variant.weight).range(0.0..=100.0).speed(0.1).prefix("weight ")).changed();
                                if ui.small_button("🗑").clicked() {
                                    drop = Some(index);
                                }
                            });
                        });
                    }
                    if let Some(index) = drop {
                        rule.variants.remove(index);
                        changed = true;
                    }
                    if ui.small_button("➕ Weighted variant").on_hover_text("The rule then picks one of its variants by weight").clicked() {
                        rule.variants.push(WeightedStructure { structure: rule.structure.clone(), weight: 1.0 });
                        changed = true;
                    }
                    let mut ground = rule.ground.join(", ");
                    if ui
                        .add(egui::TextEdit::singleline(&mut ground).hint_text("ground blocks (empty = all)"))
                        .changed()
                    {
                        rule.ground = ground
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        changed = true;
                    }
                });
        });
    }
    if let Some(index) = remove {
        biome.structures.remove(index);
        changed = true;
    }
    if ui.button("➕ Structure rule").clicked() {
        biome.structures.push(StructureRule {
            structure: structure_names.first().cloned().unwrap_or_default(),
            chance_per_chunk: 0.2,
            attempts: 1,
            max_slope: 2,
            ground: vec![biome.surface_block.clone()],
            variants: Vec::new(),
        });
        changed = true;
    }
    if changed {
        library.touch();
    }
}
