//! Open, modular `.ron` project format.
//!
//! A project is a directory named `project` next to the executable's working
//! directory. The manifest is intentionally small: every subsystem owns its own
//! folder and can evolve independently without breaking the others.

use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::core::app_state::{EditorState, EditorUiState};

pub const PROJECT_DIRECTORY: &str = "project";
pub const MANIFEST_FILE: &str = "polyhedron.ron";

pub const PROJECT_MODULES: [&str; 9] = [
    "blocks",
    "entities",
    "actions",
    "scripts",
    "biomes",
    "structures",
    "ui",
    "shaders",
    "items",
];

/// Serialized project descriptor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub format_version: u32,
    pub name: String,
    pub description: String,
    pub engine_version: String,
    pub entry_scene: String,
    pub modules: Vec<String>,
}

impl Default for ProjectManifest {
    fn default() -> Self {
        Self {
            format_version: 1,
            name: "Polyhedron Sample".to_string(),
            description: "A procedural voxel project authored with Polyhedron Studio.".to_string(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            entry_scene: "entities/demo_scene.ron".to_string(),
            modules: PROJECT_MODULES
                .iter()
                .map(|module| (*module).to_string())
                .collect(),
        }
    }
}

/// Filesystem layout and loaded manifest.
#[derive(Resource, Clone, Debug)]
pub struct ProjectLayout {
    pub root: PathBuf,
    pub manifest: ProjectManifest,
    pub modules: Vec<&'static str>,
}

impl Default for ProjectLayout {
    fn default() -> Self {
        Self {
            root: default_project_root(),
            manifest: ProjectManifest::default(),
            modules: PROJECT_MODULES.to_vec(),
        }
    }
}

impl ProjectLayout {
    pub fn manifest_path(&self) -> PathBuf {
        self.root.join(MANIFEST_FILE)
    }

    pub fn module_path(&self, module: &str) -> PathBuf {
        self.root.join(module)
    }

    /// Folder holding serialized voxel worlds.
    pub fn worlds_path(&self) -> PathBuf {
        self.root.join("worlds")
    }

    /// Create missing directories and load or write the manifest.
    pub fn ensure(&mut self) -> Result<(), String> {
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("cannot create {}: {error}", self.root.display()))?;

        for module in &self.modules {
            let path = self.root.join(module);
            fs::create_dir_all(&path)
                .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
        }

        fs::create_dir_all(self.worlds_path())
            .map_err(|error| format!("cannot create {}: {error}", self.worlds_path().display()))?;

        let manifest_path = self.manifest_path();
        if manifest_path.is_file() {
            self.manifest = load_ron(&manifest_path)?;
        } else {
            save_ron(&manifest_path, &self.manifest)?;
        }

        write_if_missing(
            &self.root.join("entities").join("demo_scene.ron"),
            "(\n    name: \"Demo Scene\",\n    root: \"Ground Plane\",\n    objects: [\"Voxel Cube\", \"Voxel Wedge\", \"Voxel Cylinder\"],\n)\n",
        )?;
        write_if_missing(
            &self.root.join("ui").join("workspace.ron"),
            "(\n    theme: \"polyhedron-dark\",\n    layout: \"studio-default\",\n)\n",
        )?;

        Ok(())
    }
}

/// Read any RON document into a strongly typed value.
pub fn load_ron<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    ron::from_str(&source).map_err(|error| format!("cannot parse {}: {error}", path.display()))
}

/// Write any serializable value as pretty RON.
pub fn save_ron<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let pretty = ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())
        .map_err(|error| format!("cannot serialize {}: {error}", path.display()))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }

    fs::write(path, pretty)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn write_if_missing(path: &Path, contents: &str) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }

    fs::write(path, contents).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn default_project_root() -> PathBuf {
    crate::core::projects::startup_root()
}

/// Startup bootstrap for the project layout.
pub fn initialize_project_system(
    mut layout: ResMut<ProjectLayout>,
    mut editor: ResMut<EditorState>,
    mut ui: ResMut<EditorUiState>,
) {
    match layout.ensure() {
        Ok(()) => {
            editor.project_name = layout.manifest.name.clone();
            ui.info(format!(
                "Project '{}' ready at {}",
                layout.manifest.name,
                layout.root.display()
            ));
        }
        Err(error) => {
            ui.error(format!("Project initialization failed: {error}"));
        }
    }
}
