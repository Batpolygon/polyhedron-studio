//! 🖼 Vitrine: widget tree (left), drag-and-drop canvas preview (centre) and
//! properties (right), with anchors and bindings. Layouts are saved to `project/ui/`.
//! The layout named `HUD` is drawn over the game during PIE.

use egui::{Color32, RichText, Sense, Stroke, vec2};

use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::editor::theme;
use crate::ui::render::{draw_layout, HudData};
use crate::ui::{
    default_hud, Anchor, Binding, ButtonAction, ListSource, ProgressStyle, UiLibrary, Widget, WidgetKind,
};

/// Selected layout and widget path, validated.
fn selection_path(library: &UiLibrary) -> Vec<usize> {
    let path = library.widget_path.clone();
    match library.layouts.get(library.selection) {
        Some(layout) if layout.root.at_path(&path).is_some() => path,
        _ => Vec::new(),
    }
}

/// Left panel: layouts, widget tree and tree editing.
pub fn show_hierarchy(ui: &mut egui::Ui, state: &mut EditorUiState, library: &mut UiLibrary, project: &ProjectLayout) {
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("umg_layout")
            .selected_text(
                library
                    .layouts
                    .get(library.selection)
                    .map(|l| l.name.clone())
                    .unwrap_or_else(|| "-".to_string()),
            )
            .show_ui(ui, |ui| {
                for index in 0..library.layouts.len() {
                    let name = library.layouts[index].name.clone();
                    if ui.selectable_label(library.selection == index, name).clicked() {
                        library.selection = index;
                        library.widget_path.clear();
                    }
                }
            });
        if ui.button("➕ Layout").clicked() {
            let mut layout = default_hud();
            layout.name = format!("Layout {}", library.layouts.len() + 1);
            layout.root.children.clear();
            library.layouts.push(layout);
            library.selection = library.layouts.len() - 1;
            library.widget_path.clear();
            library.touch();
        }
        if ui.button("💾").clicked() {
            match library.save_all(project) {
                Ok(count) => state.action(format!("Saved {count} UI layout(s).")),
                Err(error) => state.error(error),
            }
        }
        if ui.button("📂").clicked() {
            match library.load_from_project(project) {
                Ok(_) => state.action("UI layouts reloaded."),
                Err(error) => state.error(error),
            }
        }
        if library.revision != library.last_saved_revision {
            ui.label(RichText::new("● modified").size(9.0).color(theme::WARN));
        }
    });


    let selection = library.selection;
    let mut path = selection_path(library);
    let Some(layout) = library.layouts.get_mut(selection) else {
        ui.label("No layout.");
        return;
    };
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label("Name");
        changed |= ui.text_edit_singleline(&mut layout.name).changed();
    });
    if layout.name == crate::ui::HUD_LAYOUT {
        ui.label(RichText::new("HUD active in game").size(9.5).color(theme::ACCENT));
    }
    theme::section_label(ui, "HIERARCHY");
        ui.horizontal_wrapped(|ui| {
            ui.menu_button("➕ Widget", |ui| {
                for kind in WidgetKind::palette() {
                    if ui.button(kind.type_name()).clicked() {
                        // Add to the selected panel, or to its parent.
                        let mut parent_path = path.clone();
                        let accepts = layout
                            .root
                            .at_path(&parent_path)
                            .map(|w| w.kind.accepts_children())
                            .unwrap_or(false);
                        if !accepts {
                            parent_path.pop();
                        }
                        if let Some(parent) = layout.root.at_path_mut(&parent_path) {
                            let name = format!("{} {}", kind.type_name(), parent.children.len() + 1);
                            parent.children.push(Widget::new(&name, kind, Anchor::Center, [-50.0, -12.0], [100.0, 24.0]));
                            parent_path.push(parent.children.len() - 1);
                            path = parent_path;
                            changed = true;
                        }
                        ui.close();
                    }
                }
            });
            if ui.add_enabled(!path.is_empty(), egui::Button::new("🗑")).clicked() {
                let index = path.pop().unwrap_or(0);
                if let Some(parent) = layout.root.at_path_mut(&path) {
                    if index < parent.children.len() {
                        parent.children.remove(index);
                        changed = true;
                    }
                }
            }
            if ui.add_enabled(!path.is_empty(), egui::Button::new("⬆")).clicked() {
                if let Some(index) = path.last().copied() {
                    let mut parent_path = path.clone();
                    parent_path.pop();
                    if index > 0 {
                        if let Some(parent) = layout.root.at_path_mut(&parent_path) {
                            parent.children.swap(index, index - 1);
                            *path.last_mut().expect("non-empty") = index - 1;
                            changed = true;
                        }
                    }
                }
            }
            if ui.add_enabled(!path.is_empty(), egui::Button::new("🗐")).clicked() {
                if let Some(widget) = layout.root.at_path(&path).cloned() {
                    let mut parent_path = path.clone();
                    parent_path.pop();
                    if let Some(parent) = layout.root.at_path_mut(&parent_path) {
                        let mut copy = widget;
                        copy.name = format!("{} copy", copy.name);
                        copy.offset[0] += 12.0;
                        copy.offset[1] += 12.0;
                        parent.children.push(copy);
                        changed = true;
                    }
                }
            }
        });

        egui::ScrollArea::vertical().id_salt("umg_tree").show(ui, |ui| {
            tree(ui, &layout.root, &mut Vec::new(), &mut path);
        });

    if changed {
        library.touch();
    }
    library.widget_path = path;
}

