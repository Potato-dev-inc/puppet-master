//! The chat view for an agent that writes no session file: the items are read from the pane's own text.
//!
//! `parse` cuts the text into turns with the profile's tables; `ScreenChat` (here) keeps the turns read so far,
//! follows them as the screen changes and says what the viewer must be sent. Item ids are stable
//! (`screen:<turn>:<kind>[:<k>]`), so a reply that grows is sent again under the same id and replaces its old
//! copy; when an item disappears or moves the viewer is sent everything again (`Change::Reset`).
//! Chat text is never logged here.

mod follow;
mod parse;
mod registry;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod watch_tests;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::model::{ChatItem, TurnOutcome};
use super::watch::Change;
use crate::agent_runs::completion::normalize_echo;
use parse::{parse_turns, read_rows, rows_since, Parsed};

pub(super) use follow::ScreenFollower;
pub use registry::read_pane;

/// What the screen reader gets from a pane.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenRead {
    /// The pane's pane status as text (`running`, `waiting_input`, `idle`, `error`).
    pub status: String,
    /// How much of the pane's output the text includes, when known; passed back as `known` on the next read.
    pub position: Option<u64>,
    /// The pane's text with its scrollback. `None`: the output position is the one the caller already read, so
    /// there is nothing new to read.
    pub text: Option<String>,
}

/// Idle polls in a row with the same text before a turn counts as ended.
const ENDED_AFTER_IDLE_POLLS: u8 = 2;
/// Longest reply kept, in characters.
const MAX_REPLY_CHARS: usize = 20_000;
/// Rows from the end of the text the model footer is looked for in.
const MODEL_TAIL_ROWS: usize = 80;

pub struct ScreenChat {
    agent_type: String,
    /// The turns read so far, oldest first. Turns that scrolled out of sight stay.
    turns: Vec<Parsed>,
    /// What the viewer has been sent.
    shown: Vec<ChatItem>,
    model: Option<String>,
    last_text: Option<u64>,
    idle_polls: u8,
    /// A profile with no readable echo: turns are cut by the pane's status. The text before the running turn.
    baseline: String,
    /// The text as of the last idle poll (the next turn's baseline).
    quiet_text: String,
    running: bool,
}

impl ScreenChat {
    pub fn new(agent_type: &str) -> Self {
        ScreenChat {
            agent_type: agent_type.to_string(),
            turns: Vec::new(),
            shown: Vec::new(),
            model: None,
            last_text: None,
            idle_polls: 0,
            baseline: String::new(),
            quiet_text: String::new(),
            running: false,
        }
    }

    /// Takes one look: `text` is the pane's text when it may have changed (`None`: it did not), `status` the
    /// pane's status. Returns what the viewer must be sent to match what the screen shows now.
    pub fn observe(&mut self, text: Option<&str>, status: &str) -> Change {
        let idle = status == "idle";
        let mut changed = false;
        match text {
            Some(text) if self.last_text != Some(hash_of(text)) => {
                self.last_text = Some(hash_of(text));
                let before = self.turns.clone();
                self.read(text, idle);
                changed = before != self.turns;
            }
            _ if idle => self.running = false,
            _ => {}
        }
        self.idle_polls = match (idle, changed) {
            (false, _) => 0,
            (true, true) => 1,
            (true, false) => self.idle_polls.saturating_add(1),
        };
        let next = self.items(!idle);
        diff(&mut self.shown, next)
    }

    /// Reads `text` into the turns.
    fn read(&mut self, text: &str, idle: bool) {
        let profile = crate::tui::profile_for(&self.agent_type);
        if profile.startup_gate(text).is_some() {
            return;
        }
        let tail: Vec<&str> = text.lines().rev().take(MODEL_TAIL_ROWS).collect();
        let tail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
        if let Some(model) = profile.detect_footer_model(&tail) {
            self.model = Some(model);
        }
        if profile.reads_user_messages() {
            let parsed = parse_turns(text, &*profile, &self.agent_type);
            self.merge(parsed, text);
        } else {
            self.read_by_status(text, idle, &*profile);
        }
    }

    /// A profile with no echo to find: a turn is what appeared on screen while the pane was busy.
    fn read_by_status(&mut self, text: &str, idle: bool, profile: &dyn crate::tui::TuiProfile) {
        if !idle && !self.running {
            self.running = true;
            self.baseline = std::mem::take(&mut self.quiet_text);
            self.turns.push(Parsed::default());
        }
        if self.running {
            let rows = rows_since(text, &self.baseline);
            let turn = read_rows(&rows, None, &self.agent_type, profile);
            if let Some(last) = self.turns.last_mut() {
                *last = turn;
            }
        }
        if idle {
            self.running = false;
        }
        self.quiet_text = text.to_string();
    }

