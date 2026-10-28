//! Keep going: the live hand-over (SPEC n32 section 3.5).
//!
//! The order is the point, because one folder must never have two writers:
//!
//! 1. every guard and the brief, nothing touched yet (`prepare`);
//! 2. the old run ends first, as cancelled `HANDED_OFF` (so it cannot race into `AGENT_EXITED`);
//! 3. the old agent is stopped by its own pid and confirmed gone (`pty::agent_stop`); if it is not
//!    gone within `STOP_WAIT_MS`, nothing new starts;
//! 4. only then the new agent starts in the same folder (or the same separate copy) with the brief.
//!
//! None of it runs under the hand-off engine's lock: a card answer only queues a [`Request`]
//! (`spawn`), and the thread does the work. Every failure after the first step is recorded
//! (`State::Failed`) and raised as a card, and the pane's in-flight mark ends on every path.
//!
//! Puppet Master never types into a pane here, and never answers the new agent's trust, permission
//! or login screens: the new run is an ordinary visible run (`trust_workspace: false`, no shell
//! allowance), so those screens park it and raise the usual cards.

use super::brief::{self, BriefInputs};
use super::chain;
use super::live_io::{goal_of, FlightGuard, LiveIo, PaneFacts, StartRequest};
use super::model::{
    Back, BriefMeta, Candidate, Handover, KeepGoingOffer, Offer, Reason, Side, Source, State, Target, COME_BACK_GRACE_MS,
    COME_BACK_RETRY_MS, COME_BACK_TRIES, ERR_ALREADY, ERR_BUSY, ERR_HOP_CAP, ERR_NOT_AT_LIMIT, ERR_NOT_HERE, ERR_NO_TARGET,
    ERR_OLD_DID_NOT_STOP, ERR_SAME_VENDOR, ERR_START_FAILED, ERR_TARGET_LIMITED, HOP_CAP,
};
use super::offers;
use super::store::new_id;
use crate::audit::Who;
use crate::handoff::card;
use crate::handoff::reset::when_text;
use crate::operations::{self, OperationError, OperationSnapshot, OperationStatus};
use crate::plans_report::limit_log::Entry;
use crate::tui::ActivityKind;
use serde_json::json;

/// The run's timeout when the old one had none to copy.
const DEFAULT_TIMEOUT_MS: u64 = 30 * 60_000;
/// Re-check wait when the first agent's limit has no known end.
const UNKNOWN_RESET_RECHECK_MS: u64 = 30 * 60_000;
/// Characters of the task in the new run's name and in the come-back card.
const NAME_GOAL_CHARS: usize = 40;
const RECORD_GOAL_CHARS: usize = 1_200;

/// What a person's answer asks for. Built by `handoff::Engine::resolved` (card answers) and the
/// desktop command; run on a thread, never under the engine's lock.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// The limit card's `Continue on <agent>`.
    Continue { pane_id: String, target: Target, who: Who },
    /// The come-back card's `Move back`. `pane_id` is the NEW agent's pane.
    ComeBack { pane_id: String, who: Who },
    /// The come-back card's `Stay`.
    Stay { pane_id: String },
    /// The failure card's `Try again`. `pane_id` is the OLD agent's pane.
    Retry { pane_id: String, target: Target, who: Who },
}

/// Who a card answer came from (`Answer.answered_by`).
pub fn who_of(answered_by: &str) -> Who {
    if answered_by == "phone" {
        Who::Device(None)
    } else {
        Who::Desktop
    }
}

/// Run `request` on a thread named `keep-going` with the live app.
pub fn spawn(request: Request) {
    let _ = std::thread::Builder::new().name("keep-going".into()).spawn(move || run_in(super::live_app::app_io(), request));
}

/// Run `request` now. Failures become cards; nothing is returned.
pub fn run_in(io: &dyn LiveIo, request: Request) {
    match request {
        Request::Continue { pane_id, target, who } => {
            let _ = hand_over_in(io, Args::new(pane_id, target, Reason::UsageLimit, who));
        }
        Request::ComeBack { pane_id, who } => come_back(io, &pane_id, who),
        Request::Stay { pane_id } => stay(io, &pane_id),
        Request::Retry { pane_id, target, who } => retry(io, &pane_id, target, who),
    }
}

// ---- the offer ------------------------------------------------------------------------------------

