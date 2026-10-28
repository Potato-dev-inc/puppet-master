//! Tests for `live::lookup` with a fake screen, a fake clock and a fake registry. No test reaches the internet.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::*;
use crate::package_check::model::{FetchError, Fetcher, Got, RowState, Settings};
use crate::prompt_risk::packages::extract;

const CRATE_DOC: &str = include_str!("fixtures/crate.json");
const NOW_MS: u64 = 1_800_000_000_000;
const CWD: &str = r"C:\Users\you\proj";

/// A registry that answers every crates.io call with the fixture, records each URL, and can hold every call
/// until the test lets it go (dropping the sender releases them all).
struct Fake {
    gets: Mutex<Vec<String>>,
    entered: AtomicUsize,
    gate: Option<Mutex<Receiver<()>>>,
    answer: fn(&str) -> Result<Got, FetchError>,
}

fn crate_doc(_url: &str) -> Result<Got, FetchError> {
    Ok(Got::Ok(CRATE_DOC.as_bytes().to_vec()))
}

fn unreachable(_url: &str) -> Result<Got, FetchError> {
    Err(FetchError::Unreachable)
}

fn not_found(_url: &str) -> Result<Got, FetchError> {
    Ok(Got::NotFound)
}

impl Fake {
    fn new(answer: fn(&str) -> Result<Got, FetchError>) -> Arc<Self> {
        Arc::new(Self { gets: Mutex::new(Vec::new()), entered: AtomicUsize::new(0), gate: None, answer })
    }

    fn gated(answer: fn(&str) -> Result<Got, FetchError>) -> (Arc<Self>, mpsc::Sender<()>) {
        let (tx, rx) = mpsc::channel();
        let fake = Arc::new(Self { gets: Mutex::new(Vec::new()), entered: AtomicUsize::new(0), gate: Some(Mutex::new(rx)), answer });
        (fake, tx)
    }

    fn gets(&self) -> Vec<String> {
        self.gets.lock().unwrap().clone()
    }
}

impl Fetcher for Fake {
    fn get(&self, url: &str, _accept: &str, _max_bytes: usize) -> Result<Got, FetchError> {
        self.gets.lock().unwrap().push(url.to_string());
        self.entered.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.gate {
            let _ = gate.lock().unwrap().recv();
        }
        (self.answer)(url)
    }
}

struct FakeIo {
    prompt: Option<(Value, String)>,
    fetcher: Arc<Fake>,
    lookups: bool,
    files: HashMap<PathBuf, String>,
    env: HashMap<String, String>,
}

impl FakeIo {
    fn new(command: &str, fetcher: &Arc<Fake>) -> Self {
        Self {
            prompt: Some((prompt_for(command), CWD.to_string())),
            fetcher: fetcher.clone(),
            lookups: true,
            files: HashMap::new(),
            env: HashMap::new(),
        }
    }
}

impl LiveIo for FakeIo {
    fn prompt(&self, _pane_id: &str) -> Option<(Value, String)> {
        self.prompt.clone()
    }
    fn fetcher(&self) -> Arc<dyn Fetcher> {
        self.fetcher.clone()
    }
    fn now_ms(&self) -> u64 {
        NOW_MS
    }
    fn settings(&self) -> Settings {
        Settings { lookups: self.lookups }
    }
    fn read_text(&self, p: &Path) -> Option<String> {
        self.files.get(p).cloned()
    }
    fn env(&self, k: &str) -> Option<String> {
        self.env.get(k).cloned()
    }
    fn home(&self) -> Option<PathBuf> {
        None
    }
}

fn prompt_for(command: &str) -> Value {
    json!({
        "prompt_id": "pid-1",
        "kind": "command_approval",
        "command": command,
        "packages": serde_json::to_value(extract(command, false)).unwrap(),
    })
}

fn ask(io: &FakeIo, cache: &Cache, prompt_id: &str, again: bool) -> Result<PackageCheckView, String> {
    lookup(io, cache, "pane-1", prompt_id, again)
}

#[test]
fn a_stale_prompt_id_is_an_error_and_makes_no_request() {
    let fake = Fake::new(crate_doc);
    let io = FakeIo::new("cargo add serde", &fake);
    let error = ask(&io, &Cache::new(10), "an-older-id", false).unwrap_err();
    assert_eq!(error, "STALE_PROMPT");
    assert!(fake.gets().is_empty());
}

#[test]
fn a_pane_with_no_prompt_is_stale_and_makes_no_request() {
    let fake = Fake::new(crate_doc);
    let mut io = FakeIo::new("cargo add serde", &fake);
    io.prompt = None;
    assert_eq!(ask(&io, &Cache::new(10), "pid-1", false).unwrap_err(), "STALE_PROMPT");
    assert!(fake.gets().is_empty());
}

