//! Serializable mob rig, action clip and keyframe vocabulary.
//!
//! A mob is a hierarchy of boxes ("bones"). Each box stores its full extents,
//! a pivot expressed relative to the box centre, and a rest offset relative to
//! the parent pivot. Sockets attach to a box and expose an anchor point for
//! weapons, cameras or particles. Action clips animate boxes with keyframes.
//!
//! Everything is plain data: the editor, the sampler and the `.ron` files all
//! speak the same types, so a rig round-trips without engine state.

use serde::{Deserialize, Serialize};

/// Interpolation used for the segment that *starts* at a keyframe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interpolation {
    /// Hold the value until the next key.
    Step,
    /// Straight line between two keys.
    Linear,
    /// Cubic ease-in/ease-out between two keys.
    Bezier,
    /// Tangent-continuous Catmull-Rom through the surrounding keys.
    Auto,
}

impl Interpolation {
    pub const ALL: [Self; 4] = [Self::Step, Self::Linear, Self::Bezier, Self::Auto];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Step => "Step",
            Self::Linear => "Linear",
            Self::Bezier => "Bezier",
            Self::Auto => "Auto",
        }
    }
}

impl Default for Interpolation {
    fn default() -> Self {
        Self::Auto
    }
}

/// One animated box pose at a given time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub time: f32,
    #[serde(default)]
    pub translation: [f32; 3],
    /// Euler angles in degrees, applied in XYZ order.
    #[serde(default)]
    pub rotation: [f32; 3],
    #[serde(default = "one")]
    pub scale: [f32; 3],
    #[serde(default)]
    pub interpolation: Interpolation,
}

impl Keyframe {
    pub fn new(time: f32) -> Self {
        Self {
            time,
            translation: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
            interpolation: Interpolation::Auto,
        }
    }

    pub fn with_translation(mut self, translation: [f32; 3]) -> Self {
        self.translation = translation;
        self
    }

    pub fn with_rotation(mut self, rotation: [f32; 3]) -> Self {
        self.rotation = rotation;
        self
    }

    pub fn with_scale(mut self, scale: [f32; 3]) -> Self {
        self.scale = scale;
        self
    }

    pub fn with_interpolation(mut self, interpolation: Interpolation) -> Self {
        self.interpolation = interpolation;
        self
    }
}

/// Keyframes attached to one box inside one clip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoneTrack {
    pub bone: String,
    pub keys: Vec<Keyframe>,
}

impl BoneTrack {
    pub fn new(bone: impl Into<String>) -> Self {
        Self {
            bone: bone.into(),
            keys: Vec::new(),
        }
    }

    /// Insert a key, keeping the track sorted and replacing any key at the same
    /// time. Returns the index of the stored key.
    pub fn upsert(&mut self, key: Keyframe) -> usize {
        let time = key.time.max(0.0);
        let mut key = key;
        key.time = time;

        if let Some(index) = self
            .keys
            .iter()
            .position(|existing| (existing.time - time).abs() <= 1e-4)
        {
            self.keys[index] = key;
            return index;
        }

        let index = self
            .keys
            .iter()
            .position(|existing| existing.time > time)
            .unwrap_or(self.keys.len());
        self.keys.insert(index, key);
        index
    }

    pub fn remove_at(&mut self, time: f32) -> bool {
        if let Some(index) = self
            .keys
            .iter()
            .position(|existing| (existing.time - time).abs() <= 1e-4)
        {
            self.keys.remove(index);
            return true;
        }
        false
    }

    pub fn key_at(&self, time: f32) -> Option<&Keyframe> {
        self.keys
            .iter()
            .find(|key| (key.time - time).abs() <= 1e-4)
    }

    pub fn key_at_mut(&mut self, time: f32) -> Option<&mut Keyframe> {
        self.keys
            .iter_mut()
            .find(|key| (key.time - time).abs() <= 1e-4)
    }
}

/// A named animation clip made of per-box tracks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionClip {
    pub name: String,
    #[serde(default)]
    pub duration: f32,
    #[serde(default)]
    pub looping: bool,
    #[serde(default)]
    pub tracks: Vec<BoneTrack>,
}

impl ActionClip {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            duration: 1.0,
            looping: true,
            tracks: Vec::new(),
        }
    }

    pub fn track(&self, bone: &str) -> Option<&BoneTrack> {
        self.tracks.iter().find(|track| track.bone == bone)
    }

    pub fn track_mut(&mut self, bone: &str) -> Option<&mut BoneTrack> {
        self.tracks.iter_mut().find(|track| track.bone == bone)
    }

    /// Create the track when missing.
    pub fn ensure_track(&mut self, bone: &str) -> &mut BoneTrack {
        if let Some(index) = self.tracks.iter().position(|track| track.bone == bone) {
            return &mut self.tracks[index];
        }
        self.tracks.push(BoneTrack::new(bone));
        let last = self.tracks.len() - 1;
        &mut self.tracks[last]
    }

    pub fn remove_track(&mut self, bone: &str) -> bool {
        if let Some(index) = self.tracks.iter().position(|track| track.bone == bone) {
            self.tracks.remove(index);
            return true;
        }
        false
    }

    pub fn key_count(&self) -> usize {
        self.tracks.iter().map(|track| track.keys.len()).sum()
    }

    pub fn duration_or(&self, fallback: f32) -> f32 {
        if self.duration.is_finite() && self.duration > 0.0 {
            self.duration
        } else {
            fallback
        }
    }
}

/// One box of the rig.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MobBox {
    pub name: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default = "unit")]
    pub size: [f32; 3],
    /// Rotation origin, relative to the box centre.
    #[serde(default)]
    pub pivot: [f32; 3],
    /// Rest position of the pivot, relative to the parent pivot.
    #[serde(default)]
    pub offset: [f32; 3],
    /// Rest rotation in degrees.
    #[serde(default)]
    pub rotation: [f32; 3],
    #[serde(default = "bone_color")]
    pub color: [f32; 3],
    /// Glow of the box (eyes, fires, crystals).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub emissive: f32,
    #[serde(default = "default_true")]
    pub visible: bool,
}

fn is_zero(value: &f32) -> bool {
    *value == 0.0
}

