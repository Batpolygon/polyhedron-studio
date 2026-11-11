//! 🗡 Armurerie panels: item list and box list (left), item and box details
//! (right). The item is previewed and edited in the viewport
//! ([`crate::editor::item_studio`]).

use egui::RichText;

use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::editor::item_studio::ItemStudio;
use crate::editor::theme;
use crate::gameplay::items::{ItemDef, ItemKind, ItemLibrary, ItemPart};

fn swatch(ui: &mut egui::Ui, color: [u8; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(color[0], color[1], color[2]));
}

/// Left panel.
pub fn show_list(
    ui: &mut egui::Ui,
    state: &mut EditorUiState,
    library: &mut ItemLibrary,
    studio: &mut ItemStudio,
    project: &ProjectLayout,
) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("➕ New item").clicked() {
            let mut index = library.items.len() + 1;
            while library.find(&format!("item_{index}")).is_some() {
                index += 1;
            }
            let mut item = ItemDef::new(&format!("item_{index}"), &format!("Item {index}"), ItemKind::Material);
            item.parts.push(ItemPart::new("Body", [6.0, 0.0, 6.0], [10.0, 8.0, 10.0], [200, 200, 210]));
            library.items.push(item);
            library.selection = library.items.len() - 1;
            studio.select(Some(0), false);
            studio.frame_request = true;
            library.touch();
        }
        if ui.button("🗐").on_hover_text("Duplicate").clicked() {
            if let Some(mut copy) = library.items.get(library.selection).cloned() {
                copy.id = format!("{}_copie", copy.id);
                copy.label = format!("{} (copy)", copy.label);
                library.items.push(copy);
                library.selection = library.items.len() - 1;
                library.touch();
            }
        }
        if ui.add_enabled(!library.items.is_empty(), egui::Button::new("🗑")).on_hover_text("Delete").clicked() {
            library.items.remove(library.selection);
            library.selection = library.selection.min(library.items.len().saturating_sub(1));
            studio.select(None, false);
            library.touch();
        }
        if ui.button("💾").on_hover_text("Save every item").clicked() {
            match library.save_all(project) {
                Ok(count) => state.action(format!("{count} item(s) saved.")),
                Err(error) => state.error(error),
            }
        }
        if ui.button("📂").on_hover_text("Reload").clicked() {
            match library.load_from_project(project) {
                Ok(_) => state.action("Items reloaded."),
                Err(error) => state.error(error),
            }
        }
        if library.is_dirty() {
            ui.label(RichText::new("● modified").size(9.5).color(theme::WARN));
        }
    });
    theme::section_label(ui, "ITEMS");
    egui::ScrollArea::vertical().id_salt("item_list").max_height(ui.available_height() * 0.5).show(ui, |ui| {
        let mut select = None;
        for (index, item) in library.items.iter().enumerate() {
            ui.horizontal(|ui| {
                swatch(ui, item.color());
                if ui
                    .selectable_label(library.selection == index, format!("{}  -  {}", item.label, item.kind.label()))
                    .clicked()
                {
                    select = Some(index);
                }
            });
        }
        if let Some(index) = select {
            library.selection = index;
            studio.select(None, false);
            studio.frame_request = true;
        }
    });

    ui.separator();
    theme::section_label(ui, "MODEL BOXES");
    let selection = library.selection;
    let Some(item) = library.items.get_mut(selection) else {
        return;
    };
    let mut changed = false;
    let mut remove = None;
    let mut duplicate = None;
    let selected = studio.selected();
    let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
    let mut select = None;
    for (index, part) in item.parts.iter().enumerate() {
        ui.horizontal(|ui| {
            swatch(ui, part.color);
            let name = if part.name.is_empty() { format!("Box {}", index + 1) } else { part.name.clone() };
            if ui
                .selectable_label(selected.contains(&index), name)
                .on_hover_text("Shift / Ctrl + click: add to the selection")
                .clicked()
            {
                select = Some(index);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("🗑").clicked() {
                    remove = Some(index);
                }
                if ui.small_button("🗐").clicked() {
                    duplicate = Some(index);
                }
            });
        });
    }
    if let Some(index) = select {
        studio.select(Some(index), add);
    }
    if let Some(index) = duplicate {
        let mut sources = selected.clone();
        if !sources.contains(&index) {
            sources = vec![index];
        }
        let first_new = item.parts.len();
        for source in &sources {
            if let Some(part) = item.parts.get(*source) {
                let mut copy = part.clone();
                copy.name = format!("{} copy", copy.name);
                item.parts.push(copy);
            }
        }
        studio.select(Some(first_new), false);
        for extra in 1..sources.len() {
            studio.select(Some(first_new + extra), true);
        }
        changed = true;
    }
    if let Some(index) = remove {
        item.parts.remove(index);
        studio.select(None, false);
        changed = true;
    }
    if ui.button("➕ Box").clicked() {
        let color = item.color();
        item.parts.push(ItemPart::new(&format!("Box {}", item.parts.len() + 1), [6.0, 6.0, 6.0], [10.0, 10.0, 10.0], color));
        studio.select(Some(item.parts.len() - 1), false);
        changed = true;
    }
    if changed {
        library.touch();
    }
}

