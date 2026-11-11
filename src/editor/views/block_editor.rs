//! 🔨 Forge panels: the block library (left) and the block details (right).
//!
//! The details edit the forge draft ([`BlockStudio::draft`]), previewed live
//! in the viewport; *Appliquer* pushes it to the registry (which remeshes
//! the world) and *Enregistrer* also writes `project/blocks/<id>.ron`.

use egui::RichText;

use crate::core::app_state::{EditorState, EditorUiState};
use crate::core::project_io::ProjectLayout;
use crate::editor::block_studio::BlockStudio;
use crate::editor::theme;
use crate::voxel::blocks::{BlockDef, BlockFunction, BlockId, BlockRegistry, SurfaceMaterial, VoronoiCells};
use crate::voxel::shapes::VoxelShape;

fn swatch(ui: &mut egui::Ui, color: [u8; 3], size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(color[0], color[1], color[2]));
}

fn unique_id(registry: &BlockRegistry, base: &str) -> String {
    let mut index = 2;
    while registry.find(&format!("{base}_{index}")).is_some() {
        index += 1;
    }
    format!("{base}_{index}")
}

/// Left panel: every block, plus creation and the model element list.
pub fn show_library(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    state: &mut EditorUiState,
    registry: &mut BlockRegistry,
    studio: &mut BlockStudio,
) {
    ui.horizontal(|ui| {
        if ui.button("➕ New block").clicked() {
            let id = unique_id(registry, "nouveau_bloc");
            let mut def = BlockDef::new(&id, "New block", VoxelShape::Cube, [170, 170, 180]);
            def.label = format!("Block {}", registry.len() + 1);
            let block = registry.upsert(def);
            editor.selected_block = block;
            studio.edit(registry, block);
            state.action("New block created in the forge.");
        }
        if ui.button("🗐 Duplicate").clicked() {
            let mut copy = studio.draft.clone();
            copy.id = unique_id(registry, &copy.id);
            copy.label = format!("{} (copy)", copy.label);
            let block = registry.upsert(copy);
            editor.selected_block = block;
            studio.edit(registry, block);
            state.action("Block duplicated.");
        }
    });
    ui.add(egui::TextEdit::singleline(&mut studio.filter).hint_text("🔍 search"));
    theme::section_label(ui, &format!("LIBRARY - {} BLOCKS", registry.len()));
    egui::ScrollArea::vertical()
        .id_salt("forge_block_list")
        .max_height(ui.available_height() * 0.55)
        .show(ui, |ui| {
            let filter = studio.filter.to_lowercase();
            let entries: Vec<(BlockId, String, [u8; 3], &'static str)> = registry
                .blocks()
                .filter(|(_, def)| def.variant_of.is_none())
                .filter(|(_, def)| {
                    filter.is_empty() || def.label.to_lowercase().contains(&filter) || def.id.contains(&filter)
                })
                .map(|(id, def)| {
                    let tag = if def.is_liquid() {
                        "💧"
                    } else if def.custom_model().is_some() {
                        "📦"
                    } else {
                        def.shape.short_label()
                    };
                    (id, def.label.clone(), def.color, tag)
                })
                .collect();
            for (block, label, color, tag) in entries {
                ui.horizontal(|ui| {
                    swatch(ui, color, 12.0);
                    let selected = studio.source == Some(block);
                    if ui
                        .selectable_label(selected, format!("{label}  -  {tag}"))
                        .clicked()
                    {
                        editor.selected_block = block;
                        studio.edit(registry, block);
                    }
                });
            }
        });

    ui.separator();
    theme::section_label(ui, "MODEL ELEMENTS");
    if studio.elements().is_empty() {
        theme::tip(ui, "Simple block (shape). Add an element to model it with 16x16x16 boxes.");
    }
    let mut remove = None;
    let mut duplicate = None;
    let selected = studio.selected();
    let mut select = None;
    let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
    let block_color = studio.draft.color;
    for (index, element) in studio.elements().iter().enumerate() {
        ui.horizontal(|ui| {
            swatch(ui, element.color.unwrap_or(block_color), 10.0);
            let name = if element.name.is_empty() { format!("Element {}", index + 1) } else { element.name.clone() };
            if ui
                .selectable_label(selected.contains(&index), name)
                .on_hover_text("Shift / Ctrl + click: add to the selection")
                .clicked()
            {
                select = Some(index);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("🗑").on_hover_text("Delete (Del)").clicked() {
                    remove = Some(index);
                }
                if ui.small_button("🗐").on_hover_text("Duplicate").clicked() {
                    duplicate = Some(index);
                }
            });
        });
    }
    if let Some(index) = select {
        studio.select(Some(index), add);
    }
    if let Some(index) = duplicate {
        // Duplicate the whole selection when the clicked element is part of it.
        let mut sources = studio.selected();
        if !sources.contains(&index) {
            sources = vec![index];
        }
        let first_new = studio.elements().len();
        for source in &sources {
            let mut copy = studio.elements()[*source].clone();
            copy.name = format!("{} copy", copy.name);
            studio.elements_mut().push(copy);
        }
        studio.select(Some(first_new), false);
        for extra in 1..sources.len() {
            studio.select(Some(first_new + extra), true);
        }
        studio.touch();
    }
    if let Some(index) = remove {
        studio.elements_mut().remove(index);
        studio.select(None, false);
        if studio.elements().is_empty() {
            studio.draft.model = None;
        }
        studio.touch();
    }
    ui.horizontal(|ui| {
        if ui.button("➕ Element").clicked() {
            studio.add_element();
        }
        if !studio.elements().is_empty() && ui.button("✖ Back to a shape").clicked() {
            studio.draft.model = None;
            studio.select(None, false);
            studio.touch();
        }
    });
}

