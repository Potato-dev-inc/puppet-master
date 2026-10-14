//! Turn summaries and the away digest: assembly, ring file, presence, digest and the HTTP route.
//! No app, no windows: a fake environment stands in for the registry and the checkpoints.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::json;

use super::digest::{self, LastSeen, NeedsYou};
use super::http::{respond, Source};
use super::model::{assemble, Approval, EndState, Facts, FileStats, ReceiptFacts, TurnSummary};
use super::presence::{Presence, Return, DEFAULT_AWAY_MS};
use super::store::{ring_path, Store, KEEP_PER_PANE, MAX_PANES_IN_FILE};
use super::tracker::{Env, PaneInfo, Tracker};
use crate::checkpoints::FileChange;

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pm-turns-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }
    fn p(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- fake environment ----

struct FakeEnv {
    now: AtomicU64,
    cwd: PathBuf,
    has_turns: AtomicBool,
    pane_exists: AtomicBool,
    files: Mutex<Option<FileStats>>,
    prompt: Mutex<Option<String>>,
    receipt: Mutex<Option<ReceiptFacts>>,
}

impl FakeEnv {
    fn new(cwd: &Path) -> Arc<Self> {
        Arc::new(Self {
            now: AtomicU64::new(1_000),
            cwd: cwd.to_path_buf(),
            has_turns: AtomicBool::new(true),
            pane_exists: AtomicBool::new(true),
            files: Mutex::new(None),
            prompt: Mutex::new(None),
            receipt: Mutex::new(None),
        })
    }
    fn at(&self, ms: u64) {
        self.now.store(ms, Ordering::SeqCst);
    }
}

impl Env for FakeEnv {
    fn now_ms(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
    fn pane(&self, _pane_id: &str) -> Option<PaneInfo> {
        self.pane_exists.load(Ordering::SeqCst).then(|| PaneInfo {
            cwd: self.cwd.to_string_lossy().to_string(),
            label: "Claude Code".into(),
            has_turns: self.has_turns.load(Ordering::SeqCst),
        })
    }
    fn files(&self, _pane_id: &str, _started_at_ms: u64) -> Option<FileStats> {
        self.files.lock().clone()
    }
    fn end_prompt(&self, _pane_id: &str) -> Option<String> {
        self.prompt.lock().clone()
    }
    fn receipt(&self, _pane_id: &str) -> Option<ReceiptFacts> {
        self.receipt.lock().clone()
    }
}

/// The diff between "before turn n" and "after turn n": 4 files, +86 -12.
fn synthetic_diff() -> Vec<FileChange> {
    let change = |path: &str, added: u32, deleted: u32| FileChange {
        path: path.into(),
        status: "M".into(),
        added,
        deleted,
        binary: false,
    };
    vec![change("src/a.ts", 40, 5), change("src/b.ts", 30, 4), change("README.md", 10, 3), change("package.json", 6, 0)]
}

fn approval(id: &str, kind: &str, decision: &str) -> Approval {
    Approval { at_ms: 5, prompt_id: id.into(), prompt: "allow `npm test`?".into(), kind: kind.into(), decision: decision.into() }
}

fn finish_idle(tracker: &Tracker, env: &FakeEnv, start: u64, end: u64) -> TurnSummary {
    env.at(start);
    assert!(tracker.on_status("p1", "running").is_none());
    env.at(end);
    tracker.finish(tracker.on_status("p1", "idle").expect("turn came to rest")).expect("summary")
}

// ---- summary assembly ----

#[test]
fn assembly_from_a_synthetic_checkpoint_diff_approvals_and_end_state() {
    let stats = FileStats::from_changes(&synthetic_diff());
    assert_eq!((stats.count, stats.added, stats.deleted), (4, 86, 12));
    let summary = assemble(Facts {
        pane_id: "p1".into(),
        agent: "Claude Code".into(),
        turn: 2,
        started_at_ms: 10_000,
        ended_at_ms: 190_000,
        files: Some(stats),
        approvals: vec![approval("a1", "command", "allow")],
        end_status: "waiting_input".into(),
        end_prompt: Some("allow `npm test`?".into()),
        receipt: None,
    });
    assert_eq!(summary.duration_ms, 180_000);
    assert_eq!(summary.end, EndState { kind: "waiting_input".into(), prompt: Some("allow `npm test`?".into()) });
    assert_eq!(
        summary.sentence,
        "Claude Code edited 4 files (+86 \u{2212}12), you approved 1 command, now waiting: allow `npm test`?"
    );
    assert_eq!(summary.files.as_ref().unwrap().paths.len(), 4);
}

#[test]
fn summary_round_trips_through_json_in_camel_case() {
    let summary = assemble(Facts {
        pane_id: "p1".into(),
        agent: "Codex".into(),
        turn: 1,
        started_at_ms: 0,
        ended_at_ms: 5_000,
        files: None,
        approvals: vec![],
        end_status: "idle".into(),
        end_prompt: None,
        receipt: Some(ReceiptFacts { checks: "2/2 passed".into(), outcome: "passed_checks".into() }),
    });
    let value = serde_json::to_value(&summary).unwrap();
    assert_eq!(value["paneId"], "p1");
    assert_eq!(value["endedAtMs"], 5_000);
    assert_eq!(value["files"], json!(null));
    assert_eq!(value["receipt"]["outcome"], "passed_checks");
    let back: TurnSummary = serde_json::from_value(value).unwrap();
    assert_eq!(back, summary);
}

// ---- tracker ----

#[test]
fn a_turn_across_an_approval_is_one_turn_and_the_last_summary_wins() {
    let tmp = Tmp::new("lifecycle");
    let env = FakeEnv::new(tmp.p());
    let tracker = Tracker::new(env.clone());
    *env.files.lock() = Some(FileStats::from_changes(&synthetic_diff()));
    *env.prompt.lock() = Some("allow `npm test`?".into());

    env.at(1_000);
    assert!(tracker.on_status("p1", "running").is_none());
    env.at(30_000);
    let ended = tracker.on_status("p1", "waiting_input").expect("rest point");
    assert!(ended.from_running);
    let first = tracker.finish(ended).unwrap();
    assert_eq!(first.sentence, "Claude Code edited 4 files (+86 \u{2212}12), now waiting: allow `npm test`?");

    // The user approves; the agent resumes the same turn and stops on another prompt.
    tracker.record_approval("p1", approval("a1", "command", "allow"));
    env.at(40_000);
    assert!(tracker.on_status("p1", "running").is_none());
    env.at(90_000);
    let second = tracker.finish(tracker.on_status("p1", "waiting_input").unwrap()).unwrap();
    assert_eq!(second.turn, first.turn, "approval resumes the same turn");
    assert_eq!(second.started_at_ms, 1_000);
    assert_eq!(
        second.sentence,
        "Claude Code edited 4 files (+86 \u{2212}12), you approved 1 command, now waiting: allow `npm test`?"
    );

    // It finishes: the idle summary replaces the earlier ones of the same turn.
    env.at(100_000);
    tracker.on_status("p1", "running");
    env.at(120_000);
    let ended = tracker.on_status("p1", "idle").unwrap();
    let last = tracker.finish(ended).unwrap();
    assert_eq!(last.sentence, "Claude Code edited 4 files (+86 \u{2212}12), you approved 1 command, now idle.");
    let all = tracker.summaries("p1");
    assert_eq!(all.len(), 1, "one entry per turn");
    assert_eq!(all[0], last);
}

#[test]
fn the_next_turn_after_idle_gets_the_next_number_and_no_old_approvals() {
    let tmp = Tmp::new("turns");
    let env = FakeEnv::new(tmp.p());
    let tracker = Tracker::new(env.clone());
    tracker.on_status("p1", "idle");
    env.at(1_000);
    tracker.on_status("p1", "running");
    tracker.record_approval("p1", approval("a1", "command", "allow"));
    env.at(2_000);
    let one = tracker.finish(tracker.on_status("p1", "idle").unwrap()).unwrap();
    assert_eq!((one.turn, one.approvals.len()), (1, 1));

    env.at(10_000);
    tracker.on_status("p1", "running");
    env.at(12_000);
    let two = tracker.finish(tracker.on_status("p1", "idle").unwrap()).unwrap();
    assert_eq!((two.turn, two.approvals.len()), (2, 0));
    assert_eq!(tracker.summaries("p1").iter().map(|s| s.turn).collect::<Vec<_>>(), vec![1, 2]);
}

#[test]
fn plain_shells_have_no_turns() {
    let tmp = Tmp::new("shell");
    let env = FakeEnv::new(tmp.p());
    env.has_turns.store(false, Ordering::SeqCst);
    let tracker = Tracker::new(env.clone());
    assert!(tracker.on_status("p1", "running").is_none());
    assert!(tracker.on_status("p1", "idle").is_none(), "no turn was opened, so none ends");
    assert!(tracker.summaries("p1").is_empty());
}

#[test]
fn answers_are_counted_once_and_only_inside_a_turn() {
    let tmp = Tmp::new("answers");
    let env = FakeEnv::new(tmp.p());
    let tracker = Tracker::new(env.clone());
    // No open turn yet: ignored.
    tracker.record_approval("p1", approval("early", "command", "allow"));
    tracker.on_status("p1", "running");
    tracker.record_approval("p1", approval("a1", "command", "allow"));
    tracker.record_approval("p1", approval("a1", "command", "allow")); // a replayed answer
    tracker.record_approval("p1", approval("a2", "command", "deny"));
    let s = tracker.finish(tracker.on_status("p1", "idle").unwrap()).unwrap();
    assert_eq!(s.approvals.len(), 2);
    assert_eq!(s.sentence, "Claude Code worked for 0s, you approved 1 command, you declined 1 command, now idle.");
}

#[test]
fn end_states_and_receipt_facts_reach_the_sentence() {
    let tmp = Tmp::new("endstates");
    let env = FakeEnv::new(tmp.p());
    let tracker = Tracker::new(env.clone());
    *env.files.lock() = Some(FileStats { count: 0, added: 0, deleted: 0, paths: vec![] });
    *env.receipt.lock() = Some(ReceiptFacts { checks: "3/3 passed".into(), outcome: "passed_checks".into() });
    let ok = finish_idle(&tracker, &env, 1_000, 61_000);
    assert_eq!(ok.sentence, "Claude Code made no file changes, all checks passed (3/3), now idle.");
    assert_eq!(ok.duration_ms, 60_000);

    env.at(70_000);
    tracker.on_status("p1", "running");
    env.at(80_000);
    *env.receipt.lock() = Some(ReceiptFacts { checks: "1/3 passed".into(), outcome: "failed_checks".into() });
    let bad = tracker.finish(tracker.on_status("p1", "error").unwrap()).unwrap();
    assert_eq!(bad.sentence, "Claude Code made no file changes, checks failed (1/3 passed), ended with an error.");
    assert_eq!(bad.end.kind, "error");
}

#[test]
fn a_pane_that_is_gone_has_no_summary() {
    let tmp = Tmp::new("gone");
    let env = FakeEnv::new(tmp.p());
    let tracker = Tracker::new(env.clone());
    tracker.on_status("p1", "running");
    env.at(5_000);
    let ended = tracker.on_status("p1", "idle").unwrap();
    env.pane_exists.store(false, Ordering::SeqCst);
    assert!(tracker.finish(ended).is_none());
}

#[test]
fn summaries_survive_a_restart_through_the_ring_file() {
    let tmp = Tmp::new("restart");
    let env = FakeEnv::new(tmp.p());
    let first_run = Tracker::new(env.clone());
    let kept = finish_idle(&first_run, &env, 1_000, 9_000);
    assert!(ring_path(tmp.p()).is_file());

    // A new process: nothing in memory, the pane's project file is read on first use.
    let second_run = Tracker::new(env.clone());
    assert_eq!(second_run.summaries("p1"), vec![kept.clone()]);
    assert_eq!(second_run.since(0), vec![kept]);
}

// ---- the ring file ----

fn summary(pane: &str, turn: u32, ended: u64) -> TurnSummary {
    assemble(Facts {
        pane_id: pane.into(),
        agent: "Claude Code".into(),
        turn,
        started_at_ms: ended.saturating_sub(10),
        ended_at_ms: ended,
        files: None,
        approvals: vec![],
        end_status: "idle".into(),
        end_prompt: None,
        receipt: None,
    })
}

#[test]
fn ring_keeps_the_last_100_per_pane_in_memory_and_on_disk() {
    let tmp = Tmp::new("cap");
    let mut store = Store::default();
    for turn in 1..=(KEEP_PER_PANE as u32 + 5) {
        store.record(tmp.p(), summary("p1", turn, u64::from(turn) * 1000)).unwrap();
    }
    let in_memory = store.for_pane("p1");
    assert_eq!(in_memory.len(), KEEP_PER_PANE);
    assert_eq!(in_memory.first().unwrap().turn, 6, "the five oldest were dropped");
    assert_eq!(in_memory.last().unwrap().turn, 105);

    let mut reloaded = Store::default();
    reloaded.ensure_loaded(tmp.p());
    assert_eq!(reloaded.for_pane("p1"), in_memory, "the file holds the same ring");
}

#[test]
fn the_ring_file_keeps_only_the_most_recent_panes() {
    let tmp = Tmp::new("panes");
    let mut store = Store::default();
    for i in 0..(MAX_PANES_IN_FILE as u64 + 5) {
        store.record(tmp.p(), summary(&format!("pane-{i:02}"), 1, 1_000 * (i + 1))).unwrap();
    }
    let mut reloaded = Store::default();
    reloaded.ensure_loaded(tmp.p());
    let all = reloaded.since(0);
    assert_eq!(all.len(), MAX_PANES_IN_FILE);
    assert!(all.iter().all(|s| s.pane_id.as_str() >= "pane-05"), "the oldest five panes aged out");
}

#[test]
fn the_same_turn_replaces_its_entry() {
    let tmp = Tmp::new("replace");
    let mut store = Store::default();
    store.record(tmp.p(), summary("p1", 1, 1_000)).unwrap();
    store.record(tmp.p(), summary("p1", 1, 2_000)).unwrap();
    let items = store.for_pane("p1");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].ended_at_ms, 2_000);
}

