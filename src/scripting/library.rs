//! Authoring library: programs, actions, validation and project persistence.
//!
//! The library owns the editable data. The VM snapshots it on start, so the
//! editor can keep mutating names and blocks without corrupting a running
//! execution. `project/scripts/` and `project/actions/` are the only sources of
//! truth on disk; saving rewrites every file and removes stale entries.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::scripting::model::{
    Operand,
    ActionDef, CompareOp, ScriptBlock, ScriptProgram, ValidationIssue,
};

/// Which editor entry is currently open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptSelection {
    Program(usize),
    Action(usize),
}

impl Default for ScriptSelection {
    fn default() -> Self {
        Self::Program(0)
    }
}

/// Summary returned by [`ScriptLibrary::save_all`].
#[derive(Clone, Debug)]
pub struct SaveReport {
    pub programs: usize,
    pub actions: usize,
    pub removed: usize,
}

/// Summary returned by [`ScriptLibrary::load_from_project`].
#[derive(Clone, Debug)]
pub struct LoadReport {
    pub programs: usize,
    pub actions: usize,
    pub created_defaults: bool,
    pub warnings: Vec<String>,
}

/// Editable Blockly library.
#[derive(Resource, Debug)]
pub struct ScriptLibrary {
    pub programs: Vec<ScriptProgram>,
    pub actions: Vec<ActionDef>,
    pub selection: ScriptSelection,
    pub revision: u64,
    pub last_saved_revision: u64,
    pub loaded: bool,
}

impl Default for ScriptLibrary {
    fn default() -> Self {
        Self {
            programs: Vec::new(),
            actions: Vec::new(),
            selection: ScriptSelection::Program(0),
            revision: 0,
            last_saved_revision: 0,
            loaded: false,
        }
    }
}

