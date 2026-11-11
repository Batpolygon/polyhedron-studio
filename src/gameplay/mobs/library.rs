//! Mob library: authoring state, validation and `.ron` persistence.
//!
//! Rigs live in `project/entities/mobs/<Name>.ron`. The library is the only
//! owner of the editable data; the viewport samples it through
//! [`crate::gameplay::mobs::pose`] and never mutates it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::gameplay::mobs::model::{ActionClip, MobRig};

/// Severity of a mob authoring finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MobSeverity {
    Warning,
    Error,
}


/// One validation finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MobIssue {
    pub severity: MobSeverity,
    pub location: String,
    pub message: String,
}

impl MobIssue {
    fn error(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: MobSeverity::Error,
            location: location.into(),
            message: message.into(),
        }
    }

    fn warning(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: MobSeverity::Warning,
            location: location.into(),
            message: message.into(),
        }
    }
}

/// Playback state of the animator preview.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MobPreview {
    pub time: f32,
    pub playing: bool,
    pub speed: f32,
    pub loop_clip: bool,
}

impl Default for MobPreview {
    fn default() -> Self {
        Self {
            time: 0.0,
            playing: true,
            speed: 1.0,
            loop_clip: true,
        }
    }
}

impl MobPreview {
    pub fn advance(&mut self, delta: f32, duration: f32, looping: bool) {
        if !self.playing {
            return;
        }
        let speed = if self.speed.is_finite() {
            self.speed.clamp(-4.0, 4.0)
        } else {
            1.0
        };
        self.time += delta * speed;

        let duration = duration.max(0.01);
        if looping && self.loop_clip {
            self.time = self.time.rem_euclid(duration);
        } else {
            self.time = self.time.clamp(0.0, duration);
            if self.time >= duration {
                self.playing = false;
            }
        }
    }

    pub fn scrub(&mut self, time: f32, duration: f32) {
        self.time = time.clamp(0.0, duration.max(0.01));
    }
}

/// Editable mob library.
#[derive(Resource, Debug)]
pub struct MobLibrary {
    pub rigs: Vec<MobRig>,
    pub selection: usize,
    pub bone_selection: Option<String>,
    /// Other selected boxes (Shift / Ctrl + click): the gizmo, Delete and
    /// Duplicate apply to all of them.
    pub bone_extra: Vec<String>,
    pub socket_selection: Option<String>,
    pub clip_selection: String,
    pub preview: MobPreview,
    /// World anchor of the viewport preview, so the rig floats above the demo
    /// terrain instead of being buried in it.
    pub preview_origin: [f32; 3],
    /// Box owning the selected keyframe.
    pub key_bone: Option<String>,
    /// Time of the selected keyframe.
    pub key_time: Option<f32>,
    pub revision: u64,
    pub last_saved_revision: u64,
    pub loaded: bool,
}

impl Default for MobLibrary {
    fn default() -> Self {
        Self {
            rigs: Vec::new(),
            selection: 0,
            bone_selection: None,
            bone_extra: Vec::new(),
            socket_selection: None,
            clip_selection: String::new(),
            preview: MobPreview::default(),
            preview_origin: [0.0, 6.0, 0.0],
            key_bone: None,
            key_time: None,
            revision: 0,
            last_saved_revision: 0,
            loaded: false,
        }
    }
}

impl MobLibrary {
    /// Every selected box name (the primary one first).
    pub fn selected_bones(&self) -> Vec<String> {
        let mut list: Vec<String> = self.bone_selection.iter().cloned().collect();
        for name in &self.bone_extra {
            if !list.contains(name) {
                list.push(name.clone());
            }
        }
        list
    }

    /// Click on a box: replace the selection, or toggle it with `add`.
    pub fn select_bone(&mut self, name: Option<String>, add: bool) {
        if !add {
            self.bone_selection = name;
            self.bone_extra.clear();
            return;
        }
        let Some(name) = name else {
            return;
        };
        if self.bone_selection.as_deref() == Some(name.as_str()) {
            self.bone_selection = if self.bone_extra.is_empty() { None } else { Some(self.bone_extra.remove(0)) };
        } else if let Some(position) = self.bone_extra.iter().position(|n| *n == name) {
            self.bone_extra.remove(position);
        } else if self.bone_selection.is_none() {
            self.bone_selection = Some(name);
        } else {
            self.bone_extra.push(name);
        }
    }