#[test]
fn a_damaged_ring_file_is_an_empty_ring_and_is_rewritten() {
    let tmp = Tmp::new("damaged");
    std::fs::create_dir_all(tmp.p().join(".puppet-master")).unwrap();
    std::fs::write(ring_path(tmp.p()), "{ not json").unwrap();
    let mut store = Store::default();
    store.ensure_loaded(tmp.p());
    assert!(store.since(0).is_empty());
    store.record(tmp.p(), summary("p1", 1, 1_000)).unwrap();
    let mut again = Store::default();
    again.ensure_loaded(tmp.p());
    assert_eq!(again.for_pane("p1").len(), 1);
}

// ---- presence ----

const MIN: u64 = 60_000;

#[test]
fn presence_goes_away_after_the_threshold_and_returns_when_the_cockpit_is_focused() {
    let mut p = Presence::new(DEFAULT_AWAY_MS);
    assert_eq!(p.tick(0, 0), None);
    assert_eq!(p.tick(2 * MIN, 2 * MIN), None, "two minutes is not away");
    // Input stopped at t = 0; at 5 minutes the user counts as away.
    assert_eq!(p.tick(5 * MIN, 5 * MIN), None);
    assert_eq!(p.tick(30 * MIN, 30 * MIN), None, "still away, nothing to show yet");
    // Back at the keyboard, but the cockpit is not in front: the digest waits.
    assert_eq!(p.tick(31 * MIN, 1_000), None);
    assert_eq!(p.tick(32 * MIN, 2_000), None);
    // Opening or focusing the cockpit delivers it, once, from the moment input stopped.
    assert_eq!(p.set_focus(true), Some(Return { since_ms: 0 }));
    assert_eq!(p.set_focus(false), None);
    assert_eq!(p.set_focus(true), None, "already delivered");
}

