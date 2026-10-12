//! JSON-schema helpers shared by the catalog families.

use super::types::ToolVisibility;
use serde_json::{json, Value};

pub(super) fn visible_everywhere() -> ToolVisibility {
    ToolVisibility {
        sidebar: true,
        external_mcp: true,
    }
}

pub(super) fn object_schema(properties: Value, required: Vec<&'static str>) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

/// The one schema for every tool input naming an agent (`agent_type`, `backend`). A validated
/// string, not an enum: registering an agent must never change the catalog (`catalog_version`).
/// Unknown ids are rejected by the bridge with UNKNOWN_AGENT_TYPE.
pub(super) fn agent_type_schema() -> Value {
    json!({
        "type": "string",
        "pattern": "^[a-z][a-z0-9_]{1,31}$",
        "description": "Agent id, e.g. claude, codex, cursor_agent, opencode_native. The registered ids and what each supports are listed by GET /agents/types; an unregistered id fails with UNKNOWN_AGENT_TYPE (context.valid lists the accepted ids).",
    })
}

pub(super) fn env_tool_schema() -> Value {
    json!({"type":"object","properties":{"present":{"type":"boolean"},"version":{"type":["string","null"]},"path":{"type":["string","null"]}},"required":["present"]})
}

pub(super) fn operation_snapshot_schema() -> Value {
    json!({"type":"object","properties":{
        "operation_id":{"type":"string"},"project_path":{"type":"string"},"task":{"type":"string"},
        "pane_id":{"type":["string","null"]},"idempotency_key":{"type":"string"},
        "status":{"type":"string","enum":["queued","starting","running","waiting_input","cancelling","completed","failed","cancelled"]},
        "revision":{"type":"integer","minimum":0},"created_at_ms":{"type":"integer"},"updated_at_ms":{"type":"integer"},
        "source":{"type":"string","enum":["native","inferred"]},"stage":{"type":["string","null"]},
        "progress_pct":{"type":["integer","null"],"minimum":0,"maximum":100},
        "result":{"type":["string","null"]},"error":{"type":["object","null"],"properties":{
            "code":{"type":"string"},"message":{"type":"string"},"recoverable":{"type":"boolean"},
            "retry_after_ms":{"type":"integer"},"context":{}
        },"required":["code","message","recoverable"]},"required_action":{},"pane_state":{}
    },"required":["operation_id","project_path","task","idempotency_key","status","revision","created_at_ms","updated_at_ms","source"]})
}

pub(super) fn receipt_schema() -> Value {
    json!({"type":"object","description":"What this run verified and what it did not. not_verified entries are derived from recorded facts only.","properties":{
        "backend":{"type":"string"},"model":{"type":"string","description":"Resolved model when confirmed, else the requested one."},
        "read_only":{"type":"string","enum":["enforced","unenforced","off"]},
        "checks":{"type":"string","description":"`none configured` or `<passed>/<total> passed`."},
        "result_capture":{"type":"string","enum":["authoritative","inferred","missing","pending"]},
        "not_verified":{"type":"array","items":{"type":"string"}},"outcome":{"type":"string","enum":["passed_checks","completed_unchecked","failed_checks","failed","cancelled","needs_input","in_progress"],"description":"The one field for did-it-work. Only passed_checks means the requested behaviour was proven by acceptance checks; completed_unchecked means the worker finished but nothing verified it."},"build_id":{"type":"string","description":"Puppet Master build that produced this run."},
        "fallback":{"type":["object","null"],"description":"Null when the run used the mode that was asked for. {from:\"visible\",to:\"headless\",reason} when a visible run ran headless: reason no_visible_mode (the agent has no visible mode) or visible_failed (the pane failed to launch or the prompt could not be delivered before the turn started; the failed pane was closed and the task ran once, headless).","properties":{"from":{"type":"string"},"to":{"type":"string"},"reason":{"type":"string","enum":["no_visible_mode","visible_failed"]}}},
        "standby_used":{"type":"boolean","description":"Present and true when a warm standby worker (opt-in pool) ran this turn, so no pane spawn or CLI startup was paid for it."}
    },"required":["backend","read_only","checks","result_capture","not_verified"]})
}

pub(super) fn agent_run_schema() -> Value {
    let mut schema = json!({"type":"object","properties":{
        "handle":{"type":"string"},"operation_id":{"type":"string"},"turn_id":{"type":"string"},
        "fallback":{"type":["object","null"],"description":"Null when the run used the mode that was asked for. {from:\"visible\",to:\"headless\",reason} when a visible run ran headless: reason no_visible_mode (the agent has no visible mode) or visible_failed (the pane failed to launch or the prompt could not be delivered before the turn started; the failed pane was closed and the task ran once, headless).","properties":{"from":{"type":"string"},"to":{"type":"string"},"reason":{"type":"string","enum":["no_visible_mode","visible_failed"]}}},
        "status":{"type":"string","enum":["running","needs_input","completed","failed","cancelled","timeout","interrupted","closed"]},
        "result":{"type":["string","null"]},"result_capture":{"type":"string","enum":["authoritative","inferred","missing","pending"]},
        "watch_command":{"type":"string"},
        "acceptance_status":{"type":"string","enum":["not_checked","passed","failed","blocked"]},
        "verified":{"type":"boolean"},
        "prompt":{"type":["object","null"]},"error":{"type":["object","null"]},
        "pane_id":{"type":["string","null"]},"duration_ms":{"type":["integer","null"],"description":"Prompt sent to completion."},"total_ms":{"type":["integer","null"],"description":"Run created to completion, including pane spawn and readiness."},"setup_ms":{"type":["integer","null"]},"pane_closing":{"type":"boolean"},"standby_used":{"type":"boolean","description":"Present and true when a warm standby worker ran this turn; the stage log then shows standby_claimed."},"revision":{"type":"integer"},
        "next_cursor":{"type":"integer","minimum":0},
        "name":{"type":["string","null"]},
        "context_policy":{"type":"string","enum":["fresh","packet","parent_summary","resume","selected_history"]},
        "context_continuity":{"type":"string","enum":["resume","reconstructed_summary","none"]},
        "requested_model":{"type":["string","null"]},"resolved_model":{"type":["string","null"]},
        "requested_reasoning":{"type":["string","null"]},"resolved_reasoning":{"type":["string","null"]},
        "model_mismatch":{"type":"boolean"},
        "warnings":{"type":"array","description":"Caveats on a finished run (never change status): SHELL_COMMAND_REJECTED, PROVENANCE_INFERRED, MODEL_UNRESOLVED, RESULT_TRUNCATED (result is only the tail of a reply longer than the screen), OPENCODE_SERVER_UNREACHABLE. source=heuristic means a text match, not a fact.","items":{"type":"object","properties":{"code":{"type":"string"},"source":{"type":"string","enum":["capture","heuristic"]},"detail":{"type":"string"}},"required":["code","source","detail"]}},
        "capabilities":{"type":"object","properties":{
            "session_resume":{"type":"boolean"},"live_messages":{"type":"boolean"},
            "queued_followups":{"type":"boolean"},"structured_results":{"type":"boolean"},
            "model_selection":{"type":"boolean"},"reasoning_selection":{"type":"boolean"},
            "graceful_interrupt":{"type":"boolean"},"visible_terminal":{"type":"boolean"},
            "interface":{"type":"string","enum":["native_protocol","structured_headless","tui_observation"]},
            "notes":{"type":"array","items":{"type":"string"}}
        }}
    },"required":["handle","operation_id","status","verified","revision"]});
    if let Some(properties) = schema
        .pointer_mut("/properties")
        .and_then(Value::as_object_mut)
    {
        properties.insert("result_unchanged".into(), json!({"type":"boolean"}));
        properties.insert("wake_reason".into(), json!({"type":"string"}));
        properties.insert("result_verified".into(), json!({"type":"boolean"}));
        properties.insert("model_verified".into(), json!({"type":["boolean","null"]}));
        properties.insert("recovery".into(), json!({"type":["object","null"]}));
        properties.insert("receipt".into(), receipt_schema());
    }
    schema
}
