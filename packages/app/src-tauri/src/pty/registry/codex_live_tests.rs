//! Live check against the real Codex CLI (not part of the normal run: it starts `codex` and spends
//! a few tokens). Run it with
//!
//! `cargo test --lib codex_live -- --ignored --nocapture`
//!
//! It starts Codex on a ConPTY exactly like a pane (same command line, same environment), feeds its
//! output to the same `vt100` emulator, and drives the same startup-gate and prompt-delivery code
//! that `run_agent` uses, with the real Codex profile. Needs a Codex install and login.

use super::*;
use crate::agent_runs::delivery::{deliver_prompt_traced, Delivery, DeliveryIo};
use crate::bridge::startup_gate::{dismiss_startup_gates, GateIo};
use crate::tui::{profile_for, ComposerState};

struct Live {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    last_output: Arc<Mutex<Instant>>,
    submit_keys: &'static str,
    settle: Duration,
}

impl Live {
    fn text(&self) -> String {
        self.parser.lock().screen().contents()
    }
    fn send(&mut self, bytes: &str) -> Result<(), String> {
        self.writer
            .write_all(bytes.as_bytes())
            .and_then(|()| self.writer.flush())
            .map_err(|e| e.to_string())
    }
}

impl GateIo for Live {
    fn screen(&mut self) -> String {
        self.text()
    }
    fn write(&mut self, keys: &str) -> Result<(), String> {
        self.send(keys)
    }
    fn pause(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

impl DeliveryIo for Live {
    fn write(&mut self, text: &str, enter: bool) -> Result<(), String> {
        if !text.is_empty() {
            self.send(text)?;
        }
        if enter {
            let keys = self.submit_keys;
            self.send(keys)?;
        }
        Ok(())
    }
    fn screen(&mut self) -> String {
        self.text()
    }
    fn busy(&mut self) -> bool {
        // Like a pane's status: anything drawn in the last half second reads as running.
        self.last_output.lock().elapsed() < Duration::from_millis(500)
    }
    fn pause(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
    fn enter_settle(&mut self) -> Duration {
        self.settle
    }
    fn composer_state(&mut self, screen: &str) -> ComposerState {
        profile_for("codex").composer_state(screen)
    }
    fn startup_gate(&mut self, screen: &str) -> Option<crate::tui::StartupGate> {
        profile_for("codex").startup_gate(screen)
    }
    fn reads_composer(&mut self) -> bool {
        profile_for("codex").reads_composer()
    }
}

/// Codex on a PTY with NOTHING waited for: the state a fresh visible run's pane is in the moment it
/// counts as ready (live: 0 ms after spawn, before Codex has drawn its update menu).
fn spawn_codex() -> (Live, PtyChild) {
    let cwd = std::env::var("PM_LIVE_CWD").unwrap_or_else(|_| "D:/pm-demo".into());
    let cmd = build_command(AgentType::Codex, &cwd, &[], &crate::tui::LaunchEnv::default());
    let mut child = open_pty_child(cmd, 30, 120).expect("start codex on a PTY");
    let parser = Arc::new(Mutex::new(vt100::Parser::new(30, 120, 1000)));
    let last_output = Arc::new(Mutex::new(Instant::now()));
    let mut reader = child.master.try_clone_reader().expect("reader");
    {
        let (parser, last_output) = (Arc::clone(&parser), Arc::clone(&last_output));
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                parser.lock().process(&buf[..n]);
                *last_output.lock() = Instant::now();
            }
        });
    }
    let sequence = profile_for("codex").submit_sequence();
    let live = Live {
        parser,
        writer: child.master.take_writer().expect("writer"),
        last_output,
        submit_keys: sequence.keys,
        settle: sequence.settle,
    };
    (live, child)
}

/// VERIFY #4 on the real CLI, the way a fresh visible run hits it: the task is delivered the moment
/// the pane is spawned, with no startup pass beforehand. Before the fix the update menu (drawn
/// about a second in) was typed over, Enter chose "Update now" and Codex quit. Run with
/// `cargo test --lib codex_live_fresh -- --ignored --nocapture` (one tiny turn; needs a pending
/// update notice to be a meaningful check, which the trace shows).
#[test]
#[ignore = "live: starts the real codex CLI and spends one tiny turn"]
fn codex_live_fresh_pane_gets_its_task_although_the_menu_draws_after_readiness() {
    let (mut live, mut child) = spawn_codex();
    let codex = profile_for("codex");
    let prompt = "Reply with the single word KUMQUAT. Do not use any tools.";
    let mut trace = Vec::new();
    let delivery = deliver_prompt_traced(&mut live, codex.clear_input_sequence(), prompt, &mut trace);
    println!("delivery: {delivery:?}\n{}", trace.join("\n"));
    let saw_menu = trace.iter().any(|line| line.contains("update_available"));
    println!("update menu met and closed: {saw_menu}");
    assert_eq!(delivery, Delivery::Started, "{}", live.text());
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        thread::sleep(Duration::from_millis(500));
        let screen = live.text();
        if screen.to_ascii_uppercase().contains("KUMQUAT") && screen.contains("Worked for") {
            println!("--- final screen ---\n{screen}");
            break;
        }
        assert!(Instant::now() < deadline, "the turn did not finish:\n{screen}");
    }
    let _ = child.child.kill();
}

