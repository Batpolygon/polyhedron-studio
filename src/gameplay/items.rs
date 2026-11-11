//! Data-driven items (`project/items/*.ron`), authored in the 🗡 Armurerie.
//!
//! An item is a small model made of coloured boxes (1/16 block units, like
//! the forge elements) plus its gameplay: weapon damage and knockback, tool
//! mining speed, food healing, stack size and an optional Trame program.
//! Items appear in the creative inventory, can be dropped, picked up and
//! held in the hand.

use std::fs;
use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::app_state::EditorUiState;
use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::gameplay::inventory::{HotbarItem, Inventory};
use crate::voxel::biomes::{file_stem, ron_files};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemKind {
    /// Crafting material, no special use.
    #[default]
    Material,
    /// Mines faster (`mining_speed`, on `mining_target` blocks when set:
    /// Pickaxe = rock, Axe = floral).
    Tool,
    /// Melee weapon (`damage`, `knockback`), uses the sword combo.
    Weapon,
    /// Use (right click) heals `heal` hearts and consumes one.
    Food,
    /// Wire: shows the input / output ports of the machines and connects an
    /// output to an input of the same kind.
    Cable,
    /// Ranged weapon: `Attack` fires a bolt (`damage`), consuming one `ammo`
    /// resource per shot in survival (Gun: Mineral, VoltiGun: Voltic).
    Blaster,
    /// Remote control: connects to a Controller and drives it from afar.
    Remote,
    /// Recolours the aimed block with the colour configured beforehand (`C`).
    Brush,
    /// Rotates the aimed block one step.
    Wrench,
    /// Binds an entity in exchange for resources: it follows and fights
    /// with the player.
    Binder,
    /// Mines every resource very fast and sometimes gives Atomic.
    Atomixer,
    /// Gravity gun: grabs and moves entities / movables, costs Ethereal.
    EtheraGun,
    /// Explosion that breaks blocks (resources collected) and throws the
    /// entities, costs Conflux.
    BlastyGun,
    /// Worn with `Use`: damage reduction (`armor`), extra jumps, flight,
    /// bonus damage and health.
    Armor,
    /// Teleports to an Etheraltar, costs Ethereal.
    Etherealink,
}

impl ItemKind {
    pub const ALL: [Self; 15] = [
        Self::Material,
        Self::Tool,
        Self::Weapon,
        Self::Food,
        Self::Cable,
        Self::Blaster,
        Self::Remote,
        Self::Brush,
        Self::Wrench,
        Self::Binder,
        Self::Atomixer,
        Self::EtheraGun,
        Self::BlastyGun,
        Self::Armor,
        Self::Etherealink,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Material => "Material",
            Self::Tool => "Tool",
            Self::Weapon => "Weapon",
            Self::Food => "Food",
            Self::Cable => "Wire",
            Self::Blaster => "Gun",
            Self::Remote => "Remote control",
            Self::Brush => "Brush",
            Self::Wrench => "Wrench",
            Self::Binder => "Binder",
            Self::Atomixer => "Atomixer",
            Self::EtheraGun => "EtheraGun",
            Self::BlastyGun => "BlastyGun",
            Self::Armor => "Armor",
            Self::Etherealink => "Etherealink",
        }
    }
}

/// One coloured box of an item model, in 1/16 block units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemPart {
    #[serde(default)]
    pub name: String,
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub color: [u8; 3],
}

impl ItemPart {
    pub fn new(name: &str, from: [f32; 3], to: [f32; 3], color: [u8; 3]) -> Self {
        Self {
            name: name.to_string(),
            from,
            to,
            color,
        }
    }

    /// Bounds in block units (min, max).
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let a = Vec3::from_array(self.from) / 16.0;
        let b = Vec3::from_array(self.to) / 16.0;
        (a.min(b), a.max(b))
    }
}

/// One hit of a melee combo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComboStep {
    pub damage: f32,
    pub knockback: f32,
}