#[test]
fn presence_returns_on_the_next_tick_when_the_cockpit_already_has_focus() {
    let mut p = Presence::new(DEFAULT_AWAY_MS);
    assert_eq!(p.set_focus(true), None, "focus alone is not a return");
    assert_eq!(p.tick(10 * MIN, 10 * MIN), None);
    assert_eq!(p.tick(10 * MIN + 5_000, 500), Some(Return { since_ms: 0 }));
    assert_eq!(p.tick(10 * MIN + 10_000, 600), None);
}

#[test]
fn presence_keeps_the_earliest_start_if_the_user_leaves_again_before_looking() {
    let mut p = Presence::new(DEFAULT_AWAY_MS);
    p.tick(6 * MIN, 6 * MIN); // away since t = 0
    p.tick(7 * MIN, 1_000); // back, cockpit not focused
    p.tick(13 * MIN, 6 * MIN); // away again (input stopped at 7 min)
    p.tick(14 * MIN, 500); // back again
    assert_eq!(p.set_focus(true), Some(Return { since_ms: 0 }), "the digest covers the whole gap");
}

#[test]
fn presence_threshold_is_configurable_and_zero_turns_it_off() {
    let mut p = Presence::new(MIN);
    p.tick(2 * MIN, 2 * MIN);
    p.tick(3 * MIN, 100);
    assert_eq!(p.set_focus(true), Some(Return { since_ms: 0 }), "a one minute threshold");

    p.set_threshold(0);
    assert_eq!(p.tick(100 * MIN, 100 * MIN), None);
    assert_eq!(p.tick(101 * MIN, 0), None);
    assert_eq!(p.set_focus(true), None);
}