impl ScriptLibrary {
    /// Mark the authoring data as changed. The running VM restarts on the next
    /// tick when it sees a different revision.
    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.last_saved_revision
    }

    pub fn action_names(&self) -> Vec<String> {
        self.actions.iter().map(|action| action.name.clone()).collect()
    }

    pub fn add_program(&mut self) -> usize {
        let names: Vec<String> = self
            .programs
            .iter()
            .map(|program| program.name.clone())
            .collect();
        let name = unique_name(&names, "Program");
        self.programs.push(ScriptProgram::new(name));
        self.selection = ScriptSelection::Program(self.programs.len() - 1);
        self.touch();
        self.programs.len() - 1
    }

    /// Adds a copy of a template under a free name.
    pub fn add_template(&mut self, mut template: ScriptProgram) -> usize {
        let names: Vec<String> = self.programs.iter().map(|program| program.name.clone()).collect();
        template.name = unique_name(&names, &template.name);
        self.programs.push(template);
        self.selection = ScriptSelection::Program(self.programs.len() - 1);
        self.touch();
        self.programs.len() - 1
    }

    pub fn add_action(&mut self) -> usize {
        let names: Vec<String> = self
            .actions
            .iter()
            .map(|action| action.name.clone())
            .collect();
        let name = unique_name(&names, "Action");
        self.actions.push(ActionDef::new(name));
        self.selection = ScriptSelection::Action(self.actions.len() - 1);
        self.touch();
        self.actions.len() - 1
    }

    pub fn duplicate_program(&mut self, index: usize) -> Option<String> {
        let source = self.programs.get(index)?.clone();
        let names: Vec<String> = self
            .programs
            .iter()
            .map(|program| program.name.clone())
            .collect();
        let name = unique_name(&names, &format!("{} Copy", source.name));
        let mut copy = source;
        copy.name = name.clone();
        self.programs.push(copy);
        self.selection = ScriptSelection::Program(self.programs.len() - 1);
        self.touch();
        Some(name)
    }

    pub fn duplicate_action(&mut self, index: usize) -> Option<String> {
        let source = self.actions.get(index)?.clone();
        let names: Vec<String> = self
            .actions
            .iter()
            .map(|action| action.name.clone())
            .collect();
        let name = unique_name(&names, &format!("{} Copy", source.name));
        let mut copy = source;
        copy.name = name.clone();
        self.actions.push(copy);
        self.selection = ScriptSelection::Action(self.actions.len() - 1);
        self.touch();
        Some(name)
    }

    pub fn remove_program(&mut self, index: usize) -> Option<String> {
        if index >= self.programs.len() {
            return None;
        }
        let name = self.programs.remove(index).name;
        self.clamp_selection();
        self.touch();
        Some(name)
    }

    pub fn remove_action(&mut self, index: usize) -> Option<String> {
        if index >= self.actions.len() {
            return None;
        }
        let name = self.actions.remove(index).name;
        self.clamp_selection();
        self.touch();
        Some(name)
    }

    /// Read every `.ron` file from the project and create the starter library
    /// when the folders are empty.
    pub fn load_from_project(&mut self, project: &ProjectLayout) -> Result<LoadReport, String> {
        let scripts_dir = project.module_path("scripts");
        let actions_dir = project.module_path("actions");
        create_dir(&scripts_dir)?;
        create_dir(&actions_dir)?;

        let mut warnings = Vec::new();
        let mut programs = Vec::new();
        for path in ron_files(&scripts_dir)? {
            match load_ron::<ScriptProgram>(&path) {
                Ok(program) => programs.push(program),
                Err(error) => warnings.push(error),
            }
        }

        let mut actions = Vec::new();
        for path in ron_files(&actions_dir)? {
            match load_ron::<ActionDef>(&path) {
                Ok(action) => actions.push(action),
                Err(error) => warnings.push(error),
            }
        }

        let mut created_defaults = false;
        if programs.is_empty() {
            programs = default_programs();
            created_defaults = true;
        }
        if actions.is_empty() {
            actions = default_actions();
            created_defaults = true;
        }

        // Starter programs added by the gameplay update (AI, world rules,
        // block logic). A marker file records the merge so deleting one of
        // them later is respected.
        let marker = scripts_dir.join(".builtin_v2");
        if !marker.exists() {
            for program in [mob_brain_program(), day_night_program(), signal_lamp_program()] {
                if !programs.iter().any(|existing| existing.name == program.name) {
                    programs.push(program);
                    created_defaults = true;
                }
            }
            if let Err(error) = std::fs::write(&marker, "starter programs merged\n") {
                warnings.push(format!("cannot write {}: {error}", marker.display()));
            }
        }

        programs.sort_by(|left, right| left.name.cmp(&right.name));
        actions.sort_by(|left, right| left.name.cmp(&right.name));

        self.programs = programs;
        self.actions = actions;
        self.selection = if self.programs.is_empty() {
            ScriptSelection::Action(0)
        } else {
            ScriptSelection::Program(0)
        };
        self.revision = self.revision.wrapping_add(1);
        self.last_saved_revision = self.revision;
        self.loaded = true;

        if created_defaults {
            self.save_all(project)?;
        }

        Ok(LoadReport {
            programs: self.programs.len(),
            actions: self.actions.len(),
            created_defaults,
            warnings,
        })
    }

    /// Write the whole library and remove stale `.ron` files.
    pub fn save_all(&mut self, project: &ProjectLayout) -> Result<SaveReport, String> {
        let scripts_dir = project.module_path("scripts");
        let actions_dir = project.module_path("actions");
        create_dir(&scripts_dir)?;
        create_dir(&actions_dir)?;

        let mut program_stems = BTreeSet::new();
        for program in &self.programs {
            let stem = sanitize_file_stem(&program.name);
            let key = stem.to_ascii_lowercase();
            if !program_stems.insert(key) {
                return Err(format!(
                    "Two programs map to the same file name '{stem}.ron'."
                ));
            }
            save_ron(&scripts_dir.join(format!("{stem}.ron")), program)?;
        }

        let mut action_stems = BTreeSet::new();
        for action in &self.actions {
            let stem = sanitize_file_stem(&action.name);
            let key = stem.to_ascii_lowercase();
            if !action_stems.insert(key) {
                return Err(format!(
                    "Two actions map to the same file name '{stem}.ron'."
                ));
            }
            save_ron(&actions_dir.join(format!("{stem}.ron")), action)?;
        }

        let removed =
            remove_stale(&scripts_dir, &program_stems)? + remove_stale(&actions_dir, &action_stems)?;
        self.last_saved_revision = self.revision;

        Ok(SaveReport {
            programs: self.programs.len(),
            actions: self.actions.len(),
            removed,
        })
    }

    /// Structural validation. Errors block meaningful execution; warnings are
    /// authoring hints and do not prevent the program from running.
    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        let action_names: BTreeSet<String> = self
            .actions
            .iter()
            .map(|action| action.name.clone())
            .collect();

        self.validate_names(&mut issues);

        for program in &self.programs {
            let base = format!("Program '{}'", program.name);
            if program.is_empty() {
                issues.push(ValidationIssue::warning(
                    base.clone(),
                    "the program has no block in any event handler.",
                ));
            }
            for (handler, blocks) in program.handlers() {
                validate_blocks(blocks, &format!("{base} / {handler}"), &action_names, &mut issues);
            }
            for handler in &program.events {
                validate_blocks(&handler.blocks, &format!("{base} / {}", handler.event.label()), &action_names, &mut issues);
            }
        }

        for action in &self.actions {
            validate_blocks(
                &action.blocks,
                &format!("Action '{}'", action.name),
                &action_names,
                &mut issues,
            );
        }

        self.validate_action_cycles(&mut issues);
        issues
    }

    fn validate_names(&self, issues: &mut Vec<ValidationIssue>) {
        let mut program_names = BTreeSet::new();
        let mut program_files = BTreeSet::new();
        for program in &self.programs {
            if program.name.trim().is_empty() {
                issues.push(ValidationIssue::error(
                    "Program",
                    "a program has an empty name.",
                ));
            }
            let key = program.name.to_ascii_lowercase();
            if !program_names.insert(key) {
                issues.push(ValidationIssue::error(
                    format!("Program '{}'", program.name),
                    "duplicate program name.",
                ));
            }
            let file = sanitize_file_stem(&program.name).to_ascii_lowercase();
            if !program_files.insert(file) {
                issues.push(ValidationIssue::error(
                    format!("Program '{}'", program.name),
                    "two programs map to the same file name.",
                ));
            }
        }

        let mut action_names = BTreeSet::new();
        let mut action_files = BTreeSet::new();
        for action in &self.actions {
            if action.name.trim().is_empty() {
                issues.push(ValidationIssue::error(
                    "Action",
                    "an action has an empty name.",
                ));
            }
            let key = action.name.to_ascii_lowercase();
            if !action_names.insert(key) {
                issues.push(ValidationIssue::error(
                    format!("Action '{}'", action.name),
                    "duplicate action name.",
                ));
            }
            let file = sanitize_file_stem(&action.name).to_ascii_lowercase();
            if !action_files.insert(file) {
                issues.push(ValidationIssue::error(
                    format!("Action '{}'", action.name),
                    "two actions map to the same file name.",
                ));
            }
        }
    }

    fn validate_action_cycles(&self, issues: &mut Vec<ValidationIssue>) {
        let graph: std::collections::BTreeMap<&str, Vec<&str>> = self
            .actions
            .iter()
            .map(|action| {
                (
                    action.name.as_str(),
                    collect_calls(&action.blocks),
                )
            })
            .collect();

        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        for action in &self.actions {
            detect_cycle(
                action.name.as_str(),
                &graph,
                &mut visiting,
                &mut visited,
                issues,
            );
        }
    }

    fn clamp_selection(&mut self) {
        if self.programs.is_empty() && self.actions.is_empty() {
            self.selection = ScriptSelection::Program(0);
            return;
        }

        match self.selection {
            ScriptSelection::Program(index) => {
                if self.programs.is_empty() {
                    self.selection = ScriptSelection::Action(0);
                } else if index >= self.programs.len() {
                    self.selection = ScriptSelection::Program(self.programs.len() - 1);
                }
            }
            ScriptSelection::Action(index) => {
                if self.actions.is_empty() {
                    self.selection = ScriptSelection::Program(0);
                } else if index >= self.actions.len() {
                    self.selection = ScriptSelection::Action(self.actions.len() - 1);
                }
            }
        }
    }
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