/// What the limit card offers for `pane_id`. `limited` is the engine's own note lookup: this runs
/// inside the engine's lock, so it must not call back into it. Cheap and read-only.
pub fn offer_for_in(io: &dyn LiveIo, pane_id: &str, limited: &dyn Fn(&str, Option<&str>) -> Option<Option<u64>>) -> KeepGoingOffer {
    let Some(facts) = io.pane(pane_id) else { return KeepGoingOffer::NotHere };
    if facts.companion || facts.mission {
        return KeepGoingOffer::NotHere;
    }
    let records = io.records();
    let ranked = rank_for(io, &facts, &records, limited);
    match offers::first_ok(&ranked.offers) {
        Some(offer) => KeepGoingOffer::Offer { target: offer.target.clone(), label: offer.label.clone() },
        None => KeepGoingOffer::None {
            why: match ranked.offers.first() {
                Some(first) => format!("{}: {}", first.label, first.why_not.clone().unwrap_or_default()),
                None => "no other agent is set up for this folder".to_string(),
            },
        },
    }
}

/// The picker rows for `pane_id` (`keep_going_targets`).
pub fn targets_in(io: &dyn LiveIo, pane_id: &str) -> Result<Vec<Offer>, OperationError> {
    let facts = io.pane(pane_id).ok_or_else(|| err(ERR_NOT_HERE, "That pane isn't open any more."))?;
    if facts.companion || facts.mission {
        return Err(err(ERR_NOT_HERE, "Keep going isn't available in this pane."));
    }
    let records = io.records();
    let limited = |agent: &str, account: Option<&str>| io.engine_limit(agent, account);
    Ok(rank_for(io, &facts, &records, &limited).offers)
}

struct Ranked {
    offers: Vec<Offer>,
    hop_next: u8,
    chain: Option<String>,
}

fn rank_for(
    io: &dyn LiveIo,
    facts: &PaneFacts,
    records: &[Handover],
    limited: &dyn Fn(&str, Option<&str>) -> Option<Option<u64>>,
) -> Ranked {
    let now = io.now_ms();
    let settings = io.store().settings();
    let extra: Vec<Target> = settings.order.iter().filter(|target| target.account.is_some()).cloned().collect();
    let mut candidates: Vec<Candidate> = io.candidates(&folder_of(facts), &extra);
    for candidate in &mut candidates {
        let by_engine = limited(&candidate.target.agent_id, candidate.target.account.as_deref());
        let by_records = chain::limited_by_records(records, &candidate.target, now);
        candidate.limited_until = merge_limits(by_engine, by_records);
    }
    let source = Source { agent_id: facts.agent_id.clone(), account: facts.account.clone(), vendor: io.vendor_of(&facts.agent_id) };
    let (chain, hop_next) = chain::hop_for(records, &facts.pane_id);
    let clock = io.clock();
    let when = |ms: u64| when_text(i64::try_from(ms).unwrap_or(i64::MAX), clock);
    Ranked { offers: offers::rank(&source, &settings.order, &candidates, hop_next, now, &when), hop_next, chain }
}

/// Two views of "out of usage": the later known end wins; a known end beats an unknown one.
fn merge_limits(a: Option<Option<u64>>, b: Option<Option<u64>>) -> Option<Option<u64>> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    }
}

fn folder_of(facts: &PaneFacts) -> String {
    facts.worktree.as_ref().map(|copy| copy.cwd.clone()).unwrap_or_else(|| facts.cwd.clone())
}

// ---- the hand-over --------------------------------------------------------------------------------

/// What one hand-over is asked to do.
#[derive(Clone)]
pub struct Args {
    pub pane_id: String,
    pub target: Target,
    pub reason: Reason,
    pub who: Who,
    /// A failed start being tried again: its brief and chain are reused, and the old agent (already
    /// stopped) is not stopped again.
    pub retry_of: Option<Handover>,
    /// The record a come-back returns from; it is finished once this hand-over is under way.
    pub previous: Option<String>,
}

impl Args {
    pub fn new(pane_id: String, target: Target, reason: Reason, who: Who) -> Args {
        Args { pane_id, target, reason, who, retry_of: None, previous: None }
    }
}

/// A hand-over that was refused or failed. `card_for` is the old pane when a card should say so.
pub struct Refusal {
    pub error: OperationError,
    card_for: Option<CardFor>,
}

struct CardFor {
    pane_id: String,
    from_label: String,
    to_label: String,
    retry: Option<Target>,
}

impl Refusal {
    fn quiet(error: OperationError) -> Refusal {
        Refusal { error, card_for: None }
    }
}

