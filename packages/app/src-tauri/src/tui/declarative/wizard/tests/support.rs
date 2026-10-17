//! Shared test setup: the fake CLI (compiled once per test run), a temp tree and a wizard that
//! looks for programs only in the fake's folder.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};

use crate::tui::declarative::wizard::session::{Limits, Wizard, WizardEnv};

use super::fake_host::{quick_cfg, FakeHost};

pub const GOOD: &str = include_str!("drafts/good.toml");
pub const WRONG_PATH: &str = include_str!("drafts/wrong_path.toml");
pub const NO_PATH: &str = include_str!("drafts/no_path.toml");
pub const MINIMAL: &str = include_str!("drafts/minimal.toml");
pub const WRONG_BINARY: &str = include_str!("drafts/wrong_binary.toml");

/// Copies of the fake CLI under other names; the name selects odd behaviour (see `fake_cli.rs`).
pub const VARIANTS: &[&str] = &["hang-agent", "flood-agent", "secret-agent", "silent-agent", "raw-agent", "linger-agent", "oldstyle-agent", "stdin-agent", "noisy-agent", "broken-agent", "unauth-agent", "crash-agent", "git"];

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

static FAKE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// The folder holding `fake-agent` and its variants, built on first use with the `rustc` that built
/// this test (no extra crates, a second or two).
pub fn bin_dir() -> &'static Path {
    FAKE_DIR.get_or_init(|| {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake-cli/fake_cli.rs");
        let dir = std::env::temp_dir().join(format!("pm-wizard-fakecli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp folder for the fake cli");
        let rustc = std::env::var_os("CARGO")
            .map(|c| PathBuf::from(c).with_file_name(if cfg!(windows) { "rustc.exe" } else { "rustc" }))
            .filter(|p| p.is_file())
            .unwrap_or_else(|| PathBuf::from("rustc"));
        let main = dir.join(exe_name("fake-agent"));
        let status = std::process::Command::new(rustc)
            .args(["--edition", "2021", "-C", "opt-level=0", "-o"])
            .arg(&main)
            .arg(&source)
            .status()
            .expect("rustc is needed to build tests/fake-cli/fake_cli.rs");
        assert!(status.success(), "building the fake cli failed");
        for name in VARIANTS {
            std::fs::copy(&main, dir.join(exe_name(name))).expect("copy the fake cli");
        }
        // The first start of a new exe can be slow (a virus scanner looks at it); pay that here.
        let all: Vec<PathBuf> = std::iter::once("fake-agent").chain(VARIANTS.iter().copied()).map(|n| dir.join(exe_name(n))).collect();
        let starters: Vec<_> = all
            .into_iter()
            .map(|exe| std::thread::spawn(move || std::process::Command::new(exe).arg("--warmup").status()))
            .collect();
        for s in starters {
            let _ = s.join();
        }
        dir
    })
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A temp tree with an agents folder and a parent for work folders; removed on drop.
pub struct Tree {
    pub root: PathBuf,
    pub agents: PathBuf,
    pub temp: PathBuf,
}

impl Tree {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "pm-wizard-test-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let agents = root.join("agents");
        let temp = root.join("temp");
        std::fs::create_dir_all(&temp).unwrap();
        Self { root, agents, temp }
    }

    pub fn env(&self) -> WizardEnv {
        WizardEnv {
            agents_dir: self.agents.clone(),
            temp_root: self.temp.clone(),
            path_dirs: Some(vec![bin_dir().to_path_buf()]),
            code_ids: ["claude", "codex", "opencode", "opencode_native", "cursor", "cursor_agent", "cmd", "powershell", "bash"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            app_build: "test-build".into(),
            limits: Limits {
                version_timeout: Duration::from_secs(30),
                probe_timeout: Duration::from_secs(30),
                live_timeout: Duration::from_secs(90),
                live_grace: Duration::from_millis(300),
                ..Limits::default()
            },
            host: FakeHost::new(),
            visible: quick_cfg(),
        }
    }

    /// A wizard whose panes are a scripted host; the host is returned so the test can drive the screens.
    pub fn visible_wizard(&self) -> (Wizard, Arc<FakeHost>) {
        self.visible_wizard_with(|_| ())
    }

    /// `visible_wizard` with the environment changed (shorter limits, say).
    pub fn visible_wizard_with(&self, change: impl FnOnce(&mut WizardEnv)) -> (Wizard, Arc<FakeHost>) {
        let host = FakeHost::new();
        let mut env = self.env();
        env.host = host.clone();
        change(&mut env);
        (Wizard::new(move || env.clone()), host)
    }

    pub fn wizard(&self) -> Wizard {
        let env = self.env();
        Wizard::new(move || env.clone())
    }

    pub fn wizard_with(&self, change: impl Fn(&mut WizardEnv) + Send + Sync + 'static) -> Wizard {
        let mut env = self.env();
        change(&mut env);
        Wizard::new(move || env.clone())
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn start(wizard: &Wizard, command: &str) -> String {
    let out = crate::tui::declarative::wizard::api::handle(wizard, "start", &json!({"command": command})).expect("start");
    out["session"].as_str().expect("session id").to_string()
}

pub fn call(wizard: &Wizard, action: &str, body: Value) -> Result<Value, crate::tui::declarative::wizard::WizardError> {
    crate::tui::declarative::wizard::api::handle(wizard, action, &body)
}

pub fn ok(wizard: &Wizard, action: &str, body: Value) -> Value {
    call(wizard, action, body).unwrap_or_else(|e| panic!("{action}: {e}"))
}

/// start -> probe -> verify(draft) must pass; returns the session id.
pub fn verified(wizard: &Wizard, command: &str, draft: &str) -> String {
    let id = start(wizard, command);
    ok(wizard, "probe", json!({"session": id}));
    let v = ok(wizard, "verify", json!({"session": id, "toml": draft}));
    assert_eq!(v["ok"], true, "verify: {}", v["text"]);
    id
}

/// The fingerprint of the planned commands for the session's verified draft (what the UI sends back).
pub fn plan_of(wizard: &Wizard, id: &str) -> String {
    wizard
        .read(id, |s| {
            let def = crate::tui::declarative::schema::parse_definition(&s.draft.as_ref().expect("a verified draft").text).unwrap();
            crate::tui::declarative::wizard::livetest::plan(s, &def).expect("a plan").fingerprint
        })
        .unwrap()
}

/// A live-test request the way the UI sends it: confirmed, with the fingerprint of the commands it saw.
pub fn live_body(wizard: &Wizard, id: &str) -> Value {
    json!({"session": id, "confirm": true, "plan": plan_of(wizard, id)})
}

pub fn tested(wizard: &Wizard, command: &str, draft: &str) -> String {
    let id = verified(wizard, command, draft);
    let t = ok(wizard, "livetest", live_body(wizard, &id));
    assert_eq!(t["ok"], true, "livetest: {}", t["text"]);
    id
}

pub fn provenance() -> Value {
    json!({"drafter": "Test drafter", "drafter_kind": "other", "model": null, "prompt_version": "1", "rounds": 1})
}

// ---- the visible path ----------------------------------------------------------------------------

use crate::tui::declarative::wizard::session::TestWords;
use crate::tui::declarative::wizard::visible::capture::{CapFrame, CaptureSet};

pub const VISIBLE: &str = include_str!("drafts/visible_nimbus.toml");

/// The recorded visible draft, for a program named `command`.
pub fn visible_draft(command: &str) -> String {
    VISIBLE.replace("@@COMMAND@@", command)
}

/// The recorded draft as a drafter would have written it from the plain screens only: without `[tui.thinking]` and
/// `[tui.summary]`, the tables for the rows an agent prints while it works and after a turn.
pub fn visible_draft_without_work_tables(command: &str) -> String {
    let full = visible_draft(command);
    let start = full.find("# Rows the agent prints while it thinks or works").expect("the thinking tables are in the recorded draft");
    let end = full.find("[[tui.prompt]]").expect("the prompts follow");
    format!("{}{}", &full[..start], &full[end..])
}

fn frame(name: &str, text: &str) -> CapFrame {
    CapFrame { name: name.into(), text: text.into(), variants: Vec::new() }
}

/// A capture set of the made-up agent, with the session's real words in it.
pub fn capture_set(words: &TestWords) -> CaptureSet {
    use super::fake_host::screens;
    CaptureSet {
        frames: vec![
            frame("tui_trust", screens::TRUST),
            frame("tui_idle", screens::IDLE),
            frame("tui_working", &screens::working(&words.one, 0)),
            frame("tui_reply", &screens::reply(&words.one)),
            frame("tui_permission", screens::PERMISSION),
        ],
        grid: (120, 40),
        exit_method: "slash_exit".into(),
        ..CaptureSet::default()
    }
}

/// start -> probe, so a capture or a verify can run.
pub fn probed(wizard: &Wizard, command: &str) -> String {
    let id = start(wizard, command);
    ok(wizard, "probe", json!({"session": id}));
    id
}

/// Give the session a finished capture (as if the guided capture had run), built with its own words.
pub fn with_captures(wizard: &Wizard, id: &str) {
    wizard
        .read(id, |s| {
            let set = capture_set(&s.words);
            *s.visible.captures.lock() = Some(set);
        })
        .unwrap();
}

/// The words of a session (the real ones: a test may look, an AI may not).
pub fn words_of(wizard: &Wizard, id: &str) -> TestWords {
    wizard.read(id, |s| s.words.clone()).unwrap()
}

/// A draft with both halves: the recorded headless draft for the fake agent and the visible tables.
pub fn hybrid_draft() -> String {
    let tui = &VISIBLE[VISIBLE.find("[tui]").expect("the visible draft has a [tui] table")..];
    format!("{}\n{}", GOOD.trim_end(), tui)
}