fn create_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| format!("cannot create {}: {error}", path.display()))
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

fn validate_blocks(
    blocks: &[ScriptBlock],
    location: &str,
    action_names: &BTreeSet<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    for (index, block) in blocks.iter().enumerate() {
        let block_location = format!("{location} / block {}", index + 1);
        match block {
            ScriptBlock::Move { distance } => {
                if !distance.is_finite() {
                    issues.push(ValidationIssue::error(
                        block_location,
                        "move distance is not finite.",
                    ));
                }
            }
            ScriptBlock::Rotate { degrees } => {
                if !degrees.is_finite() {
                    issues.push(ValidationIssue::error(
                        block_location,
                        "rotation is not finite.",
                    ));
                }
            }
            ScriptBlock::Wait { seconds } => {
                if !seconds.is_valid() || matches!(seconds, Operand::Number(value) if *value < 0.0) {
                    issues.push(ValidationIssue::error(
                        block_location,
                        "wait duration must be a positive number or a variable.",
                    ));
                } else if matches!(seconds, Operand::Number(value) if *value > 60.0) {
                    issues.push(ValidationIssue::warning(
                        block_location,
                        "wait duration above 60 seconds will pause the program for a long time.",
                    ));
                }
            }
            ScriptBlock::Emit { signal } => {
                if signal.trim().is_empty() {
                    issues.push(ValidationIssue::warning(
                        block_location,
                        "the emitted signal name is empty.",
                    ));
                }
            }
            ScriptBlock::Log { message } => {
                if message.trim().is_empty() {
                    issues.push(ValidationIssue::warning(
                        block_location,
                        "the log message is empty.",
                    ));
                }
            }
            ScriptBlock::SetVar { name, value } | ScriptBlock::AddVar { name, value } => {
                if name.trim().is_empty() {
                    issues.push(ValidationIssue::error(
                        block_location.clone(),
                        "variable name is empty.",
                    ));
                }
                if !value.is_valid() {
                    issues.push(ValidationIssue::error(
                        block_location,
                        "variable value is not finite.",
                    ));
                }
            }
            ScriptBlock::AddEnergy { amount } => {
                if !amount.is_finite() {
                    issues.push(ValidationIssue::error(
                        block_location,
                        "energy amount is not finite.",
                    ));
                }
            }
            ScriptBlock::Repeat { times, body } => {
                if *times == 0 {
                    issues.push(ValidationIssue::warning(
                        block_location.clone(),
                        "repeat count is zero and the body will never run.",
                    ));
                } else if *times > 10_000 {
                    issues.push(ValidationIssue::warning(
                        block_location.clone(),
                        "repeat count is above the VM limit and will be clamped.",
                    ));
                }
                validate_blocks(
                    body,
                    &format!("{block_location} / body"),
                    action_names,
                    issues,
                );
            }
            ScriptBlock::If {
                variable,
                op: _,
                value,
                then_body,
                else_body,
            } => {
                if variable.trim().is_empty() {
                    issues.push(ValidationIssue::error(
                        block_location.clone(),
                        "condition variable is empty.",
                    ));
                }
                if !value.is_valid() {
                    issues.push(ValidationIssue::error(
                        block_location.clone(),
                        "condition value is not finite.",
                    ));
                }
                validate_blocks(
                    then_body,
                    &format!("{block_location} / then"),
                    action_names,
                    issues,
                );
                validate_blocks(
                    else_body,
                    &format!("{block_location} / else"),
                    action_names,
                    issues,
                );
            }
            ScriptBlock::While {
                variable,
                op: _,
                value,
                max_iterations,
                body,
            } => {
                if variable.trim().is_empty() {
                    issues.push(ValidationIssue::error(
                        block_location.clone(),
                        "while variable is empty.",
                    ));
                }
                if !value.is_valid() {
                    issues.push(ValidationIssue::error(
                        block_location.clone(),
                        "while value is not finite.",
                    ));
                }
                if *max_iterations == 0 {
                    issues.push(ValidationIssue::error(
                        block_location.clone(),
                        "while max iterations must be at least 1.",
                    ));
                } else if *max_iterations > 10_000 {
                    issues.push(ValidationIssue::warning(
                        block_location.clone(),
                        "while max iterations is above the VM limit and will be clamped.",
                    ));
                }
                validate_blocks(
                    body,
                    &format!("{block_location} / body"),
                    action_names,
                    issues,
                );
            }
            ScriptBlock::CallAction { action, .. } => {
                if action.trim().is_empty() {
                    issues.push(ValidationIssue::error(
                        block_location,
                        "called action name is empty.",
                    ));
                } else if !action_names.contains(action) {
                    issues.push(ValidationIssue::error(
                        block_location,
                        format!("unknown action '{action}'."),
                    ));
                }
            }
            ScriptBlock::Math { name, value, .. } => {
                if name.trim().is_empty() {
                    issues.push(ValidationIssue::error(block_location.clone(), "math variable is empty."));
                }
                if !value.is_valid() {
                    issues.push(ValidationIssue::error(block_location, "math operand is not finite."));
                }
            }
            ScriptBlock::Sense { name, .. } => {
                if name.trim().is_empty() {
                    issues.push(ValidationIssue::error(block_location, "sense variable is empty."));
                }
            }
            ScriptBlock::SetBlock { block, .. } => {
                if block.trim().is_empty() {
                    issues.push(ValidationIssue::error(block_location, "block key is empty."));
                }
            }
            ScriptBlock::SpawnParticle { count, .. } if *count > 256 => {
                issues.push(ValidationIssue::warning(
                    block_location,
                    "more than 256 particles per call will be clamped.",
                ));
            }
            ScriptBlock::DropResource { resource, .. } => {
                if resource.trim().is_empty() {
                    issues.push(ValidationIssue::error(block_location, "resource name is empty."));
                }
            }
            ScriptBlock::MoveToward { speed, .. } => {
                if !speed.is_finite() || *speed < 0.0 {
                    issues.push(ValidationIssue::error(block_location, "speed must be positive."));
                }
            }
            ScriptBlock::ApplyImpulse { impulse } => {
                if impulse.iter().any(|v| !v.is_finite()) {
                    issues.push(ValidationIssue::error(block_location, "impulse is not finite."));
                }
            }
            ScriptBlock::PlayAction { action, .. } => {
                if action.trim().is_empty() {
                    issues.push(ValidationIssue::warning(block_location, "action clip name is empty."));
                }
            }
            ScriptBlock::SetSignal { port, value } => {
                if port.trim().is_empty() {
                    issues.push(ValidationIssue::error(block_location.clone(), "signal port is empty."));
                }
                if !value.is_valid() {
                    issues.push(ValidationIssue::error(block_location, "signal value is not finite."));
                }
            }
            ScriptBlock::Compute { name, .. }
            | ScriptBlock::RandomRange { name, .. }
            | ScriptBlock::Ask { name, .. }
            | ScriptBlock::ListGet { name, .. }
            | ScriptBlock::ListLength { name, .. } => {
                if name.trim().is_empty() {
                    issues.push(ValidationIssue::error(block_location, "the result variable is empty."));
                }
            }
            ScriptBlock::ListPush { list, .. } | ScriptBlock::ListSet { list, .. } | ScriptBlock::ListClear { list } => {
                if list.trim().is_empty() {
                    issues.push(ValidationIssue::error(block_location, "the list name is empty."));
                }
            }
            ScriptBlock::GiveItem { key, .. } | ScriptBlock::TakeItem { key, .. } | ScriptBlock::SaveValue { key, .. } => {
                if key.trim().is_empty() {
                    issues.push(ValidationIssue::error(block_location, "the name is empty."));
                }
            }
            ScriptBlock::MoveTo { .. }
            | ScriptBlock::Shoot { .. }
            | ScriptBlock::Broadcast { .. }
            | ScriptBlock::SetPlayerStat { .. }
            | ScriptBlock::DamagePlayer { .. }
            | ScriptBlock::TeleportPlayer { .. }
            | ScriptBlock::ShowMessage { .. }
            | ScriptBlock::OpenScreen { .. }
            | ScriptBlock::CloseScreen { .. }
            | ScriptBlock::SetWeather { .. }
            | ScriptBlock::StartQuest { .. }
            | ScriptBlock::ShowDialogue { .. }
            | ScriptBlock::Breakpoint => {}
            ScriptBlock::SpawnParticle { .. }
            | ScriptBlock::BreakBlock { .. }
            | ScriptBlock::PlaySound { .. }
            | ScriptBlock::SetTimeOfDay { .. }
            | ScriptBlock::SpawnMob { .. }
            | ScriptBlock::SetHealth { .. }
            | ScriptBlock::Attack { .. }
            | ScriptBlock::TameEntity => {}
        }
    }
}

