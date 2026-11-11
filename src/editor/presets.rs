//! Customisation presets and the import / export of customisations and
//! worlds.
//!
//! - Presets: `project/ui/presets/<name>.ron` (the colour of each box of the
//!   player model).
//! - Exports: `project/exports/<name>.custom.ron` for a customisation,
//!   `project/exports/worlds/<world>/` (world `.ron` + `.meta`) for a world.
//!   Copy these files to another project's `exports` folder to import them.

use std::fs;
use std::path::{Path, PathBuf};

use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::core::settings::BoxColors;
use crate::voxel::biomes::file_stem;

const CUSTOM_SUFFIX: &str = ".custom.ron";

pub fn presets_dir(project: &ProjectLayout) -> PathBuf {
    project.root.join("ui").join("presets")
}

pub fn exports_dir(project: &ProjectLayout) -> PathBuf {
    project.root.join("exports")
}

fn names_in(dir: &Path, suffix: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
                .filter_map(|name| name.strip_suffix(suffix).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Names of the saved customisation presets.
pub fn preset_names(project: &ProjectLayout) -> Vec<String> {
    names_in(&presets_dir(project), ".ron")
}

pub fn save_preset(project: &ProjectLayout, name: &str, colors: &BoxColors) -> Result<String, String> {
    let stem = file_stem(name);
    if stem.is_empty() {
        return Err("Give the preset a name.".to_string());
    }
    let dir = presets_dir(project);
    fs::create_dir_all(&dir).map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    save_ron(&dir.join(format!("{stem}.ron")), colors)?;
    Ok(stem)
}

pub fn load_preset(project: &ProjectLayout, name: &str) -> Result<BoxColors, String> {
    load_ron(&presets_dir(project).join(format!("{name}.ron")))
}

pub fn delete_preset(project: &ProjectLayout, name: &str) {
    let _ = fs::remove_file(presets_dir(project).join(format!("{name}.ron")));
}

/// Write a customisation to `exports/<name>.custom.ron`.
pub fn export_custom(project: &ProjectLayout, name: &str, colors: &BoxColors) -> Result<PathBuf, String> {
    let stem = file_stem(name);
    let stem = if stem.is_empty() { "character".to_string() } else { stem };
    let dir = exports_dir(project);
    fs::create_dir_all(&dir).map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    let path = dir.join(format!("{stem}{CUSTOM_SUFFIX}"));
    save_ron(&path, colors)?;
    Ok(path)
}

/// Customisations found in the exports folder.
pub fn importable_customs(project: &ProjectLayout) -> Vec<String> {
    names_in(&exports_dir(project), CUSTOM_SUFFIX)
}

pub fn import_custom(project: &ProjectLayout, name: &str) -> Result<BoxColors, String> {
    load_ron(&exports_dir(project).join(format!("{name}{CUSTOM_SUFFIX}")))
}

fn worlds_export_dir(project: &ProjectLayout) -> PathBuf {
    exports_dir(project).join("worlds")
}

/// Copy a world (`.ron` + `.meta`) to `exports/worlds/<stem>/`.
pub fn export_world(project: &ProjectLayout, stem: &str) -> Result<PathBuf, String> {
    let source = project.module_path("worlds");
    let target = worlds_export_dir(project).join(stem);
    fs::create_dir_all(&target).map_err(|error| format!("cannot create {}: {error}", target.display()))?;
    let mut copied = 0;
    for extension in ["ron", "meta"] {
        let from = source.join(format!("{stem}.{extension}"));
        if from.is_file() {
            fs::copy(&from, target.join(format!("{stem}.{extension}"))).map_err(|error| format!("cannot copy {}: {error}", from.display()))?;
            copied += 1;
        }
    }
    if copied == 0 {
        return Err(format!("The world '{stem}' has no file yet (play it once)."));
    }
    Ok(target)
}

/// Worlds found in `exports/worlds/`.
pub fn importable_worlds(project: &ProjectLayout) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(worlds_export_dir(project))
        .map(|entries| entries.flatten().filter(|entry| entry.path().is_dir()).filter_map(|entry| entry.file_name().to_str().map(str::to_string)).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// Copy an exported world into `project/worlds` (a free name is chosen when
/// the world already exists). Returns the stem it got.
pub fn import_world(project: &ProjectLayout, stem: &str) -> Result<String, String> {
    let source = worlds_export_dir(project).join(stem);
    let target = project.module_path("worlds");
    fs::create_dir_all(&target).map_err(|error| format!("cannot create {}: {error}", target.display()))?;
    let mut name = stem.to_string();
    let mut index = 2;
    while target.join(format!("{name}.meta")).exists() || target.join(format!("{name}.ron")).exists() {
        name = format!("{stem}_{index}");
        index += 1;
    }
    for extension in ["ron", "meta"] {
        let from = source.join(format!("{stem}.{extension}"));
        if from.is_file() {
            fs::copy(&from, target.join(format!("{name}.{extension}"))).map_err(|error| format!("cannot copy {}: {error}", from.display()))?;
        }
    }
    // The meta keeps the world's display name in sync with its new file.
    let meta_path = target.join(format!("{name}.meta"));
    if name != stem && meta_path.is_file() {
        if let Ok(mut meta) = load_ron::<crate::editor::worlds::WorldMeta>(&meta_path) {
            meta.name = name.replace('_', " ");
            let _ = save_ron(&meta_path, &meta);
        }
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_exports_round_trip() {
        let root = std::env::temp_dir().join(format!("polyhedron_presets_{}", std::process::id()));
        let project = ProjectLayout { root: root.clone(), ..ProjectLayout::default() };
        let mut colors = BoxColors::new();
        colors.insert("Head".to_string(), [1, 2, 3]);
        let stem = save_preset(&project, "My hero", &colors).expect("saved");
        assert_eq!(preset_names(&project), vec![stem.clone()]);
        assert_eq!(load_preset(&project, &stem).expect("loaded"), colors);
        export_custom(&project, "My hero", &colors).expect("exported");
        assert_eq!(importable_customs(&project), vec!["My_hero".to_string()]);
        assert_eq!(import_custom(&project, "My_hero").expect("imported"), colors);
        delete_preset(&project, &stem);
        assert!(preset_names(&project).is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