/// A hand-over that passed every guard: the record is saved as `Stopping` and nothing else has
/// happened yet.
pub struct Prepared {
    pub record: Handover,
    facts: PaneFacts,
    run: Option<OperationSnapshot>,
    target: Target,
    to_label: String,
    who: Who,
    skip_stop: bool,
    _guard: FlightGuard,
}

/// The whole hand-over, now: guards, brief, then [`execute`]. A refusal raises a card (except for a
/// repeat tap), and the error comes back too.
pub fn hand_over_in(io: &dyn LiveIo, args: Args) -> Result<Handover, OperationError> {
    match prepare(io, args) {
        Ok(prepared) => execute(io, prepared),
        Err(refusal) => Err(announce(io, refusal)),
    }
}

/// The desktop command's start: guards and brief now (so the person sees why it was refused), the
/// rest on a thread. Returns the `Stopping` record.
pub fn begin(pane_id: &str, target: Target, who: Who) -> Result<Handover, OperationError> {
    let io = super::live_app::app_io();
    let prepared = prepare(io, Args::new(pane_id.to_string(), target, Reason::UsageLimit, who)).map_err(|refusal| refusal.error)?;
    let record = prepared.record.clone();
    let id = record.id.clone();
    let spawned = std::thread::Builder::new().name("keep-going".into()).spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute(io, prepared)));
        if outcome.is_err() {
            fail_by_id(io, &id, ERR_START_FAILED, "Keep going stopped unexpectedly. Check the pane before trying again.");
        }
    });
    if spawned.is_err() {
        fail_by_id(io, &record.id, ERR_START_FAILED, "Keep going could not start its worker.");
        return Err(err(ERR_START_FAILED, "Keep going could not start its worker."));
    }
    Ok(record)
}

fn err(code: &str, message: impl Into<String>) -> OperationError {
    OperationError::new(code, message, false)
}

/// Raise the card for a refusal (a repeat tap and a missing pane stay quiet) and give back the error.
fn announce(io: &dyn LiveIo, refusal: Refusal) -> OperationError {
    if let Some(note) = &refusal.card_for {
        let card =
            card::keep_going_failed_card(&note.from_label, &note.to_label, &note.pane_id, &refusal.error.message, note.retry.as_ref());
        card::raise(io.questions(), card, io.now_ms());
    }
    refusal.error
}