    /// Rigs that are creatures (not the player body or hand).
    pub fn mob_rigs(&self) -> impl Iterator<Item = &MobRig> {
        self.rigs.iter().filter(|rig| rig.role == crate::gameplay::mobs::model::RigRole::Mob)
    }

    /// The player body rig (the default one when the project has none).
    pub fn player_rig(&self) -> std::borrow::Cow<'_, MobRig> {
        use crate::gameplay::mobs::model::{default_player_rig, RigRole};
        match self.rigs.iter().find(|rig| rig.role == RigRole::Player) {
            Some(rig) => std::borrow::Cow::Borrowed(rig),
            None => std::borrow::Cow::Owned(default_player_rig()),
        }
    }

    /// The first-person hand rig (the default one when the project has none).
    pub fn hand_rig(&self) -> std::borrow::Cow<'_, MobRig> {
        use crate::gameplay::mobs::model::{default_player_hand_rig, RigRole};
        match self.rigs.iter().find(|rig| rig.role == RigRole::PlayerHand) {
            Some(rig) => std::borrow::Cow::Borrowed(rig),
            None => std::borrow::Cow::Owned(default_player_hand_rig()),
        }
    }

    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.last_saved_revision
    }

    pub fn current(&self) -> Option<&MobRig> {
        self.rigs.get(self.selection)
    }

    pub fn current_mut(&mut self) -> Option<&mut MobRig> {
        self.rigs.get_mut(self.selection)
    }

    pub fn current_clip(&self) -> Option<&ActionClip> {
        let rig = self.current()?;
        if let Some(clip) = rig.clip(&self.clip_selection) {
            return Some(clip);
        }
        rig.resolved_default_clip()
    }

    pub fn current_clip_mut(&mut self) -> Option<&mut ActionClip> {
        let name = self.clip_selection.clone();
        let rig = self.rigs.get_mut(self.selection)?;
        if rig.clip(&name).is_some() {
            return rig.clip_mut(&name);
        }
        let fallback = rig.resolved_default_clip().map(|clip| clip.name.clone())?;
        rig.clip_mut(&fallback)
    }

    pub fn clip_name(&self) -> String {
        self.current_clip()
            .map(|clip| clip.name.clone())
            .unwrap_or_default()
    }

    pub fn clip_duration(&self) -> f32 {
        self.current_clip()
            .map(|clip| clip.duration_or(1.0))
            .unwrap_or(1.0)
    }

    pub fn clip_looping(&self) -> bool {
        self.current_clip()
            .map(|clip| clip.looping)
            .unwrap_or(true)
    }

    /// Keep the selections valid after structural edits.
    pub fn clamp_selection(&mut self) {
        if self.rigs.is_empty() {
            self.selection = 0;
            self.bone_selection = None;
            self.socket_selection = None;
            self.clip_selection.clear();
            return;
        }

        self.selection = self.selection.min(self.rigs.len() - 1);
        let rig = &self.rigs[self.selection];

        if let Some(bone) = &self.bone_selection {
            if rig.bone(bone).is_none() {
                self.bone_selection = rig.boxes.first().map(|bone| bone.name.clone());
            }
        } else {
            self.bone_selection = rig.boxes.first().map(|bone| bone.name.clone());
        }

        if let Some(socket) = &self.socket_selection {
            if !rig.sockets.iter().any(|candidate| &candidate.name == socket) {
                self.socket_selection = None;
            }
        }

        if rig.clip(&self.clip_selection).is_none() {
            self.clip_selection = rig
                .resolved_default_clip()
                .map(|clip| clip.name.clone())
                .unwrap_or_default();
        }

        if let Some(bone) = &self.key_bone {
            if rig.bone(bone).is_none() {
                self.key_bone = None;
                self.key_time = None;
            }
        }
    }

    /// Add a clip to the current rig and select it.
    pub fn add_clip(&mut self) -> Option<String> {
        let rig = self.current_mut()?;
        let index = rig.add_clip();
        let name = rig.clips[index].name.clone();
        if rig.default_clip.is_empty() {
            rig.default_clip = name.clone();
        }
        self.clip_selection = name.clone();
        self.preview.time = 0.0;
        self.key_bone = None;
        self.key_time = None;
        self.touch();
        Some(name)
    }

    pub fn add_rig(&mut self) -> usize {
        let names: Vec<String> = self.rigs.iter().map(|rig| rig.name.clone()).collect();
        let name = unique_name(&names, "New Mob");
        let mut rig = MobRig::new(name);

        let root = rig.add_box(None);
        rig.boxes[root].name = "Root".to_string();
        rig.boxes[root].size = [0.6, 0.6, 0.6];

        let clip = rig.add_clip();
        rig.clips[clip].name = "Idle".to_string();
        rig.default_clip = "Idle".to_string();

        self.rigs.push(rig);
        self.selection = self.rigs.len() - 1;
        self.bone_selection = Some("Root".to_string());
        self.socket_selection = None;
        self.clip_selection = "Idle".to_string();
        self.preview.time = 0.0;
        self.touch();
        self.rigs.len() - 1
    }

    pub fn remove_rig(&mut self, index: usize) -> Option<String> {
        if index >= self.rigs.len() {
            return None;
        }
        // The player body and hand cannot be deleted (only edited).
        if self.rigs[index].role != crate::gameplay::mobs::model::RigRole::Mob {
            return None;
        }
        let removed = self.rigs.remove(index).name;
        self.clamp_selection();
        self.touch();
        Some(removed)
    }

    pub fn duplicate_rig(&mut self, index: usize) -> Option<String> {
        let source = self.rigs.get(index)?.clone();
        let names: Vec<String> = self.rigs.iter().map(|rig| rig.name.clone()).collect();
        let mut copy = source;
        copy.name = unique_name(&names, &format!("{} Copy", copy.name));
        // A copy of the player model is an ordinary mob.
        copy.role = crate::gameplay::mobs::model::RigRole::Mob;
        self.rigs.push(copy);
        self.selection = self.rigs.len() - 1;
        self.clamp_selection();
        self.touch();
        Some(self.rigs[self.selection].name.clone())
    }

    /// Directory holding mob rigs.
    pub fn rigs_path(project: &ProjectLayout) -> PathBuf {
        project.module_path("entities").join("mobs")
    }

    pub fn load_from_project(&mut self, project: &ProjectLayout) -> Result<MobLoadReport, String> {
        let directory = Self::rigs_path(project);
        fs::create_dir_all(&directory).map_err(|error| {
            format!("cannot create {}: {error}", directory.display())
        })?;

        let mut warnings = Vec::new();
        let mut rigs = Vec::new();

        for path in ron_files(&directory)? {
            match load_ron::<MobRig>(&path) {
                Ok(rig) => rigs.push(rig),
                Err(error) => warnings.push(error),
            }
        }

        let mut created_defaults = false;
        // Example rigs of earlier versions are removed (their file too).
        for retired in crate::gameplay::mobs::bestiary::RETIRED_RIGS {
            if rigs.iter().any(|rig| rig.name == retired) {
                rigs.retain(|rig| rig.name != retired);
                let _ = fs::remove_file(directory.join(format!("{}.ron", sanitize_file_stem(retired))));
            }
        }
        // Every entity of the game missing from the folder is added.
        for rig in crate::gameplay::mobs::bestiary::spec_rigs() {
            if !rigs.iter().any(|existing| existing.name == rig.name) {
                rigs.push(rig);
                created_defaults = true;
            }
        }
        // The player body and hand are rigs too (edited like the mobs).
        use crate::gameplay::mobs::model::{default_player_hand_rig, default_player_rig, RigRole};
        if !rigs.iter().any(|rig| rig.role == RigRole::Player) {
            rigs.push(default_player_rig());
            created_defaults = true;
        }
        if !rigs.iter().any(|rig| rig.role == RigRole::PlayerHand) {
            rigs.push(default_player_hand_rig());
            created_defaults = true;
        }
        // The player animations are clips of these rigs: install the default
        // ones they miss (saved, then editable in the Mobs workspace).
        for rig in &mut rigs {
            if crate::gameplay::mobs::model::install_player_clips(rig) > 0 {
                created_defaults = true;
            }
        }

        rigs.sort_by(|left, right| left.name.cmp(&right.name));

        self.rigs = rigs;
        self.selection = 0;
        self.loaded = true;
        self.clamp_selection();
        self.preview = MobPreview::default();
        self.revision = self.revision.wrapping_add(1);
        self.last_saved_revision = self.revision;

        if created_defaults {
            self.save_all(project)?;
        }

        Ok(MobLoadReport {
            rigs: self.rigs.len(),
            created_defaults,
            warnings,
        })
    }

    pub fn save_all(&mut self, project: &ProjectLayout) -> Result<MobSaveReport, String> {
        let directory = Self::rigs_path(project);
        fs::create_dir_all(&directory).map_err(|error| {
            format!("cannot create {}: {error}", directory.display())
        })?;

        let mut stems = BTreeSet::new();
        for rig in &self.rigs {
            let stem = sanitize_file_stem(&rig.name);
            if !stems.insert(stem.to_ascii_lowercase()) {
                return Err(format!(
                    "Two mobs map to the same file name '{stem}.ron'."
                ));
            }
            save_ron(&directory.join(format!("{stem}.ron")), rig)?;
        }

        let removed = remove_stale(&directory, &stems)?;
        self.last_saved_revision = self.revision;

        Ok(MobSaveReport {
            rigs: self.rigs.len(),
            removed,
        })
    }

    /// Structural validation of every rig.
    pub fn validate(&self) -> Vec<MobIssue> {
        let mut issues = Vec::new();
        let mut names = BTreeSet::new();
        let mut files = BTreeSet::new();

        for rig in &self.rigs {
            let base = format!("Mob '{}'", rig.name);

            if rig.name.trim().is_empty() {
                issues.push(MobIssue::error("Mob", "a mob has an empty name."));
            }
            if !names.insert(rig.name.to_ascii_lowercase()) {
                issues.push(MobIssue::error(base.clone(), "duplicate mob name."));
            }
            if !files.insert(sanitize_file_stem(&rig.name).to_ascii_lowercase()) {
                issues.push(MobIssue::error(
                    base.clone(),
                    "two mobs map to the same file name.",
                ));
            }

            self.validate_rig(rig, &base, &mut issues);
        }

        issues
    }

    fn validate_rig(&self, rig: &MobRig, base: &str, issues: &mut Vec<MobIssue>) {
        if rig.boxes.is_empty() {
            issues.push(MobIssue::error(base, "the mob has no box."));
        }

        let mut bone_names = BTreeSet::new();
        for bone in &rig.boxes {
            let location = format!("{base} / box '{}'", bone.name);

            if bone.name.trim().is_empty() {
                issues.push(MobIssue::error(base, "a box has an empty name."));
            }
            if !bone_names.insert(bone.name.clone()) {
                issues.push(MobIssue::error(
                    location.clone(),
                    "duplicate box name.",
                ));
            }

            if let Some(parent) = &bone.parent {
                if parent == &bone.name {
                    issues.push(MobIssue::error(
                        location.clone(),
                        "a box cannot parent itself.",
                    ));
                } else if rig.bone(parent).is_none() {
                    issues.push(MobIssue::error(
                        location.clone(),
                        format!("unknown parent box '{parent}'."),
                    ));
                } else if rig.is_ancestor(&bone.name, parent) {
                    issues.push(MobIssue::error(
                        location.clone(),
                        format!("'{parent}' is a descendant; the hierarchy loops."),
                    ));
                }
            }

            for (axis, value) in ["x", "y", "z"].iter().zip(bone.size) {
                if !value.is_finite() || value.abs() < 1e-3 {
                    issues.push(MobIssue::error(
                        location.clone(),
                        format!("size {axis} must be a non-zero finite number."),
                    ));
                }
            }
            for value in bone.pivot.iter().chain(bone.offset.iter()).chain(bone.rotation.iter()) {
                if !value.is_finite() {
                    issues.push(MobIssue::error(
                        location.clone(),
                        "pivot, offset and rotation must be finite.",
                    ));
                    break;
                }
            }
        }

        let mut socket_names = BTreeSet::new();
        for socket in &rig.sockets {
            let location = format!("{base} / socket '{}'", socket.name);
            if !socket_names.insert(socket.name.clone()) {
                issues.push(MobIssue::error(location.clone(), "duplicate socket name."));
            }
            if rig.bone(&socket.bone).is_none() {
                issues.push(MobIssue::error(
                    location,
                    format!("socket is attached to unknown box '{}'.", socket.bone),
                ));
            }
        }

        if rig.clips.is_empty() {
            issues.push(MobIssue::warning(base, "the mob has no action clip."));
        }

        let mut clip_names = BTreeSet::new();
        for clip in &rig.clips {
            let location = format!("{base} / clip '{}'", clip.name);
            if clip.name.trim().is_empty() {
                issues.push(MobIssue::error(base, "a clip has an empty name."));
            }
            if !clip_names.insert(clip.name.clone()) {
                issues.push(MobIssue::error(location.clone(), "duplicate clip name."));
            }
            if !clip.duration.is_finite() || clip.duration <= 0.0 {
                issues.push(MobIssue::error(
                    location.clone(),
                    "clip duration must be a positive finite number.",
                ));
            }

            for track in &clip.tracks {
                if rig.bone(&track.bone).is_none() {
                    issues.push(MobIssue::error(
                        location.clone(),
                        format!("track targets unknown box '{}'.", track.bone),
                    ));
                }
                if track.keys.is_empty() {
                    issues.push(MobIssue::warning(
                        location.clone(),
                        format!("track '{}' has no keyframe.", track.bone),
                    ));
                }

                let mut previous = f32::NEG_INFINITY;
                for key in &track.keys {
                    if !key.time.is_finite() || key.time < 0.0 {
                        issues.push(MobIssue::error(
                            location.clone(),
                            "keyframe time must be a positive finite number.",
                        ));
                    }
                    if key.time <= previous + 1e-4 {
                        issues.push(MobIssue::error(
                            location.clone(),
                            "keyframes must be strictly sorted by time.",
                        ));
                    }
                    previous = key.time;

                    if key.time > clip.duration + 1e-3 {
                        issues.push(MobIssue::warning(
                            location.clone(),
                            format!(
                                "key at {:.2}s is beyond the clip duration ({:.2}s).",
                                key.time, clip.duration
                            ),
                        ));
                    }

                    for value in key
                        .translation
                        .iter()
                        .chain(key.rotation.iter())
                        .chain(key.scale.iter())
                    {
                        if !value.is_finite() {
                            issues.push(MobIssue::error(
                                location.clone(),
                                "keyframe values must be finite.",
                            ));
                            break;
                        }
                    }
                }
            }
        }

        if !rig.default_clip.is_empty() && rig.clip(&rig.default_clip).is_none() {
            issues.push(MobIssue::error(
                base,
                format!("default clip '{}' does not exist.", rig.default_clip),
            ));
        }
    }
}