impl MobBox {
    pub fn new(name: impl Into<String>, parent: Option<String>) -> Self {
        Self {
            name: name.into(),
            parent,
            size: [0.5; 3],
            pivot: [0.0; 3],
            offset: [0.0; 3],
            rotation: [0.0; 3],
            color: bone_color(),
            emissive: 0.0,
            visible: true,
        }
    }
}

/// Attachment point exposed by a box.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Socket {
    pub name: String,
    pub bone: String,
    #[serde(default)]
    pub offset: [f32; 3],
    /// Optional rotation hint in degrees, used by gameplay attachments.
    #[serde(default)]
    pub rotation: [f32; 3],
}

impl Socket {
    pub fn new(name: impl Into<String>, bone: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            bone: bone.into(),
            offset: [0.0; 3],
            rotation: [0.0; 3],
        }
    }
}

/// What a rig is used for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RigRole {
    /// A creature spawned in the world.
    #[default]
    Mob,
    /// The third-person player body (also the HUD and menu replicas).
    Player,
    /// The first-person arm, in camera space.
    PlayerHand,
}

impl RigRole {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mob => "Mob",
            Self::Player => "Player body",
            Self::PlayerHand => "Player hand (first person)",
        }
    }
}

/// Name of the player body rig.
pub const PLAYER_RIG: &str = "Player";
/// Name of the first-person hand rig.
pub const PLAYER_HAND_RIG: &str = "Player Hand";

/// A complete mob: box hierarchy, sockets and action clips.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MobRig {
    pub name: String,
    /// Mob, player body or first-person hand.
    #[serde(default)]
    pub role: RigRole,
    #[serde(default)]
    pub description: String,
    #[serde(default = "one")]
    pub scale: [f32; 3],
    #[serde(default)]
    pub boxes: Vec<MobBox>,
    #[serde(default)]
    pub sockets: Vec<Socket>,
    #[serde(default)]
    pub clips: Vec<ActionClip>,
    #[serde(default)]
    pub default_clip: String,
    /// Gameplay: hit points when spawned in play mode.
    #[serde(default = "default_mob_health")]
    pub health: f32,
    /// Gameplay: bound Blockly program driving the AI.
    #[serde(default = "default_mob_script")]
    pub script: Option<String>,
    /// Gameplay: hostile mobs attack the player, passive ones can be tamed.
    #[serde(default = "default_true_flag")]
    pub hostile: bool,
    /// Gameplay: resource dropped on death.
    #[serde(default)]
    pub loot: Option<String>,
    /// Gameplay: how many copies spawn around the player when PIE starts.
    #[serde(default = "default_spawn_count")]
    pub spawn_count: u32,
    /// Gameplay: extra Trame programs bound to every copy (with `script`).
    #[serde(default)]
    pub scripts: Vec<String>,
    /// Gameplay: multiplier of every movement speed requested by the AI.
    #[serde(default = "default_one")]
    pub speed: f32,
    /// Gameplay: multiplier of the damage of every attack.
    #[serde(default = "default_one")]
    pub damage: f32,
    /// Gameplay: seconds between two attacks.
    #[serde(default = "default_attack_cooldown")]
    pub attack_cooldown: f32,
    /// Gameplay: multiplier of the attack reach.
    #[serde(default = "default_one")]
    pub reach: f32,
    /// Gameplay: `0` = full knockback, `1` = immovable.
    #[serde(default)]
    pub knockback_resistance: f32,
    /// Gameplay: native behaviour (`Script` = the bound Weave programs only).
    #[serde(default)]
    pub behavior: MobBehavior,
    /// Sound of a hit, made in the Ambience workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hurt_sound: Option<String>,
    /// Sound of its death.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub death_sound: Option<String>,
    /// Gameplay: rules of the `Brain` behaviour.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub brain: Vec<crate::gameplay::mobs::brain::BrainRule>,
    /// Gameplay: biomes where it spawns naturally (empty = never).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub biomes: Vec<String>,
    /// Gameplay: when it spawns naturally.
    #[serde(default)]
    pub spawn_time: SpawnTime,
    /// Gameplay: Ethereal it holds (Ethereaper harvest, x2).
    #[serde(default = "default_ethereal")]
    pub ethereal: f32,
    /// Gameplay: extra resources dropped on death (Faunal, Ethereal...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drops: Vec<(String, u32)>,
    /// Gameplay: entities it is afraid of (rig names).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fears: Vec<String>,
    /// Gameplay: can be ridden once bound (Galopin).
    #[serde(default)]
    pub rideable: bool,
    /// Gameplay: explodes when it dies (Grom).
    #[serde(default)]
    pub explodes: bool,
}

/// Native behaviours of the spec entities (`gameplay::mob_ai`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MobBehavior {
    /// Driven only by its Weave programs.
    #[default]
    Script,
    /// Assembled in the editor from rules.
    Brain,
    /// Hits back when hit and defends the entities being hit (Gim-Gim).
    Retaliate,
    /// Aggressive, attacks in packs (Rockle).
    Pack,
    /// Defends its territory, calm outside of it (Gardener).
    Territorial,
    /// Flees whatever touches it, then hides (Miby).
    Fearful,
    /// Harmless with lots of health, flees only when weak (Wise).
    Tank,
    /// Comes close discreetly to socialise, scared when hit, rideable when
    /// bound (Galopin).
    Social,
    /// Aggressive, shoots laser projectiles (Sulfy).
    Shooter,
    /// Hides and leaps at the player (Cronil).
    Ambush,
    /// Passive unless attacked, aggressive toward every small mob (Squipter).
    Bully,
    /// Gathers resources, places pure resource blocks for its nest,
    /// reproduces, explodes on death (Grom).
    Nester,
    /// Strikes then flees when hit or afraid (Tolossus).
    HitAndRun,
    /// Senses movement: walking -> comes gently, running / blocks -> charges
    /// with a knockback blow (Niff).
    Noise,
    /// Throws far away whoever attacks it (Gelanto).
    Repel,
    /// Jumps and strikes from behind, flees when hit (Leafer).
    Backstab,
    /// Attacks when hit or walked on (Arna).
    Stepped,
    /// Attacks every entity (Notturo).
    Hunter,
    /// Slow and harmless by day, aggressive toward everyone at night;
    /// strikes back by day (Vorator).
    Nocturnal,
    /// Attracts the entities around with a sound, then attacks (Clanger).
    Caller,
    /// Attacks the player (Sarcophager).
    PlayerHunter,
    /// Attacks everyone (MnemoEater).
    Berserk,
    /// Never moves, only watches; ignores hits (MnemoWatcher).
    Watcher,
}