fn collect_calls(blocks: &[ScriptBlock]) -> Vec<&str> {
    let mut calls = Vec::new();
    for block in blocks {
        match block {
            ScriptBlock::CallAction { action, .. } => calls.push(action.as_str()),
            ScriptBlock::Repeat { body, .. } | ScriptBlock::While { body, .. } => {
                calls.extend(collect_calls(body));
            }
            ScriptBlock::If {
                then_body,
                else_body,
                ..
            } => {
                calls.extend(collect_calls(then_body));
                calls.extend(collect_calls(else_body));
            }
            _ => {}
        }
    }
    calls
}

fn detect_cycle<'a>(
    name: &'a str,
    graph: &std::collections::BTreeMap<&'a str, Vec<&'a str>>,
    visiting: &mut BTreeSet<&'a str>,
    visited: &mut BTreeSet<&'a str>,
    issues: &mut Vec<ValidationIssue>,
) {
    if visited.contains(name) {
        return;
    }
    if !visiting.insert(name) {
        issues.push(ValidationIssue::error(
            format!("Action '{name}'"),
            "recursive action call detected; the VM would never finish.",
        ));
        return;
    }

    if let Some(calls) = graph.get(name) {
        for call in calls {
            if graph.contains_key(call) {
                detect_cycle(call, graph, visiting, visited, issues);
            }
        }
    }

    visiting.remove(name);
    visited.insert(name);
}

