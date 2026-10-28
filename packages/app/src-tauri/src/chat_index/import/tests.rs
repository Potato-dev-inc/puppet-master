//! The importer against fake stores and an in-memory index: what it reads, what it skips, how it stops.

use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use super::fakes::{
    finished, importer, is_reading, memory_index, source, wait_until, FakeHistory, NoPanes,
};
use super::{ImportRequest, Importer, Phase, Progress, IMPORT_NOTHING_CHOSEN, IMPORT_RUNNING, INDEX_OFF};
use crate::chat_index::store::Store;
use crate::chat_index::{ChatIndex, ContextLookup, PaneContext};
use crate::session_chat::history::{HistoryEnv, ReadError};
use crate::session_chat::{ChatItem, ItemObserver};

const NOW_MS: i64 = 1_790_000_000_000;

fn request(agents: &[&str]) -> ImportRequest {
    ImportRequest { agents: agents.iter().map(|id| id.to_string()).collect(), since_days: None }
}

fn env() -> HistoryEnv {
    HistoryEnv::isolated()
}

/// A fake store with `count` chats, `sess-0001`... newest first by number.
fn store_of(count: usize, words: &str) -> &'static FakeHistory {
    let fake = FakeHistory::new();
    for n in 1..=count {
        fake.add(&format!("sess-{n:04}"), &format!("{n}:100"), NOW_MS - n as i64 * 1000, &format!("{words} number{n}"));
    }
    fake.leak()
}

#[test]
fn scan_lists_without_reading_any_chat() {
    let (index, _) = memory_index();
    let fake = store_of(3, "scanword");
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    let found = importer.scan(&env(), None);
    assert_eq!(found.len(), 1);
    let found = &found[0];
    assert_eq!((found.id.as_str(), found.chats, found.bytes, found.already, found.too_big), ("agent-a", 3, 300, 0, 0));
    assert_eq!((found.oldest_ms, found.newest_ms), (Some(NOW_MS - 3000), Some(NOW_MS - 1000)));
    assert_eq!(found.places, vec!["~/.fake/chats".to_string()]);
    assert!(found.can_continue && !found.from_definition);
    assert_eq!(fake.read_count(), 0, "a scan reads no chat");
    assert_eq!(index.status().unwrap().imported_chats, 0);
}

#[test]
fn scan_counts_what_is_already_in_the_index_and_passes_the_time_filter() {
    let (index, _) = memory_index();
    let fake = store_of(2, "words");
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    assert_eq!(finished(&importer).phase, Phase::Done);
    let found = importer.scan(&env(), Some(30));
    assert_eq!((found[0].chats, found[0].already), (2, 2));
    let limits = fake.last_limits.lock().clone().unwrap();
    let since = limits.since_ms.expect("a time filter");
    let thirty_days = 30 * 86_400_000;
    assert!((super::now_ms() - thirty_days - since).abs() < 60_000);
    // A changed stamp is no longer "already".
    fake.set_stamp("sess-0001", "changed");
    assert_eq!(importer.scan(&env(), None)[0].already, 1);
}

