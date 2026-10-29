//! The live wiring of branches to the running app (SPEC n33 section 3.7). Desktop only.
//!
//! - `preview`: what the sheet shows. Reads only.
//! - `start`: every guard and the brief on the caller's thread, then a thread named `branch-from` makes
//!   the copy and starts the agent in it. Git never runs on an async or PTY reader thread.
//! - `lineage`, `brief`: what the banner and "Show brief" read.
//!
//! The source pane is only ever READ: its save points, its chat items and its turn summaries. Nothing here
//! writes to a pane, stops it, or answers a trust, permission or login screen of the new agent: the new
//! run starts with trust and shell off, so those screens park it and raise the usual cards. The window's
//! save point is never trusted: it is resolved again from the pane's own save points on every start.

use super::brief::{self, BriefInputs};
use super::live_io::{is_open, CopyRequest, FlightGuard, LiveIo, PaneFacts, StartRequest};
use super::model::*;
use super::offers;
use super::point::{self, Resolved};
use super::store::new_id;
use super::view::{point_text, BranchView, BriefPreview, Lineage, PointView, Preview};
use crate::audit::Who;
use crate::handoff::keep_going::model::{Offer, Target};
use crate::operations::OperationError;
use crate::session_chat::model::ChatItem;
use serde_json::json;
use std::sync::Arc;

/// A refusal: a stable code and its plain words.
type Refusal = (&'static str, String);

/// Characters of the person's message in the new run's name.
const NAME_MESSAGE_CHARS: usize = 50;
/// Paths of files that could not be written, kept in the record.
const FAILED_PATHS_KEPT: usize = 20;

// ---- the preview ----------------------------------------------------------------------------------

/// What the sheet asks about. `items` are the source pane's chat items (the caller reads them).
pub struct PreviewInput<'a> {
    pub pane_id: &'a str,
    pub from: &'a BranchFrom,
    /// The picked agent (`agent` or `agent@account`). The brief does not depend on it; it is accepted so
    /// the window can send one request shape.
    pub target: Option<&'a str>,
    /// The person's message so far. With one, the preview holds the brief text.
    pub message: Option<&'a str>,
    pub items: &'a [ChatItem],
}

/// What the sheet shows. Never writes anything.
pub fn preview(io: &dyn LiveIo, input: &PreviewInput) -> Preview {
    let _ = input.target;
    let facts = match gate(io, input.pane_id) {
        Ok(facts) => facts,
        Err((_, why)) => return Preview::refused("", why),
    };
    let from_label = facts.agent_label.clone();
    let planned = plan(io, &facts, input.from, input.items);
    let (resolved, summaries) = match planned {
        Ok(planned) => planned,
        Err((_, why)) => return Preview::refused(&from_label, why),
    };
    let open = open_branches(io, &facts.pane_id);
    let (first_task, turns) = turn_lines(input.items, &summaries, &resolved);
    let turn_count = u32::try_from(turns.len()).unwrap_or(u32::MAX);
    let message = input.message.map(str::trim).filter(|text| !text.is_empty() && text.chars().count() <= MESSAGE_MAX_CHARS);
    let brief = message.map(|message| {
        let made = build_brief(io, &facts, &resolved, first_task, turns, message, &format!("pm/{PLACEHOLDER_ID}"));
        BriefPreview { text: made.text, bytes: made.bytes, trimmed: made.trimmed.iter().map(|step| step.to_string()).collect() }
    });
    let (offers, _) = offers_for(io, &facts);
    let too_many = open >= u32::try_from(OPEN_PER_PANE_CAP).unwrap_or(u32::MAX);
    Preview {
        from_label,
        point: Some(point_view(&resolved)),
        can_branch: !too_many,
        why_not: too_many.then(|| MSG_BRANCH_TOO_MANY.to_string()),
        offers,
        prefill: resolved.prefill.clone(),
        turns: turn_count,
        open_branches: open,
        brief,
    }
}

/// The branch name shown in a preview, before an id exists.
const PLACEHOLDER_ID: &str = "bf-xxxxxxxxxxxx";

// ---- the start ------------------------------------------------------------------------------------

/// What the sheet's Start button sends.
pub struct StartInput {
    pub pane_id: String,
    pub from: BranchFrom,
    /// `agent` or `agent@account`.
    pub target: String,
    pub message: String,
    pub items: Vec<ChatItem>,
    pub who: Who,
}