/// One thing an item does when used.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ItemEffect {
    /// Stops the chain when the player cannot pay.
    Spend { resource: String, amount: u32 },
    /// Consumes the held item.
    SpendHeld,
    Heal(f32),
    Stamina(f32),
    /// Instant shot along the view.
    Beam { damage: f32, range: f32, knockback: f32 },
    /// Hits what stands around the player.
    Zone { radius: f32, damage: f32, knockback: f32 },
    /// Explosion at the aimed point (see the Rules panel).
    Blast,
    /// Throws the player along the view.
    Dash(f32),
    Sparks(u32),
    Sound(String),
    /// Sends a Weave message.
    Broadcast(String),
}

impl ItemEffect {
    pub fn palette() -> Vec<Self> {
        vec![
            Self::Spend { resource: "mineral".to_string(), amount: 1 },
            Self::SpendHeld,
            Self::Heal(2.0),
            Self::Stamina(2.0),
            Self::Beam { damage: 2.0, range: 40.0, knockback: 2.0 },
            Self::Zone { radius: 4.0, damage: 2.0, knockback: 6.0 },
            Self::Blast,
            Self::Dash(14.0),
            Self::Sparks(12),
            Self::Sound("laser".to_string()),
            Self::Broadcast("item".to_string()),
        ]
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::Spend { .. } => "Spend",
            Self::SpendHeld => "Consume the item",
            Self::Heal(_) => "Heal",
            Self::Stamina(_) => "Stamina",
            Self::Beam { .. } => "Beam",
            Self::Zone { .. } => "Area hit",
            Self::Blast => "Explosion",
            Self::Dash(_) => "Dash",
            Self::Sparks(_) => "Sparks",
            Self::Sound(_) => "Sound",
            Self::Broadcast(_) => "Weave message",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemDef {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub kind: ItemKind,
    #[serde(default)]
    pub parts: Vec<ItemPart>,
    #[serde(default = "default_damage")]
    pub damage: f32,
    #[serde(default = "default_knockback")]
    pub knockback: f32,
    #[serde(default = "default_one")]
    pub mining_speed: f32,
    #[serde(default)]
    pub heal: f32,
    #[serde(default = "default_stack")]
    pub stack: u32,
    /// Size of the model in the hand.
    #[serde(default = "default_one")]
    pub hold_scale: f32,
    #[serde(default)]
    pub script: Option<String>,
    /// Resource spent per use (Gun: mineral, VoltiGun: voltic, EtheraGun:
    /// ethereal, BlastyGun: conflux, Etherealink: ethereal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ammo: Option<String>,
    /// Tools: the faster mining only applies to this block or group
    /// (`@mineral`, `@floral`); `None` = every block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mining_target: Option<String>,
    /// Armor: damage points removed from every hit taken.
    #[serde(default)]
    pub armor: f32,
    /// Armor: extra jumps in the air.
    #[serde(default)]
    pub extra_jumps: u32,
    /// Armor: lets the player fly in survival.
    #[serde(default)]
    pub flight: bool,
    /// Armor: added to every attack.
    #[serde(default)]
    pub bonus_damage: f32,
    /// Armor: extra hearts.
    #[serde(default)]
    pub bonus_health: f32,
    /// Effects of the Use button, whatever the kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_use: Vec<ItemEffect>,
    /// Effects of the Attack button, whatever the kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_attack: Vec<ItemEffect>,
    /// Seconds between two uses of the effects.
    #[serde(default)]
    pub cooldown: f32,
    /// Melee hits of this weapon (empty = the combo of the Rules panel).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub combo: Vec<ComboStep>,
}

fn default_damage() -> f32 {
    2.0
}
fn default_knockback() -> f32 {
    3.0
}
fn default_one() -> f32 {
    1.0
}
fn default_stack() -> u32 {
    64
}

impl ItemDef {
    pub fn new(id: &str, label: &str, kind: ItemKind) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            kind,
            parts: Vec::new(),
            damage: default_damage(),
            knockback: default_knockback(),
            mining_speed: 1.0,
            heal: 0.0,
            stack: default_stack(),
            hold_scale: 1.0,
            script: None,
            ammo: None,
            mining_target: None,
            armor: 0.0,
            extra_jumps: 0,
            flight: false,
            bonus_damage: 0.0,
            bonus_health: 0.0,
            on_use: Vec::new(),
            on_attack: Vec::new(),
            cooldown: 0.0,
            combo: Vec::new(),
        }
    }

