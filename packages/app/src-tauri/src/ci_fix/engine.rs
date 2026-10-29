//! The engine of "Red to Green" (SPEC n33 section 3.5 A to C): the watch loop, the offer cards and the
//! restart. It talks to the world only through [`Io`], so the tests drive it with a fake and a virtual clock.
//!
//! - A tick (every 20 seconds, only while the Labs switch is on) takes each watched pull request that is due:
//!   it reads the pull request and the checks of its head commit through the person's `gh` login, asks
//!   `filter` whether Puppet Master owns the pull request and whether a failure may raise a card, and raises
//!   one card per failing commit. It then picks up fixes that need work (`resume`).
//! - A card answer (`answers.rs`) or a Settings button changes one saved fix. The work (`flows.rs`: start, finish,
//!   push) runs on its own thread.
//! - One thread at most works on one watch or one fix: an id is "claimed" while a job holds it.
//!
//! Safety rules kept here: an answer acts only on a question id stored in a fix (or one of our own follow-up
//! cards), never on a card's kind or text. Nothing here writes to a pane or to GitHub. Every GitHub read fails
//! closed: a read that fails or a pull request that is not ours raises no card. The store lock is never held
//! across `gh` or `git`. A fix's log text is only ever the cleaned snapshot (`logs`), never the task.

use super::filter::{self, Offer};
use super::github::{self, ApiCheck, ApiPull, CiRead};
use super::live::Io;
use super::model::*;
use super::store::{new_fix_id, new_watch_id};
use super::text;
use super::view::GhView;
use crate::issue_intake::model::valid_owner_repo;
use crate::questions::{Choice, NewQuestion, Question, QuestionState};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// How long a good `gh` answer is trusted; a bad one is asked again after a minute.
const GH_GOOD_MS: u64 = 10 * 60 * 1000;
const GH_BAD_MS: u64 = 60 * 1000;
/// The repo's default branch is asked again after this long (or 5 minutes after a failed ask).
const DEFAULT_BRANCH_MS: u64 = 60 * 60 * 1000;
const DEFAULT_BRANCH_RETRY_MS: u64 = 5 * 60 * 1000;
/// A plain notice is not a person-only card, so it may stay open for an hour at most.
const NOTICE_TIMEOUT_S: u64 = 3600;
const FIRST_LOOK_DELAY: Duration = Duration::from_secs(3);
const SLICE: Duration = Duration::from_millis(250);
/// Extra choices of our own follow-up cards.
pub(super) const CHOICE_RETRY: &str = "retry";
pub(super) const CHOICE_GOT_IT: &str = "got_it";

/// A card of ours that is not stored in a fix's state (it only offers a follow-up).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CardKind {
    /// The agent finished with no changes: `Open the agent` / `Got it`.
    NoChanges,
    /// A guard stopped the push: `Look first` / `Don't push`.
    Held,
    /// The push failed: `Try again` / `Got it`.
    RetryPush,
    /// The tries are used up: `Open the copy` / `Stop watching`.
    TriesUsed,
}

#[derive(Debug, Clone)]
pub(super) struct CardRef {
    pub fix_id: String,
    pub kind: CardKind,
}

#[derive(Debug, Clone)]
struct GhCache {
    state: String,
    login: String,
    at_ms: u64,
}

/// What the poller remembers about one watched pull request (in memory only; a restart looks again).
#[derive(Debug, Default)]
pub(super) struct WatchPoll {
    pub last_poll_ms: u64,
    pub backoff_s: u64,
    /// Checks still run (or are expected to start), so the next look is soon.
    pub pending: bool,
    pull_etag: Option<String>,
    pull: Option<ApiPull>,
    checks_etag: Option<String>,
    checks_sha: Option<String>,
    checks: Option<Vec<ApiCheck>>,
    /// (commit, when its first failure was seen).
    first_failure: Option<(String, u64)>,
    /// One plain sentence from the last look that replaces "Watching CI" on the row.
    pub note: Option<String>,
    /// The repo's default branch and when it was asked.
    default_branch: Option<(Option<String>, u64)>,
    pub error: Option<String>,
}

#[derive(Default)]
pub(super) struct State {
    gh: Option<GhCache>,
    pub(super) polls: HashMap<String, WatchPoll>,
    pub(super) busy: HashSet<String>,
    pub(super) cards: HashMap<String, CardRef>,
    poll_now: bool,
}

pub struct Engine<I: Io> {
    pub(super) io: I,
    pub(super) state: Mutex<State>,
    pub(super) queue: Mutex<()>,
    /// One writer at a time for read-modify-write of a watch or a fix record.
    pub(super) edit: Mutex<()>,
}

/// A watch or a fix is being worked on by one job. Dropping it frees the id.
pub(super) struct Claim<I: Io + 'static> {
    engine: Arc<Engine<I>>,
    id: String,
}

impl<I: Io + 'static> Drop for Claim<I> {
    fn drop(&mut self) {
        self.engine.state.lock().busy.remove(&self.id);
    }
}