impl MobBehavior {
    pub const ALL: [Self; 23] = [
        Self::Script,
        Self::Brain,
        Self::Retaliate,
        Self::Pack,
        Self::Territorial,
        Self::Fearful,
        Self::Tank,
        Self::Social,
        Self::Shooter,
        Self::Ambush,
        Self::Bully,
        Self::Nester,
        Self::HitAndRun,
        Self::Noise,
        Self::Repel,
        Self::Backstab,
        Self::Stepped,
        Self::Hunter,
        Self::Nocturnal,
        Self::Caller,
        Self::PlayerHunter,
        Self::Berserk,
        Self::Watcher,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Script => "Weave script",
            Self::Brain => "Brain (rules)",
            Self::Retaliate => "Retaliate / defend",
            Self::Pack => "Pack hunter",
            Self::Territorial => "Territorial",
            Self::Fearful => "Fearful (flee & hide)",
            Self::Tank => "Tank",
            Self::Social => "Social (rideable)",
            Self::Shooter => "Laser shooter",
            Self::Ambush => "Ambush",
            Self::Bully => "Bully of small mobs",
            Self::Nester => "Nester (gathers, breeds, explodes)",
            Self::HitAndRun => "Hit and run",
            Self::Noise => "Noise sensitive",
            Self::Repel => "Repel attackers",
            Self::Backstab => "Backstab",
            Self::Stepped => "Attacks when hit or stepped on",
            Self::Hunter => "Hunts entities",
            Self::Nocturnal => "Nocturnal",
            Self::Caller => "Caller (ringer)",
            Self::PlayerHunter => "Player hunter",
            Self::Berserk => "Berserk",
            Self::Watcher => "Watcher",
        }
    }
}

/// When an entity spawns naturally.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpawnTime {
    #[default]
    Any,
    Day,
    Night,
}

impl SpawnTime {
    pub const ALL: [Self; 3] = [Self::Any, Self::Day, Self::Night];

    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Day and night",
            Self::Day => "Day",
            Self::Night => "Night",
        }
    }
}

fn default_ethereal() -> f32 {
    3.0
}

fn default_one() -> f32 {
    1.0
}

fn default_attack_cooldown() -> f32 {
    0.5
}

fn default_spawn_count() -> u32 {
    1
}

fn default_mob_health() -> f32 {
    10.0
}

fn default_mob_script() -> Option<String> {
    Some("Mob Brain".to_string())
}

fn default_true_flag() -> bool {
    true
}

