use super::*;

use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

use crate::session_chat::model::SessionFormat;

/// Reads lines like `u:hello` as a user message and ignores everything else.
struct FakeFormat;

impl SessionFormat for FakeFormat {
    fn parse_line(&self, line: &str, offset: u64) -> Vec<ChatItem> {
        match line.strip_prefix("u:") {
            Some(text) => vec![ChatItem::User { id: format!("{offset}:0"), at: None, text: text.to_string() }],
            None => Vec::new(),
        }
    }
}

/// A source whose answer the test sets.
struct FakeSource {
    located: Mutex<Located>,
    place: Mutex<Option<PathBuf>>,
}

impl SessionSource for FakeSource {
    fn format(&self) -> &dyn SessionFormat {
        &FakeFormat
    }

    fn locate(&self, _pane: &PaneFacts) -> Located {
        self.located.lock().clone()
    }

    fn looking_in(&self, _pane: &PaneFacts) -> Option<PathBuf> {
        self.place.lock().clone()
    }
}

const PLACE: &str = "/sessions/proj";

fn source(located: Located) -> &'static FakeSource {
    Box::leak(Box::new(FakeSource { located: Mutex::new(located), place: Mutex::new(Some(PathBuf::from(PLACE))) }))
}

#[derive(Default)]
struct FakeSink {
    sent: Mutex<Vec<(String, Vec<ChatItem>, bool)>>,
    waits: Mutex<Vec<(String, Option<String>)>>,
}

impl Sink for FakeSink {
    fn send(&self, pane_id: &str, items: Vec<ChatItem>, reset: bool) {
        self.sent.lock().push((pane_id.to_string(), items, reset));
    }

    fn waiting(&self, pane_id: &str, waiting_for: Option<&str>) {
        self.waits.lock().push((pane_id.to_string(), waiting_for.map(str::to_string)));
    }
}

impl FakeSink {
    /// Each batch as its texts and its reset flag.
    fn texts(&self) -> Vec<(Vec<String>, bool)> {
        self.sent.lock().iter().map(|(_, items, reset)| (texts(items), *reset)).collect()
    }
}

/// The pane exists until the test removes it.
struct FakePanes {
    facts: Mutex<Option<PaneFacts>>,
}

impl PaneLookup for FakePanes {
    fn facts(&self, _pane_id: &str) -> Option<PaneFacts> {
        self.facts.lock().clone()
    }
}

fn facts() -> PaneFacts {
    PaneFacts {
        cwd: PathBuf::from(r"C:\Users\you\proj"),
        started_at: SystemTime::now(),
        provider_session_id: None,
        home: None,
        launch: Default::default(),
    }
}

fn texts(items: &[ChatItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            ChatItem::User { text, .. } => text.clone(),
            other => panic!("{other:?}"),
        })
        .collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

struct Rig {
    watchers: Watchers,
    sink: Arc<FakeSink>,
    panes: Arc<FakePanes>,
    dir: PathBuf,
}

