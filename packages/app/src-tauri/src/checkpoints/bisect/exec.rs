//! Run one declared test command once, in a copy folder (SPEC-n33 3.4, guard S1, S5, S6, S9).
//!
//! The command is the person's own argv (never a shell string). It starts in a copy folder under
//! `what-broke-it/copies/<8 hex>`, with stdin closed, a clean environment (no `GIT_*`, `PUPPET_MASTER*`,
//! `PM_*`), `CI=1`, and below-normal priority on Windows. A cancel flag or the time limit stops it by
//! killing only this child's process tree (by PID, never by image name). Output is kept as a head and
//! a tail, so a flood stays bounded. Raw output is returned to the caller only as a tail; it is never
//! logged here, and the caller redacts it before anything is stored.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::agent_runs::checks::counts::{self, TestCounts};
use crate::worktrees::proc::ToolEnv;

/// How often the runner checks the cancel flag and the time limit.
const POLL_MS: u64 = 50;
/// How much of a stream is kept from its start, and from its end (the summary is at the end).
const HEAD_BYTES: usize = 512 * 1024;
const TAIL_BYTES: usize = 512 * 1024;
/// The returned tail: the last lines, and at most this many bytes.
const TAIL_LINES: usize = 200;
const TAIL_MAX_BYTES: usize = 64 * 1024;
/// How long to wait for the output readers once the child is done. A grandchild that escaped the
/// kill could keep a pipe open; the run must not hang on it.
const DRAIN_WAIT: Duration = Duration::from_secs(2);
/// Environment names always removed from the child: they belong to the app, not to the test.
const ALWAYS_DROP: &[&str] = &["GIT_", "PUPPET_MASTER", "PM_"];
/// Windows `BELOW_NORMAL_PRIORITY_CLASS`.
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// What to run. `argv[0]` is looked up on `PATH` (`npm` finds `npm.cmd`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    /// The copy folder the command runs in. Must be inside `what-broke-it/copies/`.
    pub cwd: PathBuf,
    pub argv: Vec<String>,
    pub timeout_ms: u64,
    /// Extra environment name prefixes to remove from the child, on top of `ALWAYS_DROP`.
    pub env_drop_prefixes: Vec<&'static str>,
}

/// What one run produced. Only the tail of the output is kept, and it is not redacted yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    /// The command exited with code 0.
    pub exit_ok: bool,
    /// The time limit was reached and the tree was killed.
    pub timed_out: bool,
    /// The cancel flag was set and the tree was killed (or the wait on it failed).
    pub stopped: bool,
    /// The program could not be run at all; the message says why in plain words.
    pub not_started: Option<String>,
    /// Test counts from the runner's summary, when one is recognised.
    pub counts: Option<TestCounts>,
    /// The last 200 lines of stdout then stderr (at most 64 KiB). Raw: not redacted.
    pub output_tail: String,
    /// Wall time of the run in milliseconds.
    pub ms: u64,
}

/// Run `spec` once. Blocks until the command ends, the time limit passes, or `cancel` is set.
pub fn run_test(spec: &RunSpec, cancel: &AtomicBool) -> RunResult {
    let started = Instant::now();
    if cancel.load(Ordering::SeqCst) {
        return RunResult { stopped: true, ..not_run(None, started) };
    }
    let Some(program) = spec.argv.first() else {
        return not_run(Some("there is no command to run".into()), started);
    };
    let refusal = cwd_refusal(&spec.cwd);
    debug_assert!(refusal.is_none(), "run_test must only run inside a copy folder");
    if let Some(why) = refusal {
        return not_run(Some(why), started);
    }
    let Some(resolved) = ToolEnv::default().resolve(program) else {
        return not_run(Some(format!("{program} was not found on this PC")), started);
    };

    let mut command = crate::proc_util::hidden_command(&resolved);
    command
        .args(&spec.argv[1..])
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, _) in std::env::vars_os() {
        if drops(&key.to_string_lossy(), &spec.env_drop_prefixes) {
            command.env_remove(&key);
        }
    }
    command.env("CI", "1");

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return not_run(Some(format!("{program} could not start: {error}")), started),
    };
    lower_priority(&child);
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());

    let deadline = started + Duration::from_millis(spec.timeout_ms);
    let mut timed_out = false;
    let mut stopped = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => {
                // Cannot ask the child any more: stop its tree and report it as stopped, so the
                // search says "not sure" instead of blaming a turn.
                stopped = true;
                kill_tree(&mut child);
                break None;
            }
        }
        let cancelled = cancel.load(Ordering::SeqCst);
        if cancelled || Instant::now() >= deadline {
            stopped = cancelled;
            timed_out = !cancelled;
            kill_tree(&mut child);
            break None;
        }
        std::thread::sleep(Duration::from_millis(POLL_MS));
    };

    let output = format!("{}\n{}", wait_text(out), wait_text(err));
    RunResult {
        exit_ok: status.is_some_and(|s| s.success()),
        timed_out,
        stopped,
        not_started: None,
        counts: counts::parse(&output),
        output_tail: output_tail(&output),
        ms: elapsed_ms(started),
    }
}

