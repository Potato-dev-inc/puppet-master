//! Spotting a live pane that stays on a screen its profile cannot read, so the screen can be saved
//! (`captures`).
//!
//! Cost model (the same as `pane_activity`): `observe` is called for panes whose output changed,
//! at most once a second each, and only remembers what it saw. `tick` runs once a second and
//! looks at nothing but that small table. A pane that is silent costs nothing.
//!
//! "Unreadable" means the screen holds still, is not blank, and the profile recognises none of: a
//! startup gate, a prompt, a working turn, an input box. For a profile that reads its composer
//! that is `ComposerState::Unknown`. For one that only knows its ready screen it is "has not yet
//! been ready since it started": once such a pane has been ready, a later screen it cannot
//! classify is far more likely a menu the person opened than a changed CLI, so it is not saved.

use crate::pty::status::PaneStatus;
use crate::tui::{ComposerState, StandbyScreen, TuiProfile};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

/// How long a still, unreadable screen must stay before it is saved.
pub const UNKNOWN_AFTER: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// The profile recognises an input box, a working turn, a gate or a prompt.
    Known,
    /// Nothing the profile can classify.
    Unknown,
}

/// What the profile makes of `screen`. `seen_ready`: this pane has been recognised as ready before.
pub fn read_screen(profile: &dyn TuiProfile, screen: &str, status: PaneStatus, seen_ready: bool) -> (Read, bool) {
    if screen.trim().is_empty() || matches!(status, PaneStatus::Error | PaneStatus::WaitingInput) {
        return (Read::Known, seen_ready);
    }
    if profile.startup_gate(screen).is_some() || profile.detect_prompt(screen, false).is_some() || profile.looks_like_menu(screen) {
        return (Read::Known, seen_ready);
    }
    if profile.reads_composer() {
        return match profile.composer_state(screen) {
            ComposerState::Unknown => (Read::Unknown, seen_ready),
            _ => (Read::Known, true),
        };
    }
    let support = profile.standby_support();
    if !support.visible || support.experimental {
        // No positive ready screen to compare with: nothing can be called unreadable.
        return (Read::Known, seen_ready);
    }
    match profile.standby_screen(screen) {
        StandbyScreen::Ready => (Read::Known, true),
        StandbyScreen::Dialog | StandbyScreen::NeedsAttention => (Read::Known, seen_ready),
        StandbyScreen::NotReady if seen_ready => (Read::Known, true),
        StandbyScreen::NotReady => (Read::Unknown, false),
    }
}

fn hash_of(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

struct Watched {
    agent: String,
    seen_ready: bool,
    /// The screen being timed, its hash and when it first showed (unchanged since).
    screen: String,
    hash: u64,
    since: Option<Instant>,
    saved: bool,
}

/// A screen that stayed unreadable long enough to save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    pub pane_id: String,
    pub agent: String,
    pub screen: String,
    pub unreadable_for: Duration,
}

#[derive(Default)]
pub struct Tracker {
    panes: HashMap<String, Watched>,
}

impl Tracker {
    pub fn observe(&mut self, now: Instant, pane_id: &str, agent: &str, profile: &dyn TuiProfile, screen: &str, status: PaneStatus) {
        let entry = self.panes.entry(pane_id.to_string()).or_insert_with(|| Watched {
            agent: agent.to_string(),
            seen_ready: false,
            screen: String::new(),
            hash: 0,
            since: None,
            saved: false,
        });
        let (read, seen_ready) = read_screen(profile, screen, status, entry.seen_ready);
        entry.seen_ready = seen_ready;
        match read {
            Read::Known => {
                entry.since = None;
                entry.saved = false;
                entry.hash = 0;
            }
            Read::Unknown => {
                let hash = hash_of(screen);
                if entry.since.is_none() || entry.hash != hash {
                    entry.hash = hash;
                    entry.since = Some(now);
                    entry.saved = false;
                    entry.screen = screen.to_string();
                }
            }
        }
    }

    /// Screens that have been unreadable for `UNKNOWN_AFTER`, each handed out once; panes that are
    /// gone (`alive` false) are forgotten.
    pub fn due(&mut self, now: Instant, alive: &dyn Fn(&str) -> bool) -> Vec<Due> {
        self.panes.retain(|id, _| alive(id));
        let mut out = Vec::new();
        for (id, watched) in self.panes.iter_mut() {
            let Some(since) = watched.since else { continue };
            let waited = now.saturating_duration_since(since);
            if !watched.saved && waited >= UNKNOWN_AFTER {
                watched.saved = true;
                out.push(Due {
                    pane_id: id.clone(),
                    agent: watched.agent.clone(),
                    screen: watched.screen.clone(),
                    unreadable_for: waited,
                });
            }
        }
        out
    }
}

static TRACKER: Lazy<Mutex<Tracker>> = Lazy::new(Default::default);

/// Called from the pane-activity ticker for a pane whose screen changed.
pub fn observe(pane_id: &str, agent: &str, screen: &str, status: PaneStatus) {
    let profile = crate::tui::profile_for(agent);
    TRACKER.lock().observe(Instant::now(), pane_id, agent, profile.as_ref(), screen, status);
}

