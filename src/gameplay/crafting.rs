//! Crafting: resources, recipes, crafting stations and the Omnicore network.
//!
//! Everything is project data: `project/crafting.ron` lists the resources,
//! the resource groups (`@floral`, `@mineral`, `@crystalline`), the recipes of every
//! station and the default cost of blocks and items. It is written with the
//! engine defaults on first run and can be edited freely (no recompilation).
//!
//! - **Hand** (inventory, Craft tab): any block or item of the project. A
//!   block / item without its own recipe costs `default_cost` (Synthetic).
//! - **Synthesizer**: 1 Floral + 1 Mineral (or 1 Floral + 1 Crystalline)
//!   -> **Synthetic**, the universal material.
//! - **Assembler**: Synthetic, Technic, Conflux and the end-game Omnicore.
//! - **Infuser**: infusions of two resources (Celliom, Aetech, Voltiom,
//!   Refractal, Fluxiom, Refreatech). A failed infusion (no recipe for the
//!   two resources) gives **Garbage**. Needs power.
//! - **Atomiser**: Conflux (or Garbage) + 100 Voltic -> **Atomic**.
//!
//! The **Omnicore network** is a per-world pool: in survival every chat
//! command spends 1 Omnicore from it. An Omnichorus block transfers the
//! Omnicore carried in the inventory into the network.

use std::sync::{LazyLock, RwLock, RwLockReadGuard};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::project_io::ProjectLayout;
use crate::gameplay::inventory::{HotbarItem, Inventory};
use crate::gameplay::items::ItemLibrary;
use crate::voxel::blocks::BlockRegistry;

/// File of the crafting data inside the project folder.
pub const CRAFTING_FILE: &str = "crafting.ron";

/// Where a recipe is crafted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Station {
    Hand,
    Synthesizer,
    Assembler,
    Infuser,
    Atomiser,
}

impl Station {
    pub const ALL: [Self; 5] = [Self::Hand, Self::Synthesizer, Self::Assembler, Self::Infuser, Self::Atomiser];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Hand => "Hand",
            Self::Synthesizer => "Synthesizer",
            Self::Assembler => "Assembler",
            Self::Infuser => "Infuser",
            Self::Atomiser => "Atomiser",
        }
    }

    /// Stations that only work while powered.
    pub const fn needs_power(self) -> bool {
        matches!(self, Self::Infuser | Self::Atomiser | Self::Assembler)
    }
}

/// Tier of a raw resource (inventory colour and sorting).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tier {
    Raw,
    Basic,
    Overcharged,
    EndGame,
}

impl Tier {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Raw => "Natural resource",
            Self::Basic => "Crafted resource",
            Self::Overcharged => "Infused resource",
            Self::EndGame => "End-game resource",
        }
    }
}

/// A resource that is neither a block nor an item.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceInfo {
    pub id: String,
    pub label: String,
    pub color: [u8; 3],
    pub tier: Tier,
    #[serde(default)]
    pub description: String,
}

/// A named set of resources accepted by a recipe ingredient (`@floral`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceGroup {
    /// Key used in the recipes, starting with `@`.
    pub id: String,
    pub label: String,
    pub members: Vec<String>,
}

/// One crafting recipe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    pub station: Station,
    pub output: String,
    #[serde(default = "one")]
    pub count: u32,
    pub inputs: Vec<(String, u32)>,
}

const fn one() -> u32 {
    1
}

/// Turns a collected block (or any member of a group) into a pure resource
/// from the inventory: grass -> vegetal matter, stone -> mineral matter...
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conversion {
    /// Resource id or group key (`@vegetal`).
    pub from: String,
    pub to: String,
    #[serde(default = "one")]
    pub amount: u32,
}

/// Format generation of `crafting.ron`: older files are replaced by the
/// engine defaults (a `.bak` copy is kept).
pub const BOOK_VERSION: u32 = 3;

