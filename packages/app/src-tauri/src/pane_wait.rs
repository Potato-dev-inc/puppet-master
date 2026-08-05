//! Long-poll worker pane wait — avoids orchestrator MCP polling loops.

use crate::opencode::status::{self, OpenCodeWaitSnapshot};
use crate::pty::registry::PaneRegistry;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitUntil {
    Idle,
    WaitingInput,
    Error,
    Gone,
    Permission,
    Unhealthy,
}

impl WaitUntil {
    pub fn parse_all(raw: &[String]) -> Result<Vec<Self>, String> {
        if raw.is_empty() {
            return Ok(default_until());
        }
        raw.iter().map(|value| Self::parse(value)).collect()
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_lowercase().as_str() {
            "idle" => Ok(Self::Idle),
            "waiting_input" | "waiting" | "input" => Ok(Self::WaitingInput),
            "error" | "errored" => Ok(Self::Error),
            "gone" | "exit" | "exited" => Ok(Self::Gone),
            "permission" | "permissions" => Ok(Self::Permission),
            "unhealthy" | "health" => Ok(Self::Unhealthy),
            other => Err(format!("unsupported wait trigger '{other}'")),
        }
    }
}

fn default_until() -> Vec<WaitUntil> {
    vec![
        WaitUntil::Idle,
        WaitUntil::WaitingInput,
        WaitUntil::Error,
        WaitUntil::Gone,
        WaitUntil::Permission,
        WaitUntil::Unhealthy,
    ]
}

#[derive(Debug, Clone, Deserialize)]
pub struct WaitForPanesRequest {
    pub pane_ids: Vec<String>,
    #[serde(default)]
    pub until: Vec<String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    120_000
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WaitForPanesResult {
    pub reason: String,
    pub pane_id: String,
    pub status: Option<String>,
    pub opencode: Option<OpenCodeWaitSnapshot>,
}

pub fn wait_for_panes(
    registry: &Arc<Mutex<PaneRegistry>>,
    req: WaitForPanesRequest,
) -> Result<WaitForPanesResult, String> {
    if req.pane_ids.is_empty() {
        return Err("pane_ids must not be empty".into());
    }
    let until = WaitUntil::parse_all(&req.until)?;
    let timeout = Duration::from_millis(req.timeout_ms.clamp(1_000, 300_000));
    let deadline = Instant::now() + timeout;
    let poll = Duration::from_millis(400);

    loop {
        for pane_id in &req.pane_ids {
            if let Some(result) = check_pane(registry, pane_id, &until) {
                return Ok(result);
            }
        }

        if Instant::now() >= deadline {
            let pane_id = req.pane_ids[0].clone();
            let status = status::pane_info(registry, &pane_id).map(|info| info.status);
            let opencode = status::wait_snapshot(registry, &pane_id).ok().flatten();
            return Ok(WaitForPanesResult {
                reason: "timeout".into(),
                pane_id,
                status,
                opencode,
            });
        }
        thread::sleep(poll);
    }
}

fn check_pane(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    until: &[WaitUntil],
) -> Option<WaitForPanesResult> {
    let info = match status::pane_info(registry, pane_id) {
        Some(info) => info,
        None if until.contains(&WaitUntil::Gone) => {
            return Some(wait_result("gone", pane_id, "error".into(), None));
        }
        None => return None,
    };
    let opencode = status::wait_snapshot(registry, pane_id).ok().flatten();

    if let Some(snapshot) = opencode.as_ref() {
        if until.contains(&WaitUntil::Unhealthy) && !snapshot.serve_healthy {
            return Some(wait_result("unhealthy", pane_id, info.status, opencode));
        }
        if until.contains(&WaitUntil::Permission) && snapshot.pending_permission_count > 0 {
            return Some(wait_result("permission", pane_id, info.status, opencode));
        }
    }

    if until.contains(&WaitUntil::Idle) && info.status == "idle" {
        return Some(wait_result("status_changed", pane_id, info.status, opencode));
    }
    if until.contains(&WaitUntil::WaitingInput) && info.status == "waiting_input" {
        return Some(wait_result("status_changed", pane_id, info.status, opencode));
    }
    if until.contains(&WaitUntil::Error) && info.status == "error" {
        return Some(wait_result("status_changed", pane_id, info.status, opencode));
    }
    if until.contains(&WaitUntil::Gone) && info.status == "error" {
        return Some(wait_result("gone", pane_id, info.status, opencode));
    }

    None
}

fn wait_result(
    reason: &str,
    pane_id: &str,
    status: String,
    opencode: Option<OpenCodeWaitSnapshot>,
) -> WaitForPanesResult {
    WaitForPanesResult {
        reason: reason.to_string(),
        pane_id: pane_id.to_string(),
        status: Some(status),
        opencode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wait_triggers() {
        assert_eq!(WaitUntil::parse("permission").unwrap(), WaitUntil::Permission);
        assert_eq!(WaitUntil::parse("waiting").unwrap(), WaitUntil::WaitingInput);
    }

    #[test]
    fn default_until_includes_permission_and_idle() {
        let triggers = default_until();
        assert!(triggers.contains(&WaitUntil::Permission));
        assert!(triggers.contains(&WaitUntil::Idle));
    }
}
