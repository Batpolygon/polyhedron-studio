//! Player inventory: resource counts and a 9-slot hotbar.
//!
//! Resources are named stacks (`"stone"`, `"floral"`, `"voltic"`...). Breaking
//! a block adds its drop, machines convert resources, and placing a block in
//! play mode consumes one unit of that block when survival rules are on.
//! The inventory is reset at every PIE start.

use crate::core::game_rules::game_rules;
use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::core::pie_manager::{PieSet, PieState};
use crate::voxel::blocks::{BlockFunction, BlockId, BlockRegistry};

/// What the player holds in a hotbar slot.
#[derive(Clone, Debug, PartialEq)]
pub enum HotbarItem {
    Empty,
    /// LMB mines, RMB places this block.
    Block(BlockId),
    /// Custom item from the armoury (`project/items`), by id.
    Item(String),
    /// Spawn egg of a mob rig (by name): RMB on a block summons it.
    SpawnEgg(String),
    /// A raw resource held in hand (dragged from the inventory): it can be
    /// dropped or given to a machine, not placed.
    Resource(String),
}


/// Slots used by `count` units.
pub fn stacks_for(count: u32) -> usize {
    count.div_ceil(game_rules().inventory.stack_size) as usize
}

/// Items every player starts with (slots 1 to 3).

/// Game mode of the player.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayMode {
    #[default]
    Creative,
    Survival,
    /// Flies through everything, cannot interact, invisible.
    Spectator,
}

impl PlayMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Creative => "Creative",
            Self::Survival => "Survival",
            Self::Spectator => "Spectator",
        }
    }
}

impl HotbarItem {
    pub fn label(&self, registry: &BlockRegistry, items: &crate::gameplay::items::ItemLibrary) -> String {
        match self {
            Self::Empty => "Hand".to_string(),
            Self::Block(id) => registry.get(*id).label.clone(),
            Self::Item(id) => items.find(id).map(|item| item.label.clone()).unwrap_or_else(|| id.clone()),
            Self::SpawnEgg(name) => format!("{name} Egg"),
            Self::Resource(id) => crate::gameplay::crafting::resource_label(id),
        }
    }

    /// Inventory resource counted for this slot in survival (block or item id).
    pub fn resource_key(&self, registry: &BlockRegistry) -> Option<String> {
        match self {
            Self::Block(id) => Some(registry.get(*id).resource_id().to_string()),
            Self::Item(id) => Some(id.clone()),
            Self::SpawnEgg(name) => Some(format!("egg:{name}")),
            Self::Resource(id) => Some(id.clone()),
            _ => None,
        }
    }

    /// Slot for a resource name: a block, an item, or nothing (raw material).
    pub fn from_resource(
        resource: &str,
        registry: &BlockRegistry,
        items: &crate::gameplay::items::ItemLibrary,
    ) -> Option<Self> {
        if let Some(name) = resource.strip_prefix("egg:") {
            return Some(Self::SpawnEgg(name.to_string()));
        }
        if let Some(block) = registry.find(resource) {
            return Some(Self::Block(block));
        }
        items.find(resource).map(|item| Self::Item(item.id.clone()))
    }
}

#[derive(Resource, Debug, Clone)]
pub struct Inventory {
    pub resources: BTreeMap<String, u32>,
    pub hotbar: [HotbarItem; 9],
    pub selected: usize,
    /// When true, placing consumes resources and nothing is free.
    pub survival: bool,
    /// Spectator mode (implies `survival == false`).
    pub spectator: bool,
    /// Revision bumped on every change (HUD bindings).
    pub revision: u64,
    /// Entries to throw on the ground this frame: (resource, count).
    pub throw_requests: Vec<(String, u32)>,
    /// Order of the stacks in the inventory slots (first collected first).
    pub order: Vec<String>,
    /// Survival: what each of the 30 storage slots holds (one resource key
    /// per slot, the hotbar keys excepted). Kept tidy by [`Inventory::tidy`].
    pub grid: Vec<Option<String>>,
    /// Armour worn (item id), equipped with `Use`.
    pub armor: Option<String>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            resources: BTreeMap::new(),
            // The starter tools are Armoury items (`project/items`).
            hotbar: [
                HotbarItem::Item("sword".to_string()),
                HotbarItem::Item("remote".to_string()),
                HotbarItem::Item("blaster".to_string()),
                HotbarItem::Empty,
                HotbarItem::Empty,
                HotbarItem::Empty,
                HotbarItem::Empty,
                HotbarItem::Empty,
                HotbarItem::Empty,
            ],
            selected: 0,
            survival: false,
            spectator: false,
            revision: 0,
            throw_requests: Vec::new(),
            order: Vec::new(),
            grid: vec![None; game_rules().inventory.slots],
            armor: None,
        }
    }
}