    /// Main colour (first part), for icons and particles.
    pub fn color(&self) -> [u8; 3] {
        self.model().first().map(|p| p.color).unwrap_or([200, 200, 210])
    }

    /// The boxes drawn in the hand, as a drop and as an icon: the authored
    /// parts, or (no part yet) the engine model of an item of the same kind,
    /// so an item never becomes invisible.
    pub fn model(&self) -> std::borrow::Cow<'_, [ItemPart]> {
        if !self.parts.is_empty() {
            return std::borrow::Cow::Borrowed(&self.parts);
        }
        std::borrow::Cow::Owned(placeholder_parts(self.kind))
    }

    /// Bounds of the whole model in block units.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        let model = self.model();
        for part in model.iter() {
            let (a, b) = part.bounds();
            min = min.min(a);
            max = max.max(b);
        }
        if model.is_empty() { (Vec3::ZERO, Vec3::ONE) } else { (min, max) }
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct ItemLibrary {
    pub items: Vec<ItemDef>,
    pub selection: usize,
    pub revision: u64,
    pub last_saved_revision: u64,
}

impl ItemLibrary {
    pub fn dir(project: &ProjectLayout) -> PathBuf {
        project.module_path("items")
    }

    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.last_saved_revision
    }

    pub fn find(&self, id: &str) -> Option<&ItemDef> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn load_from_project(&mut self, project: &ProjectLayout) -> Result<Vec<String>, String> {
        let dir = Self::dir(project);
        fs::create_dir_all(&dir).map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        let mut warnings = Vec::new();
        let mut items = Vec::new();
        for path in ron_files(&dir)? {
            match load_ron::<ItemDef>(&path) {
                Ok(item) => items.push(item),
                Err(error) => warnings.push(error),
            }
        }
        // Example items of earlier versions are removed (their file too).
        for retired in RETIRED_ITEMS {
            if items.iter().any(|item| item.id == retired) {
                items.retain(|item| item.id != retired);
                let _ = fs::remove_file(dir.join(format!("{}.ron", file_stem(retired))));
            }
        }
        let empty = items.is_empty();
        // Every engine tool missing from the folder is added (and written).
        let mut added = false;
        for item in default_items() {
            if !items.iter().any(|existing| existing.id == item.id) {
                items.push(item);
                added = true;
            }
        }
        items.sort_by(|a, b| a.label.cmp(&b.label));
        self.items = items;
        self.selection = self.selection.min(self.items.len().saturating_sub(1));
        self.touch();
        self.last_saved_revision = self.revision;
        if empty || added {
            for item in &self.items {
                let path = dir.join(format!("{}.ron", file_stem(&item.id)));
                if !path.exists() {
                    save_ron(&path, item)?;
                }
            }
        }
        Ok(warnings)
    }

    pub fn save_all(&mut self, project: &ProjectLayout) -> Result<usize, String> {
        let dir = Self::dir(project);
        for item in &self.items {
            save_ron(&dir.join(format!("{}.ron", file_stem(&item.id))), item)?;
        }
        let keep: Vec<String> = self.items.iter().map(|i| format!("{}.ron", file_stem(&i.id))).collect();
        for path in ron_files(&dir)? {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if !keep.contains(&name) {
                let _ = fs::remove_file(&path);
            }
        }
        self.last_saved_revision = self.revision;
        Ok(self.items.len())
    }
}

/// Items created as examples by earlier versions (removed on load).
pub const RETIRED_ITEMS: [&str; 2] = ["apple", "crystal_blade"];