#[test]
fn a_return_shows_a_digest_only_when_something_happened() {
    let mut p = Presence::new(DEFAULT_AWAY_MS);
    p.set_focus(true);
    p.tick(6 * MIN, 6 * MIN);
    let ret = p.tick(7 * MIN, 100).expect("returned");
    // Nothing finished while away: an empty digest (the app shows nothing).
    let quiet = digest::build(&[summary("p1", 1, 100)], vec![], ret.since_ms + 1_000, 7 * MIN);
    assert!(quiet.is_empty());
    // One turn finished while away: there is a digest.
    let busy = digest::build(&[summary("p1", 1, 3 * MIN)], vec![], ret.since_ms, 7 * MIN);
    assert!(!busy.is_empty());
}

#[test]
fn the_idle_reading_is_sane_on_this_machine() {
    // Windows reads GetLastInputInfo; other systems have no reading (never "away").
    let reading = super::presence::system_idle_ms();
    if cfg!(windows) {
        assert!(reading.is_some_and(|ms| ms < 365 * 24 * 3600 * 1000), "{reading:?}");
    } else {
        assert_eq!(reading, None);
    }
}

// ---- digest ----

#[test]
fn digest_has_one_line_per_agent_newest_first_and_counts_turns() {
    let a1 = summary("pa", 1, 10_000);
    let a2 = summary("pa", 2, 50_000);
    let b1 = summary("pb", 1, 30_000);
    let old = summary("pc", 1, 1_000);
    let needs = vec![NeedsYou { pane_id: "pb".into(), agent: "Codex".into(), status: "waiting_input".into(), prompt: Some("allow `rm x`?".into()) }];
    let d = digest::build(&[a1, a2, b1, old], needs.clone(), 5_000, 99_000);
    assert_eq!((d.agents_worked, d.turns), (2, 3));
    assert_eq!(d.lines.iter().map(|l| l.pane_id.as_str()).collect::<Vec<_>>(), vec!["pa", "pb"]);
    assert_eq!(d.lines[0].turns, 2);
    assert_eq!(d.lines[0].ended_at_ms, 50_000, "the latest summary of the agent is the line");
    assert_eq!(d.needs_you, needs);
    assert_eq!((d.since_ms, d.now_ms), (5_000, 99_000));
}

