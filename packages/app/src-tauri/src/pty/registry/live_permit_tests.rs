//! Live checks of the typing permit against the REAL agent CLIs (ignored: they start `claude` /
//! `codex` and some spend a tiny turn). Each starts the agent on a ConPTY exactly like a pane (same
//! command line through the `-NoExit` PowerShell host, same environment), mirrors the output into
//! the same `vt100` emulator while counting the bytes (the pane's stream position), reads the agent
//! process with `pty::agent_proc`, and drives the real `deliver_prompt_traced` with the real
//! profile. Every write goes through the permit's own `admit` with the live byte count, as the
//! pane's write path does.
//!
//! `cargo test --lib live_permit -- --ignored --nocapture --test-threads=1`
//! Env: `PM_LIVE_CWD` (default `D:/pm-demo`), `PM_CAPTURE_DIR` (writes the screens it sees).

use super::*;
use crate::agent_runs::delivery::{self, deliver_prompt_traced, Delivery, DeliveryIo};
use crate::agent_runs::readiness::{self, HostShell, Observation, Readiness};
use crate::pty::agent_proc::{self, AgentProcess};
use crate::pty::WriteOrigin;
use crate::tui::{profile_for, ComposerState, TuiProfile};
use std::sync::atomic::{AtomicU64, Ordering};

mod send_probe;

struct LivePane {
    id: String,
    agent: &'static str,
    parser: Arc<Mutex<vt100::Parser>>,
    /// Bytes read from the PTY and processed by the emulator (the stream position).
    pos: Arc<AtomicU64>,
    writer: Box<dyn Write + Send>,
    last_output: Arc<Mutex<Instant>>,
    child: PtyChild,
    /// Bytes this harness wrote into the PTY, in order (what reached the terminal).
    written: Vec<String>,
    /// The pane status the app's PTY reader and idle watcher would hold for this screen.
    status: Arc<send_probe::LiveStatus>,
}

impl LivePane {
    #[cfg(windows)]
    fn start(agent: &'static str, kind: AgentType) -> Self {
        let cwd = std::env::var("PM_LIVE_CWD").unwrap_or_else(|_| "D:/pm-demo".into());
        Self::start_in(agent, kind, &cwd, &[])
    }

