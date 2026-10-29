//! The flows of one fix try (SPEC n33 section 3.5 D to G): start the fix in a pane that cannot push, check
//! the change when the agent is done, ask, and push it to the same `pm/` branch.
//!
//! Every flow works on a fix its caller has claimed, and ends by saving the fix's new state. None of them
//! writes to a pane or to GitHub except the one fast-forward push at the end. The log reaches the agent only
//! through the file written from the stored snapshot (`logs::log_file`); the task is Puppet Master's own
//! paragraph (`text::task`) with no log byte and no check name in it.

use super::engine::{choice, clean_message, err, fix_detail, route_a_ok, CardKind, Engine, CHOICE_GOT_IT, CHOICE_RETRY};
use super::filter;
use super::fixcheck;
use super::github::{self, ApiPull};
use super::live::{Io, RunFacts, Started};
use super::logs;
use super::model::*;
use super::text;
use crate::agent_runs::AgentRunRequest;
use crate::issue_intake::pushcheck;
use crate::operations::OperationError;
use crate::pty::env_guard;
use crate::worktrees::WorktreeRef;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

/// Names and lines kept in a stored fact.
const FACT_LIST_CAP: usize = 50;
const NAME_MAX_CHARS: usize = 200;
/// The error codes `send_agent` answers when the pane of a conversation cannot take a follow-up any more:
/// the task moved to another agent, or the pane is closed. The fix then starts in a new pane (route B).
const ROUTE_A_REFUSALS: &[&str] = &["HANDED_OFF", "PANE_GONE", "PANE_CLOSED_AFTER_RUN"];

/// A file name from the agent's copy as Puppet Master prints it: one line, no control characters.
fn clean_name(name: &str) -> String {
    let flat: String = name.chars().map(|c| if c.is_control() { '?' } else { c }).collect();
    flat.chars().take(NAME_MAX_CHARS).collect()
}

fn cap<T>(mut items: Vec<T>) -> Vec<T> {
    items.truncate(FACT_LIST_CAP);
    items
}

fn places(items: Vec<(String, u32)>) -> Vec<(String, u32)> {
    cap(items.into_iter().map(|(path, line)| (clean_name(&path), line)).collect())
}

fn names(items: Vec<String>) -> Vec<String> {
    cap(items.iter().map(|name| clean_name(name)).collect())
}

/// Facts that must be the same between the card and the push (the run's own numbers may differ).
fn same_change(a: &FixFacts, b: &FixFacts) -> bool {
    a.push.files == b.push.files
        && a.push.additions == b.push.additions
        && a.push.deletions == b.push.deletions
        && a.push.secret_lines == b.push.secret_lines
        && a.push.home_path_lines == b.push.home_path_lines
        && a.skipped_tests == b.skipped_tests
        && a.deleted_tests == b.deleted_tests
        && a.ci_run_files == b.ci_run_files
        && a.web_lines == b.web_lines
        && a.commits == b.commits
}

/// The code a held push is saved with (the first cause, in the order the card lists them).
fn held_code(f: &FixFacts) -> &'static str {
    if f.push.git_changed {
        CI_FIX_GIT_CHANGED
    } else if f.remote_moved {
        CI_FIX_REMOTE_MOVED
    } else if f.history_rewritten {
        CI_FIX_HISTORY_REWRITTEN
    } else if !f.push.ci_files.is_empty() {
        CI_FIX_CI_FILES
    } else {
        CI_FIX_PUSH_HELD
    }
}