#[test]
fn a_run_indexes_every_chat_and_never_the_tool_output() {
    let (index, _) = memory_index();
    let fake = store_of(4, "stripe webhook");
    let (importer, emitted) = importer(vec![source("agent-a", false, fake)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    let end = finished(&importer);
    assert_eq!(end.phase, Phase::Done);
    assert_eq!((end.chats_done, end.chats_total, end.items), (4, 4, 12), "user, assistant and tool title per chat");
    let status = index.status().unwrap();
    assert_eq!((status.imported_chats, status.imported_items, status.items), (4, 12, 0));
    let hits = index.search_hits("stripe webhook", 50).unwrap();
    assert_eq!(hits.len(), 12);
    assert!(hits.iter().all(|hit| hit.imported));
    assert!(index.search_hits("TOOLOUTPUTMARKER", 10).unwrap().is_empty());
    let row = index.imported_chat("agent-a", "sess-0001").unwrap().expect("the record");
    assert_eq!(row.title, "question about stripe webhook number1");
    assert_eq!((row.folder.as_str(), row.cwd.as_str(), row.items, row.cut_short), ("shop", "~/projects/shop", 3, false));
    assert_eq!(row.stamp, "1:100");
    // The events: the new run, the reading phase and the end, in that order.
    let phases: Vec<Phase> = emitted.lock().iter().map(|p| p.phase).collect();
    assert_eq!(phases.first(), Some(&Phase::Looking));
    assert!(phases.contains(&Phase::Reading));
    assert_eq!(phases.last(), Some(&Phase::Done));
}

#[test]
fn an_agent_that_is_not_chosen_has_none_of_its_files_opened() {
    let (index, _) = memory_index();
    let (on, off) = (store_of(2, "wanted"), store_of(2, "unwanted"));
    let (importer, _) = importer(vec![source("agent-a", false, on), source("agent-b", false, off)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    assert_eq!(finished(&importer).phase, Phase::Done);
    assert_eq!((on.read_count(), on.lists.load(Ordering::SeqCst)), (2, 1));
    assert_eq!((off.read_count(), off.lists.load(Ordering::SeqCst)), (0, 0), "not listed, not read");
    assert!(index.search_hits("unwanted", 10).unwrap().is_empty());
    assert_eq!(index.status().unwrap().imported_chats, 2);
}

#[test]
fn a_second_run_with_unchanged_stamps_reads_nothing() {
    let (index, _) = memory_index();
    let fake = store_of(3, "again");
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    assert_eq!(finished(&importer).phase, Phase::Done);
    assert_eq!(fake.read_count(), 3);
    importer.start(request(&["agent-a"]), env()).unwrap();
    let second = finished(&importer);
    assert_eq!(second.phase, Phase::Done);
    assert_eq!((fake.read_count(), second.unchanged, second.items), (3, 3, 0), "nothing was read again");
    assert_eq!(index.status().unwrap().imported_items, 9, "and nothing was doubled");
}

#[test]
fn a_changed_stamp_reads_exactly_that_chat_again() {
    let (index, _) = memory_index();
    let fake = store_of(3, "before");
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    assert_eq!(finished(&importer).phase, Phase::Done);
    fake.set_stamp("sess-0002", "2:999");
    fake.items.lock().insert("sess-0002".into(), super::fakes::conversation("after change"));
    importer.start(request(&["agent-a"]), env()).unwrap();
    let second = finished(&importer);
    assert_eq!((second.unchanged, second.items), (2, 3));
    assert_eq!(fake.read_count(), 4);
    assert_eq!(fake.read_sessions.lock().last().map(String::as_str), Some("sess-0002"));
    assert!(index.search_hits("after change", 10).unwrap().len() == 3);
    assert!(index.search_hits("before number2", 10).unwrap().is_empty(), "the old copy of that chat is replaced");
    assert_eq!(index.status().unwrap().imported_chats, 3);
}

#[test]
fn start_refuses_with_a_code_when_it_cannot_run() {
    let (index, switch) = memory_index();
    let fake = store_of(1, "words");
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    assert_eq!(importer.start(request(&[]), env()), Err(IMPORT_NOTHING_CHOSEN.to_string()));
    assert_eq!(importer.start(request(&["agent-unknown"]), env()), Err(IMPORT_NOTHING_CHOSEN.to_string()));
    switch.store(false, Ordering::SeqCst);
    assert_eq!(importer.start(request(&["agent-a"]), env()), Err(INDEX_OFF.to_string()));
    assert_eq!(importer.progress().phase, Phase::Idle, "a refused start changes nothing");
    assert_eq!(fake.lists.load(Ordering::SeqCst), 0);
}

#[test]
fn a_second_start_while_running_is_refused() {
    let (index, _) = memory_index();
    let fake = FakeHistory::new();
    for n in 1..=3 {
        fake.add(&format!("sess-{n:04}"), "1:1", NOW_MS - n, "slow words");
    }
    let fake = FakeHistory { delay: Duration::from_millis(30), ..fake }.leak();
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    assert_eq!(importer.start(request(&["agent-a"]), env()), Err(IMPORT_RUNNING.to_string()));
    assert_eq!(finished(&importer).phase, Phase::Done);
    // After the end the next start is accepted.
    wait_until("the worker to let go", || importer.start(request(&["agent-a"]), env()).is_ok());
    assert_eq!(finished(&importer).phase, Phase::Done);
}

/// Hands the importer to a hook that is built before it exists.
type Late = Arc<OnceLock<Arc<Importer>>>;

#[test]
fn stop_between_chats_leaves_only_whole_chats() {
    let (index, _) = memory_index();
    let fake = FakeHistory::new();
    for n in 1..=6 {
        fake.add(&format!("sess-{n:04}"), "1:1", NOW_MS - n, &format!("whole{n}"));
    }
    let fake = fake.leak();
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    let late: Late = Arc::new(OnceLock::new());
    let hook_late = late.clone();
    *fake.on_read.lock() = Some(Box::new(move |session| {
        // The person presses Stop while the third chat is being read.
        if session == "sess-0004" {
            hook_late.get().expect("importer").cancel();
        }
    }));
    late.set(importer.clone()).ok();
    importer.start(request(&["agent-a"]), env()).unwrap();
    let end = finished(&importer);
    assert_eq!((end.phase, end.message.clone()), (Phase::Cancelled, None));
    // Sessions are read newest first: 0001..0003 were written whole, 0004 (in hand) was dropped, nothing after it.
    assert_eq!(fake.read_count(), 4);
    let status = index.status().unwrap();
    assert_eq!((status.imported_chats, status.imported_items), (3, 9));
    for n in 1..=3 {
        let row = index.imported_chat("agent-a", &format!("sess-{n:04}")).unwrap().expect("whole chat");
        assert_eq!(row.items, 3);
    }
    assert!(index.imported_chat("agent-a", "sess-0004").unwrap().is_none(), "the chat in hand is not half written");
    assert!(index.search_hits("whole4", 10).unwrap().is_empty());
}

#[test]
fn cancel_does_nothing_when_no_import_runs() {
    let (index, _) = memory_index();
    let fake = store_of(1, "words");
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    importer.cancel();
    importer.start(request(&["agent-a"]), env()).unwrap();
    assert_eq!(finished(&importer).phase, Phase::Done, "an old cancel does not stop the next run");
}

#[test]
fn switching_the_index_off_mid_run_cancels_it_and_writes_nothing_more() {
    let (index, switch) = memory_index();
    let fake = FakeHistory::new();
    for n in 1..=6 {
        fake.add(&format!("sess-{n:04}"), "1:1", NOW_MS - n, &format!("switch{n}"));
    }
    let fake = fake.leak();
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    let flag = switch.clone();
    *fake.on_read.lock() = Some(Box::new(move |session| {
        if session == "sess-0003" {
            flag.store(false, Ordering::SeqCst);
        }
    }));
    importer.start(request(&["agent-a"]), env()).unwrap();
    let end = finished(&importer);
    assert_eq!(end.phase, Phase::Cancelled);
    assert_eq!(end.message.as_deref(), Some("Chat search was turned off"));
    assert_eq!(fake.read_count(), 3, "no chat is read after the switch");
    // The switch is off, so the numbers come from the store directly: two whole chats and no third.
    switch.store(true, Ordering::SeqCst);
    let status = index.status().unwrap();
    assert_eq!((status.imported_chats, status.imported_items), (2, 6));
    assert!(index.imported_chat("agent-a", "sess-0003").unwrap().is_none());
}

#[test]
fn a_chat_that_fails_is_counted_and_the_run_goes_on() {
    let (index, _) = memory_index();
    let fake = store_of(6, "mixed");
    fake.errors.lock().insert("sess-0001".into(), ReadError::TooBig);
    fake.errors.lock().insert("sess-0002".into(), ReadError::Unreadable);
    fake.errors.lock().insert("sess-0003".into(), ReadError::Format);
    fake.errors.lock().insert("sess-0004".into(), ReadError::Missing);
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    let end = finished(&importer);
    assert_eq!(end.phase, Phase::Done);
    assert_eq!((end.too_big, end.unreadable, end.chats_done, end.items), (1, 3, 6, 6));
    assert_eq!(index.status().unwrap().imported_chats, 2);
}

#[test]
fn a_store_that_panics_ends_the_run_as_failed_and_frees_the_importer() {
    let (index, _) = memory_index();
    let fake = store_of(3, "panic");
    fake.panics.lock().push("sess-0002".into());
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    let end = finished(&importer);
    assert_eq!(end.phase, Phase::Failed);
    assert_eq!(end.message.as_deref(), Some("Reading stopped unexpectedly"));
    fake.panics.lock().clear();
    wait_until("the worker to let go", || importer.start(request(&["agent-a"]), env()).is_ok());
    assert_eq!(finished(&importer).phase, Phase::Done);
}

#[test]
fn an_id_that_is_not_a_safe_session_id_is_not_read() {
    let (index, _) = memory_index();
    let fake = FakeHistory::new();
    fake.add("short", "1:1", NOW_MS, "bad id");
    fake.add("has space in it", "1:1", NOW_MS - 1, "bad id");
    fake.add("fine-session-1", "1:1", NOW_MS - 2, "good id");
    let fake = fake.leak();
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    let end = finished(&importer);
    assert_eq!((end.phase, end.unreadable, end.items), (Phase::Done, 2, 3));
    assert_eq!(fake.read_count(), 1);
}

/// A lookup that says every live pane is `session` of `agent-a`.
struct LiveSession(&'static str);

impl ContextLookup for LiveSession {
    fn context(&self, _pane_id: &str) -> Option<PaneContext> {
        Some(PaneContext { agent: "agent-a".into(), folder: "shop".into(), session: self.0.into() })
    }
}

#[test]
fn a_chat_that_is_already_live_is_skipped() {
    let switch = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let index = ChatIndex::start_switched(Store::open_memory(1000).unwrap(), Arc::new(LiveSession("sess-0001")), switch);
    index.observe("pane-1", &[ChatItem::User { id: "5:0".into(), at: None, text: "live words here".into() }]);
    wait_until("the live row", || index.count() == 1);
    let fake = store_of(3, "oldwords");
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    let end = finished(&importer);
    assert_eq!((end.phase, end.unchanged, end.chats_done), (Phase::Done, 1, 3));
    assert_eq!(fake.read_count(), 2, "the live chat was not read");
    assert!(index.imported_chat("agent-a", "sess-0001").unwrap().is_none());
    assert_eq!(index.status().unwrap().imported_chats, 2);
}

#[test]
fn imported_chats_are_invisible_to_a_search_limited_to_panes() {
    let (index, _) = memory_index();
    let fake = store_of(2, "secretplan");
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    assert_eq!(finished(&importer).phase, Phase::Done);
    assert!(!index.search_hits("secretplan", 10).unwrap().is_empty());
    let panes: Vec<String> = ["pane-1", "pane-2", "agent-a", "sess-0001"].iter().map(|s| s.to_string()).collect();
    assert!(index.search_in("secretplan", 10, &panes).unwrap().hits.is_empty(), "the companion's recall never sees old chats");
}

#[test]
fn the_worker_yields_so_the_live_writer_still_gets_rows_in() {
    let switch = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let index = ChatIndex::start_switched(Store::open_memory(1000).unwrap(), Arc::new(NoPanes), switch);
    let fake = FakeHistory::new();
    for n in 1..=40 {
        fake.add(&format!("sess-{n:04}"), "1:1", NOW_MS - n, &format!("slowdata{n}"));
    }
    let fake = FakeHistory { delay: Duration::from_millis(15), chunk: 2, ..fake }.leak();
    let (importer, _) = importer(vec![source("agent-a", false, fake)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    wait_until("the reading phase", || is_reading(&importer) && importer.progress().chats_done >= 2);
    index.observe("pane-live", &[ChatItem::User { id: "9:0".into(), at: None, text: "livewords arrive meanwhile".into() }]);
    wait_until("the live row", || index.count() >= 1 + index.status().unwrap().imported_items);
    assert!(is_reading(&importer), "the live row was written while the import was still running");
    importer.cancel();
    assert_eq!(finished(&importer).phase, Phase::Cancelled);
    assert_eq!(index.search_hits("livewords", 10).unwrap().len(), 1);
}

#[test]
fn the_progress_event_is_throttled_and_always_carries_the_end() {
    let (index, _) = memory_index();
    let fake = store_of(40, "many");
    let (importer, emitted) = importer(vec![source("agent-a", false, fake)], &index);
    importer.start(request(&["agent-a"]), env()).unwrap();
    let end = finished(&importer);
    assert_eq!((end.phase, end.chats_done), (Phase::Done, 40));
    let seen = emitted.lock().clone();
    assert!(seen.len() < 20, "{} events for 40 chats", seen.len());
    assert_eq!(seen.last().map(|p| p.phase), Some(Phase::Done));
    assert_eq!(seen.last().map(|p| p.chats_done), Some(40));
}

#[test]
fn progress_is_sent_to_the_screen_in_camel_case_with_a_lowercase_phase() {
    let progress = Progress { phase: Phase::Reading, agent: Some("agent-a".into()), chats_done: 3, chats_total: 9, ..Progress::idle() };
    let json = serde_json::to_value(&progress).unwrap();
    assert_eq!(json["phase"], "reading");
    assert_eq!((json["chatsDone"].as_u64(), json["chatsTotal"].as_u64()), (Some(3), Some(9)));
    assert!(json.get("tooBig").is_some() && json.get("unreadable").is_some() && json["message"].is_null());
    for (phase, name) in [(Phase::Idle, "idle"), (Phase::Looking, "looking"), (Phase::Done, "done"), (Phase::Cancelled, "cancelled"), (Phase::Failed, "failed")] {
        assert_eq!(serde_json::to_value(phase).unwrap(), name);
    }
}

#[test]
fn the_registry_catalog_lists_only_profiles_that_answer_session_history() {
    use super::Catalog;
    let sources = super::RegistryCatalog.sources();
    let registry = crate::tui::registry::registry();
    let code = registry.code_ids();
    assert!(!sources.is_empty(), "the built-in agents with old chats are listed");
    for source in &sources {
        let profile = registry.find(&source.id).expect("a registered profile");
        assert!(profile.session_history().is_some());
        assert_eq!(source.from_definition, !code.contains(&profile.id()), "from a definition = not a compiled-in profile");
    }
    let everything: Vec<_> = registry.ordered().iter().filter(|p| p.session_history().is_some()).map(|p| p.id()).collect();
    assert_eq!(sources.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), everything, "registry order");
}

// ---- prefs ----

fn temp_file(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-bc17-prefs-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn prefs_default_to_a_year_with_built_in_agents_on_and_definitions_off() {
    use super::{ImportPrefs, DEFAULT_SINCE_DAYS};
    let prefs = ImportPrefs::default();
    assert_eq!((prefs.asked, prefs.since_days, prefs.last_run_ms), (false, Some(DEFAULT_SINCE_DAYS), 0));
    assert_eq!(DEFAULT_SINCE_DAYS, 365);
    assert!(prefs.enabled_for("any-built-in", false));
    assert!(!prefs.enabled_for("from-a-definition", true));
}

#[test]
fn prefs_round_trip_and_a_switch_wins_over_the_default() {
    use super::ImportPrefs;
    let path = temp_file(super::prefs::FILE_NAME);
    let mut prefs = ImportPrefs { asked: true, since_days: None, last_run_ms: 77, ..ImportPrefs::default() };
    prefs.set_agent("agent-a", false);
    prefs.set_agent("agent-def", true);
    prefs.save_to(&path).unwrap();
    let back = ImportPrefs::load_from(&path);
    assert_eq!(back, prefs);
    assert_eq!(back.since_days, None, "null in the file means all chats");
    assert!(!back.enabled_for("agent-a", false) && back.enabled_for("agent-def", true));
    assert!(!path.with_extension("json.tmp").exists(), "the temp file is renamed away");
}

#[test]
fn prefs_that_are_missing_damaged_or_partial_fall_back_to_the_defaults() {
    use super::ImportPrefs;
    let path = temp_file(super::prefs::FILE_NAME);
    assert_eq!(ImportPrefs::load_from(&path), ImportPrefs::default());
    std::fs::write(&path, "{ half written").unwrap();
    assert_eq!(ImportPrefs::load_from(&path), ImportPrefs::default());
    // A file from another build: unknown fields are ignored, missing ones are the defaults.
    std::fs::write(&path, r#"{"asked":true,"agents":{"agent-a":false},"future_field":1}"#).unwrap();
    let loaded = ImportPrefs::load_from(&path);
    assert!(loaded.asked && !loaded.enabled_for("agent-a", false));
    assert_eq!(loaded.since_days, Some(365));
}

#[test]
fn the_progress_reporter_sends_phase_changes_at_once_and_other_updates_at_most_four_times_a_second() {
    use super::progress::Reporter;
    use super::Emitter;
    let seen: Arc<parking_lot::Mutex<Vec<Progress>>> = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = seen.clone();
    let emit: Emitter = Arc::new(move |p: &Progress| sink.lock().push(p.clone()));
    let reporter = Reporter::new(emit);
    let started = std::time::Instant::now();
    reporter.update(|p| p.phase = Phase::Looking);
    for _ in 0..1000 {
        reporter.update(|p| p.chats_done += 1);
    }
    // One event per elapsed quarter second at most (plus the phase change itself).
    let allowed = 1 + 1 + (started.elapsed().as_millis() / 250) as usize;
    assert!(seen.lock().len() <= allowed, "a thousand quick updates are not a thousand events");
    reporter.update(|p| p.phase = Phase::Done);
    let seen = seen.lock();
    assert_eq!(seen.last().map(|p| (p.phase, p.chats_done)), Some((Phase::Done, 1000)), "the end carries the latest numbers");
    assert_eq!(reporter.get().chats_done, 1000);
}