/// A branch that passed every guard: the record is saved as `Preparing`, the pane is marked in flight,
/// and nothing has been copied yet. Run it with [`run`].
pub struct Job {
    _flight: FlightGuard,
    record: Branch,
    brief_text: String,
    target: Target,
    name: String,
    commit_message: String,
    checkpoint_id: String,
    who: Who,
}

impl Job {
    /// The `Preparing` view `start` answers with.
    pub fn view(&self) -> BranchView {
        BranchView::of(&self.record, None)
    }
}

/// Start a branch: guards, the brief and the `Preparing` record now, the copy and the agent on a thread
/// named `branch-from`. Returns the `Preparing` view at once.
pub fn start(io: Arc<dyn LiveIo>, input: StartInput) -> Result<BranchView, OperationError> {
    let job = prepare(&*io, input)?;
    let view = job.view();
    let id = job.record.id.clone();
    let worker = io.clone();
    let spawned = std::thread::Builder::new().name("branch-from".into()).spawn(move || {
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&*worker, job)));
        if ran.is_err() {
            fail_unfinished(&*worker, &id);
        }
    });
    if spawned.is_err() {
        fail_unfinished(&*io, &view.id);
    }
    Ok(view)
}

/// Flow B steps 1 and 2: every guard in order, the brief, the record, the first `branch://changed`.
pub fn prepare(io: &dyn LiveIo, input: StartInput) -> Result<Job, OperationError> {
    let StartInput { pane_id, from, target, message, items, who } = input;
    let facts = gate(io, &pane_id).map_err(refused)?;
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err(refused((ERR_BRANCH_EMPTY_MESSAGE, MSG_BRANCH_EMPTY_MESSAGE.to_string())));
    }
    if message.chars().count() > MESSAGE_MAX_CHARS {
        return Err(refused((ERR_BRANCH_MESSAGE_TOO_LONG, MSG_BRANCH_MESSAGE_TOO_LONG.to_string())));
    }
    let (target, offer) = pick(io, &facts, &target).map_err(refused)?;
    let flight = io
        .in_flight()
        .enter(&pane_id)
        .ok_or_else(|| refused((ERR_BRANCH_BUSY, MSG_BRANCH_BUSY.to_string())))?;
    if open_branches(io, &pane_id) >= u32::try_from(OPEN_PER_PANE_CAP).unwrap_or(u32::MAX) {
        return Err(refused((ERR_BRANCH_TOO_MANY, MSG_BRANCH_TOO_MANY.to_string())));
    }
    let (resolved, summaries) = plan(io, &facts, &from, &items).map_err(refused)?;

    let id = new_id();
    let (first_task, turns) = turn_lines(&items, &summaries, &resolved);
    let made = build_brief(io, &facts, &resolved, first_task, turns, &message, &format!("pm/{id}"));
    let file = io
        .store()
        .write_brief(&id, &made.text)
        .map_err(|error| save_failed(&error.to_string()))?;
    let parent_branch = io.store().to_pane(&pane_id).map(|record| record.id);
    let record = Branch {
        id: id.clone(),
        at_ms: io.now_ms(),
        from: SourceSide {
            pane_id: pane_id.clone(),
            agent_id: facts.agent_id.clone(),
            account: facts.account.clone(),
            label: facts.label(),
            cwd: facts.cwd.clone(),
        },
        point: resolved.point.clone(),
        when: match &from {
            BranchFrom::Message { when, .. } => Some(*when),
            BranchFrom::SavePoint { .. } => None,
        },
        to: NewSide {
            agent_id: target.agent_id.clone(),
            account: target.account.clone(),
            label: offer.label.clone(),
            pane_id: String::new(),
            handle: None,
            operation_id: None,
        },
        project: facts.origin.clone(),
        copy: None,
        parent_branch,
        brief: BriefMeta {
            file,
            sha8: made.sha8.clone(),
            bytes: made.bytes,
            turns: made.turns,
            trimmed: made.trimmed.iter().map(|step| step.to_string()).collect(),
        },
        state: State::Preparing,
    };
    io.store().put(&record).map_err(|error| save_failed(&error))?;
    io.changed(&[&pane_id]);
    let name = run_name(io, &message);
    let commit_message = commit_message(&resolved);
    Ok(Job {
        _flight: flight,
        record,
        brief_text: made.text,
        target,
        name,
        commit_message,
        checkpoint_id: resolved.point.checkpoint_id,
        who,
    })
}

