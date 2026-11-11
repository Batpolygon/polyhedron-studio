//! Reference pickers: every name of the project (block, item, resource, group,
//! biome, mob) is chosen in a searchable list instead of being typed. A name
//! that no longer exists stays visible, in red.

use std::sync::{Arc, LazyLock, RwLock};

use bevy::prelude::*;
use egui::RichText;

use crate::editor::theme;
use crate::gameplay::crafting::book;
use crate::gameplay::items::ItemLibrary;
use crate::gameplay::mobs::library::MobLibrary;
use crate::voxel::biomes::BiomeLibrary;
use crate::voxel::blocks::BlockRegistry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Block,
    Item,
    Resource,
    Group,
    Biome,
    Mob,
}

impl Kind {
    const fn title(self) -> &'static str {
        match self {
            Self::Block => "BLOCKS",
            Self::Item => "ITEMS",
            Self::Resource => "RESOURCES",
            Self::Group => "GROUPS",
            Self::Biome => "BIOMES",
            Self::Mob => "MOBS",
        }
    }
}

/// Every referencable name: (key, label).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub blocks: Vec<(String, String)>,
    pub items: Vec<(String, String)>,
    pub resources: Vec<(String, String)>,
    pub groups: Vec<(String, String)>,
    pub biomes: Vec<(String, String)>,
    pub mobs: Vec<(String, String)>,
}

impl Catalog {
    pub fn entries(&self, kind: Kind) -> &[(String, String)] {
        match kind {
            Kind::Block => &self.blocks,
            Kind::Item => &self.items,
            Kind::Resource => &self.resources,
            Kind::Group => &self.groups,
            Kind::Biome => &self.biomes,
            Kind::Mob => &self.mobs,
        }
    }

    fn label(&self, key: &str, kinds: &[Kind]) -> Option<&str> {
        kinds.iter().flat_map(|kind| self.entries(*kind)).find(|(entry, _)| entry == key).map(|(_, label)| label.as_str())
    }
}

static CATALOG: LazyLock<RwLock<Arc<Catalog>>> = LazyLock::new(|| RwLock::new(Arc::new(Catalog::default())));

pub fn catalog() -> Arc<Catalog> {
    CATALOG.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

/// Keeps the catalog in step with the libraries (twice a second is plenty).
pub fn refresh_catalog(
    time: Res<Time>,
    mut timer: Local<f32>,
    registry: Res<BlockRegistry>,
    items: Res<ItemLibrary>,
    biomes: Res<BiomeLibrary>,
    mobs: Res<MobLibrary>,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 0.5;
    let crafting = book();
    let mut biome_entries = vec![("*".to_string(), "All biomes".to_string())];
    biome_entries.extend(biomes.biomes.iter().map(|b| (b.name.clone(), b.name.clone())));
    let fresh = Catalog {
        blocks: registry.blocks().map(|(_, def)| (def.id.clone(), def.label.clone())).collect(),
        items: items.items.iter().map(|item| (item.id.clone(), item.label.clone())).collect(),
        resources: crafting.resources.iter().map(|r| (r.id.clone(), r.label.clone())).collect(),
        groups: crafting.groups.iter().map(|g| (g.id.clone(), g.label.clone())).collect(),
        biomes: biome_entries,
        mobs: mobs.rigs.iter().map(|rig| (rig.name.clone(), rig.name.clone())).collect(),
    };
    let mut slot = CATALOG.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    if **slot != fresh {
        *slot = Arc::new(fresh);
    }
}

/// The searchable list. `pick` receives the chosen key.
fn list(ui: &mut egui::Ui, id: egui::Id, kinds: &[Kind], mut pick: impl FnMut(&str)) {
    let cat = catalog();
    let filter_id = id.with("filter");
    let mut filter: String = ui.ctx().data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
    ui.add(egui::TextEdit::singleline(&mut filter).hint_text("🔍 search").desired_width(170.0));
    ui.ctx().data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    let needle = filter.to_lowercase();
    egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
        for kind in kinds {
            let matching: Vec<&(String, String)> = cat
                .entries(*kind)
                .iter()
                .filter(|(key, label)| needle.is_empty() || key.to_lowercase().contains(&needle) || label.to_lowercase().contains(&needle))
                .collect();
            if matching.is_empty() {
                continue;
            }
            if kinds.len() > 1 {
                ui.label(RichText::new(kind.title()).size(9.5).color(theme::TEXT_DIM));
            }
            for (key, label) in matching {
                if ui.selectable_label(false, label.as_str()).on_hover_text(key.as_str()).clicked() {
                    pick(key);
                    ui.close();
                }
            }
        }
    });
}