impl Rig {
    fn new(max_items: usize) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "pm-session-chat-watch-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sink = Arc::new(FakeSink::default());
        let panes = Arc::new(FakePanes { facts: Mutex::new(Some(facts())) });
        let tuning = Tuning { poll: Duration::from_millis(10), screen_poll: Duration::from_millis(10), api_poll: Duration::from_millis(10), relocate: Duration::from_millis(10), max_items };
        let watchers = Watchers::with_tuning(sink.clone(), panes.clone(), tuning);
        Rig { watchers, sink, panes, dir }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.watchers.close_all();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn append(path: &Path, text: &str) {
    let mut file = std::fs::OpenOptions::new().append(true).create(true).open(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn items_already_in_the_file_come_back_from_open_and_are_not_sent_again() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("a.jsonl");
    append(&path, "u:one\nnoise\nu:two\n");
    let src = source(Located::Found(path.clone()));

    let items = rig.watchers.open("p1", "w", src);
    assert_eq!(texts(&items), strings(&["one", "two"]));
    assert_eq!(items[1].id(), "12:0");
    assert!(rig.sink.texts().is_empty());

    append(&path, "u:three\n");
    wait_until("the appended line", || rig.sink.texts() == vec![(strings(&["three"]), false)]);
    assert_eq!(rig.sink.sent.lock()[0].0, "p1");
}

#[test]
fn a_file_that_appears_late_is_found_and_read_while_the_pane_lives() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("late.jsonl");
    let src = source(Located::NotYet);

    assert!(rig.watchers.open("p1", "w", src).is_empty());
    std::thread::sleep(Duration::from_millis(60));
    assert!(rig.sink.texts().is_empty());
    assert_eq!(rig.watchers.running(), 1);

    append(&path, "u:hello\n");
    *src.located.lock() = Located::Found(path);
    wait_until("the late file", || rig.sink.texts() == vec![(strings(&["hello"]), false)]);
    assert_eq!(texts(&rig.watchers.open("p1", "w", src)), strings(&["hello"]));
}

#[test]
fn an_ambiguous_answer_waits_like_not_yet() {
    let rig = Rig::new(MAX_ITEMS);
    let (a, b) = (rig.file("a.jsonl"), rig.file("b.jsonl"));
    append(&a, "u:a\n");
    append(&b, "u:b\n");
    let src = source(Located::Ambiguous(vec![a.clone(), b]));

    assert!(rig.watchers.open("p1", "w", src).is_empty());
    std::thread::sleep(Duration::from_millis(60));
    assert!(rig.sink.texts().is_empty());

    *src.located.lock() = Located::Found(a);
    wait_until("the settled file", || rig.sink.texts() == vec![(strings(&["a"]), false)]);
}

#[test]
fn lines_appended_in_pieces_are_sent_once_each_when_complete() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("pieces.jsonl");
    append(&path, "");
    rig.watchers.open("p1", "w", source(Located::Found(path.clone())));

    append(&path, "u:fir");
    std::thread::sleep(Duration::from_millis(60));
    assert!(rig.sink.texts().is_empty());

    append(&path, "st\nu:second\n");
    wait_until("both lines", || rig.sink.texts().iter().map(|(t, _)| t.len()).sum::<usize>() == 2);
    let mut sent = Vec::new();
    for (batch, reset) in rig.sink.texts() {
        assert!(!reset);
        sent.extend(batch);
    }
    assert_eq!(sent, strings(&["first", "second"]));
}

#[test]
fn a_truncated_file_sends_a_reset_with_what_is_left() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("trunc.jsonl");
    append(&path, "u:one\nu:two\n");
    let src = source(Located::Found(path.clone()));
    rig.watchers.open("p1", "w", src);

    std::fs::write(&path, "u:new\n").unwrap();
    wait_until("the reset", || rig.sink.texts() == vec![(strings(&["new"]), true)]);
    assert_eq!(texts(&rig.watchers.open("p1", "w", src)), strings(&["new"]));
}

#[test]
fn a_different_file_for_the_pane_sends_a_reset_with_the_new_file() {
    let rig = Rig::new(MAX_ITEMS);
    let (old, new) = (rig.file("old.jsonl"), rig.file("new.jsonl"));
    append(&old, "u:before\n");
    let src = source(Located::Found(old.clone()));
    assert_eq!(texts(&rig.watchers.open("p1", "w", src)), strings(&["before"]));

    append(&new, "u:after\n");
    *src.located.lock() = Located::Found(new);
    wait_until("the switch", || rig.sink.texts() == vec![(strings(&["after"]), true)]);

    // The old file is no longer read.
    append(&old, "u:ignored\n");
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(rig.sink.texts().len(), 1);
}

#[test]
fn a_new_file_that_is_still_empty_clears_the_view() {
    let rig = Rig::new(MAX_ITEMS);
    let old = rig.file("old.jsonl");
    append(&old, "u:before\n");
    let src = source(Located::Found(old));
    rig.watchers.open("p1", "w", src);

    *src.located.lock() = Located::Found(rig.file("not-written-yet.jsonl"));
    wait_until("the clear", || rig.sink.texts() == vec![(Vec::new(), true)]);
    assert!(rig.watchers.open("p1", "w", src).is_empty());
}