/// Summary of a library load.
#[derive(Clone, Debug)]
pub struct MobLoadReport {
    pub rigs: usize,
    pub created_defaults: bool,
    pub warnings: Vec<String>,
}

/// Summary of a library save.
#[derive(Clone, Debug)]
pub struct MobSaveReport {
    pub rigs: usize,
    pub removed: usize,
}

/// Turn a display name into a safe file stem.
pub fn sanitize_file_stem(name: &str) -> String {
    let filtered: String = name
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();

    if filtered.is_empty() {
        "untitled".to_string()
    } else {
        filtered
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

fn ron_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
        let path = entry.path();
        let is_ron = path
            .extension()
            .map(|extension| extension.eq_ignore_ascii_case("ron"))
            .unwrap_or(false);
        if path.is_file() && is_ron {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn remove_stale(directory: &Path, keep: &BTreeSet<String>) -> Result<usize, String> {
    let mut removed = 0;
    for path in ron_files(directory)? {
        let Some(stem) = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
        else {
            continue;
        };
        if !keep.contains(&stem) {
            fs::remove_file(&path)
                .map_err(|error| format!("cannot remove {}: {error}", path.display()))?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::project_io::{ProjectManifest, PROJECT_MODULES};
    use crate::gameplay::mobs::model::{Interpolation, Keyframe, MobBox};

    fn temp_project(label: &str) -> ProjectLayout {
        let root = std::env::temp_dir().join(format!(
            "polyhedron_mobs_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        let _ = fs::remove_dir_all(&root);
        ProjectLayout {
            root,
            manifest: ProjectManifest::default(),
            modules: PROJECT_MODULES.to_vec(),
        }
    }

    /// The rigs shipped in `project/` are read as they are, nothing is written.
    #[test]
    fn shipped_rigs_are_valid() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("project/entities/mobs");
        let mut library = MobLibrary::default();
        for path in ron_files(&directory).expect("mob folder") {
            let rig = load_ron::<crate::gameplay::mobs::model::MobRig>(&path)
                .unwrap_or_else(|error| panic!("{error}"));
            for clip in ["Idle", "Walk", "Attack"] {
                assert!(rig.clip(clip).is_some(), "{} has no {clip} clip", rig.name);
            }
            library.rigs.push(rig);
        }
        assert!(library.rigs.len() >= 23);
        let errors: Vec<String> = library
            .validate()
            .into_iter()
            .filter(|issue| matches!(issue.severity, MobSeverity::Error))
            .map(|issue| format!("{}: {}", issue.location, issue.message))
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
    }

    #[test]
    fn file_stems_are_safe() {
        assert_eq!(sanitize_file_stem("Sentinel Drone"), "Sentinel_Drone");
        assert_eq!(sanitize_file_stem("a/b"), "a_b");
        assert_eq!(sanitize_file_stem("   "), "untitled");
    }

    #[test]
    fn starter_rig_is_valid_and_round_trips() {
        let rig = crate::gameplay::mobs::model::starter_rig();
        let text = ron::ser::to_string_pretty(&rig, ron::ser::PrettyConfig::default())
            .expect("rig serializes");
        let parsed: MobRig = ron::from_str(&text).expect("rig parses");
        assert_eq!(rig, parsed);
    }

    #[test]
    fn cycles_are_reported() {
        let mut library = MobLibrary::default();
        let mut rig = MobRig::new("Cyclic");
        rig.boxes.push(MobBox::new("A", Some("B".to_string())));
        rig.boxes.push(MobBox::new("B", Some("A".to_string())));
        library.rigs.push(rig);

        let issues = library.validate();
        assert!(issues
            .iter()
            .any(|issue| issue.message.contains("loops") || issue.message.contains("descendant")));
    }

    #[test]
    fn reparenting_rejects_cycles() {
        let mut rig = MobRig::new("Rig");
        rig.boxes.push(MobBox::new("Root", None));
        rig.boxes.push(MobBox::new("Child", Some("Root".to_string())));

        assert!(rig.reparent("Root", Some("Child".to_string())).is_err());
        assert!(rig.reparent("Child", None).is_ok());
        assert!(rig.bone("Child").unwrap().parent.is_none());
    }

    #[test]
    fn unsorted_keyframes_are_reported() {
        let mut library = MobLibrary::default();
        let mut rig = MobRig::new("Broken");
        rig.boxes.push(MobBox::new("Root", None));

        let mut clip = ActionClip::new("Bad");
        clip.duration = 1.0;
        let track = clip.ensure_track("Root");
        track.keys.push(Keyframe::new(0.5));
        track.keys.push(
            Keyframe::new(0.2).with_interpolation(Interpolation::Linear),
        );
        rig.clips.push(clip);
        library.rigs.push(rig);

        let issues = library.validate();
        assert!(issues
            .iter()
            .any(|issue| issue.message.contains("strictly sorted")));
    }

    #[test]
    fn project_library_writes_and_reloads() {
        let project = temp_project("io");
        let mut library = MobLibrary::default();
        let report = library.load_from_project(&project).expect("library loads");
        assert!(report.created_defaults);
        // The 21 entities of the game plus the player body and hand rigs.
        let total = crate::gameplay::mobs::bestiary::spec_rigs().len() + 2;
        assert_eq!(report.rigs, total);
        assert!(MobLibrary::rigs_path(&project).join("Gim-Gim.ron").is_file());
        assert!(MobLibrary::rigs_path(&project).join("Player.ron").is_file());
        assert_eq!(library.mob_rigs().count(), total - 2);

        let mut reloaded = MobLibrary::default();
        let report = reloaded.load_from_project(&project).expect("library reloads");
        assert_eq!(report.rigs, total);
        assert!(!report.created_defaults);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(reloaded.rigs[0].name, library.rigs[0].name);

        let _ = fs::remove_dir_all(&project.root);
    }

    #[test]
    fn stale_files_are_removed_on_save() {
        let project = temp_project("stale");
        let mut library = MobLibrary::default();
        library.load_from_project(&project).expect("library loads");

        let directory = MobLibrary::rigs_path(&project);
        fs::write(directory.join("Ghost.ron"), "()").expect("ghost written");

        let report = library.save_all(&project).expect("library saves");
        assert_eq!(report.removed, 1);
        assert!(!directory.join("Ghost.ron").exists());

        let _ = fs::remove_dir_all(&project.root);
    }

    #[test]
    fn preview_wraps_and_clamps() {
        let mut preview = MobPreview::default();
        preview.advance(1.2, 1.0, true);
        assert!(preview.time < 0.3);
        assert!(preview.playing);

        preview.advance(5.0, 1.0, false);
        assert!((preview.time - 1.0).abs() < 1e-4);
        assert!(!preview.playing);
    }
}