impl Inventory {
    /// Fill slots 4..9 with the most useful blocks of the registry.
    pub fn seed_blocks(&mut self, registry: &BlockRegistry, preferred: BlockId) {
        // Hidden blocks (`BlockDef::hidden`, e.g. the legacy cable node) never
        // reach the inventory.
        let mut picks: Vec<BlockId> = vec![preferred].into_iter().filter(|id| !registry.get(*id).hidden).collect();
        for key in ["stone", "water", "frame", "motor", "controller"] {
            if let Some(id) = registry.find(key) {
                if !picks.contains(&id) && !registry.get(id).hidden {
                    picks.push(id);
                }
            }
        }
        for (slot, id) in (3..9).zip(picks) {
            self.hotbar[slot] = HotbarItem::Block(id);
        }
        // Survival-friendly starter kit.
        self.add("floral", 10);
        self.add("mineral", 10);
    }

    pub fn mode(&self) -> PlayMode {
        if self.spectator {
            PlayMode::Spectator
        } else if self.survival {
            PlayMode::Survival
        } else {
            PlayMode::Creative
        }
    }

    pub fn set_mode(&mut self, mode: PlayMode) {
        self.survival = mode == PlayMode::Survival;
        self.spectator = mode == PlayMode::Spectator;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn current(&self) -> &HotbarItem {
        &self.hotbar[self.selected.min(8)]
    }

    /// Put a slot in the first empty hotbar cell unless it is already there.
    pub fn auto_assign(&mut self, item: HotbarItem) {
        if self.hotbar.contains(&item) {
            return;
        }
        if let Some(slot) = self.hotbar.iter().position(|slot| *slot == HotbarItem::Empty) {
            self.hotbar[slot] = item;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn count(&self, resource: &str) -> u32 {
        self.resources.get(resource).copied().unwrap_or(0)
    }

    pub fn add(&mut self, resource: &str, amount: u32) {
        if amount == 0 {
            return;
        }
        *self.resources.entry(resource.to_string()).or_insert(0) += amount;
        if !self.order.iter().any(|key| key == resource) {
            self.order.push(resource.to_string());
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Slots in use (stacks of [`STACK_SIZE`]).
    pub fn used_slots(&self) -> usize {
        self.resources.values().map(|count| stacks_for(*count)).sum()
    }

    /// How many units of `resource` still fit in the 30 slots (unlimited in
    /// creative).
    pub fn room_for(&self, resource: &str, wanted: u32) -> u32 {
        if !self.survival {
            return wanted;
        }
        let current = self.count(resource);
        let free = game_rules().inventory.slots.saturating_sub(self.used_slots()) as u32;
        let partial = (stacks_for(current) as u32 * game_rules().inventory.stack_size).saturating_sub(current);
        wanted.min(partial + free * game_rules().inventory.stack_size)
    }

    /// Add what fits; returns the units added.
    pub fn add_capped(&mut self, resource: &str, amount: u32) -> u32 {
        let fits = self.room_for(resource, amount);
        self.add(resource, fits);
        fits
    }

    /// Survival layout: the hotbar keys leave the grid, every other owned
    /// key gets a grid slot, spent keys free theirs.
    pub fn tidy(&mut self, registry: &BlockRegistry) {
        if self.grid.len() != game_rules().inventory.slots {
            self.grid.resize(game_rules().inventory.slots, None);
        }
        let hotbar_keys: Vec<String> = self.hotbar.iter().filter_map(|item| item.resource_key(registry)).collect();
        let mut changed = false;
        for slot in self.grid.iter_mut() {
            let drop = match slot {
                Some(key) => hotbar_keys.contains(key) || self.resources.get(key).copied().unwrap_or(0) == 0,
                None => false,
            };
            if drop {
                *slot = None;
                changed = true;
            }
        }
        // First seen, first placed.
        let mut keys: Vec<String> = self.order.iter().filter(|key| self.count(key) > 0).cloned().collect();
        for key in self.resources.keys() {
            if !keys.contains(key) && self.count(key) > 0 {
                keys.push(key.clone());
            }
        }
        for key in keys {
            if hotbar_keys.contains(&key) || self.grid.iter().any(|slot| slot.as_deref() == Some(key.as_str())) {
                continue;
            }
            if let Some(free) = self.grid.iter().position(Option::is_none) {
                self.grid[free] = Some(key);
                changed = true;
            }
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Survival: swap two storage slots.
    pub fn swap_grid(&mut self, a: usize, b: usize) {
        if a < self.grid.len() && b < self.grid.len() && a != b {
            self.grid.swap(a, b);
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Creative: put `key` in storage slot `cell` (a key lives in one slot
    /// only: an older slot of the same key empties).
    pub fn set_grid(&mut self, cell: usize, key: Option<String>) {
        if cell >= self.grid.len() {
            return;
        }
        if let Some(key) = &key {
            for slot in self.grid.iter_mut() {
                if slot.as_deref() == Some(key.as_str()) {
                    *slot = None;
                }
            }
        }
        self.grid[cell] = key;
        self.revision = self.revision.wrapping_add(1);
    }

    /// Survival: exchange storage slot `cell` with hotbar slot `slot` (either
    /// may be empty).
    pub fn swap_grid_hotbar(&mut self, cell: usize, slot: usize, registry: &BlockRegistry, items: &crate::gameplay::items::ItemLibrary) {
        if cell >= self.grid.len() || slot >= self.hotbar.len() {
            return;
        }
        let from_grid = self.grid[cell].take();
        let from_hotbar = std::mem::replace(&mut self.hotbar[slot], HotbarItem::Empty);
        self.grid[cell] = from_hotbar.resource_key(registry);
        if let Some(key) = from_grid {
            self.hotbar[slot] = HotbarItem::from_resource(&key, registry, items).unwrap_or(HotbarItem::Resource(key));
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// The stacks in slot order: (resource, count <= STACK_SIZE).
    pub fn stacks(&self) -> Vec<(String, u32)> {
        let mut keys: Vec<&String> = self.order.iter().filter(|key| self.count(key) > 0).collect();
        for key in self.resources.keys() {
            if !keys.contains(&key) && self.count(key) > 0 {
                keys.push(key);
            }
        }
        let mut stacks = Vec::new();
        for key in keys {
            let mut left = self.count(key);
            while left > 0 {
                let stack = left.min(game_rules().inventory.stack_size);
                stacks.push((key.clone(), stack));
                left -= stack;
            }
        }
        stacks
    }

    /// Remove `amount` if available.
    pub fn take(&mut self, resource: &str, amount: u32) -> bool {
        let Some(count) = self.resources.get_mut(resource) else {
            return amount == 0;
        };
        if *count < amount {
            return false;
        }
        *count -= amount;
        if *count == 0 {
            self.resources.remove(resource);
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }

}

/// Resource produced when a block is broken.
pub fn drop_for(registry: &BlockRegistry, block: BlockId) -> Option<String> {
    let def = registry.get(block);
    if def.function != BlockFunction::None && def.drop.is_none() {
        return Some(def.id.clone());
    }
    match &def.drop {
        Some(drop) if drop.is_empty() => None,
        Some(drop) => Some(drop.clone()),
        None => Some(def.id.clone()),
    }
}

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inventory>().add_systems(
            Update,
            (reset_inventory_on_pie_start, select_hotbar_slot, clear_spent_slots)
                .chain()
                .after(PieSet::Transition),
        );
    }
}

fn reset_inventory_on_pie_start(
    pie: Res<PieState>,
    registry: Res<BlockRegistry>,
    editor: Res<crate::core::app_state::EditorState>,
    mut inventory: ResMut<Inventory>,
) {
    if pie.started_this_frame {
        // Play started from the studio (F7) is always creative; a world
        // launched from the main menu keeps its own game mode.
        let survival = inventory.survival && pie.persist_world.is_some();
        *inventory = Inventory {
            survival,
            ..Inventory::default()
        };
        if !survival {
            // Creative: a ready-made hotbar; survival starts with tools only.
            inventory.seed_blocks(&registry, editor.selected_block);
        } else {
            inventory.add("floral", 10);
            for tool in game_rules().starter_tools.0.iter() {
                inventory.add(tool, 1);
            }
        }
    }
}

/// Survival: a hotbar slot whose block / item / resource ran out empties;
/// the storage slots stay tidy.
fn clear_spent_slots(pie: Res<PieState>, registry: Res<BlockRegistry>, mut inventory: ResMut<Inventory>) {
    if !pie.is_playing() {
        return;
    }
    if inventory.survival {
        inventory.tidy(&registry);
    }
    let survival = inventory.survival;
    let spent: Vec<usize> = inventory
        .hotbar
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let hidden = matches!(item, HotbarItem::Block(id) if registry.get(*id).hidden);
            hidden || (survival && item.resource_key(&registry).is_some_and(|key| inventory.count(&key) == 0))
        })
        .map(|(slot, _)| slot)
        .collect();
    // Stored units of hidden blocks.
    let hidden_keys: Vec<String> = inventory
        .resources
        .keys()
        .filter(|key| registry.find(key).is_some_and(|id| registry.get(id).hidden))
        .cloned()
        .collect();
    for key in hidden_keys {
        inventory.resources.remove(&key);
        inventory.order.retain(|entry| *entry != key);
        for slot in inventory.grid.iter_mut() {
            if slot.as_deref() == Some(key.as_str()) {
                *slot = None;
            }
        }
        inventory.revision = inventory.revision.wrapping_add(1);
    }
    if spent.is_empty() {
        return;
    }
    for slot in spent {
        inventory.hotbar[slot] = HotbarItem::Empty;
    }
    inventory.revision = inventory.revision.wrapping_add(1);
}

fn select_hotbar_slot(
    pie: Res<PieState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    mut inventory: ResMut<Inventory>,
) {
    if !pie.is_playing() || !pie.cursor_captured || inventory.spectator {
        return;
    }
    const KEYS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    for (index, key) in KEYS.iter().enumerate() {
        if keyboard.just_pressed(*key) {
            inventory.selected = index;
            inventory.revision = inventory.revision.wrapping_add(1);
        }
    }
    let scroll = mouse_scroll.delta.y;
    if scroll.abs() > 0.5 {
        let step = if scroll > 0.0 { 8 } else { 1 };
        inventory.selected = (inventory.selected + step) % 9;
        inventory.revision = inventory.revision.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_only_succeeds_with_enough_stock() {
        let mut inventory = Inventory::default();
        inventory.add("stone", 3);
        assert!(!inventory.take("stone", 4));
        assert!(inventory.take("stone", 3));
        assert_eq!(inventory.count("stone"), 0);
        assert!(inventory.resources.is_empty());
    }

    #[test]
    fn grid_slots_follow_the_hotbar_and_can_be_reordered() {
        let registry = BlockRegistry::builtin();
        let items = crate::gameplay::items::ItemLibrary { items: crate::gameplay::items::default_items(), ..Default::default() };
        let mut inventory = Inventory { survival: true, hotbar: std::array::from_fn(|_| HotbarItem::Empty), ..Inventory::default() };
        inventory.add("floral", 5);
        inventory.add("stone", 3);
        inventory.tidy(&registry);
        assert_eq!(inventory.grid[0].as_deref(), Some("floral"));
        assert_eq!(inventory.grid[1].as_deref(), Some("stone"));
        inventory.swap_grid(0, 5);
        assert_eq!(inventory.grid[5].as_deref(), Some("floral"));
        // Into the hotbar and back: nothing is lost.
        inventory.swap_grid_hotbar(1, 2, &registry, &items);
        assert!(matches!(inventory.hotbar[2], HotbarItem::Block(_)));
        inventory.tidy(&registry);
        assert!(!inventory.grid.iter().any(|slot| slot.as_deref() == Some("stone")));
        assert_eq!(inventory.count("stone"), 3);
        inventory.swap_grid_hotbar(7, 2, &registry, &items);
        assert_eq!(inventory.grid[7].as_deref(), Some("stone"));
        assert_eq!(inventory.hotbar[2], HotbarItem::Empty);
    }
}
