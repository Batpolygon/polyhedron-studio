//! Assembly editors: what used to need code, composed from parts.
//! Machine components (Forge), mob brains (Rig), item effects (Armoury).

use egui::RichText;

use crate::editor::pickers::{self, Kind};
use crate::editor::theme;
use crate::gameplay::items::{ComboStep, ItemDef, ItemEffect};
use crate::gameplay::machines::components::{
    BiomeYield, MachineComponent, MachineOutput, MachineRecipe, RunCondition, SignalSource, ZoneEffect,
};
use crate::gameplay::mobs::brain::{self, BrainAction, BrainCondition, BrainRule};

const STUFF: [Kind; 3] = [Kind::Resource, Kind::Item, Kind::Block];

fn number(ui: &mut egui::Ui, label: &str, value: &mut f32) -> bool {
    ui.label(RichText::new(label).size(9.5).color(theme::TEXT_DIM));
    ui.add(egui::DragValue::new(value).speed(0.05).max_decimals(2)).changed()
}

/// Up, down and delete buttons of a list row.
#[derive(Default)]
struct RowOps {
    remove: Option<usize>,
    up: Option<usize>,
    down: Option<usize>,
}

impl RowOps {
    fn buttons(&mut self, ui: &mut egui::Ui, index: usize) {
        if ui.small_button("🔼").clicked() {
            self.up = Some(index);
        }
        if ui.small_button("🔽").clicked() {
            self.down = Some(index);
        }
        if ui.small_button("🗙").clicked() {
            self.remove = Some(index);
        }
    }

    fn apply<T>(self, list: &mut Vec<T>) -> bool {
        if let Some(index) = self.remove {
            list.remove(index);
            return true;
        }
        if let Some(index) = self.up.filter(|index| *index > 0) {
            list.swap(index, index - 1);
            return true;
        }
        if let Some(index) = self.down.filter(|index| index + 1 < list.len()) {
            list.swap(index, index + 1);
            return true;
        }
        false
    }
}

fn weights(ui: &mut egui::Ui, salt: &str, list: &mut Vec<(String, f32)>) -> bool {
    let mut changed = false;
    let mut remove = None;
    for (index, (key, weight)) in list.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            changed |= pickers::pick(ui, &format!("{salt}{index}"), key, &STUFF, None);
            changed |= ui.add(egui::DragValue::new(weight).speed(0.01).range(0.0..=100.0).prefix("weight ")).changed();
            if ui.small_button("🗙").clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        list.remove(index);
        changed = true;
    }
    if ui.small_button("+ resource").clicked() {
        list.push((String::new(), 1.0));
        changed = true;
    }
    changed
}

fn output_editor(ui: &mut egui::Ui, salt: &str, output: &mut MachineOutput) -> bool {
    let mut changed = false;
    let choices = [
        MachineOutput::Nothing,
        MachineOutput::Fixed(String::new()),
        MachineOutput::Weighted(Vec::new()),
        MachineOutput::ByBiome(vec![BiomeYield::default()]),
    ];
    egui::ComboBox::from_id_salt(format!("{salt}_kind")).selected_text(output.label()).show_ui(ui, |ui| {
        for choice in choices {
            if ui.selectable_label(choice.label() == output.label(), choice.label()).clicked() {
                *output = choice;
                changed = true;
            }
        }
    });
    match output {
        MachineOutput::Nothing => {}
        MachineOutput::Fixed(key) => changed |= pickers::pick(ui, &format!("{salt}_key"), key, &STUFF, None),
        MachineOutput::Weighted(list) => {
            ui.vertical(|ui| changed |= weights(ui, salt, list));
        }
        MachineOutput::ByBiome(tables) => {
            ui.vertical(|ui| {
                let mut remove = None;
                for (index, table) in tables.iter_mut().enumerate() {
                    let salt = format!("{salt}_b{index}");
                    ui.horizontal(|ui| {
                        changed |= pickers::pick_list(ui, &salt, &mut table.biomes, &[Kind::Biome]);
                        if ui.small_button("🗙").clicked() {
                            remove = Some(index);
                        }
                    });
                    if table.biomes.is_empty() {
                        theme::tip(ui, "No biome: used everywhere else");
                    }
                    changed |= weights(ui, &salt, &mut table.weights);
                    ui.separator();
                }
                if let Some(index) = remove {
                    tables.remove(index);
                    changed = true;
                }
                if ui.small_button("+ biome table").clicked() {
                    tables.push(BiomeYield::default());
                    changed = true;
                }
            });
        }
    }
    changed
}