/// Right panel.
pub fn show_details(ui: &mut egui::Ui, library: &mut ItemLibrary, studio: &mut ItemStudio, programs: &[String]) {
    let selection = library.selection;
    let Some(item) = library.items.get_mut(selection) else {
        ui.label("No item.");
        return;
    };
    let mut changed = false;
    theme::section_label(ui, "ITEM");
    egui::Grid::new("item_fields").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Id");
        changed |= ui.text_edit_singleline(&mut item.id).changed();
        ui.end_row();
        ui.label("Name");
        changed |= ui.text_edit_singleline(&mut item.label).changed();
        ui.end_row();
        ui.label("Type");
        egui::ComboBox::from_id_salt("item_kind")
            .selected_text(item.kind.label())
            .show_ui(ui, |ui| {
                for kind in ItemKind::ALL {
                    changed |= ui.selectable_value(&mut item.kind, kind, kind.label()).changed();
                }
            });
        ui.end_row();
        match item.kind {
            ItemKind::Weapon | ItemKind::Tool => {
                ui.label("⚔ Damage");
                changed |= ui.add(egui::Slider::new(&mut item.damage, 0.0..=40.0)).changed();
                ui.end_row();
                ui.label("💥 Knockback");
                changed |= ui.add(egui::Slider::new(&mut item.knockback, 0.0..=20.0)).changed();
                ui.end_row();
                if item.kind == ItemKind::Tool {
                    ui.label("⛏ Mining speed x");
                    changed |= ui.add(egui::Slider::new(&mut item.mining_speed, 0.1..=20.0).logarithmic(true)).changed();
                    ui.end_row();
                }
            }
            ItemKind::Food => {
                ui.label("❤ Heal");
                changed |= ui.add(egui::Slider::new(&mut item.heal, 0.0..=20.0)).changed();
                ui.end_row();
            }
            ItemKind::Blaster => {
                ui.label("⚔ Bolt damage");
                changed |= ui.add(egui::Slider::new(&mut item.damage, 0.0..=50.0)).changed();
                ui.end_row();
            }
            ItemKind::Armor => {
                ui.label("Armour (damage removed)");
                changed |= ui.add(egui::Slider::new(&mut item.armor, 0.0..=10.0)).changed();
                ui.end_row();
                ui.label("Extra jumps");
                changed |= ui.add(egui::Slider::new(&mut item.extra_jumps, 0..=3)).changed();
                ui.end_row();
                ui.label("Flight");
                changed |= ui.checkbox(&mut item.flight, "").changed();
                ui.end_row();
                ui.label("Bonus damage");
                changed |= ui.add(egui::Slider::new(&mut item.bonus_damage, 0.0..=10.0)).changed();
                ui.end_row();
                ui.label("Bonus hearts");
                changed |= ui.add(egui::Slider::new(&mut item.bonus_health, 0.0..=10.0)).changed();
                ui.end_row();
            }
            ItemKind::Atomixer => {
                ui.label("⛏ Mining speed x");
                changed |= ui.add(egui::Slider::new(&mut item.mining_speed, 0.1..=20.0).logarithmic(true)).changed();
                ui.end_row();
            }
            ItemKind::BlastyGun => {
                ui.label("⚔ Explosion damage");
                changed |= ui.add(egui::Slider::new(&mut item.damage, 0.0..=50.0)).changed();
                ui.end_row();
            }
            ItemKind::Material | ItemKind::Cable | ItemKind::Remote | ItemKind::Brush | ItemKind::Wrench | ItemKind::Binder | ItemKind::EtheraGun | ItemKind::Etherealink => {}
        }
        // Resource spent per use (Gun: mineral, VoltiGun: voltic...).
        ui.label("Ammo / cost");
        changed |= crate::editor::pickers::pick_option(ui, "item_ammo", &mut item.ammo, &[crate::editor::pickers::Kind::Resource], "none");
        ui.end_row();
        if item.kind == ItemKind::Tool {
            ui.label("Faster on");
            changed |= crate::editor::pickers::pick_option(ui, "item_target", &mut item.mining_target, &[crate::editor::pickers::Kind::Group, crate::editor::pickers::Kind::Block], "everything");
            ui.end_row();
        }
        ui.label("📦 Max stack");
        changed |= ui.add(egui::DragValue::new(&mut item.stack).range(1..=999)).changed();
        ui.end_row();
        ui.label("✋ Size in hand");
        changed |= ui.add(egui::Slider::new(&mut item.hold_scale, 0.2..=3.0)).changed();
        ui.end_row();
        ui.label("🕸 Weave program");
        egui::ComboBox::from_id_salt("item_script")
            .selected_text(item.script.clone().unwrap_or_else(|| "-".to_string()))
            .show_ui(ui, |ui| {
                if ui.selectable_label(item.script.is_none(), "-").clicked() {
                    item.script = None;
                    changed = true;
                }
                for program in programs {
                    if ui.selectable_label(item.script.as_deref() == Some(program), program).clicked() {
                        item.script = Some(program.clone());
                        changed = true;
                    }
                }
            });
        ui.end_row();
    });

    ui.horizontal(|ui| {
        for gizmo in crate::editor::block_studio::ForgeGizmo::ALL {
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

    if let Some(part) = studio.part.and_then(|index| item.parts.get_mut(index)) {
        theme::section_label(ui, "SELECTED BOX");
        egui::Grid::new("item_part").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Name");
            changed |= ui.text_edit_singleline(&mut part.name).changed();
            ui.end_row();
            ui.label("Colour");
            changed |= ui.color_edit_button_srgb(&mut part.color).changed();
            ui.end_row();
            for (label, values) in [("From (px)", &mut part.from), ("To (px)", &mut part.to)] {
                ui.label(label);
                ui.horizontal(|ui| {
                    for (axis, value) in values.iter_mut().enumerate() {
                        changed |= ui
                            .add(egui::DragValue::new(value).range(-16.0..=32.0).speed(0.25).prefix(["x ", "y ", "z "][axis]))
                            .changed();
                    }
                });
                ui.end_row();
            }
        });
    }
    theme::section_label(ui, "EFFECTS");
    changed |= crate::editor::views::assembly::item_effects(ui, item);
    if changed {
        library.touch();
    }
}