    /// Puts the turns just read on `text` into the turns kept.
    fn merge(&mut self, mut parsed: Vec<Parsed>, text: &str) {
        if parsed.is_empty() {
            // Nothing on screen is a message or an answer: a cleared screen, when none of what was read is there.
            if !self.turns.is_empty() && !self.still_on(text) {
                self.turns.clear();
            }
            return;
        }
        // Rows above the first message on screen are the end of a turn whose message scrolled off the top.
        if parsed[0].user.is_none() && !self.turns.is_empty() && align(&self.turns, &parsed).is_none() {
            let head = parsed.remove(0);
            if let Some(last) = self.turns.last_mut() {
                grow(last, head);
            }
            if parsed.is_empty() {
                return;
            }
        }
        let count = parsed.len();
        match align(&self.turns, &parsed) {
            Some(first) => {
                for (offset, turn) in parsed.into_iter().enumerate() {
                    match self.turns.get_mut(first + offset) {
                        Some(known) if turn.user.is_none() => grow(known, turn),
                        Some(known) => {
                            let user = longer(known.user.take(), turn.user.clone());
                            *known = Parsed { user, ..turn };
                        }
                        None => self.turns.push(turn),
                    }
                }
                // A message that was typed and then deleted leaves no turn behind.
                while self.turns.len() > first + count && self.turns.last().is_some_and(|turn| !turn.has_body()) {
                    self.turns.pop();
                }
            }
            // Another conversation: the screen was cleared, or a new session began.
            None => self.turns = parsed,
        }
    }

    /// Whether anything read before is still in `text`.
    fn still_on(&self, text: &str) -> bool {
        let on_screen = normalize_echo(text);
        self.turns.iter().any(|turn| {
            let first_reply_row = turn.reply.as_deref().and_then(|reply| reply.lines().next()).map(normalize_echo);
            [turn.user.as_deref().map(normalize_echo), first_reply_row]
                .into_iter()
                .flatten()
                .chain(turn.tools.iter().map(|tool| normalize_echo(tool)))
                .any(|known| known.len() >= 8 && on_screen.contains(&known))
        })
    }

    /// The items of the turns read, for a pane that is busy (`busy`) or not.
    fn items(&self, busy: bool) -> Vec<ChatItem> {
        let mut items = Vec::new();
        if let Some(model) = &self.model {
            items.push(ChatItem::Session { id: "screen:session".into(), at: None, model: Some(model.clone()), cli_version: None });
        }
        // A message with nothing under it while the pane is quiet is a draft in the input box.
        let shown = match self.turns.last() {
            Some(last) if !last.has_body() && !busy => self.turns.len() - 1,
            _ => self.turns.len(),
        };
        for (index, turn) in self.turns.iter().take(shown).enumerate() {
            let n = index + 1;
            let ended = if n < shown { true } else { turn.has_body() && self.idle_polls >= ENDED_AFTER_IDLE_POLLS };
            if let Some(user) = &turn.user {
                items.push(ChatItem::User { id: format!("screen:{n}:user"), at: None, text: user.clone() });
            }
            for (k, tool) in turn.tools.iter().enumerate() {
                let id = format!("screen:{n}:tool:{k}");
                items.push(ChatItem::ToolCall { id: id.clone(), at: None, call_id: id, name: String::new(), summary: tool.clone() });
            }
            if let Some(reply) = &turn.reply {
                items.push(ChatItem::Assistant {
                    id: format!("screen:{n}:reply"),
                    at: None,
                    text: reply.chars().take(MAX_REPLY_CHARS).collect(),
                    model: self.model.clone(),
                    final_answer: ended,
                });
            }
            if ended && turn.has_body() {
                items.push(ChatItem::TurnEnd { id: format!("screen:{n}:end"), at: None, outcome: TurnOutcome::Completed, duration_ms: None });
            }
        }
        items
    }
}

/// Where `parsed` begins among `turns`: the latest place its messages line up with the turns' messages.
fn align(turns: &[Parsed], parsed: &[Parsed]) -> Option<usize> {
    (0..turns.len()).rev().find(|&first| {
        parsed.iter().zip(&turns[first..]).all(|(new, known)| same_message(known.user.as_deref(), new.user.as_deref()))
    })
}

/// Two readings of one message: equal, or one the beginning of the other (the box wraps, a draft grows).
fn same_message(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let (a, b) = (normalize_echo(a), normalize_echo(b));
            let head = |text: &str| text.chars().take(40).collect::<String>();
            let (a, b) = (head(&a), head(&b));
            a.starts_with(&b) || b.starts_with(&a)
        }
        _ => false,
    }
}

fn longer(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if a.chars().count() > b.chars().count() { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// Adds what `more` shows of a turn that is only partly on screen: new tool rows, and a reply when it is not
/// shorter than the one known.
fn grow(known: &mut Parsed, more: Parsed) {
    for tool in more.tools {
        if !known.tools.contains(&tool) {
            known.tools.push(tool);
        }
    }
    let rows = |reply: &Option<String>| reply.as_deref().map_or(0, |reply| reply.lines().count());
    if more.reply.is_some() && rows(&more.reply) >= rows(&known.reply) {
        known.reply = more.reply;
    }
}

/// What to send so the viewer's `shown` becomes `next`: nothing, the items that changed or were added (same ids
/// replace in place), or everything when an item went away or the order changed.
fn diff(shown: &mut Vec<ChatItem>, next: Vec<ChatItem>) -> Change {
    if *shown == next {
        return Change::Nothing;
    }
    let in_place = next.len() >= shown.len() && shown.iter().zip(&next).all(|(old, new)| old.id() == new.id());
    let change = if in_place {
        let changed = next.iter().enumerate().filter(|(index, item)| shown.get(*index) != Some(*item)).map(|(_, item)| item.clone());
        Change::Upsert(changed.collect())
    } else {
        Change::Reset(next.clone())
    };
    *shown = next;
    change
}

fn hash_of(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}
