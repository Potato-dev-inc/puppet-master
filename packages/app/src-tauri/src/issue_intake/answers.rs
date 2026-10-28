//! The answers, the queue and the Settings buttons of the engine (SPEC n32 section 3.6, 3.7): what a
//! resolved card does to its run, which approved run starts next, the five buttons on a run row, and the
//! view the Settings page reads. A card answer acts only on a question id stored in a run (or one of our
//! own follow-up cards, remembered in `State::cards`), never on a card's kind or text.

use super::engine::{err, CardKind, Engine, Job, BASELINE_SKIP, CHOICE_RETRY};
use super::filter;
use super::live::Io;
use super::model::*;
use super::view::{self, GhView, IntakeView, RepoStatusView, RunView};
use crate::questions::{Question, QuestionState};
use std::sync::Arc;

impl<I: Io + 'static> Engine<I> {

    /// A card was answered or dismissed (called from `live::on_resolved`).
    pub fn on_resolved(self: &Arc<Self>, question: &Question) {
        let known_follow_up = self.state.lock().cards.contains_key(&question.id);
        if self.io.store().by_question(&question.id).is_none() && !known_follow_up {
            return;
        }
        let this = self.clone();
        let question = question.clone();
        self.io.spawn(
            "issue-intake-answer",
            Box::new(move || {
                let id = this
                    .io
                    .store()
                    .by_question(&question.id)
                    .map(|run| run.id)
                    .or_else(|| this.state.lock().cards.get(&question.id).map(|card| card.run_id.clone()));
                let Some(id) = id else { return };
                let Some(claim) = this.claim(&id) else { return };
                if let Some(run) = this.io.store().by_id(&id) {
                    this.apply_card_answer(run, &question);
                }
                drop(claim);
                this.drive_queue();
            }),
        );
    }

    /// Do what a resolved card says. The run's state must still point at this card.
    pub(super) fn apply_card_answer(self: &Arc<Self>, mut run: IssueRun, q: &Question) {
        let choice = q.answer.as_ref().and_then(|a| a.choice_id.clone());
        let by = match q.answer.as_ref().map(|a| a.answered_by.as_str()) {
            Some("phone") => "phone",
            _ => "desktop",
        };
        let dismissed = q.state == QuestionState::Cancelled;
        let follow_up = self.state.lock().cards.remove(&q.id);
        match (&run.state.clone(), follow_up) {
            (RunState::Asking { question_id }, None) if *question_id == q.id => {
                match (choice.as_deref(), dismissed) {
                    (Some(CHOICE_START), _) => self.approve(&mut run, by),
                    (Some(CHOICE_IGNORE), _) => self.ignore(&mut run),
                    _ => {
                        run.state = RunState::Declined;
                        self.put(&run);
                    }
                }
            }
            (RunState::AskPush { question_id, facts }, None) if *question_id == q.id => {
                match (choice.as_deref(), dismissed) {
                    (Some(CHOICE_PUSH), _) => {
                        run.state = RunState::Pushing;
                        self.put(&run);
                        self.push_job(&run.id);
                    }
                    (Some(CHOICE_LOOK), _) => {
                        if let Some(id) = self.reraise_push_card(&run, facts) {
                            run.state = RunState::AskPush { question_id: id, facts: facts.clone() };
                            self.put(&run);
                        }
                    }
                    (Some(CHOICE_THROW), _) => {
                        // Refused while the agent works: the card must not be left pointing at a dead question.
                        if self.throw_away(&mut run).is_err() {
                            if let Some(id) = self.reraise_push_card(&run, facts) {
                                run.state = RunState::AskPush { question_id: id, facts: facts.clone() };
                                self.put(&run);
                            }
                        }
                    }
                    _ => {
                        run.state = RunState::Held {
                            code: ERR_PUSH_HELD.to_string(),
                            message: "You closed the card. Nothing was pushed.".to_string(),
                        };
                        self.put(&run);
                    }
                }
            }
            (state, Some(card)) if card.run_id == run.id => self.apply_follow_up(run, state.clone(), card.kind, choice.as_deref()),
            _ => {}
        }
    }

    /// The person chose Start: the run waits for a free slot (`drive_queue` starts it).
    pub(super) fn approve(&self, run: &mut IssueRun, by: &str) {
        run.approved_by = Some(by.to_string());
        run.state = RunState::Queued { why: "Waiting for a free slot".to_string() };
        self.put(run);
    }

    fn ignore(&self, run: &mut IssueRun) {
        let mut cfg = self.io.store().config();
        let pair = (run.issue.owner_repo.clone(), run.issue.number);
        if !cfg.ignored.contains(&pair) {
            cfg.ignored.push(pair);
            if let Err(message) = self.io.store().save_config(&cfg) {
                tracing::warn!(%message, "issue-intake: could not save an ignored issue");
            }
        }
        run.state = RunState::Ignored;
        self.put(run);
    }

    fn apply_follow_up(self: &Arc<Self>, mut run: IssueRun, state: RunState, kind: CardKind, choice: Option<&str>) {
        match (kind, &state, choice) {
            (CardKind::NoChanges, RunState::NoChanges { message }, Some(CHOICE_LOOK)) => {
                self.raise_no_changes_card(&run, message);
            }
            (CardKind::Held, RunState::Held { message, .. }, Some(CHOICE_LOOK)) => {
                self.raise_held_card(&run, message);
            }
            (CardKind::NoChanges | CardKind::Held, _, Some(CHOICE_THROW)) => {
                let _ = self.throw_away(&mut run);
            }
            (CardKind::RetryPush, RunState::Failed { .. }, Some(CHOICE_RETRY)) => {
                run.state = RunState::Pushing;
                self.put(&run);
                self.push_job(&run.id);
            }
            (CardKind::RetryCheck, RunState::Failed { .. }, Some(CHOICE_RETRY)) => {
                run.state = RunState::Checking;
                self.put(&run);
                self.finish_job(&run.id);
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------------- queue

    /// Start approved runs while a slot is free. Marks each `Starting` before its thread begins, so the
    /// parallel cap and the day cap are honoured even when two answers arrive together.
    pub(super) fn drive_queue(self: &Arc<Self>) {
        let _turn = self.queue.lock();
        loop {
            let cfg = self.io.store().config();
            if !cfg.enabled {
                return;
            }
            let now = self.io.now_ms();
            let runs = self.io.store().runs();
            let Some(id) = filter::next_queued(&runs, &cfg, now) else { return };
            let Some(claim) = self.claim(&id) else { return };
            let Some(mut run) = self.io.store().by_id(&id) else { return };
            run.state = RunState::Starting;
            run.started_at_ms = Some(now);
            self.put(&run);
            let this = self.clone();
            self.io.spawn(
                "issue-intake-start",
                Box::new(move || {
                    this.start_job(&id);
                    drop(claim);
                }),
            );
        }
    }

    // ------------------------------------------------------------------------- Settings buttons

    /// One Settings button on a run. `who` is `desktop` or `phone`.
    pub fn act(self: &Arc<Self>, run_id: &str, action: &str, who: &str) -> Result<RunView, crate::operations::OperationError> {
        if !matches!(who, "desktop" | "phone") {
            return Err(err("INVALID_ARGUMENT", "who must be desktop or phone"));
        }
        if !valid_run_id(run_id) {
            return Err(err("INVALID_ARGUMENT", "that is not an issue run id"));
        }
        let claim = self.claim(run_id).ok_or_else(|| {
            crate::operations::OperationError::new("ISSUE_BUSY", "Puppet Master is working on this run. Try again in a moment.", true)
        })?;
        let mut run = self.io.store().by_id(run_id).ok_or_else(|| err("ISSUE_RUN_NOT_FOUND", "There is no such run."))?;
        let mut start_queue = false;
        match action {
            "start" => {
                let RunState::Asking { question_id } = run.state.clone() else {
                    return Err(err("INVALID_ARGUMENT", "This run is not waiting for a yes."));
                };
                self.approve(&mut run, who);
                self.withdraw(&question_id);
                start_queue = true;
            }
            "ask_again" => self.ask_again(&mut run)?,
            "push" => match run.state.clone() {
                RunState::AskPush { question_id, .. } => {
                    run.state = RunState::Pushing;
                    self.put(&run);
                    self.withdraw(&question_id);
                    self.spawn_job(claim, &run.id, Job::Push);
                    return self.run_view_of(run_id);
                }
                RunState::Held { .. } | RunState::Failed { .. } if run.handle.is_some() => {
                    run.state = RunState::Checking;
                    self.withdraw_cards_of(&run.id);
                    self.put(&run);
                    self.spawn_job(claim, &run.id, Job::Finish);
                    return self.run_view_of(run_id);
                }
                _ => return Err(err("INVALID_ARGUMENT", "There is nothing to push for this run.")),
            },
            "throw_away" => self.throw_away(&mut run)?,
            "unignore" => self.unignore(&mut run)?,
            _ => return Err(err("INVALID_ARGUMENT", "unknown action")),
        }
        drop(claim);
        if start_queue {
            self.drive_queue();
        }
        self.run_view_of(run_id)
    }

    fn run_view_of(&self, run_id: &str) -> Result<RunView, crate::operations::OperationError> {
        let run = self.io.store().by_id(run_id).ok_or_else(|| err("ISSUE_RUN_NOT_FOUND", "There is no such run."))?;
        Ok(view::run_view(&run))
    }

    /// Raise the start card again for a run that said no, ran out, or never got a copy.
    fn ask_again(&self, run: &mut IssueRun) -> Result<(), crate::operations::OperationError> {
        let allowed = match &run.state {
            RunState::Skipped { why } => {
                if !why.starts_with(BASELINE_SKIP) {
                    return Err(err(ERR_NOT_ALLOWED, "Only you or someone on your list can start a run."));
                }
                true
            }
            RunState::Declined | RunState::Expired => true,
            RunState::Failed { .. } => run.handle.is_none(),
            _ => false,
        };
        if !allowed {
            return Err(err("INVALID_ARGUMENT", "This run cannot be asked about again."));
        }
        let repo = self.repo_of(run).ok_or_else(|| err(ERR_REPO_MISMATCH, "This repository is not on your list any more."))?;
        let me = self.gh_status().login.unwrap_or_default();
        run.forced_ask = filter::forced_reasons(&repo, &me, &self.io.store().config(), &run.issue, &self.io.store().runs());
        run.agent = repo.agent.clone();
        run.agent_label = self.io.agent_label(&repo.agent.agent_type);
        let id = self.raise_start_card(run).ok_or_else(|| err("TOO_MANY_QUESTIONS", "There are too many open cards. Answer some first."))?;
        run.started_at_ms = None;
        run.state = RunState::Asking { question_id: id };
        self.put(run);
        Ok(())
    }

    fn unignore(&self, run: &mut IssueRun) -> Result<(), crate::operations::OperationError> {
        if run.state != RunState::Ignored {
            return Err(err("INVALID_ARGUMENT", "This issue is not ignored."));
        }
        let mut cfg = self.io.store().config();
        let pair = (run.issue.owner_repo.clone(), run.issue.number);
        cfg.ignored.retain(|item| *item != pair);
        self.io.store().save_config(&cfg).map_err(|message| err("INVALID_ARGUMENT", &message))?;
        run.state = RunState::Declined;
        self.put(run);
        Ok(())
    }

    // ------------------------------------------------------------------------- view

    /// What the Settings page shows.
    pub fn view(&self) -> IntakeView {
        let cfg = self.io.store().config();
        let gh = self.gh_status();
        let runs = self.io.store().runs();
        let repos = cfg
            .repos
            .iter()
            .map(|repo| {

                let status = if !cfg.enabled {
                    "Off".to_string()
                } else if repo.paused {
                    "Paused".to_string()
                } else if gh.state != "ready" {
                    "Waiting for the GitHub CLI".to_string()
                } else {
                    match self.repo_error(&repo.id) {
                        Some(reason) => format!("Couldn't read GitHub: {reason}"),
                        None => "Watching".to_string(),
                    }
                };
                RepoStatusView { repo: repo.clone(), agent_label: self.io.agent_label(&repo.agent.agent_type), status }
            })
            .collect();
        IntakeView { config: cfg, gh, repos, runs: view::recent(&runs) }
    }
}

/// The settings view when the engine has not started (the app is still starting).
pub(super) fn offline_view(store: &super::store::Store) -> IntakeView {
    let config = store.config();
    let repos = config
        .repos
        .iter()
        .map(|repo| RepoStatusView {
            repo: repo.clone(),
            agent_label: crate::crews::live::label_of(&repo.agent.agent_type),
            status: "Starting".to_string(),
        })
        .collect();
    IntakeView { config, gh: GhView { state: "missing".to_string(), login: None }, repos, runs: view::recent(&store.runs()) }
}