#[test]
fn a_prompt_without_packages_gives_a_view_with_no_rows() {
    let fake = Fake::new(crate_doc);
    let mut io = FakeIo::new("cargo add serde", &fake);
    io.prompt = Some((json!({"prompt_id": "pid-1", "kind": "command_approval", "command": "cargo build"}), CWD.into()));
    let view = ask(&io, &Cache::new(10), "pid-1", false).unwrap();
    assert!(view.rows.is_empty());
    assert!(!view.can_retry);
    assert!(fake.gets().is_empty());
}

#[test]
fn lookups_off_makes_no_request_and_says_so_in_every_row() {
    let fake = Fake::new(crate_doc);
    let mut io = FakeIo::new("cargo add serde-demo other-demo", &fake);
    io.lookups = false;
    let view = ask(&io, &Cache::new(10), "pid-1", false).unwrap();
    assert!(fake.gets().is_empty());
    assert!(view.off);
    assert_eq!(view.rows.len(), 2);
    for row in &view.rows {
        assert_eq!(row.state, RowState::NotChecked);
        assert!(row.summary.contains("package lookups are off"), "{}", row.summary);
    }
    assert_eq!(view.status_text, "Lookups are off");
}

#[test]
fn a_package_on_the_projects_own_server_gets_no_request_but_the_others_do() {
    let fake = Fake::new(crate_doc);
    let mut io = FakeIo::new("npm i @acme/tools left-pad", &fake);
    io.files.insert(PathBuf::from(CWD).join(".npmrc"), "@acme:registry=https://npm.acme.test/\n".to_string());
    let view = ask(&io, &Cache::new(10), "pid-1", false).unwrap();
    let gets = fake.gets();
    assert!(!gets.is_empty(), "left-pad is still looked up");
    assert!(gets.iter().all(|u| !u.contains("acme") && !u.contains("tools")), "{gets:?}");
    assert!(gets.iter().all(|u| u.contains("left-pad")), "{gets:?}");
    assert_eq!(view.rows[0].name, "@acme/tools");
    assert_eq!(view.rows[0].state, RowState::NotChecked);
    assert_eq!(view.rows[0].summary, "Not checked: this project uses its own package server");
}

#[test]
fn an_own_server_from_the_environment_stops_every_pypi_request() {
    let fake = Fake::new(crate_doc);
    let mut io = FakeIo::new("pip install internal-demo", &fake);
    io.env.insert("PIP_INDEX_URL".into(), "https://pypi.acme.test/simple".into());
    let view = ask(&io, &Cache::new(10), "pid-1", false).unwrap();
    assert!(fake.gets().is_empty());
    assert_eq!(view.rows[0].state, RowState::NotChecked);
}

#[test]
fn nine_packages_look_up_eight_and_the_ninth_says_too_many() {
    let fake = Fake::new(crate_doc);
    let io = FakeIo::new("cargo add dem-a dem-b dem-c dem-d dem-e dem-f dem-g dem-h dem-i", &fake);
    let view = ask(&io, &Cache::new(20), "pid-1", false).unwrap();
    assert_eq!(view.rows.len(), 9);
    assert_eq!(fake.gets().len(), 8);
    assert_eq!(view.rows[8].name, "dem-i");
    assert_eq!(view.rows[8].state, RowState::NotChecked);
    assert_eq!(view.rows[8].summary, "Not checked: too many packages in one command");
    assert!(fake.gets().iter().all(|u| !u.contains("dem-i")));
}

#[test]
fn a_link_a_folder_and_a_list_file_are_not_looked_up() {
    let fake = Fake::new(crate_doc);
    let io = FakeIo::new("pip install ./local-demo -r requirements.txt git+https://github.com/someone/demo.git", &fake);
    let view = ask(&io, &Cache::new(10), "pid-1", false).unwrap();
    assert!(fake.gets().is_empty());
    assert!(view.rows.len() >= 2);
    assert!(view.rows.iter().all(|r| r.state == RowState::NotChecked));
    assert!(view.rows.iter().any(|r| r.look && r.summary.contains("github.com")), "{:?}", view.rows);
}

#[test]
fn a_name_that_looks_like_a_secret_is_counted_and_never_requested() {
    let fake = Fake::new(crate_doc);
    let io = FakeIo::new("npm i sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789", &fake);
    let view = ask(&io, &Cache::new(10), "pid-1", false).unwrap();
    assert_eq!(view.hidden, 1);
    assert!(view.rows.is_empty());
    assert!(fake.gets().is_empty());
    assert!(view.footer.contains("looked like a secret"), "{}", view.footer);
}