    #[cfg(windows)]
    fn start_in(agent: &'static str, kind: AgentType, cwd: &str, extra_args: &[String]) -> Self {
        let cmd = build_command(kind, cwd, extra_args, &crate::tui::LaunchEnv::default());
        let child = open_pty_child(cmd, 30, 120).expect("start the agent on a PTY");
        let parser = Arc::new(Mutex::new(vt100::Parser::new(30, 120, 1000)));
        let pos = Arc::new(AtomicU64::new(0));
        let last_output = Arc::new(Mutex::new(Instant::now()));
        let status = send_probe::LiveStatus::start(agent);
        let mut reader = child.master.try_clone_reader().expect("reader");
        {
            let (parser, pos, last_output, status) = (Arc::clone(&parser), Arc::clone(&pos), Arc::clone(&last_output), Arc::clone(&status));
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut screen = parser.lock();
                    screen.process(&buf[..n]);
                    pos.fetch_add(n as u64, Ordering::SeqCst);
                    *last_output.lock() = Instant::now();
                    status.feed(n, &screen.screen().contents());
                }
            });
        }
        let writer = child.master.take_writer().expect("writer");
        Self {
            id: format!("live-{agent}-{}", uuid::Uuid::new_v4()),
            agent,
            parser,
            pos,
            writer,
            last_output,
            child,
            written: Vec::new(),
            status,
        }
    }

    fn text(&self) -> String {
        snapshot_text(self.parser.lock().screen(), None)
    }

    fn profile_arc(&self) -> Arc<dyn TuiProfile> {
        profile_for(self.agent)
    }

    fn verdict(&mut self) -> readiness::Verdict {
        let profile = self.profile_arc();
        let observation = self.observe();
        readiness::assess(profile.as_ref(), observation)
    }

    /// Polls until `done(verdict)` or `secs` pass; returns the last verdict.
    fn wait_until(&mut self, secs: u64, done: impl Fn(&readiness::Verdict) -> bool) -> readiness::Verdict {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let verdict = self.verdict();
            if done(&verdict) || Instant::now() > deadline {
                return verdict;
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    fn capture(&self, name: &str) {
        if let Ok(dir) = std::env::var("PM_CAPTURE_DIR") {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(std::path::Path::new(&dir).join(name), self.text());
        }
    }

    fn agent_pid(&self) -> Option<u32> {
        agent_proc::snapshot()?.into_iter().find(|p| p.parent == self.child.pid).map(|p| p.pid)
    }
}

impl Drop for LivePane {
    fn drop(&mut self) {
        self.status.stop();
        crate::pty::proc_tree::kill_process_tree(self.child.pid);
        let _ = self.child.child.kill();
    }
}

impl DeliveryIo for LivePane {
    fn write(&mut self, origin: WriteOrigin<'_>, text: &str, enter: bool) -> Result<(), String> {
        // The pane's write path, with the live stream position and the live agent process.
        let (id, pid) = (self.id.clone(), self.child.pid);
        origin.admit_terminal_write(&id, text, enter, Some(self.pos.load(Ordering::SeqCst)), || agent_proc::state(&id, pid))?;
        let sequence = self.profile_arc().submit_sequence();
        let mut send = |bytes: &str, written: &mut Vec<String>| -> Result<(), String> {
            self.writer.write_all(bytes.as_bytes()).and_then(|()| self.writer.flush()).map_err(|e| e.to_string())?;
            written.push(bytes.to_string());
            Ok(())
        };
        let mut written = std::mem::take(&mut self.written);
        if !text.is_empty() {
            send(text, &mut written)?;
        }
        if enter {
            if !text.is_empty() {
                thread::sleep(super::pane_io::submit_pause(text, &sequence));
            }
            send(sequence.keys, &mut written)?;
        }
        self.written = written;
        Ok(())
    }
    fn screen(&mut self) -> String {
        self.text()
    }
    fn observe(&mut self) -> Observation {
        // Screen and position read under the same emulator lock, like `read_snapshot_full_at`.
        let (screen, pos) = {
            let parser = self.parser.lock();
            (snapshot_text(parser.screen(), None), self.pos.load(Ordering::SeqCst))
        };
        Observation {
            pane_id: self.id.clone(),
            screen,
            output_pos: Some(pos),
            agent: agent_proc::state(&self.id, self.child.pid),
        }
    }
    fn busy(&mut self) -> bool {
        self.last_output.lock().elapsed() < Duration::from_millis(500)
    }
    fn pause(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
    fn enter_settle(&mut self) -> Duration {
        self.profile_arc().submit_sequence().settle
    }
    fn profile(&mut self) -> Arc<dyn TuiProfile> {
        self.profile_arc()
    }
}

fn deliver(pane: &mut LivePane, prompt: &str) -> (Delivery, Vec<String>) {
    let clear = pane.profile_arc().clear_input_sequence();
    let mut trace = Vec::new();
    let outcome = deliver_prompt_traced(pane, clear, prompt, &mut trace);
    println!("delivery: {outcome:?}\n{}", trace.join("\n"));
    (outcome, trace)
}

/// Claude Code, zero tokens: the fresh screen reads Ready through the recogniser (no banner rule),
/// a draft typed into the box reads HasText (not Ready: Claude has no clear key), and the blank
/// pane before Claude draws is Unknown.
#[cfg(windows)]
#[test]
#[ignore = "live: starts the real claude CLI (no turn)"]
fn live_permit_claude_fresh_screen_is_ready_and_a_draft_is_not() {
    let mut pane = LivePane::start("claude", AgentType::Claude);
    let first = pane.verdict();
    println!("first look: {:?} ({})", first.readiness(), first.why());
    let ready = pane.wait_until(60, |v| v.is_ready());
    pane.capture("claude_ready_fresh_live.txt");
    println!("--- fresh screen ---\n{}", ready.screen());
    assert!(ready.is_ready(), "{:?}: {}\n{}", ready.readiness(), ready.why(), ready.screen());
    assert_eq!(ready.composer(), ComposerState::Empty);
    assert_eq!(ready.basis(), readiness::Basis::Recognised);
    // A draft in the box: typed as a person would (not a run's input).
    pane.writer.write_all(b"half a thought").unwrap();
    pane.writer.flush().unwrap();
    let draft = pane.wait_until(10, |v| v.composer() == ComposerState::HasText);
    pane.capture("claude_draft_live.txt");
    assert_eq!(draft.composer(), ComposerState::HasText, "{}", draft.screen());
    assert!(!draft.is_ready(), "a box holding someone's text is not typed into");
}

/// Claude Code, zero tokens (plain-mode live bug, r12): the follow-up typed TWICE into the box (the
/// first copy taken late) is erased by the profile's retype clear key in one press, and the box then
/// reads Empty, which is what lets delivery say "cleared". Nothing is submitted.
#[cfg(windows)]
#[test]
#[ignore = "live: starts the real claude CLI (no turn)"]
fn live_permit_claude_retype_clear_key_erases_two_copies_of_its_own_text() {
    let mut pane = LivePane::start("claude", AgentType::Claude);
    let ready = pane.wait_until(60, |v| v.is_ready());
    assert!(ready.is_ready(), "{}", ready.screen());
    let text = "Make the shop name at the top bigger, and add a small line that says Open every day.";
    for _ in 0..2 {
        pane.writer.write_all(text.as_bytes()).unwrap();
        pane.writer.flush().unwrap();
        thread::sleep(Duration::from_millis(300));
    }
    let doubled = pane.wait_until(10, |v| v.composer() == ComposerState::HasText);
    pane.capture("claude_two_copies_in_box_live.txt");
    println!("--- two copies ---\n{}", doubled.screen());
    assert_eq!(doubled.composer(), ComposerState::HasText, "{}", doubled.screen());
    // One press erases one visual line (probe, 2.1.300: two wrapped copies needed two presses), so
    // the profile's key is a run of presses; it goes in as one write, as delivery sends it.
    let clear = pane.profile_arc().retype_clear_sequence().expect("claude declares a retype clear key");
    pane.writer.write_all(clear.as_bytes()).unwrap();
    pane.writer.flush().unwrap();
    let cleared = pane.wait_until(10, |v| v.composer() == ComposerState::Empty);
    pane.capture("claude_after_retype_clear_live.txt");
    println!("--- after the clear key ---\n{}", cleared.screen());
    assert_eq!(cleared.composer(), ComposerState::Empty, "{}", cleared.screen());
    assert!(cleared.is_ready());
}

/// Claude Code, two tiny turns: a first task and a follow-up, both delivered under permits through
/// the real protocol. The follow-up is the case that stalled when readiness demanded the startup
/// screen: the recogniser must read the box after a turn.
#[cfg(windows)]
#[test]
#[ignore = "live: starts the real claude CLI and spends two tiny turns"]
fn live_permit_claude_first_turn_and_follow_up_are_delivered_under_permits() {
    let mut pane = LivePane::start("claude", AgentType::Claude);
    let (first, _) = deliver(&mut pane, "Reply with the single word KIWI. Do not use any tools.");
    assert_eq!(first, Delivery::Started, "{}", pane.text());
    // While the turn runs: Busy, never Ready.
    let working = pane.wait_until(20, |v| matches!(v.readiness(), Readiness::Busy));
    pane.capture("claude_working_live.txt");
    println!("during the turn: {:?}", working.readiness());
    let done = pane.wait_until(120, |v| v.is_ready() && v.screen().to_ascii_uppercase().contains("KIWI"));
    pane.capture("claude_ready_after_turn_live.txt");
    println!("--- after the first turn ---\n{}", done.screen());
    assert!(done.is_ready(), "{:?} {}\n{}", done.readiness(), done.why(), done.screen());
    let (second, _) = deliver(&mut pane, "Now reply with the single word LIME. Do not use any tools.");
    assert_eq!(second, Delivery::Started, "{}", pane.text());
    let done = pane.wait_until(120, |v| v.is_ready() && v.screen().to_ascii_uppercase().contains("LIME"));
    pane.capture("claude_ready_after_follow_up_live.txt");
    assert!(done.is_ready(), "{}", done.screen());
}

/// The host-shell sentinel on the real CLI, zero tokens: Claude reaches its input box, then its
/// process is killed (a crash). Delivery ends `AgentGone(AgentExited)` and NOT ONE byte reaches the
/// PowerShell that is left.
#[cfg(windows)]
#[test]
#[ignore = "live: starts the real claude CLI (no turn) and kills it"]
fn live_permit_claude_killed_before_delivery_types_zero_bytes_into_the_shell() {
    let mut pane = LivePane::start("claude", AgentType::Claude);
    let ready = pane.wait_until(60, |v| v.is_ready());
    assert!(ready.is_ready(), "{}", ready.screen());
    let agent = pane.agent_pid().expect("claude runs under its host shell");
    crate::pty::proc_tree::kill_process_tree(agent);
    let (outcome, trace) = deliver(&mut pane, "Reply with the single word PEAR. Do not use any tools.");
    pane.capture("claude_killed_host_shell_live.txt");
    assert_eq!(outcome, Delivery::AgentGone(HostShell::AgentExited), "{trace:#?}\n{}", pane.text());
    assert!(pane.written.is_empty(), "bytes typed into the shell: {:?}", pane.written);
    assert_eq!(agent_proc::state(&pane.id, pane.child.pid), AgentProcess::Exited);
}

/// Codex, one tiny turn: the permit path on the current Codex release (its recogniser was written
/// against 0.159.3; this also checks a newer one still reads Ready).
#[cfg(windows)]
#[test]
#[ignore = "live: starts the real codex CLI and spends one tiny turn"]
fn live_permit_codex_fresh_pane_is_delivered_under_a_permit() {
    let mut pane = LivePane::start("codex", AgentType::Codex);
    let (outcome, trace) = deliver(&mut pane, "Reply with the single word PLUM. Do not use any tools.");
    pane.capture("codex_after_delivery_live.txt");
    assert_eq!(outcome, Delivery::Started, "{trace:#?}\n{}", pane.text());
    let done = pane.wait_until(120, |v| v.is_ready() && v.screen().to_ascii_uppercase().contains("PLUM"));
    pane.capture("codex_ready_after_turn_live.txt");
    assert!(done.is_ready(), "{:?} {}\n{}", done.readiness(), done.why(), done.screen());
}

/// Codex, zero tokens: the fresh screen of the installed release reads Ready (or a gate it closes).
#[cfg(windows)]
#[test]
#[ignore = "live: starts the real codex CLI (no turn)"]
fn live_permit_codex_fresh_screen_reads_ready_or_a_gate() {
    let mut pane = LivePane::start("codex", AgentType::Codex);
    let verdict = pane.wait_until(60, |v| v.is_ready() || v.gate().is_some());
    pane.capture("codex_fresh_live.txt");
    println!("{:?} {}\n{}", verdict.readiness(), verdict.why(), verdict.screen());
    assert!(verdict.is_ready() || verdict.gate().is_some(), "{}", verdict.screen());
}

/// The gate adapter for a live pane: the dismissal step the outbox and every run use, over the real CLI.
#[cfg(windows)]
impl crate::bridge::startup_gate::GateIo for LivePane {
    fn screen(&mut self) -> String {
        self.text()
    }
    fn observe(&mut self) -> Observation {
        DeliveryIo::observe(self)
    }
    fn write(&mut self, permit: &readiness::GatePermit) -> Result<(), String> {
        DeliveryIo::write(self, WriteOrigin::Gate(permit), permit.keys(), false)
    }
    fn pause(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

/// Codex, zero tokens, nothing typed but the product's own fixed dismiss key: when the installed release shows its
/// "Update available" menu, the pane is a startup gate (never ready, never idle), the status heuristics do not
/// call it idle-able, and the one gate step closes it with Esc only. Skips (passes) when no menu is shown.
/// `PM_LIVE_CWD=D:/pm-tour/fresh-1 cargo test --lib live_permit_codex_update_menu -- --ignored --nocapture`
#[cfg(windows)]
#[test]
#[ignore = "live: starts the real codex CLI (no turn); presses only Esc on its update menu"]
fn live_permit_codex_update_menu_is_a_gate_that_one_esc_closes() {
    let mut pane = LivePane::start("codex", AgentType::Codex);
    let first = pane.wait_until(60, |v| v.is_ready() || v.gate().is_some());
    pane.capture("codex_update_menu_live.txt");
    println!("first: {:?} {}\n{}", first.readiness(), first.why(), first.screen());
    let Some(gate) = first.gate().filter(|gate| gate.id == "update_available") else {
        println!("no update menu on this release; nothing to check");
        return;
    };
    assert_eq!(gate.dismiss, Some("\u{1b}"));
    // Status: a gate on screen is never ready and never idle-eligible.
    assert!(!first.is_ready());
    assert_eq!(first.composer(), ComposerState::Unknown);
    assert!(crate::pty::status::startup_gate_visible("codex", first.screen()));
    assert_ne!(crate::pty::status::classify_agent_observation("codex", first.screen(), ""), crate::pty::status::PaneStatus::Idle);
    // The outbox hands it to delivery rather than waiting for ever.
    let look = crate::pane_outbox::Look::from_verdict(&first, true);
    assert_eq!(crate::pane_outbox::decide(&look), crate::pane_outbox::Step::Deliver);
    // The gate step: Esc, then the screen changes and shows the composer.
    let profile = pane.profile_arc();
    let report = crate::bridge::startup_gate::dismiss_startup_gates(&mut pane, profile.as_ref());
    println!("report: {report:?}\nwritten: {:?}", pane.written);
    pane.capture("codex_after_esc_live.txt");
    assert_eq!(report.dismissed, vec!["update_available"]);
    assert_eq!(report.blocked, None);
    assert_eq!(pane.written, vec!["\u{1b}".to_string()], "only the declared key went in");
    let after = pane.wait_until(30, |v| v.is_ready());
    assert!(after.is_ready(), "{:?} {}\n{}", after.readiness(), after.why(), after.screen());
    assert!(!crate::pty::status::startup_gate_visible("codex", after.screen()));
}
