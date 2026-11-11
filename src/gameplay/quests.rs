//! Quests, dialogues and achievements as data (`project/quests.ron`), driven
//! by the game events that Weave also listens to. Weave starts them with the
//! blocks `Start quest` and `Show dialogue`, and hears `Quest completed` and
//! `Achievement unlocked`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::pie_manager::PieState;
use crate::core::project_io::{load_ron, save_ron, ProjectLayout};
use crate::gameplay::inventory::Inventory;
use crate::scripting::context::{emit_game_event, GameEvent, GameEventRecord};

const STORY_FILE: &str = "quests.ron";
const PROGRESS_FILE: &str = "quest_progress.ron";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Objective {
    pub label: String,
    pub event: GameEvent,
    /// Empty = any.
    #[serde(default)]
    pub key: String,
    pub count: u32,
}

impl Default for Objective {
    fn default() -> Self {
        Self { label: "Objective".to_string(), event: GameEvent::MobKilled, key: String::new(), count: 1 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Quest {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Starts with the game.
    #[serde(default)]
    pub auto_start: bool,
    pub objectives: Vec<Objective>,
    #[serde(default)]
    pub reward: Vec<(String, u32)>,
    /// Quest that follows.
    #[serde(default)]
    pub next: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DialogueChoice {
    pub text: String,
    /// Line it leads to (none = the end).
    #[serde(default)]
    pub goto: Option<usize>,
    /// Sent to Weave as a Button pressed event (empty = none).
    #[serde(default)]
    pub event: String,
    /// Quest it starts.
    #[serde(default)]
    pub quest: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DialogueLine {
    pub speaker: String,
    pub text: String,
    #[serde(default)]
    pub choices: Vec<DialogueChoice>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Dialogue {
    pub id: String,
    pub lines: Vec<DialogueLine>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Achievement {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub event: GameEvent,
    #[serde(default)]
    pub key: String,
    pub count: u32,
}

impl Default for Achievement {
    fn default() -> Self {
        Self { id: "achievement".to_string(), title: "Achievement".to_string(), description: String::new(), event: GameEvent::BlockBroken, key: String::new(), count: 1 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StoryData {
    #[serde(default)]
    pub quests: Vec<Quest>,
    #[serde(default)]
    pub dialogues: Vec<Dialogue>,
    #[serde(default)]
    pub achievements: Vec<Achievement>,
}

impl StoryData {
    /// The content of the example project.
    pub fn starter() -> Self {
        let line = |speaker: &str, text: &str, choices| DialogueLine { speaker: speaker.to_string(), text: text.to_string(), choices };
        Self {
            quests: vec![
                Quest {
                    id: "first_steps".to_string(),
                    title: "First steps".to_string(),
                    description: "Learn how the world works.".to_string(),
                    auto_start: true,
                    objectives: vec![
                        Objective { label: "Break blocks".to_string(), event: GameEvent::BlockBroken, key: String::new(), count: 5 },
                        Objective { label: "Place a block".to_string(), event: GameEvent::BlockPlaced, key: String::new(), count: 1 },
                    ],
                    reward: Vec::new(),
                    next: Some("hunter".to_string()),
                },
                Quest {
                    id: "hunter".to_string(),
                    title: "The hunter".to_string(),
                    description: "Prove yourself against the creatures.".to_string(),
                    auto_start: false,
                    objectives: vec![Objective { label: "Defeat a creature".to_string(), event: GameEvent::MobKilled, key: String::new(), count: 1 }],
                    reward: Vec::new(),
                    next: None,
                },
            ],
            dialogues: vec![Dialogue {
                id: "welcome".to_string(),
                lines: vec![
                    line("Guide", "Welcome. This world was made entirely in the editor.", Vec::new()),
                    line(
                        "Guide",
                        "Are you ready to begin?",
                        vec![
                            DialogueChoice { text: "Yes".to_string(), goto: None, event: "guide_ready".to_string(), quest: None },
                            DialogueChoice { text: "Tell me again".to_string(), goto: Some(0), ..DialogueChoice::default() },
                        ],
                    ),
                ],
            }],
            achievements: vec![Achievement {
                id: "miner".to_string(),
                title: "Miner".to_string(),
                description: "Break 50 blocks.".to_string(),
                event: GameEvent::BlockBroken,
                key: String::new(),
                count: 50,
            }],
        }
    }

    pub fn path(project: &ProjectLayout) -> std::path::PathBuf {
        project.root.join(STORY_FILE)
    }

    pub fn save(&self, project: &ProjectLayout) -> Result<(), String> {
        save_ron(&Self::path(project), self)
    }
}

static DATA: LazyLock<RwLock<Arc<StoryData>>> = LazyLock::new(|| RwLock::new(Arc::new(StoryData::default())));
static REVISION: AtomicU64 = AtomicU64::new(1);

pub fn story() -> Arc<StoryData> {
    DATA.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

pub fn set_story(data: StoryData) {
    *DATA.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(data);
    REVISION.fetch_add(1, Ordering::Relaxed);
}

pub fn story_revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ActiveQuest {
    pub id: String,
    pub counts: Vec<u32>,
}

/// What the player did, kept between sessions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub active: Vec<ActiveQuest>,
    pub done: Vec<String>,
    pub counters: Vec<(String, u32)>,
    pub unlocked: Vec<String>,
}

#[derive(Resource, Debug, Default)]
pub struct QuestLog {
    pub progress: Progress,
    /// Open dialogue and its line.
    pub dialogue: Option<(String, usize)>,
    /// Message and time left.
    pub toasts: Vec<(String, f32)>,
    rewards: Vec<(String, u32)>,
    dirty: bool,
}

fn matches(event: GameEvent, key: &str, record: &GameEventRecord) -> bool {
    event == record.event && (key.is_empty() || key == record.key)
}

impl QuestLog {
    pub fn start(&mut self, id: &str, data: &StoryData) -> bool {
        let known = self.progress.done.iter().chain(self.progress.active.iter().map(|active| &active.id)).any(|name| name == id);
        let Some(quest) = data.quests.iter().find(|quest| quest.id == id).filter(|_| !known) else {
            return false;
        };
        self.progress.active.push(ActiveQuest { id: id.to_string(), counts: vec![0; quest.objectives.len()] });
        self.toasts.push((format!("New quest: {}", quest.title), 4.0));
        self.dirty = true;
        true
    }

    pub fn on_event(&mut self, record: &GameEventRecord, data: &StoryData) {
        let mut finished = Vec::new();
        for active in &mut self.progress.active {
            let Some(quest) = data.quests.iter().find(|quest| quest.id == active.id) else {
                continue;
            };
            active.counts.resize(quest.objectives.len(), 0);
            for (objective, count) in quest.objectives.iter().zip(&mut active.counts) {
                if *count < objective.count && matches(objective.event, &objective.key, record) {
                    *count += 1;
                    self.dirty = true;
                }
            }
            if quest.objectives.iter().zip(&active.counts).all(|(objective, count)| *count >= objective.count) {
                finished.push(quest);
            }
        }
        for quest in finished {
            self.progress.active.retain(|active| active.id != quest.id);
            self.progress.done.push(quest.id.clone());
            self.rewards.extend(quest.reward.iter().cloned());
            self.toasts.push((format!("Quest completed: {}", quest.title), 4.0));
            emit_game_event(GameEvent::QuestDone, &quest.id, 0.0, None);
            if let Some(next) = &quest.next {
                self.start(next, data);
            }
        }
        for achievement in &data.achievements {
            if self.progress.unlocked.contains(&achievement.id) || !matches(achievement.event, &achievement.key, record) {
                continue;
            }
            let position = self.progress.counters.iter().position(|(id, _)| *id == achievement.id).unwrap_or_else(|| {
                self.progress.counters.push((achievement.id.clone(), 0));
                self.progress.counters.len() - 1
            });
            self.progress.counters[position].1 += 1;
            self.dirty = true;
            if self.progress.counters[position].1 >= achievement.count {
                self.progress.unlocked.push(achievement.id.clone());
                self.toasts.push((format!("Achievement: {}", achievement.title), 5.0));
                emit_game_event(GameEvent::AchievementUnlocked, &achievement.id, 0.0, None);
            }
        }
    }

    /// Follow a dialogue: `choice` is the button pressed, or none for Next.
    pub fn advance(&mut self, choice: Option<usize>, data: &StoryData) {
        let Some((id, index)) = self.dialogue.clone() else { return };
        let dialogue = data.dialogues.iter().find(|dialogue| dialogue.id == id);
        let Some(line) = dialogue.and_then(|dialogue| dialogue.lines.get(index)) else {
            self.dialogue = None;
            return;
        };
        let next = match choice.and_then(|index| line.choices.get(index)) {
            Some(picked) => {
                if !picked.event.is_empty() {
                    emit_game_event(GameEvent::UiAction, &picked.event, 0.0, None);
                }
                if let Some(quest) = &picked.quest {
                    self.start(quest, data);
                }
                picked.goto
            }
            None => Some(index + 1),
        };
        let length = dialogue.map_or(0, |dialogue| dialogue.lines.len());
        self.dialogue = next.filter(|next| *next < length).map(|next| (id, next));
    }
}

pub struct QuestPlugin;

impl Plugin for QuestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuestLog>()
            .add_systems(Startup, load_story.after(crate::core::project_io::initialize_project_system))
            .add_systems(Update, run_quests);
    }
}

fn load_story(project: Res<ProjectLayout>, mut ui: ResMut<crate::core::app_state::EditorUiState>) {
    let path = StoryData::path(&project);
    if !path.is_file() {
        return;
    }
    match load_ron::<StoryData>(&path) {
        Ok(loaded) => set_story(loaded),
        Err(error) => ui.warn(format!("Quests: {error}")),
    }
}

fn run_quests(
    time: Res<Time>,
    pie: Res<PieState>,
    project: Res<ProjectLayout>,
    mut log: ResMut<QuestLog>,
    mut inventory: ResMut<Inventory>,
    mut ui: ResMut<crate::core::app_state::EditorUiState>,
) {
    if pie.started_this_frame {
        *log = QuestLog::default();
        log.progress = load_ron(&project.root.join(PROGRESS_FILE)).unwrap_or_default();
        let data = story();
        for quest in data.quests.iter().filter(|quest| quest.auto_start) {
            log.start(&quest.id, &data);
        }
    }
    if !pie.is_playing() {
        return;
    }
    for (key, amount) in std::mem::take(&mut log.rewards) {
        inventory.add(&key, amount);
    }
    let delta = time.delta_secs();
    log.toasts.iter_mut().for_each(|toast| toast.1 -= delta);
    log.toasts.retain(|toast| toast.1 > 0.0);
    if log.dirty {
        log.dirty = false;
        if let Err(error) = save_ron(&project.root.join(PROGRESS_FILE), &log.progress) {
            ui.warn(error);
        }
    }
}

/// Forget what the player did in this project.
pub fn reset_progress(project: &ProjectLayout) {
    let _ = std::fs::remove_file(project.root.join(PROGRESS_FILE));
}

/// Quest tracker, messages and the open dialogue.
pub fn draw(ctx: &egui::Context, log: &mut QuestLog, tracker: bool) {
    use crate::editor::theme;
    use egui::{pos2, Align2, Color32, FontId};
    let data = story();
    let screen = ctx.viewport_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("quest_hud")));
    if tracker {
        let mut y = screen.top() + 170.0;
        let x = screen.right() - 18.0;
        for active in &log.progress.active {
            let Some(quest) = data.quests.iter().find(|quest| quest.id == active.id) else { continue };
            painter.text(pos2(x, y), Align2::RIGHT_TOP, &quest.title, FontId::proportional(14.0), Color32::WHITE);
            y += 18.0;
            for (objective, count) in quest.objectives.iter().zip(&active.counts) {
                painter.text(pos2(x, y), Align2::RIGHT_TOP, format!("{} {}/{}", objective.label, count, objective.count), FontId::proportional(12.0), theme::TEXT_DIM);
                y += 15.0;
            }
            y += 8.0;
        }
    }
    for (index, (text, _)) in log.toasts.iter().enumerate() {
        theme::hud_pill(&painter, pos2(screen.center().x, screen.top() + 70.0 + index as f32 * 30.0), Align2::CENTER_CENTER, text, 15.0, theme::ACCENT);
    }
    let Some((id, index)) = log.dialogue.clone() else { return };
    let Some(line) = data.dialogues.iter().find(|dialogue| dialogue.id == id).and_then(|dialogue| dialogue.lines.get(index)) else {
        log.dialogue = None;
        return;
    };
    let mut pressed = None;
    egui::Area::new(egui::Id::new("quest_dialogue")).order(egui::Order::Foreground).anchor(Align2::CENTER_BOTTOM, egui::vec2(0.0, -110.0)).show(ctx, |ui| {
        theme::window_frame().show(ui, |ui| {
            ui.set_width(560.0);
            ui.label(egui::RichText::new(&line.speaker).size(13.0).color(theme::ACCENT));
            ui.label(egui::RichText::new(&line.text).size(15.0).color(Color32::WHITE));
            ui.add_space(6.0);
            if line.choices.is_empty() {
                if theme::game_button(ui, "Next", true, 180.0).clicked() {
                    pressed = Some(None);
                }
            } else {
                for (index, choice) in line.choices.iter().enumerate() {
                    if theme::game_button(ui, &choice.text, index == 0, 520.0).clicked() {
                        pressed = Some(Some(index));
                    }
                }
            }
        });
    });
    if let Some(choice) = pressed {
        log.advance(choice, &data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(event: GameEvent, key: &str) -> GameEventRecord {
        GameEventRecord { event, key: key.to_string(), value: 0.0, position: None }
    }

    #[test]
    fn a_quest_counts_its_events_then_starts_the_next() {
        let data = StoryData::starter();
        let mut log = QuestLog::default();
        assert!(log.start("first_steps", &data));
        assert!(!log.start("first_steps", &data));
        for _ in 0..4 {
            log.on_event(&record(GameEvent::BlockBroken, "stone"), &data);
        }
        assert_eq!(log.progress.active[0].counts, vec![4, 0]);
        log.on_event(&record(GameEvent::BlockBroken, "stone"), &data);
        log.on_event(&record(GameEvent::BlockPlaced, "stone"), &data);
        assert_eq!(log.progress.done, vec!["first_steps".to_string()]);
        assert_eq!(log.progress.active[0].id, "hunter");
    }

    #[test]
    fn objectives_with_a_key_only_count_that_key() {
        let quest = Quest {
            id: "q".to_string(),
            objectives: vec![Objective { key: "spider".to_string(), ..Objective::default() }],
            reward: vec![("stone".to_string(), 2)],
            ..Quest::default()
        };
        let data = StoryData { quests: vec![quest], ..StoryData::default() };
        let mut log = QuestLog::default();
        log.start("q", &data);
        log.on_event(&record(GameEvent::MobKilled, "wolf"), &data);
        assert_eq!(log.progress.active.len(), 1);
        log.on_event(&record(GameEvent::MobKilled, "spider"), &data);
        assert!(log.progress.active.is_empty());
        assert_eq!(log.rewards, vec![("stone".to_string(), 2)]);
    }

    #[test]
    fn achievements_unlock_once() {
        let achievement = Achievement { id: "a".to_string(), count: 2, ..Achievement::default() };
        let data = StoryData { achievements: vec![achievement], ..StoryData::default() };
        let mut log = QuestLog::default();
        for _ in 0..3 {
            log.on_event(&record(GameEvent::BlockBroken, ""), &data);
        }
        assert_eq!(log.progress.unlocked, vec!["a".to_string()]);
        assert_eq!(log.progress.counters[0].1, 2);
    }

    #[test]
    fn dialogue_follows_choices_to_its_end() {
        let data = StoryData::starter();
        let mut log = QuestLog { dialogue: Some(("welcome".to_string(), 0)), ..QuestLog::default() };
        log.advance(None, &data);
        assert_eq!(log.dialogue, Some(("welcome".to_string(), 1)));
        log.advance(Some(1), &data);
        assert_eq!(log.dialogue, Some(("welcome".to_string(), 0)));
        log.advance(None, &data);
        log.advance(Some(0), &data);
        assert_eq!(log.dialogue, None);
    }

    #[test]
    fn the_story_round_trips() {
        let data = StoryData::starter();
        let text = ron::ser::to_string_pretty(&data, ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(data, ron::from_str::<StoryData>(&text).unwrap());
    }
}
