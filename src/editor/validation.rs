//! Global check of a project: every name a piece of data uses must exist.

use std::collections::HashSet;

use crate::audio::sound_events::sound_names;
use crate::audio::synth::SoundBank;
use crate::editor::pickers::{Catalog, Kind};
use crate::gameplay::atmosphere::Atmosphere;
use crate::gameplay::crafting::CraftingBook;
use crate::gameplay::quests::StoryData;
use crate::voxel::biomes::BiomeLibrary;
use crate::voxel::structures::StructureLibrary;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub area: String,
    pub message: String,
}

struct Report {
    issues: Vec<Issue>,
}

impl Report {
    fn add(&mut self, area: &str, message: String) {
        self.issues.push(Issue { area: area.to_string(), message });
    }

    fn need(&mut self, area: &str, known: &HashSet<&str>, name: &str, what: &str) {
        if !name.is_empty() && !known.contains(name) {
            self.add(area, format!("unknown {what} '{name}'"));
        }
    }
}

fn names<'a>(catalog: &'a Catalog, kinds: &[Kind]) -> HashSet<&'a str> {
    kinds.iter().flat_map(|kind| catalog.entries(*kind)).map(|(key, _)| key.as_str()).collect()
}

/// Check the data no library validates by itself.
pub fn validate(catalog: &Catalog, crafting: &CraftingBook, biomes: &BiomeLibrary, structures: &StructureLibrary, bank: &SoundBank, atmosphere: &Atmosphere, story: &StoryData) -> Vec<Issue> {
    let mut report = Report { issues: Vec::new() };
    let blocks = names(catalog, &[Kind::Block]);
    let things = names(catalog, &[Kind::Block, Kind::Item, Kind::Resource, Kind::Group]);
    let biome_names = names(catalog, &[Kind::Biome]);
    let structure_names: HashSet<&str> = structures.structures.iter().map(|structure| structure.name.as_str()).collect();

    for recipe in &crafting.recipes {
        let area = format!("Recipe {}", recipe.output);
        report.need(&area, &things, &recipe.output, "output");
        for (input, _) in &recipe.inputs {
            report.need(&area, &things, input, "ingredient");
        }
    }
    for conversion in &crafting.conversions {
        report.need("Conversion", &things, &conversion.from, "source");
        report.need("Conversion", &things, &conversion.to, "result");
    }

    let world = &biomes.settings;
    for name in [&world.blocks.water, &world.blocks.ice, &world.blocks.beach, &world.blocks.rock, &world.cave.fluid, &world.cave.floor, &world.cave.tuft] {
        report.need("World", &blocks, name, "block");
    }
    for ore in &world.ores {
        report.need("World ores", &blocks, &ore.block, "block");
    }
    for layer in &world.layers {
        report.need("Rock layers", &blocks, &layer.block, "block");
    }
    for biome in &biomes.biomes {
        let area = format!("Biome {}", biome.name);
        for name in [&biome.surface_block, &biome.subsurface_block, &biome.stone_block] {
            report.need(&area, &blocks, name, "block");
        }
        for ore in &biome.ores {
            report.need(&area, &blocks, &ore.block, "block");
        }
        for form in biome.forms() {
            for name in [&form.surface, &form.subsurface, &form.core].into_iter().flatten() {
                report.need(&area, &blocks, name, "block");
            }
        }
        for rule in &biome.structures {
            if rule.variants.is_empty() {
                report.need(&area, &structure_names, &rule.structure, "structure");
            }
            for variant in &rule.variants {
                report.need(&area, &structure_names, &variant.structure, "structure");
            }
            for ground in &rule.ground {
                report.need(&area, &blocks, ground, "block");
            }
        }
    }

    let sounds = sound_names();
    let sounds: HashSet<&str> = sounds.iter().map(String::as_str).chain(bank.sounds.iter().map(|sound| sound.name.as_str())).collect();
    for song in &bank.songs {
        let area = format!("Song {}", song.name);
        for track in &song.tracks {
            report.need(&area, &sounds, &track.sound, "sound");
        }
        for biome in &song.biomes {
            report.need(&area, &biome_names, biome, "biome");
        }
    }
    for weather in &atmosphere.weather {
        for biome in &weather.biomes {
            report.need(&format!("Weather {}", weather.name), &biome_names, biome, "biome");
        }
    }

    let quests: HashSet<&str> = story.quests.iter().map(|quest| quest.id.as_str()).collect();
    for quest in &story.quests {
        let area = format!("Quest {}", quest.title);
        if quest.objectives.is_empty() {
            report.add(&area, "no objective".to_string());
        }
        for (reward, _) in &quest.reward {
            report.need(&area, &things, reward, "reward");
        }
        if let Some(next) = &quest.next {
            report.need(&area, &quests, next, "quest");
        }
    }
    for dialogue in &story.dialogues {
        let area = format!("Dialogue {}", dialogue.id);
        for line in &dialogue.lines {
            for choice in &line.choices {
                if choice.goto.is_some_and(|line| line >= dialogue.lines.len()) {
                    report.add(&area, format!("'{}' leads to a line that does not exist", choice.text));
                }
                if let Some(quest) = &choice.quest {
                    report.need(&area, &quests, quest, "quest");
                }
            }
        }
    }
    report.issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gameplay::quests::Quest;
    use crate::voxel::blocks::BlockRegistry;

    fn catalog() -> Catalog {
        let registry = BlockRegistry::builtin();
        Catalog { blocks: registry.blocks().map(|(_, def)| (def.id.clone(), def.label.clone())).collect(), ..Catalog::default() }
    }

    #[test]
    fn the_shipped_project_has_no_broken_reference() {
        use crate::core::project_io::ProjectLayout;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("project");
        let project = ProjectLayout { root: root.clone(), ..ProjectLayout::default() };
        let mut registry = BlockRegistry::builtin();
        registry.load_from_dir(&root.join("blocks")).unwrap();
        let mut items = crate::gameplay::items::ItemLibrary::default();
        items.load_from_project(&project).unwrap();
        let crafting: CraftingBook = crate::core::project_io::load_ron(&root.join("crafting.ron")).unwrap();
        let mut biomes = BiomeLibrary::default();
        biomes.load_from_project(&project).unwrap();
        let mut structures = StructureLibrary::default();
        structures.load_from_project(&project).unwrap();
        let mut biome_entries = vec![("*".to_string(), String::new())];
        biome_entries.extend(biomes.biomes.iter().map(|biome| (biome.name.clone(), String::new())));
        let catalog = Catalog {
            blocks: registry.blocks().map(|(_, def)| (def.id.clone(), def.label.clone())).collect(),
            items: items.items.iter().map(|item| (item.id.clone(), item.label.clone())).collect(),
            resources: crafting.resources.iter().map(|r| (r.id.clone(), r.label.clone())).collect(),
            groups: crafting.groups.iter().map(|g| (g.id.clone(), g.label.clone())).collect(),
            biomes: biome_entries,
            mobs: Vec::new(),
        };
        let atmosphere = Atmosphere::starter();
        let issues = validate(&catalog, &crafting, &biomes, &structures, &SoundBank::default(), &atmosphere, &StoryData::starter());
        assert_eq!(issues, Vec::new());
    }

    #[test]
    fn a_missing_name_is_reported_where_it_is_used() {
        let story = StoryData {
            quests: vec![Quest { id: "q".to_string(), title: "Q".to_string(), next: Some("nowhere".to_string()), ..Quest::default() }],
            ..StoryData::default()
        };
        let empty = CraftingBook { recipes: Vec::new(), conversions: Vec::new(), ..CraftingBook::builtin() };
        let issues = validate(&catalog(), &empty, &BiomeLibrary::default(), &StructureLibrary::default(), &SoundBank::default(), &Atmosphere::default(), &story);
        assert!(issues.iter().any(|issue| issue.message == "no objective"));
        assert!(issues.iter().any(|issue| issue.message == "unknown quest 'nowhere'"));
    }

    #[test]
    fn the_builtin_world_settings_are_clean() {
        let empty = CraftingBook { recipes: Vec::new(), conversions: Vec::new(), ..CraftingBook::builtin() };
        let issues = validate(&catalog(), &empty, &BiomeLibrary::default(), &StructureLibrary::default(), &SoundBank::default(), &Atmosphere::default(), &StoryData::default());
        assert_eq!(issues, Vec::new());
    }
}