/// Right panel: properties of the selected widget.
pub fn show_properties(ui: &mut egui::Ui, library: &mut UiLibrary) {
    let selection = library.selection;
    let path = selection_path(library);
    let Some(layout) = library.layouts.get_mut(selection) else {
        return;
    };
    theme::section_label(ui, "PROPERTIES");
    let mut changed = layout_settings(ui, layout);
    if let Some(widget) = layout.root.at_path_mut(&path) {
        changed |= properties(ui, widget, path.is_empty());
    }
    if changed {
        library.touch();
    }
}

/// Centre: the canvas preview (drag to move, corner to resize).
pub fn show_canvas(ui: &mut egui::Ui, library: &mut UiLibrary) {
    let selection = library.selection;
    let mut path = selection_path(library);
    let Some(layout) = library.layouts.get_mut(selection) else {
        return;
    };
    let mut changed = false;
    theme::tip(ui, "Drag to move - corner to resize - click to select");
    let room = ui.available_size() - vec2(8.0, 8.0);
    let aspect = layout.reference[1] / layout.reference[0].max(1.0);
    let width = room.x.min(room.y / aspect.max(0.01)).max(160.0);
    let size = vec2(width, width * aspect);
    let (outer, _) = ui.allocate_exact_size(room.max(size), Sense::hover());
    let canvas = egui::Rect::from_center_size(outer.center(), size);
    let response = ui.interact(canvas, ui.id().with("umg_canvas"), Sense::click_and_drag());
    let painter = ui.painter_at(canvas);
    painter.rect_filled(canvas, 2.0, Color32::from_rgb(28, 40, 56));
    // Fake game backdrop.
    painter.rect_filled(
        egui::Rect::from_min_max(canvas.left_center(), canvas.max),
        0.0,
        Color32::from_rgb(44, 70, 48),
    );
    let output = draw_layout(&painter, layout, canvas, &HudData::preview(), crate::ui::render::DrawInput::default(), &mut crate::ui::render::WidgetState::default());
    let scale = (canvas.width() / layout.reference[0].max(1.0)).min(canvas.height() / layout.reference[1].max(1.0));

    // Selection outline + resize handle.
    if let Some((_, rect)) = output.rects.iter().find(|(p, _)| *p == path) {
        painter.rect_stroke(*rect, 0.0, Stroke::new(1.5, theme::WARN), egui::StrokeKind::Outside);
        let handle = egui::Rect::from_center_size(rect.max, vec2(8.0, 8.0));
        painter.rect_filled(handle, 1.0, theme::WARN);
        // Anchor marker on the parent.
        let mut parent_path = path.clone();
        parent_path.pop();
        if let (Some(widget), Some((_, parent_rect))) = (
            layout.root.at_path(&path),
            output.rects.iter().find(|(p, _)| *p == parent_path),
        ) {
            if !path.is_empty() {
                let f = widget.anchor.factor();
                let anchor = parent_rect.min + vec2(parent_rect.width() * f[0], parent_rect.height() * f[1]);
                painter.circle_stroke(anchor, 5.0, Stroke::new(1.5, theme::ACCENT));
                painter.line_segment([anchor, rect.min], Stroke::new(1.0, theme::ACCENT_DIM));
            }
        }
    }

    let id = egui::Id::new("umg_drag_mode");
    if response.drag_started() {
        if let Some(pointer) = response.interact_pointer_pos() {
            // Resize handle has priority, then the deepest widget under the cursor.
            let resize = output
                .rects
                .iter()
                .find(|(p, _)| *p == path)
                .map(|(_, rect)| egui::Rect::from_center_size(rect.max, vec2(12.0, 12.0)).contains(pointer))
                .unwrap_or(false);
            if !resize {
                if let Some((hit, _)) = output.rects.iter().rev().find(|(_, rect)| rect.contains(pointer)) {
                    path = hit.clone();
                }
            }
            ui.ctx().data_mut(|d| d.insert_temp(id, resize));
        }
    }
    if response.dragged() && !path.is_empty() {
        let resize: bool = ui.ctx().data_mut(|d| d.get_temp(id)).unwrap_or(false);
        let delta = response.drag_delta() / scale.max(0.01);
        if let Some(widget) = layout.root.at_path_mut(&path) {
            if resize {
                widget.size[0] = (widget.size[0] + delta.x).max(4.0);
                widget.size[1] = (widget.size[1] + delta.y).max(4.0);
            } else {
                widget.offset[0] += delta.x;
                widget.offset[1] += delta.y;
            }
            changed = true;
        }
    }
    if response.clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            path = output
                .rects
                .iter()
                .rev()
                .find(|(_, rect)| rect.contains(pointer))
                .map(|(p, _)| p.clone())
                .unwrap_or_default();
        }
    }

    if changed {
        library.touch();
    }
    library.widget_path = path;
}