impl MobRig {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            brain: Vec::new(),
            hurt_sound: None,
            death_sound: None,
            name: name.into(),
            role: RigRole::Mob,
            description: String::new(),
            scale: [1.0; 3],
            boxes: Vec::new(),
            sockets: Vec::new(),
            clips: Vec::new(),
            default_clip: String::new(),
            health: default_mob_health(),
            script: default_mob_script(),
            hostile: true,
            loot: None,
            spawn_count: default_spawn_count(),
            scripts: Vec::new(),
            speed: 1.0,
            damage: 1.0,
            attack_cooldown: default_attack_cooldown(),
            reach: 1.0,
            knockback_resistance: 0.0,
            behavior: MobBehavior::Script,
            biomes: Vec::new(),
            spawn_time: SpawnTime::Any,
            ethereal: default_ethereal(),
            drops: Vec::new(),
            fears: Vec::new(),
            rideable: false,
            explodes: false,
        }
    }

    /// Every program bound to the mob (`script` first, then `scripts`).
    pub fn all_scripts(&self) -> Vec<String> {
        let mut list: Vec<String> = self.script.iter().cloned().collect();
        for name in &self.scripts {
            if !list.contains(name) {
                list.push(name.clone());
            }
        }
        list
    }

    /// Replace the bound programs.
    pub fn set_scripts(&mut self, list: Vec<String>) {
        let mut list = list.into_iter();
        self.script = list.next();
        self.scripts = list.collect();
    }

    pub fn bone(&self, name: &str) -> Option<&MobBox> {
        self.boxes.iter().find(|bone| bone.name == name)
    }

    pub fn bone_mut(&mut self, name: &str) -> Option<&mut MobBox> {
        self.boxes.iter_mut().find(|bone| bone.name == name)
    }

    pub fn bone_index(&self, name: &str) -> Option<usize> {
        self.boxes.iter().position(|bone| bone.name == name)
    }

    pub fn clip(&self, name: &str) -> Option<&ActionClip> {
        self.clips.iter().find(|clip| clip.name == name)
    }

    pub fn clip_mut(&mut self, name: &str) -> Option<&mut ActionClip> {
        self.clips.iter_mut().find(|clip| clip.name == name)
    }

    pub fn clip_index(&self, name: &str) -> Option<usize> {
        self.clips.iter().position(|clip| clip.name == name)
    }

    /// Rename a box and update every reference (children, sockets, tracks).
    pub fn rename_box(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = to.trim();
        if to.is_empty() {
            return Err("a box name cannot be empty.".to_string());
        }
        if from == to {
            return Ok(());
        }
        if self.bone(to).is_some() {
            return Err(format!("a box named '{to}' already exists."));
        }
        let Some(bone) = self.bone_mut(from) else {
            return Err(format!("unknown box '{from}'."));
        };
        bone.name = to.to_string();

        for candidate in &mut self.boxes {
            if candidate.parent.as_deref() == Some(from) {
                candidate.parent = Some(to.to_string());
            }
        }
        for socket in &mut self.sockets {
            if socket.bone == from {
                socket.bone = to.to_string();
            }
        }
        for clip in &mut self.clips {
            if let Some(track) = clip.track_mut(from) {
                track.bone = to.to_string();
            }
        }

        Ok(())
    }

    /// Bones ordered so every parent appears before its children.
    ///
    /// Cycles are broken deterministically: a box whose ancestry loops is
    /// treated as a root, which keeps the viewport usable while the validator
    /// reports the actual problem.
    pub fn hierarchy_order(&self) -> Vec<usize> {
        let mut order = Vec::with_capacity(self.boxes.len());
        let mut placed = vec![false; self.boxes.len()];

        for index in 0..self.boxes.len() {
            let mut chain = Vec::new();
            let mut cursor = Some(index);
            let mut guard = 0;

            while let Some(current) = cursor {
                if placed[current] || chain.contains(&current) || guard > self.boxes.len() {
                    break;
                }
                chain.push(current);
                cursor = self.boxes[current]
                    .parent
                    .as_deref()
                    .and_then(|parent| self.bone_index(parent));
                guard += 1;
            }

            for current in chain.into_iter().rev() {
                if !placed[current] {
                    placed[current] = true;
                    order.push(current);
                }
            }
        }

        order
    }

    /// Depth of a box in the hierarchy, used by the tree view.
    pub fn depth_of(&self, name: &str) -> usize {
        let mut depth = 0;
        let mut cursor = self.bone(name).and_then(|bone| bone.parent.as_deref());
        let mut guard = 0;
        while let Some(parent) = cursor {
            if guard > self.boxes.len() {
                break;
            }
            depth += 1;
            cursor = self.bone(parent).and_then(|bone| bone.parent.as_deref());
            guard += 1;
        }
        depth
    }

    pub fn unique_bone_name(&self, base: &str) -> String {
        let names: Vec<String> = self.boxes.iter().map(|bone| bone.name.clone()).collect();
        unique_name(&names, base)
    }

    pub fn unique_clip_name(&self, base: &str) -> String {
        let names: Vec<String> = self.clips.iter().map(|clip| clip.name.clone()).collect();
        unique_name(&names, base)
    }

    pub fn unique_socket_name(&self, base: &str) -> String {
        let names: Vec<String> = self.sockets.iter().map(|socket| socket.name.clone()).collect();
        unique_name(&names, base)
    }

    /// Add a box as a child of `parent`, with a sensible default offset.
    pub fn add_box(&mut self, parent: Option<String>) -> usize {
        let base = match &parent {
            Some(parent) => format!("{parent} Child"),
            None => "Box".to_string(),
        };
        let name = self.unique_bone_name(&base);
        let mut bone = MobBox::new(name, parent.clone());

        if let Some(parent_name) = &parent {
            if let Some(parent_bone) = self.bone(parent_name) {
                bone.offset = [0.0, parent_bone.size[1].abs() * 0.5 + bone.size[1] * 0.5, 0.0];
                bone.color = shift_color(parent_bone.color, 0.12);
            }
        }

        self.boxes.push(bone);
        self.boxes.len() - 1
    }

    /// Remove a box and re-parent its children to the removed box's parent.
    pub fn remove_box(&mut self, name: &str) -> Option<String> {
        let index = self.bone_index(name)?;
        let removed = self.boxes.remove(index);
        let new_parent = removed.parent.clone();

        for bone in &mut self.boxes {
            if bone.parent.as_deref() == Some(name) {
                bone.parent = new_parent.clone();
            }
        }

        self.sockets.retain(|socket| socket.bone != name);
        for clip in &mut self.clips {
            clip.remove_track(name);
        }

        Some(removed.name)
    }

    /// Re-parent a box while rejecting cycles.
    pub fn reparent(&mut self, name: &str, parent: Option<String>) -> Result<(), String> {
        if let Some(parent) = &parent {
            if parent == name {
                return Err("a box cannot parent itself.".to_string());
            }
            if self.bone(parent).is_none() {
                return Err(format!("unknown parent box '{parent}'."));
            }
            if self.is_ancestor(name, parent) {
                return Err(format!(
                    "'{parent}' is a descendant of '{name}'; the hierarchy would loop."
                ));
            }
        }

        if let Some(bone) = self.bone_mut(name) {
            bone.parent = parent;
            Ok(())
        } else {
            Err(format!("unknown box '{name}'."))
        }
    }

    /// True when `candidate` is inside the subtree rooted at `root`.
    pub fn is_ancestor(&self, root: &str, candidate: &str) -> bool {
        let mut cursor = Some(candidate.to_string());
        let mut guard = 0;
        while let Some(current) = cursor {
            if current == root {
                return true;
            }
            if guard > self.boxes.len() {
                break;
            }
            cursor = self
                .bone(&current)
                .and_then(|bone| bone.parent.clone());
            guard += 1;
        }
        false
    }

    pub fn add_socket(&mut self, bone: String) -> usize {
        let name = self.unique_socket_name(&format!("{bone} Socket"));
        self.sockets.push(Socket::new(name, bone));
        self.sockets.len() - 1
    }

    pub fn remove_socket(&mut self, name: &str) -> Option<String> {
        let index = self.sockets.iter().position(|socket| socket.name == name)?;
        Some(self.sockets.remove(index).name)
    }

    pub fn add_clip(&mut self) -> usize {
        let name = self.unique_clip_name("New Clip");
        self.clips.push(ActionClip::new(name));
        self.clips.len() - 1
    }

    pub fn remove_clip(&mut self, name: &str) -> Option<String> {
        let index = self.clip_index(name)?;
        let removed = self.clips.remove(index);
        if self.default_clip == removed.name {
            self.default_clip = self
                .clips
                .first()
                .map(|clip| clip.name.clone())
                .unwrap_or_default();
        }
        Some(removed.name)
    }

    pub fn duplicate_clip(&mut self, name: &str) -> Option<String> {
        let source = self.clip(name)?.clone();
        let copy_name = self.unique_clip_name(&format!("{} Copy", source.name));
        let mut copy = source;
        copy.name = copy_name;
        self.clips.push(copy);
        Some(name.to_string())
    }

    pub fn resolved_default_clip(&self) -> Option<&ActionClip> {
        if let Some(clip) = self.clip(&self.default_clip) {
            return Some(clip);
        }
        self.clips.first()
    }

    pub fn box_count(&self) -> usize {
        self.boxes.len()
    }
}

