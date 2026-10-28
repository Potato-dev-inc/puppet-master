//! The engine of "Assign an issue" (SPEC n32 section 3.6): the watch loop, the cards and the answers.
//! It talks to the world only through [`Io`], so the tests drive it with a fake and a virtual clock.
//!
//! - A tick (every 15 seconds, only while the Labs switch is on) looks at each repo's labeled issues
//!   through the person's `gh` login, asks `filter` who may start a run, and raises a card or queues
//!   the run. It then picks up runs that need work: a finished agent, a card that timed out, a run the
//!   app lost in a restart.
//! - A card answer (`on_resolved`) or a Settings button (`act`) changes one saved run. The work
//!   (`flows.rs`: start, finish, push, throw away) runs on its own thread.
//! - One thread at most works on one run: a run id is "claimed" while a job holds it.
//!
//! Safety rules kept here: an answer acts only on a question id stored in a run (or one of our own
//! follow-up cards), never on a card's kind or text. Nothing here writes to a pane. Every GitHub
//! read fails closed. The store lock is never held across `gh` or `git`.

use super::filter;
use super::github::{self, ApiIssue, LabelEvent};
use super::live::Io;
use super::model::*;
use super::text;
use super::view::GhView;
use crate::questions::{Choice, NewQuestion, Question, QuestionState};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

/// The label came before the repo was added (`filter::candidate` words it this way).
pub(super) const BASELINE_SKIP: &str = "Labeled before you added this repo";
/// How long a good `gh` answer is trusted; a bad one is asked again after a minute.
const GH_GOOD_MS: u64 = 10 * 60 * 1000;
/// A plain notice is not a person-only card, so it may stay open for an hour at most.
const NOTICE_TIMEOUT_S: u64 = 3600;
const GH_BAD_MS: u64 = 60 * 1000;
/// Extra choices of our own follow-up cards.
pub(super) const CHOICE_RETRY: &str = "retry";
pub(super) const CHOICE_GOT_IT: &str = "got_it";
const FIRST_LOOK_DELAY: Duration = Duration::from_secs(3);
const SLICE: Duration = Duration::from_millis(250);

/// A card of ours that is not stored in a run's state (it only offers a follow-up).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CardKind {
    /// The agent finished with no changes: `Open the agent` / `Throw away`.
    NoChanges,
    /// A guard stopped the push: `Look first` / `Throw away`.
    Held,
    /// The PR failed: `Try again` runs the push again.
    RetryPush,
    /// The check before the push card failed: `Try again` runs the check again.
    RetryCheck,
}

#[derive(Debug, Clone)]
pub(super) struct CardRef {
    pub run_id: String,
    pub kind: CardKind,
}

#[derive(Debug, Clone)]
struct GhCache {
    state: String,
    login: Option<String>,
    at_ms: u64,
}

/// What the poller remembers about one repo (in memory only; a restart looks again).
#[derive(Debug, Default)]
struct RepoPoll {
    etag: Option<String>,
    next_at_ms: u64,
    backoff_s: u64,
    checked_ms: Option<u64>,
    error: Option<String>,
    /// Issue number -> `updated_at` when its label events were last read.
    seen: HashMap<u64, String>,
}

#[derive(Default)]
pub(super) struct State {
    gh: Option<GhCache>,
    polls: HashMap<String, RepoPoll>,
    pub(super) busy: HashSet<String>,
    pub(super) cards: HashMap<String, CardRef>,
    poll_now: bool,
}

pub struct Engine<I: Io> {
    pub(super) io: I,
    pub(super) state: Mutex<State>,
    pub(super) queue: Mutex<()>,
}

/// One run is being worked on by one job. Dropping it frees the run.
pub(super) struct Claim<I: Io + 'static> {
    engine: Arc<Engine<I>>,
    id: String,
}

impl<I: Io + 'static> Drop for Claim<I> {
    fn drop(&mut self) {
        self.engine.state.lock().busy.remove(&self.id);
    }
}

/// A card of the issue kinds (`issue_start`, `issue_push`).
pub(super) fn is_issue_card(question: &Question) -> bool {
    matches!(question.kind.as_deref(), Some(CARD_KIND_START | CARD_KIND_PUSH))
}