/// Flow B steps 3 to 5, on the `branch-from` thread. Every failure is recorded; the pane's in-flight
/// mark ends when `job` is dropped, on every path.
pub fn run(io: &dyn LiveIo, job: Job) {
    let Job { _flight, mut record, brief_text, target, name, commit_message, checkpoint_id, who } = job;
    let source = record.from.pane_id.clone();

    // 3. the copy. A failure throws the half-made copy away (inside `make_copy`); no agent ran in it.
    let made = io.make_copy(&CopyRequest {
        origin: record.project.clone(),
        handle: record.id.clone(),
        source_cwd: record.from.cwd.clone(),
        pane_id: source.clone(),
        checkpoint_id,
        commit_message,
    });
    let made = match made {
        Ok(made) => made,
        Err(error) => {
            let message = if error.code == ERR_BRANCH_COPY_FAILED { error.message } else { copy_failed_text(&io.clean(&error.message)) };
            return finish(io, &mut record, State::Failed { code: ERR_BRANCH_COPY_FAILED.into(), message }, &[&source]);
        }
    };
    record.copy = Some(CopyFacts {
        handle: made.worktree.handle.clone(),
        branch: made.worktree.branch.clone(),
        path: made.worktree.path.clone(),
        start_commit: made.start_commit.clone(),
        written: made.export.written,
        removed: made.export.removed,
        failed: made.export.failed.iter().take(FAILED_PATHS_KEPT).cloned().collect(),
    });
    // 4. the agent, in the copy, with every gate closed.
    finish(io, &mut record, State::Starting, &[&source]);
    let started = io.start(&StartRequest {
        project_path: record.project.clone(),
        task: brief_text,
        agent_type: target.agent_id.clone(),
        account: target.account.clone(),
        headless: false,
        background: true,
        keep_pane: true,
        wait_ms: 0,
        timeout_ms: RUN_TIMEOUT_MS,
        idempotency_key: format!("branch:{}", record.id),
        name,
        handle: record.id.clone(),
        trust_workspace: false,
        allow_shell: false,
        allowed_commands: None,
        read_only: false,
        context_policy: "fresh".to_string(),
        guard_env: crate::pty::env_guard::NO_PUSH.to_string(),
        join_worktree: made.worktree.clone(),
    });
    let started = match started {
        Ok(started) => started,
        Err(error) => {
            tracing::warn!(code = %error.code, "a branch's agent could not start; the copy is kept");
            let message = MSG_BRANCH_START_FAILED.replace("<Agent>", &record.to.label);
            return finish(io, &mut record, State::Failed { code: ERR_BRANCH_START_FAILED.into(), message }, &[&source]);
        }
    };
    // 5. nest the new pane under the old one, record it, audit it.
    let new_pane = started.pane_id.clone().unwrap_or_default();
    if !new_pane.is_empty() {
        io.set_parent(&new_pane, &source);
    }
    record.to.pane_id = new_pane.clone();
    record.to.handle = Some(started.handle);
    record.to.operation_id = Some(started.operation_id);
    let summary = format!("Branched {} to {} from {}", record.from.label, record.to.label, record.point.label);
    io.audit(who, &summary, json!({ "id": record.id, "pane": source, "point": record.point.checkpoint_id }));
    finish(io, &mut record, State::Running, &[&source, &new_pane]);
}

/// Save `record` in `state` and tell the windows.
fn finish(io: &dyn LiveIo, record: &mut Branch, state: State, panes: &[&str]) {
    record.state = state;
    if let Err(error) = io.store().put(record) {
        tracing::warn!(%error, "could not save a branch record");
    }
    let panes: Vec<&str> = panes.iter().copied().filter(|pane| !pane.is_empty()).collect();
    io.changed(&panes);
}

