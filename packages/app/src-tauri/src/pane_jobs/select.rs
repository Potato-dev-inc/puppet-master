//! The pure part of the Background card: which processes of a pane are "jobs the agent left
//! running", and may the app stop one. No OS calls here; `sys` and `mod` feed it a process table.

use crate::pty::agent_proc::{ProcEntry, NOT_AGENTS};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// A process must have lived this long to count (a tool call's shell is young).
pub const MIN_AGE_MS: u64 = 5_000;
/// Longest command text sent to the UI.
pub const MAX_COMMAND_CHARS: usize = 200;
/// A process the agent starts within this long of its own start is part of the agent's startup (its
/// tool servers, its helpers), not something it left running later.
pub const STARTUP_GRACE_MS: u64 = 10_000;
/// Puppet Master's own MCP shim: every agent starts one, it is plumbing and not a job.
const OWN_HELPERS: [&str; 1] = ["puppet-master-mcp.exe"];

/// One process under a pane, with what the OS says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub pid: u32,
    pub parent: u32,
    /// Lowercase image name (`python.exe`).
    pub image: String,
    /// When it started, milliseconds since 1970. `None` when the OS would not say.
    pub started_ms: Option<u64>,
    /// Full command line, already redacted.
    pub command: String,
}

/// A long-lived child process (with everything under it) that an agent started in a pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    /// The process to stop; its whole tree goes with it.
    pub pid: u32,
    pub parent_pid: u32,
    /// `python`, `node`: the image name without `.exe`.
    pub name: String,
    /// Short command line (the program shown by its name only), at most [`MAX_COMMAND_CHARS`].
    pub command: String,
    pub started_at_ms: u64,
    /// Ports this job (or something under it) listens on, ascending.
    pub ports: Vec<u16>,
}

/// Pids below `seeds` in the process tree (the seeds themselves are not included).
pub fn descendant_pids(procs: &[ProcEntry], seeds: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut seen: HashSet<u32> = seeds.iter().copied().collect();
    let mut queue: Vec<u32> = seeds.to_vec();
    while let Some(parent) = queue.pop() {
        for p in procs.iter().filter(|p| p.parent == parent && p.pid != parent) {
            if seen.insert(p.pid) {
                out.push(p.pid);
                queue.push(p.pid);
            }
        }
    }
    out
}

fn is_plumbing(image: &str) -> bool {
    NOT_AGENTS.contains(&image) || OWN_HELPERS.contains(&image)
}

/// Does a profile declare this process as the agent's own helper?
fn is_helper(row: &Row, markers: &[&str]) -> bool {
    if markers.is_empty() {
        return false;
    }
    let command = row.command.to_ascii_lowercase();
    markers.iter().any(|m| !m.is_empty() && (row.image.contains(m) || command.contains(m)))
}