/// Steps 1 to 3 of the flow: guards, inputs, brief, and the `Stopping` record.
pub fn prepare(io: &dyn LiveIo, args: Args) -> Result<Prepared, Refusal> {
    let facts = io.pane(&args.pane_id).ok_or_else(|| Refusal::quiet(err(ERR_NOT_HERE, "That pane isn't open any more.")))?;
    if facts.companion || facts.mission {
        return Err(Refusal::quiet(err(ERR_NOT_HERE, "Keep going isn't available in this pane.")));
    }
    let from_label = facts.label();
    let refuse = |code: &str, message: String, to_label: &str| Refusal {
        error: err(code, message),
        card_for: Some(CardFor {
            pane_id: facts.pane_id.clone(),
            from_label: from_label.clone(),
            to_label: to_label.to_string(),
            retry: None,
        }),
    };
    let records = io.records();
    if chain::already_from(&records, &facts.pane_id).is_some() {
        return Err(Refusal::quiet(err(ERR_ALREADY, format!("{from_label} in this pane was already moved. Nothing was changed."))));
    }
    let guard =
        io.in_flight().enter(&facts.pane_id).ok_or_else(|| Refusal::quiet(err(ERR_BUSY, "A move is already under way for this pane.")))?;
    let skip_stop = args.retry_of.as_ref().is_some_and(|old| matches!(&old.state, State::Failed { code, .. } if code == ERR_START_FAILED));
    if args.reason == Reason::UsageLimit && !skip_stop && !io.at_limit(&facts.pane_id) {
        return Err(refuse(ERR_NOT_AT_LIMIT, format!("{from_label} is working again. Nothing was moved."), &args.target.key()));
    }

    let limited = |agent: &str, account: Option<&str>| io.engine_limit(agent, account);
    let ranked = rank_for(io, &facts, &records, &limited);
    let Some(offer) = ranked.offers.iter().find(|offer| offer.target == args.target) else {
        return Err(refuse(
            ERR_NO_TARGET,
            format!("{} can't take over this task. Nothing was moved.", args.target.key()),
            &args.target.key(),
        ));
    };
    if !offer.ok {
        let why = offer.why_not.clone().unwrap_or_default();
        let (code, message) = if ranked.hop_next > HOP_CAP {
            (ERR_HOP_CAP, format!("This task has moved {HOP_CAP} times already. Nothing was moved."))
        } else if why.starts_with("Same company") {
            (ERR_SAME_VENDOR, format!("{} is the same company as {from_label}, so nothing was moved.", offer.label))
        } else if why.starts_with("Out of usage") {
            (ERR_TARGET_LIMITED, format!("{} is out of usage too. Nothing was moved.", offer.label))
        } else {
            (ERR_NO_TARGET, format!("{} can't take over now: {why}. Nothing was moved.", offer.label))
        };
        return Err(refuse(code, message, &offer.label));
    }
    let to_label = offer.label.clone();

    // The run of the old pane (a retry reads the one it already ended).
    let run = match &args.retry_of {
        Some(old) => old
            .from
            .operation_id
            .as_deref()
            .and_then(|id| facts.run_projects().iter().find_map(|project| operations::get_operation(project, id).ok())),
        None => active_run(&facts),
    };
    let built = build_brief(io, &args, &facts, run.as_ref(), &records, &from_label, &to_label);

    let id = new_id();
    let (chain_id, hop) = match &args.retry_of {
        Some(old) => (old.chain_id.clone(), old.hop),
        None => {
            let (chain, hop) = (ranked.chain.clone(), ranked.hop_next);
            (chain.unwrap_or_else(|| id.clone()), hop)
        }
    };
    let file = io
        .store()
        .write_brief(&id, &built.text)
        .map_err(|_| Refusal::quiet(err(ERR_START_FAILED, "Keep going could not save its brief, so nothing was moved.")))?;
    let record = Handover {
        id: id.clone(),
        chain_id,
        hop,
        at_ms: io.now_ms(),
        reason: args.reason,
        from: Side {
            pane_id: facts.pane_id.clone(),
            agent_id: facts.agent_id.clone(),
            account: facts.account.clone(),
            label: from_label.clone(),
            handle: run.as_ref().map(handle_of),
            operation_id: run.as_ref().map(|run| run.operation_id.clone()),
        },
        to: Side {
            pane_id: String::new(),
            agent_id: args.target.agent_id.clone(),
            account: args.target.account.clone(),
            label: to_label.clone(),
            handle: None,
            operation_id: None,
        },
        project: facts.worktree.as_ref().map(|copy| copy.repo.clone()).unwrap_or_else(|| facts.project.clone()),
        folder: folder_of(&facts),
        worktree_handle: facts.worktree.as_ref().map(|copy| copy.handle.clone()),
        from_reset_at_ms: built.reset_ms,
        goal: built.goal.clone(),
        back: Back::Off,
        brief: BriefMeta { file, sha8: built.sha8.clone(), bytes: built.bytes, trimmed: built.trimmed.clone(), turns: built.turns },
        state: State::Stopping,
    };
    save(io, &record, &[&facts.pane_id]).map_err(|message| Refusal::quiet(err(ERR_START_FAILED, message)))?;
    if let Some(previous) = args.previous.as_deref() {
        set_back(io, previous, Back::Done);
    }
    Ok(Prepared { record, facts, run, target: args.target, to_label, who: args.who, skip_stop, _guard: guard })
}

/// The brief and the facts that go in the record. A retry reuses the brief that was already written
/// (the old agent is stopped, so nothing it did has changed since); anything else reads the old pane.
struct Built {
    text: String,
    sha8: String,
    bytes: usize,
    trimmed: Vec<String>,
    turns: u32,
    goal: String,
    reset_ms: Option<u64>,
}

