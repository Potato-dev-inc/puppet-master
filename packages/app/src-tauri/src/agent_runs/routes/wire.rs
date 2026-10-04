//! Request bodies of the `/agents/*` routes and the query-string helpers they share.

use crate::agent_runs::runtime::{self};
use crate::operations::{OperationError, OperationStatus};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub(super) struct WaitAgentsRequest {
    #[serde(default)]
    pub(super) project_path: Option<String>,
    pub(super) handles: Vec<String>,
    #[serde(default)]
    pub(super) after_revisions: HashMap<String, u64>,
    #[serde(default)]
    pub(super) after_cursor: Option<u64>,
    #[serde(default)]
    pub(super) until: Option<Vec<OperationStatus>>,
    #[serde(default = "default_wait_timeout", alias = "timeout_ms")]
    pub(super) wait_ms: u64,
    #[serde(default)]
    pub(super) mode: Option<String>,
    #[serde(default)]
    pub(super) wake_on_progress: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct SendAgentRequest {
    #[serde(default)]
    pub(super) project_path: Option<String>,
    pub(super) handle: String,
    #[serde(default)]
    pub(super) task: Option<String>,
    #[serde(default)]
    pub(super) message: Option<String>,
    #[serde(default)]
    pub(super) delivery: Option<String>,
    #[serde(default)]
    pub(super) idempotency_key: Option<String>,
    #[serde(default)]
    pub(super) context_policy: Option<crate::operations::ContextPolicy>,
    #[serde(default)]
    pub(super) selected_history: Option<Vec<String>>,
    #[serde(default)]
    pub(super) model: Option<String>,
    #[serde(default)]
    pub(super) reasoning: Option<String>,
    #[serde(default)]
    pub(super) scope: Option<String>,
    /// Same meaning as on run_agent: how long this call waits for the turn, whether it returns
    /// immediately, and the acceptance gate for this turn.
    #[serde(default)]
    pub(super) wait_ms: Option<u64>,
    #[serde(default)]
    pub(super) background: bool,
    #[serde(default)]
    pub(super) acceptance_criteria: Option<Vec<String>>,
    #[serde(default)]
    pub(super) checks: Option<Vec<crate::agent_runs::checks::Check>>,
    #[serde(default)]
    pub(super) locks: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AgentHandleRequest {
    #[serde(default)]
    pub(super) project_path: Option<String>,
    pub(super) handle: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct AnswerPromptRequest {
    #[serde(default)]
    pub(super) project_path: Option<String>,
    #[serde(default)]
    pub(super) handle: Option<String>,
    #[serde(default)]
    pub(super) pane_id: Option<String>,
    pub(super) prompt_id: String,
    pub(super) choice: String,
    #[serde(default)]
    pub(super) allow_broad: bool,
}

fn default_wait_timeout() -> u64 {
    120_000
}

pub(super) fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, OperationError> {
    let mut value: Value = serde_json::from_slice(body)
        .map_err(|error| OperationError::new("INVALID_JSON", error.to_string(), false))?;
    runtime::normalize_context_policy_fields(&mut value)?;
    serde_json::from_value(value)
        .map_err(|error| OperationError::new("INVALID_JSON", error.to_string(), false))
}

pub(super) fn project_path(
    requested: Option<&str>,
    query: &str,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
) -> String {
    requested
        .filter(|path| !path.trim().is_empty())
        .map(str::to_string)
        .or_else(|| query_value(query, "project_path"))
        .unwrap_or_else(|| registry.lock().project_path.clone())
}

pub(super) fn query_value(query: &str, key: &str) -> Option<String> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| percent_decode(value))
}

pub(super) fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                    decoded.push(high * 16 + low);
                    index += 3;
                } else {
                    decoded.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