/// Every tool and armour of the game.
pub fn default_items() -> Vec<ItemDef> {
    let wood = [150, 104, 66];
    let stone = [140, 150, 165];
    let steel = [210, 220, 235];
    let grip = [102, 64, 38];
    let gold = [240, 190, 80];
    let part = ItemPart::new;
    let one = |mut item: ItemDef| {
        item.stack = 1;
        item
    };

    let mut sword = one(ItemDef::new("sword", "Sword", ItemKind::Weapon));
    sword.damage = 2.0;
    sword.knockback = 3.0;
    sword.parts = vec![
        part("Grip", [7.2, 0.0, 7.2], [8.8, 3.2, 8.8], grip),
        part("Guard", [4.2, 3.2, 7.4], [11.8, 3.8, 8.6], gold),
        part("Blade", [7.4, 3.8, 7.8], [8.6, 14.0, 8.2], steel),
        part("Tip", [7.7, 14.0, 7.8], [8.3, 14.8, 8.2], steel),
    ];

    let mut pickaxe = one(ItemDef::new("stone_pickaxe", "Pickaxe", ItemKind::Tool));
    pickaxe.mining_speed = 3.0;
    pickaxe.mining_target = Some("@mineral".to_string());
    pickaxe.parts = vec![
        part("Handle", [7.0, 0.0, 7.0], [9.0, 14.0, 9.0], wood),
        part("Head", [2.0, 12.0, 6.5], [14.0, 15.0, 9.5], stone),
        part("Tip L", [1.0, 10.0, 7.0], [3.0, 13.0, 9.0], stone),
        part("Tip R", [13.0, 10.0, 7.0], [15.0, 13.0, 9.0], stone),
    ];

    let mut axe = one(ItemDef::new("stone_axe", "Axe", ItemKind::Tool));
    axe.mining_speed = 3.0;
    axe.mining_target = Some("@floral".to_string());
    axe.damage = 2.0;
    axe.parts = vec![
        part("Handle", [7.0, 0.0, 7.0], [9.0, 14.0, 9.0], wood),
        part("Blade", [8.5, 9.0, 7.0], [13.0, 15.0, 9.0], stone),
    ];

    let mut gun = one(ItemDef::new("blaster", "Gun", ItemKind::Blaster));
    gun.damage = 3.0;
    gun.ammo = Some("mineral".to_string());
    gun.parts = vec![
        part("Grip", [7.0, 0.0, 6.6], [9.0, 2.6, 9.4], grip),
        part("Body", [6.8, 2.6, 5.6], [9.2, 8.0, 10.4], [150, 156, 170]),
        part("Barrel", [7.4, 8.0, 7.4], [8.6, 10.6, 8.6], steel),
        part("Sight", [7.6, 2.0, 5.0], [8.4, 4.0, 5.6], gold),
    ];

    let mut brush = one(ItemDef::new("brush", "Brush", ItemKind::Brush));
    brush.damage = 0.5;
    brush.parts = vec![
        part("Handle", [7.3, 0.0, 7.3], [8.7, 10.0, 8.7], wood),
        part("Ferrule", [7.0, 10.0, 7.0], [9.0, 11.5, 9.0], steel),
        part("Bristles", [6.8, 11.5, 6.8], [9.2, 15.0, 9.2], [230, 90, 160]),
    ];

    let mut wrench = one(ItemDef::new("wrench", "Wrench", ItemKind::Wrench));
    wrench.damage = 1.0;
    wrench.parts = vec![
        part("Handle", [7.2, 0.0, 7.4], [8.8, 11.0, 8.6], [120, 126, 140]),
        part("Jaw L", [5.6, 11.0, 7.4], [7.2, 14.5, 8.6], steel),
        part("Jaw R", [8.8, 11.0, 7.4], [10.4, 14.5, 8.6], steel),
    ];

    let mut wire = one(ItemDef::new("cable", "Wire", ItemKind::Cable));
    wire.damage = 0.5;
    wire.parts = vec![
        part("Coil", [4.0, 2.0, 6.0], [12.0, 8.0, 10.0], [40, 44, 52]),
        part("Wire", [7.0, 8.0, 7.0], [9.0, 14.0, 9.0], gold),
        part("Plug", [6.5, 14.0, 6.5], [9.5, 16.0, 9.5], [90, 200, 240]),
    ];

    let mut remote = one(ItemDef::new("remote", "Remote Control", ItemKind::Remote));
    remote.parts = vec![
        part("Case", [7.0, 0.0, 7.6], [9.0, 3.2, 8.4], [70, 170, 235]),
        part("Antenna", [8.5, 3.2, 7.9], [8.8, 5.8, 8.2], steel),
        part("Button", [7.6, 1.4, 7.4], [8.4, 2.2, 7.6], [240, 200, 80]),
    ];

    let mut binder = one(ItemDef::new("binder", "Binder", ItemKind::Binder));
    binder.ammo = Some("faunal".to_string());
    binder.parts = vec![
        part("Handle", [7.2, 0.0, 7.2], [8.8, 6.0, 8.8], grip),
        part("Ring", [5.5, 6.0, 7.4], [10.5, 11.0, 8.6], [120, 230, 160]),
        part("Core", [7.4, 7.6, 7.2], [8.6, 9.4, 8.8], [255, 255, 255]),
    ];

    let mut atomixer = one(ItemDef::new("atomixer", "Atomixer", ItemKind::Atomixer));
    atomixer.mining_speed = 8.0;
    atomixer.damage = 2.0;
    atomixer.parts = vec![
        part("Handle", [7.0, 0.0, 7.0], [9.0, 12.0, 9.0], [60, 64, 80]),
        part("Head", [3.0, 11.0, 6.5], [13.0, 14.0, 9.5], [240, 240, 255]),
        part("Core", [7.0, 11.5, 6.0], [9.0, 13.5, 10.0], [140, 220, 255]),
    ];

    let mut etheragun = one(ItemDef::new("etheragun", "EtheraGun", ItemKind::EtheraGun));
    etheragun.ammo = Some("ethereal".to_string());
    etheragun.damage = 0.0;
    etheragun.parts = vec![
        part("Grip", [7.0, 0.0, 6.6], [9.0, 2.6, 9.4], grip),
        part("Body", [6.6, 2.6, 5.4], [9.4, 8.0, 10.6], [200, 150, 255]),
        part("Claw", [6.0, 8.0, 6.0], [10.0, 10.0, 10.0], [240, 220, 255]),
    ];

    let mut blastygun = one(ItemDef::new("blastygun", "BlastyGun", ItemKind::BlastyGun));
    blastygun.ammo = Some("conflux".to_string());
    blastygun.damage = 8.0;
    blastygun.knockback = 12.0;
    blastygun.parts = vec![
        part("Grip", [7.0, 0.0, 6.6], [9.0, 2.6, 9.4], grip),
        part("Body", [6.4, 2.6, 5.0], [9.6, 8.4, 11.0], [255, 150, 70]),
        part("Barrel", [7.0, 8.4, 7.0], [9.0, 11.6, 9.0], [80, 40, 30]),
    ];

    let mut voltigun = one(ItemDef::new("voltigun", "VoltiGun", ItemKind::Blaster));
    voltigun.ammo = Some("voltic".to_string());
    voltigun.damage = 5.0;
    voltigun.parts = vec![
        part("Grip", [7.0, 0.0, 6.6], [9.0, 2.6, 9.4], grip),
        part("Body", [6.8, 2.6, 5.6], [9.2, 8.0, 10.4], [90, 170, 255]),
        part("Coil", [7.2, 8.0, 7.2], [8.8, 11.0, 8.8], [200, 230, 255]),
    ];

    let mut saber = one(ItemDef::new("astral_saber", "AstralSaber", ItemKind::Weapon));
    saber.damage = 4.0;
    saber.knockback = 4.0;
    saber.parts = vec![
        part("Hilt", [7.2, 0.0, 7.2], [8.8, 4.0, 8.8], [60, 64, 80]),
        part("Emitter", [7.0, 4.0, 7.0], [9.0, 4.8, 9.0], gold),
        part("Blade", [7.5, 4.8, 7.5], [8.5, 16.0, 8.5], [255, 236, 150]),
    ];

    let armor = |id: &str, label: &str, color: [u8; 3], reduction: f32| {
        let mut item = ItemDef::new(id, label, ItemKind::Armor);
        item.stack = 1;
        item.damage = 0.5;
        item.armor = reduction;
        item.parts = vec![
            part("Chest", [4.0, 2.0, 6.0], [12.0, 12.0, 10.0], color),
            part("Shoulder L", [2.0, 10.0, 6.5], [4.0, 12.0, 9.5], color),
            part("Shoulder R", [12.0, 10.0, 6.5], [14.0, 12.0, 9.5], color),
            part("Belt", [4.0, 2.0, 5.8], [12.0, 3.5, 10.2], [60, 64, 80]),
        ];
        item
    };
    let mineral_coat = armor("mineral_coat", "MineralCoat", [150, 150, 160], 1.0);
    let mut technic_armor = armor("technic_armor", "TechnicArmor", [160, 170, 190], 2.0);
    technic_armor.extra_jumps = 1;
    let mut refreatech_armor = armor("refreatech_armor", "RefreatechArmor", [200, 220, 255], 3.0);
    refreatech_armor.flight = true;
    let astral_revetment = armor("astral_revetment", "AstralRevetment", [255, 236, 150], 4.0);
    let mut symbiote = armor("mnemo_symbiote", "Mnemo-Symbiote", [220, 74, 92], 2.0);
    symbiote.bonus_damage = 1.0;
    symbiote.bonus_health = 2.0;

    let mut link = one(ItemDef::new("etherealink", "Etherealink", ItemKind::Etherealink));
    link.ammo = Some("ethereal".to_string());
    link.parts = vec![
        part("Base", [6.0, 0.0, 6.0], [10.0, 2.0, 10.0], [60, 64, 80]),
        part("Gem", [6.8, 2.0, 6.8], [9.2, 6.0, 9.2], [200, 150, 255]),
    ];

    vec![
        sword, pickaxe, axe, gun, brush, wrench, wire, remote, binder, atomixer, etheragun, blastygun, voltigun, saber, mineral_coat,
        technic_armor, refreatech_armor, astral_revetment, symbiote, link,
    ]
}

