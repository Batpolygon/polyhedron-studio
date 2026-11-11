//! Integrated help: how each part of the studio is used.

use egui::RichText;

use crate::editor::theme;

const TOPICS: [(&str, &str); 12] = [
    ("Start", "Everything is made in the studio: no file is ever imported.\n\nPlay tests the game (F7), Stop puts the world back as it was. Ctrl+Z undoes the last change anywhere, Ctrl+Y redoes it. Save all (Ctrl+S) writes every change of the studio: the button is orange while something is not saved.\n\nThe Project panel creates, opens and duplicates projects, checks them and exports the game. New projects can start from the Example."),
    ("World", "Paint the world with the Toolbox, and choose a tool and a block.\n\nThe world list is in the Project panel."),
    ("Forge", "The Forge makes blocks. Pick a part to change only its colour, add animated parts, and give the block a function.\n\nThe function Custom builds a machine from components: processor, gatherer, puller, zone, signal. Break and Place sound take any sound of the Ambience workspace."),
    ("Items", "An item has a kind and a list of effects, run on use and on attack: spend, heal, beam, zone, blast, dash, sparks, sound, broadcast. A weapon can have a combo with one step per hit."),
    ("Mobs", "Build the body in the viewport, animate it in the timeline, and choose a behaviour.\n\nThe behaviour Brain is a list of rules: when a condition holds, do an action. Templates give a Hunter, a Shooter, a Coward, a Guard and a Pack."),
    ("Biomes", "A biome has a climate, a relief, blocks and landforms. Landforms are noise rules: ravines, hills, pits, spikes, patches, craters.\n\nWORLD GENERATION holds the caves, the ores, the rock layers and the seed. The world map shows the result by biome, by height or by block. Structure rules can pick among weighted variants."),
    ("Weave", "Weave is the visual language: blocks run in order, started by events.\n\nON GAME EVENT reacts to anything in the game, for one key or for all. Variables starting with g. are shared by every program. Pause, Resume and Step debug a running program; Breakpoint stops it at a block. The Template menu gives ready-made programs."),
    ("Showcase", "The Showcase workspace designs the interface: the HUD and any screen. A screen can be an Overlay, a Modal (it frees the mouse), a Death screen or a Pause screen.\n\nWidgets read values by binding; buttons run actions, and can send events to Weave. The HUD settings switch off the built-in parts so a project can rebuild them."),
    ("Rules", "Every value of the game is in the Rules workspace, with a search field. Crafting lists recipes, resources, groups and conversions, and every name is chosen in a list."),
    ("Ambience", "Sounds are stacked oscillator layers: wave, pitch sweep, envelope, filter, vibrato. Songs are step grids: a click adds a note. A song plays in its biomes and at its time of day.\n\nParticles are played by the Weave block Spawn particle, with their name. Sky keys replace the sky of the biomes. Weather comes by biome, or from the Weave block Set weather."),
    ("Story", "Quests have objectives counted from game events, a reward and a next quest. Dialogues have lines and choices; a choice can go to another line, start a quest, or send an event to Weave. Achievements count an event.\n\nIn Weave: Start quest and Show dialogue, and the events Quest completed and Achievement unlocked."),
    ("Export", "Check the whole project first, from the Project panel. Export writes exports/<project>: an executable, the assets and the project, without the saved worlds. It starts on the main menu with no studio."),
];

pub fn show(ctx: &egui::Context, open: &mut bool) {
    let id = egui::Id::new("help_topic");
    let mut topic: usize = ctx.data_mut(|d| d.get_temp(id)).unwrap_or(0);
    egui::Window::new("Help").id(id.with("window")).open(open).default_size([620.0, 420.0]).collapsible(false).show(ctx, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(110.0);
                for (index, (title, _)) in TOPICS.iter().enumerate() {
                    if ui.selectable_label(topic == index, *title).clicked() {
                        topic = index;
                    }
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                let (title, body) = TOPICS[topic.min(TOPICS.len() - 1)];
                ui.label(RichText::new(title).size(15.0).color(theme::ACCENT));
                ui.add_space(6.0);
                egui::ScrollArea::vertical().show(ui, |ui| ui.label(RichText::new(body).size(11.5)));
            });
        });
    });
    ctx.data_mut(|d| d.insert_temp(id, topic));
}