fn build_brief(
    io: &dyn LiveIo,
    args: &Args,
    facts: &PaneFacts,
    run: Option<&OperationSnapshot>,
    records: &[Handover],
    from_label: &str,
    to_label: &str,
) -> Built {
    if let Some(old) = &args.retry_of {
        if let Ok(text) = io.store().read_brief(&old.id) {
            return Built {
                text,
                sha8: old.brief.sha8.clone(),
                bytes: old.brief.bytes,
                trimmed: old.brief.trimmed.clone(),
                turns: old.brief.turns,
                goal: old.goal.clone(),
                reset_ms: old.from_reset_at_ms,
            };
        }
    }
    let gathered = io.gather(facts, run);
    // The task: the chain's own, else what the person first asked, else the run's.
    let raw_goal = goal_of(records, &facts.pane_id).or(gathered.goal.clone()).or_else(|| run.map(|run| run.task.clone()));
    let clean = |text: &str| io.clean(text);
    let reset_ms = match args.reason {
        Reason::UsageLimit => io.engine_limit(&facts.agent_id, facts.account.as_deref()).flatten(),
        Reason::CameBack => None,
    };
    let inputs = BriefInputs {
        from_label: from_label.to_string(),
        to_label: to_label.to_string(),
        reason: args.reason,
        reset_text: reset_ms.map(|ms| when_text(i64::try_from(ms).unwrap_or(i64::MAX), io.clock())),
        goal: raw_goal.clone(),
        turns: gathered.turns,
        diff: gathered.diff,
        open_plan: gathered.open_plan,
        last_message: gathered.last_message,
        open_question: gathered.open_question,
        folder_note: match &facts.worktree {
            Some(copy) => format!("its separate copy (branch {})", copy.branch),
            None => "the project folder".to_string(),
        },
    };
    let built = brief::build(&inputs, &clean);
    Built {
        sha8: built.sha8.clone(),
        bytes: built.bytes,
        trimmed: built.trimmed.iter().map(|step| step.to_string()).collect(),
        turns: inputs.turns.iter().map(|turn| turn.n).max().unwrap_or(0),
        goal: cut_chars(&clean(raw_goal.as_deref().unwrap_or("")), RECORD_GOAL_CHARS),
        reset_ms,
        text: built.text,
    }
}

/// Steps 4 to 8: end the old run, stop the old agent, start the new one. Every failure is recorded
/// and raised as a card.
pub fn execute(io: &dyn LiveIo, prepared: Prepared) -> Result<Handover, OperationError> {
    let Prepared { mut record, facts, run, target, to_label, who, skip_stop, _guard } = prepared;
    let from_label = record.from.label.clone();

    // 4. The old run ends first, so it cannot race into AGENT_EXITED when its agent stops.
    if let Some(run) = run.as_ref().filter(|run| !run.status.terminal()) {
        match operations::mark_operation_handed_off(&run.project_path, &run.operation_id, json!({ "keep_going": record.id })) {
            Ok(ended) => io.publish(&ended),
            Err(_) => {
                return Err(fail(
                    io,
                    &mut record,
                    &facts,
                    &to_label,
                    ERR_OLD_DID_NOT_STOP,
                    format!("{from_label}'s task could not be ended, so {to_label} wasn't started. Nothing else was changed."),
                    None,
                ));
            }
        }
    }

    // 5. The old agent is stopped by its own pid and must be gone before anything new starts.
    if !skip_stop {
        io.mark(&facts.pane_id, &format!("keep-going:stop {}", record.id));
        if io.stop_agent(&facts.pane_id).is_err() {
            return Err(fail(
                io,
                &mut record,
                &facts,
                &to_label,
                ERR_OLD_DID_NOT_STOP,
                format!("{from_label} didn't stop, so {to_label} wasn't started. Close its terminal, then try again."),
                None,
            ));
        }
    }

    // 6. The waiting time in "Your plans" ends here.
    if record.reason == Reason::UsageLimit {
        io.log_limit(Entry::End { at_ms: io.now_ms(), pane: facts.pane_id.clone() });
    }
    io.mark(&facts.pane_id, &format!("keep-going:from {} -> {}", record.id, target.key()));
    record.state = State::Starting;
    let _ = save(io, &record, &[&facts.pane_id]);

    // 7. The new agent: an ordinary visible run. Its trust, permission and login screens park it.
    let request = start_request(io, &record, &facts, run.as_ref(), &target);
    let started = match io.start(&request) {
        Ok(started) => started,
        Err(error) => {
            return Err(fail(
                io,
                &mut record,
                &facts,
                &to_label,
                ERR_START_FAILED,
                format!("{to_label} couldn't start ({}). {from_label} was stopped, so nothing is running here now.", error.code),
                Some(&target),
            ));
        }
    };

    // 8. Link, remember, tell.
    let new_pane = started.pane_id.clone().or_else(|| pane_of(&started.operation_id, &facts)).unwrap_or_default();
    if !new_pane.is_empty() {
        io.set_parent(&new_pane, &facts.pane_id);
    }
    if let Some(run) = &run {
        let _ = operations::set_handed_off_to(
            &run.project_path,
            &run.operation_id,
            json!({ "handle": started.handle, "agent_type": target.agent_id }),
        );
    }
    record.to.pane_id = new_pane.clone();
    record.to.handle = Some(started.handle.clone());
    record.to.operation_id = Some(started.operation_id.clone());
    record.state = State::Running;
    record.back = match (record.reason, record.from_reset_at_ms, io.store().settings().ask_back) {
        (Reason::UsageLimit, Some(reset), true) => Back::AtReset { due_ms: reset.saturating_add(COME_BACK_GRACE_MS), tries: 0 },
        _ => Back::Off,
    };
    let _ = save(io, &record, &[&facts.pane_id, &new_pane]);
    let summary = match record.reason {
        Reason::UsageLimit => format!("{from_label} handed to {to_label} at a usage limit"),
        Reason::CameBack => format!("{from_label} handed back to {to_label}"),
    };
    io.audit(
        who,
        &summary,
        json!({
            "record": record.id,
            "chain": record.chain_id,
            "hop": record.hop,
            "reason": record.reason,
            "from_pane": record.from.pane_id,
            "to_pane": new_pane,
            "from_agent": record.from.agent_id,
            "to_agent": record.to.agent_id,
            "brief_bytes": record.brief.bytes,
        }),
    );
    Ok(record)
}

