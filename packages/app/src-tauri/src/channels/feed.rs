//! Text Your Agents: what the app tells the chat. `offer` (a notification candidate), `companion_reply`
//! and the 5 s `tick` (a summary of what was held back, and the end of a turn in a pane the person is
//! talking to) hand things to the `Notifier`, which decides; what it returns is queued for the poller's
//! send thread. Nothing here decides what the chat may read: that is all in `notify`.

use super::notify::{Links, Note};
use super::runtime::Runtime;
use crate::push::hub::Candidate;
use crate::push::triggers::Kind;
use crate::turn_summary::model::TurnSummary;

/// A question's candidate key is `q-<question id>` (see `push::notify_question`).
const QUESTION_KEY: &str = "q-";

impl Runtime {
    /// A notification candidate happened on the desktop. Cheap when the chat is not listening.
    pub fn offer(&self, c: &Candidate) {
        if !self.listening() {
            return;
        }
        let cfg = self.deps.host.config();
        let now = self.deps.host.now_ms();
        let question = match c.kind {
            Kind::Question => c.key.strip_prefix(QUESTION_KEY).and_then(|id| self.deps.host.question(id)),
            _ => None,
        };
        let note = self.with_links(|links| self.notifier.offer(c, question.as_ref(), &cfg, links, now));
        self.send_note(note);
    }

    /// A companion's reply went into its feed. It reaches the chat only inside a conversation the
    /// person started from the chat (D-8).
    pub fn companion_reply(&self, companion_id: &str, name: &str, text: &str) {
        if !self.listening() {
            return;
        }
        let cfg = self.deps.host.config();
        let now = self.deps.host.now_ms();
        let note = self.with_links(|links| self.notifier.companion_reply(companion_id, name, text, &cfg, links, now));
        self.send_note(note);
    }

    /// Every 5 s. Ends a poller that is no longer wanted, then (while listening) sends the one
    /// "N more things need you" note for what was held back, and "Done · ..." for a turn that ended in
    /// a pane the person texted in the last 15 minutes.
    pub fn tick(&self) {
        self.watch();
        let now = self.deps.host.now_ms();
        if !self.listening() {
            // Turns that end while the chat is off are never reported later.
            *self.turns_seen_ms.lock() = now;
            return;
        }
        let held = {
            let mut limits = self.engine.limits().lock();
            self.notifier.flush_held(&mut limits.outbound, now)
        };
        self.send_note(held);
        let since = {
            let mut seen = self.turns_seen_ms.lock();
            std::mem::replace(&mut *seen, now)
        };
        let turns = (self.deps.turns)(since.saturating_add(1));
        if !turns.is_empty() {
            self.turns_ended(&turns, now);
        }
    }

    /// The end of each turn, for panes the chat can see (companion and shell panes are not listed
    /// by the host, so they never get a "Done" line).
    fn turns_ended(&self, turns: &[TurnSummary], now: u64) {
        let cfg = self.deps.host.config();
        let panes = self.deps.host.panes();
        for turn in turns {
            let Some(line) = panes.iter().find(|p| p.id == turn.pane_id) else { continue };
            let note = self.with_links(|links| {
                self.notifier.turn_finished(&turn.pane_id, &line.agent_label, Some(&line.heading), &turn.sentence, &cfg, links, now)
            });
            self.send_note(note);
        }
    }

    /// The refs and the outbound bucket are shared with the engine. It never holds both at once, so
    /// taking them together here cannot deadlock.
    fn with_links<T>(&self, f: impl FnOnce(Links<'_>) -> T) -> T {
        let refs = self.engine.refs().lock();
        let mut limits = self.engine.limits().lock();
        f(Links { refs: &refs, grants: self.engine.grants(), outbound: &mut limits.outbound })
    }

    fn send_note(&self, note: Option<Note>) {
        if let Some(note) = note {
            self.queue(note.into_effect());
        }
    }
}