/// A small test rig (boxes, sockets and clips) used by the unit tests.
#[cfg(test)]
pub fn starter_rig() -> MobRig {
    let mut rig = MobRig::new("Sentinel Drone");
    rig.description =
        "Hovering sentinel used to validate the mob editor, sockets and clips.".to_string();

    let mut torso = MobBox::new("Torso", None);
    torso.size = [0.7, 0.55, 0.45];
    torso.offset = [0.0, 0.0, 0.0];
    torso.color = [0.31, 0.72, 0.78];
    rig.boxes.push(torso);

    let mut head = MobBox::new("Head", Some("Torso".to_string()));
    head.size = [0.38, 0.34, 0.36];
    head.offset = [0.0, 0.44, 0.0];
    head.color = [0.86, 0.78, 0.45];
    rig.boxes.push(head);

    let mut arm_left = MobBox::new("Arm L", Some("Torso".to_string()));
    arm_left.size = [0.16, 0.5, 0.16];
    arm_left.pivot = [0.0, 0.22, 0.0];
    arm_left.offset = [-0.43, 0.14, 0.0];
    arm_left.color = [0.42, 0.62, 0.72];
    rig.boxes.push(arm_left);

    let mut arm_right = MobBox::new("Arm R", Some("Torso".to_string()));
    arm_right.size = [0.16, 0.5, 0.16];
    arm_right.pivot = [0.0, 0.22, 0.0];
    arm_right.offset = [0.43, 0.14, 0.0];
    arm_right.color = [0.42, 0.62, 0.72];
    rig.boxes.push(arm_right);

    let mut thruster = MobBox::new("Thruster", Some("Torso".to_string()));
    thruster.size = [0.46, 0.18, 0.36];
    thruster.offset = [0.0, -0.38, 0.0];
    thruster.color = [0.72, 0.38, 0.32];
    rig.boxes.push(thruster);

    rig.sockets.push(
        Socket::new("Muzzle", "Arm R")
            .with_offset([0.0, -0.28, 0.16])
            .with_rotation([-90.0, 0.0, 0.0]),
    );
    rig.sockets.push(Socket::new("Antenna", "Head"));
    rig.sockets
        .push(Socket::new("Exhaust", "Thruster").with_offset([0.0, -0.1, 0.0]));

    let mut idle = ActionClip::new("Idle");
    idle.duration = 2.0;
    idle.looping = true;
    idle.ensure_track("Torso").upsert(
        Keyframe::new(0.0)
            .with_translation([0.0, 0.0, 0.0])
            .with_interpolation(Interpolation::Auto),
    );
    idle.track_mut("Torso").unwrap().upsert(
        Keyframe::new(1.0)
            .with_translation([0.0, 0.06, 0.0])
            .with_interpolation(Interpolation::Auto),
    );
    idle.track_mut("Torso").unwrap().upsert(
        Keyframe::new(2.0)
            .with_translation([0.0, 0.0, 0.0])
            .with_interpolation(Interpolation::Auto),
    );
    idle.ensure_track("Arm L").upsert(
        Keyframe::new(0.0)
            .with_rotation([0.0, 0.0, 6.0])
            .with_interpolation(Interpolation::Bezier),
    );
    idle.track_mut("Arm L").unwrap().upsert(
        Keyframe::new(1.0)
            .with_rotation([0.0, 0.0, -6.0])
            .with_interpolation(Interpolation::Bezier),
    );
    idle.track_mut("Arm L").unwrap().upsert(
        Keyframe::new(2.0)
            .with_rotation([0.0, 0.0, 6.0])
            .with_interpolation(Interpolation::Bezier),
    );
    idle.ensure_track("Arm R").upsert(
        Keyframe::new(0.0)
            .with_rotation([0.0, 0.0, -6.0])
            .with_interpolation(Interpolation::Bezier),
    );
    idle.track_mut("Arm R").unwrap().upsert(
        Keyframe::new(1.0)
            .with_rotation([0.0, 0.0, 6.0])
            .with_interpolation(Interpolation::Bezier),
    );
    idle.track_mut("Arm R").unwrap().upsert(
        Keyframe::new(2.0)
            .with_rotation([0.0, 0.0, -6.0])
            .with_interpolation(Interpolation::Bezier),
    );
    idle.ensure_track("Thruster").upsert(
        Keyframe::new(0.0)
            .with_scale([1.0, 1.0, 1.0])
            .with_interpolation(Interpolation::Linear),
    );
    idle.track_mut("Thruster").unwrap().upsert(
        Keyframe::new(0.5)
            .with_scale([1.15, 0.8, 1.15])
            .with_interpolation(Interpolation::Linear),
    );
    idle.track_mut("Thruster").unwrap().upsert(
        Keyframe::new(1.0)
            .with_scale([1.0, 1.0, 1.0])
            .with_interpolation(Interpolation::Linear),
    );
    rig.clips.push(idle);

    let mut scan = ActionClip::new("Scan");
    scan.duration = 1.5;
    scan.looping = false;
    scan.ensure_track("Head").upsert(
        Keyframe::new(0.0)
            .with_rotation([0.0, 0.0, 0.0])
            .with_interpolation(Interpolation::Bezier),
    );
    scan.track_mut("Head").unwrap().upsert(
        Keyframe::new(0.75)
            .with_rotation([0.0, 75.0, 0.0])
            .with_interpolation(Interpolation::Bezier),
    );
    scan.track_mut("Head").unwrap().upsert(
        Keyframe::new(1.5)
            .with_rotation([0.0, 0.0, 0.0])
            .with_interpolation(Interpolation::Bezier),
    );
    scan.ensure_track("Arm R").upsert(
        Keyframe::new(0.0)
            .with_rotation([0.0, 0.0, 0.0])
            .with_interpolation(Interpolation::Step),
    );
    scan.track_mut("Arm R").unwrap().upsert(
        Keyframe::new(0.5)
            .with_rotation([-55.0, 0.0, 0.0])
            .with_interpolation(Interpolation::Step),
    );
    scan.track_mut("Arm R").unwrap().upsert(
        Keyframe::new(1.5)
            .with_rotation([0.0, 0.0, 0.0])
            .with_interpolation(Interpolation::Step),
    );
    rig.clips.push(scan);

    rig.default_clip = "Idle".to_string();
    rig
}