#[test]
fn losing_the_answer_keeps_following_the_file_it_had() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("keep.jsonl");
    append(&path, "u:one\n");
    let src = source(Located::Found(path.clone()));
    rig.watchers.open("p1", "w", src);

    *src.located.lock() = Located::NotYet;
    append(&path, "u:two\n");
    wait_until("the next line", || rig.sink.texts() == vec![(strings(&["two"]), false)]);
}

#[test]
fn opening_an_open_pane_starts_no_second_thread() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("once.jsonl");
    append(&path, "u:one\n");
    let src = source(Located::Found(path));

    assert_eq!(texts(&rig.watchers.open("p1", "w", src)), strings(&["one"]));
    assert_eq!(texts(&rig.watchers.open("p1", "w", src)), strings(&["one"]));
    assert_eq!(rig.watchers.running(), 1);

    rig.watchers.open("p2", "w", src);
    assert_eq!(rig.watchers.running(), 2);
}

#[test]
fn close_stops_the_thread_and_sends_nothing_more() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("close.jsonl");
    append(&path, "u:one\n");
    rig.watchers.open("p1", "w", source(Located::Found(path.clone())));
    assert_eq!(rig.watchers.running(), 1);

    rig.watchers.close("p1", "w");
    wait_until("the thread to stop", || rig.watchers.running() == 0);
    append(&path, "u:late\n");
    std::thread::sleep(Duration::from_millis(60));
    assert!(rig.sink.texts().is_empty());

    // Closing again, or a pane that was never open, is fine.
    rig.watchers.close("p1", "w");
    rig.watchers.close("never-open", "w");
}

#[test]
fn a_pane_that_is_gone_stops_its_thread_and_can_be_opened_again() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("gone.jsonl");
    append(&path, "u:one\n");
    let src = source(Located::Found(path));
    rig.watchers.open("p1", "w", src);

    *rig.panes.facts.lock() = None;
    wait_until("the thread to stop", || rig.watchers.running() == 0);
    assert!(rig.watchers.open("p1", "w", src).is_empty());

    *rig.panes.facts.lock() = Some(facts());
    assert_eq!(texts(&rig.watchers.open("p1", "w", src)), strings(&["one"]));
    wait_until("the new thread", || rig.watchers.running() == 1);
}

#[test]
fn close_all_stops_every_thread() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("all.jsonl");
    append(&path, "u:one\n");
    let src = source(Located::Found(path));
    rig.watchers.open("p1", "w", src);
    rig.watchers.open("p2", "w", src);
    assert_eq!(rig.watchers.running(), 2);

    rig.watchers.close_all();
    wait_until("every thread to stop", || rig.watchers.running() == 0);
}

#[test]
fn dropping_the_watchers_stops_the_threads() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("drop.jsonl");
    append(&path, "u:one\n");
    let alive = rig.watchers.alive.clone();
    let watchers = Watchers::with_tuning(rig.sink.clone(), rig.panes.clone(), Tuning { poll: Duration::from_millis(10), ..Tuning::default() });
    watchers.open("p1", "w", source(Located::Found(path)));
    assert_eq!(watchers.running(), 1);

    drop(watchers);
    wait_until("the thread to stop", || alive.load(Ordering::SeqCst) == 0 && rig.watchers.running() == 0);
}

#[test]
fn only_the_last_items_are_kept() {
    let rig = Rig::new(5);
    let path = rig.file("cap.jsonl");
    let lines: String = (1..=12).map(|n| format!("u:m{n}\n")).collect();
    append(&path, &lines);
    let src = source(Located::Found(path.clone()));

    let items = rig.watchers.open("p1", "w", src);
    assert_eq!(texts(&items), strings(&["m8", "m9", "m10", "m11", "m12"]));

    // One more line at the cap is an ordinary append; the oldest item falls out of the snapshot.
    append(&path, "u:m13\n");
    wait_until("the append", || rig.sink.texts() == vec![(strings(&["m13"]), false)]);
    assert_eq!(texts(&rig.watchers.open("p1", "w", src)), strings(&["m9", "m10", "m11", "m12", "m13"]));

    // A batch bigger than the cap goes out as a reset with the capped tail, not as a batch with a gap.
    let more: String = (14..=22).map(|n| format!("u:m{n}\n")).collect();
    append(&path, &more);
    wait_until("the big batch", || {
        rig.sink.texts().last() == Some(&(strings(&["m18", "m19", "m20", "m21", "m22"]), true))
    });
}