/// Record a failure, raise its card on the old pane, and give the error back.
fn fail(
    io: &dyn LiveIo,
    record: &mut Handover,
    facts: &PaneFacts,
    to_label: &str,
    code: &str,
    message: String,
    retry: Option<&Target>,
) -> OperationError {
    record.state = State::Failed { code: code.to_string(), message: message.clone() };
    let _ = save(io, record, &[&facts.pane_id]);
    let card = card::keep_going_failed_card(&record.from.label, to_label, &facts.pane_id, &message, retry);
    card::raise(io.questions(), card, io.now_ms());
    OperationError::new(code, message, retry.is_some())
}

/// A failure that only knows the record's id (a worker thread that panicked or never started).
fn fail_by_id(io: &dyn LiveIo, id: &str, code: &str, message: &str) {
    let Some(mut record) = io.store().by_id(id) else { return };
    if matches!(record.state, State::Running) {
        return;
    }
    record.state = State::Failed { code: code.to_string(), message: message.to_string() };
    let pane = record.from.pane_id.clone();
    let _ = save(io, &record, &[&pane]);
    let card = card::keep_going_failed_card(&record.from.label, &record.to.label, &pane, message, None);
    card::raise(io.questions(), card, io.now_ms());
}

fn start_request(io: &dyn LiveIo, record: &Handover, facts: &PaneFacts, run: Option<&OperationSnapshot>, target: &Target) -> StartRequest {
    let task = io.store().read_brief(&record.id).unwrap_or_default();
    let (budget_turns, budget_minutes) = match run {
        Some(run) => io.budget_left(&run.project_path, &handle_of(run)),
        None => (None, None),
    };
    // A name is a worker's identity: the next run with the same name would adopt this one. The
    // record id keeps every hand-over its own worker.
    let name = match one_line(&record.goal, NAME_GOAL_CHARS) {
        goal if goal.is_empty() => format!("Continue · {}", record.id),
        goal => format!("Continue: {goal} · {}", record.id),
    };
    StartRequest {
        project_path: record.project.clone(),
        task,
        agent_type: target.agent_id.clone(),
        account: target.account.clone(),
        headless: false,
        background: true,
        keep_pane: true,
        wait_ms: 0,
        timeout_ms: run.map(|run| run.timeout_ms).filter(|ms| *ms > 0).unwrap_or(DEFAULT_TIMEOUT_MS),
        idempotency_key: format!("keep-going:{}", record.id),
        name,
        trust_workspace: false,
        allow_shell: false,
        allowed_commands: None,
        read_only: false,
        budget_turns,
        budget_minutes,
        join_worktree: facts.worktree.clone(),
    }
}

fn pane_of(operation_id: &str, facts: &PaneFacts) -> Option<String> {
    facts.run_projects().iter().find_map(|project| operations::get_operation(project, operation_id).ok()).and_then(|op| op.pane_id)
}

fn handle_of(run: &OperationSnapshot) -> String {
    if run.agent_run_id.is_empty() {
        run.operation_id.clone()
    } else {
        run.agent_run_id.clone()
    }
}