impl Socket {
    pub fn with_offset(mut self, offset: [f32; 3]) -> Self {
        self.offset = offset;
        self
    }

    pub fn with_rotation(mut self, rotation: [f32; 3]) -> Self {
        self.rotation = rotation;
        self
    }
}

fn unique_name(existing: &[String], base: &str) -> String {
    if !existing.iter().any(|name| name == base) {
        return base.to_string();
    }
    let mut counter = 2;
    loop {
        let candidate = format!("{base} {counter}");
        if !existing.iter().any(|name| name == &candidate) {
            return candidate;
        }
        counter += 1;
    }
}

fn shift_color(color: [f32; 3], amount: f32) -> [f32; 3] {
    [
        (color[0] + amount).clamp(0.0, 1.0),
        (color[1] + amount * 0.6).clamp(0.0, 1.0),
        (color[2] + amount * 0.3).clamp(0.0, 1.0),
    ]
}

fn unit() -> [f32; 3] {
    [1.0; 3]
}

fn one() -> [f32; 3] {
    [1.0; 3]
}

fn bone_color() -> [f32; 3] {
    [0.36, 0.62, 0.72]
}

fn default_true() -> bool {
    true
}

/// Default player body: the yellow low-poly character (head with eyes,
/// torso, two-part arms and legs). Bone names drive the animation:
/// `Head`, `Arm L`, `Arm R`, `Leg L`, `Leg R`; the held item hangs on the
/// `Hand` socket.
pub fn default_player_rig() -> MobRig {
    let mut rig = MobRig::new(PLAYER_RIG);
    rig.role = RigRole::Player;
    rig.description = "Third-person player body (also used by the HUD and menu previews).".to_string();
    rig.script = None;
    rig.hostile = false;
    rig.spawn_count = 0;
    let skin = [246.0 / 255.0, 226.0 / 255.0, 32.0 / 255.0];
    let shirt = [skin[0] * 0.92, skin[1] * 0.92, skin[2] * 0.92];
    let pants = [skin[0] * 0.82, skin[1] * 0.82, skin[2] * 0.82];
    let mut add = |name: &str, parent: Option<&str>, size: [f32; 3], pivot: [f32; 3], offset: [f32; 3], color: [f32; 3]| {
        let mut bone = MobBox::new(name, parent.map(str::to_string));
        bone.size = size;
        bone.pivot = pivot;
        bone.offset = offset;
        bone.color = color;
        rig.boxes.push(bone);
    };
    add("Torso", None, [0.5, 0.62, 0.28], [0.0, 0.0, 0.0], [0.0, 0.19, 0.0], shirt);
    add("Head", Some("Torso"), [0.44, 0.44, 0.44], [0.0, -0.22, 0.0], [0.0, 0.31, 0.0], skin);
    for (side, sign) in [("L", -1.0), ("R", 1.0)] {
        add(&format!("Eye {side}"), Some("Head"), [0.1, 0.06, 0.01], [0.0; 3], [sign * 0.11, 0.12, -0.222], [0.97, 0.97, 0.97]);
        add(&format!("Pupil {side}"), Some("Head"), [0.04, 0.045, 0.01], [0.0; 3], [sign * 0.13, 0.11, -0.228], [0.05, 0.05, 0.07]);
    }
    for (side, sign) in [("L", -1.0), ("R", 1.0)] {
        add(&format!("Arm {side}"), Some("Torso"), [0.18, 0.3, 0.18], [0.0, 0.14, 0.0], [sign * 0.36, 0.27, 0.0], shirt);
        add(&format!("Hand {side}"), Some(&format!("Arm {side}")), [0.18, 0.28, 0.18], [0.0; 3], [0.0, -0.44, 0.0], skin);
    }
    for (side, sign) in [("L", -1.0), ("R", 1.0)] {
        add(&format!("Leg {side}"), Some("Torso"), [0.2, 0.6, 0.2], [0.0, 0.3, 0.0], [sign * 0.12, -0.31, 0.0], pants);
        add(&format!("Foot {side}"), Some(&format!("Leg {side}")), [0.2, 0.12, 0.24], [0.0; 3], [0.0, -0.69, -0.02], skin);
    }
    rig.sockets.push(Socket::new("Hand", "Arm R").with_offset([0.0, -0.6, -0.06]).with_rotation([-90.0, 0.0, 0.0]));
    install_player_clips(&mut rig);
    rig
}

/// Default first-person hand (camera space: X right, Y up, -Z forward).
/// The `Arm` bone is the one swung by the animations; the held item hangs
/// on the `Hand` socket.
pub fn default_player_hand_rig() -> MobRig {
    let mut rig = MobRig::new(PLAYER_HAND_RIG);
    rig.role = RigRole::PlayerHand;
    rig.description = "First-person arm, in camera space (X right, Y up, -Z forward).".to_string();
    rig.script = None;
    rig.hostile = false;
    rig.spawn_count = 0;
    let skin = [246.0 / 255.0, 226.0 / 255.0, 32.0 / 255.0];
    let shirt = [skin[0] * 0.92, skin[1] * 0.92, skin[2] * 0.92];
    let mut arm = MobBox::new("Arm", None);
    arm.size = [0.16, 0.16, 0.46];
    arm.pivot = [0.0, 0.0, -0.08];
    arm.offset = [0.0, 0.0, 0.0];
    arm.color = skin;
    rig.boxes.push(arm);
    let mut cuff = MobBox::new("Cuff", Some("Arm".to_string()));
    cuff.size = [0.165, 0.165, 0.06];
    cuff.offset = [0.0, -0.002, 0.3];
    cuff.color = shirt;
    rig.boxes.push(cuff);
    rig.sockets.push(Socket::new("Hand", "Arm").with_offset([-0.01, 0.06, -0.13]).with_rotation([-76.5, 0.0, 8.6]));
    install_player_clips(&mut rig);
    rig
}