/// Every crafting rule of the project.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CraftingBook {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub resources: Vec<ResourceInfo>,
    #[serde(default)]
    pub groups: Vec<ResourceGroup>,
    #[serde(default)]
    pub recipes: Vec<Recipe>,
    /// Hand cost of a block or an item that has no recipe of its own.
    #[serde(default)]
    pub default_cost: Vec<(String, u32)>,
    /// Inventory conversions of collected blocks into pure resources.
    #[serde(default)]
    pub conversions: Vec<Conversion>,
}

fn res(id: &str, label: &str, color: [u8; 3], tier: Tier, description: &str) -> ResourceInfo {
    ResourceInfo { id: id.to_string(), label: label.to_string(), color, tier, description: description.to_string() }
}

fn recipe(station: Station, output: &str, count: u32, inputs: &[(&str, u32)]) -> Recipe {
    Recipe {
        station,
        output: output.to_string(),
        count,
        inputs: inputs.iter().map(|(key, amount)| (key.to_string(), *amount)).collect(),
    }
}

impl CraftingBook {
    /// The engine defaults, written to `project/crafting.ron` on first run.
    pub fn builtin() -> Self {
        use Station::*;
        use Tier::{Basic, EndGame, Overcharged, Raw};
        let resources = vec![
            // --- Natural -----------------------------------------------------------
            res("floral", "Floral", [96, 200, 90], Raw, "Plants, trees, fibres. Biomass, basic fuel of the Combustor, part of Synthetic."),
            res("mineral", "Mineral", [150, 150, 160], Raw, "Stone, ores, ice. Basic building, tools (Gun ammo), part of Synthetic."),
            res("crystalline", "Crystalline", [110, 230, 240], Raw, "Gems and pure crystals. Optics, resonators, part of Synthetic and energy infusions."),
            res("faunal", "Faunal", [220, 120, 90], Raw, "Harvested from entities. Organic and bio-mechanical infusions."),
            res("ethereal", "Ethereal", [200, 150, 255], Raw, "Harvested from entities. Soul anchor (respawn), mystic and technologic infusions."),
            res("astral", "Astral", [255, 236, 150], Raw, "Extremely rare (crater biome and entities). Ultimate cosmic material of the end game."),
            res("mnemonical", "Mnemonical", [255, 70, 90], Raw, "Ultra-rare catalyst that creates autonomous machine-entities."),
            // --- Energy --------------------------------------------------------------
            res("voltic", "Voltic", [90, 170, 255], Basic, "Pure energy made by machines (Solaronel, Kineticster). Powers every machine."),
            // --- Crafted -------------------------------------------------------------
            res("synthetic", "Synthetic", [120, 230, 200], Basic, "Synthesizer: 1 Floral + 1 Mineral or 1 Floral + 1 Crystalline. Universal material of most blocks and recipes."),
            res("technic", "Technic", [160, 170, 190], Basic, "Assembler: Synthetic + Mineral + Crystalline. Mid-game machine parts, base of the technologic infusions."),
            res("conflux", "Conflux", [255, 180, 90], Basic, "Assembler: Floral + Mineral + Crystalline + Faunal + Ethereal. Main fuel of the Atomiser."),
            res("garbage", "Garbage", [110, 96, 80], Basic, "By-product of a failed infusion. Weak alternative fuel of the Atomiser."),
            res("atomic", "Atomic", [240, 240, 255], Basic, "Atomiser: Conflux (or Garbage) + 100 Voltic. Base of the Reconstructor and the Omnicore."),
            // --- Infused -------------------------------------------------------------
            res("celliom", "Celliom", [240, 120, 160], Overcharged, "Infuser: Faunal + Synthetic. Regenerating bio-polymer, base of the living machines."),
            res("aetech", "Aetech", [150, 120, 255], Overcharged, "Infuser: Technic + Ethereal. Advanced technology for communication and complex machines."),
            res("voltiom", "Voltiom", [60, 120, 255], Overcharged, "Infuser: Voltic + Voltic. Condensed energy (1 Voltiom = 10 Voltic)."),
            res("refractal", "Refractal", [180, 250, 255], Overcharged, "Infuser: Crystalline + Voltic. Catalyst to focus, spread and control energy."),
            res("fluxiom", "Fluxiom", [255, 110, 220], Overcharged, "Infuser: Voltic + Ethereal. Unstable mystic energy for the reality-bending technologies."),
            res("refreatech", "Refreatech", [200, 220, 255], Overcharged, "Infuser: Aetech + Refractal. Logic command core, brain of the end-game programming systems."),
            // --- End game ------------------------------------------------------------
            res("omnicore", "Omnicore", [255, 255, 255], EndGame, "Assembler: Refreatech + Conflux + Fluxiom. Unlocks the programming and world control commands."),
        ];
        let group = |id: &str, label: &str, members: &[&str]| ResourceGroup {
            id: id.to_string(),
            label: label.to_string(),
            members: members.iter().map(|member| member.to_string()).collect(),
        };
        let groups = vec![
            group(
                "@floral",
                "Floral",
                &["floral", "grass", "grass_dark", "log", "leaves", "pine_leaves", "palm_leaves", "burnt_log", "moss", "glow_moss", "mnemo_rot"],
            ),
            group(
                "@mineral",
                "Mineral",
                &[
                    "mineral", "stone", "deepslate", "basalt", "sand", "red_sand", "dirt", "mud", "snow", "ice", "slab_stone", "lava", "alien_crust",
                    "mineral_ore", "burnt_ground", "sandstone", "temple_brick",
                ],
            ),
            group("@crystalline", "Crystalline", &["crystalline", "crystal_ore"]),
        ];
        let recipes = vec![
            // --- Synthesizer -------------------------------------------------------
            recipe(Synthesizer, "synthetic", 1, &[("@floral", 1), ("@mineral", 1)]),
            recipe(Synthesizer, "synthetic", 1, &[("@floral", 1), ("@crystalline", 1)]),
            // --- Assembler ---------------------------------------------------------
            recipe(Assembler, "synthetic", 1, &[("@floral", 1), ("@mineral", 1)]),
            recipe(Assembler, "technic", 1, &[("synthetic", 1), ("@mineral", 1), ("@crystalline", 1)]),
            recipe(Assembler, "conflux", 1, &[("@floral", 1), ("@mineral", 1), ("@crystalline", 1), ("faunal", 1), ("ethereal", 1)]),
            recipe(Assembler, "omnicore", 1, &[("refreatech", 1), ("conflux", 1), ("fluxiom", 1)]),
            // --- Infuser -----------------------------------------------------------
            recipe(Infuser, "celliom", 1, &[("faunal", 1), ("synthetic", 1)]),
            recipe(Infuser, "aetech", 1, &[("technic", 1), ("ethereal", 1)]),
            recipe(Infuser, "voltiom", 1, &[("voltic", 5), ("voltic", 5)]),
            recipe(Infuser, "refractal", 1, &[("@crystalline", 1), ("voltic", 1)]),
            recipe(Infuser, "fluxiom", 1, &[("voltic", 1), ("ethereal", 1)]),
            recipe(Infuser, "refreatech", 1, &[("aetech", 1), ("refractal", 1)]),
            // --- Atomiser (100 Voltic per Atomic) -----------------------------------
            recipe(Atomiser, "atomic", 1, &[("conflux", 1), ("voltic", 100)]),
            recipe(Atomiser, "atomic", 1, &[("garbage", 8), ("voltic", 100)]),
            // --- Hand: the first machines and the tools ---------------------------
            recipe(Hand, "synthesizer", 1, &[("@mineral", 6), ("@floral", 4)]),
            recipe(Hand, "stone_pickaxe", 1, &[("@mineral", 3), ("@floral", 2)]),
            recipe(Hand, "stone_axe", 1, &[("@mineral", 3), ("@floral", 2)]),
            recipe(Hand, "sword", 1, &[("@mineral", 4), ("@floral", 1)]),
            recipe(Hand, "mineral_coat", 1, &[("@mineral", 12)]),
            recipe(Hand, "assembler", 1, &[("synthetic", 8), ("@mineral", 4)]),
            recipe(Hand, "combustor", 1, &[("synthetic", 4), ("@mineral", 6)]),
            recipe(Hand, "solar", 1, &[("synthetic", 4), ("@crystalline", 2)]),
            recipe(Hand, "kineticster", 1, &[("synthetic", 4), ("@mineral", 4)]),
            recipe(Hand, "energizer", 1, &[("technic", 2), ("synthetic", 2)]),
            recipe(Hand, "infuser", 1, &[("technic", 3), ("@crystalline", 2)]),
            recipe(Hand, "container", 1, &[("synthetic", 6)]),
            recipe(Hand, "blaster", 1, &[("technic", 2), ("@mineral", 4)]),
            recipe(Hand, "binder", 1, &[("technic", 1), ("faunal", 2)]),
            recipe(Hand, "technic_armor", 1, &[("technic", 8)]),
            recipe(Hand, "atomiser", 1, &[("technic", 4), ("refractal", 2)]),
            recipe(Hand, "excavator", 1, &[("technic", 4), ("@mineral", 8)]),
            recipe(Hand, "ethereaper", 1, &[("aetech", 2), ("celliom", 2)]),
            recipe(Hand, "etheraltar", 1, &[("ethereal", 6), ("technic", 2)]),
            recipe(Hand, "etherealink", 1, &[("aetech", 1), ("ethereal", 4)]),
            recipe(Hand, "etheragun", 1, &[("aetech", 2), ("ethereal", 4)]),
            recipe(Hand, "voltigun", 1, &[("refractal", 2), ("technic", 2)]),
            recipe(Hand, "blastygun", 1, &[("conflux", 2), ("technic", 3)]),
            recipe(Hand, "atomixer", 1, &[("atomic", 2), ("technic", 2)]),
            recipe(Hand, "refreatech_armor", 1, &[("refreatech", 4), ("technic", 4)]),
            recipe(Hand, "reconstructor", 1, &[("atomic", 4), ("refreatech", 1)]),
            recipe(Hand, "omnichorus", 1, &[("refreatech", 2), ("fluxiom", 2)]),
            recipe(Hand, "omnireactor", 1, &[("refreatech", 2), ("atomic", 2)]),
            recipe(Hand, "invocateral", 1, &[("fluxiom", 2), ("ethereal", 10)]),
            recipe(Hand, "astral_saber", 1, &[("astral", 3), ("refractal", 1)]),
            recipe(Hand, "astral_revetment", 1, &[("astral", 8), ("technic", 4)]),
            recipe(Hand, "vacuum", 1, &[("astral", 2), ("technic", 4)]),
            recipe(Hand, "astral_modifier", 1, &[("astral", 4), ("refreatech", 1)]),
            recipe(Hand, "astral_container", 1, &[("astral", 4), ("synthetic", 20)]),
            recipe(Hand, "mnemo_symbiote", 1, &[("mnemonical", 2), ("celliom", 4)]),
            recipe(Hand, "mnemo_digestor", 1, &[("mnemonical", 1), ("celliom", 4)]),
            recipe(Hand, "harvester", 1, &[("mnemonical", 1), ("celliom", 3)]),
            recipe(Hand, "mnemo_cloner", 1, &[("mnemonical", 2), ("celliom", 4), ("aetech", 1)]),
        ];
        let conversions = vec![
            Conversion { from: "@floral".to_string(), to: "floral".to_string(), amount: 1 },
            Conversion { from: "@mineral".to_string(), to: "mineral".to_string(), amount: 1 },
            Conversion { from: "@crystalline".to_string(), to: "crystalline".to_string(), amount: 1 },
        ];
        Self { version: BOOK_VERSION, resources, groups, recipes, default_cost: vec![("synthetic".to_string(), 1)], conversions }
    }

