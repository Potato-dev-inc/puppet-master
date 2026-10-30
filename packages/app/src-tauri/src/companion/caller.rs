//! Is this MCP connection a companion's? The companion's agent calls Puppet Master tools through the
//! same stdio shim as any other agent, so the app has to tell the callers apart by where they run.
//!
//! Two answers, for two different questions:
//!
//! - `verified`: the shim's process sits inside the process tree of a live companion pane
//!   (`pty::pane_parent::descends_from`), for the process id `decide` is given. From an MCP session that id is
//!   a header the caller writes, so `verified` there is not proof: a call is let through only on the id of the
//!   real TCP owner (`peer::recall_caller_pane`). Where the process table cannot be read it is `None`.
//! - `suspected`: `verified`, or the pane the connection CLAIMS to run in (`X-Puppet-Master-Pane`) is a
//!   companion pane. This is what keeps a call out (answering a helper's card, the default budget): a
//!   forged claim can only refuse the forger more, never let it do more.
//!
//! The decision is pure (`decide`), over data read from the registry, so it is tested without a terminal.

use std::sync::Arc;

use parking_lot::Mutex;

use super::service::is_pane_id;
use crate::pty::agent_proc::ProcEntry;
use crate::pty::pane_parent::descends_from;
use crate::pty::PaneRegistry;

/// One live companion pane: its id and the processes whose tree holds its agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanionPane {
    pub id: String,
    pub roots: Vec<u32>,
    pub exited: bool,
}

/// What is known about a caller.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    /// The companion pane the caller really runs in.
    pub verified: Option<String>,
    /// `verified`, or the companion pane the caller says it runs in.
    pub suspected: Option<String>,
}

/// Decides from the companion panes, the process table (`None` when unavailable), the connection's process id
/// (`None` when it never said) and the pane it claims.
pub fn decide(panes: &[CompanionPane], procs: Option<&[ProcEntry]>, pid: Option<u32>, hint: Option<&str>) -> Verdict {
    let verified = match (procs, pid) {
        (Some(procs), Some(pid)) => panes
            .iter()
            .find(|pane| !pane.exited && descends_from(procs, &pane.roots, pid))
            .map(|pane| pane.id.clone()),
        _ => None,
    };
    let claimed = hint.filter(|hint| is_pane_id(hint)).map(str::to_string);
    let suspected = verified.clone().or(claimed);
    Verdict { verified, suspected }
}

/// The live companion panes of the registry.
pub fn companion_panes(registry: &Arc<Mutex<PaneRegistry>>) -> Vec<CompanionPane> {
    let reg = registry.lock();
    reg.panes
        .iter()
        .filter(|(id, _)| is_pane_id(id))
        .map(|(id, pane)| {
            let mut roots = vec![pane.info.pid];
            if let Some(serve) = pane.opencode.as_ref().and_then(|link| link.serve_pid()) {
                roots.push(serve);
            }
            CompanionPane { id: id.clone(), roots, exited: *pane.exited.lock() }
        })
        .collect()
}

/// Every live pane of the registry (companion or not), for placing a caller in the pane whose process tree it sits in
/// (`peer::caller_pane`: the parent a helper is nested under).
pub fn live_panes(registry: &Arc<Mutex<PaneRegistry>>) -> Vec<CompanionPane> {
    let reg = registry.lock();
    reg.panes
        .iter()
        .map(|(id, pane)| {
            let mut roots = vec![pane.info.pid];
            if let Some(serve) = pane.opencode.as_ref().and_then(|link| link.serve_pid()) {
                roots.push(serve);
            }
            CompanionPane { id: id.clone(), roots, exited: *pane.exited.lock() }
        })
        .collect()
}

/// A companion's whole territory: its own pane, and every pane started under it (the `parent_pane_id` chain, so
/// its helpers and their helpers). One entry per pane of the territory, named by the companion pane at the top,
/// so a process in a helper's tree is placed in the companion's. Not for `companion_recall`: a helper is not the
/// companion and must not read its chats (`companion_panes` stays the narrow answer for that).
pub fn territory_panes(registry: &Arc<Mutex<PaneRegistry>>) -> Vec<CompanionPane> {
    let reg = registry.lock();
    let mut out = Vec::new();
    for (id, pane) in reg.panes.iter() {
        let Some(top) = companion_above(id, |pane_id| reg.panes.get(pane_id).and_then(|p| p.info.parent_pane_id.clone())) else { continue };
        let mut roots = vec![pane.info.pid];
        if let Some(serve) = pane.opencode.as_ref().and_then(|link| link.serve_pid()) {
            roots.push(serve);
        }
        out.push(CompanionPane { id: top, roots, exited: *pane.exited.lock() });
    }
    out
}

/// The companion pane at or above `pane` in the parent chain, if any. Ends on a loop.
fn companion_above(pane: &str, parent_of: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut current = pane.to_string();
    for _ in 0..16 {
        if is_pane_id(&current) {
            return Some(current);
        }
        match parent_of(&current) {
            Some(parent) if parent != current => current = parent,
            _ => return None,
        }
    }
    None
}

