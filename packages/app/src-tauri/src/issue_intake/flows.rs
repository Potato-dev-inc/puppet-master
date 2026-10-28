//! The four flows of one issue run (SPEC n32 section 3.6 B to E): start the agent in a separate copy,
//! check the change when it finishes, push it as a draft pull request, or throw the copy away.
//!
//! Every flow works on a run its caller has claimed, and ends by saving the run's new state. None of
//! them writes to a pane. The issue text reaches the agent only through the issue file (`text`); the
//! task is PM's own paragraph. Pushing is `pr_create_with` on a hardened git env, after the copy's
//! git settings and the remote branch were checked again, and always as a draft.

use super::engine::{choice, clean_message, err, run_detail, CardKind, Engine, CHOICE_GOT_IT, CHOICE_RETRY};
use super::live::{Io, RunSnap};
use super::model::*;
use super::pushcheck;
use super::text::{self, Approval};
use crate::agent_runs::AgentRunRequest;
use crate::operations::OperationError;
use crate::pty::env_guard;
use crate::worktrees::pr::PrFacts;
use crate::worktrees::{Changes, WorktreeRef};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

/// Names and lines kept in a stored fact.
const FACT_LIST_CAP: usize = 50;
const NAME_MAX_CHARS: usize = 200;
const RUN_NAME_TITLE_CHARS: usize = 40;
const OVER_CAP: &str = "The change is too big to check every line (over 256 KiB).";
const ASKED_YOU: &str = "You asked Puppet Master to check each push first.";

/// What one look at a finished copy found.
pub(super) struct Checked {
    pub facts: PushFacts,
    pub over_cap: bool,
    pub changes: Changes,
}

/// A file name from the agent's copy as PM prints it: one line, no control characters.
fn clean_name(name: &str) -> String {
    let flat: String = name.chars().map(|c| if c.is_control() { '?' } else { c }).collect();
    flat.chars().take(NAME_MAX_CHARS).collect()
}

fn cap<T>(mut items: Vec<T>) -> Vec<T> {
    items.truncate(FACT_LIST_CAP);
    items
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `#42 Login button does nothing`: the run's name in the pane list (cleaned title, 40 characters).
fn run_name(snapshot: &IssueSnapshot) -> String {
    let flat = snapshot.title.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = if flat.chars().count() > RUN_NAME_TITLE_CHARS {
        format!("{}\u{2026}", flat.chars().take(RUN_NAME_TITLE_CHARS - 1).collect::<String>().trim_end())
    } else {
        flat
    };
    format!("#{} {}", snapshot.number, title).trim_end().to_string()
}

/// `(open, still has the label)` from the issue's own JSON (`gh api repos/{o}/{r}/issues/{n}`).
pub(super) fn parse_open_labeled(json: &str, label: &str) -> Option<(bool, bool)> {
    let value: Value = serde_json::from_str(json).ok()?;
    let open = value.get("state")?.as_str()? == "open";
    let wanted = label.trim();
    let labeled = value
        .get("labels")
        .and_then(Value::as_array)
        .map(|labels| {
            labels.iter().any(|item| {
                item.get("name").and_then(Value::as_str).is_some_and(|name| name.trim().eq_ignore_ascii_case(wanted))
            })
        })
        .unwrap_or(false);
    Some((open, labeled))
}

/// True when the issue's own JSON (`gh api repos/{o}/{r}/issues/{n}`) is not what the person said yes to:
/// closed, the label taken off, or a title or body whose cleaned hash differs from `sha8`. A JSON that
/// cannot be read, or has no title to compare, says nothing (false): the card is not blamed on a guess.
pub(super) fn issue_differs(json: &str, label: &str, sha8: &str) -> bool {
    let Some((open, labeled)) = parse_open_labeled(json, label) else { return false };
    if !open || !labeled {
        return true;
    }
    let Ok(value) = serde_json::from_str::<Value>(json) else { return false };
    let Some(title) = value.get("title").and_then(Value::as_str) else { return false };
    let body = value.get("body").and_then(Value::as_str).unwrap_or("");
    text::sha8_of_raw(title, body) != sha8
}

/// The reasons the push card shows, and whether the push is held. A change too big to scan in full
/// always asks, even when the repo does not.
pub(super) fn reasons_for(facts: &PushFacts, over_cap: bool, ask_before_push: bool) -> (Vec<String>, bool) {
    let (mut reasons, held) = pushcheck::reasons(facts, ask_before_push);
    if over_cap {
        reasons.insert(0, OVER_CAP.to_string());
    }
    (reasons, held)
}

fn held_code(facts: &PushFacts) -> &'static str {
    if facts.git_changed {
        ERR_GIT_CHANGED
    } else if facts.remote_branch_exists {
        ERR_REMOTE_BRANCH_EXISTS
    } else {
        ERR_PUSH_HELD
    }
}

