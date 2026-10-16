//! The in-memory session store and the state machine (SPEC 2). A session is never written to disk; it
//! owns one temp work folder. Long steps do not hold the store's lock: `begin` copies the session out
//! (and marks it busy), the step runs, `end` writes the result back, unless `cancel` got there first.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use super::report::WizardError;

pub const MAX_SESSIONS: usize = 3;
pub const IDLE_LIMIT: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Resolved,
    Probed,
    Verified,
    Tested,
    Finished,
    Cancelled,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Resolved => "resolved",
            State::Probed => "probed",
            State::Verified => "verified",
            State::Tested => "tested",
            State::Finished => "finished",
            State::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramKind {
    Native,
    Shim,
}

impl ProgramKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProgramKind::Native => "native",
            ProgramKind::Shim => "shim",
        }
    }
}

/// The limits SPEC 4.2 and 5 name. Tests shrink them so a timeout test does not wait five seconds.
#[derive(Debug, Clone)]
pub struct Limits {
    /// The `--version` try when a session starts.
    pub version_timeout: Duration,
    pub probe_timeout: Duration,
    pub probe_max_bytes: usize,
    pub probe_max_runs: usize,
    pub live_timeout: Duration,
    pub live_max_bytes: usize,
    pub live_grace: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            version_timeout: Duration::from_secs(5),
            probe_timeout: Duration::from_secs(5),
            probe_max_bytes: 64 * 1024,
            probe_max_runs: 16,
            live_timeout: Duration::from_secs(90),
            live_max_bytes: 256 * 1024,
            live_grace: Duration::from_millis(1500),
        }
    }
}

/// Everything the wizard needs from the machine, so tests run against a temp tree.
#[derive(Debug, Clone)]
pub struct WizardEnv {
    /// Where `finish` writes (`<app data>/agents`).
    pub agents_dir: PathBuf,
    /// Parent of the per-session work folders.
    pub temp_root: PathBuf,
    /// Directories searched for programs; `None`: `PATH` of this process (current folder excluded).
    pub path_dirs: Option<Vec<PathBuf>>,
    /// Ids of the compiled-in agents (a draft may not reuse them).
    pub code_ids: Vec<String>,
    pub app_build: String,
    pub limits: Limits,
}

impl WizardEnv {
    pub fn app() -> Self {
        Self {
            agents_dir: crate::app_paths::agents_dir(),
            temp_root: std::env::temp_dir(),
            path_dirs: None,
            code_ids: crate::tui::registry::registry().code_ids().into_iter().map(String::from).collect(),
            app_build: env!("PM_BUILD_ID").to_string(),
            limits: Limits::default(),
        }
    }

    pub fn code_id_refs(&self) -> Vec<&str> {
        self.code_ids.iter().map(String::as_str).collect()
    }
}

/// The answers the live test asks for. Random per session and never part of the prompt, so a draft
/// cannot be written to produce them (a result regex that matches a fixed word would pass the test and
/// be useless). Anything shown to the drafter has them masked (`livetest::mask_words`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestWords {
    /// The one-word answer of the first stage (and the bad-model stage): `pong-` and six letters.
    pub one: String,
    /// The one-word answer of the resume stage.
    pub two: String,
    /// The last word of the sentence stage: "the sky is <sky>."
    pub sky: String,
}

fn six_letters() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..6).map(|_| rng.gen_range(b'a'..=b'z') as char).collect()
}

impl TestWords {
    pub fn random() -> Self {
        Self { one: format!("pong-{}", six_letters()), two: format!("pong-{}", six_letters()), sky: six_letters() }
    }

    /// The sentence the sentence stage asks for.
    pub fn sentence(&self) -> String {
        format!("the sky is {}.", self.sky)
    }

    pub fn all(&self) -> [&str; 3] {
        [&self.one, &self.two, &self.sky]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Pong,
    Sentence,
    Resume,
    BadModel,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Pong => "pong",
            Stage::Sentence => "sentence",
            Stage::Resume => "resume",
            Stage::BadModel => "bad_model",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Pong => "Answer one word",
            Stage::Sentence => "Answer a short sentence",
            Stage::Resume => "Continue the same conversation",
            Stage::BadModel => "Wrong model name is an error",
        }
    }

    /// The fixture name stem (SPEC 5.2).
    pub fn fixture_stem(self) -> &'static str {
        match self {
            Stage::Pong => "headless_success",
            Stage::Sentence => "headless_sentence",
            Stage::Resume => "headless_resume",
            Stage::BadModel => "headless_error",
        }
    }
}