#[test]
fn the_defaults_are_400ms_1s_and_3000_items() {
    let tuning = Tuning::default();
    assert_eq!(tuning.poll, Duration::from_millis(400));
    assert_eq!(tuning.relocate, Duration::from_secs(1));
    assert_eq!(tuning.max_items, 3000);
}

#[test]
fn the_follower_asks_where_the_file_is_only_once_per_relocate_interval() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("follow.jsonl");
    append(&path, "u:one\n");
    let src = source(Located::NotYet);
    let mut follower = Follower::new(src);
    let start = Instant::now();
    let every = Duration::from_secs(1);

    assert_eq!(follower.step(&facts(), start, every), Change::Nothing);
    *src.located.lock() = Located::Found(path);
    // Asked again too soon: the new answer is not looked at yet.
    assert_eq!(follower.step(&facts(), start + Duration::from_millis(500), every), Change::Nothing);
    match follower.step(&facts(), start + Duration::from_millis(1000), every) {
        Change::Append(items) => assert_eq!(texts(&items), strings(&["one"])),
        other => panic!("{other:?}"),
    }
}

fn last_wait(rig: &Rig) -> Option<String> {
    rig.sink.waits.lock().last().and_then(|(_, text)| text.clone())
}

#[test]
fn a_pane_without_a_file_says_where_it_looks_and_since_when() {
    let rig = Rig::new(MAX_ITEMS);
    assert!(rig.watchers.open("p1", "w", source(Located::NotYet)).is_empty());
    let waiting = rig.watchers.waiting_for("p1").expect("waiting");
    let want = format!("Looking in {} for a file written after 00:00:00.", Path::new(PLACE).display());
    assert_eq!(waiting.len(), want.len(), "{waiting}");
    assert_eq!(waiting[..waiting.len() - 9], want[..want.len() - 9], "{waiting}");
    assert_eq!(rig.watchers.waiting_for("other"), None);
}

#[test]
fn the_waiting_text_follows_the_file_and_ends_when_there_are_items() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("later.jsonl");
    let src = source(Located::NotYet);
    rig.watchers.open("p1", "w", src);

    // A file named but not written yet.
    *src.located.lock() = Located::Found(path.clone());
    wait_until("the named file", || last_wait(&rig).as_deref() == Some("Waiting for later.jsonl to be written."));
    // The file exists but holds nothing readable.
    append(&path, "noise\n");
    wait_until("the empty file", || last_wait(&rig).is_some_and(|text| text.starts_with("Reading later.jsonl")));
    // Items arrive: no longer waiting.
    append(&path, "u:hi\n");
    wait_until("no more waiting", || rig.sink.waits.lock().last().is_some_and(|(_, text)| text.is_none()));
    assert_eq!(rig.watchers.waiting_for("p1"), None);
    // Each change was sent once.
    let waits = rig.sink.waits.lock().clone();
    assert_eq!(waits.len(), 3, "{waits:?}");
}

#[test]
fn several_candidate_files_are_called_out() {
    let rig = Rig::new(MAX_ITEMS);
    rig.watchers.open("p1", "w", source(Located::Ambiguous(vec![rig.file("a.jsonl"), rig.file("b.jsonl")])));
    assert!(rig.watchers.waiting_for("p1").unwrap().starts_with("Several files"));
}

