//! In-game chat.
//!
//! `T` (or `Enter`) opens the chat, `/` opens it with a command prefix.
//! Game messages (deaths, pickups, script logs) are mirrored in the feed.
//! Commands (`/helpcommand`) are run by [`crate::gameplay::commands`].
//!
//! `Tab` completes the word under the cursor (commands and their arguments:
//! blocks, entities, resources, stats...); pressing it again cycles the
//! candidates.

use bevy::prelude::*;

use crate::core::app_state::{EditorUiState, LogLevel};
use crate::core::pie_manager::{PieSet, PieState};
use crate::gameplay::commands::{Arg, CommandQueue, SPECS};
use crate::gameplay::items::ItemLibrary;
use crate::gameplay::mobs::library::MobLibrary;
use crate::physics::player_controller::PlayerControllerSet;
use crate::voxel::blocks::BlockRegistry;

/// Seconds a message stays visible when the chat is closed.
pub const MESSAGE_LIFETIME: f32 = 9.0;
const MAX_LINES: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatKind {
    Player,
    System,
    Success,
    Error,
}

impl ChatKind {
    pub fn color(self) -> egui::Color32 {
        match self {
            Self::Player => egui::Color32::from_rgb(235, 240, 245),
            Self::System => egui::Color32::from_rgb(160, 176, 192),
            Self::Success => egui::Color32::from_rgb(110, 220, 170),
            Self::Error => egui::Color32::from_rgb(240, 110, 110),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChatLine {
    pub text: String,
    pub kind: ChatKind,
    /// Real time the line was posted.
    pub time: f64,
}

#[derive(Resource, Debug, Default)]
pub struct Chat {
    pub open: bool,
    pub input: String,
    pub lines: Vec<ChatLine>,
    /// Sent entries, newest last (Up / Down recall them).
    pub history: Vec<String>,
    pub history_cursor: Option<usize>,
    /// Text the input starts with ("" or "/"), re-applied on the opening
    /// frame so the opening key never leaks into the field.
    pub prefill: String,
    pub just_opened: bool,
    /// Entries submitted by the UI, executed by [`run_chat`].
    pub submitted: Vec<String>,
    /// Log lines already mirrored.
    seen_logs: u64,
    pub now: f64,
    /// Names offered by `Tab` (refreshed when the chat opens).
    pub sources: CompletionSources,
    /// Candidates being cycled by repeated `Tab` presses.
    pub cycle: Option<CompletionCycle>,
    sources_ready: bool,
}

/// Every name the completion knows besides the command tree.
#[derive(Clone, Debug, Default)]
pub struct CompletionSources {
    pub blocks: Vec<String>,
    pub items: Vec<String>,
    pub mobs: Vec<String>,
}

/// State of a `Tab` cycle: the input it started from and the candidates.
#[derive(Clone, Debug, Default)]
pub struct CompletionCycle {
    /// Input before the word under completion.
    pub prefix: String,
    pub candidates: Vec<String>,
    pub index: usize,
    /// Input produced by the last completion (a new key press resets).
    pub applied: String,
}

/// Candidates for the last word of `input` (each one is the whole word).
pub fn completions(input: &str, sources: &CompletionSources) -> Vec<String> {
    let Some(command) = input.strip_prefix('/') else {
        return Vec::new();
    };
    let words: Vec<&str> = command.split(' ').collect();
    let current = words.last().copied().unwrap_or("");
    let index = words.len() - 1;
    let lower = |s: &str| s.to_ascii_lowercase();
    let owned = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let pool: Vec<String> = if index == 0 {
        SPECS.iter().map(|spec| spec.name.to_string()).collect()
    } else {
        let head = lower(words[0]);
        // `/referenceto <entity> <command...>`: complete the nested command.
        if head == "referenceto" && index >= 2 {
            let nested = format!("/{}", words[2..].join(" "));
            return completions(&nested, sources);
        }
        let arg = crate::gameplay::commands::spec(&head).and_then(|spec| spec.args.get(index - 1).copied());
        let mobs = || sources.mobs.iter().map(|mob| mob.replace(' ', "_")).collect::<Vec<_>>();
        match arg {
            Some(Arg::Coord) => owned(&["~"]),
            Some(Arg::Block) => sources.blocks.clone(),
            Some(Arg::Entity) => {
                let mut list = owned(&["@p", "@s", "@n"]);
                list.extend(mobs());
                list
            }
            Some(Arg::Mob) => mobs(),
            Some(Arg::Resource) => {
                let mut list = crate::gameplay::crafting::resource_ids();
                if head == "give" {
                    list.extend(sources.blocks.iter().cloned());
                    list.extend(sources.items.iter().cloned());
                    list.extend(sources.mobs.iter().map(|mob| format!("egg:{}", mob.replace(' ', "_"))));
                }
                list
            }
            Some(Arg::Stat) => owned(&["health", "maxhealth", "stamina", "maxstamina", "speed", "damage", "armor", "hostile"]),
            Some(Arg::Time) => owned(&["day", "noon", "sunset", "night", "midnight", "sunrise"]),
            Some(Arg::Kind) => owned(&["energy", "resource", "signal"]),
            Some(Arg::Mode) => owned(&["creative", "survival", "spectator"]),
            Some(Arg::Value) => owned(&["true", "false"]),
            Some(Arg::Command) => SPECS.iter().map(|spec| spec.name.to_string()).collect(),
            Some(Arg::Number) if head == "sendtoomnicorenetwork" => owned(&["1", "all"]),
            Some(Arg::Number) if head == "give" => owned(&["1", "16", "64"]),
            _ => Vec::new(),
        }
    };
    let needle = lower(current);
    let mut matches: Vec<String> = pool.into_iter().filter(|candidate| lower(candidate).starts_with(&needle)).collect();
    matches.sort_by_key(|candidate| (candidate.len(), lower(candidate)));
    matches.dedup();
    matches
}

impl Chat {
    pub fn open_with(&mut self, prefill: &str) {
        self.open = true;
        self.prefill = prefill.to_string();
        self.input = prefill.to_string();
        self.just_opened = true;
        self.history_cursor = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.input.clear();
        self.history_cursor = None;
        self.cycle = None;
        self.sources_ready = false;
    }

    /// `Tab`: complete the last word, or cycle to the next candidate.
    pub fn complete(&mut self) {
        let continuing = self.cycle.as_ref().is_some_and(|cycle| cycle.applied == self.input && cycle.candidates.len() > 1);
        if continuing {
            let cycle = self.cycle.as_mut().expect("checked above");
            cycle.index = (cycle.index + 1) % cycle.candidates.len();
            let text = format!("{}{}", cycle.prefix, cycle.candidates[cycle.index]);
            cycle.applied = text.clone();
            self.input = text;
            return;
        }
        let candidates = completions(&self.input, &self.sources);
        if candidates.is_empty() {
            self.cycle = None;
            return;
        }
        let prefix = match self.input.rfind(' ') {
            Some(space) => self.input[..=space].to_string(),
            None => "/".to_string(),
        };
        let single = candidates.len() == 1;
        let mut text = format!("{prefix}{}", candidates[0]);
        if single {
            text.push(' ');
        }
        self.input = text.clone();
        self.cycle = Some(CompletionCycle { prefix, candidates, index: 0, applied: text });
    }

    pub fn post(&mut self, kind: ChatKind, text: impl Into<String>) {
        self.lines.push(ChatLine { text: text.into(), kind, time: self.now });
        if self.lines.len() > MAX_LINES {
            let overflow = self.lines.len() - MAX_LINES;
            self.lines.drain(0..overflow);
        }
    }

    /// Recall an older (`-1`) or newer (`+1`) history entry.
    pub fn recall(&mut self, step: i32) {
        if self.history.is_empty() {
            return;
        }
        let last = self.history.len() - 1;
        let next = match (self.history_cursor, step < 0) {
            (None, true) => Some(last),
            (None, false) => None,
            (Some(index), true) => Some(index.saturating_sub(1)),
            (Some(index), false) => (index < last).then_some(index + 1),
        };
        self.history_cursor = next;
        self.input = next.map(|index| self.history[index].clone()).unwrap_or_default();
    }
}

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Chat>()
            .add_systems(Update, run_chat.after(PieSet::Transition).after(PlayerControllerSet));
    }
}

#[allow(clippy::too_many_arguments)]
fn run_chat(
    time: Res<Time<Real>>,
    pie: Res<PieState>,
    mut chat: ResMut<Chat>,
    ui: Res<EditorUiState>,
    mut queue: ResMut<CommandQueue>,
    registry: Res<BlockRegistry>,
    items: Res<ItemLibrary>,
    mobs: Res<MobLibrary>,
) {
    chat.now = time.elapsed_secs_f64();
    if pie.started_this_frame {
        chat.lines.clear();
        chat.close();
        chat.seen_logs = ui.log_total.saturating_sub(1);
    }
    if !pie.is_playing() {
        chat.open = false;
        chat.seen_logs = ui.log_total;
        return;
    }

    // Mirror new game messages.
    let fresh = ui.log_total.saturating_sub(chat.seen_logs).min(ui.console.len() as u64) as usize;
    if fresh > 0 {
        let start = ui.console.len() - fresh;
        for line in ui.console[start..].to_vec() {
            let kind = match line.level {
                LogLevel::Error => ChatKind::Error,
                LogLevel::Warn => ChatKind::Error,
                LogLevel::Action => ChatKind::Success,
                _ => ChatKind::System,
            };
            chat.post(kind, line.message);
        }
    }
    chat.seen_logs = ui.log_total;

    // Names offered by Tab.
    if chat.open && !chat.sources_ready {
        chat.sources_ready = true;
        chat.sources = CompletionSources {
            blocks: registry.blocks().filter(|(_, def)| def.variant_of.is_none() && !def.hidden).map(|(_, def)| def.id.clone()).collect(),
            items: items.items.iter().map(|item| item.id.clone()).collect(),
            mobs: mobs.mob_rigs().map(|rig| rig.name.clone()).collect(),
        };
    }

    for entry in std::mem::take(&mut chat.submitted) {
        let entry = entry.trim().to_string();
        if entry.is_empty() {
            continue;
        }
        if chat.history.last() != Some(&entry) {
            chat.history.push(entry.clone());
        }
        if let Some(command) = entry.strip_prefix('/') {
            chat.post(ChatKind::Player, format!("> /{command}"));
            queue.push_player(command);
        } else {
            chat.post(ChatKind::Player, format!("<Player> {entry}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_completes_commands_arguments_and_cycles() {
        let mut chat = Chat::default();
        chat.sources = CompletionSources {
            blocks: vec!["stone".into(), "sand".into()],
            items: vec![],
            mobs: vec!["Gim-Gim".into(), "Mnemo Watcher".into()],
        };
        chat.input = "/setp".into();
        chat.complete();
        assert_eq!(chat.input, "/setpolyhedron ");
        chat.input = "/gamemode sp".into();
        chat.complete();
        assert_eq!(chat.input, "/gamemode spectator ");
        chat.input = "/settime n".into();
        chat.complete();
        assert_eq!(chat.input, "/settime noon");
        chat.complete();
        assert_eq!(chat.input, "/settime night");
        assert!(completions("/spawn mne", &chat.sources).contains(&"Mnemo_Watcher".to_string()));
        assert!(completions("/kill @", &chat.sources).contains(&"@p".to_string()));
        assert!(completions("/give egg:", &chat.sources).contains(&"egg:Gim-Gim".to_string()));
        assert!(completions("/referenceto @n sett", &chat.sources).contains(&"settime".to_string()));
        assert!(completions("hello", &chat.sources).is_empty());
    }

    #[test]
    fn history_recall_walks_back_and_forth() {
        let mut chat = Chat::default();
        chat.history = vec!["/a".into(), "/b".into()];
        chat.recall(-1);
        assert_eq!(chat.input, "/b");
        chat.recall(-1);
        assert_eq!(chat.input, "/a");
        chat.recall(1);
        assert_eq!(chat.input, "/b");
        chat.recall(1);
        assert_eq!(chat.input, "");
    }
}