/// A card of the CI kinds (`ci_fix`, `ci_push`).
pub(super) fn is_ci_card(question: &Question) -> bool {
    matches!(question.kind.as_deref(), Some(CARD_KIND_FIX | CARD_KIND_PUSH))
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

/// A paused or stopped reason in the form the row text expects: it follows `Paused: ` or `Stopped: `, so it
/// starts lower case and has no full stop (`someone else pushed to this branch`). A first word that is not a
/// plain capitalised word (`GitHub`, `PR`) keeps its case.
pub(super) fn reason(text: &str) -> String {
    let flat = clean_message(text);
    let trimmed = flat.trim().trim_end_matches('.').trim();
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else { return String::new() };
    let first_word: String = trimmed.chars().take_while(|c| !c.is_whitespace()).collect();
    let plain_word = first.is_uppercase() && first_word.chars().skip(1).all(|c| !c.is_uppercase());
    if plain_word {
        format!("{}{}", first.to_lowercase(), chars.as_str())
    } else {
        trimmed.to_string()
    }
}

/// A fix in these states uses the copy or an open card: the copy's head may be ahead of the pull request.
pub(super) fn in_flight(state: &FixState) -> bool {
    matches!(
        state,
        FixState::Queued { .. }
            | FixState::Starting
            | FixState::Running { .. }
            | FixState::Checking
            | FixState::AskPush { .. }
            | FixState::Pushing
    )
}

/// A read of GitHub that did not work. A transient one waits and tries again; a hard one (GitHub says no, or
/// says something Puppet Master cannot read) pauses the watch.
enum Failure {
    Transient(String),
    Hard(String),
}

pub(super) fn choice(id: &str, label: &str) -> Choice {
    Choice { id: id.to_string(), label: label.to_string() }
}

/// The audit detail for a fix: ids and numbers, never log text.
pub(super) fn fix_detail(w: &Watch, fix: &FixRun) -> serde_json::Value {
    serde_json::json!({ "fix": fix.id, "watch": w.id, "repo": w.owner_repo, "pr": w.pr_number, "try": fix.try_no })
}

fn watch_detail(w: &Watch) -> serde_json::Value {
    serde_json::json!({ "watch": w.id, "repo": w.owner_repo, "pr": w.pr_number })
}

impl<I: Io + 'static> Engine<I> {
    pub fn new(io: I) -> Self {
        Self { io, state: Mutex::new(State::default()), queue: Mutex::new(()), edit: Mutex::new(()) }
    }

    pub fn io(&self) -> &I {
        &self.io
    }

    // ------------------------------------------------------------------------- the loop

    /// The `ci-fix` thread body: a look every 20 seconds, sooner after `poll_now`.
    pub fn run_loop(self: Arc<Self>) {
        std::thread::sleep(FIRST_LOOK_DELAY);
        loop {
            let this = self.clone();
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || this.tick()));
            if outcome.is_err() {
                tracing::warn!("ci-fix: a look at GitHub failed unexpectedly");
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

    pub fn poll_now(&self) {
        self.state.lock().poll_now = true;
    }

    pub(super) fn take_poll_now(&self) -> bool {
        let mut state = self.state.lock();
        let asked = std::mem::take(&mut state.poll_now);
        if asked {
            state.gh = None;
            for poll in state.polls.values_mut() {
                poll.last_poll_ms = 0;
                poll.backoff_s = 0;
            }
        }
        asked
    }

    /// One look: GitHub status, each due watch, then the fixes that need work.
    pub fn tick(self: &Arc<Self>) {
        let settings = self.io.store().settings();
        if !settings.enabled {
            return;
        }
        let gh = self.gh_status();
        let ready = gh.state == "ready" && !gh.login.is_empty();
        let now = self.io.now_ms();
        if ready {
            for watch in self.io.store().watches() {
                if matches!(watch.state, WatchState::Watching) {
                    self.poll_watch(&watch, &gh.login, &settings, now);
                }
            }
        }
        // Fixes already under way finish even while GitHub is out of reach; new work waits for it.
        self.resume(&settings, &gh.login);
        if ready {
            self.drive_queue();
        }
    }

    /// `gh` ready or not, and the login (cached).
    pub(super) fn gh_status(&self) -> GhView {
        let now = self.io.now_ms();
        if let Some(cache) = &self.state.lock().gh {
            let ttl = if cache.login.is_empty() { GH_BAD_MS } else { GH_GOOD_MS };
            if now.saturating_sub(cache.at_ms) < ttl {
                return GhView { state: cache.state.clone(), login: cache.login.clone() };
            }
        }
        let state = self.io.gh_state();
        let login = (state == "ready")
            .then(|| self.io.gh_api("user", None).ok())
            .flatten()
            .filter(|response| response.status == 200)
            .and_then(|response| crate::issue_intake::github::parse_login(&response.body))
            .unwrap_or_default();
        self.state.lock().gh = Some(GhCache { state: state.clone(), login: login.clone(), at_ms: now });
        GhView { state, login }
    }

    // ------------------------------------------------------------------------- claims and edits

    pub(super) fn claim(self: &Arc<Self>, id: &str) -> Option<Claim<I>> {
        let mut state = self.state.lock();
        if !state.busy.insert(id.to_string()) {
            return None;
        }
        Some(Claim { engine: self.clone(), id: id.to_string() })
    }

    pub(super) fn put_fix(&self, fix: &FixRun) {
        if let Err(message) = self.io.store().put_fix(fix) {
            tracing::warn!(%message, "ci-fix: could not save a fix");
        }
        self.io.changed();
    }

    pub(super) fn put_watch(&self, watch: &Watch) -> bool {
        let saved = self.io.store().put_watch(watch);
        if let Err(message) = &saved {
            tracing::warn!(%message, "ci-fix: could not save a watch");
        }
        self.io.changed();
        saved.is_ok()
    }

    /// Change one watch as it is stored now (never a stale copy).
    pub(super) fn edit_watch(&self, id: &str, change: impl FnOnce(&mut Watch)) -> Option<Watch> {
        let _turn = self.edit.lock();
        let mut watch = self.io.store().watch_by_id(id)?;
        change(&mut watch);
        self.put_watch(&watch).then_some(watch)
    }

    /// Change one fix as it is stored now (never a stale copy).
    pub(super) fn edit_fix(&self, id: &str, change: impl FnOnce(&mut FixRun)) -> Option<FixRun> {
        let _turn = self.edit.lock();
        let mut fix = self.io.store().fix_by_id(id)?;
        change(&mut fix);
        self.put_fix(&fix);
        Some(fix)
    }

    fn with_poll<T>(&self, watch_id: &str, f: impl FnOnce(&mut WatchPoll) -> T) -> T {
        let mut state = self.state.lock();
        f(state.polls.entry(watch_id.to_string()).or_default())
    }

    // ------------------------------------------------------------------------- polling

    /// Look at one watch if it is due. Claimed for the whole look.
    fn poll_watch(self: &Arc<Self>, watch: &Watch, me: &str, settings: &Settings, now: u64) {
        let due = self.with_poll(&watch.id, |poll| {
            filter::due(watch, poll.last_poll_ms, poll.pending, poll.backoff_s, now)
        });
        if !due {
            return;
        }
        let Some(_claim) = self.claim(&watch.id) else { return };
        self.poll_core(&watch.id, me, settings, now, false);
    }

    /// Look at a watch once (the tick, or a person's `Fix with` / `Ask again`). With `force`, a fix that was
    /// declined or ended for this commit does not stop a new card (the person asked), but every cap still holds.
    /// Returns what the offer rules said for a failure; `None` when there was no failure to decide on.
    pub(super) fn poll_core(&self, watch_id: &str, me: &str, s: &Settings, now: u64, force: bool) -> Option<Offer> {
        let watch = self.io.store().watch_by_id(watch_id)?;
        if !matches!(watch.state, WatchState::Watching) {
            return None;
        }
        self.with_poll(&watch.id, |poll| {
            poll.last_poll_ms = now;
            poll.note = None;
        });
        if filter::expired(&watch, now) {
            self.stop(&watch, "No push for 14 days");
            return None;
        }
        let Some(copy) = self.io.find_copy(&watch.project, &watch.handle) else {
            self.stop(&watch, "The copy is gone");
            return None;
        };
        let pull = match self.read_pull(&watch) {
            Ok(pull) => pull,
            Err(Failure::Transient(message)) => return self.read_failed(&watch.id, now, &message, false),
            Err(Failure::Hard(message)) => return self.read_failed(&watch.id, now, &message, true),
        };
        if pull.merged {
            self.finish_watch(&watch, WatchState::Merged);
            return None;
        }
        if pull.state != "open" {
            self.finish_watch(&watch, WatchState::Closed);
            return None;
        }
        // Every poll: GitHub, the folder's origin and the copy's head must all agree.
        if !self.io.folder_owner_repo(&watch.project).is_some_and(|name| name.eq_ignore_ascii_case(&watch.owner_repo)) {
            self.pause(&watch, CI_FIX_NOT_YOURS, "The folder no longer points at this repository.");
            return None;
        }
        let default_branch = self.default_branch(&watch, &copy, now);
        if let Err((code, message)) = filter::owns(&watch, me, &pull, default_branch.as_deref()) {
            let why = if code == CI_FIX_HEAD_MOVED { "Someone else pushed to this branch".to_string() } else { message };
            self.pause(&watch, code, &why);
            return None;
        }
        let fixes = self.io.store().fixes();
        let busy = fixes.iter().any(|fix| fix.watch_id == watch.id && in_flight(&fix.state));
        if !busy {
            let env = match self.io.git_env() {
                Ok(env) => env,
                Err(error) => return self.read_failed(&watch.id, now, &error.message, false),
            };
            let head = match self.io.head_sha(&copy, &env) {
                Ok(head) => head,
                Err(error) => return self.read_failed(&watch.id, now, &error.message, false),
            };
            if head != pull.head_sha {
                // A copy that is ahead of the pull request holds work that was not pushed: no card for it.
                match self.io.is_ancestor(&copy, &pull.head_sha, &head, &env) {
                    Ok(true) => {
                        self.with_poll(&watch.id, |poll| {
                            poll.note = Some("The copy has changes that aren't pushed.".to_string());
                            poll.pending = false;
                        });
                    }
                    _ => self.pause(&watch, CI_FIX_HEAD_MOVED, "The copy and the pull request are not the same"),
                }
                return None;
            }
        }
        let checks = match self.read_checks(&watch, &pull.head_sha) {
            Ok(checks) => checks,
            Err(Failure::Transient(message)) => return self.read_failed(&watch.id, now, &message, false),
            Err(Failure::Hard(message)) => return self.read_failed(&watch.id, now, &message, true),
        };
        self.with_poll(&watch.id, |poll| {
            poll.backoff_s = 0;
            poll.error = None;
        });
        self.act_on_checks(&watch, &pull.head_sha, &github::read_ci(&checks), s, now, force)
    }

    /// What the checks of the pull request's head say, and what that leads to.
    fn act_on_checks(&self, watch: &Watch, sha: &str, read: &CiRead, s: &Settings, now: u64, force: bool) -> Option<Offer> {
        let mut offer = None;
        let mut pending = false;
        match read {
            CiRead::Passed => self.mark_pushed_fix(watch, sha, true),
            CiRead::Running => pending = true,
            CiRead::NoChecks => pending = now.saturating_sub(watch.last_push_ms) < HUNG_CHECK_MS,
            CiRead::Waiting { why } => {
                let note = why.clone();
                self.with_poll(&watch.id, |poll| poll.note = Some(note));
            }
            CiRead::Failed { checks, still_running } => {
                pending = *still_running;
                self.mark_pushed_fix(watch, sha, false);
                let first_failure = self.with_poll(&watch.id, |poll| match &poll.first_failure {
                    Some((seen, at)) if seen == sha => *at,
                    _ => {
                        poll.first_failure = Some((sha.to_string(), now));
                        now
                    }
                });
                let mut fixes = self.io.store().fixes();
                if force {
                    fixes.retain(|fix| !(fix.watch_id == watch.id && fix.failing_sha == sha && reaskable(&fix.state)));
                }
                let decision = filter::should_offer(watch, read, sha, &fixes, s, now, first_failure);
                match decision {
                    Offer::Card => {
                        self.raise_offer(watch, sha, checks, first_failure, now);
                    }
                    Offer::TriesUsed => self.tries_used(watch, sha, now),
                    Offer::DayCap => {
                        let note = format!("Waiting: {} today already", count(s.fixes_per_day, "fix", "fixes"));
                        self.with_poll(&watch.id, |poll| poll.note = Some(note));
                    }
                    Offer::Nothing => {}
                }
                offer = Some(decision);
            }
        }
        self.with_poll(&watch.id, |poll| poll.pending = pending);
        offer
    }

    /// A pushed fix whose commit is now the pull request's head: green, or still red. Skipped when the fix is
    /// being worked on by another job.
    fn mark_pushed_fix(&self, watch: &Watch, sha: &str, green: bool) {
        let Some(fix) = self
            .io
            .store()
            .fixes()
            .into_iter()
            .find(|fix| fix.watch_id == watch.id && matches!(&fix.state, FixState::Pushed { sha: pushed } if pushed == sha))
        else {
            return;
        };
        let Some(_claim) = self.claim_ref(&fix.id) else { return };
        let state = if green { FixState::Green { sha: sha.to_string() } } else { FixState::StillRed { sha: sha.to_string() } };
        if let Some(done) = self.edit_fix(&fix.id, |fix| fix.state = state) {
            if green {
                self.io.audit("green", "auto", &format!("CI passed on #{}", watch.pr_number), fix_detail(watch, &done));
            }
        }
    }

    /// Like [`Self::claim`], from a place that has no `Arc<Self>` (the claim lives for this call only).
    fn claim_ref(&self, id: &str) -> Option<RefClaim<'_, I>> {
        let mut state = self.state.lock();
        if !state.busy.insert(id.to_string()) {
            return None;
        }
        Some(RefClaim { engine: self, id: id.to_string() })
    }

    fn read_failed(&self, watch_id: &str, now: u64, message: &str, hard: bool) -> Option<Offer> {
        let message = clean_message(message);
        self.with_poll(watch_id, |poll| {
            poll.backoff_s = poll.backoff_s.max(POLL_PENDING_S).saturating_mul(2).min(BACKOFF_MAX_S);
            poll.last_poll_ms = now;
            poll.error = Some(message.clone());
        });
        if hard {
            if let Some(watch) = self.io.store().watch_by_id(watch_id) {
                self.pause(&watch, CI_FIX_NOT_YOURS, &message);
            }
        }
        None
    }

    /// The pull request, read with an ETag. A `304` gives the one read before.
    fn read_pull(&self, watch: &Watch) -> Result<ApiPull, Failure> {
        if !valid_owner_repo(&watch.owner_repo) || watch.pr_number == 0 {
            return Err(Failure::Hard("Puppet Master can't tell which pull request this is".to_string()));
        }
        let etag = self.with_poll(&watch.id, |poll| poll.pull_etag.clone().filter(|_| poll.pull.is_some()));
        let response = self
            .io
            .gh_api(&github::pull_path(&watch.owner_repo, watch.pr_number), etag.as_deref())
            .map_err(Failure::Transient)?;
        match response.status {
            304 => self
                .with_poll(&watch.id, |poll| poll.pull.clone())
                .ok_or_else(|| Failure::Transient("GitHub's answer could not be matched".to_string())),
            200 => {
                let pull = github::parse_pull(&response.body)
                    .ok_or_else(|| Failure::Hard("GitHub's pull request answer couldn't be read".to_string()))?;
                self.with_poll(&watch.id, |poll| {
                    poll.pull_etag = response.etag.clone();
                    poll.pull = Some(pull.clone());
                });
                Ok(pull)
            }
            401 | 403 | 404 | 410 => Err(Failure::Hard("GitHub didn't let Puppet Master read this pull request".to_string())),
            other => Err(Failure::Transient(format!("GitHub answered {other}"))),
        }
    }

    /// The check runs of one commit, read with an ETag (kept only for the same commit).
    fn read_checks(&self, watch: &Watch, sha: &str) -> Result<Vec<ApiCheck>, Failure> {
        let path = github::checks_path(&watch.owner_repo, sha)
            .ok_or_else(|| Failure::Hard("Puppet Master can't tell which commit this is".to_string()))?;
        let etag = self.with_poll(&watch.id, |poll| {
            poll.checks_etag.clone().filter(|_| poll.checks_sha.as_deref() == Some(sha) && poll.checks.is_some())
        });
        let response = self.io.gh_api(&path, etag.as_deref()).map_err(Failure::Transient)?;
        match response.status {
            304 => self
                .with_poll(&watch.id, |poll| poll.checks.clone())
                .ok_or_else(|| Failure::Transient("GitHub's answer could not be matched".to_string())),
            200 => {
                let checks = github::parse_check_runs(&response.body)
                    .ok_or_else(|| Failure::Transient("GitHub's check answer couldn't be read".to_string()))?;
                self.with_poll(&watch.id, |poll| {
                    poll.checks_etag = response.etag.clone();
                    poll.checks_sha = Some(sha.to_string());
                    poll.checks = Some(checks.clone());
                });
                Ok(checks)
            }
            401 | 403 | 404 | 410 => Err(Failure::Hard("GitHub didn't let Puppet Master read the checks".to_string())),
            other => Err(Failure::Transient(format!("GitHub answered {other}"))),
        }
    }

    /// The repo's default branch (cached). A failed ask gives `None`: the `pm/` rules still hold.
    fn default_branch(&self, watch: &Watch, copy: &crate::worktrees::WorktreeRef, now: u64) -> Option<String> {
        let cached = self.with_poll(&watch.id, |poll| poll.default_branch.clone());
        if let Some((branch, at)) = cached {
            let ttl = if branch.is_some() { DEFAULT_BRANCH_MS } else { DEFAULT_BRANCH_RETRY_MS };
            if now.saturating_sub(at) < ttl {
                return branch;
            }
        }
        let branch = match self.io.repo_facts(Path::new(&copy.path)) {
            Ok(facts) if facts.name_with_owner.eq_ignore_ascii_case(&watch.owner_repo) => facts.default_branch,
            _ => None,
        };
        self.with_poll(&watch.id, |poll| poll.default_branch = Some((branch.clone(), now)));
        branch
    }

    // ------------------------------------------------------------------------- watch life

    /// GitHub disagrees (or a read was refused): the watch pauses, with a reason the row can show. No card.
    pub(super) fn pause(&self, watch: &Watch, code: &str, why: &str) {
        let why = reason(why);
        let state = WatchState::Paused { why: why.clone() };
        self.end_cards_of(&watch.id);
        if self.edit_watch(&watch.id, |w| w.state = state).is_some() {
            let mut detail = watch_detail(watch);
            detail["code"] = serde_json::Value::from(code);
            self.io.audit("paused", "auto", &format!("Paused watching #{}: {why}", watch.pr_number), detail);
        }
    }

    /// The watch ends: the person, a cap, 14 days, or the copy is gone. Its open cards go.
    pub(super) fn stop(&self, watch: &Watch, why: &str) {
        let why = reason(why);
        self.end_cards_of(&watch.id);
        if self.edit_watch(&watch.id, |w| w.state = WatchState::Stopped { why: why.clone() }).is_some() {
            self.io.audit("stopped", "auto", &format!("Stopped watching #{}: {why}", watch.pr_number), watch_detail(watch));
        }
    }

    /// The pull request was merged or closed.
    fn finish_watch(&self, watch: &Watch, state: WatchState) {
        self.end_cards_of(&watch.id);
        self.edit_watch(&watch.id, |w| w.state = state);
    }

    /// Take down every open card of a watch. The fix is moved on first, so the dismissal is not an answer.
    pub(super) fn end_cards_of(&self, watch_id: &str) {
        let mut to_dismiss: Vec<String> = Vec::new();
        for fix in self.io.store().fixes().into_iter().filter(|fix| fix.watch_id == watch_id) {
            let next = match &fix.state {
                FixState::Asking { question_id } => {
                    to_dismiss.push(question_id.clone());
                    FixState::Declined
                }
                FixState::AskPush { question_id, .. } => {
                    to_dismiss.push(question_id.clone());
                    FixState::NotPushed
                }
                FixState::Queued { .. } => FixState::Declined,
                _ => continue,
            };
            self.edit_fix(&fix.id, |f| f.state = next);
        }
        let follow_ups: Vec<String> = {
            let fixes: HashSet<String> =
                self.io.store().fixes().into_iter().filter(|fix| fix.watch_id == watch_id).map(|fix| fix.id).collect();
            let mut state = self.state.lock();
            let ids: Vec<String> = state
                .cards
                .iter()
                .filter(|(_, card)| fixes.contains(&card.fix_id))
                .map(|(id, _)| id.clone())
                .collect();
            for id in &ids {
                state.cards.remove(id);
            }
            ids
        };
        for id in to_dismiss.into_iter().chain(follow_ups) {
            self.io.dismiss(&id);
        }
    }

    // ------------------------------------------------------------------------- C. offer

    /// A failure on a Puppet Master commit: read the logs of the failing jobs (at most three), clean them,
    /// and raise one card. The fix is stored as `Asking` BEFORE the card can be seen.
    fn raise_offer(&self, watch: &Watch, sha: &str, checks: &[FailedCheck], first_failure_ms: u64, now: u64) -> Option<String> {
        let shown = filter::shown_checks(checks).to_vec();
        let mut jobs: Vec<(String, Option<String>)> = Vec::with_capacity(shown.len());
        let mut read = 0;
        for check in &shown {
            let raw = match check.job_id {
                Some(job) if read < JOBS_READ_MAX => {
                    read += 1;
                    // An error leaves the job without a log; its text is never kept.
                    self.io.job_log(&github::job_log_path(&watch.owner_repo, job)).ok()
                }
                _ => None,
            };
            jobs.push((check.name.clone(), raw));
        }
        let log = super::logs::clean_jobs_for_store(&jobs);
        let question_id = uuid::Uuid::new_v4().to_string();
        let mut fix = FixRun {
            id: new_fix_id(),
            watch_id: watch.id.clone(),
            try_no: watch.tries_used + 1,
            failing_sha: sha.to_string(),
            checks: shown,
            log,
            at_ms: now,
            first_failure_ms,
            approved_by: None,
            git_fingerprint: None,
            state: FixState::Asking { question_id: question_id.clone() },
        };
        if let Err(message) = self.io.store().put_fix(&fix) {
            tracing::warn!(%message, "ci-fix: could not save a fix");
            return None;
        }
        self.io.changed();
        if self.raise_fix_card(watch, &mut fix, Some(question_id)).is_some() {
            self.io.audit("asked", "auto", &format!("Asked about CI on #{}", watch.pr_number), fix_detail(watch, &fix));
        }
        Some(fix.id)
    }

    /// The start card: `CI failed on #61 (lint, test). Ask <agent> to fix it?`. `id` is the id the fix
    /// already holds; `None` lets the question store pick one (the caller then stores it).
    pub(super) fn raise_fix_card(&self, watch: &Watch, fix: &mut FixRun, id: Option<String>) -> Option<String> {
        let settings = self.io.store().settings();
        let facts = self.io.run_snapshot(&watch.project, &watch.handle);
        let same_pane = facts.as_ref().is_some_and(|facts| route_a_ok(facts));
        let pane_id = facts.as_ref().filter(|facts| facts.pane_open).and_then(|facts| facts.pane_id.clone());
        let label: String = watch.agent_label.chars().take(60).collect();
        let new = NewQuestion {
            pane_id,
            question: text::card_question(watch.pr_number, &fix.checks, &watch.agent_label),
            choices: vec![
                choice(CHOICE_FIX, &format!("Ask {label} to fix")),
                choice(CHOICE_NOT_NOW, "Not now"),
                choice(CHOICE_STOP, "Stop watching this PR"),
            ],
            allow_free_text: Some(false),
            context: Some(text::card_context(watch, fix, same_pane, settings.tries_per_pr)),
            timeout_s: Some(CARD_TIMEOUT_S),
            urgency: Some("normal".to_string()),
            kind: Some(CARD_KIND_FIX.to_string()),
            ..NewQuestion::default()
        };
        let asked = match &id {
            Some(id) => self.io.ask_as(new, id, Some(&fix.id)),
            None => self.io.ask(new, Some(&fix.id)),
        };
        let asked = asked.map_err(|message| tracing::warn!(%message, "ci-fix: could not raise a card")).ok()?;
        if id.is_none() {
            fix.state = FixState::Asking { question_id: asked.clone() };
            self.put_fix(fix);
        }
        Some(asked)
    }

    /// The tries are used up: one notice for this commit, with no fix button. A marker fix is stored first, so
    /// the notice is raised once per commit even after a restart.
    fn tries_used(&self, watch: &Watch, sha: &str, now: u64) {
        let message = text::tries_used_notice(watch.pr_number, watch.tries_used);
        let marker = FixRun {
            id: new_fix_id(),
            watch_id: watch.id.clone(),
            try_no: watch.tries_used,
            failing_sha: sha.to_string(),
            checks: Vec::new(),
            log: LogSnapshot::default(),
            at_ms: now,
            first_failure_ms: now,
            approved_by: None,
            git_fingerprint: None,
            state: FixState::Failed { code: CI_FIX_TRIES_USED.to_string(), message: message.clone() },
        };
        if self.io.store().put_fix(&marker).is_err() {
            return;
        }
        self.io.changed();
        let pane_id = self.pane_of(watch);
        self.raise_follow_up(
            &marker,
            CARD_KIND_FIX,
            &message,
            format!("{} · #{}", watch.owner_repo, watch.pr_number),
            vec![choice(CHOICE_LOOK, "Open the copy"), choice(CHOICE_STOP, "Stop watching")],
            pane_id,
            CardKind::TriesUsed,
        );
    }

    // ------------------------------------------------------------------------- cards

    /// A card of ours that is remembered only in memory (a follow-up: it offers `Look first`, `Try again`...).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn raise_follow_up(
        &self,
        fix: &FixRun,
        kind: &str,
        question: &str,
        context: String,
        choices: Vec<Choice>,
        pane_id: Option<String>,
        follow_up: CardKind,
    ) -> Option<String> {
        let new = NewQuestion {
            pane_id,
            question: question.chars().take(300).collect(),
            choices,
            allow_free_text: Some(false),
            context: Some(context.chars().filter(|c| !c.is_control() || *c == '\n').take(1000).collect()),
            timeout_s: Some(CARD_TIMEOUT_S),
            urgency: Some("normal".to_string()),
            kind: Some(kind.to_string()),
            ..NewQuestion::default()
        };
        let id = self.io.ask(new, Some(&fix.id)).map_err(|message| tracing::warn!(%message, "ci-fix: could not raise a card")).ok()?;
        self.state.lock().cards.insert(id.clone(), CardRef { fix_id: fix.id.clone(), kind: follow_up });
        Some(id)
    }

    /// A plain card with one `Got it` button (nothing is acted on). Not a person-only kind, and it carries no fix.
    pub(super) fn raise_notice(&self, question: &str) {
        let new = NewQuestion {
            question: question.chars().take(300).collect(),
            choices: vec![choice(CHOICE_GOT_IT, "Got it")],
            allow_free_text: Some(false),
            timeout_s: Some(NOTICE_TIMEOUT_S),
            urgency: Some("normal".to_string()),
            ..NewQuestion::default()
        };
        let _ = self.io.ask(new, None);
    }

    /// Take a card down without it counting as an answer. The fix must already point elsewhere.
    pub(super) fn withdraw(&self, question_id: &str) {
        self.state.lock().cards.remove(question_id);
        self.io.dismiss(question_id);
    }

    /// Take down every follow-up card of a fix (its state moved on).
    pub(super) fn withdraw_cards_of(&self, fix_id: &str) {
        let ids: Vec<String> = {
            let mut state = self.state.lock();
            let ids: Vec<String> = state.cards.iter().filter(|(_, card)| card.fix_id == fix_id).map(|(id, _)| id.clone()).collect();
            for id in &ids {
                state.cards.remove(id);
            }
            ids
        };
        for id in ids {
            self.io.dismiss(&id);
        }
    }

    /// The pane the agent works in, while it is open.
    pub(super) fn pane_of(&self, watch: &Watch) -> Option<String> {
        let facts = self.io.run_snapshot(&watch.project, &watch.handle)?;
        facts.pane_open.then_some(facts.pane_id).flatten()
    }

    // ------------------------------------------------------------------------- resume

    /// Fixes that need work: a finished agent, a card that ran out, a card that vanished in a restart.
    /// Light checks run here; the long ones (the check before a push card, the push) get a thread.
    fn resume(self: &Arc<Self>, settings: &Settings, me: &str) {
        let _ = (settings, me);
        for fix in self.io.store().fixes() {
            let waits = matches!(
                fix.state,
                FixState::Asking { .. } | FixState::AskPush { .. } | FixState::Running { .. } | FixState::Checking | FixState::Pushing | FixState::Starting
            );
            if !waits {
                continue;
            }
            if let Some(claim) = self.claim(&fix.id) {
                self.resume_one(claim, &fix.id);
            }
        }
    }

    fn resume_one(self: &Arc<Self>, claim: Claim<I>, fix_id: &str) {
        let Some(mut fix) = self.io.store().fix_by_id(fix_id) else { return };
        let Some(watch) = self.io.store().watch_by_id(&fix.watch_id) else { return };
        if !matches!(watch.state, WatchState::Watching | WatchState::Paused { .. }) && !matches!(fix.state, FixState::Running { .. } | FixState::Checking | FixState::Pushing) {
            // A card for a watch that ended: take it down.
            self.end_cards_of(&watch.id);
            return;
        }
        match fix.state.clone() {
            FixState::Asking { question_id } => match self.io.question(&question_id) {
                Some(q) if q.state == QuestionState::Pending => {}
                Some(q) if q.state == QuestionState::TimedOut => {
                    fix.state = FixState::Expired;
                    self.put_fix(&fix);
                }
                Some(q) => self.spawn_job(claim, fix_id, Job::Answer(Box::new(q))),
                None => {
                    // The card was lost (a restart). Ask again under a new id.
                    let id = uuid::Uuid::new_v4().to_string();
                    fix.state = FixState::Asking { question_id: id.clone() };
                    self.put_fix(&fix);
                    self.raise_fix_card(&watch, &mut fix, Some(id));
                }
            },
            FixState::AskPush { question_id, facts } => match self.io.question(&question_id) {
                Some(q) if q.state == QuestionState::Pending => {}
                Some(q) if q.state == QuestionState::TimedOut => {
                    fix.state = FixState::NotPushed;
                    self.put_fix(&fix);
                }
                Some(q) => self.spawn_job(claim, fix_id, Job::Answer(Box::new(q))),
                None => {
                    let id = uuid::Uuid::new_v4().to_string();
                    fix.state = FixState::AskPush { question_id: id.clone(), facts: facts.clone() };
                    self.put_fix(&fix);
                    self.raise_push_card(&watch, &fix, &facts, Some(id));
                }
            },
            FixState::Running { handle, .. } => match self.io.run_snapshot(&watch.project, &handle) {
                Some(facts) if facts.terminal => self.spawn_job(claim, fix_id, Job::Finish),
                Some(_) => {}
                None => self.fail(
                    &mut fix,
                    CI_FIX_START_FAILED,
                    "Puppet Master lost track of this run. Open the copy to see what it did.",
                ),
            },
            FixState::Checking => self.spawn_job(claim, fix_id, Job::Finish),
            FixState::Pushing => self.spawn_job(claim, fix_id, Job::Resolve),
            FixState::Starting => {
                // A start that a restart cut off. The try stays counted; the fix can be asked again.
                self.fail(&mut fix, CI_FIX_START_FAILED, "Puppet Master restarted while this was starting. Ask again.");
            }
            _ => {}
        }
    }

    /// Hand a claimed fix to a thread of its own. The claim is released when the job ends.
    pub(super) fn spawn_job(self: &Arc<Self>, claim: Claim<I>, fix_id: &str, job: Job) {
        let this = self.clone();
        let fix_id = fix_id.to_string();
        self.io.spawn(
            "ci-fix-job",
            Box::new(move || {
                match job {
                    Job::Finish => this.finish_job(&fix_id),
                    Job::Push(shown) => this.push_job(&fix_id, shown.map(|facts| *facts)),
                    Job::Resolve => this.resolve_pushing(&fix_id),
                    Job::Start(by) => this.start_job(&fix_id, &by),
                    Job::Answer(question) => {
                        if let Some(fix) = this.io.store().fix_by_id(&fix_id) {
                            this.apply_card_answer(fix, &question);
                        }
                    }
                }
                // A queued fix starts at the next look (`drive_queue` runs inside the queue lock, so it is not called here).
                drop(claim);
            }),
        );
    }
}