/// Called from the same ticker once a second: saves screens that stayed unreadable.
pub fn tick(alive: &dyn Fn(&str) -> bool) {
    let due = TRACKER.lock().due(Instant::now(), alive);
    for item in due {
        super::save_unrecognised(&item.agent, &item.screen, item.unreadable_for);
        super::note_signal(&item.agent, super::Signal::UnreadableScreen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::profile_for;

    const CODEX_READY: &str = include_str!("../tui/codex/fixtures/standby_ready_live.txt");
    const CODEX_UPDATE_MENU: &str = include_str!("../tui/codex/fixtures/update_prompt_live.txt");
    const CODEX_WORKING: &str = include_str!("../tui/codex/fixtures/working_live.txt");
    const UNREADABLE: &str = "  Codex 0.171\n\n  ┌──────────────────────────────┐\n  │ Pick a theme to continue     │\n  │  a) light    b) dark         │\n  └──────────────────────────────┘\n";

    fn secs(base: Instant, n: u64) -> Instant {
        base + Duration::from_secs(n)
    }

    #[test]
    fn a_screen_the_profile_cannot_read_is_due_after_the_wait_and_only_once() {
        let codex = profile_for("codex");
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.observe(t0, "p1", "codex", codex.as_ref(), UNREADABLE, PaneStatus::Idle);
        assert!(tracker.due(secs(t0, 3), &|_| true).is_empty(), "too soon");
        let due = tracker.due(secs(t0, 7), &|_| true);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].screen, UNREADABLE);
        assert!(due[0].unreadable_for >= UNKNOWN_AFTER);
        assert!(tracker.due(secs(t0, 20), &|_| true).is_empty(), "handed out once");
    }

    #[test]
    fn screens_the_profile_reads_are_never_due() {
        let codex = profile_for("codex");
        let t0 = Instant::now();
        for (name, screen, status) in [
            ("ready composer", CODEX_READY, PaneStatus::Idle),
            ("update menu (a known gate)", CODEX_UPDATE_MENU, PaneStatus::Idle),
            ("working turn", CODEX_WORKING, PaneStatus::Running),
            ("blank", "   \n  ", PaneStatus::Idle),
        ] {
            let mut tracker = Tracker::default();
            tracker.observe(t0, "p", "codex", codex.as_ref(), screen, status);
            assert!(tracker.due(secs(t0, 60), &|_| true).is_empty(), "{name}");
        }
    }

    #[test]
    fn a_changing_screen_restarts_the_wait() {
        let codex = profile_for("codex");
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.observe(t0, "p1", "codex", codex.as_ref(), UNREADABLE, PaneStatus::Idle);
        let other = format!("{UNREADABLE}\n  one more line");
        tracker.observe(secs(t0, 5), "p1", "codex", codex.as_ref(), &other, PaneStatus::Idle);
        assert!(tracker.due(secs(t0, 8), &|_| true).is_empty(), "the clock restarted at 5s");
        assert_eq!(tracker.due(secs(t0, 12), &|_| true).len(), 1);
    }

    #[test]
    fn reading_the_screen_again_clears_the_wait() {
        let codex = profile_for("codex");
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.observe(t0, "p1", "codex", codex.as_ref(), UNREADABLE, PaneStatus::Idle);
        tracker.observe(secs(t0, 2), "p1", "codex", codex.as_ref(), CODEX_READY, PaneStatus::Idle);
        assert!(tracker.due(secs(t0, 30), &|_| true).is_empty());
    }

    #[test]
    fn a_pane_that_is_gone_is_forgotten() {
        let codex = profile_for("codex");
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.observe(t0, "gone", "codex", codex.as_ref(), UNREADABLE, PaneStatus::Idle);
        assert!(tracker.due(secs(t0, 30), &|_| false).is_empty());
        assert!(tracker.panes.is_empty());
    }

    /// Claude's fresh-start screen at its empty prompt (the profile's own ready fixture).
    const CLAUDE_READY: &str = "\n \u{2590}\u{259b}\u{2588}\u{2588}\u{2588}\u{259c}\u{258c}   Claude Code v2.1.299\n\u{259d}\u{259c}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{259b}\u{2580}  Sonnet 5.5 \u{b7} Claude Pro\n\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} Try \"edit <filepath> to...\"\n\u{2500}\u{2500}\u{2500}\u{2500}\n  \u{23f5}\u{23f5} auto mode on (shift+tab to cycle)";

    #[test]
    fn a_profile_that_only_knows_its_ready_screen_flags_a_stuck_start_but_not_a_later_menu() {
        // Claude has `standby_screen` but no composer reader.
        let claude = profile_for("claude");
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.observe(t0, "c1", "claude", claude.as_ref(), UNREADABLE, PaneStatus::Idle);
        assert_eq!(tracker.due(secs(t0, 7), &|_| true).len(), 1, "never ready since it started");

        // Ready first, then a screen it cannot classify: a menu the person opened, not a changed CLI.
        let mut tracker = Tracker::default();
        tracker.observe(t0, "c2", "claude", claude.as_ref(), CLAUDE_READY, PaneStatus::Idle);
        tracker.observe(secs(t0, 1), "c2", "claude", claude.as_ref(), UNREADABLE, PaneStatus::Idle);
        assert!(tracker.due(secs(t0, 30), &|_| true).is_empty());
    }

    #[test]
    fn a_profile_with_no_ready_screen_to_compare_with_flags_nothing() {
        let generic = profile_for("definitely_not_registered");
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.observe(t0, "g", "definitely_not_registered", generic.as_ref(), UNREADABLE, PaneStatus::Idle);
        assert!(tracker.due(secs(t0, 60), &|_| true).is_empty());
    }
}