#[test]
fn digest_lines_are_capped_but_the_counter_is_not() {
    let many: Vec<TurnSummary> = (0..20u32).map(|i| summary(&format!("p{i:02}"), 1, 10_000 + u64::from(i))).collect();
    let d = digest::build(&many, vec![], 0, 50_000);
    assert_eq!(d.agents_worked, 20);
    assert_eq!(d.lines.len(), digest::MAX_LINES);
}

#[test]
fn last_seen_is_kept_per_device_and_persists() {
    let tmp = Tmp::new("seen");
    let path = tmp.p().join("last-seen.json");
    let mut seen = LastSeen::load(&path);
    assert_eq!(seen.get("phone-1"), None);
    seen.set("phone-1", 111);
    seen.set("phone-2", 222);
    let again = LastSeen::load(&path);
    assert_eq!((again.get("phone-1"), again.get("phone-2")), (Some(111), Some(222)));
}

// ---- GET /digest ----

struct FakeSource {
    now: u64,
    summaries: Vec<TurnSummary>,
}

impl Source for FakeSource {
    fn now_ms(&self) -> u64 {
        self.now
    }
    fn summaries_since(&self, since_ms: u64) -> Vec<TurnSummary> {
        self.summaries.iter().filter(|s| s.ended_at_ms >= since_ms).cloned().collect()
    }
    fn needs_you(&self) -> Vec<NeedsYou> {
        vec![NeedsYou { pane_id: "pb".into(), agent: "Codex".into(), status: "waiting_input".into(), prompt: Some("allow `npm test`?".into()) }]
    }
}