/// A branch whose thread ended without recording an outcome (a panic, or no thread): mark it failed so the
/// banner never says "Making a copy" forever.
pub(super) fn fail_unfinished(io: &dyn LiveIo, id: &str) {
    let Some(mut record) = io.store().by_id(id) else { return };
    if !matches!(record.state, State::Preparing | State::Starting) {
        return;
    }
    let state = if record.copy.is_some() {
        State::Failed {
            code: ERR_BRANCH_START_FAILED.into(),
            message: MSG_BRANCH_START_FAILED.replace("<Agent>", &record.to.label),
        }
    } else {
        State::Failed { code: ERR_BRANCH_COPY_FAILED.into(), message: copy_failed_text("something went wrong") }
    };
    let source = record.from.pane_id.clone();
    finish(io, &mut record, state, &[&source]);
}

// ---- lineage and brief ----------------------------------------------------------------------------

/// Both sides of one pane's branches: the one that made it, and the ones it made (newest first).
pub fn lineage(io: &dyn LiveIo, pane_id: &str) -> Lineage {
    if pane_id.is_empty() {
        return Lineage { from: None, children: Vec::new() };
    }
    let records = io.store().records();
    let view = |record: &Branch| {
        let closed = if record.copy.is_some() { io.worktree_closed(&record.project, &record.id) } else { None };
        BranchView::of(record, closed)
    };
    Lineage {
        from: records.iter().find(|record| record.to.pane_id == pane_id).map(view),
        children: records.iter().filter(|record| record.from.pane_id == pane_id).map(view).collect(),
    }
}

/// The stored (already cleaned) brief of one branch. A bad id reads nothing.
pub fn brief(io: &dyn LiveIo, id: &str) -> Result<String, OperationError> {
    io.store()
        .read_brief(id)
        .map_err(|_| OperationError::new(ERR_BRANCH_NOT_HERE, "That branch has no brief.", false))
}

// ---- guards ---------------------------------------------------------------------------------------

/// The pane is open and can branch. Companions, mission rounds, panes whose agent takes no part in save
/// points, and panes with Undo off cannot.
fn gate(io: &dyn LiveIo, pane_id: &str) -> Result<PaneFacts, Refusal> {
    let facts = io.pane(pane_id).ok_or_else(not_here)?;
    if facts.companion || facts.mission || !facts.takes_part {
        return Err(not_here());
    }
    if !facts.checkpoints_on {
        return Err((ERR_BRANCH_NO_SAVE_POINT, MSG_BRANCH_NO_SAVE_POINT.to_string()));
    }
    Ok(facts)
}

/// The save point, from the pane's own save points (never the window's), and Git in the origin folder.
fn plan(
    io: &dyn LiveIo,
    facts: &PaneFacts,
    from: &BranchFrom,
    items: &[ChatItem],
) -> Result<(Resolved, Vec<crate::turn_summary::TurnSummary>), Refusal> {
    let checkpoints = io
        .checkpoints(&facts.pane_id)
        .ok_or((ERR_BRANCH_NO_SAVE_POINT, MSG_BRANCH_NO_SAVE_POINT.to_string()))?;
    let summaries = io.summaries(&facts.pane_id);
    let resolved = point::resolve(from, items, &summaries, &checkpoints)?;
    match io.git_state(&facts.origin) {
        "repo" => Ok((resolved, summaries)),
        "no_commits" => Err((ERR_BRANCH_NO_COMMITS, MSG_BRANCH_NO_COMMITS.to_string())),
        _ => Err((ERR_BRANCH_NEEDS_GIT, MSG_BRANCH_NEEDS_GIT.to_string())),
    }
}

/// The picked agent: it must parse, be listed, and be ready (not limited).
fn pick(io: &dyn LiveIo, facts: &PaneFacts, text: &str) -> Result<(Target, Offer), Refusal> {
    let bad = || (ERR_BRANCH_BAD_TARGET, MSG_BRANCH_BAD_TARGET.to_string());
    let target = Target::parse(text).ok_or_else(bad)?;
    let (offers, candidates) = offers_for(io, facts);
    let offer = offers.into_iter().find(|offer| offer.target == target).ok_or_else(bad)?;
    if offer.ok {
        return Ok((target, offer));
    }
    // A live usage limit has its own words; every other reason is just "can't be picked".
    let limited = candidates.iter().find(|candidate| candidate.target == target).and_then(|candidate| candidate.limited_until);
    match limited {
        Some(reset) if candidates.iter().any(|c| c.target == target && c.installed && c.runnable.is_ok()) => {
            let text = match reset {
                Some(ms) => format!("{} is out of usage until {}.", offer.label, io.when_text(ms)),
                None => format!("{} is out of usage.", offer.label),
            };
            Err((ERR_BRANCH_TARGET_LIMITED, text))
        }
        _ => Err(bad()),
    }
}