/// Right panel: every parameter of the draft.
#[allow(clippy::too_many_arguments)]
pub fn show_details(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    state: &mut EditorUiState,
    registry: &mut BlockRegistry,
    project: &ProjectLayout,
    programs: &[String],
    studio: &mut BlockStudio,
) {
    // --- Apply / save -------------------------------------------------------
    ui.horizontal(|ui| {
        let apply = |studio: &mut BlockStudio, registry: &mut BlockRegistry, editor: &mut EditorState, state: &mut EditorUiState| {
            let key = studio.draft.id.trim().to_string();
            if key.is_empty() {
                state.error("The block id cannot be empty.");
                return None;
            }
            studio.draft.id = key;
            let id = registry.upsert(studio.draft.clone());
            editor.selected_block = id;
            studio.source = Some(id);
            studio.dirty = false;
            Some(id)
        };
        if theme::primary_button(ui, "✔ Apply", theme::ACCENT)
            .on_hover_text("Updates the registry and the world.")
            .clicked()
            && apply(studio, registry, editor, state).is_some()
        {
            state.action(format!("Block '{}' applied.", studio.draft.label));
        }
        if ui.button("💾 Save").clicked() {
            if let Some(id) = apply(studio, registry, editor, state) {
                match registry.save_block(&project.module_path("blocks"), id) {
                    Ok(path) => state.action(format!("Saved: {}", path.display())),
                    Err(error) => state.error(error),
                }
            }
        }
        if ui.add_enabled(studio.dirty, egui::Button::new("↺ Revert")).clicked() {
            if let Some(source) = studio.source {
                studio.edit(registry, source);
            }
        }
        if studio.dirty {
            ui.label(RichText::new("● modified").size(10.0).color(theme::WARN));
        }
    });
    ui.separator();

    let mut changed = false;
    let selected = studio.selected();
    let def = &mut studio.draft;

    theme::section_label(ui, "IDENTITY");
    egui::Grid::new("forge_identity").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Id");
        changed |= ui.add(egui::TextEdit::singleline(&mut def.id).desired_width(150.0)).changed();
        ui.end_row();
        ui.label("Name");
        changed |= ui.add(egui::TextEdit::singleline(&mut def.label).desired_width(150.0)).changed();
        ui.end_row();
        if def.custom_model().is_none() {
            ui.label("Shape");
            egui::ComboBox::from_id_salt("forge_shape")
                .selected_text(def.shape.label())
                .show_ui(ui, |ui| {
                    for shape in VoxelShape::ALL {
                        changed |= ui.selectable_value(&mut def.shape, shape, shape.label()).changed();
                    }
                });
            ui.end_row();
        }
        ui.label("Placement");
        egui::ComboBox::from_id_salt("forge_placement")
            .selected_text(def.placement.label())
            .show_ui(ui, |ui| {
                for placement in crate::voxel::blocks::BlockPlacement::ALL {
                    changed |= ui.selectable_value(&mut def.placement, placement, placement.label()).changed();
                }
            })
            .response
            .on_hover_text("How the block is turned when a player places it. Generated terrain gets a random direction.");
        ui.end_row();
    });

    theme::section_label(ui, "MATERIAL");
    if !selected.is_empty() {
        ui.label(
            RichText::new(format!(
                "Colour of the {} selected element(s). Click beside the block to edit the block colour.",
                selected.len()
            ))
            .size(10.0)
            .color(theme::TEXT_DIM),
        );
    }
    egui::Grid::new("forge_material").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        let block_color = def.color;
        match def.model.as_mut().filter(|_| !selected.is_empty()) {
            Some(model) => {
                ui.label("Element colour");
                let elements = &mut model.elements;
                let mut color = elements[selected[0]].color.unwrap_or(block_color);
                ui.horizontal(|ui| {
                    if ui.color_edit_button_srgb(&mut color).changed() {
                        for index in &selected {
                            elements[*index].color = Some(color);
                        }
                        changed = true;
                    }
                    let colored = selected.iter().any(|index| elements[*index].color.is_some());
                    if ui.add_enabled(colored, egui::Button::new("Use block colour").small()).clicked() {
                        for index in &selected {
                            elements[*index].color = None;
                        }
                        changed = true;
                    }
                });
            }
            None => {
                ui.label("Colour");
                changed |= ui.color_edit_button_srgb(&mut def.color).changed();
            }
        }
        ui.end_row();
        ui.label("Roughness");
        changed |= ui.add(egui::Slider::new(&mut def.roughness, 0.02..=1.0)).changed();
        ui.end_row();
        ui.label("Metal");
        changed |= ui.add(egui::Slider::new(&mut def.metallic, 0.0..=1.0)).changed();
        ui.end_row();
        ui.label("Emission");
        changed |= ui.add(egui::Slider::new(&mut def.emissive, 0.0..=6.0)).changed();
        ui.end_row();
        ui.label("Wind");
        changed |= ui
            .add(egui::Slider::new(&mut def.wind, 0.0..=1.5))
            .on_hover_text("Sway of the block in the wind (leaves, plants). 0 = still.")
            .changed();
        ui.end_row();
        ui.label("Synthetic");
        changed |= ui
            .checkbox(&mut def.synthetic, "colour and relief chosen in game")
            .on_hover_text("Players customise the colour and the Voronoi relief before placing it (Customize key).")
            .changed();
        ui.end_row();
        ui.label("Voronoi tint");
        egui::ComboBox::from_id_salt("forge_voronoi")
            .selected_text(def.voronoi.label())
            .show_ui(ui, |ui| {
                for cells in VoronoiCells::ALL {
                    changed |= ui.selectable_value(&mut def.voronoi, cells, cells.label()).changed();
                }
            });
        ui.end_row();
    });

    if def.custom_model().is_none() {
        theme::section_label(ui, "FACE COLOURS");
        for part in crate::voxel::shapes::shape_parts(def.shape) {
            let index = def.face_colors.iter().position(|(label, _)| *label == part.label);
            ui.horizontal(|ui| {
                let mut own = index.is_some();
                if ui.checkbox(&mut own, part.label.as_str()).changed() {
                    match index {
                        Some(index) => {
                            def.face_colors.remove(index);
                        }
                        None => def.face_colors.push((part.label.clone(), def.color)),
                    }
                    changed = true;
                }
                if let Some(index) = index.filter(|_| own) {
                    changed |= ui.color_edit_button_srgb(&mut def.face_colors[index].1).changed();
                }
            });
        }
    }

    theme::section_label(ui, "FLECKS (ORE, PEBBLES, FLOWERS)");
    {
        let mut enabled = def.accent.is_some();
        if ui.checkbox(&mut enabled, "Enabled").changed() {
            def.accent = enabled.then_some(crate::voxel::blocks::BlockAccent { color: [255, 255, 255], amount: 0.15, cells: 4.0, glow: 0.0 });
            changed = true;
        }
        if let Some(accent) = def.accent.as_mut() {
            egui::Grid::new("forge_accent").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label("Colour");
                changed |= ui.color_edit_button_srgb(&mut accent.color).changed();
                ui.end_row();
                ui.label("Amount");
                changed |= ui.add(egui::Slider::new(&mut accent.amount, 0.0..=1.0)).changed();
                ui.end_row();
                ui.label("Cells / block");
                changed |= ui.add(egui::Slider::new(&mut accent.cells, 0.5..=32.0).logarithmic(true)).changed();
                ui.end_row();
                ui.label("Glow");
                changed |= ui.add(egui::Slider::new(&mut accent.glow, 0.0..=6.0)).changed();
                ui.end_row();
            });
        }
    }

    theme::section_label(ui, "FACETS (LOW-POLY VORONOI NORMAL MAP)");
    egui::Grid::new("forge_normal").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        let normal = &mut def.normal;
        ui.label("Enabled");
        changed |= ui.checkbox(&mut normal.enabled, "").changed();
        ui.end_row();
        ui.label("Relief");
        changed |= ui
            .add_enabled(normal.enabled, egui::Slider::new(&mut normal.strength, 0.0..=1.5))
            .on_hover_text("Tilt of each facet")
            .changed();
        ui.end_row();
        ui.label("Facets / block");
        changed |= ui
            .add_enabled(normal.enabled, egui::Slider::new(&mut normal.cells, 0.5..=16.0).logarithmic(true))
            .on_hover_text("Voronoi cells along a block edge")
            .changed();
        ui.end_row();
        ui.label("Bevel");
        changed |= ui
            .add_enabled(normal.enabled, egui::Slider::new(&mut normal.bevel, 0.0..=1.0))
            .on_hover_text("Softens the border between facets (0 = sharp edges)")
            .changed();
        ui.end_row();
        ui.label("Seed");
        ui.horizontal(|ui| {
            changed |= ui
                .add_enabled(normal.enabled, egui::DragValue::new(&mut normal.seed).speed(0.1))
                .changed();
            if ui.small_button("🎲").on_hover_text("New pattern").clicked() {
                normal.seed = (normal.seed * 7.31 + 3.17).fract() * 100.0 + 1.0;
                changed = true;
            }
        });
        ui.end_row();
    });

    theme::section_label(ui, "LIQUID");
    egui::Grid::new("forge_liquid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Liquid");
        changed |= ui
            .checkbox(&mut def.liquid, "flows, swimmable, no collision")
            .changed();
        ui.end_row();
        if def.is_liquid() {
            ui.label("Range");
            changed |= ui
                .add(egui::Slider::new(&mut def.flow_range, 1..=7))
                .on_hover_text("Flow distance from a source")
                .changed();
            ui.end_row();
            ui.label("Slowness");
            changed |= ui
                .add(egui::Slider::new(&mut def.flow_ticks, 1..=40))
                .on_hover_text("50 ms ticks between two steps (water 4, lava 12)")
                .changed();
            ui.end_row();
            ui.label("Opacity");
            changed |= ui.add(egui::Slider::new(&mut def.opacity, 0.05..=1.0)).changed();
            ui.end_row();
        }
    });

    theme::section_label(ui, "GAMEPLAY");
    egui::Grid::new("forge_gameplay").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Surface");
        egui::ComboBox::from_id_salt("forge_surface")
            .selected_text(def.material.label())
            .show_ui(ui, |ui| {
                for material in SurfaceMaterial::ALL {
                    changed |= ui.selectable_value(&mut def.material, material, material.label()).changed();
                }
            });
        ui.end_row();
        ui.label("Hardness (s)");
        changed |= ui.add(egui::Slider::new(&mut def.hardness, 0.05..=10.0).logarithmic(true)).changed();
        ui.end_row();
        ui.label("Loot");
        {
            use crate::editor::pickers::{pick_option, Kind};
            changed |= pick_option(ui, "block_drop", &mut def.drop, &[Kind::Resource, Kind::Block, Kind::Item], "itself");
        }
        ui.end_row();
        ui.label("Function");
        egui::ComboBox::from_id_salt("forge_function")
            .selected_text(def.function.label())
            .show_ui(ui, |ui| {
                for function in BlockFunction::ALL {
                    changed |= ui.selectable_value(&mut def.function, function, function.label()).changed();
                }
            });
        ui.end_row();
        if def.function.is_generator() {
            ui.label("Power radius");
            changed |= ui.add(egui::Slider::new(&mut def.power_radius, 1.0..=48.0)).changed();
            ui.end_row();
        }
        ui.label("Weave program");
        egui::ComboBox::from_id_salt("forge_script")
            .selected_text(def.script.clone().unwrap_or_else(|| "-".to_string()))
            .show_ui(ui, |ui| {
                if ui.selectable_label(def.script.is_none(), "-").clicked() {
                    def.script = None;
                    changed = true;
                }
                for program in programs {
                    if ui.selectable_label(def.script.as_deref() == Some(program), program).clicked() {
                        def.script = Some(program.clone());
                        changed = true;
                    }
                }
            });
        ui.end_row();
    });

    // --- Sounds -----------------------------------------------------------------
    ui.horizontal(|ui| {
        ui.label("Break sound");
        changed |= crate::editor::views::ambience::sound_field(ui, "block_break_sound", &mut def.break_sound);
        ui.label("Place sound");
        changed |= crate::editor::views::ambience::sound_field(ui, "block_place_sound", &mut def.place_sound);
    });

    // --- Machine components -----------------------------------------------------
    theme::section_label(ui, "MACHINE COMPONENTS");
    if def.function.is_machine() {
        changed |= ui.checkbox(&mut def.self_powered, "Runs without power").changed();
        changed |= crate::editor::views::assembly::machine_components(ui, &mut def.components);
    } else {
        theme::tip(ui, "Choose a machine function, or Custom machine, to add components.");
    }

    // --- Cable ports ------------------------------------------------------------
    theme::section_label(ui, "CABLE PORTS");
    {
        let shapes: Vec<String> = crate::voxel::blocks::PortKind::ALL
            .iter()
            .map(|kind| format!("{} = {}", kind.label(), crate::gameplay::cable_visuals::kind_shape(*kind)))
            .collect();
        ui.label(
            egui::RichText::new(format!("In game: {} peg; inputs green, outputs red.", shapes.join(", ")))
                .size(10.5)
                .color(theme::TEXT_DIM),
        );
    }
    {
        use crate::voxel::blocks::{PortDef, PortDirection, PortKind};
        if def.ports.is_empty() {
            let defaults = def.ports();
            let text = if defaults.is_empty() {
                "No port.".to_string()
            } else {
                defaults
                    .iter()
                    .map(|port| format!("{} {}", port.kind.label(), port.direction.label().to_lowercase()))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            ui.label(RichText::new(format!("Defaults of the function: {text}")).size(10.0).color(theme::TEXT_DIM));
            if ui.button("Customize the ports").clicked() {
                def.ports = if defaults.is_empty() { vec![PortDef::new(PortKind::Signal, PortDirection::Input, "In")] } else { defaults };
                changed = true;
            }
        } else {
            let mut remove = None;
            for (index, port) in def.ports.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt(("port_kind", index)).width(80.0).selected_text(port.kind.label()).show_ui(ui, |ui| {
                        for kind in PortKind::ALL {
                            changed |= ui.selectable_value(&mut port.kind, kind, kind.label()).changed();
                        }
                    });
                    egui::ComboBox::from_id_salt(("port_direction", index)).width(70.0).selected_text(port.direction.label()).show_ui(ui, |ui| {
                        for direction in [PortDirection::Input, PortDirection::Output] {
                            changed |= ui.selectable_value(&mut port.direction, direction, direction.label()).changed();
                        }
                    });
                    changed |= ui.add(egui::TextEdit::singleline(&mut port.name).desired_width(80.0)).changed();
                    if ui.small_button("🗑").clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                def.ports.remove(index);
                changed = true;
            }
            ui.horizontal(|ui| {
                if ui.button("➕ Port").clicked() {
                    def.ports.push(PortDef::new(PortKind::Energy, PortDirection::Input, "Port"));
                    changed = true;
                }
                if ui.button("Back to the defaults").clicked() {
                    def.ports.clear();
                    changed = true;
                }
            });
        }
    }

    // --- Animated parts --------------------------------------------------------------
    theme::section_label(ui, "ANIMATED PARTS (IN GAME)");
    {
        use crate::voxel::blocks::{BlockPart, PartMotion};
        let parts = def.parts.get_or_insert_with(Vec::new);
        let mut remove = None;
        for (index, part) in parts.iter_mut().enumerate() {
            egui::CollapsingHeader::new(if part.name.is_empty() { format!("Part {}", index + 1) } else { part.name.clone() })
                .id_salt(("block_part", index))
                .show(ui, |ui| {
                    egui::Grid::new(("block_part_grid", index)).num_columns(2).spacing([6.0, 3.0]).show(ui, |ui| {
                        ui.label("Name");
                        changed |= ui.text_edit_singleline(&mut part.name).changed();
                        ui.end_row();
                        for (label, values) in [("From", &mut part.from), ("To", &mut part.to), ("Pivot", &mut part.pivot)] {
                            ui.label(label);
                            ui.horizontal(|ui| {
                                for value in values.iter_mut() {
                                    changed |= ui.add(egui::DragValue::new(value).speed(0.25).range(-16.0..=32.0)).changed();
                                }
                            });
                            ui.end_row();
                        }
                        ui.label("Rotation");
                        ui.horizontal(|ui| {
                            for value in part.rotation.iter_mut() {
                                changed |= ui.add(egui::DragValue::new(value).speed(1.0).suffix("°")).changed();
                            }
                        });
                        ui.end_row();
                        ui.label("Colour");
                        ui.horizontal(|ui| {
                            changed |= ui.color_edit_button_srgb(&mut part.color).changed();
                            changed |= ui.add(egui::Slider::new(&mut part.emissive, 0.0..=6.0).text("glow")).changed();
                        });
                        ui.end_row();
                        ui.label("Lit colour");
                        ui.horizontal(|ui| {
                            let mut lit = part.powered_color.is_some();
                            if ui.checkbox(&mut lit, "while powered").changed() {
                                part.powered_color = lit.then_some(part.color);
                                changed = true;
                            }
                            if let Some(color) = part.powered_color.as_mut() {
                                changed |= ui.color_edit_button_srgb(color).changed();
                                changed |= ui.add(egui::Slider::new(&mut part.powered_emissive, 0.0..=6.0).text("glow")).changed();
                            }
                        });
                        ui.end_row();
                        ui.label("Spin");
                        changed |= ui.add(egui::Slider::new(&mut part.spin, -20.0..=20.0).suffix(" rad/s")).changed();
                        ui.end_row();
                        ui.label("Axis (spin, rock)");
                        ui.horizontal(|ui| {
                            for value in part.spin_axis.iter_mut() {
                                changed |= ui.add(egui::DragValue::new(value).speed(0.05).range(-1.0..=1.0)).changed();
                            }
                        });
                        ui.end_row();
                        ui.label("Float");
                        changed |= ui.add(egui::Slider::new(&mut part.bob, 0.0..=0.5).suffix(" blocks")).changed();
                        ui.end_row();
                        ui.label("Float axis");
                        ui.horizontal(|ui| {
                            for value in part.bob_axis.iter_mut() {
                                changed |= ui.add(egui::DragValue::new(value).speed(0.05).range(-1.0..=1.0)).changed();
                            }
                        });
                        ui.end_row();
                        ui.label("Rock");
                        changed |= ui
                            .add(egui::Slider::new(&mut part.swing, 0.0..=180.0).suffix("°"))
                            .on_hover_text("Rocks back and forth around the axis (hammer, door, jaw)")
                            .changed();
                        ui.end_row();
                        ui.label("Pulse");
                        changed |= ui
                            .add(egui::Slider::new(&mut part.pulse, 0.0..=1.0))
                            .on_hover_text("Grows and shrinks around the pivot (flame, heart, core)")
                            .changed();
                        ui.end_row();
                        ui.label("Rhythm");
                        ui.horizontal(|ui| {
                            changed |= ui
                                .add(egui::DragValue::new(&mut part.speed).speed(0.1).range(0.0..=40.0).suffix(" rad/s"))
                                .on_hover_text("Speed of the float, the rocking and the pulse")
                                .changed();
                            changed |= ui
                                .add(egui::DragValue::new(&mut part.phase).speed(0.05).prefix("phase "))
                                .on_hover_text("Parts of one mechanism share the same phase")
                                .changed();
                        });
                        ui.end_row();
                        ui.label("Moves");
                        egui::ComboBox::from_id_salt(("block_part_motion", index)).selected_text(part.motion.label()).show_ui(ui, |ui| {
                            for motion in PartMotion::ALL {
                                changed |= ui.selectable_value(&mut part.motion, motion, motion.label()).changed();
                            }
                        });
                        ui.end_row();
                    });
                    if ui.small_button("🗑 Remove the part").clicked() {
                        remove = Some(index);
                    }
                });
        }
        if let Some(index) = remove {
            parts.remove(index);
            changed = true;
        }
        if ui.button("➕ Part").clicked() {
            parts.push(BlockPart::centered("Part", [0.0, 0.6, 0.0], [0.25, 0.25, 0.25], [230, 230, 240]));
            changed = true;
        }
    }

    // --- Selected element -----------------------------------------------------
    if let Some(index) = studio.element {
        if let Some(element) = studio.draft.model.as_mut().and_then(|m| m.elements.get_mut(index)) {
            theme::section_label(ui, "SELECTED ELEMENT");
            egui::Grid::new("forge_element").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label("Name");
                changed |= ui.text_edit_singleline(&mut element.name).changed();
                ui.end_row();
                for (label, values) in [("From (px)", &mut element.from), ("To (px)", &mut element.to)] {
                    ui.label(label);
                    ui.horizontal(|ui| {
                        for (axis, value) in values.iter_mut().enumerate() {
                            changed |= ui
                                .add(
                                    egui::DragValue::new(value)
                                        .range(0.0..=16.0)
                                        .speed(0.25)
                                        .prefix(["x ", "y ", "z "][axis]),
                                )
                                .changed();
                        }
                    });
                    ui.end_row();
                }
            });
        }
    }

    if changed {
        studio.touch();
    }
}
