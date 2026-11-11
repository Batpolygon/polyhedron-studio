//! Story workspace: quests, dialogues and achievements.

use egui::RichText;

use crate::core::app_state::EditorUiState;
use crate::core::project_io::ProjectLayout;
use crate::editor::pickers::{self, Kind};
use crate::editor::theme;
use crate::gameplay::quests::{reset_progress, set_story, story, Achievement, Dialogue, DialogueChoice, DialogueLine, Objective, Quest, StoryData};
use crate::scripting::context::GameEvent;

const REWARDS: [Kind; 3] = [Kind::Resource, Kind::Block, Kind::Item];

fn text(ui: &mut egui::Ui, label: &str, value: &mut String) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(value).changed()
    })
    .inner
}

fn event_fields(ui: &mut egui::Ui, event: &mut GameEvent, key: &mut String) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("event").selected_text(event.label()).show_ui(ui, |ui| {
            for candidate in GameEvent::ALL {
                changed |= ui.selectable_value(event, candidate, candidate.label()).changed();
            }
        });
        ui.label(event.key_hint());
        changed |= ui.add(egui::TextEdit::singleline(key).hint_text("any").desired_width(110.0)).changed();
    });
    changed
}

/// Rows with a name, Add and Delete.
fn rows<T>(ui: &mut egui::Ui, title: &str, items: &mut Vec<T>, name: impl Fn(&T) -> &str, fresh: impl Fn() -> T, mut body: impl FnMut(&mut egui::Ui, &mut T) -> bool) -> bool {
    let mut changed = false;
    let mut remove = None;
    theme::section_label(ui, title);
    for (index, item) in items.iter_mut().enumerate() {
        ui.push_id((title, index), |ui| {
            egui::CollapsingHeader::new(name(item).to_string()).id_salt("row").show(ui, |ui| {
                changed |= body(ui, item);
                if ui.small_button("🗑 Delete").clicked() {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        items.remove(index);
        changed = true;
    }
    if ui.small_button("➕ Add").clicked() {
        items.push(fresh());
        changed = true;
    }
    ui.separator();
    changed
}

fn quest_editor(ui: &mut egui::Ui, quest: &mut Quest) -> bool {
    let mut changed = text(ui, "Key", &mut quest.id);
    changed |= text(ui, "Title", &mut quest.title);
    changed |= text(ui, "Description", &mut quest.description);
    changed |= ui.checkbox(&mut quest.auto_start, "Starts with the game").changed();
    let mut remove = None;
    for (index, objective) in quest.objectives.iter_mut().enumerate() {
        ui.push_id(("objective", index), |ui| {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut objective.label).desired_width(150.0)).changed();
                changed |= ui.add(egui::DragValue::new(&mut objective.count).range(1..=9999).prefix("x")).changed();
                if ui.small_button("🗑").clicked() {
                    remove = Some(index);
                }
            });
            changed |= event_fields(ui, &mut objective.event, &mut objective.key);
        });
    }
    if let Some(index) = remove {
        quest.objectives.remove(index);
        changed = true;
    }
    if ui.small_button("➕ Objective").clicked() {
        quest.objectives.push(Objective::default());
        changed = true;
    }
    ui.label("Reward");
    changed |= pickers::pick_amounts(ui, "quest_reward", &mut quest.reward, &REWARDS);
    let mut next = quest.next.clone().unwrap_or_default();
    if text(ui, "Next quest", &mut next) {
        quest.next = (!next.is_empty()).then_some(next);
        changed = true;
    }
    changed
}

fn line_editor(ui: &mut egui::Ui, line: &mut DialogueLine) -> bool {
    let mut changed = text(ui, "Speaker", &mut line.speaker);
    changed |= ui.add(egui::TextEdit::multiline(&mut line.text).desired_rows(2)).changed();
    let mut remove = None;
    for (index, choice) in line.choices.iter_mut().enumerate() {
        ui.push_id(("choice", index), |ui| {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut choice.text).hint_text("choice").desired_width(150.0)).changed();
                let mut target = choice.goto.map_or(0, |line| line + 1);
                if ui.add(egui::DragValue::new(&mut target).range(0..=99).custom_formatter(|value, _| if value < 1.0 { "end".to_string() } else { format!("line {value}") })).changed() {
                    choice.goto = target.checked_sub(1);
                    changed = true;
                }
                if ui.small_button("🗑").clicked() {
                    remove = Some(index);
                }
            });
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut choice.event).hint_text("Weave event").desired_width(110.0)).changed();
                let mut quest = choice.quest.clone().unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut quest).hint_text("starts quest").desired_width(110.0)).changed() {
                    choice.quest = (!quest.is_empty()).then_some(quest);
                    changed = true;
                }
            });
        });
    }
    if let Some(index) = remove {
        line.choices.remove(index);
        changed = true;
    }
    if ui.small_button("➕ Choice").clicked() {
        line.choices.push(DialogueChoice { text: "Choice".to_string(), ..DialogueChoice::default() });
        changed = true;
    }
    changed
}