#[test]
fn get_digest_returns_what_happened_since_and_remembers_the_device() {
    let src = FakeSource { now: 100_000, summaries: vec![summary("pa", 1, 20_000), summary("pb", 1, 60_000)] };
    let seen = Mutex::new(LastSeen::default());

    // Explicit `since`, no marking: request -> response.
    let (status, body) = respond(&src, &seen, "GET", &["digest"], "since=50000", "phone-1").unwrap().unwrap();
    assert_eq!(status, 200);
    assert_eq!(body["agentsWorked"], 1);
    assert_eq!(body["lines"][0]["paneId"], "pb");
    assert_eq!(body["lines"][0]["sentence"], "Claude Code worked for 0s, now idle.");
    assert_eq!(body["needsYou"][0]["prompt"], "allow `npm test`?");
    assert_eq!(body["empty"], false);
    assert_eq!(body["previousSeenMs"], json!(null));
    assert_eq!(seen.lock().get("phone-1"), None, "not marked without seen=1");

    // Mark: the device's last visit becomes "now"; the next default window starts there.
    let (_, first) = respond(&src, &seen, "GET", &["digest"], "since=0&seen=1", "phone-1").unwrap().unwrap();
    assert_eq!(first["agentsWorked"], 2);
    assert_eq!(seen.lock().get("phone-1"), Some(100_000));
    let later = FakeSource { now: 200_000, summaries: vec![summary("pa", 1, 20_000), summary("pa", 2, 150_000)] };
    let (_, second) = respond(&later, &seen, "GET", &["digest"], "seen=1", "phone-1").unwrap().unwrap();
    assert_eq!(second["sinceMs"], 100_000);
    assert_eq!(second["previousSeenMs"], 100_000);
    assert_eq!(second["lines"].as_array().unwrap().len(), 1);
    assert_eq!(second["lines"][0]["turns"], 1);

    // Another device has its own last visit: nothing stored, so the window is empty.
    let (_, other) = respond(&later, &seen, "GET", &["digest"], "", "phone-2").unwrap().unwrap();
    assert_eq!(other["empty"], true);
}

#[test]
fn get_digest_rejects_a_bad_since_and_ignores_other_routes() {
    let src = FakeSource { now: 1, summaries: vec![] };
    let seen = Mutex::new(LastSeen::default());
    let (status, body) = respond(&src, &seen, "GET", &["digest"], "since=yesterday", "d").unwrap().unwrap_err();
    assert_eq!((status, body["code"].as_str()), (400, Some("INVALID_ARGUMENT")));
    assert!(respond(&src, &seen, "POST", &["digest"], "", "d").is_none());
    assert!(respond(&src, &seen, "GET", &["digests"], "", "d").is_none());
    assert!(respond(&src, &seen, "GET", &["panes", "p1", "checkpoints"], "", "d").is_none());
}

#[test]
fn digest_route_needs_a_paired_device_when_called_remotely() {
    use crate::mobile_pairing::{authorize_bridge_request, MobilePairingStore, PairRequestBody};
    use base64::Engine;

    // A remote caller without a token is refused before any route runs; the desktop's own
    // loopback tooling is let through.
    let denied = authorize_bridge_request("Host: example.com\r\n", false, &["digest"], "GET").unwrap_err();
    assert_eq!((denied.0, denied.1["error"].as_str()), (401, Some("pairing_required")));
    let proxied = authorize_bridge_request("X-PM-Proxied: 1\r\n", true, &["digest"], "GET").unwrap_err();
    assert_eq!(proxied.0, 401, "the PWA proxy does not get the loopback bypass");
    assert!(authorize_bridge_request("Host: 127.0.0.1\r\n", true, &["digest"], "GET").is_ok());

    // A paired device's bearer token maps to its device id, and a stranger's token to nothing.
    let tmp = Tmp::new("pairing");
    let mut store = MobilePairingStore::load_or_create(tmp.p().join("pairing.json")).unwrap();
    let session = store.create_pairing_session("http://127.0.0.1:1".into());
    let key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng).verifying_key().to_bytes();
    let paired = store
        .pair_device(PairRequestBody {
            pairing_code: session.pairing_code,
            device_name: "Test phone".into(),
            device_public_key: base64::engine::general_purpose::STANDARD.encode(key),
        })
        .unwrap();
    assert_eq!(store.device_id_for_token(&paired.device_token), Some(paired.device_id));
    assert_eq!(store.device_id_for_token("AAAA"), None);
}

