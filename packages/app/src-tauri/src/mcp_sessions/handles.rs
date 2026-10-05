//! Handle and pane-id resolution plus the error values the ownership checks return.

use crate::operations::OperationError;
use serde_json::{json, Value};

pub(super) fn pane_not_found(pane: &str) -> OperationError {
    OperationError::new(
        "PANE_NOT_FOUND",
        format!("pane {pane} is not registered to this MCP session"),
        false,
    )
}

pub(super) const MIN_PANE_PREFIX_LEN: usize = 4;

/// Error for an agent handle this connection cannot resolve. The message is generic here;
/// `enrich_handle_error` upgrades it when the id turns out to be a pane id.
pub fn agent_not_found(handle: &str) -> OperationError {
    let mut error = OperationError::new(
        "AGENT_NOT_FOUND",
        format!(
            "agent handle {handle} is not registered to this MCP connection; expected an agent run handle from run_agent or list_workers for this project (runs started by another connection show as owned=false; UI panes appear in list_workers and can be adopted with run_agent(worker_id))"
        ),
        false,
    );
    error.context = json!({"handle": handle, "expected": "agent_run_handle"});
    error
}

/// If an AGENT_NOT_FOUND error names an id that is actually a pane id (or pane id prefix),
/// say so and point at the right parameter instead of a bare "not found".
pub fn enrich_handle_error(error: OperationError, pane_ids: &[String]) -> OperationError {
    enrich_handle_error_with(error, pane_ids, |_| None)
}

/// Exact wording for a pane id given where an agent run handle is required.
pub fn pane_id_as_handle_message(
    given: &str,
    kind: &str,
    pane_id: &str,
    run_handle: Option<&str>,
) -> String {
    let mut message = format!(
        "{given} is {kind}, not an agent run handle. This tool needs an agent run handle; get it from the run_agent result field `handle` or from list_agents `handle`."
    );
    match run_handle {
        Some(handle) => message.push_str(&format!(
            " Pane {pane_id} belongs to run handle `{handle}`; pass that handle instead."
        )),
        None => message.push_str(&format!(
            " Pane {pane_id} has no known run; to control a pane directly use take_over with pane_id (grant=true for panes this connection did not start)."
        )),
    }
    message
}

/// Like `enrich_handle_error`, with a lookup that maps a pane id to its latest run handle.
pub fn enrich_handle_error_with(
    mut error: OperationError,
    pane_ids: &[String],
    run_handle_for_pane: impl Fn(&str) -> Option<String>,
) -> OperationError {
    if error.code != "AGENT_NOT_FOUND" {
        return error;
    }
    let Some(handle) = error
        .context
        .get("handle")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return error;
    };
    if let Ok(pane) = resolve_pane_id(&handle, pane_ids) {
        let kind = if pane == handle {
            "a pane id"
        } else {
            "a pane id prefix"
        };
        let run_handle = run_handle_for_pane(&pane);
        error.message = pane_id_as_handle_message(&handle, kind, &pane, run_handle.as_deref());
        error.context = json!({"handle": handle, "expected": "agent_run_handle", "actual_kind": "pane_id", "pane_id": pane, "run_handle": run_handle});
    }
    error
}

/// Resolve a caller-supplied pane reference: a full pane id or a unique prefix (min 4 chars).
pub fn resolve_pane_id(input: &str, pane_ids: &[String]) -> Result<String, OperationError> {
    let input = input.trim();
    if pane_ids.iter().any(|id| id == input) {
        return Ok(input.to_owned());
    }
    let lower = input.to_ascii_lowercase();
    if lower.len() >= MIN_PANE_PREFIX_LEN {
        let matches: Vec<&String> = pane_ids
            .iter()
            .filter(|id| id.to_ascii_lowercase().starts_with(&lower))
            .collect();
        match matches.as_slice() {
            [one] => return Ok((*one).clone()),
            [] => {}
            many => {
                let mut error = OperationError::new(
                    "AMBIGUOUS_PANE_ID",
                    format!(
                        "pane id prefix {input} matches {} panes; pass a longer prefix or the full pane id",
                        many.len()
                    ),
                    false,
                );
                error.context = json!({"candidates": many});
                return Err(error);
            }
        }
    }
    let hint = if input.chars().all(|c| c.is_ascii_digit()) {
        " (looks like a UI display number, which is not a pane id)"
    } else if lower.len() < MIN_PANE_PREFIX_LEN {
        " (prefixes need at least 4 characters)"
    } else {
        ""
    };
    let mut error = OperationError::new(
        "PANE_NOT_FOUND",
        format!(
            "unknown pane: {input}{hint}; pane_id must be a full pane id or a unique prefix, see list_panes (agent run handles go in `handle`)"
        ),
        false,
    );
    error.context = json!({"pane_id": input, "expected": "pane_id"});
    Err(error)
}

pub(super) fn denied(message: &str) -> OperationError {
    OperationError::new("AUTHORIZATION_DENIED", message, false)
}