fn active_run(facts: &PaneFacts) -> Option<OperationSnapshot> {
    facts.run_projects().iter().find_map(|project| operations::active_operation_for_pane(project, &facts.pane_id).ok().flatten())
}

// ---- records --------------------------------------------------------------------------------------

fn save(io: &dyn LiveIo, record: &Handover, panes: &[&str]) -> Result<(), String> {
    io.store().put(record)?;
    let mut changed: Vec<&str> = panes.iter().copied().filter(|pane| !pane.is_empty()).collect();
    if !record.to.pane_id.is_empty() && !changed.contains(&record.to.pane_id.as_str()) {
        changed.push(record.to.pane_id.as_str());
    }
    io.changed(&changed);
    Ok(())
}

fn set_back(io: &dyn LiveIo, id: &str, back: Back) {
    let Some(mut record) = io.store().by_id(id) else { return };
    let panes = [record.from.pane_id.clone(), record.to.pane_id.clone()];
    record.back = back;
    let _ = save(io, &record, &[&panes[0], &panes[1]]);
}

// ---- coming back ----------------------------------------------------------------------------------

/// `Request::ComeBack`: the same hand-over, from the new agent's pane to the first agent.
fn come_back(io: &dyn LiveIo, pane_id: &str, who: Who) {
    let records = io.records();
    let Some(record) = records.iter().find(|r| r.to.pane_id == pane_id && matches!(r.state, State::Running)).cloned() else {
        return;
    };
    if !matches!(record.back, Back::Asked) {
        return;
    }
    if pane_is_busy(io, pane_id) {
        rearm(io, &record, io.now_ms());
        return;
    }
    let target = Target { agent_id: record.from.agent_id.clone(), account: record.from.account.clone() };
    let mut args = Args::new(pane_id.to_string(), target, Reason::CameBack, who);
    args.previous = Some(record.id.clone());
    let _ = hand_over_in(io, args);
}

/// `Request::Stay`.
fn stay(io: &dyn LiveIo, pane_id: &str) {
    if let Some(record) = io.records().into_iter().find(|r| r.to.pane_id == pane_id && matches!(r.back, Back::Asked)) {
        set_back(io, &record.id, Back::Done);
    }
}

/// `Request::Retry`: the failed start again, with its brief.
fn retry(io: &dyn LiveIo, pane_id: &str, target: Target, who: Who) {
    let records = io.records();
    let failed = records
        .iter()
        .find(|r| r.from.pane_id == pane_id)
        .filter(|r| matches!(&r.state, State::Failed { code, .. } if code == ERR_START_FAILED))
        .cloned();
    let Some(failed) = failed else { return };
    let mut args = Args::new(pane_id.to_string(), target, failed.reason, who);
    args.retry_of = Some(failed);
    let _ = hand_over_in(io, args);
}

/// The new agent is mid-turn: the hand-back waits.
fn pane_is_busy(io: &dyn LiveIo, pane_id: &str) -> bool {
    let Some(facts) = io.pane(pane_id) else { return false };
    if facts.status == crate::pty::status::PaneStatus::Running.as_str() {
        return true;
    }
    active_run(&facts).is_some_and(|run| run.status == OperationStatus::Running)
}

/// The come-back check again later, or never after `COME_BACK_TRIES`.
fn rearm(io: &dyn LiveIo, record: &Handover, now: u64) {
    let tries = match record.back {
        Back::AtReset { tries, .. } => tries,
        _ => 0,
    };
    let next = tries.saturating_add(1);
    if next >= COME_BACK_TRIES {
        set_back(io, &record.id, Back::Done);
    } else {
        set_back(io, &record.id, Back::AtReset { due_ms: now.saturating_add(COME_BACK_RETRY_MS), tries: next });
    }
}

