//! The host-shell sentinel on a REAL ConPTY with the real host command (`powershell -NoExit`), with
//! harmless stand-ins for the agent (no agent CLI, no tokens): `ping` as an agent that runs for a few
//! seconds, a killed `ping` as an agent that dies mid-run, and a command that does not exist as an
//! agent that is not installed.
//!
//! Set `PM_CAPTURE_DIR` to write the screens these tests see (the not-installed fixture under
//! `tui/shell/fixtures/` came from here).

use super::*;
use crate::pty::agent_proc::{self, AgentProcess};

struct HostPane {
    parser: Arc<Mutex<vt100::Parser>>,
    child: PtyChild,
    /// Held open: the host shell exits on end of input.
    _writer: Box<dyn Write + Send>,
}

impl HostPane {
    #[cfg(windows)]
    fn start(program: &str, args: &[&str]) -> Self {
        let cwd = std::env::temp_dir().to_string_lossy().into_owned();
        let invocation = powershell_agent_command(program, args, &[]);
        let cmd = powershell_host_command(invocation, &cwd, &crate::tui::LaunchEnv::default());
        let child = open_pty_child(cmd, 30, 120).expect("start the host shell on a PTY");
        let parser = Arc::new(Mutex::new(vt100::Parser::new(30, 120, 1000)));
        let mut reader = child.master.try_clone_reader().expect("reader");
        let sink = Arc::clone(&parser);
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock().process(&buf[..n]);
            }
        });
        let _writer = child.master.take_writer().expect("writer");
        Self { parser, child, _writer }
    }

    fn screen(&self) -> String {
        self.parser.lock().screen().contents()
    }

    /// Polls `state` until `want` or `secs` pass; returns the last state.
    fn wait_for(&self, pane: &str, want: AgentProcess, secs: u64) -> AgentProcess {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let now = agent_proc::state(pane, self.child.pid);
            if now == want || Instant::now() > deadline {
                return now;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn wait_for_screen(&self, secs: u64, done: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let screen = self.screen();
            if done(&screen) || Instant::now() > deadline {
                return screen;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for HostPane {
    fn drop(&mut self) {
        crate::pty::proc_tree::kill_process_tree(self.child.pid);
        let _ = self.child.child.kill();
    }
}

fn capture(name: &str, screen: &str) {
    if let Ok(dir) = std::env::var("PM_CAPTURE_DIR") {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(std::path::Path::new(&dir).join(name), screen);
    }
}

/// An agent that runs and exits on its own: alive while it runs, exited after, and the host shell's
/// prompt is what the screen shows then (the shape `tui::agent_gone_to_host_shell` reads).
#[cfg(windows)]
#[test]
fn the_agent_under_the_host_shell_is_tracked_until_it_exits() {
    let pane = HostPane::start("ping", &["-n", "4", "127.0.0.1"]);
    let id = "host-shell-test-exits";
    assert_eq!(pane.wait_for(id, AgentProcess::Alive, 20), AgentProcess::Alive, "{}", pane.screen());
    assert_eq!(pane.wait_for(id, AgentProcess::Exited, 20), AgentProcess::Exited, "{}", pane.screen());
    let screen = pane.wait_for_screen(10, crate::tui::shell::host_prompt_visible);
    capture("host_shell_after_agent_exit.txt", &screen);
    assert!(crate::tui::shell::host_prompt_visible(&screen), "{screen}");
    assert!(super::spawn::process_alive(pane.child.pid), "the host shell itself stays (-NoExit)");
}

/// An agent killed mid-run (a crash, a kill from outside): the process side says Exited at once,
/// whatever the screen shows yet.
#[cfg(windows)]
#[test]
fn an_agent_killed_mid_run_reads_exited() {
    let pane = HostPane::start("ping", &["-n", "60", "127.0.0.1"]);
    let id = "host-shell-test-killed";
    assert_eq!(pane.wait_for(id, AgentProcess::Alive, 20), AgentProcess::Alive, "{}", pane.screen());
    // Mid-run: it has been running (and looked at) for a while. A child that vanishes within a
    // second of first being seen reads as a shell-profile helper, not the agent (review R12 1.3).
    thread::sleep(Duration::from_millis(1_200));
    assert_eq!(pane.wait_for(id, AgentProcess::Alive, 5), AgentProcess::Alive, "{}", pane.screen());
    let agent = agent_proc::snapshot()
        .unwrap()
        .into_iter()
        .find(|p| p.parent == pane.child.pid && p.image == "ping.exe")
        .expect("the stand-in agent is the host's child");
    crate::pty::proc_tree::kill_process_tree(agent.pid);
    assert_eq!(pane.wait_for(id, AgentProcess::Exited, 10), AgentProcess::Exited);
    capture("host_shell_after_agent_killed.txt", &pane.wait_for_screen(5, crate::tui::shell::host_prompt_visible));
}

/// The agent's command does not exist (not installed, not on PATH): the shell prints its error and
/// its prompt, no agent process ever appears, and the screen says why.
#[cfg(windows)]
#[test]
fn an_agent_that_is_not_installed_never_starts_and_the_screen_says_so() {
    let pane = HostPane::start("pm-agent-not-installed-7f3", &[]);
    let id = "host-shell-test-missing";
    let screen = pane.wait_for_screen(20, crate::tui::shell::host_prompt_visible);
    capture("command_not_found_powershell_live.txt", &screen);
    assert!(crate::tui::shell::host_prompt_visible(&screen), "{screen}");
    assert_eq!(
        crate::tui::shell::command_not_found(&screen).as_deref(),
        Some("pm-agent-not-installed-7f3"),
        "{screen}"
    );
    assert_eq!(agent_proc::state(id, pane.child.pid), AgentProcess::Starting, "no agent process ever ran");
}