/// Ready-made programs offered by the New menu.
pub fn program_templates() -> Vec<ScriptProgram> {
    use crate::scripting::model::{CompareOp, EventHandler, GameEvent, Question, ScriptTarget, Sensor};
    let var = |name: &str| Operand::Var(name.to_string());
    let sense = |name: &str, sensor| ScriptBlock::Sense { name: name.to_string(), sensor };
    let when = |variable: &str, op, value: f32, then_body, else_body| ScriptBlock::If {
        variable: variable.to_string(),
        op,
        value: value.into(),
        then_body,
        else_body,
    };
    let bound = |name: &str, description: &str| ScriptProgram {
        description: description.to_string(),
        bound: true,
        enabled: true,
        ..ScriptProgram::new(name.to_string())
    };
    vec![
        ScriptProgram {
            on_tick: vec![
                sense("distance", Sensor::DistanceToPlayer),
                when(
                    "distance",
                    CompareOp::Lt,
                    18.0,
                    vec![
                        when(
                            "distance",
                            CompareOp::Lt,
                            7.0,
                            vec![ScriptBlock::MoveToward { target: ScriptTarget::AwayFromPlayer, speed: 2.6 }],
                            vec![ScriptBlock::MoveToward { target: ScriptTarget::Player, speed: 2.6 }],
                        ),
                        ScriptBlock::Shoot { damage: 1.5.into(), range: 19.0.into() },
                        ScriptBlock::Wait { seconds: 2.2.into() },
                    ],
                    vec![ScriptBlock::MoveToward { target: ScriptTarget::Home, speed: 1.2 }],
                ),
            ],
            ..bound("Shooter", "Keeps its distance and fires at the player. Bind it to a mob.")
        },
        ScriptProgram {
            on_tick: vec![
                sense("distance", Sensor::DistanceToPlayer),
                when(
                    "distance",
                    CompareOp::Lt,
                    14.0,
                    vec![ScriptBlock::Shoot { damage: 2.0.into(), range: 14.0.into() }, ScriptBlock::Wait { seconds: 1.0.into() }],
                    Vec::new(),
                ),
            ],
            ..bound("Turret", "Fires at the player. Bind it to a block.")
        },
        ScriptProgram {
            on_tick: vec![
                sense("distance", Sensor::DistanceToPlayer),
                when("distance", CompareOp::Gt, 3.0, vec![ScriptBlock::MoveToward { target: ScriptTarget::Player, speed: 3.5 }], Vec::new()),
            ],
            ..bound("Follower", "Walks after the player. Bind it to a mob.")
        },
        ScriptProgram {
            on_interact: vec![
                ScriptBlock::If {
                    variable: "open".to_string(),
                    op: CompareOp::Lt,
                    value: 1.0.into(),
                    then_body: vec![
                        ScriptBlock::SetVar { name: "open".to_string(), value: 1.0.into() },
                        ScriptBlock::BreakBlock { offset: [0, 1, 0], drop: false },
                    ],
                    else_body: vec![
                        ScriptBlock::SetVar { name: "open".to_string(), value: 0.0.into() },
                        ScriptBlock::SetBlock { offset: [0, 1, 0], block: "stone".to_string(), rot: 0 },
                    ],
                },
            ],
            ..bound("Door", "Opens or closes the block above when used. Bind it to a block.")
        },
        ScriptProgram {
            enabled: true,
            description: "Counts kills and gives a reward at the fifth.".to_string(),
            events: vec![EventHandler {
                event: GameEvent::MobKilled,
                filter: String::new(),
                blocks: vec![
                    ScriptBlock::AddVar { name: "g.hunted".to_string(), value: 1.0.into() },
                    ScriptBlock::ShowMessage { text: "Hunted: {g.hunted} / 5".to_string() },
                    when(
                        "g.hunted",
                        CompareOp::Ge,
                        5.0,
                        vec![
                            ScriptBlock::GiveItem { key: "synthetic".to_string(), amount: 5.0.into() },
                            ScriptBlock::SetVar { name: "g.hunted".to_string(), value: 0.0.into() },
                        ],
                        Vec::new(),
                    ),
                ],
            }],
            ..ScriptProgram::new("Hunt reward".to_string())
        },
        ScriptProgram {
            enabled: true,
            description: "Sells 3 synthetic for a heal.".to_string(),
            on_interact: vec![
                ScriptBlock::Ask { name: "stock".to_string(), question: Question::ItemCount { key: "synthetic".to_string() } },
                ScriptBlock::TakeItem { key: "synthetic".to_string(), amount: 3.0.into(), result: "paid".to_string() },
                when(
                    "paid",
                    CompareOp::Ge,
                    1.0,
                    vec![
                        ScriptBlock::SetPlayerStat { stat: crate::scripting::model::PlayerStat::Health, value: 6.0.into(), add: true },
                        ScriptBlock::ShowMessage { text: "Healed.".to_string() },
                    ],
                    vec![ScriptBlock::ShowMessage { text: "You need 3 synthetic ({stock} held).".to_string() }],
                ),
            ],
            bound: true,
            ..ScriptProgram::new("Healing shop".to_string())
        },
        ScriptProgram {
            enabled: true,
            description: "Keeps the best score across runs.".to_string(),
            events: vec![EventHandler {
                event: GameEvent::MobKilled,
                filter: String::new(),
                blocks: vec![
                    ScriptBlock::Ask { name: "best".to_string(), question: Question::Saved { key: "best".to_string() } },
                    ScriptBlock::Ask { name: "kills".to_string(), question: Question::Stat { stat: crate::scripting::model::PlayerStat::Kills } },
                    ScriptBlock::If {
                        variable: "kills".to_string(),
                        op: CompareOp::Gt,
                        value: var("best"),
                        then_body: vec![ScriptBlock::SaveValue { key: "best".to_string(), value: var("kills") }],
                        else_body: Vec::new(),
                    },
                ],
            }],
            ..ScriptProgram::new("Best score".to_string())
        },
    ]
}

