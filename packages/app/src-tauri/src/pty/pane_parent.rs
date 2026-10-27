//! Helper lineage for panes: which pane's agent started this pane over MCP.
//!
//! The link is display-only (the sidebar nests a helper under its parent). It never grants control:
//! `mcp_sessions` still decides who may touch what from the connection's own lineage. So the pane id
//! an MCP shim sends (`X-Puppet-Master-Pane`, read from `PANE_ID_ENV`) is only a hint, and it is
//! believed only when it checks out:
//!
//! - the pane it names exists and its process has not exited, and
//! - the shim's process (`X-Puppet-Master-Pid`) is a descendant of that pane's process tree, so an
//!   agent in pane A cannot say it is pane B just by setting the variable (a forged header, a shell
//!   in another pane, an MCP host outside every pane). Where the process table cannot be read, the
//!   pane existing is all that can be checked and the link is still only a label.

use super::agent_proc::ProcEntry;
use super::registry::PaneRegistry;
use parking_lot::Mutex;
use std::sync::Arc;

/// How far up the process tree to walk; also ends a malformed (cyclic) table.
const MAX_DEPTH: usize = 64;

/// A pane id sent in a header, cut down to plain characters (pane ids are UUIDs or short names).
pub fn clean_hint(raw: &str) -> Option<String> {
    let clean: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(96)
        .collect();
    (!clean.is_empty()).then_some(clean)
}

/// True when `pid` is one of `roots` or has one of them somewhere up its chain of parents.
pub fn descends_from(procs: &[ProcEntry], roots: &[u32], pid: u32) -> bool {
    let mut current = pid;
    for _ in 0..MAX_DEPTH {
        if current != 0 && roots.contains(&current) {
            return true;
        }
        match procs.iter().find(|p| p.pid == current) {
            Some(entry) if entry.parent != current => current = entry.parent,
            _ => return false,
        }
    }
    false
}

/// What the registry says about the pane a hint names.
pub struct ClaimedPane {
    /// Process ids whose tree holds the pane's agent (the pane's own process, plus a native
    /// worker's server process).
    pub roots: Vec<u32>,
    pub exited: bool,
}

/// The parent a hint earns for the new pane `child`: the hinted pane, or `None` when the hint is
/// missing, names the child itself, names a pane that is gone, or the caller's process is not
/// inside that pane. `procs` is `None` when the process table is unavailable.
pub fn accept_hint(
    hint: Option<&str>,
    child: &str,
    claimed: Option<&ClaimedPane>,
    caller_pid: Option<u32>,
    procs: Option<&[ProcEntry]>,
) -> Option<String> {
    let hint = hint?;
    let claimed = claimed?;
    if hint == child || claimed.exited {
        return None;
    }
    if let Some(procs) = procs {
        if !descends_from(procs, &claimed.roots, caller_pid?) {
            return None;
        }
    }
    Some(hint.to_string())
}

/// Look the hinted pane up in the live registry.
pub fn claimed_pane(registry: &Arc<Mutex<PaneRegistry>>, hint: &str) -> Option<ClaimedPane> {
    let reg = registry.lock();
    let pane = reg.panes.get(hint)?;
    let mut roots = vec![pane.info.pid];
    if let Some(serve) = pane.opencode.as_ref().and_then(|link| link.serve_pid()) {
        roots.push(serve);
    }
    let exited = *pane.exited.lock();
    Some(ClaimedPane { roots, exited })
}

/// Records `parent` as the helper-of pane for `child` unless it already has one (set once, kept for
/// the pane's life). True when the pane list changed.
pub fn set_parent(registry: &Arc<Mutex<PaneRegistry>>, child: &str, parent: &str) -> bool {
    let mut reg = registry.lock();
    match reg.panes.get_mut(child) {
        Some(pane) if pane.info.parent_pane_id.is_none() => {
            pane.info.parent_pane_id = Some(parent.to_string());
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