fn dialogue_editor(ui: &mut egui::Ui, dialogue: &mut Dialogue) -> bool {
    let mut changed = text(ui, "Key", &mut dialogue.id);
    let mut remove = None;
    for (index, line) in dialogue.lines.iter_mut().enumerate() {
        ui.push_id(("line", index), |ui| {
            ui.label(RichText::new(format!("Line {}", index + 1)).size(10.0).color(theme::ACCENT_DIM));
            changed |= line_editor(ui, line);
            if ui.small_button("🗑 Delete the line").clicked() {
                remove = Some(index);
            }
            ui.separator();
        });
    }
    if let Some(index) = remove {
        dialogue.lines.remove(index);
        changed = true;
    }
    if ui.small_button("➕ Line").clicked() {
        dialogue.lines.push(DialogueLine::default());
        changed = true;
    }
    changed
}

fn achievement_editor(ui: &mut egui::Ui, achievement: &mut Achievement) -> bool {
    let mut changed = text(ui, "Key", &mut achievement.id);
    changed |= text(ui, "Title", &mut achievement.title);
    changed |= text(ui, "Description", &mut achievement.description);
    changed |= event_fields(ui, &mut achievement.event, &mut achievement.key);
    changed |= ui.add(egui::DragValue::new(&mut achievement.count).range(1..=999_999).prefix("x")).changed();
    changed
}

pub fn show(ui: &mut egui::Ui, state: &mut EditorUiState, project: &ProjectLayout) {
    let mut data: StoryData = (*story()).clone();
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.button("💾 Save").on_hover_text("Write project/quests.ron").clicked() {
            match data.save(project) {
                Ok(()) => state.action("Story saved to project/quests.ron."),
                Err(error) => state.error(error),
            }
        }
        if ui.button("↺ Reset progress").on_hover_text("Forget the quests already done while testing").clicked() {
            reset_progress(project);
            state.action("Quest progress reset.");
        }
    });
    theme::tip(ui, "In Weave: Start quest, Show dialogue, and the events Quest completed and Achievement unlocked.");
    egui::ScrollArea::vertical().id_salt("story_scroll").show(ui, |ui| {
        changed |= rows(ui, "QUESTS", &mut data.quests, |quest| &quest.title, || Quest { id: "quest".to_string(), title: "Quest".to_string(), objectives: vec![Objective::default()], ..Quest::default() }, quest_editor);
        changed |= rows(ui, "DIALOGUES", &mut data.dialogues, |dialogue| &dialogue.id, || Dialogue { id: "dialogue".to_string(), lines: vec![DialogueLine::default()] }, dialogue_editor);
        changed |= rows(ui, "ACHIEVEMENTS", &mut data.achievements, |achievement| &achievement.title, Achievement::default, achievement_editor);
    });
    if changed {
        set_story(data);
    }
}