fn tree(ui: &mut egui::Ui, widget: &Widget, current: &mut Vec<usize>, selected: &mut Vec<usize>) {
    let depth = current.len();
    let label = format!("{}{}  ({})", "  ".repeat(depth), widget.name, widget.kind.type_name());
    if ui
        .selectable_label(*current == *selected, RichText::new(label).size(10.0).color(if widget.visible {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        }))
        .clicked()
    {
        *selected = current.clone();
    }
    for (index, child) in widget.children.iter().enumerate() {
        current.push(index);
        tree(ui, child, current, selected);
        current.pop();
    }
}

fn binding_combo(ui: &mut egui::Ui, id: &str, binding: &mut Binding) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(binding.label())
        .show_ui(ui, |ui| {
            for preset in Binding::presets() {
                let same = std::mem::discriminant(binding) == std::mem::discriminant(&preset);
                if ui.selectable_label(same, preset.label()).clicked() && !same {
                    *binding = preset;
                    changed = true;
                }
            }
        });
    match binding {
        Binding::Resource(name) => {
            changed |= ui.add(egui::TextEdit::singleline(name).hint_text("resource").desired_width(90.0)).changed();
        }
        Binding::Fixed(value) => {
            changed |= ui.add(egui::DragValue::new(value).speed(0.05)).changed();
        }
        Binding::Variable(name) => {
            changed |= ui.add(egui::TextEdit::singleline(name).hint_text("variable").desired_width(90.0)).changed();
        }
        _ => {}
    }
    changed
}

fn action_combo(ui: &mut egui::Ui, id: &str, action: &mut ButtonAction) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).selected_text(action.label()).show_ui(ui, |ui| {
        for preset in ButtonAction::presets() {
            if ui.selectable_label(preset.label() == action.label(), preset.label()).clicked() {
                *action = preset;
                changed = true;
            }
        }
    });
    match action {
        ButtonAction::Signal(text) | ButtonAction::Event(text) | ButtonAction::OpenScreen(text) | ButtonAction::CloseScreen(text) | ButtonAction::ToggleScreen(text) => {
            changed |= ui.add(egui::TextEdit::singleline(text).desired_width(90.0)).changed();
        }
        _ => {}
    }
    changed
}

/// How a layout is used, and which built-in HUD parts stay.
fn layout_settings(ui: &mut egui::Ui, layout: &mut crate::ui::UiLayout) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Role");
        egui::ComboBox::from_id_salt("umg_role").selected_text(layout.role.label()).show_ui(ui, |ui| {
            for role in crate::ui::ScreenRole::ALL {
                changed |= ui.selectable_value(&mut layout.role, role, role.label()).changed();
            }
        });
    });
    if layout.name == crate::ui::HUD_LAYOUT {
        ui.label(RichText::new("Built-in HUD parts").size(10.0).color(theme::TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            changed |= ui.checkbox(&mut layout.parts.hotbar, "Hotbar").changed();
            changed |= ui.checkbox(&mut layout.parts.status, "Status card").changed();
            changed |= ui.checkbox(&mut layout.parts.crosshair, "Crosshair").changed();
            changed |= ui.checkbox(&mut layout.parts.chat, "Chat").changed();
            changed |= ui.checkbox(&mut layout.parts.quests, "Quest tracker").changed();
        });
    }
    ui.separator();
    changed
}