// --- Built-in clips of the player rigs -------------------------------------------
//
// The player body and the first-person hand are animated only by clips of
// their rigs (edited in the Mobs workspace like any mob). These are the
// defaults installed in a rig that lacks a clip of that name; edit or delete
// their keys freely (a clip with no key is simply not played).

fn key(time: f32, rotation: [f32; 3]) -> Keyframe {
    Keyframe::new(time).with_rotation(rotation)
}

fn moved(time: f32, translation: [f32; 3], rotation: [f32; 3]) -> Keyframe {
    Keyframe::new(time).with_translation(translation).with_rotation(rotation)
}

fn clip(name: &str, duration: f32, looping: bool, tracks: Vec<(&str, Vec<Keyframe>)>) -> ActionClip {
    let mut clip = ActionClip::new(name);
    clip.duration = duration;
    clip.looping = looping;
    for (bone, keys) in tracks {
        let track = clip.ensure_track(bone);
        for key in keys {
            track.upsert(key);
        }
    }
    clip
}

/// Default clips of the third-person body ("Player" rig).
pub fn default_player_clips() -> Vec<ActionClip> {
    // Walk: one stride cycle (the clip time follows the distance walked).
    let w = 1.05;
    let q = w / 4.0;
    let walk = clip(
        "Walk",
        w,
        true,
        vec![
            ("Leg L", vec![key(0.0, [0.0; 3]), key(q, [43.0, 0.0, 0.0]), key(2.0 * q, [0.0; 3]), key(3.0 * q, [-43.0, 0.0, 0.0]), key(w, [0.0; 3])]),
            ("Leg R", vec![key(0.0, [0.0; 3]), key(q, [-43.0, 0.0, 0.0]), key(2.0 * q, [0.0; 3]), key(3.0 * q, [43.0, 0.0, 0.0]), key(w, [0.0; 3])]),
            ("Arm L", vec![key(0.0, [0.0, 0.0, -3.0]), key(q, [-34.0, 0.0, -3.0]), key(2.0 * q, [0.0, 0.0, -3.0]), key(3.0 * q, [34.0, 0.0, -3.0]), key(w, [0.0, 0.0, -3.0])]),
            ("Arm R", vec![key(0.0, [0.0, 0.0, 3.0]), key(q, [17.0, 0.0, 3.0]), key(2.0 * q, [0.0, 0.0, 3.0]), key(3.0 * q, [-17.0, 0.0, 3.0]), key(w, [0.0, 0.0, 3.0])]),
        ],
    );
    // Idle: slow breathing of the arms.
    let i = 3.9;
    let idle = clip(
        "Idle",
        i,
        true,
        vec![
            ("Arm L", vec![key(0.0, [0.0, 0.0, -3.4]), key(i * 0.25, [2.3, 0.0, -4.7]), key(i * 0.5, [0.0, 0.0, -3.4]), key(i * 0.75, [-2.3, 0.0, -2.1]), key(i, [0.0, 0.0, -3.4])]),
            ("Arm R", vec![key(0.0, [0.0, 0.0, 3.4]), key(i * 0.25, [-2.3, 0.0, 4.7]), key(i * 0.5, [0.0, 0.0, 3.4]), key(i * 0.75, [2.3, 0.0, 2.1]), key(i, [0.0, 0.0, 3.4])]),
            ("Head", vec![key(0.0, [0.0; 3]), key(i * 0.5, [2.0, 0.0, 0.0]), key(i, [0.0; 3])]),
        ],
    );
    // Swim: crawl strokes and flutter kicks.
    let s = 1.8;
    let swim = clip(
        "Swim",
        s,
        true,
        vec![
            ("Arm L", vec![key(0.0, [137.0, 0.0, 0.0]), key(s * 0.25, [103.0, 0.0, 0.0]), key(s * 0.5, [137.0, 0.0, 0.0]), key(s * 0.75, [172.0, 0.0, 0.0]), key(s, [137.0, 0.0, 0.0])]),
            ("Arm R", vec![key(0.0, [137.0, 0.0, 0.0]), key(s * 0.25, [172.0, 0.0, 0.0]), key(s * 0.5, [137.0, 0.0, 0.0]), key(s * 0.75, [103.0, 0.0, 0.0]), key(s, [137.0, 0.0, 0.0])]),
            ("Leg L", vec![key(0.0, [0.0; 3]), key(s * 0.125, [20.0, 0.0, 0.0]), key(s * 0.375, [-20.0, 0.0, 0.0]), key(s * 0.625, [20.0, 0.0, 0.0]), key(s * 0.875, [-20.0, 0.0, 0.0]), key(s, [0.0; 3])]),
            ("Leg R", vec![key(0.0, [0.0; 3]), key(s * 0.125, [-20.0, 0.0, 0.0]), key(s * 0.375, [20.0, 0.0, 0.0]), key(s * 0.625, [-20.0, 0.0, 0.0]), key(s * 0.875, [20.0, 0.0, 0.0]), key(s, [0.0; 3])]),
        ],
    );
    // Fly: arms spread, legs trailing, a slow hover.
    let fly = clip(
        "Fly",
        2.0,
        true,
        vec![
            ("Arm L", vec![key(0.0, [0.0, 0.0, -28.0]), key(1.0, [0.0, 0.0, -34.0]), key(2.0, [0.0, 0.0, -28.0])]),
            ("Arm R", vec![key(0.0, [0.0, 0.0, 28.0]), key(1.0, [0.0, 0.0, 34.0]), key(2.0, [0.0, 0.0, 28.0])]),
            ("Leg L", vec![key(0.0, [-12.0, 0.0, 0.0])]),
            ("Leg R", vec![key(0.0, [-16.0, 0.0, 0.0])]),
        ],
    );
    // Sit: legs forward, hands on the knees.
    let sit = clip(
        "Sit",
        1.0,
        true,
        vec![
            ("Leg L", vec![key(0.0, [90.0, 0.0, 4.0])]),
            ("Leg R", vec![key(0.0, [90.0, 0.0, -4.0])]),
            ("Arm L", vec![key(0.0, [35.0, 0.0, -4.0])]),
            ("Arm R", vec![key(0.0, [35.0, 0.0, 4.0])]),
        ],
    );
    // Hold: the right arm forward while an item is in hand.
    let hold = clip("Hold", 1.0, true, vec![("Arm R", vec![key(0.0, [20.0, 0.0, 3.0])])]);
    // Block: guard up.
    let block = clip("Block", 1.0, true, vec![("Arm R", vec![key(0.0, [89.0, 0.0, 3.0])]), ("Arm L", vec![key(0.0, [30.0, 0.0, 10.0])])]);
    // Attack: one swing (also loops while mining).
    let attack = clip(
        "Attack",
        0.35,
        false,
        vec![("Arm R", vec![key(0.0, [20.0, 0.0, 3.0]), key(0.175, [129.0, 0.0, 20.0]), key(0.35, [20.0, 0.0, 3.0])])],
    );
    // Use: a short push forward.
    let use_clip = clip("Use", 0.2, false, vec![("Arm R", vec![key(0.0, [66.0, 0.0, 3.0]), key(0.2, [20.0, 0.0, 3.0])])]);
    vec![idle, walk, swim, fly, sit, hold, block, attack, use_clip]
}