/// Model of an item that has no part: the default item of the same kind
/// (pickaxe for a tool, sword for a weapon...), else a small ingot.
pub fn placeholder_parts(kind: ItemKind) -> Vec<ItemPart> {
    default_items()
        .into_iter()
        .find(|item| item.kind == kind && !item.parts.is_empty())
        .map(|item| item.parts)
        .unwrap_or_else(|| {
            vec![
                ItemPart::new("Ingot", [4.0, 0.0, 6.0], [12.0, 4.0, 10.0], [190, 196, 210]),
                ItemPart::new("Top", [5.0, 4.0, 7.0], [11.0, 5.0, 9.0], [225, 230, 240]),
            ]
        })
}

impl ItemLibrary {
    /// The item `id`, or a placeholder (Material model) when it is unknown, so
    /// a hotbar entry always shows something.
    pub fn find_or_placeholder(&self, id: &str) -> std::borrow::Cow<'_, ItemDef> {
        match self.find(id) {
            Some(def) => std::borrow::Cow::Borrowed(def),
            None => std::borrow::Cow::Owned(ItemDef::new(id, id, ItemKind::Material)),
        }
    }
}

/// Look of a spawn egg, shared by its icon and the egg held in hand: a
/// cubic egg coloured like the mob (its first box), with spots of its second
/// box. Boxes as (centre, size, colour), about one unit tall, centred.
pub fn egg_boxes(rig: Option<&crate::gameplay::mobs::model::MobRig>) -> Vec<(Vec3, Vec3, [u8; 3])> {
    let to_u8 = |c: [f32; 3]| c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
    let base = rig.and_then(|rig| rig.boxes.first()).map(|b| to_u8(b.color)).unwrap_or([236, 224, 200]);
    let spot = rig.and_then(|rig| rig.boxes.get(1)).map(|b| to_u8(b.color)).unwrap_or([90, 70, 60]);
    let shade = base.map(|v| (v as f32 * 0.82) as u8);
    vec![
        (Vec3::ZERO, Vec3::new(0.7, 0.86, 0.7), base),
        // Narrower top: the egg shape, in blocks.
        (Vec3::new(0.0, 0.5, 0.0), Vec3::new(0.48, 0.14, 0.48), shade),
        (Vec3::new(0.0, -0.46, 0.0), Vec3::new(0.56, 0.06, 0.56), shade),
        // Spots on three sides.
        (Vec3::new(0.15, 0.18, 0.36), Vec3::new(0.2, 0.2, 0.04), spot),
        (Vec3::new(-0.14, -0.16, 0.36), Vec3::new(0.14, 0.14, 0.04), spot),
        (Vec3::new(0.36, 0.0, -0.12), Vec3::new(0.04, 0.18, 0.18), spot),
        (Vec3::new(0.36, -0.24, 0.16), Vec3::new(0.04, 0.12, 0.12), spot),
        (Vec3::new(-0.36, 0.12, 0.05), Vec3::new(0.04, 0.16, 0.16), spot),
    ]
}

