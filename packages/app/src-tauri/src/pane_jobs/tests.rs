use super::select::*;
use crate::pty::agent_proc::ProcEntry;
use std::collections::HashMap;

const NOW: u64 = 1_000_000;
/// The start time a job was listed with, and that the system reports for it now (nothing replaced it).
const SEEN: u64 = NOW - 60_000;

fn row(pid: u32, parent: u32, image: &str, age_ms: u64, command: &str) -> Row {
    Row { pid, parent, image: image.into(), started_ms: Some(NOW - age_ms), command: command.into() }
}

fn entry(pid: u32, parent: u32, image: &str) -> ProcEntry {
    ProcEntry { pid, parent, image: image.into() }
}

fn no_ports() -> HashMap<u32, Vec<u16>> {
    HashMap::new()
}

#[test]
fn a_long_lived_child_of_the_agent_is_a_job_and_the_agent_is_not() {
    let rows = [
        row(11, 10, "node.exe", 600_000, "node agent.js"),
        row(20, 11, "python.exe", 60_000, r#""C:\Python\python.exe" -m http.server 8765"#),
    ];
    let ports = HashMap::from([(20, vec![8765])]);
    let jobs = select(&rows, &ports, Some((11, None)), &[], NOW);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].pid, 20);
    assert_eq!(jobs[0].parent_pid, 11);
    assert_eq!(jobs[0].name, "python");
    assert_eq!(jobs[0].command, "python -m http.server 8765");
    assert_eq!(jobs[0].ports, vec![8765]);
    assert_eq!(jobs[0].started_at_ms, NOW - 60_000);
}

#[test]
fn processes_younger_than_five_seconds_or_of_unknown_age_are_left_out() {
    let mut unknown = row(22, 11, "node.exe", 0, "node a.js");
    unknown.started_ms = None;
    let rows = [row(20, 11, "bash.exe", 4_999, "bash -c ls"), row(21, 11, "bash.exe", 5_000, "bash -c sleep 99"), unknown];
    let jobs = select(&rows, &no_ports(), Some((11, None)), &[], NOW);
    assert_eq!(jobs.iter().map(|j| j.pid).collect::<Vec<_>>(), vec![21]);
}

#[test]
fn console_hosts_prompt_tools_and_the_mcp_shim_are_not_jobs() {
    let rows = [
        row(20, 10, "conhost.exe", 60_000, ""),
        row(21, 10, "openconsole.exe", 60_000, ""),
        row(22, 10, "oh-my-posh.exe", 60_000, ""),
        row(23, 11, "puppet-master-mcp.exe", 60_000, ""),
        row(24, 11, "node.exe", 60_000, "node server.js"),
    ];
    let jobs = select(&rows, &no_ports(), Some((11, None)), &[], NOW);
    assert_eq!(jobs.iter().map(|j| j.pid).collect::<Vec<_>>(), vec![24]);
}

#[test]
fn a_declared_helper_and_everything_under_it_are_left_out() {
    let rows = [
        row(20, 11, "node.exe", 60_000, "node C:/tools/mcp-helper-server/index.js"),
        row(21, 20, "node.exe", 60_000, "node worker.js"),
        row(22, 11, "node.exe", 60_000, "node dev.js"),
    ];
    let jobs = select(&rows, &no_ports(), Some((11, None)), &["mcp-helper-server"], NOW);
    assert_eq!(jobs.iter().map(|j| j.pid).collect::<Vec<_>>(), vec![22]);
    // With no declaration all three trees count, as two jobs (the helper's worker is under it).
    let all = select(&rows, &no_ports(), Some((11, None)), &[], NOW);
    assert_eq!(all.iter().map(|j| j.pid).collect::<Vec<_>>(), vec![20, 22]);
}