/// The picker rows for `facts`' pane, and the candidates they came from.
fn offers_for(io: &dyn LiveIo, facts: &PaneFacts) -> (Vec<Offer>, Vec<offers::Candidate>) {
    let candidates = io.candidates(facts);
    let when = |ms: u64| io.when_text(ms);
    (offers::rank(&facts.target(), &candidates, io.now_ms(), &when), candidates)
}

/// Branch copies of this pane that still count against the cap.
fn open_branches(io: &dyn LiveIo, pane_id: &str) -> u32 {
    let count = io.store().records().iter().filter(|record| record.from.pane_id == pane_id && is_open(io, record)).count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn not_here() -> Refusal {
    (ERR_BRANCH_NOT_HERE, MSG_BRANCH_NOT_HERE.to_string())
}

fn refused((code, message): Refusal) -> OperationError {
    OperationError::new(code, message, true)
}

// ---- the brief ------------------------------------------------------------------------------------

/// The chat turns up to the save point: from the chat items, else from the turn summaries.
fn turn_lines(
    items: &[ChatItem],
    summaries: &[crate::turn_summary::TurnSummary],
    resolved: &Resolved,
) -> (Option<String>, Vec<brief::TurnLine>) {
    let (first_task, turns) = brief::turns_from_items(items, &resolved.cut);
    if !turns.is_empty() {
        return (first_task, turns);
    }
    // A save point taken before turn N has turns below N; one taken after turn N has turns up to N.
    let limit = if resolved.point.kind == "before" { resolved.point.turn } else { resolved.point.turn.saturating_add(1) };
    (None, brief::turns_from_summaries(summaries, limit))
}

fn build_brief(
    io: &dyn LiveIo,
    facts: &PaneFacts,
    resolved: &Resolved,
    first_task: Option<String>,
    turns: Vec<brief::TurnLine>,
    message: &str,
    branch_name: &str,
) -> brief::Brief {
    let inputs = BriefInputs {
        from_label: facts.agent_label.clone(),
        point_text: point_text(&resolved.point.label),
        point_hhmm: brief::hhmm_of_ms(resolved.point.at_ms).unwrap_or_default(),
        branch_name: branch_name.to_string(),
        first_task,
        turns,
        message: message.to_string(),
    };
    brief::build(&inputs, &|text| io.clean(text))
}

fn point_view(resolved: &Resolved) -> PointView {
    PointView {
        checkpoint_id: resolved.point.checkpoint_id.clone(),
        label: resolved.point.label.clone(),
        at_ms: resolved.point.at_ms,
        files: resolved.point.files,
    }
}

/// "Branch: <message, cleaned, 50 chars>" for the new run's name.
fn run_name(io: &dyn LiveIo, message: &str) -> String {
    let cleaned = io.clean(message);
    let one_line = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = one_line.chars().take(NAME_MESSAGE_CHARS).collect();
    format!("Branch: {cut}")
}

/// "Puppet Master: start from save point (after turn 5, 14:02)": one plain line, at most 120 characters.
fn commit_message(resolved: &Resolved) -> String {
    let time = brief::hhmm_of_ms(resolved.point.at_ms).map(|t| format!(", {t}")).unwrap_or_default();
    let text = format!("Puppet Master: start from save point ({}{time})", point_text(&resolved.point.label));
    text.chars().filter(|c| !c.is_control()).take(120).collect()
}

fn copy_failed_text(reason: &str) -> String {
    MSG_BRANCH_COPY_FAILED.replace("<git reason, cleaned>", reason)
}

/// The record or the brief file could not be saved: nothing was copied. The reason (it holds a path of
/// the private store) goes to the log, not to the window.
fn save_failed(reason: &str) -> OperationError {
    tracing::warn!(reason, "a branch could not be saved");
    OperationError::new(
        ERR_BRANCH_COPY_FAILED,
        copy_failed_text("Puppet Master couldn't save its notes"),
        true,
    )
}