/// Mesh of an item model: coloured boxes (vertex colours), centred on the
/// grip (the bottom centre of the bounds) and in block units.
pub fn item_mesh(def: &ItemDef) -> Mesh {
    let (min, max) = def.bounds();
    let pivot = Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5);
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for part in def.model().iter() {
        let (a, b) = part.bounds();
        let (a, b) = (a - pivot, b - pivot);
        let color = Color::srgb_u8(part.color[0], part.color[1], part.color[2]).to_linear();
        let c = [color.red, color.green, color.blue, 1.0];
        let p = |x: f32, y: f32, z: f32| [x, y, z];
        let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
            ([1.0, 0.0, 0.0], [p(b.x, a.y, a.z), p(b.x, b.y, a.z), p(b.x, b.y, b.z), p(b.x, a.y, b.z)]),
            ([-1.0, 0.0, 0.0], [p(a.x, a.y, b.z), p(a.x, b.y, b.z), p(a.x, b.y, a.z), p(a.x, a.y, a.z)]),
            ([0.0, 1.0, 0.0], [p(a.x, b.y, a.z), p(a.x, b.y, b.z), p(b.x, b.y, b.z), p(b.x, b.y, a.z)]),
            ([0.0, -1.0, 0.0], [p(a.x, a.y, b.z), p(a.x, a.y, a.z), p(b.x, a.y, a.z), p(b.x, a.y, b.z)]),
            ([0.0, 0.0, 1.0], [p(b.x, a.y, b.z), p(b.x, b.y, b.z), p(a.x, b.y, b.z), p(a.x, a.y, b.z)]),
            ([0.0, 0.0, -1.0], [p(a.x, a.y, a.z), p(a.x, b.y, a.z), p(b.x, b.y, a.z), p(b.x, a.y, a.z)]),
        ];
        for (normal, corners) in faces {
            let base = positions.len() as u32;
            for corner in corners {
                positions.push(corner);
                normals.push(normal);
                colors.push(c);
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Gameplay stats of what the player holds, refreshed every frame.
#[derive(Resource, Debug, Clone)]
pub struct HeldTool {
    pub mining_speed: f32,
    /// The faster mining only applies to this block / group.
    pub mining_target: Option<String>,
    /// Melee weapon: (damage, knockback).
    pub weapon: Option<(f32, f32)>,
    /// Food: hearts healed on use.
    pub food: Option<f32>,
    /// The wire is in hand (ports shown, no mining / placing).
    pub cable: bool,
    /// A gun is in hand: damage of its bolt.
    pub blaster: Option<f32>,
    /// A remote control is in hand.
    pub remote: bool,
    /// Melee hits of the held weapon.
    pub combo: Vec<ComboStep>,
    /// Kind and id of the held item.
    pub kind: Option<ItemKind>,
    pub item: Option<String>,
    /// Resource spent per use.
    pub ammo: Option<String>,
    /// Damage / knockback of the held item (special tools).
    pub damage: f32,
    pub knockback: f32,
}

impl Default for HeldTool {
    fn default() -> Self {
        Self {
            combo: Vec::new(),
            mining_speed: 1.0,
            mining_target: None,
            weapon: None,
            food: None,
            cable: false,
            blaster: None,
            remote: false,
            kind: None,
            item: None,
            ammo: None,
            damage: 0.0,
            knockback: 0.0,
        }
    }
}


pub struct ItemPlugin;

impl Plugin for ItemPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ItemLibrary>()
            .init_resource::<HeldTool>()
            .add_systems(Startup, load_items.after(crate::core::project_io::initialize_project_system))
            .add_systems(Update, (update_held_tool, eat_food).chain());
    }
}

