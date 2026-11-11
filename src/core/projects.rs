//! Project management: create, open, duplicate and export projects, rename
//! and delete their content safely. Nothing is ever imported from outside.
//!
//! The open project is remembered in `polyhedron_projects.ron`; switching
//! project restarts the application on it, so no state of the old project
//! leaks. A folder holding `game.ron` next to the executable is an exported
//! game: it starts without the studio.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::core::project_io::{load_ron, save_ron, ProjectLayout, ProjectManifest, MANIFEST_FILE, PROJECT_DIRECTORY};
use crate::voxel::biomes::file_stem;

const INDEX_FILE: &str = "polyhedron_projects.ron";
const GAME_FILE: &str = "game.ron";
const PROJECTS_FOLDER: &str = "projects";
/// Deleted content goes here, inside the project, and can be taken back.
pub const TRASH_FOLDER: &str = ".trash";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectIndex {
    pub current: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
}

/// Marks a folder as an exported game.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GameInfo {
    pub title: String,
}

fn working_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

static GAME: LazyLock<Option<GameInfo>> = LazyLock::new(|| load_ron(&exe_dir()?.join(GAME_FILE)).ok());

/// The exported game this executable belongs to, if any.
pub fn game_info() -> Option<&'static GameInfo> {
    GAME.as_ref()
}

/// True in an exported game: no studio.
pub fn game_only() -> bool {
    GAME.is_some()
}

pub fn load_index() -> ProjectIndex {
    load_ron(&working_dir().join(INDEX_FILE)).unwrap_or_default()
}

fn save_index(index: &ProjectIndex) -> Result<(), String> {
    save_ron(&working_dir().join(INDEX_FILE), index)
}

/// The project to open at startup.
pub fn startup_root() -> PathBuf {
    if game_only() {
        if let Some(dir) = exe_dir() {
            return dir.join(PROJECT_DIRECTORY);
        }
    }
    load_index()
        .current
        .filter(|path| path.join(MANIFEST_FILE).is_file())
        .unwrap_or_else(|| working_dir().join(PROJECT_DIRECTORY))
}

