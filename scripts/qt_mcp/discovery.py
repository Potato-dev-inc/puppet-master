"""Locate the MCP binary and discover structured records (operations, agents, panes) inside
MCP tool results and progress payloads."""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]


def default_command() -> tuple[str, list[str]]:
    binary_name = "puppet-master-mcp.exe" if os.name == "nt" else "puppet-master-mcp"
    updated_name = "puppet-master-mcp-updated.exe" if os.name == "nt" else "puppet-master-mcp-updated"
    debug_dir = REPO_ROOT / "packages/app/src-tauri/target/debug"
    available = [path for path in (debug_dir / binary_name, debug_dir / updated_name) if path.is_file()]
    if available:
        return str(max(available, key=lambda path: path.stat().st_mtime_ns)), []
    packaged = REPO_ROOT / "packages/mcp-server/dist" / binary_name
    if packaged.is_file():
        return str(packaged), []
    launcher = REPO_ROOT / "packages/mcp-server/dist/index.js"
    if launcher.is_file():
        return "node", [str(launcher)]
    return binary_name, []


def operation_snapshot(value: Any) -> dict[str, Any] | None:
    """Find an operation in structured MCP content, progress, or text output."""
    pending = [value]
    visited: set[int] = set()
    while pending:
        current = pending.pop(0)
        if not isinstance(current, dict) or id(current) in visited:
            continue
        visited.add(id(current))
        if current.get("isError") is True:
            continue
        if "operation_id" in current:
            return current
        for key in ("structuredContent", "snapshot", "message"):
            nested = current.get(key)
            if isinstance(nested, dict):
                pending.append(nested)
            elif isinstance(nested, str):
                try:
                    parsed = json.loads(nested)
                except (ValueError, TypeError):
                    continue
                if isinstance(parsed, dict):
                    pending.append(parsed)
        content = current.get("content")
        if isinstance(content, list):
            for item in content:
                if isinstance(item, dict) and isinstance(item.get("text"), str):
                    try:
                        parsed = json.loads(item["text"])
                    except (ValueError, TypeError):
                        continue
                    if isinstance(parsed, dict):
                        pending.append(parsed)
    return None


def agent_runs(value: Any) -> list[dict[str, Any]]:
    """Extract structured agent records from MCP results and progress payloads."""
    found: dict[str, dict[str, Any]] = {}
    pending = [value]
    visited: set[int] = set()
    while pending:
        current = pending.pop(0)
        if isinstance(current, list):
            pending.extend(current)
            continue
        if not isinstance(current, dict) or id(current) in visited:
            continue
        visited.add(id(current))
        if current.get("isError") is True:
            continue
        handle = current.get("handle")
        if isinstance(handle, str) and handle and ("status" in current or "operation_id" in current):
            found[handle] = current
        for key in ("structuredContent", "agent", "agents", "message", "data"):
            nested = current.get(key)
            if isinstance(nested, (dict, list)):
                pending.append(nested)
            elif isinstance(nested, str):
                try:
                    parsed = json.loads(nested)
                except (ValueError, TypeError):
                    continue
                if isinstance(parsed, (dict, list)):
                    pending.append(parsed)
        content = current.get("content")
        if isinstance(content, list):
            for item in content:
                if isinstance(item, dict) and isinstance(item.get("text"), str):
                    try:
                        parsed = json.loads(item["text"])
                    except (ValueError, TypeError):
                        continue
                    if isinstance(parsed, (dict, list)):
                        pending.append(parsed)
    return list(found.values())


def is_pane(item: Any) -> bool:
    return (
        isinstance(item, dict)
        and bool(item.get("pane_id") or item.get("id"))
        and ("agent_type" in item or "agent" in item)
    )


def pane_id_of(pane: dict[str, Any]) -> str:
    return str(pane.get("pane_id") or pane.get("id") or "")


def pane_label(pane: dict[str, Any]) -> str:
    pane_id = pane_id_of(pane)
    short = pane_id[:8] if pane_id else "unknown"
    agent = str(pane.get("agent_type") or pane.get("agent") or "worker")
    status = str(pane.get("status") or "")
    cwd = str(pane.get("cwd") or pane.get("working_directory") or "")
    cwd_name = Path(cwd).name if cwd else ""
    parts = [agent, short]
    if status:
        parts.append(status)
    if cwd_name:
        parts.append(cwd_name)
    return " · ".join(parts)


def extract_panes(value: Any) -> list[dict[str, Any]]:
    """Collect every worker pane in an MCP result, not just the first nested list."""
    found: list[dict[str, Any]] = []
    seen: set[str] = set()
    pending: list[Any] = [value]
    visited: set[int] = set()
    while pending:
        current = pending.pop(0)
        identity = id(current)
        if identity in visited:
            continue
        if isinstance(current, dict):
            visited.add(identity)
            if is_pane(current):
                pane_id = pane_id_of(current)
                if pane_id.startswith("puppet-master-orchestrator-"):
                    continue
                if pane_id and pane_id not in seen:
                    seen.add(pane_id)
                    found.append(current)
                continue
            pending.extend(current.values())
            continue
        if isinstance(current, list):
            pending.extend(current)
            continue
        if isinstance(current, str):
            try:
                pending.append(json.loads(current))
            except (ValueError, TypeError):
                continue
    return found