/// What a job does for a claimed fix (see `spawn_job`).
#[derive(Debug, Clone)]
pub(super) enum Job {
    Answer(Box<Question>),
    /// Start the fix; the text is who said yes (`desktop` or `phone`).
    Start(String),
    Finish,
    /// Push the fix; the facts are what the card showed (`None`: a retry).
    Push(Option<Box<FixFacts>>),
    /// A restart cut a push off: find out from the remote whether it went through.
    Resolve,
}

/// A claim held by a call that has no `Arc` to the engine.
struct RefClaim<'a, I: Io + 'static> {
    engine: &'a Engine<I>,
    id: String,
}

impl<I: Io + 'static> Drop for RefClaim<'_, I> {
    fn drop(&mut self) {
        self.engine.state.lock().busy.remove(&self.id);
    }
}

/// A fix whose card may be raised again by the person (it did not run, or ended without a push).
pub(super) fn reaskable(state: &FixState) -> bool {
    matches!(
        state,
        FixState::Declined
            | FixState::Expired
            | FixState::NoChanges { .. }
            | FixState::NotPushed
            | FixState::Held { .. }
    ) || matches!(state, FixState::Failed { code, .. } if code != CI_FIX_TRIES_USED)
}

/// Route A is allowed only into the pane of a run that is guarded, still open, idle, and not moved to another agent.
pub(super) fn route_a_ok(facts: &super::live::RunFacts) -> bool {
    facts.guarded && facts.pane_open && facts.terminal && !facts.handed_off
}

