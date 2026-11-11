//! Rules workspace: every tuning value of the game, applied at once.

use egui::RichText;

use crate::core::app_state::EditorUiState;
use crate::core::game_rules::{game_rules, set_rules, GameRules};
use crate::core::project_io::ProjectLayout;
use crate::editor::pickers::{self, Kind};
use crate::editor::theme;

macro_rules! section {
    ($ui:expr, $filter:expr, $changed:ident, $value:expr, $ty:ty) => {
        if <$ty>::matches($filter) {
            egui::CollapsingHeader::new(RichText::new(<$ty>::TITLE).size(11.0).color(theme::ACCENT))
                .id_salt(<$ty>::TITLE)
                .default_open(true)
                .show($ui, |ui| {
                    egui::Grid::new(<$ty>::TITLE).num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
                        $changed |= $value.rows(ui, $filter);
                    });
                });
        }
    };
}

pub fn show(ui: &mut egui::Ui, state: &mut EditorUiState, project: &ProjectLayout) {
    use crate::core::game_rules as r;
    let mut data: GameRules = (*game_rules()).clone();
    let mut changed = false;
    let filter_id = ui.id().with("rules_filter");
    let mut filter: String = ui.ctx().data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();

    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut filter).hint_text("🔍 search a rule").desired_width(180.0));
        if ui.button("💾 Save").on_hover_text("Write project/game_rules.ron").clicked() {
            match data.save(project) {
                Ok(()) => state.action("Rules saved to project/game_rules.ron."),
                Err(error) => state.error(error),
            }
        }
        if ui.button("↺ All defaults").clicked() {
            data = GameRules::default();
            changed = true;
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    theme::tip(ui, "Changes apply at once, even during Play. Save keeps them.");
    ui.separator();

    let needle = filter.to_lowercase();
    let needle = needle.as_str();
    egui::ScrollArea::vertical().id_salt("rules_scroll").show(ui, |ui| {
        section!(ui, needle, changed, data.movement, r::MovementRules);
        section!(ui, needle, changed, data.vitals, r::VitalRules);
        section!(ui, needle, changed, data.combat, r::CombatRules);
        section!(ui, needle, changed, data.tools, r::ToolRules);
        section!(ui, needle, changed, data.player, r::PlayerRules);
        section!(ui, needle, changed, data.inventory, r::InventoryRules);
        if needle.is_empty() || "starter items".contains(needle) {
            ui.label(RichText::new("STARTER ITEMS").size(11.0).color(theme::ACCENT));
            changed |= pickers::pick_list(ui, "starter_items", &mut data.starter_tools.0, &[Kind::Item]);
        }
        section!(ui, needle, changed, data.drops, r::DropRules);
        section!(ui, needle, changed, data.energy, r::EnergyRules);
        section!(ui, needle, changed, data.machines, r::MachineRules);
        section!(ui, needle, changed, data.vehicles, r::VehicleRules);
        section!(ui, needle, changed, data.spawns, r::SpawnRules);
    });

    if changed {
        set_rules(data);
    }
}