/// Default clips of the first-person hand ("Player Hand" rig).
pub fn default_player_hand_clips() -> Vec<ActionClip> {
    // Walk: the arm sways with the steps (follows the distance walked).
    let w = 1.05;
    let e = w / 8.0;
    let sway: Vec<Keyframe> = (0..=8)
        .map(|k| {
            let phase = k as f32 * std::f32::consts::FRAC_PI_4;
            moved(k as f32 * e, [phase.sin() * 0.018, -(phase * 2.0).sin().abs() * 0.014, 0.0], [0.0; 3])
        })
        .collect();
    let walk = clip("Walk", w, true, vec![("Arm", sway)]);
    let i = 3.9;
    let idle = clip(
        "Idle",
        i,
        true,
        vec![("Arm", vec![moved(0.0, [0.0; 3], [0.0; 3]), moved(i * 0.5, [0.0, -0.005, 0.0], [1.0, 0.0, 0.0]), moved(i, [0.0; 3], [0.0; 3])])],
    );
    let s = 3.14;
    let swim = clip(
        "Swim",
        s,
        true,
        vec![(
            "Arm",
            vec![
                moved(0.0, [0.0; 3], [0.0; 3]),
                moved(s * 0.25, [0.0, 0.012, 0.0], [0.0; 3]),
                moved(s * 0.5, [0.0; 3], [0.0; 3]),
                moved(s * 0.75, [0.0, -0.012, 0.0], [0.0; 3]),
                moved(s, [0.0; 3], [0.0; 3]),
            ],
        )],
    );
    let block = clip("Block", 1.0, true, vec![("Arm", vec![moved(0.0, [-0.16, 0.08, 0.04], [0.0, 34.0, 0.0])])]);
    let attack = clip(
        "Attack",
        0.35,
        false,
        vec![("Arm", vec![moved(0.0, [0.0; 3], [0.0; 3]), moved(0.175, [-0.08, 0.04, -0.14], [-63.0, 0.0, 0.0]), moved(0.35, [0.0; 3], [0.0; 3])])],
    );
    let use_clip = clip("Use", 0.2, false, vec![("Arm", vec![moved(0.0, [0.0, 0.0, -0.08], [-20.0, 0.0, 0.0]), moved(0.2, [0.0; 3], [0.0; 3])])]);
    vec![idle, walk, swim, block, attack, use_clip]
}

/// Add the default clips a player rig does not have yet (by name). Returns
/// how many were added.
pub fn install_player_clips(rig: &mut MobRig) -> usize {
    let defaults = match rig.role {
        RigRole::Player => default_player_clips(),
        RigRole::PlayerHand => default_player_hand_clips(),
        _ => return 0,
    };
    let mut added = 0;
    for clip in defaults {
        if rig.clip(&clip.name).is_none() {
            rig.clips.push(clip);
            added += 1;
        }
    }
    added
}

#[cfg(test)]
mod player_clip_tests {
    use super::*;

    /// World position of the wrist in the shipped Player rig at a clip time.
    fn hand_at(clip: &str, time: f32) -> (bevy::math::Vec3, bevy::math::Vec3) {
        use crate::gameplay::mobs::pose::{sample_track, world_matrices, RigPose};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("project/entities/mobs/Player.ron");
        let rig: MobRig = crate::core::project_io::load_ron(&path).unwrap();
        let clip = rig.clip(clip).unwrap();
        let mut pose = RigPose::default();
        for track in &clip.tracks {
            pose.bones.insert(track.bone.clone(), sample_track(track, time * clip.duration_or(1.0)));
        }
        let matrices = world_matrices(&rig, &pose);
        (matrices["Hand R"].w_axis.truncate(), matrices["Head"].w_axis.truncate())
    }

    #[test]
    fn the_arms_of_the_player_move_in_front_of_the_body() {
        let (hand, head) = hand_at("Attack", 0.26);
        assert!(hand.y > head.y, "the blade goes up: {hand}");
        let (hand, _) = hand_at("Attack", 0.44);
        assert!(hand.z < -0.4, "the blow lands forward: {hand}");
        let (hand, _) = hand_at("Block", 0.0);
        assert!(hand.z < -0.3, "the guard is forward: {hand}");
        let (hand, _) = hand_at("Hold", 0.0);
        assert!(hand.z < -0.2, "the item is held forward: {hand}");
    }

    #[test]
    fn player_rigs_get_their_default_clips_once() {
        let rig = default_player_rig();
        for name in ["Idle", "Walk", "Swim", "Fly", "Sit", "Hold", "Block", "Attack", "Use"] {
            assert!(rig.clip(name).is_some_and(|clip| clip.key_count() > 0), "{name}");
        }
        let mut hand = default_player_hand_rig();
        assert!(hand.clip("Attack").is_some());
        assert_eq!(install_player_clips(&mut hand), 0, "nothing added twice");
        // Every track animates a bone of its rig.
        for rig in [&rig, &hand] {
            for clip in &rig.clips {
                for track in &clip.tracks {
                    assert!(rig.boxes.iter().any(|bone| bone.name == track.bone), "{} / {}", clip.name, track.bone);
                }
            }
        }
    }
}