    pub fn path(project: &ProjectLayout) -> std::path::PathBuf {
        project.root.join(CRAFTING_FILE)
    }

    /// Read `project/crafting.ron`, writing the defaults when it is missing.
    pub fn load_or_install(project: &ProjectLayout) -> Result<Self, String> {
        let path = Self::path(project);
        if !path.is_file() {
            let book = Self::builtin();
            crate::core::project_io::save_ron(&path, &book)?;
            return Ok(book);
        }
        let book: Self = crate::core::project_io::load_ron(&path)?;
        if book.version < BOOK_VERSION {
            // Older generation: keep a copy, install the current defaults.
            let _ = std::fs::copy(&path, path.with_extension("ron.bak"));
            let fresh = Self::builtin();
            crate::core::project_io::save_ron(&path, &fresh)?;
            return Ok(fresh);
        }
        Ok(book)
    }

    pub fn save(&self, project: &ProjectLayout) -> Result<(), String> {
        crate::core::project_io::save_ron(&Self::path(project), self)
    }
}

static BOOK: LazyLock<RwLock<CraftingBook>> = LazyLock::new(|| RwLock::new(CraftingBook::builtin()));
static BOOK_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Changes whenever the crafting data is edited.
pub fn book_revision() -> u64 {
    BOOK_REVISION.load(std::sync::atomic::Ordering::Relaxed)
}

