//! Catalog entries: Agent runs, workers, ownership and sessions.

use super::super::schema::*;
use super::super::types::{ToolDefinition, ToolSafety};
use serde_json::json;

pub(super) fn tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "set_mode",
            description: "Choose which MCP tool catalog this connection can use: agent, shell, or both. The selection is isolated to this MCP connection.",
            input_schema: object_schema(json!({"mode":{"type":"string","enum":["agent","shell","both"]}}), vec!["mode"]),
            output_schema: Some(json!({"type":"object","properties":{"mode":{"type":"string","enum":["agent","shell","both"]},"tool_count":{"type":"integer","minimum":0}},"required":["mode","tool_count"]})),
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(),
            method: "POST",
            path: "/mcp/mode",
        },
        ToolDefinition {
            name: "quick_ask",
            description: "Ask a tiny one-shot question and get just the answer, like a native subagent call. Runs a fresh headless read-only worker (no checks, no shell, pane closed afterwards) on the first installed backend (claude, codex, cursor_agent) unless `backend` says otherwise. Returns {answer, receipt}; receipt.not_verified lists what was not verified. If it is still running after wait_ms (default 90s, max 120s) it returns {status:\"running\", handle, hint:\"wait_agents\"}: call wait_agents(handle). For real work, checks, or follow-ups use run_agent.",
            input_schema: object_schema(json!({"task":{"type":"string"},"project_path":{"type":"string"},"backend":agent_type_schema(),"model":{"type":"string"},"wait_ms":{"type":"integer","minimum":0,"maximum":120000,"default":90000},"read_only":{"type":"boolean","default":true,"description":"Defaults to true. Only pass false when the question needs the agent to write."},"idempotency_key":{"type":"string"}}), vec!["task"]),
            output_schema: Some(json!({"type":"object","properties":{"answer":{"type":["string","null"]},"receipt":receipt_schema(),"status":{"type":"string","enum":["running","needs_input"]},"handle":{"type":"string"},"hint":{"type":"string"},"prompt":{"type":["object","null"]}}})),
            safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/quick",
        },
        ToolDefinition {
            name: "run_agent",
            description: "Start or adopt a worker with a task and return a stable handle. Happy path: list_agents → run_agent(worker_id or pane_id, task) → handle. Waits up to wait_ms (120s default). On WAIT_TIMEOUT, retry with the same idempotency_key and call wait_agents — do not start a duplicate turn. Steering: send_message; next turn: followup_task(handle). Closing the user's pane is not cleanup. keep_pane defaults to false: a pane this run created is closed automatically when the run completes, after which take_over, switch_agent_model and live pane inspection fail with PANE_CLOSED_AFTER_RUN (agent_transcript keeps working from the stored transcript). Pass keep_pane=true to keep the pane until close_agent is called. Adopted UI panes are never closed. Runs are visible by default (headless=false): the worker gets its own pop-out pane window, so the user can watch it. headless=true is the explicit opt-out (no pane); it is also the automatic backup when the agent has no visible mode or the visible pane fails to launch or take the prompt before the turn starts, in which case the result carries fallback:{from:\"visible\",to:\"headless\",reason} (reason no_visible_mode | visible_failed) and the receipt lists it in not_verified. Pass placement=\"grid\" to use the main window's terminal grid instead of a pop-out. Steering limits: Cursor headless steering works only by restart from turn 2, and visible Cursor panes are queue-only; send_message can queue the message as the next turn (delivery=queue) instead of steering live. watch_command is valid in PowerShell and Git Bash; pass shell=bash|powershell|cmd to target one.",
            input_schema: object_schema(json!({"project_path":{"type":"string"},"task":{"type":"string"},"name":{"type":"string"},"handle":{"type":"string"},"worker_id":{"type":"string"},"agent_run_id":{"type":"string"},"agent_type":agent_type_schema(),"headless":{"type":"boolean","default":false,"description":"Default false: the run opens a visible pane (its own pop-out window). Pass true to run a headless child process with no pane. Headless is also the automatic backup: if the agent has no visible mode, or the pane cannot be launched / the prompt cannot be delivered before the turn starts, the run falls back to headless once and reports receipt.fallback."},"background":{"type":"boolean","default":false},"wait_ms":{"type":"integer","minimum":0,"maximum":300000,"default":120000},"read_only":{"type":"boolean","default":false},"placement":{"type":"string","enum":["auto","grid","window"],"default":"window","description":"Where a visible (headless=false) pane opens. window (default): its own pop-out OS window, one per pane; grid/auto: inside the main Puppet Master window's terminal grid when that window is open and visible (pop-out otherwise). Ignored for headless runs."},"isolation":{"type":"string","enum":["none","worktree"],"default":"none","description":"worktree: the worker runs in its own git worktree on branch pm/<handle> under <project>/.puppet-master/worktrees, so parallel agents never touch the same files; the run, locks and list_agents still key on project_path. The receipt gains worktree {path,branch,base,state,files_changed,additions,deletions}. Nothing is auto-deleted: review with review_worktree (diff, merge, discard) or in the Puppet Master cockpit. Requires a git repo with a commit (NOT_A_GIT_REPO, NO_COMMITS). Starts a new worker: cannot be combined with pane_id or worker_id. Works with every agent_type."},"keep_pane":{"type":"boolean","default":false,"description":"Default false: a pane created for this run is closed when the run completes (take_over/switch_agent_model then fail with PANE_CLOSED_AFTER_RUN). true keeps the pane until close_agent."},"timeout_ms":{"type":"integer","minimum":1,"maximum":86400000,"default":900000},"idempotency_key":{"type":"string"},"shell":{"type":"string","enum":["bash","powershell","cmd"],"description":"Shell that will run watch_command (default: a form valid in both PowerShell and Git Bash)."},"acceptance_criteria":{"type":"array","items":{"type":"string","minLength":1},"description":"Optional free-text guidance for the worker; NOT machine-checked. For verified acceptance use `checks`."},"checks":{"type":"array","maxItems":20,"description":"Acceptance checks evaluated at completion. file_* and result_contains are read-only; command_exits runs the given argv (no shell) in project_path and compares its exit code. Paths are relative to project_path. A failing check fails the run with ACCEPTANCE_FAILED.","items":{"type":"object","properties":{"type":{"type":"string","enum":["file_exists","file_contains","file_absent","result_contains","command_exits"]},"path":{"type":"string","minLength":1,"description":"Required for file_* checks."},"text":{"type":"string","minLength":1},"command":{"type":"array","items":{"type":"string"},"minItems":1,"description":"command_exits: argv run in project_path without a shell, e.g. [\"python\",\"-m\",\"pytest\",\"-q\"]."},"expect_exit":{"type":"integer","description":"command_exits only; expected exit code, 0 when omitted."},"timeout_ms":{"type":"integer","minimum":1,"maximum":600000,"description":"command_exits only; 120000 when omitted."}},"required":["type"]}},"pane_id":{"type":"string"},"allow_broad":{"type":"boolean","default":false},"trust_workspace":{"type":"boolean","default":false,"description":"Opt-in, headless cursor_agent only: pass --trust so Cursor trusts project_path without an interactive prompt. Only set for a folder the user named."},"allow_shell":{"type":"boolean","default":false,"description":"Opt-in, headless cursor_agent only (not with read_only): pass --force so Cursor's shell commands run instead of being rejected. Commands then run without review inside project_path; pair with `checks`. Without it, headless Cursor cannot run tests and the result carries SHELL_COMMAND_REJECTED. opencode_native already runs shell commands (bash) without this flag, so there it is accepted as a no-op; it is rejected with read_only and for backends that do not need it."},"allowed_commands":{"type":"array","maxItems":10,"items":{"type":"string","minLength":1},"description":"Opt-in, visible cursor_agent pane only: bare program names (e.g. [\"python\",\"pytest\"]) answered allow-once when Cursor stops on Not in allowlist: <program>. Also argv patterns with `*` as a glob (e.g. \"python todo.py add *\"; a leading `cd <dir> &&` is ignored, every `&&`-joined command must match; `;` `||` `|` redirections `$()` sudo and rm -rf are never auto-approved). Never allow-always; any other prompt still needs answer_prompt."},"context_policy":{"type":"string","enum":["fresh","packet","parent_summary","resume","selected_history"]},"context_mode":{"type":"string","enum":["fresh","packet","parent_summary","resume","selected_history"],"description":"alias for context_policy"},"model":{"type":"string"},"reasoning":{"type":"string"},"role":{"type":"string"},"scope":{"type":"string"},"selected_history":{"type":"array","items":{"type":"string"}},"locks":{"type":"array","items":{"type":"string","minLength":1},"description":"Optional resource locks (type:name, e.g. file:src/a.rs) acquired before the run starts and released when it ends. Fails fast with RESOURCE_LOCKED (context resource_id, owner_id) if another run holds one."}}), vec!["task"]),
            output_schema: Some(agent_run_schema()), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/run",
        },
        ToolDefinition {
            name: "wait_agents",
            description: "Wait for worker handles. Default until is completed, failed, cancelled, and needs_input (progress revisions do not wake). Pass wake_on_progress=true to wake on any revision. Omits result when unchanged since after_cursor (result_unchanged). Default and maximum timeout_ms is 120000: MCP hosts commonly abandon a call after 2-3 minutes (Request timed out) and a timeout never drops ownership, so loop wait_agents for longer runs.",
            input_schema: object_schema(json!({"project_path":{"type":"string"},"handles":{"type":"array","minItems":1,"items":{"type":"string","minLength":1}},"after_cursor":{"type":"integer","minimum":0},"after_revisions":{"type":"object","additionalProperties":{"type":"integer","minimum":0}},"until":{"type":"array","items":{"type":"string","enum":["queued","starting","running","waiting_input","needs_input","cancelling","completed","failed","cancelled"]}},"wake_on_progress":{"type":"boolean","default":false},"timeout_ms":{"type":"integer","minimum":0,"maximum":120000,"default":120000},"mode":{"type":"string","enum":["any","all"],"default":"any"},"shell":{"type":"string","enum":["bash","powershell","cmd"],"description":"Shell that will run watch_command (default: valid in both PowerShell and Git Bash)."}}), vec!["handles"]),
            output_schema: Some(json!({"type":"object","properties":{"agents":{"type":"array","items":agent_run_schema()},"reason":{"type":"string"},"mode":{"type":"string","enum":["any","all"]}},"required":["agents","reason","mode"]})), safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(), method: "POST", path: "/agents/wait",
        },
        ToolDefinition {
            name: "send_message",
            description: "Deliver a correction or steering message to the current turn. Returns a receipt with state accepted|queued|processed|deferred|unsupported|expired|rejected. accepted means the native session took the message; processed is set when the next assistant reply is recorded. Steering a finished turn returns rejected and suggests followup_task. Never injects arbitrary text into a busy TUI. delivery=live_or_queue (default).",
            input_schema: object_schema(json!({"handle":{"type":"string"},"message":{"type":"string","minLength":1},"task":{"type":"string","minLength":1},"delivery":{"type":"string","enum":["live_or_queue","live","queue"],"default":"live_or_queue"},"idempotency_key":{"type":"string"},"project_path":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(json!({"type":"object","properties":{"message_id":{"type":"string"},"idempotency_key":{"type":"string"},"disposition":{"type":"string","enum":["accepted","accepted_via_restart","queued","processed","deferred","unsupported","expired","rejected"]},"state":{"type":"string","enum":["accepted","accepted_via_restart","queued","processed","deferred","unsupported","expired","rejected"]},"handle":{"type":"string"},"turn_id":{"type":"string"},"result":{"type":["string","null"]},"suggestion":{"type":"string"},"next_cursor":{"type":"integer"},"last_steer":{"type":"object","properties":{"message_id":{"type":"string"},"state":{"type":"string"},"turn_id":{"type":"string"}}}},"required":["message_id","disposition"]})), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/send",
        },
        ToolDefinition {
            name: "followup_task",
            description: "Give the same worker a new task as the next turn. Like run_agent, this waits up to 120 seconds and returns the settled turn (status/result). Call wait_agents only if status is still queued or running; a transport timeout still keeps this handle — wait_agents recovers the answer. The handle stays stable; a finished task does not dispose the worker.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"task":{"type":"string","minLength":1},"project_path":{"type":"string"},"idempotency_key":{"type":"string"},"context_policy":{"type":"string","enum":["fresh","packet","parent_summary","resume","selected_history"]},"context_mode":{"type":"string","enum":["fresh","packet","parent_summary","resume","selected_history"],"description":"alias for context_policy"},"selected_history":{"type":"array","items":{"type":"string"}},"acceptance_criteria":{"type":"array","items":{"type":"string","minLength":1},"description":"Optional free-text guidance for the worker; NOT machine-checked. For verified acceptance use `checks`."},"checks":{"type":"array","maxItems":20,"description":"Acceptance checks evaluated at completion. file_* and result_contains are read-only; command_exits runs the given argv (no shell) in project_path and compares its exit code. Paths are relative to project_path. A failing check fails the run with ACCEPTANCE_FAILED.","items":{"type":"object","properties":{"type":{"type":"string","enum":["file_exists","file_contains","file_absent","result_contains","command_exits"]},"path":{"type":"string","minLength":1,"description":"Required for file_* checks."},"text":{"type":"string","minLength":1},"command":{"type":"array","items":{"type":"string"},"minItems":1,"description":"command_exits: argv run in project_path without a shell, e.g. [\"python\",\"-m\",\"pytest\",\"-q\"]."},"expect_exit":{"type":"integer","description":"command_exits only; expected exit code, 0 when omitted."},"timeout_ms":{"type":"integer","minimum":1,"maximum":600000,"description":"command_exits only; 120000 when omitted."}},"required":["type"]}},"wait_ms":{"type":"integer","minimum":0,"maximum":300000,"default":120000},"background":{"type":"boolean","default":false},"locks":{"type":"array","items":{"type":"string","minLength":1},"description":"Optional resource locks (type:name, e.g. file:src/a.rs) acquired before the run starts and released when it ends. Fails fast with RESOURCE_LOCKED (context resource_id, owner_id) if another run holds one."},"model":{"type":"string"},"reasoning":{"type":"string"},"shell":{"type":"string","enum":["bash","powershell","cmd"],"description":"Shell that will run watch_command (default: valid in both PowerShell and Git Bash)."}}), vec!["handle","task"]),
            output_schema: Some(agent_run_schema()), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/followup",
        },
        ToolDefinition {
            name: "send_agent",
            description: "Compatibility wrapper for followup_task. Prefer followup_task for a new turn or send_message for steering the current turn.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"task":{"type":"string","minLength":1},"message":{"type":"string","minLength":1},"project_path":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(agent_run_schema()), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/followup",
        },
        ToolDefinition {
            name: "answer_prompt",
            description: "Answer the approval prompt blocking a worker. Pass handle (agent run) or pane_id (pane this connection controls). Get prompt_id from wait_agents or inspect_agent. With handle, project_path is optional: the handle's own project is used (a project_path that is a different project is rejected with PROJECT_MISMATCH, a handle present in several projects with AMBIGUOUS_HANDLE; both before any key is sent). Every refusal (stale prompt_id, foreign owner, broad choice without allow_broad) also happens before input is sent. The reply carries answer.applied=true once the key is delivered; answer.warning means only the run-state bookkeeping failed, so do not answer again. Retrying the same prompt_id and choice after it was applied returns the prior receipt with answer.already_applied=true and presses nothing.",
            input_schema: json!({
                "type":"object",
                "properties":{
                    "handle":{"type":"string"},
                    "pane_id":{"type":"string"},
                    "project_path":{"type":"string"},
                    "prompt_id":{"type":"string"},
                    "choice":{"type":"string"},
                    "allow_broad":{"type":"boolean","default":false}
                },
                "required":["prompt_id","choice"],
                "anyOf":[{"required":["handle"]},{"required":["pane_id"]}]
            }),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/answer",
        },
        ToolDefinition {
            name: "cancel_agent",
            description: "Compatibility wrapper for interrupt_agent. Stops the current turn and keeps the worker handle.",
            input_schema: object_schema(json!({"handle":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(agent_run_schema()), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/cancel",
        },
        ToolDefinition {
            name: "interrupt_agent",
            description: "Stop the current turn without disposing the worker. The handle stays valid for followup_task. Resolves the worker by handle even when project_path is omitted.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"project_path":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(agent_run_schema()), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/cancel",
        },
        ToolDefinition {
            name: "list_agents",
            description: "List workers this connection can adopt: managed runs and live UI panes. Each entry has worker_id, backend, workspace, status, model when known, grant_required, and read_only_supported. Pass worker_id to run_agent to adopt. Alias: list_workers.",
            input_schema: object_schema(json!({"project_path":{"type":"string"},"mine_only":{"type":"boolean","default":false,"description":"Only runs and panes this connection controls."},"active_only":{"type":"boolean","default":false,"description":"Hide runs that have ended (completed/failed/cancelled)."},"ended_within_ms":{"type":"integer","minimum":0,"description":"Hide ended runs that finished longer ago than this. Runs are never deleted; this only narrows the listing."}}), vec![]),
            output_schema: Some(json!({"type":"object","properties":{"workers":{"type":"array","items":{"type":"object","properties":{"worker_id":{"type":"string"},"handle":{"type":"string"},"pane_id":{"type":["string","null"]},"backend":{"type":"string"},"workspace":{"type":"string"},"status":{"type":"string"},"model":{"type":["string","null"]},"owned":{"type":"boolean"},"grant_required":{"type":"boolean"},"read_only_supported":{"type":"boolean"},"adoptable":{"type":"boolean"}}}}},"required":["workers"]})),
            safety: ToolSafety::ReadOnly, visibility: visible_everywhere(), method: "GET", path: "/agents",
        },
        ToolDefinition {
            name: "list_workers",
            description: "Same as list_agents: discover UI panes and managed runs in agent mode, then adopt with run_agent(worker_id, task).",
            input_schema: object_schema(json!({"project_path":{"type":"string"},"mine_only":{"type":"boolean","default":false},"active_only":{"type":"boolean","default":false},"ended_within_ms":{"type":"integer","minimum":0}}), vec![]),
            output_schema: Some(json!({"type":"object","properties":{"workers":{"type":"array"}},"required":["workers"]})),
            safety: ToolSafety::ReadOnly, visibility: visible_everywhere(), method: "GET", path: "/agents",
        },
        ToolDefinition {
            name: "inspect_agent",
            description: "Inspect a worker: status, current task, context_policy, requested vs resolved model, capabilities, and turn ids. Pass the handle from run_agent, not a pane id.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"project_path":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(), method: "GET", path: "/agents/{handle}",
        },
        ToolDefinition {
            name: "agent_transcript",
            description: "Read typed transcript events for a worker (user task/steering, assistant commentary and final response, tool activity, prompts, interruption, failure). Truncation never drops the stored turn result — use wait_agents or inspect_agent for that.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"project_path":{"type":"string"},"after":{"type":"integer","minimum":0},"operation_id":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::ReadOnly,
            visibility: visible_everywhere(), method: "GET", path: "/agents/{handle}/transcript",
        },
        ToolDefinition {
            name: "close_agent",
            description: "Explicitly end a worker session. Cancels the current turn, releases this connection's control, and returns status=closed.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"project_path":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(agent_run_schema()), safety: ToolSafety::Destructive,
            visibility: visible_everywhere(), method: "POST", path: "/agents/close",
        },
        ToolDefinition {
            name: "review_worktree",
            description: "Review the git worktree of a run started with run_agent(isolation=\"worktree\"). action=diff (default): changed files with +/- counts and a capped unified patch (pass path to narrow it). action=merge: --no-ff merge of the worker's branch into the project's current branch, then removes the worktree; refuses with ORIGIN_DIRTY_OVERLAP if the project has uncommitted edits to the same files, RUN_ACTIVE while the worker is still working, and on MERGE_CONFLICT aborts so the project is unchanged and lists the conflicted files. action=discard: deletes the worktree and branch (unmerged work is lost). Only for handles this connection owns.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"action":{"type":"string","enum":["diff","merge","discard"],"default":"diff"},"path":{"type":"string","description":"diff only: narrow the patch to one file."},"project_path":{"type":"string"}}), vec!["handle"]),
            output_schema: None,
            safety: ToolSafety::Destructive,
            visibility: visible_everywhere(), method: "POST", path: "/worktrees/review",
        },
        ToolDefinition {
            name: "take_over",
            description: "Take temporary pane or worker control and return the rendered screen. Pass `handle` (run handle or a list_agents worker_id/pane id) or `pane_id`. A pane not created by this connection requires grant=true. Prefer run_agent(worker_id) to adopt. This does not transfer ownership; use transfer_agent for that.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"project_path":{"type":"string"},"pane_id":{"type":"string"},"grant":{"type":"boolean","default":false}}), vec![]),
            output_schema: Some(json!({"type":"object","properties":{"handle":{"type":"string"},"pane_id":{"type":"string"},"screen":{"type":"string"},"prompt":{},"control":{"type":"string","enum":["agent","shell"]}},"required":["handle","pane_id","screen","control"]})),
            safety: ToolSafety::Mutating, visibility: visible_everywhere(), method: "POST", path: "/agents/take-over",
        },
        ToolDefinition {
            name: "shell_exec",
            description: "Run a shell command in the selected project directory.",
            input_schema: object_schema(json!({"command":{"type":"string","minLength":1},"cwd":{"type":"string"},"pane_id":{"type":"string"},"timeout_ms":{"type":"integer","minimum":1}}), vec!["command"]),
            output_schema: Some(json!({"type":"object","properties":{"stdout":{"type":"string"},"stderr":{"type":"string"},"exit_code":{"type":"integer"}}})),
            safety: ToolSafety::Mutating, visibility: visible_everywhere(), method: "POST", path: "/shell/exec",
        },
        ToolDefinition {
            name: "release",
            description: "Return temporary manual pane control to the worker's owner. Does not drop the worker lease and does not transfer ownership. Use release_lease to drop the lease, or transfer_agent to move it.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"pane_id":{"type":"string"}}), vec![]),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/release",
        },
        ToolDefinition {
            name: "session_identity",
            description: "Read this connection's coordinator identity. The connection UUID is a transport id; coordinator_id plus attach_token are the reconnect credentials. A caller-supplied name is not an identity.",
            input_schema: object_schema(json!({}), vec![]),
            output_schema: Some(json!({"type":"object","properties":{"connection_id":{"type":"string"},"coordinator_id":{"type":"string"},"attach_token":{"type":"string"}},"required":["connection_id","coordinator_id","attach_token"]})),
            safety: ToolSafety::ReadOnly, visibility: visible_everywhere(), method: "GET", path: "/mcp/session",
        },
        ToolDefinition {
            name: "attach_agents",
            description: "Re-attach this connection to a coordinator's workers after reconnect. Requires the server-issued attach_token from session_identity. coordinator_name is ignored and never grants authority.",
            input_schema: object_schema(json!({"attach_token":{"type":"string"},"coordinator_name":{"type":"string"}}), vec!["attach_token"]),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/attach",
        },
        ToolDefinition {
            name: "release_lease",
            description: "Drop this connection's worker control lease. Distinct from release (return pane control) and transfer_agent (move ownership).",
            input_schema: object_schema(json!({"handle":{"type":"string"}}), vec!["handle"]),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/release-lease",
        },
        ToolDefinition {
            name: "transfer_agent",
            description: "Transfer worker ownership to another live MCP connection (to_session_id). A coordinator name does not grant authority and is not a valid target.",
            input_schema: object_schema(json!({"handle":{"type":"string"},"to_session_id":{"type":"string"},"coordinator_name":{"type":"string"}}), vec!["handle","to_session_id"]),
            output_schema: Some(json!({"type":"object"})), safety: ToolSafety::Mutating,
            visibility: visible_everywhere(), method: "POST", path: "/agents/transfer",
        },
    ]
}