fn load_items(project: Res<ProjectLayout>, mut library: ResMut<ItemLibrary>, mut ui: ResMut<EditorUiState>) {
    match library.load_from_project(&project) {
        Ok(warnings) => {
            ui.info(format!("Armoury: {} item(s).", library.items.len()));
            for warning in warnings {
                ui.warn(warning);
            }
        }
        Err(error) => ui.error(format!("Items: {error}")),
    }
}

fn update_held_tool(inventory: Res<Inventory>, library: Res<ItemLibrary>, mut held: ResMut<HeldTool>) {
    let mut next = HeldTool::default();
    if let HotbarItem::Item(id) = inventory.current() {
        if let Some(def) = library.find(id) {
            next.kind = Some(def.kind);
            next.item = Some(def.id.clone());
            next.combo = def.combo.clone();
            next.ammo = def.ammo.clone();
            next.damage = def.damage;
            next.knockback = def.knockback;
            match def.kind {
                ItemKind::Tool => {
                    next.mining_speed = def.mining_speed.max(0.1);
                    next.mining_target = def.mining_target.clone();
                    next.weapon = Some((def.damage, def.knockback));
                }
                ItemKind::Atomixer => {
                    next.mining_speed = def.mining_speed.max(0.1);
                    next.weapon = Some((def.damage, def.knockback));
                }
                ItemKind::Weapon => {
                    next.weapon = Some((def.damage, def.knockback));
                    next.mining_speed = 0.0;
                }
                ItemKind::Food => next.food = Some(def.heal),
                ItemKind::Cable => {
                    next.cable = true;
                    next.mining_speed = 0.0;
                }
                ItemKind::Blaster => {
                    next.blaster = Some(def.damage);
                    next.mining_speed = 0.0;
                }
                ItemKind::Remote => {
                    next.remote = true;
                    next.mining_speed = 0.0;
                }
                ItemKind::Brush
                | ItemKind::Wrench
                | ItemKind::Binder
                | ItemKind::EtheraGun
                | ItemKind::BlastyGun
                | ItemKind::Armor
                | ItemKind::Etherealink => {
                    next.mining_speed = 0.0;
                }
                ItemKind::Material => {}
            }
        }
    }
    *held = next;
}

/// Food items heal on use and are consumed in survival.
fn eat_food(
    pie: Res<crate::core::pie_manager::PieState>,
    input: crate::core::settings::GameInput,
    held: Res<HeldTool>,
    mut inventory: ResMut<Inventory>,
    mut stats: ResMut<crate::gameplay::player_stats::PlayerStats>,
    mut ui: ResMut<EditorUiState>,
) {
    let Some(heal) = held.food else {
        return;
    };
    if !pie.is_playing() || !pie.cursor_captured || !input.just_pressed(crate::core::settings::GameAction::Use) {
        return;
    }
    if stats.health >= stats.max_health {
        return;
    }
    let HotbarItem::Item(id) = inventory.current().clone() else {
        return;
    };
    if inventory.survival && !inventory.take(&id, 1) {
        ui.warn("Nothing left to eat.");
        return;
    }
    stats.heal(heal);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_items_round_trip_and_mesh() {
        for item in default_items() {
            let text = ron::ser::to_string(&item).expect("serialize");
            let back: ItemDef = ron::from_str(&text).expect("parse");
            assert_eq!(back, item);
            let mesh = item_mesh(&item);
            assert_eq!(mesh.count_vertices(), item.parts.len() * 24);
        }
    }
}
