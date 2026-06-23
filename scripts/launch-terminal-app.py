#!/usr/bin/env python3
"""Spawn or reuse a Puppet Master terminal pane and detach it into its own window.

Requires the Puppet Master desktop app to be running, because the bridge and
native detached window are owned by the app process.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path


DEFAULT_HOST = "127.0.0.1"
PORT_MIN = 17321
PORT_MAX = 17399


def request_json(method: str, url: str, payload: dict | None = None, timeout: float = 3.0) -> dict | list:
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    headers = {"Content-Type": "application/json"} if payload is not None else {}
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    with urllib.request.urlopen(req, timeout=timeout) as res:
        raw = res.read().decode("utf-8")
        return json.loads(raw) if raw else {}


def discover_bridge(explicit: str | None, wait_seconds: float) -> str:
    if explicit:
        return explicit.rstrip("/")

    deadline = time.time() + wait_seconds
    last_error: Exception | None = None
    while time.time() <= deadline:
        for port in range(PORT_MIN, PORT_MAX + 1):
            base = f"http://{DEFAULT_HOST}:{port}"
            try:
                health = request_json("GET", f"{base}/health", timeout=0.25)
                if isinstance(health, dict) and health.get("ok"):
                    return base
            except Exception as exc:  # keep scanning; bridge may still be booting
                last_error = exc
        time.sleep(0.25)

    detail = f" Last error: {last_error}" if last_error else ""
    raise RuntimeError(f"Puppet Master bridge not found on ports {PORT_MIN}-{PORT_MAX}.{detail}")


def find_reusable_pane(bridge_url: str, agent_type: str) -> str | None:
    panes = request_json("GET", f"{bridge_url}/panes")
    if not isinstance(panes, list):
        return None
    for pane in panes:
        if (
            isinstance(pane, dict)
            and pane.get("agent_type") == agent_type
            and pane.get("status") != "error"
        ):
            return str(pane["id"])
    return None


def spawn_pane(bridge_url: str, args: argparse.Namespace) -> str:
    payload: dict[str, object] = {
        "agent_type": args.agent_type,
        "cols": args.cols,
        "rows": args.rows,
    }
    if args.cwd:
        payload["cwd"] = str(Path(args.cwd).resolve())
    if args.pane_id:
        payload["pane_id"] = args.pane_id

    created = request_json("POST", f"{bridge_url}/panes", payload)
    if not isinstance(created, dict) or "pane_id" not in created:
        raise RuntimeError(f"Unexpected spawn response: {created!r}")
    return str(created["pane_id"])


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch a detached Puppet Master terminal pane.")
    parser.add_argument("--bridge-url", help="Bridge URL, e.g. http://127.0.0.1:17321")
    parser.add_argument("--pane-id", help="Open an existing pane id instead of spawning/reusing one")
    parser.add_argument("--agent-type", default="cmd", help="Pane agent type to spawn/reuse (default: cmd)")
    parser.add_argument("--cwd", help="Working directory for a newly spawned pane")
    parser.add_argument("--cols", type=int, default=120, help="New pane columns")
    parser.add_argument("--rows", type=int, default=32, help="New pane rows")
    parser.add_argument("--force-new", action="store_true", help="Always spawn a new pane")
    parser.add_argument("--wait", type=float, default=8.0, help="Seconds to wait for the bridge")
    parser.add_argument("--send", help="Optional command/input to send after opening")
    parser.add_argument("--no-enter", action="store_true", help="Do not append Enter to --send")
    args = parser.parse_args()

    try:
        bridge_url = discover_bridge(args.bridge_url, args.wait)
        pane_id = args.pane_id
        if not pane_id and not args.force_new:
            pane_id = find_reusable_pane(bridge_url, args.agent_type)
        if not pane_id:
            pane_id = spawn_pane(bridge_url, args)

        request_json("POST", f"{bridge_url}/panes/{pane_id}/detach", {})

        if args.send:
            request_json(
                "POST",
                f"{bridge_url}/panes/{pane_id}/input",
                {"text": args.send, "append_newline": not args.no_enter},
            )

        print(f"Detached pane {pane_id} via {bridge_url}")
        return 0
    except (RuntimeError, urllib.error.URLError, urllib.error.HTTPError) as exc:
        print(f"launch-terminal-app: {exc}", file=sys.stderr)
        print("Make sure the Puppet Master desktop app is running first.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