fn color_field(ui: &mut egui::Ui, color: &mut [u8; 4]) -> bool {
    let mut c = Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
    let changed = ui.color_edit_button_srgba(&mut c).changed();
    if changed {
        *color = c.to_srgba_unmultiplied();
    }
    changed
}

fn properties(ui: &mut egui::Ui, widget: &mut Widget, is_root: bool) -> bool {
    let mut changed = false;
    egui::Grid::new("umg_props").num_columns(2).spacing([6.0, 3.0]).show(ui, |ui| {
        ui.label("Name");
        changed |= ui.text_edit_singleline(&mut widget.name).changed();
        ui.end_row();
        ui.label("Visible");
        changed |= ui.checkbox(&mut widget.visible, "").changed();
        ui.end_row();
        ui.label("Tooltip");
        changed |= ui.text_edit_singleline(&mut widget.tooltip).changed();
        ui.end_row();
        if !is_root {
            ui.label("Anchor");
            egui::ComboBox::from_id_salt("umg_anchor")
                .selected_text(widget.anchor.label())
                .show_ui(ui, |ui| {
                    for anchor in Anchor::ALL {
                        changed |= ui.selectable_value(&mut widget.anchor, anchor, anchor.label()).changed();
                    }
                });
            ui.end_row();
            ui.label("Offset");
            ui.horizontal(|ui| {
                changed |= ui.add(egui::DragValue::new(&mut widget.offset[0])).changed();
                changed |= ui.add(egui::DragValue::new(&mut widget.offset[1])).changed();
            });
            ui.end_row();
            ui.label("Size");
            ui.horizontal(|ui| {
                changed |= ui.add(egui::DragValue::new(&mut widget.size[0]).range(1.0..=4000.0)).changed();
                changed |= ui.add(egui::DragValue::new(&mut widget.size[1]).range(1.0..=4000.0)).changed();
            });
            ui.end_row();
        }

        match &mut widget.kind {
            WidgetKind::CanvasPanel { background } => {
                ui.label("Background");
                changed |= color_field(ui, background);
                ui.end_row();
            }
            WidgetKind::GridSlotContainer {
                columns,
                slots,
                slot_size,
                spacing,
            } => {
                ui.label("Columns");
                changed |= ui.add(egui::DragValue::new(columns).range(1..=16)).changed();
                ui.end_row();
                ui.label("Slots");
                changed |= ui.add(egui::DragValue::new(slots).range(1..=9)).changed();
                ui.end_row();
                ui.label("Slot size");
                changed |= ui.add(egui::DragValue::new(slot_size).range(8.0..=128.0)).changed();
                ui.end_row();
                ui.label("Spacing");
                changed |= ui.add(egui::DragValue::new(spacing).range(0.0..=32.0)).changed();
                ui.end_row();
            }
            WidgetKind::ProgressBar {
                binding,
                max,
                style,
                fill,
                background,
            } => {
                ui.label("Binding");
                ui.horizontal(|ui| changed |= binding_combo(ui, "umg_pb_binding", binding));
                ui.end_row();
                ui.label("Max");
                changed |= ui.add(egui::DragValue::new(max).speed(0.1).range(0.01..=10000.0)).changed();
                ui.end_row();
                ui.label("Style");
                ui.horizontal(|ui| {
                    let mut pips = matches!(style, ProgressStyle::Pips(_));
                    if ui.checkbox(&mut pips, "Pips").changed() {
                        *style = if pips { ProgressStyle::Pips(10) } else { ProgressStyle::Bar };
                        changed = true;
                    }
                    if let ProgressStyle::Pips(count) = style {
                        changed |= ui.add(egui::DragValue::new(count).range(1..=40)).changed();
                    }
                });
                ui.end_row();
                ui.label("Fill");
                changed |= color_field(ui, fill);
                ui.end_row();
                ui.label("Background");
                changed |= color_field(ui, background);
                ui.end_row();
            }
            WidgetKind::Label {
                text,
                binding,
                font_size,
                color,
                centered,
            } => {
                ui.label("Centred");
                changed |= ui.checkbox(centered, "").changed();
                ui.end_row();
                ui.label("Text");
                changed |= ui.text_edit_singleline(text).on_hover_text("{value} is replaced by the binding").changed();
                ui.end_row();
                ui.label("Binding");
                ui.horizontal(|ui| changed |= binding_combo(ui, "umg_label_binding", binding));
                ui.end_row();
                ui.label("Font size");
                changed |= ui.add(egui::DragValue::new(font_size).range(6.0..=64.0)).changed();
                ui.end_row();
                ui.label("Colour");
                changed |= color_field(ui, color);
                ui.end_row();
            }
            WidgetKind::Button { text, action } => {
                ui.label("Text");
                changed |= ui.text_edit_singleline(text).changed();
                ui.end_row();
                ui.label("Action");
                ui.horizontal(|ui| changed |= action_combo(ui, "umg_action", action));
                ui.end_row();
            }
            WidgetKind::Icon { glyph, color } => {
                ui.label("Glyph");
                changed |= ui.add(egui::TextEdit::singleline(glyph).desired_width(60.0)).changed();
                ui.end_row();
                ui.label("Colour");
                changed |= color_field(ui, color);
                ui.end_row();
            }
            WidgetKind::ResourceSlot { key } => {
                ui.label("Resource");
                changed |= crate::editor::pickers::pick(ui, "umg_resource", key, &[crate::editor::pickers::Kind::Resource], None);
                ui.end_row();
            }
            WidgetKind::ListView { source, row_height, color, on_row } => {
                ui.label("Shows");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("umg_list").selected_text(source.label()).show_ui(ui, |ui| {
                        for candidate in [ListSource::Inventory, ListSource::WeaveList("list".to_string()), ListSource::Altars] {
                            if ui.selectable_label(candidate.label() == source.label(), candidate.label()).clicked() {
                                *source = candidate;
                                changed = true;
                            }
                        }
                    });
                    if let ListSource::WeaveList(name) = source {
                        changed |= ui.add(egui::TextEdit::singleline(name).desired_width(80.0)).changed();
                    }
                });
                ui.end_row();
                ui.label("Row height");
                changed |= ui.add(egui::DragValue::new(row_height).range(8.0..=120.0)).changed();
                ui.end_row();
                ui.label("Colour");
                changed |= color_field(ui, color);
                ui.end_row();
                ui.label("On click");
                ui.horizontal(|ui| changed |= action_combo(ui, "umg_row", on_row));
                ui.end_row();
            }
            WidgetKind::Tabs { tab_height } => {
                ui.label("Tab height");
                changed |= ui.add(egui::DragValue::new(tab_height).range(12.0..=80.0)).changed();
                ui.end_row();
            }
            WidgetKind::ScrollPanel { content_height } => {
                ui.label("Content height");
                changed |= ui.add(egui::DragValue::new(content_height).range(1.0..=8000.0)).changed();
                ui.end_row();
            }
            WidgetKind::ButtonBar { buttons, vertical, spacing } => {
                ui.label("Stacked");
                changed |= ui.checkbox(vertical, "").changed();
                ui.end_row();
                ui.label("Spacing");
                changed |= ui.add(egui::DragValue::new(spacing).range(0.0..=64.0)).changed();
                ui.end_row();
                let mut remove = None;
                for (index, (text, action)) in buttons.iter_mut().enumerate() {
                    ui.label(format!("Button {}", index + 1));
                    ui.horizontal(|ui| {
                        changed |= ui.add(egui::TextEdit::singleline(text).desired_width(110.0)).changed();
                        changed |= action_combo(ui, &format!("umg_bar{index}"), action);
                        if ui.small_button("🗙").clicked() {
                            remove = Some(index);
                        }
                    });
                    ui.end_row();
                }
                if let Some(index) = remove {
                    buttons.remove(index);
                    changed = true;
                }
                ui.label("");
                if ui.small_button("+ button").clicked() {
                    buttons.push(("Button".to_string(), ButtonAction::None));
                    changed = true;
                }
                ui.end_row();
            }
            WidgetKind::Minimap => {}
            WidgetKind::ItemSlot { index } => {
                ui.label("Hotbar index");
                changed |= ui.add(egui::DragValue::new(index).range(0..=8)).changed();
                ui.end_row();
            }
        }
    });
    changed
}