/// What a fresh look at the pull request found wrong, as (code, plain sentence).
type Refusal = (&'static str, String);

impl<I: Io + 'static> Engine<I> {
    // ------------------------------------------------------------------------------ D. start

    /// A start that failed before any agent ran: the try is given back, and a card says so.
    fn fail_start(&self, fix: &mut FixRun, watch: &Watch, counted: bool, code: &str, message: &str) {
        if counted {
            self.edit_watch(&watch.id, |w| w.tries_used = w.tries_used.saturating_sub(1));
        }
        self.fail(fix, code, message);
        self.raise_notice(&format!("Couldn't start the fix for #{}: {}", watch.pr_number, clean_message(message)));
    }

    /// The yes is kept: the copy (or the one-fix-at-a-time slot) is busy. After 30 minutes it fails.
    fn queue_fix(&self, fix: &mut FixRun, watch: &Watch, why: &str) {
        let now = self.io().now_ms();
        let since = match fix.state {
            FixState::Queued { since_ms, .. } => since_ms,
            _ => now,
        };
        if now.saturating_sub(since) >= BUSY_WAIT_MS {
            return self.fail_start(
                fix,
                watch,
                false,
                CI_FIX_COPY_BUSY,
                "The agent was busy for too long, so the fix did not start. Ask again when it is free.",
            );
        }
        fix.state = FixState::Queued { why: why.to_string(), since_ms: since };
        self.put_fix(fix);
    }

    /// The pull request as GitHub says it right now (no cache), checked against the watch.
    pub(super) fn fresh_pull(&self, watch: &Watch, copy: &WorktreeRef, me: &str) -> Result<ApiPull, Refusal> {
        let path = github::pull_path(&watch.owner_repo, watch.pr_number);
        let response = self
            .io()
            .gh_api(&path, None)
            .map_err(|message| (CI_FIX_START_FAILED, format!("Couldn't read the pull request on GitHub: {message}")))?;
        let unreadable = || (CI_FIX_START_FAILED, "Couldn't read the pull request on GitHub.".to_string());
        if response.status != 200 {
            return Err(unreadable());
        }
        let pull = github::parse_pull(&response.body).ok_or_else(unreadable)?;
        let default_branch = self.default_branch_now(watch, copy);
        filter::owns(watch, me, &pull, default_branch.as_deref())?;
        Ok(pull)
    }

    fn default_branch_now(&self, watch: &Watch, copy: &WorktreeRef) -> Option<String> {
        match self.io().repo_facts(Path::new(&copy.path)) {
            Ok(facts) if facts.name_with_owner.eq_ignore_ascii_case(&watch.owner_repo) => facts.default_branch,
            _ => None,
        }
    }

    /// Flow D. The fix is claimed and says yes (`Asking`), waits (`Queued`) or comes from the queue (`Starting`).
    /// Checks everything again, writes the log file from the stored snapshot, and starts the agent in a pane
    /// that cannot push.
    pub(super) fn start_job(self: &Arc<Self>, fix_id: &str, by: &str) {
        let io = self.io();
        let Some(mut fix) = io.store().fix_by_id(fix_id) else { return };
        if !matches!(fix.state, FixState::Asking { .. } | FixState::Queued { .. } | FixState::Starting) {
            return;
        }
        let Some(watch) = self.watch_of(&fix) else {
            return self.fail(&mut fix, CI_FIX_START_FAILED, "Puppet Master isn't watching this pull request any more.");
        };
        if fix.approved_by.is_none() {
            fix.approved_by = Some(by.to_string());
        }
        let settings = io.store().settings();
        if !settings.enabled {
            return self.fail_start(&mut fix, &watch, false, CI_FIX_START_FAILED, "Red to Green is turned off.");
        }
        if !matches!(watch.state, WatchState::Watching) {
            return self.fail_start(&mut fix, &watch, false, CI_FIX_START_FAILED, "Puppet Master isn't watching this pull request any more.");
        }
        if watch.tries_used >= settings.tries_per_pr {
            return self.fail_start(&mut fix, &watch, false, CI_FIX_TRIES_USED, &text::tries_used_notice(watch.pr_number, watch.tries_used));
        }
        let others: Vec<FixRun> = io.store().fixes().into_iter().filter(|other| other.id != fix.id).collect();
        let now = io.now_ms();
        if filter::day_count(&others, now) >= settings.fixes_per_day {
            let message = format!("{} started today already.", super::engine::count(settings.fixes_per_day, "fix", "fixes"));
            return self.fail_start(&mut fix, &watch, false, CI_FIX_DAY_CAP, &message);
        }
        if filter::one_running(&others) {
            return self.queue_fix(&mut fix, &watch, "Another fix is running");
        }
        let Some(copy) = io.find_copy(&watch.project, &watch.handle) else {
            return self.fail_start(&mut fix, &watch, false, CI_FIX_COPY_GONE, "The copy is gone.");
        };
        if io.copy_busy(&watch.project, &copy) {
            return self.queue_fix(&mut fix, &watch, "The agent is still working in this copy");
        }
        let gh = self.gh_status();
        if gh.state != "ready" || gh.login.is_empty() {
            let code = if gh.state == "signed_out" { CI_FIX_GH_SIGNED_OUT } else { CI_FIX_GH_MISSING };
            return self.fail_start(&mut fix, &watch, false, code, "Install the GitHub CLI and run gh auth login.");
        }
        if !io.folder_owner_repo(&watch.project).is_some_and(|name| name.eq_ignore_ascii_case(&watch.owner_repo)) {
            return self.fail_start(&mut fix, &watch, false, CI_FIX_NOT_YOURS, "The folder no longer points at this repository.");
        }
        let pull = match self.fresh_pull(&watch, &copy, &gh.login) {
            Ok(pull) => pull,
            Err((code, message)) => return self.refused(&mut fix, &watch, code, &message),
        };
        if pull.head_sha != fix.failing_sha {
            return self.fail_start(&mut fix, &watch, false, CI_FIX_HEAD_MOVED, "The pull request has a newer commit than the one that failed.");
        }
        let env = match io.git_env() {
            Ok(env) => env,
            Err(error) => return self.fail_start(&mut fix, &watch, false, CI_FIX_START_FAILED, &error.message),
        };
        let checked = (|| -> Result<Option<(&'static str, &'static str)>, OperationError> {
            if io.current_branch(&copy, &env)?.as_deref() != Some(watch.branch.as_str()) {
                return Ok(Some((CI_FIX_NOT_PM_BRANCH, "The copy is on another branch.")));
            }
            if io.head_sha(&copy, &env)? != fix.failing_sha {
                return Ok(Some((CI_FIX_HEAD_MOVED, "The copy is not at the commit that failed.")));
            }
            if io.remote_branch_sha(&copy, &env)?.as_deref() != Some(fix.failing_sha.as_str()) {
                return Ok(Some((CI_FIX_REMOTE_MOVED, "Someone changed the branch on GitHub after the failure.")));
            }
            Ok(None)
        })();
        match checked {
            Err(error) => return self.fail_start(&mut fix, &watch, false, CI_FIX_START_FAILED, &error.message),
            Ok(Some((code, message))) => return self.refused(&mut fix, &watch, code, message),
            Ok(None) => {}
        }
        if !io.agent_ready(&watch.agent.agent_type) {
            let message = format!("{} can't start right now.", watch.agent_label);
            return self.fail_start(&mut fix, &watch, false, CI_FIX_AGENT_NOT_READY, &message);
        }
        let fingerprint = match io.fingerprint(&copy) {
            Ok(fingerprint) => fingerprint,
            Err(error) => return self.fail_start(&mut fix, &watch, false, CI_FIX_START_FAILED, &error.message),
        };
        // The log file comes from the stored snapshot the person saw, not from a new read.
        let try_no = watch.tries_used + 1;
        let file_name = format!("ci-{}-try{}.md", watch.pr_number, try_no);
        let body = logs::log_file(watch.pr_number, &fix.failing_sha, &fix.checks, &fix.log);
        if let Err(message) = io.write_log_file(&copy, &file_name, &body) {
            return self.fail_start(&mut fix, &watch, false, CI_FIX_START_FAILED, &message);
        }
        // The try is counted and saved before the agent starts.
        fix.try_no = try_no;
        fix.at_ms = now;
        fix.git_fingerprint = Some(fingerprint);
        fix.state = FixState::Starting;
        self.put_fix(&fix);
        self.edit_watch(&watch.id, |w| w.tries_used = try_no);
        let task = text::task(&watch.owner_repo, watch.pr_number, &watch.branch, &format!(".puppet-master/{file_name}"));
        match self.start_agent(&watch, &fix, &copy, task, &settings) {
            Ok((route, started, pane_id)) => {
                fix.state = FixState::Running { route: route.to_string(), handle: started.handle, operation_id: started.operation_id, pane_id };
                self.put_fix(&fix);
                let who = fix.approved_by.clone().unwrap_or_else(|| by.to_string());
                let mut detail = fix_detail(&watch, &fix);
                detail["route"] = json!(route);
                io.audit("started", &who, &format!("Started a fix for #{}", watch.pr_number), detail);
            }
            Err(error) => self.fail_start(&mut fix, &watch, true, CI_FIX_START_FAILED, &error.message),
        }
    }

    /// A fresh look found the pull request is not ours any more (or closed): the fix ends, and the watch
    /// pauses or closes with it.
    fn refused(&self, fix: &mut FixRun, watch: &Watch, code: &str, message: &str) {
        match code {
            CI_FIX_PR_CLOSED => {
                self.end_cards_of(&watch.id);
                self.edit_watch(&watch.id, |w| w.state = WatchState::Closed);
            }
            CI_FIX_FORK | CI_FIX_NOT_YOURS | CI_FIX_NOT_PM_BRANCH | CI_FIX_HEAD_MOVED | CI_FIX_REMOTE_MOVED => {
                let why = if code == CI_FIX_HEAD_MOVED || code == CI_FIX_REMOTE_MOVED { "Someone else pushed to this branch" } else { message };
                self.pause(watch, code, why);
            }
            _ => {}
        }
        self.fail_start(fix, watch, false, code, message);
    }

    /// Route A (the same chat) only when the run is guarded and its pane is open and idle; else route B
    /// (a new guarded pane in the same copy). Returns the route, what started, and the pane.
    fn start_agent(
        &self,
        watch: &Watch,
        fix: &FixRun,
        copy: &WorktreeRef,
        task: String,
        settings: &Settings,
    ) -> Result<(&'static str, Started, Option<String>), OperationError> {
        let io = self.io();
        let key = format!("cifix:{}", fix.id);
        let facts: Option<RunFacts> = io.run_snapshot(&watch.project, &watch.handle);
        if let Some(facts) = facts.as_ref().filter(|facts| route_a_ok(facts)) {
            match io.followup(&watch.project, &watch.handle, task.clone(), &key) {
                Ok(started) => {
                    let pane = started.pane_id.clone().or_else(|| facts.pane_id.clone());
                    return Ok(("same_pane", started, pane));
                }
                // The conversation cannot take it (moved to another agent, pane closed): a new pane, once.
                Err(error) if ROUTE_A_REFUSALS.contains(&error.code.as_str()) => {}
                Err(error) => return Err(error),
            }
        }
        let request = self.build_request(watch, copy, task, &key, settings)?;
        let started = io.run_agent(request)?;
        let pane = started.pane_id.clone();
        Ok(("new_pane", started, pane))
    }

    /// The `run_agent` request of route B (SPEC 3.5 D3): a visible, kept pane in the same copy under the
    /// `no_push` guard, with the per-try budgets, no shell and no trust.
    fn build_request(
        &self,
        watch: &Watch,
        copy: &WorktreeRef,
        task: String,
        key: &str,
        settings: &Settings,
    ) -> Result<AgentRunRequest, OperationError> {
        let handle = format!("cifix-{}-{}", watch.pr_number, &uuid::Uuid::new_v4().simple().to_string()[..4]);
        let mut request: AgentRunRequest = serde_json::from_value(json!({
            "project_path": watch.project,
            "task": task,
            "agent_type": watch.agent.agent_type,
            "account": watch.agent.account,
            "headless": false,
            "background": true,
            "keep_pane": true,
            "wait_ms": 0,
            "timeout_ms": u64::from(settings.minutes_per_try) * 60_000,
            "budget_turns": settings.turns_per_try,
            "budget_minutes": settings.minutes_per_try,
            "idempotency_key": key,
            "name": format!("Fix CI #{}", watch.pr_number),
            "handle": handle,
            "trust_workspace": false,
            "allow_shell": false,
            "allowed_commands": null,
            "read_only": false,
            "context_policy": "fresh",
        }))
        .map_err(|error| err(CI_FIX_START_FAILED, &error.to_string()))?;
        request.join_worktree = Some(copy.clone());
        request.guard_env = Some(env_guard::NO_PUSH.to_string());
        Ok(request)
    }

    // ---------------------------------------------------------------------------- E. finish

    /// Flow E. The agent's turn is over: save its edits as a commit, look at the change, and raise the push
    /// card (or a held card, or a notice that nothing changed).
    pub(super) fn finish_job(self: &Arc<Self>, fix_id: &str) {
        let io = self.io();
        let Some(mut fix) = io.store().fix_by_id(fix_id) else { return };
        let running = matches!(fix.state, FixState::Running { .. });
        if !running && !matches!(fix.state, FixState::Checking) {
            return;
        }
        let Some(watch) = self.watch_of(&fix) else {
            return self.fail(&mut fix, CI_FIX_START_FAILED, "Puppet Master isn't watching this pull request any more.");
        };
        let snap = io.run_snapshot(&watch.project, &watch.handle);
        if running && snap.as_ref().is_some_and(|snap| !snap.terminal) {
            return;
        }
        let Some(copy) = io.find_copy(&watch.project, &watch.handle) else {
            return self.fail(&mut fix, CI_FIX_COPY_GONE, "The copy is gone, so there is nothing to check.");
        };
        // A follow-up typed into the pane: wait until the agent is idle again.
        if io.copy_busy(&watch.project, &copy) {
            return;
        }
        fix.state = FixState::Checking;
        self.put_fix(&fix);
        if !matches!(watch.state, WatchState::Watching) {
            fix.state = FixState::NotPushed;
            return self.put_fix(&fix);
        }
        let pane_id = snap.as_ref().filter(|snap| snap.pane_open).and_then(|snap| snap.pane_id.clone());
        let env = match io.git_env().and_then(|env| io.commit_pending(&copy, &env).map(|_| env)) {
            Ok(env) => env,
            Err(error) => return self.check_failed(&mut fix, &watch, &error.message, pane_id),
        };
        let head = match io.head_sha(&copy, &env) {
            Ok(head) => head,
            Err(error) => return self.check_failed(&mut fix, &watch, &error.message, pane_id),
        };
        if head == fix.failing_sha {
            let message = pane_id
                .as_deref()
                .and_then(|pane| io.turn_sentence(pane))
                .map(|sentence| clean_message(&sentence))
                .unwrap_or_else(|| "The agent finished without changing any files.".to_string());
            fix.state = FixState::NoChanges { message: message.clone() };
            self.put_fix(&fix);
            self.raise_follow_up(
                &fix,
                CARD_KIND_PUSH,
                &format!("No fix for #{}", watch.pr_number),
                format!("{} · #{}\n{}", watch.owner_repo, watch.pr_number, message),
                vec![choice(CHOICE_LOOK, "Open the agent"), choice(CHOICE_GOT_IT, "Got it")],
                pane_id,
                CardKind::NoChanges,
            );
            return;
        }
        let facts = match self.gather_facts(&watch, &fix, &copy, &env, snap.as_ref()) {
            Ok(facts) => facts,
            Err(error) => return self.check_failed(&mut fix, &watch, &error.message, pane_id),
        };
        let (reasons, held) = fixcheck::reasons(&facts);
        if held {
            return self.hold(&mut fix, &watch, &facts, &reasons, pane_id);
        }
        // Stored as `AskPush` before the card can be seen, so an answer always finds its fix.
        let id = uuid::Uuid::new_v4().to_string();
        fix.state = FixState::AskPush { question_id: id.clone(), facts: facts.clone() };
        self.put_fix(&fix);
        if self.raise_push_card(&watch, &fix, &facts, Some(id)).is_some() {
            let who = fix.approved_by.clone().unwrap_or_else(|| "auto".to_string());
            io.audit("asked_push", &who, &format!("Asked about pushing the fix for #{}", watch.pr_number), fix_detail(&watch, &fix));
        }
    }

    /// Read the copy: the files, the patch, the git settings, the branch and the remote. Fails closed: any read
    /// that fails is an error, never "nothing found".
    pub(super) fn gather_facts(
        &self,
        watch: &Watch,
        fix: &FixRun,
        copy: &WorktreeRef,
        env: &[(String, String)],
        snap: Option<&RunFacts>,
    ) -> Result<FixFacts, OperationError> {
        let io = self.io();
        let base = fix.failing_sha.as_str();
        let files = io.changed_files(copy, base, env)?;
        let diff = io.push_diff(copy, base, env)?;
        let statuses = io.name_status(copy, base, env)?;
        let (secret_lines, home_lines) = io.scan_added(&diff.text);
        let now_fingerprint = io.fingerprint(copy)?;
        let remote = io.remote_branch_sha(copy, env)?;
        let head = io.head_sha(copy, env)?;
        let ancestor = io.is_ancestor(copy, base, &head, env)?;
        let branch = io.current_branch(copy, env)?;
        let commits = io.commit_count(copy, base, env)?;
        let (additions, deletions) = pushcheck::diff_totals(&diff.text);
        let push = crate::issue_intake::model::PushFacts {
            files: u32::try_from(files.len()).unwrap_or(u32::MAX),
            additions,
            deletions,
            ci_files: names(pushcheck::ci_paths(&files)),
            secret_files: names(pushcheck::secret_paths(&files)),
            secret_lines: places(secret_lines),
            home_path_lines: places(home_lines),
            internal_files: names(pushcheck::internal_paths(&files)),
            git_changed: fix.git_fingerprint.as_deref() != Some(now_fingerprint.as_str()),
            remote_branch_exists: false,
            issue_changed: false,
            run_status: snap.map_or_else(|| "unknown".to_string(), |snap| snap.status.clone()),
            turns: snap.map_or(0, |snap| snap.turns),
            minutes: snap.map_or(0, |snap| snap.minutes),
        };
        Ok(FixFacts {
            push,
            skipped_tests: places(fixcheck::skipped_tests(&diff.text)),
            deleted_tests: names(fixcheck::deleted_tests(&statuses)),
            ci_run_files: names(fixcheck::ci_run_files(&files)),
            web_lines: places(fixcheck::web_lines(&diff.text)),
            remote_moved: remote.as_deref() != Some(base),
            history_rewritten: !ancestor,
            wrong_branch: branch.as_deref() != Some(watch.branch.as_str()),
            over_cap: diff.over_cap,
            commits,
        })
    }

    /// The push card: `Push the fix to #61?` with the facts and the reasons, `Push fix` / `Look first` / `Don't push`.
    pub(super) fn raise_push_card(&self, watch: &Watch, fix: &FixRun, facts: &FixFacts, id: Option<String>) -> Option<String> {
        let (reasons, _) = fixcheck::reasons(facts);
        let new = crate::questions::NewQuestion {
            pane_id: self.pane_of(watch),
            question: text::push_question(watch.pr_number),
            choices: vec![choice(CHOICE_PUSH, "Push fix"), choice(CHOICE_LOOK, "Look first"), choice(CHOICE_KEEP, "Don't push")],
            allow_free_text: Some(false),
            context: Some(text::push_context(watch, facts, &reasons)),
            timeout_s: Some(CARD_TIMEOUT_S),
            urgency: Some("normal".to_string()),
            kind: Some(CARD_KIND_PUSH.to_string()),
            ..crate::questions::NewQuestion::default()
        };
        let asked = match &id {
            Some(id) => self.io().ask_as(new, id, Some(&fix.id)),
            None => self.io().ask(new, Some(&fix.id)),
        };
        asked.map_err(|message| tracing::warn!(%message, "ci-fix: could not raise a card")).ok()
    }

    /// A guard stopped the push: save `Held` and raise a card with no push button.
    fn hold(&self, fix: &mut FixRun, watch: &Watch, facts: &FixFacts, reasons: &[String], pane_id: Option<String>) {
        let held_line = "Puppet Master won't push this. Check the copy yourself.";
        let message: String = reasons.iter().map(|r| r.as_str()).filter(|r| *r != held_line).collect::<Vec<_>>().join(" ");
        let message: String = message.chars().take(300).collect();
        let message = if message.is_empty() { held_line.to_string() } else { message };
        fix.state = FixState::Held { code: held_code(facts).to_string(), message: message.clone() };
        self.put_fix(fix);
        self.raise_held_card(fix, watch, &message, pane_id);
        let who = fix.approved_by.clone().unwrap_or_else(|| "auto".to_string());
        let mut detail = fix_detail(watch, fix);
        detail["code"] = json!(held_code(facts));
        self.io().audit("held", &who, &format!("Held the push for #{}", watch.pr_number), detail);
    }

    pub(super) fn raise_held_card(&self, fix: &FixRun, watch: &Watch, message: &str, pane_id: Option<String>) {
        self.raise_follow_up(
            fix,
            CARD_KIND_PUSH,
            &format!("The fix for #{} is on hold", watch.pr_number),
            format!("{} · #{}\n{} {}", watch.owner_repo, watch.pr_number, message, "Puppet Master won't push this."),
            vec![choice(CHOICE_LOOK, "Look first"), choice(CHOICE_KEEP, "Don't push")],
            pane_id.or_else(|| self.pane_of(watch)),
            CardKind::Held,
        );
    }

    /// The check could not be done. The copy stays; `Try again` runs it again.
    fn check_failed(&self, fix: &mut FixRun, watch: &Watch, message: &str, pane_id: Option<String>) {
        let message = clean_message(message);
        fix.state = FixState::Failed { code: CI_FIX_PUSH_FAILED.to_string(), message: message.clone() };
        self.put_fix(fix);
        self.raise_retry_card(fix, watch, &message, pane_id);
    }

    fn raise_retry_card(&self, fix: &FixRun, watch: &Watch, message: &str, pane_id: Option<String>) {
        self.raise_follow_up(
            fix,
            CARD_KIND_PUSH,
            &format!("Couldn't push the fix for #{}", watch.pr_number),
            format!("{} · #{}\n{}", watch.owner_repo, watch.pr_number, message),
            vec![choice(CHOICE_RETRY, "Try again"), choice(CHOICE_GOT_IT, "Got it")],
            pane_id.or_else(|| self.pane_of(watch)),
            CardKind::RetryPush,
        );
    }

    // ------------------------------------------------------------------------------ F. push

    /// Flow F. The fix is `Pushing` and claimed. Looks at the copy and at GitHub again, then pushes the new
    /// commits to the same `pm/` branch (never forced). `shown` is what the card showed; a change that moved
    /// since goes back to a new card.
    pub(super) fn push_job(self: &Arc<Self>, fix_id: &str, shown: Option<FixFacts>) {
        let io = self.io();
        let Some(mut fix) = io.store().fix_by_id(fix_id) else { return };
        if fix.state != FixState::Pushing {
            return;
        }
        let Some(watch) = self.watch_of(&fix) else {
            return self.fail(&mut fix, CI_FIX_PUSH_FAILED, "Puppet Master isn't watching this pull request any more.");
        };
        if !matches!(watch.state, WatchState::Watching) {
            fix.state = FixState::NotPushed;
            return self.put_fix(&fix);
        }
        let Some(copy) = io.find_copy(&watch.project, &watch.handle) else {
            return self.fail(&mut fix, CI_FIX_PUSH_FAILED, "The copy is gone, so there is nothing to push.");
        };
        let pane_id = self.pane_of(&watch);
        let env = match io.git_env().and_then(|env| io.commit_pending(&copy, &env).map(|_| env)) {
            Ok(env) => env,
            Err(error) => return self.push_failed(&mut fix, &watch, &error.message, pane_id),
        };
        let snap = io.run_snapshot(&watch.project, &watch.handle);
        let facts = match self.gather_facts(&watch, &fix, &copy, &env, snap.as_ref()) {
            Ok(facts) => facts,
            Err(error) => return self.push_failed(&mut fix, &watch, &error.message, pane_id),
        };
        let (reasons, held) = fixcheck::reasons(&facts);
        if held {
            return self.hold(&mut fix, &watch, &facts, &reasons, pane_id);
        }
        if shown.as_ref().is_some_and(|shown| !same_change(shown, &facts)) {
            // The change moved after the person looked: ask again about what is there now.
            let id = uuid::Uuid::new_v4().to_string();
            fix.state = FixState::AskPush { question_id: id.clone(), facts: facts.clone() };
            self.put_fix(&fix);
            self.raise_push_card(&watch, &fix, &facts, Some(id));
            return;
        }
        let gh = self.gh_status();
        if gh.state != "ready" || gh.login.is_empty() {
            return self.push_failed(&mut fix, &watch, "Install the GitHub CLI and run gh auth login.", pane_id);
        }
        if !io.folder_owner_repo(&watch.project).is_some_and(|name| name.eq_ignore_ascii_case(&watch.owner_repo)) {
            return self.hold_with(&mut fix, &watch, CI_FIX_NOT_YOURS, "The folder no longer points at this repository.", pane_id);
        }
        let repo = match io.repo_facts(Path::new(&copy.path)) {
            Ok(repo) if repo.name_with_owner.eq_ignore_ascii_case(&watch.owner_repo) => repo,
            Ok(_) => return self.hold_with(&mut fix, &watch, CI_FIX_NOT_YOURS, "GitHub says the copy is a different repository.", pane_id),
            Err(message) => return self.push_failed(&mut fix, &watch, &message, pane_id),
        };
        let pull = match self.fresh_pull(&watch, &copy, &gh.login) {
            Ok(pull) => pull,
            Err((CI_FIX_PR_CLOSED, _)) => {
                self.end_cards_of(&watch.id);
                self.edit_watch(&watch.id, |w| w.state = WatchState::Closed);
                fix.state = FixState::NotPushed;
                return self.put_fix(&fix);
            }
            Err((code, message)) => return self.hold_with(&mut fix, &watch, code, &message, pane_id),
        };
        if pull.head_sha != fix.failing_sha {
            return self.hold_with(&mut fix, &watch, CI_FIX_HEAD_MOVED, "The pull request has a newer commit than the one that failed.", pane_id);
        }
        match io.push_fix(&watch.project, &watch.handle, &fix.failing_sha, repo.default_branch.as_deref(), &env) {
            Ok(sha) if valid_sha(&sha) => self.pushed(&mut fix, &watch, &sha),
            Ok(_) => self.push_failed(&mut fix, &watch, "Git gave no commit for the pushed branch.", pane_id),
            Err(error) if error.code == CI_FIX_REMOTE_MOVED => {
                self.pause(&watch, CI_FIX_REMOTE_MOVED, "Someone else pushed to this branch");
                self.hold_with(&mut fix, &watch, CI_FIX_REMOTE_MOVED, &error.message, pane_id);
            }
            Err(error) => self.push_failed(&mut fix, &watch, &error.message, pane_id),
        }
    }

    /// The new commit is on GitHub: remember it as ours, and look at the checks soon.
    fn pushed(&self, fix: &mut FixRun, watch: &Watch, sha: &str) {
        let now = self.io().now_ms();
        self.edit_watch(&watch.id, |w| {
            w.pushed_shas.push(sha.to_string());
            while w.pushed_shas.len() > PUSHED_SHAS_CAP {
                w.pushed_shas.remove(0);
            }
            w.last_push_ms = now;
        });
        fix.state = FixState::Pushed { sha: sha.to_string() };
        self.put_fix(fix);
        self.poll_soon(&watch.id);
        let who = fix.approved_by.clone().unwrap_or_else(|| "auto".to_string());
        self.io().audit("pushed", &who, &format!("Pushed a fix to #{}", watch.pr_number), fix_detail(watch, fix));
    }

    /// The push is not allowed now (the pull request or the folder is not what it was): held, no push button.
    fn hold_with(&self, fix: &mut FixRun, watch: &Watch, code: &str, message: &str, pane_id: Option<String>) {
        let message = clean_message(message);
        fix.state = FixState::Held { code: code.to_string(), message: message.clone() };
        self.put_fix(fix);
        self.raise_held_card(fix, watch, &message, pane_id);
        let who = fix.approved_by.clone().unwrap_or_else(|| "auto".to_string());
        let mut detail = fix_detail(watch, fix);
        detail["code"] = json!(code);
        self.io().audit("held", &who, &format!("Held the push for #{}", watch.pr_number), detail);
    }

    /// The push failed. The commits stay in the copy; `Try again` pushes again.
    fn push_failed(&self, fix: &mut FixRun, watch: &Watch, message: &str, pane_id: Option<String>) {
        let message = clean_message(message);
        fix.state = FixState::Failed { code: CI_FIX_PUSH_FAILED.to_string(), message: message.clone() };
        self.put_fix(fix);
        self.raise_retry_card(fix, watch, &message, pane_id);
    }

    /// A restart cut a push off: if the remote branch is now the copy's head, the push went through.
    pub(super) fn resolve_pushing(self: &Arc<Self>, fix_id: &str) {
        let io = self.io();
        let Some(mut fix) = io.store().fix_by_id(fix_id) else { return };
        if fix.state != FixState::Pushing {
            return;
        }
        let Some(watch) = self.watch_of(&fix) else {
            return self.fail(&mut fix, CI_FIX_PUSH_FAILED, "Puppet Master isn't watching this pull request any more.");
        };
        let Some(copy) = io.find_copy(&watch.project, &watch.handle) else {
            return self.fail(&mut fix, CI_FIX_PUSH_FAILED, "The copy is gone.");
        };
        let pane_id = self.pane_of(&watch);
        let read = io.git_env().and_then(|env| Ok((io.remote_branch_sha(&copy, &env)?, io.head_sha(&copy, &env)?)));
        match read {
            Ok((Some(remote), head)) if remote == head && head != fix.failing_sha => self.pushed(&mut fix, &watch, &head),
            Ok(_) => self.push_failed(&mut fix, &watch, "Puppet Master restarted while pushing. Nothing was pushed.", pane_id),
            Err(error) => self.push_failed(&mut fix, &watch, &error.message, pane_id),
        }
    }

    // ------------------------------------------------------------------------------ G. stop

    /// The person stops watching (a card, or the row). Open cards of that watch go.
    pub(super) fn stop_by_person(&self, watch: &Watch, who: &str) {
        self.stop(watch, "You stopped watching");
        self.io().audit("stopped", who, &format!("Stopped watching #{}", watch.pr_number), json!({ "watch": watch.id }));
    }
}