/// Codex on a PTY, past its startup screens and at an empty composer.
fn start_codex() -> (Live, PtyChild) {
    let cwd = std::env::var("PM_LIVE_CWD").unwrap_or_else(|_| "D:/pm-demo".into());
    let cmd = build_command(AgentType::Codex, &cwd, &[], &crate::tui::LaunchEnv::default());
    let mut child = open_pty_child(cmd, 30, 120).expect("start codex on a PTY");
    let parser = Arc::new(Mutex::new(vt100::Parser::new(30, 120, 1000)));
    let last_output = Arc::new(Mutex::new(Instant::now()));
    let mut reader = child.master.try_clone_reader().expect("reader");
    {
        let (parser, last_output) = (Arc::clone(&parser), Arc::clone(&last_output));
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                parser.lock().process(&buf[..n]);
                *last_output.lock() = Instant::now();
            }
        });
    }
    let sequence = profile_for("codex").submit_sequence();
    let mut live = Live {
        parser,
        writer: child.master.take_writer().expect("writer"),
        last_output,
        submit_keys: sequence.keys,
        settle: sequence.settle,
    };

    // Startup: wait for Codex to draw something and hold still (the pane readiness wait).
    let started = Instant::now();
    let mut last = String::new();
    let mut stable = 0;
    while started.elapsed() < Duration::from_secs(40) && stable < 8 {
        thread::sleep(Duration::from_millis(250));
        let now = live.text();
        stable = if !now.trim().is_empty() && now == last { stable + 1 } else { 0 };
        last = now;
    }
    println!("--- screen at startup ---\n{last}\n");

    let codex = profile_for("codex");
    let gate_before = codex.startup_gate(&last);
    println!("startup gate before: {gate_before:?}");
    let report = dismiss_startup_gates(&mut live, codex.as_ref());
    println!("gate report: {report:?}");
    assert_eq!(report.blocked, None, "{}", live.text());
    thread::sleep(Duration::from_millis(800));
    let after_gate = live.text();
    println!("--- screen after the gate pass ---\n{after_gate}\n");
    assert_eq!(codex.startup_gate(&after_gate), None);
    assert_eq!(codex.composer_state(&after_gate), ComposerState::Empty, "the composer is ready");
    (live, child)
}

/// The behaviour this fix works around, on the real CLI: text, then Enter 50 ms later (what
/// `write_input` did before the profile declared a settle). If Codex ever stops reading that Enter
/// as a line break this test fails, and the settle in `tui/codex/mod.rs` can be dropped.
#[test]
#[ignore = "live: starts the real codex CLI and spends a few tokens"]
fn codex_live_an_enter_50ms_after_the_text_leaves_the_task_unsent() {
    let (mut live, mut child) = start_codex();
    let codex = profile_for("codex");
    let prompt = "Reply with the single word MANGO. Do not use any tools.";
    live.send(prompt).unwrap();
    thread::sleep(Duration::from_millis(50));
    live.send("\r").unwrap();
    thread::sleep(Duration::from_secs(4));
    let screen = live.text();
    println!("--- 4s after text + Enter(50ms) ---\n{screen}");
    let _ = child.child.kill();
    assert_eq!(codex.composer_state(&screen), ComposerState::HasText, "{screen}");
    assert!(!screen.contains("Worked for"), "no turn ran");
}

#[test]
#[ignore = "live: starts the real codex CLI and spends a few tokens"]
fn codex_live_closes_the_update_menu_and_the_task_is_submitted_by_one_enter() {
    let (mut live, mut child) = start_codex();
    let codex = profile_for("codex");
    let sequence = codex.submit_sequence();
    live.submit_keys = sequence.keys;
    live.settle = sequence.settle;
    let prompt = "Reply with the single word PINEAPPLE. Do not use any tools.";
    let mut trace = Vec::new();
    let delivery =
        deliver_prompt_traced(&mut live, codex.clear_input_sequence(), prompt, &mut trace);
    println!("delivery: {delivery:?}\n{}", trace.join("\n"));
    assert_eq!(delivery, Delivery::Started, "{}", live.text());
    assert!(
        !trace.iter().any(|line| line.contains("Enter sent once more")),
        "one Enter was enough: {trace:#?}"
    );

    // The turn runs to its end and the composer is empty again.
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        thread::sleep(Duration::from_millis(500));
        let screen = live.text();
        if screen.to_ascii_uppercase().contains("PINEAPPLE")
            && screen.contains("Worked for")
            && codex.composer_state(&screen) == ComposerState::Empty
        {
            println!("--- final screen ---\n{screen}");
            break;
        }
        assert!(Instant::now() < deadline, "the turn did not finish:\n{screen}");
    }
    let _ = child.child.kill();
}