/// The verdict for an MCP connection, from the live registry and the process table. The process id is the one the
/// caller SENT (`X-Puppet-Master-Pid`): good enough for `suspected` (a forged claim only refuses the forger more),
/// never for letting a call through. `companion_recall` uses the real TCP owner instead (`peer::recall_caller_pane`).
pub fn of_session(registry: &Arc<Mutex<PaneRegistry>>, session: &str) -> Verdict {
    let hint = crate::mcp_sessions::claimed_session_pane(session);
    let pid = crate::mcp_sessions::session_pid(session);
    let panes = companion_panes(registry);
    // Asked of the OS only when there is a companion pane to compare with.
    let procs = if panes.is_empty() || pid.is_none() { None } else { crate::pty::agent_proc::snapshot() };
    decide(&panes, procs.as_deref(), pid, hint.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, parent: u32) -> ProcEntry {
        ProcEntry { pid, parent, image: "x.exe".into() }
    }

    fn pane(id: &str, root: u32) -> CompanionPane {
        CompanionPane { id: id.into(), roots: vec![root], exited: false }
    }

    /// pane root 100 -> agent 101 -> mcp shim 102; another pane 200 -> 201 -> shim 202; a stranger 300.
    fn table() -> Vec<ProcEntry> {
        vec![proc(100, 1), proc(101, 100), proc(102, 101), proc(200, 1), proc(201, 200), proc(202, 201), proc(300, 1)]
    }

    #[test]
    fn a_shim_inside_the_companions_pane_is_verified() {
        let panes = vec![pane("companion-mochi", 100)];
        let verdict = decide(&panes, Some(&table()), Some(102), Some("companion-mochi"));
        assert_eq!(verdict.verified.as_deref(), Some("companion-mochi"));
        assert_eq!(verdict.suspected.as_deref(), Some("companion-mochi"));
    }

    #[test]
    fn the_claim_alone_never_verifies_but_still_counts_as_suspected() {
        let panes = vec![pane("companion-mochi", 100)];
        // A shim in another pane that says it is Mochi's.
        let forged = decide(&panes, Some(&table()), Some(202), Some("companion-mochi"));
        assert_eq!(forged.verified, None);
        assert_eq!(forged.suspected.as_deref(), Some("companion-mochi"));
        // No process id, or no process table: nothing is verified (fail closed).
        assert_eq!(decide(&panes, Some(&table()), None, Some("companion-mochi")).verified, None);
        assert_eq!(decide(&panes, None, Some(102), Some("companion-mochi")).verified, None);
    }

    #[test]
    fn a_caller_that_hides_its_claim_is_still_found_by_where_it_runs() {
        let panes = vec![pane("companion-mochi", 100)];
        let verdict = decide(&panes, Some(&table()), Some(102), None);
        assert_eq!(verdict.suspected.as_deref(), Some("companion-mochi"), "no header does not hide a companion");
    }

    #[test]
    fn an_agent_in_an_ordinary_pane_is_not_a_companion() {
        let panes = vec![pane("companion-mochi", 100)];
        let verdict = decide(&panes, Some(&table()), Some(300), Some("5f3c-ordinary-pane"));
        assert_eq!(verdict, Verdict::default());
        assert_eq!(decide(&panes, Some(&table()), Some(300), None), Verdict::default());
    }

    #[test]
    fn each_companion_only_verifies_its_own_pane() {
        let panes = vec![pane("companion-mochi", 100), pane("companion-bit", 200)];
        assert_eq!(decide(&panes, Some(&table()), Some(202), Some("companion-mochi")).verified.as_deref(), Some("companion-bit"));
    }

    #[test]
    fn an_exited_pane_verifies_nobody() {
        let mut dead = pane("companion-mochi", 100);
        dead.exited = true;
        assert_eq!(decide(&[dead], Some(&table()), Some(102), None).verified, None);
    }

    #[test]
    fn the_territory_holds_the_companion_and_every_pane_under_it() {
        let registry = Arc::new(Mutex::new(PaneRegistry::new()));
        {
            let mut reg = registry.lock();
            let mut add = |id: &str, pid: u32, parent: Option<&str>| {
                let mut pane = PaneRegistry::test_pane_stub(id);
                pane.info.pid = pid;
                pane.info.parent_pane_id = parent.map(str::to_string);
                reg.panes.insert(id.to_string(), pane);
            };
            add("companion-mochi", 100, None);
            add("helper", 400, Some("companion-mochi"));
            add("grand-helper", 410, Some("helper"));
            add("ordinary", 300, None);
            add("ordinary-child", 310, Some("ordinary"));
            add("loop-a", 500, Some("loop-b"));
            add("loop-b", 510, Some("loop-a"));
        }
        let mut roots: Vec<(String, Vec<u32>)> = territory_panes(&registry).into_iter().map(|p| (p.id, p.roots)).collect();
        roots.sort();
        assert_eq!(
            roots,
            vec![
                ("companion-mochi".to_string(), vec![100]),
                ("companion-mochi".to_string(), vec![400]),
                ("companion-mochi".to_string(), vec![410]),
            ]
        );
        // The narrow answer (for recall) is only the companion's own pane.
        let narrow: Vec<Vec<u32>> = companion_panes(&registry).into_iter().map(|p| p.roots).collect();
        assert_eq!(narrow, vec![vec![100]]);
    }
}
