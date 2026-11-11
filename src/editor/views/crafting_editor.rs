//! Crafting panel of the Rules workspace: the whole of `project/crafting.ron`,
//! with every name chosen in a list.

use egui::{Color32, RichText};

use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::editor::pickers::{self, Kind};
use crate::editor::theme;
use crate::gameplay::crafting::{book, set_book, Conversion, Recipe, ResourceGroup, ResourceInfo, Station, Tier};

const INGREDIENTS: [Kind; 3] = [Kind::Resource, Kind::Group, Kind::Block];
const OUTPUTS: [Kind; 3] = [Kind::Resource, Kind::Block, Kind::Item];

fn header(ui: &mut egui::Ui, title: &str, open: bool, add: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).size(11.0).color(theme::ACCENT)).id_salt(title).default_open(open).show(ui, add);
}

pub fn show(ui: &mut egui::Ui, state: &mut EditorUiState, project: &ProjectLayout) {
    let mut data = book().clone();
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.button("💾 Save").on_hover_text("Write project/crafting.ron").clicked() {
            match data.save(project) {
                Ok(()) => state.action("Crafting saved to project/crafting.ron."),
                Err(error) => state.error(error),
            }
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().id_salt("crafting_scroll").show(ui, |ui| {
        header(ui, "RECIPES", true, |ui| {
            let mut remove = None;
            for (index, recipe) in data.recipes.iter_mut().enumerate() {
                ui.push_id(("recipe", index), |ui| {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("station").width(82.0).selected_text(recipe.station.label()).show_ui(ui, |ui| {
                            for station in Station::ALL {
                                changed |= ui.selectable_value(&mut recipe.station, station, station.label()).changed();
                            }
                        });
                        changed |= pickers::pick(ui, "output", &mut recipe.output, &OUTPUTS, None);
                        changed |= ui.add(egui::DragValue::new(&mut recipe.count).range(1..=999).prefix("x").speed(0.1)).changed();
                        if ui.small_button("🗑").clicked() {
                            remove = Some(index);
                        }
                    });
                    changed |= pickers::pick_amounts(ui, "inputs", &mut recipe.inputs, &INGREDIENTS);
                });
                ui.separator();
            }
            if let Some(index) = remove {
                data.recipes.remove(index);
                changed = true;
            }
            if ui.button("➕ Recipe").clicked() {
                data.recipes.push(Recipe { station: Station::Hand, output: String::new(), count: 1, inputs: Vec::new() });
                changed = true;
            }
            ui.label(RichText::new("Default cost of what has no recipe").size(10.0));
            changed |= pickers::pick_amounts(ui, "default_cost", &mut data.default_cost, &INGREDIENTS);
        });

        header(ui, "RESOURCES", false, |ui| {
            let mut remove = None;
            for (index, resource) in data.resources.iter_mut().enumerate() {
                ui.push_id(("resource", index), |ui| {
                    ui.horizontal(|ui| {
                        changed |= ui.add(egui::TextEdit::singleline(&mut resource.id).desired_width(80.0).hint_text("id")).changed();
                        changed |= ui.add(egui::TextEdit::singleline(&mut resource.label).desired_width(90.0).hint_text("name")).changed();
                        changed |= ui.color_edit_button_srgb(&mut resource.color).changed();
                        egui::ComboBox::from_id_salt("tier").width(110.0).selected_text(resource.tier.label()).show_ui(ui, |ui| {
                            for tier in [Tier::Raw, Tier::Basic, Tier::Overcharged, Tier::EndGame] {
                                changed |= ui.selectable_value(&mut resource.tier, tier, tier.label()).changed();
                            }
                        });
                        if ui.small_button("🗑").clicked() {
                            remove = Some(index);
                        }
                    });
                    changed |= ui.add(egui::TextEdit::singleline(&mut resource.description).hint_text("description")).changed();
                });
            }
            if let Some(index) = remove {
                data.resources.remove(index);
                changed = true;
            }
            if ui.button("➕ Resource").clicked() {
                data.resources.push(ResourceInfo {
                    id: format!("resource_{}", data.resources.len() + 1),
                    label: "New resource".to_string(),
                    color: [200, 200, 200],
                    tier: Tier::Raw,
                    description: String::new(),
                });
                changed = true;
            }
        });

        header(ui, "GROUPS", false, |ui| {
            let mut remove = None;
            for (index, group) in data.groups.iter_mut().enumerate() {
                ui.push_id(("group", index), |ui| {
                    ui.horizontal(|ui| {
                        changed |= ui.add(egui::TextEdit::singleline(&mut group.id).desired_width(90.0)).changed();
                        changed |= ui.add(egui::TextEdit::singleline(&mut group.label).desired_width(110.0)).changed();
                        if !group.id.starts_with('@') {
                            ui.colored_label(Color32::from_rgb(240, 104, 104), "must start with @");
                        }
                        if ui.small_button("🗑").clicked() {
                            remove = Some(index);
                        }
                    });
                    changed |= pickers::pick_list(ui, "members", &mut group.members, &[Kind::Resource, Kind::Block]);
                });
                ui.separator();
            }
            if let Some(index) = remove {
                data.groups.remove(index);
                changed = true;
            }
            if ui.button("➕ Group").clicked() {
                data.groups.push(ResourceGroup { id: format!("@group_{}", data.groups.len() + 1), label: "New group".to_string(), members: Vec::new() });
                changed = true;
            }
        });

        header(ui, "CONVERSIONS", false, |ui| {
            theme::tip(ui, "Turns a collected block or a group into a pure resource.");
            let mut remove = None;
            for (index, conversion) in data.conversions.iter_mut().enumerate() {
                ui.push_id(("conversion", index), |ui| {
                    ui.horizontal(|ui| {
                        changed |= pickers::pick(ui, "from", &mut conversion.from, &INGREDIENTS, None);
                        ui.label("➡");
                        changed |= pickers::pick(ui, "to", &mut conversion.to, &[Kind::Resource], None);
                        changed |= ui.add(egui::DragValue::new(&mut conversion.amount).range(1..=999).prefix("x").speed(0.1)).changed();
                        if ui.small_button("🗑").clicked() {
                            remove = Some(index);
                        }
                    });
                });
            }
            if let Some(index) = remove {
                data.conversions.remove(index);
                changed = true;
            }
            if ui.button("➕ Conversion").clicked() {
                data.conversions.push(Conversion { from: String::new(), to: String::new(), amount: 1 });
                changed = true;
            }
        });
    });
    if changed {
        set_book(data);
    }
}