// ---- live bug #5: "Last turn" said Turn 5, 0s, 0 files while History said "After turn 1 ... 1 file" ----

/// The real sequence from the live run (times are the ones in `turn-summaries.json`): two quiet
/// turns, the edit turn (166s), then two sub-second "running" flickers after it.
#[test]
fn flickers_are_not_turns_and_do_not_replace_the_last_turn() {
    let tmp = Tmp::new("blips");
    let env = FakeEnv::new(tmp.p());
    let tracker = Tracker::new(env.clone());
    tracker.on_status("p1", "idle");
    finish_idle(&tracker, &env, 21_404, 24_345);
    finish_idle(&tracker, &env, 31_839, 33_471);
    *env.files.lock() = Some(FileStats::from_changes(&[FileChange {
        path: "sum.js".into(),
        status: "M".into(),
        added: 1,
        deleted: 1,
        binary: false,
    }]));
    let edit = finish_idle(&tracker, &env, 37_757, 203_604);
    assert_eq!(edit.turn, 3);
    *env.files.lock() = Some(FileStats::from_changes(&[]));

    // The two flickers: running for 0.8s and 0.9s.
    env.at(203_913);
    tracker.on_status("p1", "running");
    env.at(204_708);
    assert!(tracker.on_status("p1", "idle").is_none(), "a flicker is not a turn");
    env.at(210_669);
    tracker.on_status("p1", "running");
    env.at(211_529);
    assert!(tracker.on_status("p1", "idle").is_none());

    let all = tracker.summaries("p1");
    let last = all.last().unwrap();
    assert_eq!((last.turn, last.files.as_ref().map(|f| f.count)), (3, Some(1)), "Last turn is still the edit turn");
    assert_eq!(all.iter().map(|s| s.turn).collect::<Vec<_>>(), vec![1, 2, 3]);

    // The next real turn takes the number the flickers gave back.
    let next = finish_idle(&tracker, &env, 300_000, 310_000);
    assert_eq!(next.turn, 4);
}

#[test]
fn a_short_turn_that_needed_an_approval_is_still_a_turn() {
    let tmp = Tmp::new("short-approval");
    let env = FakeEnv::new(tmp.p());
    let tracker = Tracker::new(env.clone());
    tracker.on_status("p1", "idle");
    env.at(10_000);
    tracker.on_status("p1", "running");
    tracker.record_approval("p1", approval("a1", "command", "allow"));
    env.at(10_300);
    assert!(tracker.on_status("p1", "idle").is_some());
}

#[test]
fn summaries_that_finish_out_of_order_are_listed_by_turn() {
    let mut ring = std::collections::VecDeque::new();
    for turn in [3, 5, 4] {
        super::store::upsert(&mut ring, summary("p1", turn, u64::from(turn) * 1000), 100);
    }
    assert_eq!(ring.iter().map(|s| s.turn).collect::<Vec<_>>(), vec![3, 4, 5]);
}

/// Checkpoints take their turn number from the tracker, so History's "After turn N" is the
/// "Turn N" of Last turn even when earlier turns changed nothing (and left no checkpoint).
#[test]
fn checkpoint_turn_numbers_follow_the_trackers_turns() {
    let tmp = Tmp::new("ckpt-turn");
    let env = FakeEnv::new(tmp.p());
    let tracker = Tracker::new(env.clone());
    assert_eq!(tracker.turn_for_checkpoint("p1", true), None, "unknown pane");
    tracker.on_status("p1", "idle");
    assert_eq!(tracker.turn_for_checkpoint("p1", true), Some(1), "the turn about to start");
    finish_idle(&tracker, &env, 2_000, 5_000);
    finish_idle(&tracker, &env, 6_000, 9_000);
    assert_eq!(tracker.turn_for_checkpoint("p1", true), Some(3));
    env.at(10_000);
    tracker.on_status("p1", "running");
    assert_eq!(tracker.turn_for_checkpoint("p1", true), Some(3), "before, once the turn is open");
    env.at(20_000);
    tracker.on_status("p1", "idle");
    assert_eq!(tracker.turn_for_checkpoint("p1", false), Some(3), "after belongs to the turn that ended");
}