fn default_programs() -> Vec<ScriptProgram> {
    vec![
        ScriptProgram {
            name: "Beacon Pulse".to_string(),
            enabled: true,
            description: "Pulses a beacon signal three times and tracks a slow heartbeat."
                .to_string(),
            on_start: vec![
                ScriptBlock::log_default(),
                ScriptBlock::Emit {
                    signal: "boot".to_string(),
                },
                ScriptBlock::SetVar {
                    name: "energy".to_string(),
                    value: 0.0.into(),
                },
                ScriptBlock::Repeat {
                    times: 3,
                    body: vec![
                        ScriptBlock::Wait { seconds: 0.6.into() },
                        ScriptBlock::AddEnergy { amount: 12.0 },
                        ScriptBlock::Emit {
                            signal: "pulse".to_string(),
                        },
                    ],
                },
                ScriptBlock::CallAction {
                    action: "Patrol".to_string(),
args: Vec::new(),
                },
            ],
            on_tick: vec![
                ScriptBlock::AddVar {
                    name: "ticks".to_string(),
                    value: 1.0.into(),
                },
                ScriptBlock::If {
                    variable: "ticks".to_string(),
                    op: CompareOp::Ge,
                    value: 120.0.into(),
                    then_body: vec![
                        ScriptBlock::SetVar {
                            name: "ticks".to_string(),
                            value: 0.0.into(),
                        },
                        ScriptBlock::Emit {
                            signal: "heartbeat".to_string(),
                        },
                    ],
                    else_body: Vec::new(),
                },
            ],
            ..ScriptProgram::new(String::new())
        },
        ScriptProgram {
            name: "Patrol Loop".to_string(),
            enabled: true,
            description: "Repeats the Patrol action and emits a checkpoint after each cycle."
                .to_string(),
            on_start: vec![ScriptBlock::Repeat {
                times: 4,
                body: vec![
                    ScriptBlock::CallAction {
                        action: "Patrol".to_string(),
args: Vec::new(),
                    },
                    ScriptBlock::Wait { seconds: 0.5.into() },
                    ScriptBlock::Log {
                        message: "patrol cycle".to_string(),
                    },
                ],
            }],
            on_tick: Vec::new(),
            ..ScriptProgram::new(String::new())
        },
        mob_brain_program(),
        day_night_program(),
        signal_lamp_program(),
    ]
}