/// The real output of one live-test stage (redacted, capped). Kept so a later draft can be replayed
/// against it for free.
#[derive(Debug, Clone)]
pub struct Sample {
    pub stage: Stage,
    pub args: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub exit: Option<i32>,
    /// The answer the stage asked for (empty for the bad-model stage).
    pub want: String,
    /// The session id the resume stage asked for (so a replay renders the same command).
    pub resume_session: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Draft {
    pub text: String,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct StageSummary {
    pub name: &'static str,
    pub status: &'static str,
}

/// The result of a live test that passed.
#[derive(Debug, Clone)]
pub struct Tested {
    pub version: Option<String>,
    pub stages: Vec<StageSummary>,
    pub draft_sha256: String,
}

#[derive(Debug, Clone)]
pub struct VersionInfo {
    pub ok: bool,
    pub text: String,
    pub number: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub command: String,
    pub id_suggestion: String,
    pub program: PathBuf,
    pub kind: ProgramKind,
    pub version: VersionInfo,
    pub native_candidates: Vec<String>,
    pub words: TestWords,
    pub dirs: Vec<PathBuf>,
    pub work_dir: PathBuf,
    pub state: State,
    pub touched: SystemTime,
    pub probe: Option<super::probe::ProbeReport>,
    pub draft: Option<Draft>,
    pub samples: Vec<Sample>,
    pub tested: Option<Tested>,
    pub busy: Option<&'static str>,
    pub cancel: Arc<AtomicBool>,
}

pub struct Wizard {
    sessions: Mutex<HashMap<String, Arc<Mutex<Session>>>>,
    env: Box<dyn Fn() -> WizardEnv + Send + Sync>,
}

static APP: Lazy<Wizard> = Lazy::new(|| Wizard::new(WizardEnv::app));

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

impl Wizard {
    pub fn new(env: impl Fn() -> WizardEnv + Send + Sync + 'static) -> Self {
        Self { sessions: Mutex::new(HashMap::new()), env: Box::new(env) }
    }

    /// The process-wide wizard used by the bridge route and the Tauri commands.
    pub fn app() -> &'static Wizard {
        &APP
    }

    pub fn env(&self) -> WizardEnv {
        (self.env)()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.lock().len()
    }

    fn not_found(id: &str) -> WizardError {
        WizardError::new(404, "WIZARD_SESSION_NOT_FOUND", format!("no wizard session {id}; start again"))
    }

    fn lookup(&self, id: &str) -> Result<Arc<Mutex<Session>>, WizardError> {
        self.sessions.lock().get(id).cloned().ok_or_else(|| Self::not_found(id))
    }

    /// New session id: `wz_` and 12 hex digits.
    pub fn new_id() -> String {
        format!("wz_{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
    }

    /// Drop sessions nobody touched for `IDLE_LIMIT` (and their folders), then add `session`.
    pub fn insert(&self, session: Session) -> Result<(), WizardError> {
        let mut map = self.sessions.lock();
        let stale: Vec<String> = map
            .iter()
            .filter(|(_, s)| {
                let s = s.lock();
                s.busy.is_none() && s.touched.elapsed().is_ok_and(|e| e > IDLE_LIMIT)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            if let Some(s) = map.remove(&id) {
                remove_work_dir(&s.lock().work_dir);
            }
        }
        let live = map.values().filter(|s| s.lock().state != State::Cancelled).count();
        if live >= MAX_SESSIONS {
            return Err(WizardError::new(
                409,
                "WIZARD_TOO_MANY_SESSIONS",
                format!("{MAX_SESSIONS} wizard sessions are already open; finish or discard one first"),
            ));
        }
        map.insert(session.id.clone(), Arc::new(Mutex::new(session)));
        Ok(())
    }

    /// Check the state, mark the session busy for `op` and return a copy to work on without the lock.
    pub fn begin(&self, id: &str, op: &'static str, allowed: &[State]) -> Result<Session, WizardError> {
        let slot = self.lookup(id)?;
        let mut s = slot.lock();
        if s.state == State::Cancelled {
            return Err(Self::not_found(id));
        }
        if let Some(running) = s.busy {
            return Err(WizardError::new(
                409,
                "WIZARD_BUSY",
                format!("this wizard session is busy with {running}; wait for it or cancel"),
            ));
        }
        if !allowed.contains(&s.state) {
            let wanted: Vec<&str> = allowed.iter().map(|a| a.as_str()).collect();
            return Err(WizardError::new(
                409,
                "WIZARD_BAD_STATE",
                format!("{op} needs the session to be {} but it is {}", wanted.join(" or "), s.state.as_str()),
            ));
        }
        s.busy = Some(op);
        s.touched = SystemTime::now();
        Ok(s.clone())
    }

    /// Write a step's result back and clear busy. If the session was cancelled meanwhile, the result
    /// is dropped and its folder removed.
    pub fn end<R>(&self, id: &str, apply: impl FnOnce(&mut Session) -> R) -> Result<R, WizardError> {
        let slot = self.lookup(id)?;
        let mut s = slot.lock();
        s.busy = None;
        if s.state == State::Cancelled {
            let dir = s.work_dir.clone();
            drop(s);
            remove_work_dir(&dir);
            self.sessions.lock().remove(id);
            return Err(WizardError::new(409, "WIZARD_CANCELLED", "the wizard session was cancelled"));
        }
        s.touched = SystemTime::now();
        Ok(apply(&mut s))
    }

    /// Clear busy without changing anything (a step that failed before it produced a result).
    pub fn release(&self, id: &str) {
        if let Ok(slot) = self.lookup(id) {
            let mut s = slot.lock();
            s.busy = None;
            if s.state == State::Cancelled {
                let dir = s.work_dir.clone();
                drop(s);
                remove_work_dir(&dir);
                self.sessions.lock().remove(id);
            }
        }
    }

    /// Run a quick read on a session (no state check).
    pub fn read<R>(&self, id: &str, f: impl FnOnce(&Session) -> R) -> Result<R, WizardError> {
        let slot = self.lookup(id)?;
        let s = slot.lock();
        if s.state == State::Cancelled {
            return Err(Self::not_found(id));
        }
        Ok(f(&s))
    }

    /// Idempotent. Returns `"cancelled"` or `"gone"` (nothing by that id).
    pub fn cancel(&self, id: &str) -> &'static str {
        let Ok(slot) = self.lookup(id) else { return "gone" };
        let mut s = slot.lock();
        if s.state == State::Cancelled {
            return "gone";
        }
        s.cancel.store(true, Ordering::SeqCst);
        s.state = State::Cancelled;
        let dir = s.work_dir.clone();
        let busy = s.busy.is_some();
        drop(s);
        remove_work_dir(&dir);
        if !busy {
            self.sessions.lock().remove(id);
        }
        "cancelled"
    }

    /// Test helper: pretend the session was last used `by` ago.
    #[cfg(test)]
    pub fn backdate(&self, id: &str, by: Duration) {
        if let Ok(slot) = self.lookup(id) {
            slot.lock().touched = SystemTime::now() - by;
        }
    }

    /// A finished session is dropped; its work folder goes with it.
    pub fn drop_finished(&self, id: &str) {
        if let Some(slot) = self.sessions.lock().remove(id) {
            remove_work_dir(&slot.lock().work_dir);
        }
    }
}

/// Best effort: a folder in use on Windows is retried once the step ends.
pub fn remove_work_dir(dir: &Path) {
    if dir.file_name().is_some_and(|n| n.to_string_lossy().starts_with("pm-wizard-")) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

pub fn make_work_dir(root: &Path, id: &str) -> Result<PathBuf, WizardError> {
    let dir = root.join(format!("pm-wizard-{id}"));
    std::fs::create_dir_all(&dir)
        .map_err(|e| WizardError::new(500, "WIZARD_IO", format!("cannot create the work folder: {e}")))?;
    Ok(dir)
}