/// The result of a run that never started (`stopped` and the rest left at their defaults).
fn not_run(not_started: Option<String>, started: Instant) -> RunResult {
    RunResult {
        exit_ok: false,
        timed_out: false,
        stopped: false,
        not_started,
        counts: None,
        output_tail: String::new(),
        ms: elapsed_ms(started),
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

/// Kill `child` and every process it started, by its PID, then reap it.
fn kill_tree(child: &mut Child) {
    crate::pty::proc_tree::kill_process_tree(child.id());
    let _ = child.wait();
}

/// True when `name` (an environment variable name) is removed from the child.
fn drops(name: &str, extra: &[&str]) -> bool {
    let upper = name.to_ascii_uppercase();
    ALWAYS_DROP.iter().any(|prefix| upper.starts_with(prefix))
        || extra.iter().any(|prefix| upper.starts_with(&prefix.to_ascii_uppercase()))
}

/// Windows only: put the child in the below-normal priority class. Set after the spawn (a plain
/// `creation_flags` call would replace the no-window flag, and the `proc_util` guard keeps flag
/// calls out of this file), so the child's first few milliseconds run at normal priority.
fn lower_priority(child: &Child) {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[link(name = "kernel32")]
        extern "system" {
            fn SetPriorityClass(process: *mut core::ffi::c_void, class: u32) -> i32;
        }
        // SAFETY: the handle belongs to `child`, which is alive and not yet reaped; the call only
        // changes that process's priority class and its result is not needed.
        unsafe {
            SetPriorityClass(child.as_raw_handle(), BELOW_NORMAL_PRIORITY_CLASS);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = child;
    }
}

/// `None` when `cwd` is a copy folder (`what-broke-it/copies/<8 hex>`, or inside one). Otherwise the
/// plain reason the run is refused. The copies folder itself is checked by name, because this module
/// does not know the app data folder; `copy.rs` makes sure only such folders are ever created.
pub fn cwd_refusal(cwd: &Path) -> Option<String> {
    let Ok(real) = cwd.canonicalize() else {
        return Some("the test folder does not exist".into());
    };
    if real.ancestors().any(is_copy_folder) {
        None
    } else {
        Some("the test command may only run in a copy folder".into())
    }
}

fn is_copy_folder(dir: &Path) -> bool {
    let name_ok = dir.file_name().and_then(|n| n.to_str()).is_some_and(is_eight_hex);
    let copies = dir.parent();
    let under_app_folder = copies.and_then(|p| p.parent());
    name_ok
        && copies.and_then(|p| p.file_name()).is_some_and(|n| n == "copies")
        && under_app_folder.and_then(|p| p.file_name()).is_some_and(|n| n == "what-broke-it")
}

fn is_eight_hex(text: &str) -> bool {
    text.len() == 8 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Keeps the first `HEAD_BYTES` of a stream and the last `TAIL_BYTES`. The middle is dropped and
/// counted, so memory stays bounded however much a test prints.
#[derive(Default)]
struct Capture {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    dropped: u64,
}

impl Capture {
    fn push(&mut self, bytes: &[u8]) {
        let room = HEAD_BYTES - self.head.len();
        let (into_head, rest) = bytes.split_at(room.min(bytes.len()));
        self.head.extend_from_slice(into_head);
        if rest.is_empty() {
            return;
        }
        self.tail.extend(rest.iter().copied());
        let extra = self.tail.len().saturating_sub(TAIL_BYTES);
        self.tail.drain(..extra);
        self.dropped += extra as u64;
    }

    fn into_text(self) -> String {
        let mut bytes = self.head;
        if self.dropped > 0 {
            bytes.extend_from_slice(format!("\n[... {} bytes not kept ...]\n", self.dropped).as_bytes());
        }
        bytes.extend(self.tail);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// Read a pipe to its end on a thread, so a chatty child never blocks on a full pipe.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut capture = Capture::default();
        if let Some(mut pipe) = pipe {
            let mut chunk = [0u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => capture.push(&chunk[..n]),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        }
        let _ = sender.send(capture.into_text());
    });
    receiver
}

fn wait_text(receiver: mpsc::Receiver<String>) -> String {
    receiver.recv_timeout(DRAIN_WAIT).unwrap_or_default()
}

/// The last `TAIL_LINES` lines, cut to `TAIL_MAX_BYTES` from the front (on a character boundary).
fn output_tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let joined = lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n");
    if joined.len() <= TAIL_MAX_BYTES {
        return joined;
    }
    let mut start = joined.len() - TAIL_MAX_BYTES;
    while !joined.is_char_boundary(start) {
        start += 1;
    }
    joined[start..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::proc_tree::test_support::wait_until_dead;
    use std::sync::Arc;

    /// Time limit for node-based runs that are expected to end on their own. Below-normal priority
    /// on a busy PC can make a trivial node start take 30 s or more, so this is generous; the
    /// time-limit test uses its own short limit.
    const NODE_LIMIT_MS: u64 = 180_000;

    /// True when `node` is on this PC. Node-based tests print a note and return when it is not.
    fn node_missing(test: &str) -> bool {
        if ToolEnv::default().resolve("node").is_some() {
            return false;
        }
        eprintln!("skipped {test}: node is not installed");
        true
    }

    /// A unique copy folder `<tmp>/pm-wb-exec-<tag>/what-broke-it/copies/c0ffee01`, removed on drop.
    struct Sandbox {
        root: PathBuf,
        copy: PathBuf,
    }

    impl Sandbox {
        fn new(tag: &str) -> Sandbox {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let root = std::env::temp_dir().join(format!("pm-wb-exec-{tag}-{}-{nanos}", std::process::id()));
            let copy = root.join("what-broke-it").join("copies").join("c0ffee01");
            std::fs::create_dir_all(&copy).expect("create copy folder");
            Sandbox { root, copy }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            // Only this test's own unique temp folder.
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn spec(cwd: &Path, argv: &[&str], timeout_ms: u64) -> RunSpec {
        RunSpec {
            cwd: cwd.to_path_buf(),
            argv: argv.iter().map(|a| (*a).to_string()).collect(),
            timeout_ms,
            env_drop_prefixes: Vec::new(),
        }
    }

    fn node_spec(cwd: &Path, script: &str, timeout_ms: u64) -> RunSpec {
        spec(cwd, &["node", "-e", script], timeout_ms)
    }

    #[test]
    fn exit_zero_is_ok() {
        if node_missing("exit_zero_is_ok") {
            return;
        }
        let sandbox = Sandbox::new("ok");
        let cancel = AtomicBool::new(false);
        let result = run_test(&node_spec(&sandbox.copy, "process.stdout.write('hello from the test')", NODE_LIMIT_MS), &cancel);
        assert!(result.exit_ok, "{result:?}");
        assert!(!result.timed_out && !result.stopped);
        assert_eq!(result.not_started, None);
        assert!(result.output_tail.contains("hello from the test"), "{result:?}");
    }

    #[test]
    fn exit_one_is_not_ok() {
        if node_missing("exit_one_is_not_ok") {
            return;
        }
        let sandbox = Sandbox::new("fail");
        let cancel = AtomicBool::new(false);
        let result = run_test(&node_spec(&sandbox.copy, "console.log('expected failure'); process.exit(1)", NODE_LIMIT_MS), &cancel);
        assert!(!result.exit_ok, "{result:?}");
        assert!(!result.timed_out && !result.stopped);
        assert_eq!(result.not_started, None);
        assert!(result.output_tail.contains("expected failure"), "{result:?}");
    }

    #[test]
    fn counts_come_from_the_runner_summary() {
        if node_missing("counts_come_from_the_runner_summary") {
            return;
        }
        let sandbox = Sandbox::new("counts");
        let cancel = AtomicBool::new(false);
        let script = "console.log('# tests 3'); console.log('# pass 2'); console.log('# fail 1'); process.exit(1)";
        let result = run_test(&node_spec(&sandbox.copy, script, NODE_LIMIT_MS), &cancel);
        assert!(!result.exit_ok);
        assert_eq!(result.counts, Some(TestCounts { passed: 2, failed: 1, skipped: 0, total: 3 }));
    }

    #[test]
    fn time_limit_kills_the_run() {
        if node_missing("time_limit_kills_the_run") {
            return;
        }
        let sandbox = Sandbox::new("timeout");
        let cancel = AtomicBool::new(false);
        let result = run_test(&node_spec(&sandbox.copy, "setTimeout(() => {}, 60000)", 400), &cancel);
        assert!(result.timed_out, "{result:?}");
        assert!(!result.exit_ok && !result.stopped);
        assert!(result.ms < 20_000, "took {} ms", result.ms);
    }

    #[test]
    fn cancel_mid_run_stops_the_run_and_its_grandchild() {
        if node_missing("cancel_mid_run_stops_the_run_and_its_grandchild") {
            return;
        }
        let sandbox = Sandbox::new("cancel");
        let pid_file = sandbox.copy.join("grandchild.pid");
        // The test starts a grandchild, writes its PID to a file, then waits.
        let script = "const fs = require('fs'); const { spawn } = require('child_process'); \
            const g = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' }); \
            fs.writeFileSync('grandchild.pid', String(g.pid)); setInterval(() => {}, 1000);";
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let watcher = std::thread::spawn(move || {
            let started = Instant::now();
            loop {
                let written = std::fs::read_to_string(&pid_file).ok().and_then(|t| t.trim().parse::<u32>().ok());
                if written.is_some() || started.elapsed() > Duration::from_millis(NODE_LIMIT_MS) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            flag.store(true, Ordering::SeqCst);
            Instant::now()
        });
        let result = run_test(&node_spec(&sandbox.copy, script, NODE_LIMIT_MS), &cancel);
        let finished = Instant::now();
        let cancelled_at = watcher.join().expect("watcher");
        assert!(result.stopped, "{result:?}");
        assert!(!result.exit_ok && !result.timed_out);
        // Latency is measured from the cancel, not from the start: the grandchild's start is slow on a busy PC.
        let stop_ms = finished.saturating_duration_since(cancelled_at).as_millis();
        assert!(stop_ms < 15_000, "stop took {stop_ms} ms after the cancel");
        let grandchild: u32 = std::fs::read_to_string(sandbox.copy.join("grandchild.pid"))
            .expect("grandchild pid file")
            .trim()
            .parse()
            .expect("grandchild pid");
        assert!(wait_until_dead(grandchild), "grandchild {grandchild} survived the stop");
    }

    #[test]
    fn cancel_before_start_runs_nothing() {
        let sandbox = Sandbox::new("precancel");
        let cancel = AtomicBool::new(true);
        let result = run_test(&spec(&sandbox.copy, &["pm-wb-exec-never-runs"], 30_000), &cancel);
        assert!(result.stopped);
        assert_eq!(result.not_started, None);
        assert!(!result.exit_ok && !result.timed_out);
    }

    #[test]
    fn missing_program_is_not_started() {
        let sandbox = Sandbox::new("missing");
        let cancel = AtomicBool::new(false);
        let result = run_test(&spec(&sandbox.copy, &["pm-wb-no-such-program-7f3a"], 30_000), &cancel);
        let message = result.not_started.expect("not started");
        assert!(message.contains("pm-wb-no-such-program-7f3a"), "{message}");
        assert!(!result.exit_ok && !result.timed_out && !result.stopped);
    }

    #[test]
    fn output_flood_stays_bounded() {
        if node_missing("output_flood_stays_bounded") {
            return;
        }
        let sandbox = Sandbox::new("flood");
        let cancel = AtomicBool::new(false);
        let script = "const line = 'x'.repeat(1000); for (let i = 0; i < 2000; i++) { process.stdout.write(line + '\\n'); } \
            process.stdout.write('the end');";
        let result = run_test(&node_spec(&sandbox.copy, script, NODE_LIMIT_MS), &cancel);
        assert!(result.exit_ok, "{result:?}");
        assert!(!result.timed_out);
        assert!(result.output_tail.len() <= TAIL_MAX_BYTES, "tail is {} bytes", result.output_tail.len());
        assert!(result.output_tail.lines().count() <= TAIL_LINES);
        assert!(result.output_tail.ends_with("the end"));
    }

    #[test]
    fn capture_keeps_only_the_head_and_the_tail() {
        let mut capture = Capture::default();
        let chunk = vec![b'a'; 64 * 1024];
        for _ in 0..48 {
            capture.push(&chunk); // 3 MiB in total
        }
        capture.push(b"END");
        let text = capture.into_text();
        assert!(text.len() <= HEAD_BYTES + TAIL_BYTES + 64, "kept {} bytes", text.len());
        assert!(text.contains("bytes not kept"));
        assert!(text.ends_with("END"));
    }

    #[test]
    fn environment_is_scrubbed_and_ci_is_set() {
        if node_missing("environment_is_scrubbed_and_ci_is_set") {
            return;
        }
        // Names are unique to this test; they are removed at the end.
        let names = [
            "PM_WB_EXEC_TEST_SCRUB",
            "GIT_WB_EXEC_TEST_SCRUB",
            "PUPPET_MASTER_WB_EXEC_TEST",
            "WB_EXEC_TEST_DROP_ME",
            "WB_EXEC_TEST_KEEP",
        ];
        for name in names {
            std::env::set_var(name, "1");
        }
        let sandbox = Sandbox::new("env");
        let cancel = AtomicBool::new(false);
        let mut run = node_spec(
            &sandbox.copy,
            "for (const n of ['PM_WB_EXEC_TEST_SCRUB', 'GIT_WB_EXEC_TEST_SCRUB', 'PUPPET_MASTER_WB_EXEC_TEST', \
             'WB_EXEC_TEST_DROP_ME', 'WB_EXEC_TEST_KEEP']) { \
             console.log(n + '=' + (process.env[n] === undefined ? 'unset' : process.env[n])); } \
             console.log('CI=' + process.env.CI);",
            NODE_LIMIT_MS,
        );
        run.env_drop_prefixes = vec!["WB_EXEC_TEST_DROP_"];
        let result = run_test(&run, &cancel);
        for name in names {
            std::env::remove_var(name);
        }
        assert!(result.exit_ok, "{result:?}");
        let lines: Vec<&str> = result.output_tail.lines().collect();
        assert!(lines.contains(&"PM_WB_EXEC_TEST_SCRUB=unset"), "{lines:?}");
        assert!(lines.contains(&"GIT_WB_EXEC_TEST_SCRUB=unset"), "{lines:?}");
        assert!(lines.contains(&"PUPPET_MASTER_WB_EXEC_TEST=unset"), "{lines:?}");
        assert!(lines.contains(&"WB_EXEC_TEST_DROP_ME=unset"), "{lines:?}");
        assert!(lines.contains(&"WB_EXEC_TEST_KEEP=1"), "{lines:?}");
        assert!(lines.contains(&"CI=1"), "{lines:?}");
    }

    #[cfg(windows)]
    #[test]
    fn child_runs_at_below_normal_priority() {
        if node_missing("child_runs_at_below_normal_priority") {
            return;
        }
        let sandbox = Sandbox::new("prio");
        let cancel = AtomicBool::new(false);
        // libuv reports BELOW_NORMAL as 10 (NORMAL is 0).
        let script = "const os = require('os'); console.log('priority=' + os.getPriority(process.pid));";
        let result = run_test(&node_spec(&sandbox.copy, script, NODE_LIMIT_MS), &cancel);
        assert!(result.exit_ok, "{result:?}");
        assert!(result.output_tail.contains("priority=10"), "{result:?}");
    }

    #[test]
    fn only_copy_folders_may_run() {
        let base = std::env::temp_dir().join(format!("pm-wb-exec-cwd-{}", std::process::id()));
        let inside = base.join("what-broke-it").join("copies").join("abcdef01");
        let nested = inside.join("src");
        std::fs::create_dir_all(&nested).expect("create nested folder");
        let short = base.join("what-broke-it").join("copies").join("abc");
        let other_name = base.join("what-broke-it").join("copies").join("project01");
        let wrong_parent = base.join("elsewhere").join("abcdef01");
        for dir in [&short, &other_name, &wrong_parent] {
            std::fs::create_dir_all(dir).expect("create folder");
        }
        assert_eq!(cwd_refusal(&inside), None);
        assert_eq!(cwd_refusal(&nested), None);
        assert!(cwd_refusal(&short).is_some(), "short name accepted");
        assert!(cwd_refusal(&other_name).is_some(), "non-hex name accepted");
        assert!(cwd_refusal(&wrong_parent).is_some(), "folder outside copies accepted");
        assert!(cwd_refusal(&base.join("no-such-folder")).is_some(), "missing folder accepted");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn drop_rule_matches_names_without_case() {
        assert!(drops("git_dir", &[]));
        assert!(drops("PUPPET_MASTER_BRIDGE", &[]));
        assert!(drops("pm_token", &[]));
        assert!(drops("wb_extra", &["WB_"]));
        assert!(!drops("PATH", &[]));
        assert!(!drops("CI", &[]));
    }
}
