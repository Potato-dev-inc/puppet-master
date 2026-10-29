//! The answers, the queue and the buttons of the engine (SPEC n33 sections 3.5 and 3.6): what a resolved card
//! does to its fix, which approved fix starts next, the buttons on a row, and the views the Settings page and the
//! PR block read. A card answer acts only on a question id stored in a fix (or one of our own follow-up cards,
//! remembered in `State::cards`), never on a card's kind or text.

use super::engine::{err, reaskable, CardKind, Engine, Job, CHOICE_GOT_IT, CHOICE_RETRY};
use super::filter;
use super::live::Io;
use super::model::*;
use super::view::{self, CiFixView, FixLogView, GhView, WatchView};
use crate::operations::OperationError;
use crate::questions::{Question, QuestionState};
use std::collections::HashMap;
use std::sync::Arc;

impl<I: Io + 'static> Engine<I> {
    /// A card was answered or dismissed (called from `live::on_resolved`).
    pub fn on_resolved(self: &Arc<Self>, question: &Question) {
        let known_follow_up = self.state.lock().cards.contains_key(&question.id);
        if self.io.store().fix_by_question(&question.id).is_none() && !known_follow_up {
            return;
        }
        let this = self.clone();
        let question = question.clone();
        self.io.spawn(
            "ci-fix-answer",
            Box::new(move || {
                let id = this
                    .io
                    .store()
                    .fix_by_question(&question.id)
                    .map(|fix| fix.id)
                    .or_else(|| this.state.lock().cards.get(&question.id).map(|card| card.fix_id.clone()));
                let Some(id) = id else { return };
                let Some(claim) = this.claim(&id) else { return };
                if let Some(fix) = this.io.store().fix_by_id(&id) {
                    this.apply_card_answer(fix, &question);
                }
                drop(claim);
                this.drive_queue();
            }),
        );
    }

    /// Do what a resolved card says. The fix's state must still point at this card.
    pub(super) fn apply_card_answer(self: &Arc<Self>, mut fix: FixRun, q: &Question) {
        let choice = q.answer.as_ref().and_then(|a| a.choice_id.clone());
        let by = match q.answer.as_ref().map(|a| a.answered_by.as_str()) {
            Some("phone") => "phone",
            _ => "desktop",
        };
        let dismissed = q.state == QuestionState::Cancelled;
        let follow_up = self.state.lock().cards.remove(&q.id);
        match (&fix.state.clone(), follow_up) {
            (FixState::Asking { question_id }, None) if *question_id == q.id => match (choice.as_deref(), dismissed) {
                (Some(CHOICE_FIX), false) => self.start_job(&fix.id, by),
                (Some(CHOICE_STOP), _) => {
                    if let Some(watch) = self.watch_of(&fix) {
                        fix.state = FixState::Declined;
                        self.put_fix(&fix);
                        self.stop_by_person(&watch, by);
                    }
                }
                _ => {
                    fix.state = FixState::Declined;
                    self.put_fix(&fix);
                }
            },
            (FixState::AskPush { question_id, facts }, None) if *question_id == q.id => match (choice.as_deref(), dismissed) {
                (Some(CHOICE_PUSH), false) => {
                    fix.state = FixState::Pushing;
                    self.put_fix(&fix);
                    self.push_job(&fix.id, Some(facts.clone()));
                }
                (Some(CHOICE_LOOK), false) => {
                    // The window opened the pane; the card stays: ask again under a new id.
                    if let Some(watch) = self.watch_of(&fix) {
                        let id = uuid::Uuid::new_v4().to_string();
                        fix.state = FixState::AskPush { question_id: id.clone(), facts: facts.clone() };
                        self.put_fix(&fix);
                        self.raise_push_card(&watch, &fix, facts, Some(id));
                    }
                }
                _ => {
                    fix.state = FixState::NotPushed;
                    self.put_fix(&fix);
                    self.io.audit(
                        "not_pushed",
                        by,
                        "Chose not to push a fix",
                        serde_json::json!({ "fix": fix.id, "watch": fix.watch_id }),
                    );
                }
            },
            (state, Some(card)) if card.fix_id == fix.id => self.apply_follow_up(fix, state.clone(), card.kind, choice.as_deref(), by),
            _ => {}
        }
    }

    fn apply_follow_up(self: &Arc<Self>, mut fix: FixRun, state: FixState, kind: CardKind, choice: Option<&str>, by: &str) {
        let Some(watch) = self.watch_of(&fix) else { return };
        match (kind, &state, choice) {
            (CardKind::NoChanges, FixState::NoChanges { message }, Some(CHOICE_LOOK)) => {
                self.raise_follow_up(
                    &fix,
                    CARD_KIND_PUSH,
                    &format!("No fix for #{}", watch.pr_number),
                    format!("{} · #{}\n{}", watch.owner_repo, watch.pr_number, message),
                    vec![super::engine::choice(CHOICE_LOOK, "Open the agent"), super::engine::choice(CHOICE_GOT_IT, "Got it")],
                    self.pane_of(&watch),
                    CardKind::NoChanges,
                );
            }
            (CardKind::Held, FixState::Held { message, .. }, Some(CHOICE_LOOK)) => {
                self.raise_held_card(&fix, &watch, message, None);
            }
            (CardKind::Held, FixState::Held { .. }, Some(CHOICE_KEEP)) => {
                fix.state = FixState::NotPushed;
                self.put_fix(&fix);
                self.io.audit("not_pushed", by, "Chose not to push a fix", serde_json::json!({ "fix": fix.id }));
            }
            (CardKind::RetryPush, FixState::Failed { .. }, Some(CHOICE_RETRY)) => {
                fix.state = FixState::Pushing;
                self.put_fix(&fix);
                self.push_job(&fix.id, None);
            }
            (CardKind::TriesUsed, _, Some(CHOICE_LOOK)) => {
                if let FixState::Failed { message, .. } = &state {
                    self.raise_follow_up(
                        &fix,
                        CARD_KIND_FIX,
                        message,
                        format!("{} · #{}", watch.owner_repo, watch.pr_number),
                        vec![super::engine::choice(CHOICE_LOOK, "Open the copy"), super::engine::choice(CHOICE_STOP, "Stop watching")],
                        self.pane_of(&watch),
                        CardKind::TriesUsed,
                    );
                }
            }
            (CardKind::TriesUsed, _, Some(CHOICE_STOP)) => self.stop_by_person(&watch, by),
            _ => {}
        }
    }

    // ------------------------------------------------------------------------- queue

    /// Start approved fixes while the slot is free (one fix at a time). A fix that waits for a busy copy
    /// is started when the copy is idle, and fails after 30 minutes. Marks the fix `Starting` before its
    /// thread begins, so the one-at-a-time rule holds even when two answers arrive together.
    pub(super) fn drive_queue(self: &Arc<Self>) {
        let _turn = self.queue.lock();
        let settings = self.io.store().settings();
        if !settings.enabled {
            return;
        }
        let mut queued: Vec<FixRun> =
            self.io.store().fixes().into_iter().filter(|fix| matches!(fix.state, FixState::Queued { .. })).collect();
        // The store keeps the newest first; the one that waited longest starts first.
        queued.reverse();
        for fix in queued {
            if filter::one_running(&self.io.store().fixes()) {
                return;
            }
            let Some(claim) = self.claim(&fix.id) else { continue };
            let Some(mut current) = self.io.store().fix_by_id(&fix.id) else { continue };
            let FixState::Queued { since_ms, .. } = current.state.clone() else { continue };
            let Some(watch) = self.watch_of(&current) else { continue };
            let now = self.io.now_ms();
            let busy = self
                .io
                .find_copy(&watch.project, &watch.handle)
                .is_some_and(|copy| self.io.copy_busy(&watch.project, &copy));
            if busy && now.saturating_sub(since_ms) < BUSY_WAIT_MS {
                continue;
            }
            let by = current.approved_by.clone().unwrap_or_else(|| "desktop".to_string());
            if !busy {
                current.state = FixState::Starting;
                self.put_fix(&current);
            }
            // A fix that waited too long goes through `start_job` too: it fails there with the plain message.
            self.spawn_job(claim, &fix.id, Job::Start(by));
        }
    }

    // ------------------------------------------------------------------------- buttons

    /// One button: `stop_watching` and `watch_again` for a watch id; `ask_again`, `push` and `dont_push` for a
    /// fix id. `who` is `desktop` or `phone`.
    pub fn act(self: &Arc<Self>, id: &str, action: &str, who: &str) -> Result<CiFixView, OperationError> {
        if !matches!(who, "desktop" | "phone") {
            return Err(err("INVALID_ARGUMENT", "who must be desktop or phone"));
        }
        if valid_watch_id(id) {
            let watch = self.io.store().watch_by_id(id).ok_or_else(|| err("CI_FIX_NOT_FOUND", "There is no such pull request."))?;
            let claim = self.claim(id).ok_or_else(busy)?;
            match action {
                "stop_watching" => {
                    if !matches!(watch.state, WatchState::Watching | WatchState::Paused { .. }) {
                        return Err(err("INVALID_ARGUMENT", "This pull request is not being watched."));
                    }
                    self.stop_by_person(&watch, who);
                }
                "watch_again" => self.watch_again(&watch, who)?,
                _ => return Err(err("INVALID_ARGUMENT", "unknown action")),
            }
            drop(claim);
            return Ok(self.view());
        }
        if valid_fix_id(id) {
            let fix = self.io.store().fix_by_id(id).ok_or_else(|| err("CI_FIX_NOT_FOUND", "There is no such fix."))?;
            match action {
                "ask_again" => self.ask_again(&fix, who)?,
                "push" => self.push_button(fix, who)?,
                "dont_push" => self.dont_push_button(fix, who)?,
                _ => return Err(err("INVALID_ARGUMENT", "unknown action")),
            }
            return Ok(self.view());
        }
        Err(err("INVALID_ARGUMENT", "that is not a watch or a fix id"))
    }

    /// `Watch again`: read the pull request again (it never fetches or rewrites the copy).
    fn watch_again(&self, watch: &Watch, who: &str) -> Result<(), OperationError> {
        if !matches!(watch.state, WatchState::Paused { .. } | WatchState::Stopped { .. }) {
            return Err(err("INVALID_ARGUMENT", "This pull request is already being watched or is finished."));
        }
        let saved = self.edit_watch(&watch.id, |w| {
            w.state = WatchState::Watching;
            // The person is back: the 14 days start again.
            w.last_push_ms = self.io.now_ms();
        });
        if saved.is_none() {
            return Err(err("CI_FIX_WATCH_FAILED", "Puppet Master could not watch this pull request again. Stop another watch first."));
        }
        self.state.lock().polls.remove(&watch.id);
        self.io.audit("watched", who, &format!("Watching #{} again", watch.pr_number), serde_json::json!({ "watch": watch.id }));
        self.poll_now();
        Ok(())
    }

    /// `Ask again`: a new card for the failure that is on the pull request now. Never a direct start.
    fn ask_again(self: &Arc<Self>, fix: &FixRun, who: &str) -> Result<(), OperationError> {
        if !reaskable(&fix.state) {
            return Err(err("INVALID_ARGUMENT", "This fix cannot be asked about again."));
        }
        let watch = self.watch_of(fix).ok_or_else(|| err("CI_FIX_NOT_FOUND", "There is no such pull request."))?;
        let sha = fix.failing_sha.clone();
        self.ask_now(&watch, Some(&sha), who)
    }

    /// Look at the pull request now and raise the card for a failure on it. `only_sha`: the failure must be on
    /// that commit. Errors say plainly why there is no card.
    pub(super) fn ask_now(self: &Arc<Self>, watch: &Watch, only_sha: Option<&str>, who: &str) -> Result<(), OperationError> {
        let _ = who;
        let settings = self.io.store().settings();
        if !settings.enabled {
            return Err(err("CI_FIX_OFF", "Turn on Red to Green in Settings > Labs."));
        }
        if !matches!(watch.state, WatchState::Watching) {
            return Err(err("INVALID_ARGUMENT", "Puppet Master is not watching this pull request. Use Watch again first."));
        }
        let gh = self.gh_status();
        match gh.state.as_str() {
            "ready" if !gh.login.is_empty() => {}
            "signed_out" => return Err(err(CI_FIX_GH_SIGNED_OUT, "Run gh auth login, then try again.")),
            _ => return Err(err(CI_FIX_GH_MISSING, "Install the GitHub CLI and run gh auth login.")),
        }
        let now = self.io.now_ms();
        let fixes = self.io.store().fixes();
        if watch.tries_used >= settings.tries_per_pr {
            return Err(err(CI_FIX_TRIES_USED, &super::text::tries_used_notice(watch.pr_number, watch.tries_used)));
        }
        if filter::day_count(&fixes, now) >= settings.fixes_per_day {
            return Err(err(CI_FIX_DAY_CAP, "The fixes for today are used up."));
        }
        let claim = self.claim(&watch.id).ok_or_else(busy)?;
        let before: Vec<String> = fixes.iter().map(|fix| fix.id.clone()).collect();
        self.poll_core(&watch.id, &gh.login, &settings, now, true);
        drop(claim);
        let created = self.io.store().fixes().into_iter().any(|fix| {
            !before.contains(&fix.id) && fix.watch_id == watch.id && only_sha.is_none_or(|sha| fix.failing_sha == sha)
        });
        let open = self.io.store().fixes().into_iter().any(|fix| {
            fix.watch_id == watch.id && matches!(fix.state, FixState::Asking { .. }) && only_sha.is_none_or(|sha| fix.failing_sha == sha)
        });
        if created || open {
            Ok(())
        } else {
            Err(err("INVALID_ARGUMENT", "CI is not failing on this pull request right now."))
        }
    }

    /// `Push` from the row: the same path as the card's `Push fix`.
    fn push_button(self: &Arc<Self>, mut fix: FixRun, who: &str) -> Result<(), OperationError> {
        let claim = self.claim(&fix.id).ok_or_else(busy)?;
        match fix.state.clone() {
            FixState::AskPush { question_id, facts } => {
                fix.state = FixState::Pushing;
                self.put_fix(&fix);
                self.withdraw(&question_id);
                self.record_approval(&fix.id, who);
                self.spawn_job(claim, &fix.id, Job::Push(Some(Box::new(facts))));
                Ok(())
            }
            FixState::Failed { .. } | FixState::Held { .. } => {
                // A push that failed runs again; a held fix is looked at again from the start.
                let held = matches!(fix.state, FixState::Held { .. });
                fix.state = if held { FixState::Checking } else { FixState::Pushing };
                self.put_fix(&fix);
                self.withdraw_cards_of(&fix.id);
                self.record_approval(&fix.id, who);
                self.spawn_job(claim, &fix.id, if held { Job::Finish } else { Job::Push(None) });
                Ok(())
            }
            _ => Err(err("INVALID_ARGUMENT", "There is nothing to push for this fix.")),
        }
    }

    fn record_approval(&self, fix_id: &str, who: &str) {
        self.edit_fix(fix_id, |fix| fix.approved_by = Some(who.to_string()));
    }

    /// `Don't push` from the row: the commits stay in the copy, nothing is deleted.
    fn dont_push_button(self: &Arc<Self>, mut fix: FixRun, who: &str) -> Result<(), OperationError> {
        let _claim = self.claim(&fix.id).ok_or_else(busy)?;
        let open = match &fix.state {
            FixState::AskPush { question_id, .. } => Some(question_id.clone()),
            FixState::Held { .. } | FixState::Failed { .. } => None,
            _ => return Err(err("INVALID_ARGUMENT", "There is nothing to hold back for this fix.")),
        };
        fix.state = FixState::NotPushed;
        self.put_fix(&fix);
        if let Some(question_id) = open {
            self.withdraw(&question_id);
        }
        self.withdraw_cards_of(&fix.id);
        self.io.audit("not_pushed", who, "Chose not to push a fix", serde_json::json!({ "fix": fix.id, "watch": fix.watch_id }));
        Ok(())
    }

    /// The PR block's `Fix with <agent>` / `Watch CI`: watch the pull request of this copy, and raise the card
    /// now when its head already failed.
    pub fn offer(self: &Arc<Self>, project: Option<&str>, handle: &str) -> Result<WatchView, OperationError> {
        let settings = self.io.store().settings();
        if !settings.enabled {
            return Err(err("CI_FIX_OFF", "Turn on Red to Green in Settings > Labs."));
        }
        let (project, copy) = self
            .io
            .find_copy_any(project, handle)
            .ok_or_else(|| err("WORKTREE_NOT_FOUND", "There is no copy for this pull request. It may be merged or discarded."))?;
        let now = self.io.now_ms();
        let watch = match self.io.store().watch_for(&project, handle) {
            Some(watch) if matches!(watch.state, WatchState::Watching | WatchState::Paused { .. }) => watch,
            _ => self.add_watch(&project, &copy, now)?,
        };
        if matches!(watch.state, WatchState::Watching) {
            // A pull request that does not fail is not an error here: the person only asked to watch it.
            match self.ask_now(&watch, None, "desktop") {
                Err(error) if matches!(error.code.as_str(), CI_FIX_TRIES_USED | CI_FIX_DAY_CAP | CI_FIX_GH_MISSING | CI_FIX_GH_SIGNED_OUT | "CI_FIX_BUSY") => {
                    return Err(error)
                }
                _ => {}
            }
        }
        self.watch_view_of(&watch.id).ok_or_else(|| err("CI_FIX_NOT_FOUND", "There is no such pull request."))
    }

    // ------------------------------------------------------------------------- settings and views

    pub fn save_settings(&self, settings: &Settings, who: &str) -> Result<CiFixView, OperationError> {
        let before = self.io.store().settings();
        let saved = self
            .io
            .store()
            .save_settings(settings)
            .map_err(|message| err("INVALID_ARGUMENT", &message))?;
        self.io.audit(
            "settings",
            who,
            if saved.enabled { "Red to Green settings saved (on)" } else { "Red to Green settings saved (off)" },
            serde_json::json!({
                "enabled": saved.enabled,
                "tries_per_pr": saved.tries_per_pr,
                "turns_per_try": saved.turns_per_try,
                "minutes_per_try": saved.minutes_per_try,
                "fixes_per_day": saved.fixes_per_day,
            }),
        );
        self.io.changed();
        if saved.enabled && !before.enabled {
            self.poll_now();
        }
        Ok(self.view())
    }

    fn notes(&self) -> HashMap<String, String> {
        self.state.lock().polls.iter().filter_map(|(id, poll)| poll.note.clone().map(|note| (id.clone(), note))).collect()
    }

    fn watch_view_of(&self, watch_id: &str) -> Option<WatchView> {
        let watch = self.io.store().watch_by_id(watch_id)?;
        let settings = self.io.store().settings();
        let note = self.notes().get(watch_id).cloned();
        Some(view::watch_view(&watch, &self.io.store().fixes(), note.as_deref(), settings.tries_per_pr))
    }

    /// What the Settings page shows.
    pub fn view(&self) -> CiFixView {
        let settings = self.io.store().settings();
        let gh = self.gh_status();
        let fixes = self.io.store().fixes();
        let notes = self.notes();
        let watches = self
            .io
            .store()
            .watches()
            .iter()
            .map(|watch| view::watch_view(watch, &fixes, notes.get(&watch.id).map(String::as_str), settings.tries_per_pr))
            .collect();
        let recent = fixes
            .iter()
            .take(view::RECENT_FIXES)
            .filter_map(|fix| self.io.store().watch_by_id(&fix.watch_id).map(|watch| view::fix_view(&watch, fix)))
            .collect();
        CiFixView { settings, gh, watches, fixes: recent }
    }

    /// The stored, cleaned snapshot of one fix (what the card's `Show log` shows). Empty once the fix ended.
    pub fn log(&self, fix_id: &str) -> Result<FixLogView, OperationError> {
        log_of(self.io.store(), fix_id)
    }
}

fn busy() -> OperationError {
    OperationError::new("CI_FIX_BUSY", "Puppet Master is working on this. Try again in a moment.", true)
}

/// The log view of a fix from the store (a bad id finds nothing).
pub(super) fn log_of(store: &super::store::Store, fix_id: &str) -> Result<FixLogView, OperationError> {
    let fix = store.fix_by_id(fix_id).ok_or_else(|| err("CI_FIX_NOT_FOUND", "There is no such fix."))?;
    Ok(view::log_view(&fix))
}

/// The view when the engine has not started (the app is still starting).
pub(super) fn offline_view(store: &super::store::Store) -> CiFixView {
    let settings = store.settings();
    let fixes = store.fixes();
    let watches = store.watches().iter().map(|watch| view::watch_view(watch, &fixes, None, settings.tries_per_pr)).collect();
    CiFixView {
        settings,
        gh: GhView { state: "missing".to_string(), login: String::new() },
        watches,
        fixes: Vec::new(),
    }
}