// ---- an approval-gated edit: Receipt (Last turn) and History must agree (VERIFY #5) --------------
//
// Live, Claude pane dc69ffa9, turn 4: running -> waiting_input (approval) -> running -> idle, the file
// was edited after the approval. History read "After turn 4: 1 file", Last turn read "0 files, you
// approved 1 request". The checkpoint worker of the approval prompt finished while the end-of-turn
// one was still waiting out the 10 s debounce; its settle signal released the end-of-turn summary,
// which read the checkpoints before the end-of-turn snapshot existed.

fn one_file() -> FileStats {
    FileStats::from_changes(&synthetic_diff()[..1])
}

#[test]
fn the_end_of_turn_summary_waits_for_its_own_snapshot_and_counts_the_file_edited_after_the_approval() {
    use super::settle::Settle;
    use super::summarize;
    use std::time::Duration;

    let tmp = Tmp::new("approval-edit");
    let env = FakeEnv::new(tmp.p());
    let tracker = Arc::new(Tracker::new(env.clone()));
    let settle = Arc::new(Settle::default());

    env.at(1_000);
    assert!(tracker.on_status("p1", "running").is_none());
    env.at(3_000);
    let at_approval = tracker.on_status("p1", "waiting_input").expect("rest point at the approval prompt");
    let approval_capture = settle.request("p1");
    tracker.record_approval("p1", approval("a1", "command", "allow"));
    env.at(3_500);
    assert!(tracker.on_status("p1", "running").is_none(), "the approval resumes the same turn");
    env.at(6_000);
    let at_idle = tracker.on_status("p1", "idle").expect("rest point at the end of the turn");
    let end_capture = settle.request("p1");
    assert_eq!(at_idle.turn, at_approval.turn);

    // The snapshot taken at the approval prompt (no edit yet) finishes first.
    *env.files.lock() = Some(FileStats::from_changes(&[]));
    settle.settle("p1", approval_capture);
    let final_summary = {
        let (tracker, settle) = (tracker.clone(), settle.clone());
        std::thread::spawn(move || summarize(&tracker, &settle, at_idle, end_capture, Duration::from_secs(10)))
    };
    std::thread::sleep(Duration::from_millis(150));
    assert!(!final_summary.is_finished(), "the approval prompt's snapshot is not the end-of-turn one");

    // The end-of-turn snapshot lands, with the edit in it.
    *env.files.lock() = Some(one_file());
    settle.settle("p1", end_capture);
    let last = final_summary.join().unwrap().expect("summary");
    assert_eq!(last.files.as_ref().map(|f| f.count), Some(1), "{}", last.sentence);
    assert_eq!(last.approvals.len(), 1);

    // The approval prompt's own summary was built earlier and is only stored now: it must not win.
    *env.files.lock() = Some(FileStats::from_changes(&[]));
    let stale = summarize(&tracker, &settle, at_approval, approval_capture, Duration::from_secs(1)).expect("summary");
    assert_eq!(stale.files.as_ref().map(|f| f.count), Some(0));
    let kept = tracker.summaries("p1");
    assert_eq!(kept.len(), 1, "one entry for the turn");
    assert_eq!(kept[0].files.as_ref().map(|f| f.count), Some(1), "Last turn keeps the end-of-turn facts");
    assert!(kept[0].sentence.contains("now idle"), "{}", kept[0].sentence);
}

#[test]
fn a_stale_summary_never_replaces_a_later_one_of_the_same_turn() {
    let tmp = Tmp::new("stale");
    let mut store = Store::default();
    store.record(tmp.p(), summary("p1", 4, 9_000)).unwrap();
    store.record(tmp.p(), summary("p1", 4, 3_000)).unwrap();
    let items = store.for_pane("p1");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].ended_at_ms, 9_000);
}