fn repo_line(snapshot: &IssueSnapshot) -> String {
    format!("Repo: {} (issue #{})", snapshot.owner_repo, snapshot.number)
}

/// `3 files · +40 −5 · the agent · 12 turns`, then one line per reason, at most 1000 characters.
fn push_context(run: &IssueRun, facts: &PushFacts, reasons: &[String]) -> String {
    let mut lines = vec![
        repo_line(&run.issue),
        format!(
            "{} \u{b7} +{} \u{2212}{} \u{b7} {} \u{b7} {}",
            plural(facts.files, "file", "files"),
            facts.additions,
            facts.deletions,
            run.agent_label,
            plural(facts.turns, "turn", "turns"),
        ),
    ];
    lines.extend(reasons.iter().cloned());
    let text: String = lines.join("\n").chars().filter(|c| !c.is_control() || *c == '\n').collect();
    text.chars().take(1000).collect()
}

impl<I: Io + 'static> Engine<I> {
    /// A run that cannot go on. The reason is cleaned before it is saved.
    pub(super) fn fail(&self, run: &mut IssueRun, code: &str, message: &str) {
        run.state = RunState::Failed { code: code.to_string(), message: clean_message(message) };
        self.put(run);
    }

    /// A start that failed before any agent ran: the day cap is not used up, and a card says so.
    fn fail_start(&self, run: &mut IssueRun, code: &str, message: &str) {
        run.started_at_ms = None;
        self.fail(run, code, message);
        self.raise_notice(&format!("Couldn't start #{}: {}", run.issue.number, clean_message(message)));
    }

    // ------------------------------------------------------------------------------ B. start

    /// Flow B. The run is `Starting` and claimed. Checks everything again, makes the copy, writes the
    /// issue file, and starts the agent in a visible pane that cannot push.
    pub(super) fn start_job(self: &Arc<Self>, run_id: &str) {
        let io = self.io();
        let Some(mut run) = io.store().by_id(run_id) else { return };
        if run.state != RunState::Starting {
            return;
        }
        let Some(repo) = self.repo_of(&run) else {
            return self.fail_start(&mut run, ERR_REPO_MISMATCH, "This repository is not on your list any more.");
        };
        if repo.paused {
            run.started_at_ms = None;
            run.state = RunState::Queued { why: "This repository is paused".to_string() };
            return self.put(&run);
        }
        let folder = repo.folder.as_str();
        if !io.folder_owner_repo(folder).is_some_and(|name| name.eq_ignore_ascii_case(&repo.owner_repo)) {
            let message = format!("The folder no longer points at {}.", repo.owner_repo);
            return self.fail_start(&mut run, ERR_REPO_MISMATCH, &message);
        }
        if !io.agent_ready(&run.agent.agent_type) {
            let message = format!("{} can't start right now.", run.agent_label);
            return self.fail_start(&mut run, ERR_AGENT_NOT_READY, &message);
        }
        if let Err((code, message)) = self.issue_is_open_and_labeled(&repo, run.issue.number) {
            return self.fail_start(&mut run, code, &message);
        }
        let facts = match io.repo_facts(Path::new(folder)) {
            Ok(facts) if facts.name_with_owner.eq_ignore_ascii_case(&repo.owner_repo) => facts,
            Ok(_) => return self.fail_start(&mut run, ERR_REPO_MISMATCH, "GitHub says this folder is a different repository."),
            Err(message) => return self.fail_start(&mut run, ERR_START_FAILED, &message),
        };
        let Some(default) = facts.default_branch else {
            return self.fail_start(&mut run, ERR_START_FAILED, "GitHub did not say the default branch.");
        };
        let env = match io.git_env() {
            Ok(env) => env,
            Err(error) => return self.fail_start(&mut run, ERR_START_FAILED, &error.message),
        };
        if let Err(error) = io.fetch_branch(folder, &default, &env) {
            return self.fail_start(&mut run, ERR_START_FAILED, &error.message);
        }
        let handle = format!("issue-{}-{}", run.issue.number, &uuid::Uuid::new_v4().simple().to_string()[..4]);
        let copy = match io.create_copy(folder, &handle, &format!("origin/{default}"), &env) {
            Ok(copy) => copy,
            Err(error) => return self.fail_start(&mut run, ERR_START_FAILED, &error.message),
        };
        let prepared = io
            .write_issue_file(&copy, run.issue.number, &text::issue_file(&run.issue, &repo.label))
            .map_err(|message| err(ERR_START_FAILED, &message))
            .and_then(|()| io.fingerprint(&copy));
        let fingerprint = match prepared {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                let _ = io.discard(folder, &handle);
                return self.fail_start(&mut run, ERR_START_FAILED, &error.message);
            }
        };
        run.handle = Some(handle.clone());
        run.branch = Some(copy.branch.clone());
        run.base = Some(default);
        run.git_fingerprint = Some(fingerprint);
        self.put(&run);
        let request = match self.build_request(&run, &repo, &copy, &handle) {
            Ok(request) => request,
            Err(error) => return self.start_refused(&mut run, folder, &handle, &error.message),
        };
        match io.run_agent(request) {
            Ok(started) => {
                run.state = RunState::Running {
                    handle: started.handle,
                    operation_id: started.operation_id,
                    pane_id: started.pane_id,
                };
                self.put(&run);
                let who = run.approved_by.clone().unwrap_or_else(|| "auto".to_string());
                io.audit("started", &who, &format!("Started issue #{}", run.issue.number), run_detail(&run));
            }
            Err(error) => self.start_refused(&mut run, folder, &handle, &error.message),
        }
    }

    /// The agent was not started. The copy has no work in it, so it goes, and the run may be asked again.
    fn start_refused(&self, run: &mut IssueRun, folder: &str, handle: &str, message: &str) {
        if self.io().discard(folder, handle).is_ok() {
            run.handle = None;
            run.branch = None;
            run.git_fingerprint = None;
        }
        self.fail_start(run, ERR_START_FAILED, message);
    }

    /// One GitHub read when the agent is done (S10): is the issue still what the person said yes to?
    /// A gone issue counts as changed. A read that fails says nothing.
    fn issue_changed_since_yes(&self, run: &IssueRun, repo: &RepoWatch) -> bool {
        let path = format!("repos/{}/issues/{}", repo.owner_repo, run.issue.number);
        let Ok(response) = self.io().gh_api(&path, None) else { return false };
        match response.status {
            404 | 410 => true,
            200 => issue_differs(&response.body, &repo.label, &run.issue.sha8),
            _ => false,
        }
    }

    /// One GitHub read before the agent starts: the issue is still open and still has the label.
    fn issue_is_open_and_labeled(&self, repo: &RepoWatch, number: u64) -> Result<(), (&'static str, String)> {
        let path = format!("repos/{}/issues/{number}", repo.owner_repo);
        let response = self
            .io()
            .gh_api(&path, None)
            .map_err(|message| (ERR_START_FAILED, format!("Couldn't read the issue on GitHub: {message}")))?;
        if matches!(response.status, 404 | 410) {
            return Err((ERR_CLOSED, format!("Issue #{number} is gone.")));
        }
        let unreadable = || (ERR_START_FAILED, "Couldn't read the issue on GitHub.".to_string());
        if response.status != 200 {
            return Err(unreadable());
        }
        match parse_open_labeled(&response.body, &repo.label) {
            None => Err(unreadable()),
            Some((false, _)) => Err((ERR_CLOSED, format!("Issue #{number} is closed."))),
            Some((true, false)) => Err((ERR_LABEL_GONE, format!("The label {} was taken off #{number}.", repo.label))),
            Some((true, true)) => Ok(()),
        }
    }

    /// The `run_agent` request of SPEC 3.6 B5. The task is PM's paragraph with no issue text in it; the
    /// pane is visible, kept, untrusted-by-default, and runs under the `no_push` guard in the copy.
    fn build_request(&self, run: &IssueRun, repo: &RepoWatch, copy: &WorktreeRef, handle: &str) -> Result<AgentRunRequest, OperationError> {
        let mut request: AgentRunRequest = serde_json::from_value(json!({
            "project_path": repo.folder,
            "task": text::task(&run.issue, &copy.branch),
            "agent_type": run.agent.agent_type,
            "account": run.agent.account,
            "headless": false,
            "background": true,
            "keep_pane": true,
            "wait_ms": 0,
            "timeout_ms": u64::from(repo.minutes) * 60_000,
            "budget_turns": repo.turns,
            "budget_minutes": repo.minutes,
            "idempotency_key": format!("issue:{}", run.id),
            "name": run_name(&run.issue),
            "handle": handle,
            "trust_workspace": false,
            "allow_shell": false,
            "allowed_commands": null,
            "read_only": false,
            "context_policy": "fresh",
        }))
        .map_err(|error| err(ERR_START_FAILED, &error.to_string()))?;
        request.join_worktree = Some(copy.clone());
        request.guard_env = Some(env_guard::NO_PUSH.to_string());
        Ok(request)
    }

    // ---------------------------------------------------------------------------- C. finish

    /// Flow C. The agent's turn is over: save its edits as a commit, look at the change, and either
    /// raise the push card, say there is nothing to push, or (the repo does not ask, and the check is
    /// clean) go on to the push.
    pub(super) fn finish_job(self: &Arc<Self>, run_id: &str) {
        let io = self.io();
        let Some(mut run) = io.store().by_id(run_id) else { return };
        let started_pane = match &run.state {
            RunState::Running { pane_id, .. } => pane_id.clone(),
            RunState::Checking => None,
            _ => return,
        };
        let (Some(repo), Some(handle)) = (self.repo_of(&run), run.handle.clone()) else {
            return self.fail(&mut run, ERR_PUSH_FAILED, "Puppet Master cannot find this run's copy.");
        };
        let snap = io.run_snapshot(&repo.folder, &handle);
        if snap.as_ref().is_some_and(|snap| !snap.terminal) {
            return;
        }
        let Some(copy) = io.find_copy(&repo.folder, &handle) else {
            return self.fail(&mut run, ERR_PUSH_FAILED, "The copy is gone, so there is nothing to check.");
        };
        // A follow-up typed into the pane: wait until the agent is idle again.
        if io.copy_busy(&repo.folder, &copy) {
            return;
        }
        let pane_id = started_pane.or_else(|| snap.as_ref().and_then(|snap| snap.pane_id.clone()));
        if !matches!(run.state, RunState::Checking) {
            run.state = RunState::Checking;
            self.put(&run);
        }
        let env = match io.git_env().and_then(|env| io.commit_pending(&copy, &env).map(|_| env)) {
            Ok(env) => env,
            Err(error) => return self.check_failed(&mut run, &error.message, pane_id),
        };
        let mut checked = match self.check(&run, &copy, &env, snap.as_ref()) {
            Ok(checked) => checked,
            Err(error) => return self.check_failed(&mut run, &error.message, pane_id),
        };
        checked.facts.issue_changed = self.issue_changed_since_yes(&run, &repo);
        if checked.facts.files == 0 {
            let message = pane_id
                .as_deref()
                .and_then(|pane| io.turn_sentence(pane))
                .map(|sentence| clean_message(&sentence))
                .unwrap_or_else(|| "The agent finished without changing any files.".to_string());
            run.state = RunState::NoChanges { message: message.clone() };
            self.put(&run);
            self.raise_no_changes_card(&run, &message);
            return;
        }
        let (reasons, held) = reasons_for(&checked.facts, checked.over_cap, repo.ask_before_push);
        if held {
            self.hold(&mut run, &checked.facts, &reasons);
            return;
        }
        if reasons.is_empty() {
            run.state = RunState::Pushing;
            self.put(&run);
            return self.push_job(run_id);
        }
        let question = self.raise_push_kind(
            &run,
            &format!("Draft PR ready for #{}?", run.issue.number),
            push_context(&run, &checked.facts, &reasons),
            vec![choice(CHOICE_PUSH, "Open draft PR"), choice(CHOICE_LOOK, "Look first"), choice(CHOICE_THROW, "Throw away")],
            pane_id,
            None,
        );
        match question {
            Some(question_id) => {
                run.state = RunState::AskPush { question_id, facts: checked.facts };
                self.put(&run);
                let who = run.approved_by.clone().unwrap_or_else(|| "auto".to_string());
                io.audit("asked", &who, &format!("Asked about a draft PR for #{}", run.issue.number), run_detail(&run));
            }
            None => self.check_failed(&mut run, "There are too many open cards. Answer some, then try again.", None),
        }
    }

    /// Read the copy: the files, the patch, the git settings and the remote branch. Fails closed: any
    /// read that fails is an error, never "nothing found".
    pub(super) fn check(&self, run: &IssueRun, copy: &WorktreeRef, env: &[(String, String)], snap: Option<&RunSnap>) -> Result<Checked, OperationError> {
        let io = self.io();
        let changes = io.changes(copy, env)?;
        let names = io.changed_files(copy, env)?;
        let diff = io.push_diff(copy, env)?;
        let (secret_lines, home_lines) = io.scan_added(&diff.text);
        let now_fingerprint = io.fingerprint(copy)?;
        let remote_branch_exists = io.remote_has_branch(copy, env)?;
        let shown = |lines: Vec<(String, u32)>| cap(lines.into_iter().map(|(path, line)| (clean_name(&path), line)).collect());
        let named = |files: Vec<String>| cap(files.iter().map(|name| clean_name(name)).collect::<Vec<_>>());
        let files = u32::try_from(names.len().max(changes.files_changed)).unwrap_or(u32::MAX);
        let facts = PushFacts {
            files,
            additions: changes.additions,
            deletions: changes.deletions,
            ci_files: named(pushcheck::ci_paths(&names)),
            secret_files: named(pushcheck::secret_paths(&names)),
            secret_lines: shown(secret_lines),
            home_path_lines: shown(home_lines),
            internal_files: named(pushcheck::internal_paths(&names)),
            git_changed: run.git_fingerprint.as_deref() != Some(now_fingerprint.as_str()),
            remote_branch_exists,
            issue_changed: false,
            run_status: snap.map_or_else(|| "unknown".to_string(), |snap| snap.status.clone()),
            turns: snap.map_or(0, |snap| snap.turns),
            minutes: snap.map_or(0, |snap| snap.minutes),
        };
        Ok(Checked { facts, over_cap: diff.over_cap, changes })
    }

    /// A guard stopped the push: save `Held` and raise a card with no push button.
    fn hold(&self, run: &mut IssueRun, facts: &PushFacts, reasons: &[String]) {
        let message: String = reasons.iter().filter(|reason| reason.as_str() != ASKED_YOU).cloned().collect::<Vec<_>>().join(" ");
        let message: String = message.chars().take(300).collect();
        run.state = RunState::Held { code: held_code(facts).to_string(), message: message.clone() };
        self.put(run);
        self.raise_held_card(run, &message);
        let who = run.approved_by.clone().unwrap_or_else(|| "auto".to_string());
        self.io().audit("held", &who, &format!("Held the push for #{}", run.issue.number), run_detail(run));
    }

    pub(super) fn raise_held_card(&self, run: &IssueRun, message: &str) {
        let context: String = format!("{}\n{}", repo_line(&run.issue), message).chars().take(1000).collect();
        self.raise_push_kind(
            run,
            &format!("Draft PR for #{} is on hold", run.issue.number),
            context,
            vec![choice(CHOICE_LOOK, "Look first"), choice(CHOICE_THROW, "Throw away")],
            self.pane_of(run),
            Some(CardKind::Held),
        );
    }

    pub(super) fn raise_no_changes_card(&self, run: &IssueRun, message: &str) {
        let context: String = format!("{}\n{}", repo_line(&run.issue), message).chars().take(1000).collect();
        self.raise_push_kind(
            run,
            &format!("No changes for #{}", run.issue.number),
            context,
            vec![choice(CHOICE_LOOK, "Open the agent"), choice(CHOICE_THROW, "Throw away")],
            self.pane_of(run),
            Some(CardKind::NoChanges),
        );
    }

    /// The push card again (the person tapped Look first, or a restart lost the card).
    pub(super) fn reraise_push_card(&self, run: &IssueRun, facts: &PushFacts) -> Option<String> {
        let ask = self.repo_of(run).is_none_or(|repo| repo.ask_before_push);
        let (reasons, _) = reasons_for(facts, false, ask);
        self.raise_push_kind(
            run,
            &format!("Draft PR ready for #{}?", run.issue.number),
            push_context(run, facts, &reasons),
            vec![choice(CHOICE_PUSH, "Open draft PR"), choice(CHOICE_LOOK, "Look first"), choice(CHOICE_THROW, "Throw away")],
            self.pane_of(run),
            None,
        )
    }

    /// The pane the agent worked in, while it is still known.
    fn pane_of(&self, run: &IssueRun) -> Option<String> {
        let repo = self.repo_of(run)?;
        let handle = run.handle.as_deref()?;
        self.io().run_snapshot(&repo.folder, handle)?.pane_id
    }

    /// The check could not be done. The copy stays; `Try again` runs it again.
    fn check_failed(&self, run: &mut IssueRun, message: &str, pane_id: Option<String>) {
        self.failed_with_retry(run, message, CardKind::RetryCheck, pane_id);
    }

    fn failed_with_retry(&self, run: &mut IssueRun, message: &str, kind: CardKind, pane_id: Option<String>) {
        let message = clean_message(message);
        run.state = RunState::Failed { code: ERR_PUSH_FAILED.to_string(), message: message.clone() };
        self.put(run);
        let context: String = format!("{}\n{}", repo_line(&run.issue), message).chars().take(1000).collect();
        self.raise_push_kind(
            run,
            &format!("Couldn't open the PR for #{}", run.issue.number),
            context,
            vec![choice(CHOICE_RETRY, "Try again"), choice(CHOICE_GOT_IT, "Got it")],
            pane_id.or_else(|| self.pane_of(run)),
            Some(kind),
        );
    }

    // ------------------------------------------------------------------------------ D. push

    /// Flow D. The run is `Pushing` and claimed. Checks the copy again (git settings, remote branch,
    /// internal files), checks that the copy and the folder are still the listed repo, then pushes the
    /// branch and opens a draft pull request. Never merges, never marks it ready.
    pub(super) fn push_job(self: &Arc<Self>, run_id: &str) {
        let io = self.io();
        let Some(mut run) = io.store().by_id(run_id) else { return };
        if run.state != RunState::Pushing {
            return;
        }
        let (Some(repo), Some(handle), Some(base)) = (self.repo_of(&run), run.handle.clone(), run.base.clone()) else {
            return self.fail(&mut run, ERR_PUSH_FAILED, "Puppet Master cannot find this run's copy.");
        };
        let Some(copy) = io.find_copy(&repo.folder, &handle) else {
            return self.fail(&mut run, ERR_PUSH_FAILED, "The copy is gone, so there is nothing to push.");
        };
        let snap = io.run_snapshot(&repo.folder, &handle);
        let env = match io.git_env() {
            Ok(env) => env,
            Err(error) => return self.failed_with_retry(&mut run, &error.message, CardKind::RetryPush, None),
        };
        let checked = match self.check(&run, &copy, &env, snap.as_ref()) {
            Ok(checked) => checked,
            Err(error) => return self.failed_with_retry(&mut run, &error.message, CardKind::RetryPush, None),
        };
        let (reasons, held) = reasons_for(&checked.facts, checked.over_cap, repo.ask_before_push);
        if held {
            return self.hold(&mut run, &checked.facts, &reasons);
        }
        if !io.folder_owner_repo(&repo.folder).is_some_and(|name| name.eq_ignore_ascii_case(&repo.owner_repo)) {
            return self.fail(&mut run, ERR_REPO_MISMATCH, "The folder no longer points at the listed repository.");
        }
        match io.repo_facts(Path::new(&copy.path)) {
            Ok(facts) if facts.name_with_owner.eq_ignore_ascii_case(&repo.owner_repo) => {}
            Ok(_) => return self.fail(&mut run, ERR_REPO_MISMATCH, "GitHub says the copy is a different repository."),
            Err(message) => return self.failed_with_retry(&mut run, &message, CardKind::RetryPush, None),
        }
        let facts = PrFacts {
            task: None,
            summary: snap.as_ref().and_then(|snap| snap.pane_id.as_deref()).and_then(|pane| io.turn_sentence(pane)),
            agent: Some(run.agent_label.clone()),
            files: checked.changes.files.iter().map(|file| (clean_name(&file.path), file.additions, file.deletions)).collect(),
            additions: checked.changes.additions,
            deletions: checked.changes.deletions,
            files_total: checked.changes.files_changed,
            setup: copy.setup.as_ref().map(|setup| setup.sentence()),
            ..PrFacts::default()
        };
        let title = text::pr_title(&run.issue);
        let body = text::pr_body(
            &run.issue,
            &facts,
            &run.agent_label,
            checked.facts.turns,
            checked.facts.minutes,
            Approval::of(run.approved_by.as_deref()),
        );
        match io.pr_create(&repo.folder, &handle, &title, &body, &base, &env) {
            Ok(pr) => {
                let url = if pr.url.starts_with("https://github.com/") && !pr.url.chars().any(char::is_whitespace) {
                    pr.url.clone()
                } else {
                    format!("https://github.com/{}/pull/{}", repo.owner_repo, pr.number)
                };
                run.state = RunState::PrOpen { number: pr.number, url };
                self.put(&run);
                let who = run.approved_by.clone().unwrap_or_else(|| "auto".to_string());
                io.audit("pr_opened", &who, &format!("Opened a draft PR for #{}", run.issue.number), run_detail(&run));
            }
            Err(error) => self.failed_with_retry(&mut run, &error.message, CardKind::RetryPush, None),
        }
    }

    // ------------------------------------------------------------------------- E. throw away

    /// Flow E. Remove the copy and its branch (refused while the agent works in it). Cards of the run go.
    pub(super) fn throw_away(&self, run: &mut IssueRun) -> Result<(), OperationError> {
        let io = self.io();
        let repo = self.repo_of(run).ok_or_else(|| err(ERR_REPO_MISMATCH, "This repository is not on your list any more."))?;
        let handle = run.handle.clone().ok_or_else(|| err("INVALID_ARGUMENT", "This run has no copy."))?;
        match io.discard(&repo.folder, &handle) {
            Ok(()) => {}
            Err(error) if error.code == "WORKTREE_NOT_FOUND" => {}
            Err(error) => {
                self.raise_notice(&format!("Couldn't throw away #{}: {}", run.issue.number, clean_message(&error.message)));
                return Err(error);
            }
        }
        let open = match &run.state {
            RunState::AskPush { question_id, .. } => Some(question_id.clone()),
            _ => None,
        };
        run.state = RunState::ThrownAway;
        self.put(run);
        if let Some(question_id) = open {
            self.withdraw(&question_id);
        }
        self.withdraw_cards_of(&run.id);
        io.audit("thrown_away", "desktop", &format!("Threw away the copy for #{}", run.issue.number), run_detail(run));
        Ok(())
    }
}