/// A run that no job is waiting on and that ends an issue's story: a new label may start a new run.
pub(super) fn ends_story(state: &RunState) -> bool {
    matches!(
        state,
        RunState::Skipped { .. }
            | RunState::Declined
            | RunState::Ignored
            | RunState::Expired
            | RunState::PrOpen { .. }
            | RunState::NoChanges { .. }
            | RunState::ThrownAway
            | RunState::Failed { .. }
    )
}

pub(super) fn err(code: &str, message: &str) -> crate::operations::OperationError {
    crate::operations::OperationError::new(code, message, false)
}

/// A message that is safe to put on a card or in a row: one line, masked, the home folder as `~`, cut.
pub(super) fn clean_message(raw: &str) -> String {
    let masked = crate::redact::strict(raw).into_inner();
    let home = crate::project_path::home_dir();
    let line = crate::session_chat::home_path::home_to_tilde(&masked, home.as_deref().and_then(|p| p.to_str()));
    let flat: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let flat: String = flat.chars().filter(|c| !c.is_control()).collect();
    if flat.chars().count() > 240 {
        format!("{}\u{2026}", flat.chars().take(239).collect::<String>().trim_end())
    } else {
        flat
    }
}

impl<I: Io + 'static> Engine<I> {
    pub fn new(io: I) -> Self {
        Self { io, state: Mutex::new(State::default()), queue: Mutex::new(()) }
    }

    pub fn io(&self) -> &I {
        &self.io
    }

    // ------------------------------------------------------------------------- the loop

    /// The `issue-intake` thread body: a look every 15 seconds, sooner after `poll_now`.
    pub fn run_loop(self: Arc<Self>) {
        std::thread::sleep(FIRST_LOOK_DELAY);
        loop {
            let this = self.clone();
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || this.tick()));
            if outcome.is_err() {
                tracing::warn!("issue-intake: a look at GitHub failed unexpectedly");
            }
            let mut waited = Duration::ZERO;
            while waited < Duration::from_millis(TICK_MS) {
                if self.take_poll_now() {
                    break;
                }
                std::thread::sleep(SLICE);
                waited += SLICE;
            }
        }
    }

    /// Why the last look at a repo failed, if it did (for the Settings row).
    pub(super) fn repo_error(&self, repo_id: &str) -> Option<String> {
        self.state.lock().polls.get(repo_id).and_then(|poll| poll.error.clone())
    }

    pub fn poll_now(&self) {
        self.state.lock().poll_now = true;
    }

    pub(super) fn take_poll_now(&self) -> bool {
        let mut state = self.state.lock();
        let asked = std::mem::take(&mut state.poll_now);
        if asked {
            state.gh = None;
            for poll in state.polls.values_mut() {
                poll.next_at_ms = 0;
            }
        }
        asked
    }

    /// One look: GitHub status, each repo's labeled issues, then the runs that need work.
    pub fn tick(self: &Arc<Self>) {
        let cfg = self.io.store().config();
        if !cfg.enabled {
            return;
        }
        let gh = self.gh_status();
        let ready = gh.state == "ready";
        let me = gh.login.clone().filter(|_| ready).unwrap_or_default();
        let now = self.io.now_ms();
        if ready && !me.is_empty() {
            for repo in cfg.repos.iter().filter(|repo| !repo.paused) {
                self.poll_repo(repo, &cfg, &me, now);
            }
        }
        // Runs already under way finish even while GitHub is out of reach; new ones wait for it.
        self.resume(&cfg, &me);
        if ready && !me.is_empty() {
            self.drive_queue();
        }
    }

    /// `gh` ready or not, and the login (cached).
    pub(super) fn gh_status(&self) -> GhView {
        let now = self.io.now_ms();
        if let Some(cache) = &self.state.lock().gh {
            let ttl = if cache.login.is_some() { GH_GOOD_MS } else { GH_BAD_MS };
            if now.saturating_sub(cache.at_ms) < ttl {
                return GhView { state: cache.state.clone(), login: cache.login.clone() };
            }
        }
        let state = self.io.gh_state();
        let login = (state == "ready")
            .then(|| self.io.gh_api("user", None).ok())
            .flatten()
            .filter(|response| response.status == 200)
            .and_then(|response| github::parse_login(&response.body));
        self.state.lock().gh = Some(GhCache { state: state.clone(), login: login.clone(), at_ms: now });
        GhView { state, login }
    }

    // ------------------------------------------------------------------------- polling

    fn poll_repo(self: &Arc<Self>, repo: &RepoWatch, cfg: &Config, me: &str, now: u64) {
        if !valid_owner_repo(&repo.owner_repo) || !valid_label(&repo.label) {
            return;
        }
        let etag = {
            let mut state = self.state.lock();
            let poll = state.polls.entry(repo.id.clone()).or_default();
            if poll.next_at_ms > now {
                return;
            }
            poll.etag.clone()
        };
        let answer = self.io.gh_api(&github::issues_path(&repo.owner_repo, &repo.label), etag.as_deref());
        let response = match answer {
            Ok(response) if response.status == 200 || response.status == 304 => response,
            Ok(response) => {
                self.poll_failed(&repo.id, now, status_words(response.status));
                return;
            }
            Err(message) => {
                self.poll_failed(&repo.id, now, &clean_message(&message));
                return;
            }
        };
        let interval = response.poll_interval_s.unwrap_or(POLL_MIN_S).max(POLL_MIN_S);
        {
            let mut state = self.state.lock();
            let poll = state.polls.entry(repo.id.clone()).or_default();
            poll.next_at_ms = now + interval * 1000;
            poll.backoff_s = 0;
            poll.checked_ms = Some(now);
            poll.error = None;
            if response.status == 304 {
                return;
            }
            poll.etag = response.etag.clone();
        }
        let mut settled = true;
        for issue in github::parse_issues(&response.body).iter().take(github::ISSUES_PER_PAGE as usize) {
            settled &= self.look_at_issue(repo, cfg, me, issue, now);
        }
        if !settled {
            // Something could not be finished (a card could not be raised): ask GitHub in full next time.
            if let Some(poll) = self.state.lock().polls.get_mut(&repo.id) {
                poll.etag = None;
            }
        }
    }

    /// Back off x2 up to 15 minutes and remember the reason for the Settings row.
    fn poll_failed(&self, repo_id: &str, now: u64, reason: &str) {
        let mut state = self.state.lock();
        let poll = state.polls.entry(repo_id.to_string()).or_default();
        let next = poll.backoff_s.max(POLL_MIN_S).saturating_mul(2);
        poll.backoff_s = next.min(BACKOFF_MAX_S);
        poll.next_at_ms = now + poll.backoff_s * 1000;
        poll.error = Some(reason.to_string());
    }

    /// Read one listed issue. False when it could not be settled and must be looked at again.
    fn look_at_issue(self: &Arc<Self>, repo: &RepoWatch, cfg: &Config, me: &str, issue: &ApiIssue, now: u64) -> bool {
        let skip = {
            let state = self.state.lock();
            state.polls.get(&repo.id).and_then(|poll| poll.seen.get(&issue.number)).is_some_and(|seen| *seen == issue.updated_at)
        };
        let runs = self.io.store().runs();
        let waiting = runs
            .iter()
            .any(|run| run.repo_id == repo.id && run.issue.number == issue.number && !ends_story(&run.state));
        let ignored = cfg.ignored.iter().any(|(name, n)| *n == issue.number && name.eq_ignore_ascii_case(&repo.owner_repo));
        if skip || waiting || ignored {
            return true;
        }
        let Some(events) = self.fetch_events(&repo.owner_repo, issue.number) else {
            return true;
        };
        let settled = match github::latest_labeled(&events, &repo.label) {
            Some(event) => self.consider(repo, cfg, me, issue, &event, now),
            None => true,
        };
        if settled {
            if let Some(poll) = self.state.lock().polls.get_mut(&repo.id) {
                poll.seen.insert(issue.number, issue.updated_at.clone());
            }
        }
        settled
    }

    /// The label events of one issue as one JSON list. None = could not be read in full (fail closed).
    fn fetch_events(&self, owner_repo: &str, number: u64) -> Option<String> {
        let mut all: Vec<Value> = Vec::new();
        for page in 1..=github::EVENT_PAGES_MAX {
            let response = self.io.gh_api(&github::events_path(owner_repo, number, page), None).ok()?;
            if response.status != 200 {
                return None;
            }
            let Value::Array(items) = serde_json::from_str::<Value>(&response.body).ok()? else {
                return None;
            };
            let full = items.len() >= github::EVENTS_PER_PAGE as usize;
            all.extend(items);
            if !full {
                return Some(Value::Array(all).to_string());
            }
        }
        None
    }

    /// A labeled issue: skip it, raise the start card, or queue it. False when it must be retried.
    fn consider(self: &Arc<Self>, repo: &RepoWatch, cfg: &Config, me: &str, issue: &ApiIssue, ev: &LabelEvent, now: u64) -> bool {
        let runs = self.io.store().runs();
        let Some(decision) = filter::candidate(repo, cfg, issue, ev, &runs) else {
            return true;
        };
        let snapshot = text::snapshot(&repo.owner_repo, issue, ev);
        let new_run = |snapshot: IssueSnapshot, state: RunState, forced: Vec<String>| IssueRun {
            id: super::store::new_run_id(),
            repo_id: repo.id.clone(),
            issue: snapshot,
            agent: repo.agent.clone(),
            agent_label: self.io.agent_label(&repo.agent.agent_type),
            at_ms: now,
            started_at_ms: None,
            forced_ask: forced,
            approved_by: None,
            handle: None,
            branch: None,
            base: None,
            git_fingerprint: None,
            state,
        };
        if let Err(why) = decision {
            return self.save_new(&new_run(snapshot, RunState::Skipped { why }, Vec::new()));
        }
        let mut verdict = filter::trigger(me, cfg, ev, None);
        if matches!(verdict, Trigger::Refused { .. }) && cfg.write_access_counts && !ev.actor_is_bot && valid_login(&ev.actor) {
            let path = format!("repos/{}/collaborators/{}/permission", repo.owner_repo, ev.actor);
            let permission = self
                .io
                .gh_api(&path, None)
                .ok()
                .filter(|response| response.status == 200)
                .and_then(|response| github::parse_permission(&response.body));
            verdict = filter::trigger(me, cfg, ev, permission.as_deref());
        }
        if let Trigger::Refused { why } = verdict {
            return self.save_new(&new_run(snapshot, RunState::Skipped { why }, Vec::new()));
        }
        let forced = filter::forced_reasons(repo, me, cfg, &snapshot, &runs);
        let mut run = new_run(snapshot, RunState::Queued { why: "Waiting for a free slot".to_string() }, forced.clone());
        if repo.ask_before_start || !forced.is_empty() {
            let Some(question_id) = self.raise_start_card(&run) else {
                return false;
            };
            run.state = RunState::Asking { question_id };
            self.io.audit("asked", "auto", &format!("Asked about issue #{}", run.issue.number), run_detail(&run));
        } else {
            run.approved_by = Some("auto".to_string());
            self.io.audit("auto_approved", "auto", &format!("Queued issue #{}", run.issue.number), run_detail(&run));
        }
        self.save_new(&run)
    }

    fn save_new(&self, run: &IssueRun) -> bool {
        let saved = self.io.store().put(run).is_ok();
        if saved {
            self.io.changed();
        }
        saved
    }

    // ------------------------------------------------------------------------- cards

    /// The start card: `Start on #42 "..." with the agent?`. Returns the question id.
    pub(super) fn raise_start_card(&self, run: &IssueRun) -> Option<String> {
        let label: String = run.agent_label.chars().take(60).collect();
        let new = NewQuestion {
            question: text::card_question(&run.issue, &run.agent_label),
            choices: vec![
                choice(CHOICE_START, &format!("Start with {label}")),
                choice(CHOICE_NOT_NOW, "Not now"),
                choice(CHOICE_IGNORE, "Ignore this issue"),
            ],
            allow_free_text: Some(false),
            context: Some(text::card_context(&run.issue, &run.forced_ask)),
            timeout_s: Some(CARD_TIMEOUT_S),
            urgency: Some("normal".to_string()),
            kind: Some(CARD_KIND_START.to_string()),
            ..NewQuestion::default()
        };
        self.io.ask(new).map_err(|message| tracing::warn!(%message, "issue-intake: could not raise a card")).ok()
    }

    /// A plain card with one `Got it` button (nothing is acted on). Not a person-only kind.
    pub(super) fn raise_notice(&self, question: &str) {
        let new = NewQuestion {
            question: question.chars().take(300).collect(),
            choices: vec![choice(CHOICE_GOT_IT, "Got it")],
            allow_free_text: Some(false),
            timeout_s: Some(NOTICE_TIMEOUT_S),
            urgency: Some("normal".to_string()),
            ..NewQuestion::default()
        };
        let _ = self.io.ask(new);
    }

    /// A card of the push kind for `run`, remembered when it is not stored in the run's state.
    pub(super) fn raise_push_kind(
        &self,
        run: &IssueRun,
        question: &str,
        context: String,
        choices: Vec<Choice>,
        pane_id: Option<String>,
        follow_up: Option<CardKind>,
    ) -> Option<String> {
        let new = NewQuestion {
            pane_id,
            question: question.to_string(),
            choices,
            allow_free_text: Some(false),
            context: Some(context),
            timeout_s: Some(CARD_TIMEOUT_S),
            urgency: Some("normal".to_string()),
            kind: Some(CARD_KIND_PUSH.to_string()),
            ..NewQuestion::default()
        };
        let id = self.io.ask(new).map_err(|message| tracing::warn!(%message, "issue-intake: could not raise a card")).ok()?;
        if let Some(kind) = follow_up {
            self.state.lock().cards.insert(id.clone(), CardRef { run_id: run.id.clone(), kind });
        }
        Some(id)
    }

    /// Take a card down without it counting as an answer. The run must already point elsewhere.
    pub(super) fn withdraw(&self, question_id: &str) {
        self.state.lock().cards.remove(question_id);
        self.io.dismiss(question_id);
    }

    /// Take down every follow-up card of a run (its state moved on).
    pub(super) fn withdraw_cards_of(&self, run_id: &str) {
        let ids: Vec<String> = {
            let mut state = self.state.lock();
            let ids: Vec<String> = state.cards.iter().filter(|(_, card)| card.run_id == run_id).map(|(id, _)| id.clone()).collect();
            for id in &ids {
                state.cards.remove(id);
            }
            ids
        };
        for id in ids {
            self.io.dismiss(&id);
        }
    }

    // ------------------------------------------------------------------------- claims

    pub(super) fn claim(self: &Arc<Self>, run_id: &str) -> Option<Claim<I>> {
        let mut state = self.state.lock();
        if !state.busy.insert(run_id.to_string()) {
            return None;
        }
        Some(Claim { engine: self.clone(), id: run_id.to_string() })
    }

    pub(super) fn put(&self, run: &IssueRun) {
        if let Err(message) = self.io.store().put(run) {
            tracing::warn!(%message, "issue-intake: could not save a run");
        }
        self.io.changed();
    }

    pub(super) fn repo_of(&self, run: &IssueRun) -> Option<RepoWatch> {
        self.io.store().config().repos.into_iter().find(|repo| repo.id == run.repo_id)
    }

    // ------------------------------------------------------------------------- resume

    /// Runs that need work: a finished agent, a card that ran out, a card that vanished in a restart.
    /// Light checks run here; the long ones (the check before a push card, the push) get a thread.
    fn resume(self: &Arc<Self>, cfg: &Config, me: &str) {
        for run in self.io.store().runs() {
            let waits = matches!(
                run.state,
                RunState::Asking { .. }
                    | RunState::AskPush { .. }
                    | RunState::Running { .. }
                    | RunState::Checking
                    | RunState::Pushing
                    | RunState::Starting
            );
            if !waits {
                continue;
            }
            if let Some(claim) = self.claim(&run.id) {
                self.resume_one(claim, &run.id, cfg, me);
            }
        }
    }

    fn resume_one(self: &Arc<Self>, claim: Claim<I>, run_id: &str, cfg: &Config, me: &str) {
        let Some(mut run) = self.io.store().by_id(run_id) else { return };
        match run.state.clone() {
            RunState::Asking { question_id } => match self.io.question(&question_id) {
                Some(q) if q.state == QuestionState::Pending => {}
                Some(q) if q.state == QuestionState::TimedOut => {
                    run.state = RunState::Expired;
                    self.put(&run);
                }
                Some(q) => self.spawn_job(claim, run_id, Job::Answer(Box::new(q))),
                None => {
                    // The card was lost (a restart). Ask again, with the reasons recomputed.
                    if let Some(repo) = self.repo_of(&run) {
                        run.forced_ask = filter::forced_reasons(&repo, me, cfg, &run.issue, &self.io.store().runs());
                    }
                    if let Some(id) = self.raise_start_card(&run) {
                        run.state = RunState::Asking { question_id: id };
                        self.put(&run);
                    }
                }
            },
            RunState::AskPush { question_id, facts } => match self.io.question(&question_id) {
                Some(q) if q.state == QuestionState::Pending => {}
                Some(q) if q.state == QuestionState::TimedOut => {
                    run.state = RunState::Held {
                        code: ERR_PUSH_HELD.to_string(),
                        message: "The card timed out. Nothing was pushed.".to_string(),
                    };
                    self.put(&run);
                }
                Some(q) => self.spawn_job(claim, run_id, Job::Answer(Box::new(q))),
                None => {
                    if let Some(id) = self.reraise_push_card(&run, &facts) {
                        run.state = RunState::AskPush { question_id: id, facts };
                        self.put(&run);
                    }
                }
            },
            RunState::Running { handle, .. } => {
                let Some(repo) = self.repo_of(&run) else { return };
                match self.io.run_snapshot(&repo.folder, &handle) {
                    Some(snap) if snap.terminal => self.spawn_job(claim, run_id, Job::Finish),
                    Some(_) => {}
                    None => self.fail(
                        &mut run,
                        ERR_START_FAILED,
                        "Puppet Master lost track of this run. Open the copy to see what it did.",
                    ),
                }
            }
            RunState::Checking => self.spawn_job(claim, run_id, Job::Finish),
            RunState::Pushing => self.spawn_job(claim, run_id, Job::Push),
            RunState::Starting => {
                // A start that a restart cut off. Nothing was handed to an agent that we know of.
                run.started_at_ms = None;
                self.fail(&mut run, ERR_START_FAILED, "Puppet Master restarted while this was starting. Ask again.");
            }
            _ => {}
        }
    }

    /// Hand a claimed run to a thread of its own. The claim is released when the job ends.
    pub(super) fn spawn_job(self: &Arc<Self>, claim: Claim<I>, run_id: &str, job: Job) {
        let this = self.clone();
        let run_id = run_id.to_string();
        self.io.spawn(
            "issue-intake-job",
            Box::new(move || {
                match job {
                    Job::Finish => this.finish_job(&run_id),
                    Job::Push => this.push_job(&run_id),
                    Job::Answer(question) => {
                        if let Some(run) = this.io.store().by_id(&run_id) {
                            this.apply_card_answer(run, &question);
                        }
                    }
                }
                drop(claim);
            }),
        );
    }
}

/// What a job does for a claimed run (see `spawn_job`).
#[derive(Debug, Clone)]
pub(super) enum Job {
    Answer(Box<Question>),
    Finish,
    Push,
}

pub(super) fn choice(id: &str, label: &str) -> Choice {
    Choice { id: id.to_string(), label: label.to_string() }
}

/// The audit detail for a run: ids and numbers, never the issue text.
pub(super) fn run_detail(run: &IssueRun) -> Value {
    json!({ "run": run.id, "repo": run.issue.owner_repo, "issue": run.issue.number, "agent": run.agent.key() })
}

fn status_words(status: u16) -> &'static str {
    match status {
        401 | 403 => "GitHub did not accept the request. Check gh auth status.",
        404 => "GitHub cannot find this repository, or you cannot see it.",
        429 => "GitHub asked for a pause. Trying again soon.",
        _ => "GitHub gave an unexpected answer. Trying again soon.",
    }
}
