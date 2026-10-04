"""Window-independent helpers for MCP tool calls: UI constants, argument defaults, delegate form
shaping, and wait timeouts. Kept free of Qt so the rules are easy to test and reuse."""

from __future__ import annotations

from typing import Any

TERMINAL_STATES = ["waiting_input", "completed", "failed", "cancelled"]
MODES = ("agent", "shell", "both")
BUTTON_LABELS = {
    "bridge_health": "Check bridge",
    "list_panes": "List workers",
    "delegate_work": "Delegate task",
    "get_operation": "Get operation status",
    "wait_for_operation": "Wait for update",
    "cancel_operation": "Cancel operation",
    "run_agent": "Run agent",
    "wait_agents": "Wait for agents",
    "cancel_agent": "Cancel agent",
    "list_agents": "List agents",
    "agent_transcript": "Agent transcript",
    "take_over": "Take over worker",
    "set_mode": "Set tool mode",
    "send_agent": "Send follow-up",
    "answer_prompt": "Answer prompt",
}
AGENT_CATALOG_TOOLS = {
    "run_agent", "wait_agents", "cancel_agent", "list_agents", "agent_transcript", "take_over",
}


def as_revision(value: Any) -> int:
    """Coerce a revision field to int; anything unparseable counts as revision 0."""
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


def input_properties(tool: dict[str, Any] | None) -> dict[str, Any]:
    """The `inputSchema.properties` of an advertised tool, or {} when absent or malformed."""
    schema = (tool or {}).get("inputSchema", {})
    return schema.get("properties", {}) if isinstance(schema, dict) else {}


def tool_defaults(
    name: str,
    tool: dict[str, Any] | None,
    *,
    project_path: str,
    pane_id: str,
    operation_id: str,
    revision: int,
    agent_type: str,
    agent_handle: str,
    agent_revision: int,
) -> dict[str, Any]:
    """Prefill values for a tool's argument form from the window's current selection."""
    common: dict[str, Any] = {}
    properties = input_properties(tool)
    if "project_path" in properties:
        common["project_path"] = project_path
    if "pane_id" in properties and pane_id:
        common["pane_id"] = pane_id
    if "operation_id" in properties and operation_id:
        common["operation_id"] = operation_id
    if "handle" in properties and agent_handle:
        common["handle"] = agent_handle
    if "handles" in properties and agent_handle:
        common["handles"] = [agent_handle]
    if "after_revisions" in properties and agent_handle:
        common["after_revisions"] = {agent_handle: agent_revision}
    if name == "delegate_work":
        common.setdefault("agent_type", agent_type)
        common.setdefault("acceptance_criteria", ["Complete the requested task and report the result."])
        if pane_id and "pane_id" in properties:
            common["pane_id"] = pane_id
    if name == "run_agent":
        available_agents = properties.get("agent_type", {}).get("enum", [])
        common.setdefault("agent_type", agent_type if agent_type in available_agents else "codex")
        common.setdefault("acceptance_criteria", ["Complete the requested task and report the result."])
    if name == "wait_for_operation":
        common.setdefault("after_revision", revision)
        common.setdefault("until", TERMINAL_STATES)
        common.setdefault("timeout_ms", 30_000)
    return common


def without_idempotency_key(tool: dict[str, Any]) -> dict[str, Any]:
    """A copy of the tool whose form hides `idempotency_key` (the window supplies a stable one)."""
    schema = tool.get("inputSchema", {})
    shaped = dict(tool)
    shaped["inputSchema"] = dict(schema)
    shaped["inputSchema"]["properties"] = dict(schema.get("properties", {}))
    shaped["inputSchema"]["properties"].pop("idempotency_key", None)
    shaped["inputSchema"]["required"] = [key for key in schema.get("required", []) if key != "idempotency_key"]
    return shaped


def call_timeout_ms(name: str, args: dict[str, Any]) -> int:
    """Client-side deadline for a call: the wait window (if any) plus a 5 s margin."""
    wait_ms = args.get("timeout_ms", 120_000) if name == "wait_for_operation" else 30_000
    return int(wait_ms) + 5_000 if isinstance(wait_ms, int) and wait_ms > 0 else 35_000