#[test]
fn what_started_with_the_agent_is_its_own_tool_servers_and_not_a_job() {
    // The agent started at NOW-600s. Its tool servers came up within seconds; a dev server much later.
    let rows = [
        row(20, 11, "cmd.exe", 599_000, "cmd /c npx -y some-tool-server"),
        row(21, 20, "node.exe", 598_000, "node some-tool-server"),
        row(22, 11, "mcp-server.exe", 595_000, "mcp-server --stdio"),
        row(30, 11, "node.exe", 300_000, "node dev.js"),
    ];
    let agent = Some((11, Some(NOW - 600_000)));
    let jobs = select(&rows, &no_ports(), agent, &[], NOW);
    assert_eq!(jobs.iter().map(|j| j.pid).collect::<Vec<_>>(), vec![30]);
    // Without the agent's start time the rule cannot apply.
    let unknown = select(&rows, &no_ports(), Some((11, None)), &[], NOW);
    assert_eq!(unknown.len(), 3);
    // A job started right after the grace window counts.
    let late = [row(40, 11, "node.exe", 600_000 - STARTUP_GRACE_MS - 1, "node dev.js")];
    assert_eq!(select(&late, &no_ports(), agent, &[], NOW).len(), 1);
}

#[test]
fn a_tree_is_one_job_with_the_port_owners_command_and_every_port() {
    // npm run dev: cmd -> node (npm) -> node (vite, listening) -> esbuild
    let rows = [
        row(20, 11, "cmd.exe", 90_000, r#"C:\Windows\system32\cmd.exe /d /s /c "npm run dev""#),
        row(21, 20, "node.exe", 90_000, "node npm-cli.js run dev"),
        row(22, 21, "node.exe", 89_000, r#""C:\Program Files\nodejs\node.exe" node_modules/vite/bin/vite.js"#),
        row(23, 22, "esbuild.exe", 88_000, "esbuild --service"),
    ];
    let ports = HashMap::from([(22, vec![5173]), (23, vec![40111])]);
    let jobs = select(&rows, &ports, Some((11, None)), &[], NOW);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].pid, 20, "stopping the top takes the whole tree");
    assert_eq!(jobs[0].ports, vec![5173, 40111]);
    assert_eq!(jobs[0].name, "node");
    assert_eq!(jobs[0].command, "node node_modules/vite/bin/vite.js");
}

#[test]
fn a_young_child_does_not_hide_its_old_parent_and_an_old_child_of_a_young_parent_stands_alone() {
    let rows = [
        row(20, 11, "bash.exe", 60_000, "bash -c watch"),
        row(21, 20, "sleep.exe", 1_000, "sleep 1"),
        row(30, 11, "bash.exe", 1_000, "bash -c build"),
        row(31, 30, "node.exe", 60_000, "node long.js"),
    ];
    let jobs = select(&rows, &no_ports(), Some((11, None)), &[], NOW);
    assert_eq!(jobs.iter().map(|j| j.pid).collect::<Vec<_>>(), vec![20, 31]);
}

#[test]
fn jobs_are_listed_oldest_first() {
    let rows = [row(30, 11, "node.exe", 10_000, "node b.js"), row(20, 11, "node.exe", 90_000, "node a.js")];
    let jobs = select(&rows, &no_ports(), Some((11, None)), &[], NOW);
    assert_eq!(jobs.iter().map(|j| j.pid).collect::<Vec<_>>(), vec![20, 30]);
}

#[test]
fn a_looping_process_table_does_not_hang_the_walk() {
    let rows = [row(20, 21, "node.exe", 60_000, "node a"), row(21, 20, "node.exe", 60_000, "node b")];
    let _ = select(&rows, &no_ports(), None, &[], NOW);
    let procs = [entry(20, 21, "a.exe"), entry(21, 20, "b.exe")];
    assert_eq!(descendant_pids(&procs, &[20]), vec![21]);
}