/// The crafting data in use.
pub fn book() -> RwLockReadGuard<'static, CraftingBook> {
    BOOK.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Replace the crafting data (project load, studio edit).
pub fn set_book(book: CraftingBook) {
    *BOOK.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = book;
    BOOK_REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub fn resource_info(id: &str) -> Option<ResourceInfo> {
    book().resources.iter().find(|info| info.id == id).cloned()
}

pub fn is_resource(id: &str) -> bool {
    book().resources.iter().any(|info| info.id == id)
}

/// Ids of every resource of the project.
pub fn resource_ids() -> Vec<String> {
    book().resources.iter().map(|info| info.id.clone()).collect()
}

/// Display name of any resource key (raw resource, group or plain id).
pub fn resource_label(id: &str) -> String {
    {
        let book = book();
        if let Some(info) = book.resources.iter().find(|info| info.id == id) {
            return info.label.clone();
        }
        if let Some(group) = book.groups.iter().find(|group| group.id == id) {
            return group.label.clone();
        }
    }
    let mut text = id.trim_start_matches('@').replace('_', " ");
    if let Some(first) = text.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    text
}

/// Display name of a resource using the block / item libraries.
pub fn display_name(id: &str, registry: &BlockRegistry, items: &ItemLibrary) -> String {
    if let Some(block) = registry.find(id) {
        return registry.get(block).label.clone();
    }
    if let Some(item) = items.find(id) {
        return item.label.clone();
    }
    resource_label(id)
}

/// Pure resource a collected key converts to: (target, units per unit).
pub fn conversion_for(key: &str) -> Option<(String, u32)> {
    let book = book();
    for conversion in &book.conversions {
        if conversion.to == key {
            continue;
        }
        let matches = conversion.from == key
            || book.groups.iter().any(|group| group.id == conversion.from && group.members.iter().any(|member| member == key));
        if matches {
            return Some((conversion.to.clone(), conversion.amount.max(1)));
        }
    }
    None
}

/// Convert `units` of `key` in the inventory; returns (target, produced).
pub fn convert(inventory: &mut Inventory, key: &str, units: u32) -> Result<(String, u32), String> {
    let (target, amount) = conversion_for(key).ok_or_else(|| format!("{} cannot be converted.", resource_label(key)))?;
    let units = units.min(inventory.count(key));
    if units == 0 {
        return Err("Nothing to convert.".to_string());
    }
    let produced = units * amount;
    if inventory.room_for(&target, produced) < produced {
        return Err("The inventory is full.".to_string());
    }
    inventory.take(key, units);
    inventory.add(&target, produced);
    crate::scripting::context::emit_game_event(crate::scripting::context::GameEvent::Crafted, &target, produced as f32, None);
    Ok((target, produced))
}

/// Explicit recipes of a station.
pub fn recipes_for(station: Station) -> Vec<Recipe> {
    book().recipes.iter().filter(|recipe| recipe.station == station).cloned().collect()
}

/// Everything a station offers. The hand also crafts every block and item of
/// the project: those without a recipe of their own cost `default_cost`.
pub fn catalog(station: Station, registry: &BlockRegistry, items: &ItemLibrary) -> Vec<Recipe> {
    let mut list = recipes_for(station);
    if station != Station::Hand {
        return list;
    }
    let book = book();
    if book.default_cost.is_empty() {
        return list;
    }
    let has_recipe = |id: &str| book.recipes.iter().any(|recipe| recipe.output == id);
    let cost: Vec<(String, u32)> = book.default_cost.clone();
    for (_, def) in registry.blocks() {
        let craftable = !def.is_liquid() && def.hardness < 1000.0 && def.variant_of.is_none() && !def.hidden;
        if craftable && !has_recipe(&def.id) {
            list.push(Recipe { station: Station::Hand, output: def.id.clone(), count: 1, inputs: cost.clone() });
        }
    }
    for item in &items.items {
        if !has_recipe(&item.id) {
            list.push(Recipe { station: Station::Hand, output: item.id.clone(), count: 1, inputs: cost.clone() });
        }
    }
    list
}

/// Members of a group key, or the key itself.
fn members(key: &str) -> Vec<String> {
    if key.starts_with('@') {
        if let Some(group) = book().groups.iter().find(|group| group.id == key) {
            return group.members.clone();
        }
    }
    vec![key.to_string()]
}

/// Does ingredient `key` (an id or a group) accept `resource`?
pub fn accepts(key: &str, resource: &str) -> bool {
    members(key).iter().any(|member| member == resource)
}

/// The recipe of `station` whose ingredients are exactly the resources put in
/// the slots (in any order; a group ingredient accepts any of its members).
pub fn match_slots(station: Station, slots: &[String]) -> Option<Recipe> {
    fn assign(inputs: &[(String, u32)], slots: &[String], used: &mut [bool]) -> bool {
        let Some(((key, _), rest)) = inputs.split_first() else {
            return true;
        };
        for index in 0..slots.len() {
            if !used[index] && accepts(key, &slots[index]) {
                used[index] = true;
                if assign(rest, slots, used) {
                    return true;
                }
                used[index] = false;
            }
        }
        false
    }
    if slots.is_empty() {
        return None;
    }
    recipes_for(station)
        .into_iter()
        .find(|recipe| recipe.inputs.len() == slots.len() && assign(&recipe.inputs, slots, &mut vec![false; slots.len()]))
}

/// A failed infusion: the two resources in the Infuser slots match no recipe
/// and turn into one Garbage.
pub fn failed_infusion(slots: &[String]) -> Option<Recipe> {
    if slots.len() < 2 || match_slots(Station::Infuser, slots).is_some() {
        return None;
    }
    Some(Recipe {
        station: Station::Infuser,
        output: "garbage".to_string(),
        count: 1,
        inputs: slots.iter().map(|key| (key.clone(), 1)).collect(),
    })
}

/// Units of `key` (or of its group) in the inventory.
pub fn available(inventory: &Inventory, key: &str) -> u32 {
    members(key).iter().map(|member| inventory.count(member)).sum()
}

/// The inventory holds every ingredient.
pub fn can_afford(inventory: &Inventory, recipe: &Recipe) -> bool {
    // Group ingredients may overlap with explicit ones (log is vegetal):
    // simulate the takes on a copy.
    let mut copy = inventory.clone();
    take_inputs(&mut copy, recipe)
}

/// Remove the ingredients; `false` (and nothing removed) when one is missing.
fn take_inputs(inventory: &mut Inventory, recipe: &Recipe) -> bool {
    let mut plan: Vec<(String, u32)> = Vec::new();
    let mut scratch = inventory.resources.clone();
    // Explicit ingredients first, then the groups take whatever is left.
    let mut ordered: Vec<&(String, u32)> = recipe.inputs.iter().collect();
    ordered.sort_by_key(|(key, _)| key.starts_with('@'));
    for (key, amount) in ordered {
        let mut missing = *amount;
        for member in members(key) {
            if missing == 0 {
                break;
            }
            let stock = scratch.get(&member).copied().unwrap_or(0);
            let used = stock.min(missing);
            if used > 0 {
                *scratch.entry(member.clone()).or_insert(0) -= used;
                plan.push((member, used));
                missing -= used;
            }
        }
        if missing > 0 {
            return false;
        }
    }
    for (member, amount) in plan {
        inventory.take(&member, amount);
    }
    true
}

/// Craft `recipe` once. Creative crafts for free. Returns the produced
/// resource and count, or an error message.
pub fn craft(
    inventory: &mut Inventory,
    recipe: &Recipe,
    registry: &BlockRegistry,
    items: &ItemLibrary,
) -> Result<(String, u32), String> {
    if inventory.survival && inventory.room_for(&recipe.output, recipe.count) < recipe.count {
        return Err("The inventory is full.".to_string());
    }
    if inventory.survival && !take_inputs(inventory, recipe) {
        return Err(format!("Missing ingredients for {}.", display_name(&recipe.output, registry, items)));
    }
    inventory.add(&recipe.output, recipe.count);
    if let Some(item) = HotbarItem::from_resource(&recipe.output, registry, items) {
        if inventory.survival {
            inventory.auto_assign(item);
        }
    }
    crate::scripting::context::emit_game_event(crate::scripting::context::GameEvent::Crafted, &recipe.output, recipe.count as f32, None);
    Ok((display_name(&recipe.output, registry, items), recipe.count))
}

/// Per-world pool of Omnicore spent by chat commands in survival.
#[derive(Resource, Debug, Default)]
pub struct OmnicoreNetwork {
    pub stored: u32,
    /// Changed since the world meta was last written.
    pub dirty: bool,
}

impl OmnicoreNetwork {
    pub fn spend(&mut self, amount: u32) -> bool {
        if self.stored < amount {
            return false;
        }
        self.stored -= amount;
        self.dirty = true;
        true
    }

    pub fn deposit(&mut self, amount: u32) {
        self.stored = self.stored.saturating_add(amount);
        self.dirty = true;
    }
}

pub struct CraftingPlugin;

impl Plugin for CraftingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OmnicoreNetwork>()
            .add_systems(Startup, load_crafting_book.after(crate::core::project_io::initialize_project_system))
            .add_systems(Update, reset_network_in_studio_play.after(crate::core::pie_manager::PieSet::Transition));
    }
}