/// Every project known here: the default one, those in `projects/`, then the
/// recent ones.
pub fn known_projects() -> Vec<PathBuf> {
    let mut found = vec![working_dir().join(PROJECT_DIRECTORY)];
    if let Ok(entries) = fs::read_dir(working_dir().join(PROJECTS_FOLDER)) {
        found.extend(entries.flatten().map(|entry| entry.path()).filter(|path| path.join(MANIFEST_FILE).is_file()));
    }
    found.extend(load_index().recent.into_iter().filter(|path| path.join(MANIFEST_FILE).is_file()));
    let mut unique = Vec::new();
    for path in found {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    unique
}

fn remember(index: &mut ProjectIndex, root: &Path) {
    index.current = Some(root.to_path_buf());
    index.recent.retain(|path| path != root);
    index.recent.insert(0, root.to_path_buf());
    index.recent.truncate(12);
}

/// Open a project: the application restarts on it.
pub fn open_project(root: &Path) -> Result<(), String> {
    let mut index = load_index();
    remember(&mut index, root);
    save_index(&index)?;
    let exe = std::env::current_exe().map_err(|error| format!("cannot find the executable: {error}"))?;
    std::process::Command::new(exe).current_dir(working_dir()).spawn().map_err(|error| format!("cannot restart: {error}"))?;
    Ok(())
}

/// Folder of a new project called `name`.
pub fn project_folder(base: &Path, name: &str) -> Result<PathBuf, String> {
    let stem = file_stem(name);
    if stem.is_empty() {
        return Err("Give the project a name.".to_string());
    }
    let root = base.join(PROJECTS_FOLDER).join(stem);
    if root.exists() {
        return Err(format!("{} already exists.", root.display()));
    }
    Ok(root)
}

/// Create a project. Its content comes from the engine defaults; the example
/// also has a quest line, a dialogue, an achievement and a song.
/// Folder of the project shipped with the engine.
const DEFAULT_PROJECT: &str = "project";

pub fn create_project(base: &Path, name: &str, example: bool) -> Result<PathBuf, String> {
    let root = project_folder(base, name)?;
    let mut layout = ProjectLayout {
        root: root.clone(),
        manifest: ProjectManifest { name: name.to_string(), description: String::new(), ..ProjectManifest::default() },
        ..ProjectLayout::default()
    };
    layout.ensure()?;
    // A new project starts from the content made in the studio, when it is there.
    for folder in ["blocks", "entities", "items", "biomes", "structures"] {
        let source = base.join(DEFAULT_PROJECT).join(folder);
        if source.is_dir() {
            copy_tree(&source, &root.join(folder), &[])?;
        }
    }
    if example {
        crate::gameplay::quests::StoryData::starter().save(&layout)?;
        let mut bank = crate::audio::synth::SoundBank::default();
        bank.songs.iter_mut().for_each(|song| song.enabled = true);
        bank.save(&layout)?;
    }
    Ok(root)
}

fn copy_tree(from: &Path, to: &Path, skip: &[&str]) -> Result<u32, String> {
    fs::create_dir_all(to).map_err(|error| format!("cannot create {}: {error}", to.display()))?;
    let mut copied = 0;
    for entry in fs::read_dir(from).map_err(|error| format!("cannot read {}: {error}", from.display()))?.flatten() {
        let name = entry.file_name();
        if skip.iter().any(|skipped| name == *skipped) {
            continue;
        }
        let target = to.join(&name);
        if entry.path().is_dir() {
            copied += copy_tree(&entry.path(), &target, skip)?;
        } else {
            fs::copy(entry.path(), &target).map_err(|error| format!("cannot copy {}: {error}", entry.path().display()))?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// Copy a project under a new name.
pub fn duplicate_project(base: &Path, from: &Path, name: &str) -> Result<PathBuf, String> {
    let root = project_folder(base, name)?;
    copy_tree(from, &root, &[TRASH_FOLDER])?;
    let path = root.join(MANIFEST_FILE);
    let mut manifest: ProjectManifest = load_ron(&path)?;
    manifest.name = name.to_string();
    save_ron(&path, &manifest)?;
    Ok(root)
}

/// Write a playable folder: the executable, the assets and the project.
pub fn export_game(project: &Path, title: &str, exe: &Path, assets: &Path, target: &Path) -> Result<u32, String> {
    let stem = file_stem(title);
    if stem.is_empty() {
        return Err("The project has no name.".to_string());
    }
    if target.exists() {
        return Err(format!("{} already exists: delete it or rename the project.", target.display()));
    }
    fs::create_dir_all(target).map_err(|error| format!("cannot create {}: {error}", target.display()))?;
    let mut copied = copy_tree(project, &target.join(PROJECT_DIRECTORY), &[TRASH_FOLDER, "worlds", "saved_values.ron", "quest_progress.ron"])?;
    if assets.is_dir() {
        copied += copy_tree(assets, &target.join("assets"), &[])?;
    }
    let extension = exe.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    fs::copy(exe, target.join(format!("{stem}{extension}"))).map_err(|error| format!("cannot copy the executable: {error}"))?;
    save_ron(&target.join(GAME_FILE), &GameInfo { title: title.to_string() })?;
    Ok(copied + 2)
}

/// Export next to the project folders, in `exports/<name>`.
pub fn export_current(project: &ProjectLayout) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| format!("cannot find the executable: {error}"))?;
    let base = working_dir();
    let target = base.join("exports").join(file_stem(&project.manifest.name));
    export_game(&project.root, &project.manifest.name, &exe, &base.join("assets"), &target)?;
    Ok(target)
}

fn ron_tree(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != TRASH_FOLDER {
                ron_tree(&path, out);
            }
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("ron")) {
            out.push(path);
        }
    }
}

/// Every file of a project, sorted.
pub fn project_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    ron_tree(root, &mut files);
    files.sort();
    files
}