#[test]
fn a_found_package_gives_a_checked_row_and_one_get() {
    let fake = Fake::new(crate_doc);
    let io = FakeIo::new("cargo add pm-demo-crate", &fake);
    let view = ask(&io, &Cache::new(10), "pid-1", false).unwrap();
    assert_eq!(fake.gets().len(), 1);
    assert_eq!(view.rows[0].state, RowState::Checked);
    assert_eq!(view.status_text, "Checked just now");
    assert!(!view.can_retry);
    assert!(view.footer.starts_with("Facts from the public crates.io registry."), "{}", view.footer);
}

#[test]
fn not_found_and_failures_can_be_retried_and_check_again_asks_again() {
    let down = Fake::new(unreachable);
    let io = FakeIo::new("cargo add pm-demo-crate", &down);
    let cache = Cache::new(10);
    let first = ask(&io, &cache, "pid-1", false).unwrap();
    assert_eq!(first.rows[0].state, RowState::Failed);
    assert_eq!(first.rows[0].summary, "Could not check: crates.io could not be reached");
    assert_eq!(first.status_text, "Could not check");
    assert!(first.can_retry);
    // The failure is cached for a while: asking again without `again` makes no new request.
    ask(&io, &cache, "pid-1", false).unwrap();
    assert_eq!(down.gets().len(), 1);
    ask(&io, &cache, "pid-1", true).unwrap();
    assert_eq!(down.gets().len(), 2);

    let missing = Fake::new(not_found);
    let io = FakeIo::new("cargo add pm-demo-crate", &missing);
    let view = ask(&io, &Cache::new(10), "pid-1", false).unwrap();
    assert_eq!(view.rows[0].state, RowState::NotFound);
    assert!(view.rows[0].look);
    assert!(view.can_retry);
}

#[test]
fn a_slow_registry_gives_timeout_rows_at_the_deadline() {
    let (fake, release) = Fake::gated(crate_doc);
    let io = FakeIo::new("cargo add dem-a dem-b", &fake);
    let started = Instant::now();
    let view = lookup_within(&io, &Cache::new(10), "pane-1", "pid-1", false, 300).unwrap();
    let took = started.elapsed();
    assert!(took >= Duration::from_millis(250), "returned before the deadline: {took:?}");
    assert!(took < Duration::from_secs(5), "did not return at the deadline: {took:?}");
    assert_eq!(view.rows.len(), 2);
    for row in &view.rows {
        assert_eq!(row.state, RowState::Failed);
        assert_eq!(row.summary, "Could not check: crates.io did not answer in time");
    }
    assert_eq!(view.status_text, "Could not check");
    drop(release); // let the blocked requests end
}

#[test]
fn the_real_deadline_leaves_room_to_return_inside_ten_seconds() {
    assert!(crate::package_check::model::DEADLINE_MS + 1_000 < 10_000);
}

#[test]
fn two_concurrent_lookups_make_one_get_per_name() {
    let (fake, release) = Fake::gated(crate_doc);
    let io = FakeIo::new("cargo add pm-demo-crate", &fake);
    let cache = Cache::new(10);
    let (a, b) = std::thread::scope(|scope| {
        let first = scope.spawn(|| ask(&io, &cache, "pid-1", false));
        let wait = Instant::now();
        while fake.entered.load(Ordering::SeqCst) == 0 {
            assert!(wait.elapsed() < Duration::from_secs(5), "the first lookup never reached the registry");
            std::thread::sleep(Duration::from_millis(5));
        }
        let second = scope.spawn(|| ask(&io, &cache, "pid-1", false));
        std::thread::sleep(Duration::from_millis(200));
        drop(release);
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_eq!(fake.gets().len(), 1, "{:?}", fake.gets());
    assert_eq!(a.unwrap().rows[0].state, RowState::Checked);
    assert_eq!(b.unwrap().rows[0].state, RowState::Checked);
}

#[test]
fn a_cached_answer_makes_no_new_request() {
    let fake = Fake::new(crate_doc);
    let io = FakeIo::new("cargo add pm-demo-crate", &fake);
    let cache = Cache::new(10);
    ask(&io, &cache, "pid-1", false).unwrap();
    ask(&io, &cache, "pid-1", false).unwrap();
    assert_eq!(fake.gets().len(), 1);
}

#[test]
fn read_text_is_capped_and_missing_files_are_none() {
    let io = RegistryIo::new(Arc::new(parking_lot::Mutex::new(crate::pty::PaneRegistry::new())));
    let dir = std::env::temp_dir().join(format!("pm-pc11-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let big = dir.join("big.txt");
    std::fs::write(&big, "a".repeat(200 * 1024)).unwrap();
    assert_eq!(io.read_text(&big).unwrap().len(), 64 * 1024);
    assert!(io.read_text(&dir.join("missing.txt")).is_none());
    assert!(io.prompt("no-such-pane").is_none());
}