fn recipe_editor(ui: &mut egui::Ui, salt: &str, recipe: &mut MachineRecipe) -> bool {
    let mut changed = false;
    egui::Grid::new(salt).num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
        ui.label("Costs");
        changed |= pickers::pick_amounts(ui, &format!("{salt}_in"), &mut recipe.inputs, &STUFF);
        ui.end_row();
        ui.label("Gives");
        ui.horizontal(|ui| changed |= output_editor(ui, salt, &mut recipe.output));
        ui.end_row();
        ui.label("Amount");
        ui.horizontal(|ui| {
            changed |= ui.add(egui::DragValue::new(&mut recipe.min).range(0..=9999).prefix("min ")).changed();
            changed |= ui.add(egui::DragValue::new(&mut recipe.max).range(0..=9999).prefix("max ")).changed();
            recipe.max = recipe.max.max(recipe.min);
        });
        ui.end_row();
    });
    changed
}

fn processor_fields(
    ui: &mut egui::Ui,
    salt: &str,
    every: &mut f32,
    condition: &mut RunCondition,
    recipes: &mut Vec<MachineRecipe>,
    to_container: &mut bool,
    sparks: &mut u32,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= number(ui, "every (s)", every);
        egui::ComboBox::from_id_salt("condition").selected_text(condition.label()).show_ui(ui, |ui| {
            for candidate in RunCondition::ALL {
                changed |= ui.selectable_value(condition, candidate, candidate.label()).changed();
            }
        });
    });
    ui.horizontal(|ui| {
        changed |= ui.checkbox(to_container, "Into the connected container").changed();
        changed |= ui.add(egui::DragValue::new(sparks).range(0..=64).prefix("sparks ")).changed();
    });
    let mut ops = RowOps::default();
    for (position, recipe) in recipes.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Recipe {}", position + 1)).size(9.5).color(theme::TEXT_DIM));
            ops.buttons(ui, position);
        });
        changed |= recipe_editor(ui, &format!("{salt}_r{position}"), recipe);
    }
    changed |= ops.apply(recipes);
    if ui.small_button("+ recipe").on_hover_text("The first one it can pay runs").clicked() {
        recipes.push(MachineRecipe { inputs: Vec::new(), output: MachineOutput::Nothing, min: 1, max: 1 });
        changed = true;
    }
    changed
}

fn signal_fields(ui: &mut egui::Ui, port: &mut String, source: &mut SignalSource) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("port");
        changed |= ui.add(egui::TextEdit::singleline(port).desired_width(80.0)).changed();
        let label = match source {
            SignalSource::Constant(_) => "Constant",
            SignalSource::Powered => "Powered",
            SignalSource::Stock(_) => "Stock of",
        };
        egui::ComboBox::from_id_salt("signal").selected_text(label).show_ui(ui, |ui| {
            for (name, candidate) in [
                ("Constant", SignalSource::Constant(1.0)),
                ("Powered", SignalSource::Powered),
                ("Stock of", SignalSource::Stock(String::new())),
            ] {
                if ui.selectable_label(name == label, name).clicked() {
                    *source = candidate;
                    changed = true;
                }
            }
        });
        match source {
            SignalSource::Constant(value) => changed |= ui.add(egui::DragValue::new(value).speed(0.1)).changed(),
            SignalSource::Stock(key) => changed |= pickers::pick(ui, "stock", key, &STUFF, None),
            SignalSource::Powered => {}
        }
    });
    changed
}

/// Forge: the components of a machine.
pub fn machine_components(ui: &mut egui::Ui, components: &mut Vec<MachineComponent>) -> bool {
    let mut changed = false;
    let mut ops = RowOps::default();
    for (index, component) in components.iter_mut().enumerate() {
        let salt = format!("component_{index}");
        ui.push_id(&salt, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(component.label()).strong().color(theme::ACCENT));
                    ops.buttons(ui, index);
                });
                match component {
                    MachineComponent::Processor { every, condition, recipes, to_container, sparks } => {
                        changed |= processor_fields(ui, &salt, every, condition, recipes, to_container, sparks);
                    }
                    MachineComponent::Gatherer { every, radius, blocks } => {
                        ui.horizontal(|ui| {
                            changed |= number(ui, "every (s)", every);
                            changed |= ui.add(egui::DragValue::new(radius).range(1..=32).prefix("radius ")).changed();
                        });
                        changed |= pickers::pick_list(ui, &salt, blocks, &[Kind::Block]);
                    }
                    MachineComponent::Puller { every } => {
                        ui.horizontal(|ui| changed |= number(ui, "every (s)", every));
                        theme::tip(ui, "Radius and filter: in the machine's panel.");
                    }
                    MachineComponent::Zone { every, radius, strength, effect } => {
                        ui.horizontal(|ui| {
                            changed |= number(ui, "every (s)", every);
                            changed |= number(ui, "radius", radius);
                            changed |= number(ui, "strength", strength);
                            egui::ComboBox::from_id_salt("zone").selected_text(effect.label()).show_ui(ui, |ui| {
                                for candidate in ZoneEffect::ALL {
                                    changed |= ui.selectable_value(effect, candidate, candidate.label()).changed();
                                }
                            });
                        });
                    }
                    MachineComponent::Signal { port, source } => changed |= signal_fields(ui, port, source),
                }
            });
        });
    }
    changed |= ops.apply(components);
    ui.menu_button("+ Add a component", |ui| {
        for component in MachineComponent::palette() {
            if ui.button(component.label()).clicked() {
                components.push(component);
                changed = true;
                ui.close();
            }
        }
    });
    changed
}