#[test]
fn a_reader_two_surfaces_hold_runs_until_the_last_one_lets_go() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("held.jsonl");
    append(&path, "u:one\n");
    let src = source(Located::Found(path.clone()));
    // The cockpit and a pane view show the same pane.
    rig.watchers.open("p1", "cockpit", src);
    assert_eq!(texts(&rig.watchers.open("p1", "pane-view", src)), strings(&["one"]));
    assert_eq!(rig.watchers.running(), 1, "two holders, one reader");

    rig.watchers.close("p1", "pane-view");
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(rig.watchers.running(), 1, "the cockpit still shows it");
    append(&path, "u:two\n");
    wait_until("the line reaches the remaining surface", || rig.sink.texts() == vec![(strings(&["two"]), false)]);

    rig.watchers.close("p1", "cockpit");
    wait_until("the reader to stop", || rig.watchers.running() == 0);
}

#[test]
fn closing_twice_or_as_a_stranger_does_not_stop_the_holders_reader() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("stranger.jsonl");
    append(&path, "u:one\n");
    let src = source(Located::Found(path));
    rig.watchers.open("p1", "a", src);
    rig.watchers.open("p1", "b", src);
    rig.watchers.close("p1", "a");
    rig.watchers.close("p1", "a");
    rig.watchers.close("p1", "nobody");
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(rig.watchers.running(), 1, "b still holds it");
}

#[test]
fn a_lease_ends_by_itself_and_a_renewed_one_does_not() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("lease.jsonl");
    append(&path, "u:one\n");
    let src = source(Located::Found(path));
    let lease = || Hold::lease("phone", Duration::from_millis(120));
    rig.watchers.open("p1", lease(), src);
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(50));
        rig.watchers.open("p1", lease(), src);
        assert_eq!(rig.watchers.running(), 1, "each request renews the lease");
    }
    wait_until("the lease to run out", || rig.watchers.running() == 0);
    // Opening again starts a new reader that reads the file again.
    assert_eq!(texts(&rig.watchers.open("p1", lease(), src)), strings(&["one"]));
    assert_eq!(rig.watchers.running(), 1);
}

#[test]
fn a_surface_keeps_the_reader_after_the_phones_lease_ends_and_the_phone_keeps_it_after_the_surface_closes() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("mixed.jsonl");
    append(&path, "u:one\n");
    let src = source(Located::Found(path));
    rig.watchers.open("p1", "window", src);
    rig.watchers.open("p1", Hold::lease("phone", Duration::from_millis(60)), src);
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(rig.watchers.running(), 1, "the window outlives the lease");

    rig.watchers.open("p1", Hold::lease("phone", Duration::from_secs(30)), src);
    rig.watchers.close("p1", "window");
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(rig.watchers.running(), 1, "the phone still holds it");
    rig.watchers.close("p1", "phone");
    wait_until("the reader to stop", || rig.watchers.running() == 0);
}

#[test]
fn since_follows_the_items_of_an_open_pane() {
    let rig = Rig::new(MAX_ITEMS);
    let path = rig.file("since.jsonl");
    append(&path, "u:one\n");
    let src = source(Located::Found(path.clone()));
    assert!(rig.watchers.since("p1", None, 300).is_none(), "not open");
    rig.watchers.open("p1", "w", src);
    let first = rig.watchers.since("p1", None, 300).unwrap();
    assert_eq!(texts(&first.items), strings(&["one"]));
    append(&path, "u:two\n");
    wait_until("the new line", || rig.watchers.since("p1", Some(first.rev), 300).is_some_and(|delta| delta.items.len() == 1));
    let next = rig.watchers.since("p1", Some(first.rev), 300).unwrap();
    assert_eq!(texts(&next.items), strings(&["two"]));
    assert!(!next.reset && next.rev > first.rev);
}

#[test]
fn a_folder_under_home_is_written_with_a_tilde() {
    let home = Path::new("home").join("you");
    let other = Path::new("elsewhere").join("p");
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(tilde(&home.join("projects").join("p"), Some(&home)), format!("~{sep}projects{sep}p"));
    assert_eq!(tilde(&home, Some(&home)), "~");
    assert_eq!(tilde(&other, Some(&home)), other.display().to_string());
    assert_eq!(tilde(&home.join("x"), None), home.join("x").display().to_string());
}