/// Bound AI for mobs: chase when close, flee when hurt, strike in range.
fn mob_brain_program() -> ScriptProgram {
    use crate::scripting::model::{ScriptTarget, Sensor};
    let mut program = ScriptProgram::new("Mob Brain");
    program.bound = true;
    program.description =
        "Patrols home, chases the player inside 12 m, attacks in reach and flees below 3 HP."
            .to_string();
    program.on_start = vec![ScriptBlock::PlayAction {
        action: "Idle".to_string(),
        blend: 0.2,
    }];
    program.on_tick = vec![
        ScriptBlock::Sense {
            name: "dist".to_string(),
            sensor: Sensor::DistanceToPlayer,
        },
        ScriptBlock::Sense {
            name: "hp".to_string(),
            sensor: Sensor::Health,
        },
        ScriptBlock::If {
            variable: "hp".to_string(),
            op: CompareOp::Lt,
            value: 3.0.into(),
            then_body: vec![ScriptBlock::MoveToward {
                target: ScriptTarget::AwayFromPlayer,
                speed: 4.5,
            }],
            else_body: vec![ScriptBlock::If {
                variable: "dist".to_string(),
                op: CompareOp::Lt,
                value: 12.0.into(),
                then_body: vec![
                    ScriptBlock::MoveToward {
                        target: ScriptTarget::Player,
                        speed: 3.2,
                    },
                    ScriptBlock::If {
                        variable: "dist".to_string(),
                        op: CompareOp::Lt,
                        value: 1.9.into(),
                        then_body: vec![
                            ScriptBlock::PlayAction {
                                action: "Attack".to_string(),
                                blend: 0.08,
                            },
                            ScriptBlock::Attack {
                                damage: 1.0,
                                range: 2.0,
                            },
                            ScriptBlock::Wait { seconds: 0.9.into() },
                        ],
                        else_body: Vec::new(),
                    },
                ],
                else_body: vec![ScriptBlock::MoveToward {
                    target: ScriptTarget::Home,
                    speed: 1.5,
                }],
            }],
        },
    ];
    program.on_hit = vec![
        ScriptBlock::SpawnParticle {
            kind: "hit".to_string(),
            count: 10,
        },
        ScriptBlock::PlaySound {
            sound: "impact".to_string(),
        },
        ScriptBlock::ApplyImpulse {
            impulse: [0.0, 3.0, 0.0],
        },
    ];
    program
}

/// World rules: a 4-minute day / night cycle.
fn day_night_program() -> ScriptProgram {
    let mut program = ScriptProgram::new("Day Night");
    program.description = "World rule: advances the time of day (one cycle every 240 s).".to_string();
    program.on_start = vec![
        ScriptBlock::SetVar {
            name: "time".to_string(),
            value: 0.3.into(),
        },
        ScriptBlock::SetTimeOfDay { value: 0.3 },
    ];
    program.on_tick = vec![
        ScriptBlock::Wait { seconds: 1.0.into() },
        ScriptBlock::AddVar {
            name: "time".to_string(),
            value: Operand::Number(1.0 / 240.0),
        },
        ScriptBlock::Math {
            name: "time".to_string(),
            op: crate::scripting::model::MathOp::Mod,
            value: 1.0.into(),
        },
        ScriptBlock::SetTimeOfDay { value: -1.0 },
    ];
    program
}