pub(super) fn count(n: u32, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

impl<I: Io + 'static> Engine<I> {
    /// Look at this watch's checks at the next tick (after a push).
    pub(super) fn poll_soon(&self, watch_id: &str) {
        self.with_poll(watch_id, |poll| {
            poll.last_poll_ms = 0;
            poll.pending = true;
            poll.pull_etag = None;
        });
    }

    /// A fix that cannot go on. The reason is cleaned before it is saved.
    pub(super) fn fail(&self, fix: &mut FixRun, code: &str, message: &str) {
        fix.state = FixState::Failed { code: code.to_string(), message: clean_message(message) };
        self.put_fix(fix);
    }

    /// The watch of a fix, as stored now.
    pub(super) fn watch_of(&self, fix: &FixRun) -> Option<Watch> {
        self.io.store().watch_by_id(&fix.watch_id)
    }

    /// Add a watch for a new pull request (flow A). `source` is `issue` or `ship`.
    pub(super) fn add_watch(&self, project: &str, copy: &crate::worktrees::WorktreeRef, now: u64) -> Result<Watch, crate::operations::OperationError> {
        let pr = copy.pr.as_ref().ok_or_else(|| err("NO_PR", "This copy has no pull request yet."))?;
        if !copy.branch.starts_with("pm/") || copy.branch == pr.base || copy.branch.contains("..") {
            return Err(err(CI_FIX_NOT_PM_BRANCH, "This branch is not one Puppet Master opened."));
        }
        let owner_repo = self
            .io
            .folder_owner_repo(project)
            .filter(|name| valid_owner_repo(name))
            .ok_or_else(|| err(CI_FIX_NOT_YOURS, "This folder doesn't point at a GitHub repository."))?;
        let agent = self.io.agent_of_copy(project, &copy.handle).ok_or_else(|| {
            err(CI_FIX_START_FAILED, "Puppet Master doesn't know which agent made this copy")
        })?;
        let env = self.io.git_env()?;
        let head = self.io.head_sha(copy, &env)?;
        if !valid_sha(&head) {
            return Err(err(CI_FIX_HEAD_MOVED, "Puppet Master can't read the copy's commit."));
        }
        let prefix = format!("https://github.com/{owner_repo}/pull/");
        let pr_url = if pr.url.starts_with(&prefix) && !pr.url.chars().any(char::is_whitespace) {
            pr.url.clone()
        } else {
            format!("{prefix}{}", pr.number)
        };
        let watch = Watch {
            id: new_watch_id(),
            project: project.to_string(),
            handle: copy.handle.clone(),
            owner_repo,
            branch: copy.branch.clone(),
            base: pr.base.clone(),
            pr_number: pr.number,
            pr_url,
            agent_label: self.io.agent_label(&agent.agent_type),
            agent,
            source: if self.io.made_by_issue_run(&copy.handle) { "issue" } else { "ship" }.to_string(),
            added_at_ms: now,
            last_push_ms: now,
            pushed_shas: vec![head],
            tries_used: 0,
            state: WatchState::Watching,
        };
        self.io.store().put_watch(&watch).map_err(|message| err("CI_FIX_WATCH_FAILED", &message))?;
        self.io.changed();
        self.io.audit("watched", "auto", &format!("Watching CI on #{}", watch.pr_number), watch_detail(&watch));
        // Look soon: checks usually start within a minute of the push.
        self.with_poll(&watch.id, |poll| {
            poll.pending = true;
            poll.last_poll_ms = now;
        });
        Ok(watch)
    }

    /// A pull request was opened from one of our copies (called by `worktrees::service`). Never fails.
    pub fn on_pr_opened(&self, project: &str, copy: &crate::worktrees::WorktreeRef) {
        if !self.io.store().settings().enabled {
            return;
        }
        if self.io.store().watch_for(project, &copy.handle).is_some_and(|w| matches!(w.state, WatchState::Watching | WatchState::Paused { .. })) {
            return;
        }
        if let Err(error) = self.add_watch(project, copy, self.io.now_ms()) {
            tracing::info!(code = %error.code, "ci-fix: did not start watching a new pull request");
        }
    }
}