/// Jobs among `rows` (the processes below the pane's process), oldest first.
///
/// - the agent itself, console hosts and prompt tools, Puppet Master's MCP shim, and anything a
///   profile declares as the agent's own helper (with everything under it) are left out;
/// - a process that started within [`STARTUP_GRACE_MS`] of the agent (tool servers an agent connects
///   at launch; a shell profile's helpers) is left out, with everything under it;
/// - a process younger than [`MIN_AGE_MS`] (or of unknown age) is left out: it is most likely the
///   agent's shell for a tool call that is still running;
/// - the rest is grouped into trees: one job per top process, whose ports are every port in its
///   tree, shown with the command of the process that listens (else the top's own).
pub fn select(
    rows: &[Row],
    ports: &HashMap<u32, Vec<u16>>,
    agent: Option<(u32, Option<u64>)>,
    helpers: &[&str],
    now_ms: u64,
) -> Vec<Job> {
    let (agent, agent_started) = agent.map_or((None, None), |(pid, started)| (Some(pid), started));
    let is_startup = |r: &Row| agent_started.is_some_and(|a| r.started_ms.is_some_and(|s| s < a + STARTUP_GRACE_MS));
    let by_pid: HashMap<u32, &Row> = rows.iter().map(|r| (r.pid, r)).collect();
    // A row is blocked when it or any ancestor among the rows is plumbing or a declared helper.
    let blocked = |row: &Row| -> bool {
        let mut cur = row;
        for _ in 0..64 {
            if is_plumbing(&cur.image) || is_helper(cur, helpers) || is_startup(cur) {
                return true;
            }
            match by_pid.get(&cur.parent) {
                Some(parent) if parent.pid != cur.pid => cur = parent,
                _ => return false,
            }
        }
        true
    };
    let eligible: Vec<&Row> = rows
        .iter()
        .filter(|r| Some(r.pid) != agent)
        .filter(|r| r.started_ms.is_some_and(|s| now_ms.saturating_sub(s) >= MIN_AGE_MS))
        .filter(|r| !blocked(r))
        .collect();
    let ids: HashSet<u32> = eligible.iter().map(|r| r.pid).collect();
    let mut jobs: Vec<Job> = Vec::new();
    for top in eligible.iter().filter(|r| !ids.contains(&r.parent) || r.parent == r.pid) {
        // The tree under `top` (eligible rows only), with each member's depth.
        let mut tree: Vec<(&Row, u32)> = vec![(top, 0)];
        let mut at = 0;
        while at < tree.len() && tree.len() < 512 {
            let (node, depth) = tree[at];
            at += 1;
            for child in eligible.iter().filter(|r| r.parent == node.pid && r.pid != node.pid) {
                if !tree.iter().any(|(t, _)| t.pid == child.pid) {
                    tree.push((child, depth + 1));
                }
            }
        }
        let mut found: Vec<u16> = tree.iter().flat_map(|(r, _)| ports.get(&r.pid).into_iter().flatten().copied()).collect();
        found.sort_unstable();
        found.dedup();
        let shown = tree
            .iter()
            .filter(|(r, _)| ports.get(&r.pid).is_some_and(|p| !p.is_empty()))
            .min_by_key(|(_, depth)| *depth)
            .map_or(*top, |(r, _)| *r);
        jobs.push(Job {
            pid: top.pid,
            parent_pid: top.parent,
            name: shown.image.strip_suffix(".exe").unwrap_or(&shown.image).to_string(),
            command: short_command(&shown.command, &shown.image),
            started_at_ms: top.started_ms.unwrap_or(0),
            ports: found,
        });
    }
    jobs.sort_by_key(|j| (j.started_at_ms, j.pid));
    jobs
}

/// A command line for a narrow card: the program by its file name, arguments as typed, spaces
/// collapsed, at most [`MAX_COMMAND_CHARS`] characters. Falls back to the image name.
pub fn short_command(command: &str, image: &str) -> String {
    let command = command.trim();
    if command.is_empty() {
        return image.to_string();
    }
    let (program, rest) = match command.strip_prefix('"') {
        Some(after) => match after.find('"') {
            Some(end) => (&after[..end], &after[end + 1..]),
            None => (after, ""),
        },
        None => command.split_once(char::is_whitespace).unwrap_or((command, "")),
    };
    let file = program.rsplit(['\\', '/']).next().unwrap_or(program);
    let file = match file.get(file.len().saturating_sub(4)..) {
        Some(tail) if tail.eq_ignore_ascii_case(".exe") => &file[..file.len() - 4],
        _ => file,
    };
    let joined = std::iter::once(file).chain(rest.split_whitespace()).collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= MAX_COMMAND_CHARS {
        return joined;
    }
    let mut cut: String = joined.chars().take(MAX_COMMAND_CHARS - 1).collect();
    cut.push('\u{2026}');
    cut
}

/// Why a stop request was refused (the text is shown to the person).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Itself,
    Gone,
    NotInPane,
}

impl Refusal {
    pub fn message(&self) -> &'static str {
        match self {
            Refusal::Itself => "That is the agent itself, not a background job.",
            Refusal::Gone => "That job is already gone.",
            Refusal::NotInPane => "That process does not run under this pane, so it was not stopped.",
        }
    }
}

/// May `pid` be stopped for this pane, judged on a fresh process table? Only a process that is
/// right now below the pane's process (or below its agent, which may have outlived the shell
/// record) and is neither of them.
pub fn authorize_stop(procs: &[ProcEntry], root: u32, agent: Option<u32>, pid: u32) -> Result<(), Refusal> {
    if pid == 0 || pid == root || Some(pid) == agent {
        return Err(Refusal::Itself);
    }
    let Some(entry) = procs.iter().find(|p| p.pid == pid) else {
        return Err(Refusal::Gone);
    };
    if is_plumbing(&entry.image) {
        return Err(Refusal::NotInPane);
    }
    let mut seeds = vec![root];
    seeds.extend(agent);
    if descendant_pids(procs, &seeds).contains(&pid) {
        Ok(())
    } else {
        Err(Refusal::NotInPane)
    }
}