/// Bound block logic: a lamp toggled by a cable signal or interaction.
fn signal_lamp_program() -> ScriptProgram {
    let mut program = ScriptProgram::new("Signal Lamp");
    program.bound = true;
    program.description =
        "Block logic: forwards interactions as a cable signal and reacts to power.".to_string();
    program.on_interact = vec![
        ScriptBlock::PlaySound {
            sound: "ui_beep".to_string(),
        },
        ScriptBlock::SetSignal {
            port: "out".to_string(),
            value: 1.0.into(),
        },
    ];
    program.on_signal = vec![ScriptBlock::SpawnParticle {
        kind: "spark".to_string(),
        count: 6,
    }];
    program.on_energy = vec![ScriptBlock::Log {
        message: "Lamp power changed".to_string(),
    }];
    program
}

fn default_actions() -> Vec<ActionDef> {
    vec![ActionDef {
        name: "Patrol".to_string(),
        description: "Moves the script avatar in a square and emits a checkpoint.".to_string(),
        blocks: vec![
            ScriptBlock::Move { distance: 2.0 },
            ScriptBlock::Rotate { degrees: 90.0 },
            ScriptBlock::Move { distance: 2.0 },
            ScriptBlock::Rotate { degrees: 90.0 },
            ScriptBlock::Emit {
                signal: "checkpoint".to_string(),
            },
        ],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::project_io::{ProjectManifest, PROJECT_MODULES};
    use crate::scripting::model::IssueSeverity;

    #[test]
    fn file_stems_are_safe() {
        assert_eq!(sanitize_file_stem("Beacon Pulse"), "Beacon_Pulse");
        assert_eq!(sanitize_file_stem("a/b"), "a_b");
        assert_eq!(sanitize_file_stem("   "), "untitled");
    }

    #[test]
    fn default_library_round_trips_through_ron() {
        let program = default_programs().remove(0);
        let text = ron::ser::to_string_pretty(&program, ron::ser::PrettyConfig::default())
            .expect("program serializes");
        let parsed: ScriptProgram = ron::from_str(&text).expect("program parses");
        assert_eq!(program, parsed);
    }

    #[test]
    fn recursive_actions_are_reported() {
        let mut library = ScriptLibrary::default();
        library.actions.push(ActionDef {
            name: "A".to_string(),
            description: String::new(),
            blocks: vec![ScriptBlock::call_action_default("B")],
        });
        library.actions.push(ActionDef {
            name: "B".to_string(),
            description: String::new(),
            blocks: vec![ScriptBlock::call_action_default("A")],
        });

        let issues = library.validate();
        assert!(issues.iter().any(|issue| {
            issue.severity == IssueSeverity::Error && issue.message.contains("recursive")
        }));
    }

    #[test]
    fn unknown_actions_are_reported() {
        let mut library = ScriptLibrary::default();
        library.programs.push(ScriptProgram {
            name: "Broken".to_string(),
            enabled: true,
            description: String::new(),
            on_start: vec![ScriptBlock::call_action_default("Missing")],
            on_tick: Vec::new(),
            ..ScriptProgram::new(String::new())
        });

        let issues = library.validate();
        assert!(issues
            .iter()
            .any(|issue| issue.message.contains("unknown action 'Missing'")));
    }

    #[test]
    fn bundled_project_scripts_parse() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("project");
        let project = ProjectLayout {
            root,
            manifest: ProjectManifest::default(),
            modules: PROJECT_MODULES.to_vec(),
        };

        let mut library = ScriptLibrary::default();
        let report = library
            .load_from_project(&project)
            .expect("bundled scripts load");

        // Two authored programs plus the merged gameplay starters.
        assert!(report.programs >= 2);
        assert_eq!(report.actions, 1);
        for name in ["Mob Brain", "Day Night", "Signal Lamp"] {
            assert!(library.programs.iter().any(|program| program.name == name), "missing {name}");
        }
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(library
            .validate()
            .iter()
            .all(|issue| issue.severity != IssueSeverity::Error));
    }

    #[test]
    fn project_library_writes_and_reloads() {
        let root = std::env::temp_dir().join(format!(
            "polyhedron_script_library_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        let _ = fs::remove_dir_all(&root);

        let project = ProjectLayout {
            root: root.clone(),
            manifest: ProjectManifest::default(),
            modules: PROJECT_MODULES.to_vec(),
        };

        let mut library = ScriptLibrary::default();
        let report = library.load_from_project(&project).expect("library loads");
        assert!(report.created_defaults);
        assert!(report.programs >= 2);
        assert!(report.actions >= 1);
        assert!(root.join("scripts").join("Beacon_Pulse.ron").is_file());
        assert!(root.join("actions").join("Patrol.ron").is_file());

        let mut reloaded = ScriptLibrary::default();
        let report = reloaded.load_from_project(&project).expect("library reloads");
        assert_eq!(reloaded.programs.len(), library.programs.len());
        assert_eq!(reloaded.actions.len(), library.actions.len());
        assert!(report.warnings.is_empty());

        let _ = fs::remove_dir_all(&root);
    }
}
