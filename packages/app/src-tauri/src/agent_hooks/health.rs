//! The doctor row (§5.5): one plain sentence about an agent's hooks. Computed from the agent's
//! `[hooks]` table, the setting, what the newest spawn installed and whether the agent ever
//! reported. Words follow UX-PHASE1 §0 (plain, no ids).

use serde::Serialize;

use super::{effective_mode, last_install, requested_mode, scheme_for, InstallState, Mode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookHealthState {
    Off,
    NotSupported,
    /// Hooks are on but no pane of this agent was opened since launch.
    NotStarted,
    WaitingFolderTrust,
    WaitingVendorReview,
    BlockedSharedFile,
    Silent,
    Connected,
    UntestedVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookHealth {
    pub state: HookHealthState,
    /// The setting in force: `off`, `shadow` (always, in v0, for anything not off).
    pub mode: &'static str,
    pub sentence: String,
    /// The vendor's one-time step, when there is one (Codex `/hooks`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub how_to: Option<String>,
}

/// `None` for an agent with no hooks table at all (shells, protocol backends).
pub fn for_agent(agent: &str, label: &str, version: Option<&str>) -> Option<HookHealth> {
    let scheme = scheme_for(agent);
    let mode = effective_mode(agent);
    let row = |state, sentence: String, how_to: Option<String>| {
        Some(HookHealth { state, mode: mode.as_str(), sentence, how_to })
    };
    let Some(scheme) = scheme else {
        return row(
            HookHealthState::NotSupported,
            "This agent can't report to Puppet Master directly. Puppet Master reads its screen.".into(),
            None,
        );
    };
    if requested_mode(agent) == Mode::Off || mode == Mode::Off {
        return row(HookHealthState::Off, "Live updates are off.".into(), None);
    }
    let how_to = (!scheme.trust_screen.how_to.is_empty()).then(|| scheme.trust_screen.how_to.clone());
    if let Some(v) = version.and_then(first_version) {
        if !scheme.tested.is_empty() && !in_range(&v, &scheme.tested) {
            return row(
                HookHealthState::UntestedVersion,
                format!("Untested {label} version. Live updates stay watch only."),
                how_to,
            );
        }
    }
    if super::ledger::agent_ever_connected(agent) {
        return row(
            HookHealthState::Connected,
            format!("Connected. {label} tells Puppet Master when it starts, asks and finishes. Watch only: Puppet Master still decides from the screen."),
            None,
        );
    }
    match last_install(agent).map(|(state, _)| state) {
        None => row(
            HookHealthState::NotStarted,
            match scheme.trust.as_str() {
                "vendor_review" => format!("Turns on when you open {label} in a project. {label} then asks for your OK once per project."),
                _ => format!("Turns on the next time you open {label}."),
            },
            how_to,
        ),
        Some(InstallState::BlockedTracked) => row(
            HookHealthState::BlockedSharedFile,
            format!("This project shares its {label} settings in git, so Puppet Master doesn't change them."),
            None,
        ),
        Some(InstallState::Off) => row(HookHealthState::Off, "Live updates are off.".into(), None),
        Some(InstallState::NotSupported) => row(
            HookHealthState::NotSupported,
            "This agent can't report to Puppet Master directly. Puppet Master reads its screen.".into(),
            None,
        ),
        Some(_) => match scheme.trust.as_str() {
            "folder_trust" => row(
                HookHealthState::WaitingFolderTrust,
                format!("Waiting for you to trust this folder in {label}."),
                None,
            ),
            "vendor_review" => row(
                HookHealthState::WaitingVendorReview,
                format!("Needs your OK in {label}."),
                how_to,
            ),
            _ => row(
                HookHealthState::Silent,
                format!("{label} hasn't sent live updates yet. Its own settings may turn them off."),
                None,
            ),
        },
    }
}

/// The first `x.y.z` in a version line (`codex-cli 0.162.1`, `2.1.300 (Claude Code)`).
pub fn first_version(text: &str) -> Option<Vec<u64>> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|t| t.matches('.').count() >= 1 && t.split('.').all(|p| !p.is_empty()))
        .map(|t| t.split('.').filter_map(|p| p.parse().ok()).collect())
}

/// `tested = ">=a.b.c, <=x.y.z"` (comma-separated `>=`, `<=`, `>`, `<`, `=` clauses).
pub fn in_range(version: &[u64], range: &str) -> bool {
    range.split(',').map(str::trim).filter(|c| !c.is_empty()).all(|clause| {
        let (op, rest) = ["<=", ">=", "<", ">", "="]
            .iter()
            .find_map(|op| clause.strip_prefix(op).map(|r| (*op, r.trim())))
            .unwrap_or(("=", clause));
        let Some(bound) = first_version(rest) else { return false };
        let ord = version.cmp(&bound);
        match op {
            "<=" => ord.is_le(),
            ">=" => ord.is_ge(),
            "<" => ord.is_lt(),
            ">" => ord.is_gt(),
            _ => ord.is_eq(),
        }
    })
}