/// The name other files use for this asset: its `id`, else its `name`.
pub fn asset_id(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    for field in ["id", "name"] {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix(field).and_then(|rest| rest.trim_start().strip_prefix(':')) {
                let value = rest.trim().trim_end_matches(',');
                if let Some(value) = value.strip_prefix('"').and_then(|value| value.strip_suffix('"')) {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// Files that mention an asset, apart from its own.
pub fn references(root: &Path, id: &str, own: &Path) -> Vec<PathBuf> {
    let quoted = format!("\"{id}\"");
    project_files(root)
        .into_iter()
        .filter(|path| path != own && fs::read_to_string(path).is_ok_and(|text| text.contains(&quoted)))
        .collect()
}

/// Rename an asset, its file and every reference to it. Returns the number
/// of files changed.
pub fn rename_asset(root: &Path, path: &Path, new_id: &str) -> Result<usize, String> {
    let new_id = new_id.trim();
    if new_id.is_empty() {
        return Err("Give it a name.".to_string());
    }
    let id = asset_id(path).ok_or_else(|| format!("{} has no name to change.", path.display()))?;
    let (old, new) = (format!("\"{id}\""), format!("\"{new_id}\""));
    if project_files(root).iter().any(|file| file != path && asset_id(file).as_deref() == Some(new_id)) {
        return Err(format!("'{new_id}' already exists."));
    }
    let mut changed = 0;
    for file in project_files(root) {
        let Ok(text) = fs::read_to_string(&file) else { continue };
        if text.contains(&old) {
            fs::write(&file, text.replace(&old, &new)).map_err(|error| format!("cannot write {}: {error}", file.display()))?;
            changed += 1;
        }
    }
    let stem = file_stem(new_id);
    if !stem.is_empty() {
        let target = path.with_file_name(format!("{stem}.ron"));
        if !target.exists() {
            fs::rename(path, &target).map_err(|error| format!("cannot rename {}: {error}", path.display()))?;
        }
    }
    Ok(changed)
}

/// Move a file to the project's trash; nothing is destroyed.
pub fn trash_asset(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let relative = path.strip_prefix(root).map_err(|_| "This file is outside the project.".to_string())?;
    let mut target = root.join(TRASH_FOLDER).join(relative);
    let mut copy = 1;
    while target.exists() {
        copy += 1;
        target = root.join(TRASH_FOLDER).join(relative).with_extension(format!("{copy}.ron"));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    fs::rename(path, &target).map_err(|error| format!("cannot move {}: {error}", path.display()))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("polyhedron_projects_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn projects_are_created_and_duplicated() {
        let base = sandbox("create");
        let root = create_project(&base, "My Game", true).unwrap();
        assert!(root.join(MANIFEST_FILE).is_file());
        assert!(root.join("quests.ron").is_file());
        assert!(create_project(&base, "My Game", false).is_err());
        let copy = duplicate_project(&base, &root, "Copy").unwrap();
        let manifest: ProjectManifest = load_ron(&copy.join(MANIFEST_FILE)).unwrap();
        assert_eq!(manifest.name, "Copy");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn renaming_updates_references_and_deleting_keeps_the_file() {
        let base = sandbox("rename");
        fs::create_dir_all(base.join("blocks")).unwrap();
        let block = base.join("blocks").join("ore.ron");
        fs::write(&block, "(\n    id: \"ore\",\n)\n").unwrap();
        let recipe = base.join("crafting.ron");
        fs::write(&recipe, "(recipes: [(output: \"ore\", note: \"more\")])").unwrap();
        assert_eq!(references(&base, "ore", &block), vec![recipe.clone()]);
        assert_eq!(rename_asset(&base, &block, "gem").unwrap(), 2);
        assert!(base.join("blocks").join("gem.ron").is_file());
        let text = fs::read_to_string(&recipe).unwrap();
        assert!(text.contains("\"gem\"") && text.contains("\"more\""));
        let moved = trash_asset(&base, &base.join("blocks").join("gem.ron")).unwrap();
        assert!(moved.is_file());
        assert!(!project_files(&base).contains(&moved));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn an_export_holds_the_game_without_the_saves() {
        let base = sandbox("export");
        let project = base.join("project");
        fs::create_dir_all(project.join("worlds")).unwrap();
        fs::write(project.join("polyhedron.ron"), "()").unwrap();
        fs::write(project.join("worlds").join("save.ron"), "()").unwrap();
        let exe = base.join("engine.exe");
        fs::write(&exe, "x").unwrap();
        let target = base.join("out");
        export_game(&project, "My Game", &exe, &base.join("assets"), &target).unwrap();
        assert!(target.join("My_Game.exe").is_file());
        assert!(target.join(GAME_FILE).is_file());
        assert!(target.join("project").join("polyhedron.ron").is_file());
        assert!(!target.join("project").join("worlds").exists());
        assert!(export_game(&project, "My Game", &exe, &base.join("assets"), &target).is_err());
        let _ = fs::remove_dir_all(&base);
    }
}