fn condition_fields(ui: &mut egui::Ui, condition: &mut BrainCondition) -> bool {
    match condition {
        BrainCondition::PlayerWithin(value)
        | BrainCondition::PlayerBeyond(value)
        | BrainCondition::HealthBelow(value)
        | BrainCondition::Chance(value) => ui.add(egui::DragValue::new(value).speed(0.05).max_decimals(2)).changed(),
        BrainCondition::AlliesAtLeast { count, radius } => {
            let mut changed = ui.add(egui::DragValue::new(count).range(0..=32)).changed();
            changed |= number(ui, "in", radius);
            changed
        }
        _ => false,
    }
}

fn action_fields(ui: &mut egui::Ui, action: &mut BrainAction) -> bool {
    match action {
        BrainAction::Wander(value)
        | BrainAction::Chase(value)
        | BrainAction::Flee(value)
        | BrainAction::GoHome(value)
        | BrainAction::Melee(value)
        | BrainAction::Leap(value) => ui.add(egui::DragValue::new(value).speed(0.05).max_decimals(2)).changed(),
        BrainAction::KeepDistance { distance, speed } => {
            let mut changed = number(ui, "distance", distance);
            changed |= number(ui, "speed", speed);
            changed
        }
        BrainAction::Shoot { damage, range, reload } => {
            let mut changed = number(ui, "damage", damage);
            changed |= number(ui, "range", range);
            changed |= number(ui, "reload", reload);
            changed
        }
        BrainAction::PlayClip(text) | BrainAction::Broadcast(text) => ui.add(egui::TextEdit::singleline(text).desired_width(90.0)).changed(),
        _ => false,
    }
}

fn rule_editor(ui: &mut egui::Ui, rule: &mut BrainRule) -> bool {
    let mut changed = false;
    let mut remove = None;
    for (position, condition) in rule.when.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("if {}", condition.label())).size(10.0));
            changed |= condition_fields(ui, condition);
            if ui.small_button("🗙").clicked() {
                remove = Some(position);
            }
        });
    }
    if let Some(position) = remove {
        rule.when.remove(position);
        changed = true;
    }
    let mut remove = None;
    for (position, action) in rule.actions.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("do {}", action.label())).size(10.0).color(theme::ACCENT_DIM));
            changed |= action_fields(ui, action);
            if ui.small_button("🗙").clicked() {
                remove = Some(position);
            }
        });
    }
    if let Some(position) = remove {
        rule.actions.remove(position);
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.menu_button("+ condition", |ui| {
            for (label, condition) in BrainCondition::palette() {
                if ui.button(label).clicked() {
                    rule.when.push(condition);
                    changed = true;
                    ui.close();
                }
            }
        });
        ui.menu_button("+ action", |ui| {
            for (label, action) in BrainAction::palette() {
                if ui.button(label).clicked() {
                    rule.actions.push(action);
                    changed = true;
                    ui.close();
                }
            }
        });
    });
    changed
}

/// Rig: the rules of a mob's brain.
pub fn mob_brain(ui: &mut egui::Ui, rules: &mut Vec<BrainRule>) -> bool {
    let mut changed = false;
    theme::tip(ui, "Read from the top: the first rule whose conditions all hold runs its actions.");
    let mut ops = RowOps::default();
    for (index, rule) in rules.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}", index + 1)).color(theme::ACCENT));
                    changed |= ui.add(egui::TextEdit::singleline(&mut rule.label).hint_text("rule name").desired_width(120.0)).changed();
                    ops.buttons(ui, index);
                });
                changed |= rule_editor(ui, rule);
            });
        });
    }
    changed |= ops.apply(rules);
    ui.horizontal(|ui| {
        if ui.button("+ Add a rule").clicked() {
            rules.push(BrainRule::default());
            changed = true;
        }
        ui.menu_button("Template", |ui| {
            for (name, template) in brain::templates() {
                if ui.button(name).clicked() {
                    *rules = template;
                    changed = true;
                    ui.close();
                }
            }
        });
    });
    changed
}