#[test]
fn short_command_names_the_program_and_caps_the_length() {
    assert_eq!(short_command(r#""C:\Program Files\nodejs\node.exe" vite  --port 5173"#, "node.exe"), "node vite --port 5173");
    assert_eq!(short_command("python3 -m http.server", "python3"), "python3 -m http.server");
    assert_eq!(short_command("C:\\x\\tool.EXE", "tool.exe"), "tool");
    assert_eq!(short_command("   ", "node.exe"), "node.exe", "no command line known");
    let long = format!("node {}", "a".repeat(400));
    let cut = short_command(&long, "node.exe");
    assert_eq!(cut.chars().count(), MAX_COMMAND_CHARS);
    assert!(cut.ends_with('\u{2026}'));
}

fn table() -> Vec<ProcEntry> {
    vec![
        entry(1, 0, "explorer.exe"),
        entry(10, 1, "powershell.exe"), // the pane
        entry(11, 10, "node.exe"),     // the agent
        entry(20, 11, "python.exe"),   // a job
        entry(21, 20, "python.exe"),   // under the job
        entry(30, 1, "node.exe"),      // another pane's, or the user's
        entry(31, 30, "node.exe"),
        entry(12, 10, "conhost.exe"),
        entry(40, 11, "puppet-master-mcp.exe"),
    ]
}

#[test]
fn stop_is_allowed_for_anything_below_the_agent_or_the_pane() {
    assert_eq!(authorize_stop(&table(), 10, Some(11), 20, SEEN, Some(SEEN)), Ok(()));
    assert_eq!(authorize_stop(&table(), 10, Some(11), 21, SEEN, Some(SEEN)), Ok(()));
    // The agent is not known yet: the pane's own process is the scope.
    assert_eq!(authorize_stop(&table(), 10, None, 20, SEEN, Some(SEEN)), Ok(()));
}

#[test]
fn stop_refuses_the_agent_the_pane_and_pid_zero() {
    assert_eq!(authorize_stop(&table(), 10, Some(11), 11, SEEN, Some(SEEN)), Err(Refusal::Itself));
    assert_eq!(authorize_stop(&table(), 10, Some(11), 10, SEEN, Some(SEEN)), Err(Refusal::Itself));
    assert_eq!(authorize_stop(&table(), 10, Some(11), 0, SEEN, Some(SEEN)), Err(Refusal::Itself));
}

#[test]
fn stop_refuses_a_process_outside_the_pane() {
    for pid in [1, 30, 31] {
        assert_eq!(authorize_stop(&table(), 10, Some(11), pid, SEEN, Some(SEEN)), Err(Refusal::NotInPane), "pid {pid}");
    }
}

#[test]
fn stop_refuses_plumbing_and_a_pid_that_is_gone() {
    assert_eq!(authorize_stop(&table(), 10, Some(11), 12, SEEN, Some(SEEN)), Err(Refusal::NotInPane));
    assert_eq!(authorize_stop(&table(), 10, Some(11), 40, SEEN, Some(SEEN)), Err(Refusal::NotInPane));
    assert_eq!(authorize_stop(&table(), 10, Some(11), 999, SEEN, Some(SEEN)), Err(Refusal::Gone));
    assert!(!Refusal::NotInPane.message().is_empty());
}

// ---- a reused process id (safety net) ----

/// The job python (20) ended after the card listed it, and the system gave pid 20 to a new process. The new
/// one is below the pane too (the agent started it), so the parent chain alone cannot tell: the start time does.
#[test]
fn stop_refuses_a_pid_the_system_gave_to_a_new_process_even_when_it_is_below_the_pane() {
    let later = SEEN + 90_000;
    assert_eq!(authorize_stop(&table(), 10, Some(11), 20, SEEN, Some(later)), Err(Refusal::Replaced));
    // One millisecond off is another process too: the listed start is compared exactly.
    assert_eq!(authorize_stop(&table(), 10, Some(11), 20, SEEN, Some(SEEN + 1)), Err(Refusal::Replaced));
    assert_eq!(authorize_stop(&table(), 10, Some(11), 20, SEEN, Some(SEEN)), Ok(()));
}

#[test]
fn stop_refuses_when_the_system_will_not_say_when_the_process_started_or_the_card_sent_no_start() {
    assert_eq!(authorize_stop(&table(), 10, Some(11), 20, SEEN, None), Err(Refusal::Replaced), "cannot tell: not stopped");
    assert_eq!(authorize_stop(&table(), 10, Some(11), 20, 0, Some(0)), Err(Refusal::Replaced), "a start of 0 is no start");
    assert!(!Refusal::Replaced.message().is_empty());
}

#[test]
fn a_reused_pid_that_landed_outside_the_pane_is_still_refused_as_not_in_the_pane() {
    // pid 30 belongs to another tree: the chain check answers first, whatever its start time.
    assert_eq!(authorize_stop(&table(), 10, Some(11), 30, SEEN, Some(SEEN)), Err(Refusal::NotInPane));
}

#[test]
fn the_stop_the_job_row_sends_carries_the_start_time_it_was_listed_with() {
    let rows = [row(11, 10, "node.exe", 600_000, "node agent.js"), row(20, 11, "python.exe", 60_000, "python -m http.server")];
    let jobs = select(&rows, &no_ports(), Some((11, None)), &[], NOW);
    assert_eq!(jobs[0].started_at_ms, SEEN, "the card sends this value back with the pid");
}

#[test]
fn a_job_the_agent_started_is_still_stoppable_after_the_agent_exited() {
    // The agent (11) is gone from the table but was seen; its orphan keeps the old parent pid.
    let procs = vec![entry(10, 1, "powershell.exe"), entry(20, 11, "python.exe")];
    assert_eq!(authorize_stop(&procs, 10, Some(11), 20, SEEN, Some(SEEN)), Ok(()));
    // Without that record the orphan is not provably this pane's.
    assert_eq!(authorize_stop(&procs, 10, None, 20, SEEN, Some(SEEN)), Err(Refusal::NotInPane));
}

#[cfg(windows)]
mod windows {
    use super::super::sys;

    #[test]
    fn the_os_reports_this_process_start_time_and_command_line() {
        let d = sys::details(std::process::id());
        let now = sys::now_unix_ms();
        let started = d.started_ms.expect("start time");
        assert!(started <= now && now - started < 24 * 3600 * 1000, "started {started}, now {now}");
        assert!(!d.command.is_empty(), "command line");
    }

    #[test]
    fn the_port_table_lists_a_port_this_process_listens_on() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let ports = sys::listening_ports();
        assert!(ports.get(&std::process::id()).is_some_and(|p| p.contains(&port)), "port {port} not in {ports:?}");
    }

    #[test]
    fn stop_job_kills_only_its_own_tree_and_refuses_a_stranger() {
        use crate::pty::proc_tree::test_support::*;
        let (mut child, grandchild) = spawn_child_with_grandchild();
        let me = std::process::id();
        let pane = super::super::PaneRef { id: "test-pane-jobs".into(), agent_type: "shell".into(), pid: me };
        // The child is below this test process: it is "the pane", so the child is a job of it.
        let refused = super::super::stop_job(&pane, 4, 1).unwrap_err();
        assert!(refused.contains("not run under this pane") || refused.contains("already gone"), "{refused}");
        assert!(pid_alive(child.id()), "a refusal kills nothing");
        let started = sys::details(child.id()).started_ms.expect("the child's start time");
        // A pid that a new process now holds: right pane chain, wrong start time. Nothing is killed.
        let reused = super::super::stop_job(&pane, child.id(), started - 5_000).unwrap_err();
        assert!(reused.contains("ended") && reused.contains("Nothing was stopped"), "{reused}");
        assert!(pid_alive(child.id()) && pid_alive(grandchild), "a reused pid kills nothing");
        super::super::stop_job(&pane, child.id(), started).expect("child is below the pane");
        let _ = child.wait();
        assert!(wait_until_dead(child.id()));
        assert!(wait_until_dead(grandchild), "its tree goes with it");
    }
}