/// Read `project/crafting.ron` (installed with the defaults when missing).
fn load_crafting_book(project: Res<ProjectLayout>, mut ui: ResMut<crate::core::app_state::EditorUiState>) {
    match CraftingBook::load_or_install(&project) {
        Ok(loaded) => {
            ui.info(format!("Crafting: {} resource(s), {} recipe(s).", loaded.resources.len(), loaded.recipes.len()));
            set_book(loaded);
        }
        Err(error) => ui.error(format!("Crafting data not loaded ({error}): engine defaults used.")),
    }
}

/// Play started from the studio (F7) has no saved world: empty network.
fn reset_network_in_studio_play(pie: Res<crate::core::pie_manager::PieState>, mut network: ResMut<OmnicoreNetwork>) {
    if pie.started_this_frame && pie.persist_world.is_none() {
        network.stored = 0;
        network.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn survival() -> Inventory {
        Inventory { survival: true, ..Inventory::default() }
    }

    #[test]
    fn groups_accept_any_member_and_nothing_is_taken_on_failure() {
        let registry = BlockRegistry::builtin();
        let items = ItemLibrary { items: crate::gameplay::items::default_items(), ..Default::default() };
        let recipe = recipes_for(Station::Synthesizer).into_iter().next().expect("synthesizer recipe");
        let mut inventory = survival();
        inventory.add("leaves", 1);
        assert!(!can_afford(&inventory, &recipe));
        assert!(craft(&mut inventory, &recipe, &registry, &items).is_err());
        assert_eq!(inventory.count("leaves"), 1);
        inventory.add("basalt", 1);
        assert!(can_afford(&inventory, &recipe));
        let (_, count) = craft(&mut inventory, &recipe, &registry, &items).expect("crafted");
        assert_eq!(count, 1);
        assert_eq!(inventory.count("leaves"), 0);
        assert_eq!(inventory.count("basalt"), 0);
        assert_eq!(inventory.count(&recipe.output), 1);
    }

    #[test]
    fn explicit_ingredients_are_reserved_before_groups() {
        // Hand pickaxe: 3 @mineral + 2 @floral. Logs are floral, never mineral.
        let registry = BlockRegistry::builtin();
        let items = ItemLibrary { items: crate::gameplay::items::default_items(), ..Default::default() };
        let recipe = book().recipes.iter().find(|r| r.output == "stone_pickaxe").expect("pickaxe").clone();
        let mut inventory = survival();
        inventory.add("log", 2);
        inventory.add("stone", 3);
        assert!(craft(&mut inventory, &recipe, &registry, &items).is_ok());
        assert_eq!(inventory.count("log"), 0);
    }

    #[test]
    fn every_recipe_output_is_known() {
        let registry = BlockRegistry::builtin();
        let items = ItemLibrary { items: crate::gameplay::items::default_items(), ..Default::default() };
        for recipe in CraftingBook::builtin().recipes.iter() {
            let known = registry.find(&recipe.output).is_some() || items.find(&recipe.output).is_some() || is_resource(&recipe.output);
            assert!(known, "unknown recipe output {}", recipe.output);
            for (key, _) in &recipe.inputs {
                let known = key.starts_with('@') || registry.find(key).is_some() || items.find(key).is_some() || is_resource(key);
                assert!(known, "unknown ingredient {key} in {}", recipe.output);
            }
        }
    }

    #[test]
    fn the_hand_crafts_every_block_and_item() {
        let registry = BlockRegistry::builtin();
        let items = ItemLibrary { items: crate::gameplay::items::default_items(), ..Default::default() };
        let hand = catalog(Station::Hand, &registry, &items);
        assert!(hand.iter().any(|recipe| recipe.output == "stone"));
        assert!(hand.iter().any(|recipe| recipe.output == "sword"));
        assert!(!hand.iter().any(|recipe| recipe.output == "water"));
        let synth = recipes_for(Station::Synthesizer);
        assert!(synth.iter().all(|recipe| recipe.output == "synthetic"));
    }

    #[test]
    fn slots_find_their_recipe_in_any_order() {
        let recipe = match_slots(Station::Infuser, &["ethereal".to_string(), "voltic".to_string()]).expect("fluxiom");
        assert_eq!(recipe.output, "fluxiom");
        let volt = match_slots(Station::Infuser, &["voltic".to_string(), "voltic".to_string()]).expect("voltiom");
        assert_eq!(volt.output, "voltiom");
        let synth = match_slots(Station::Synthesizer, &["stone".to_string(), "grass".to_string()]).expect("synthetic");
        assert_eq!(synth.output, "synthetic");
        assert!(match_slots(Station::Infuser, &["stone".to_string(), "grass".to_string()]).is_none());
        let garbage = failed_infusion(&["stone".to_string(), "grass".to_string()]).expect("garbage");
        assert_eq!(garbage.output, "garbage");
        let (target, amount) = conversion_for("grass").expect("grass converts");
        assert_eq!((target.as_str(), amount), ("floral", 1));
    }

    #[test]
    fn spec_resources_exist_and_removed_ones_do_not() {
        for id in [
            "floral", "mineral", "crystalline", "faunal", "ethereal", "astral", "voltic", "synthetic", "technic", "celliom", "aetech",
            "voltiom", "refractal", "fluxiom", "conflux", "garbage", "atomic", "refreatech", "omnicore", "mnemonical",
        ] {
            assert!(is_resource(id), "missing resource {id}");
        }
        for id in ["fuel", "coal", "iron", "gold", "alloy", "circuit", "plasma", "prism", "conductor", "magnetite", "vegetal"] {
            assert!(!is_resource(id), "{id} should not exist");
        }
    }

    #[test]
    fn network_spends_only_what_it_has() {
        let mut network = OmnicoreNetwork::default();
        assert!(!network.spend(1));
        network.deposit(2);
        assert!(network.spend(1));
        assert_eq!(network.stored, 1);
    }
}