fn effect_fields(ui: &mut egui::Ui, effect: &mut ItemEffect) -> bool {
    let mut changed = false;
    match effect {
        ItemEffect::Spend { resource, amount } => {
            changed |= pickers::pick(ui, "spend", resource, &STUFF, None);
            changed |= ui.add(egui::DragValue::new(amount).range(1..=999)).changed();
        }
        ItemEffect::Heal(value) | ItemEffect::Stamina(value) | ItemEffect::Dash(value) => {
            changed |= ui.add(egui::DragValue::new(value).speed(0.05)).changed();
        }
        ItemEffect::Beam { damage, range, knockback } => {
            changed |= number(ui, "damage", damage);
            changed |= number(ui, "range", range);
            changed |= number(ui, "push", knockback);
        }
        ItemEffect::Zone { radius, damage, knockback } => {
            changed |= number(ui, "radius", radius);
            changed |= number(ui, "damage", damage);
            changed |= number(ui, "push", knockback);
        }
        ItemEffect::Sparks(count) => {
            changed |= ui.add(egui::DragValue::new(count).range(1..=128)).changed();
        }
        ItemEffect::Sound(name) => {
            egui::ComboBox::from_id_salt("sound").selected_text(name.as_str()).show_ui(ui, |ui| {
                for candidate in crate::audio::sound_events::sound_names() {
                    if ui.selectable_label(*name == candidate, candidate.as_str()).clicked() {
                        *name = candidate;
                        changed = true;
                    }
                }
            });
        }
        ItemEffect::Broadcast(text) => {
            changed |= ui.add(egui::TextEdit::singleline(text).desired_width(100.0)).changed();
        }
        ItemEffect::SpendHeld | ItemEffect::Blast => {}
    }
    changed
}

fn effect_list(ui: &mut egui::Ui, salt: &str, effects: &mut Vec<ItemEffect>) -> bool {
    let mut changed = false;
    let mut ops = RowOps::default();
    for (index, effect) in effects.iter_mut().enumerate() {
        ui.push_id((salt, index), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(effect.label()).size(10.0).color(theme::ACCENT_DIM));
                changed |= effect_fields(ui, effect);
                ops.buttons(ui, index);
            });
        });
    }
    changed |= ops.apply(effects);
    ui.push_id(salt, |ui| {
        ui.menu_button("+ Add an effect", |ui| {
            for effect in ItemEffect::palette() {
                if ui.button(effect.label()).clicked() {
                    effects.push(effect);
                    changed = true;
                    ui.close();
                }
            }
        });
    });
    changed
}

/// Armoury: what an item does, whatever its kind.
pub fn item_effects(ui: &mut egui::Ui, item: &mut ItemDef) -> bool {
    let mut changed = false;
    ui.label(RichText::new("ON USE").size(10.0).color(theme::ACCENT));
    changed |= effect_list(ui, "on_use", &mut item.on_use);
    ui.label(RichText::new("ON ATTACK").size(10.0).color(theme::ACCENT));
    changed |= effect_list(ui, "on_attack", &mut item.on_attack);
    theme::tip(ui, "Effects run in order; a Spend that cannot be paid stops them.");
    changed |= ui.add(egui::Slider::new(&mut item.cooldown, 0.0..=10.0).text("Cooldown (s)")).changed();
    ui.separator();
    ui.label(RichText::new("MELEE COMBO").size(10.0).color(theme::ACCENT));
    if item.combo.is_empty() {
        theme::tip(ui, "Uses the combo of the Rules panel.");
    }
    let mut ops = RowOps::default();
    for (index, step) in item.combo.iter_mut().enumerate() {
        ui.push_id(("combo", index), |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Hit {}", index + 1));
                changed |= number(ui, "damage", &mut step.damage);
                changed |= number(ui, "push", &mut step.knockback);
                ops.buttons(ui, index);
            });
        });
    }
    changed |= ops.apply(&mut item.combo);
    if ui.small_button("+ hit").clicked() {
        item.combo.push(ComboStep { damage: 2.0, knockback: 3.0 });
        changed = true;
    }
    changed
}