fn shown(key: &str, kinds: &[Kind]) -> RichText {
    match catalog().label(key, kinds) {
        Some(label) => RichText::new(label.to_string()),
        None => RichText::new(format!("{key} ?")).color(theme::ERROR),
    }
}

/// Choose one name. `empty` is the label of the "nothing" entry, when allowed.
pub fn pick(ui: &mut egui::Ui, salt: &str, value: &mut String, kinds: &[Kind], empty: Option<&str>) -> bool {
    let id = ui.id().with("picker").with(salt);
    let text = if value.is_empty() { RichText::new(empty.unwrap_or("choose...").to_string()) } else { shown(value, kinds) };
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).selected_text(text).show_ui(ui, |ui| {
        if let Some(empty) = empty {
            if ui.selectable_label(value.is_empty(), empty).clicked() {
                value.clear();
                changed = true;
            }
        }
        list(ui, id, kinds, |key| {
            *value = key.to_string();
            changed = true;
        });
    });
    changed
}

/// The same for an optional reference.
pub fn pick_option(ui: &mut egui::Ui, salt: &str, value: &mut Option<String>, kinds: &[Kind], empty: &str) -> bool {
    let mut text = value.clone().unwrap_or_default();
    if pick(ui, salt, &mut text, kinds, Some(empty)) {
        *value = if text.is_empty() { None } else { Some(text) };
        return true;
    }
    false
}

/// Button that adds a name to a list.
fn add_button(ui: &mut egui::Ui, id: egui::Id, kinds: &[Kind], mut add: impl FnMut(&str)) {
    egui::ComboBox::from_id_salt(id.with("add")).selected_text("➕").width(36.0).show_ui(ui, |ui| {
        list(ui, id.with("add_list"), kinds, |key| add(key));
    });
}

/// A list of names, shown as removable chips.
pub fn pick_list(ui: &mut egui::Ui, salt: &str, values: &mut Vec<String>, kinds: &[Kind]) -> bool {
    let id = ui.id().with("pick_list").with(salt);
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        let mut remove = None;
        for (index, value) in values.iter().enumerate() {
            if ui.small_button(format!("{} 🗙", shown(value, kinds).text())).on_hover_text("Remove").clicked() {
                remove = Some(index);
            }
        }
        if let Some(index) = remove {
            values.remove(index);
            changed = true;
        }
        add_button(ui, id, kinds, |key| {
            if !values.iter().any(|value| value == key) {
                values.push(key.to_string());
                changed = true;
            }
        });
    });
    changed
}

/// A list of (name, amount).
pub fn pick_amounts(ui: &mut egui::Ui, salt: &str, values: &mut Vec<(String, u32)>, kinds: &[Kind]) -> bool {
    let id = ui.id().with("pick_amounts").with(salt);
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        let mut remove = None;
        for (index, (key, amount)) in values.iter_mut().enumerate() {
            ui.label(shown(key, kinds));
            changed |= ui.add(egui::DragValue::new(amount).range(1..=9999).prefix("x").speed(0.1)).changed();
            if ui.small_button("🗙").on_hover_text("Remove").clicked() {
                remove = Some(index);
            }
        }
        if let Some(index) = remove {
            values.remove(index);
            changed = true;
        }
        add_button(ui, id, kinds, |key| {
            values.push((key.to_string(), 1));
            changed = true;
        });
    });
    changed
}