/// Once a second, after the engine's lock is released: ask to move back what is due.
pub fn tick_in(io: &dyn LiveIo, now_ms: u64) {
    let records = io.records();
    for id in chain::come_backs_due(&records, now_ms) {
        let Some(record) = records.iter().find(|r| r.id == id) else { continue };
        let Back::AtReset { tries, .. } = record.back else { continue };
        if !matches!(record.state, State::Running) || io.pane(&record.to.pane_id).is_none() || record.hop >= HOP_CAP {
            set_back(io, &id, Back::Done);
            continue;
        }
        let first = Target { agent_id: record.from.agent_id.clone(), account: record.from.account.clone() };
        let limit =
            merge_limits(io.engine_limit(&first.agent_id, first.account.as_deref()), chain::limited_by_records(&records, &first, now_ms));
        if let Some(reset) = limit {
            let due = reset
                .filter(|ms| *ms > now_ms)
                .map(|ms| ms.saturating_add(COME_BACK_GRACE_MS))
                .unwrap_or(now_ms.saturating_add(UNKNOWN_RESET_RECHECK_MS));
            set_back(io, &id, Back::AtReset { due_ms: due, tries });
            continue;
        }
        if pane_is_busy(io, &record.to.pane_id) {
            rearm(io, record, now_ms);
            continue;
        }
        let goal = one_line(&record.goal, NAME_GOAL_CHARS);
        let card = card::come_back_card(&record.to.label, &record.from.label, &record.to.pane_id, &goal);
        card::raise(io.questions(), card, now_ms);
        set_back(io, &id, Back::Asked);
    }
}

// ---- what the panes show --------------------------------------------------------------------------

/// The old pane's one-line status while a hand-over is under way or done.
pub fn status_line_from(records: &[Handover], pane_id: &str) -> Option<(ActivityKind, String)> {
    let record = records.iter().find(|r| r.from.pane_id == pane_id)?;
    match &record.state {
        State::Stopping | State::Starting => Some((ActivityKind::Working, format!("Moving to {}", record.to.label))),
        State::Running => Some((ActivityKind::Idle, format!("Handed to {}", record.to.label))),
        State::Failed { code, .. } if code == ERR_START_FAILED => {
            Some((ActivityKind::Error, format!("{} couldn't start", record.to.label)))
        }
        State::Failed { .. } => Some((ActivityKind::Error, format!("{} didn't stop", record.from.label))),
    }
}

/// The pane's status line for `pane_activity` (cached records, reloaded when a record changes).
pub fn status_line(pane_id: &str) -> Option<(ActivityKind, String)> {
    status_line_from(&super::live_app::app_io().records(), pane_id)
}

/// Both sides of a pane's hand-overs: the one that made it (`from`) and the one it made (`to`).
pub fn lineage_in(io: &dyn LiveIo, pane_id: &str) -> super::view::Lineage {
    let records = io.records();
    super::view::lineage_of(&records, pane_id)
}

// ---- the same, against the live app ---------------------------------------------------------------
//
// What `handoff::LiveEnv` and the desktop commands call. Each one is the `_in` function above with
// `live_app::app_io()`.

/// What the limit card offers (see [`offer_for_in`]). `limited` is the engine's note lookup.
pub fn offer_for(pane_id: &str, limited: &dyn Fn(&str, Option<&str>) -> Option<Option<u64>>) -> KeepGoingOffer {
    offer_for_in(super::live_app::app_io(), pane_id, limited)
}

/// The picker rows (`keep_going_targets`).
pub fn targets(pane_id: &str) -> Result<Vec<Offer>, OperationError> {
    targets_in(super::live_app::app_io(), pane_id)
}

/// The whole hand-over now, on this thread (see [`hand_over_in`]).
pub fn hand_over(args: Args) -> Result<Handover, OperationError> {
    hand_over_in(super::live_app::app_io(), args)
}

/// Both sides of a pane's hand-overs (`keep_going_lineage`).
pub fn lineage(pane_id: &str) -> super::view::Lineage {
    lineage_in(super::live_app::app_io(), pane_id)
}

/// Once a second from `handoff::tick`, outside the engine's lock. Cheap when nothing is due: one
/// cached read and a scan.
pub fn tick(now_ms: u64) {
    tick_in(super::live_app::app_io(), now_ms);
}

/// The records as they are now (cached).
pub fn records() -> Vec<Handover> {
    super::live_app::app_io().records()
}

/// The stored settings.
pub fn settings() -> super::model::Settings {
    super::live_app::app_io().store().settings()
}

/// The brief of record `id` (a bad id reads nothing).
pub fn brief_text(id: &str) -> Result<String, String> {
    super::live_app::app_io().store().read_brief(id).map_err(|error| error.to_string())
}

/// Save the settings and return what was kept.
pub fn save_settings(settings: &super::model::Settings) -> Result<super::model::Settings, String> {
    super::live_app::app_io().store().save_settings(settings)
}

// ---- small text helpers ---------------------------------------------------------------------------

fn cut_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// The first line, control characters removed, cut to `max` characters.
fn one_line(text: &str, max: usize) -> String {
    let first = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    cut_chars(&first.chars().filter(|c| !c.is_control()).collect::<String>(), max)
}
