#!/usr/bin/env python3
"""Spawn or reuse a Puppet Master worker pane in a standalone terminal window.

If the HTTP bridge is already running (desktop app or another worker host),
this reuses it and opens a detached pane window in that host.

If no bridge is found, it starts a standalone worker host via the CLI
(`puppet-master worker`) — no full desktop grid UI required.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import subprocess
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


def discover_bridge(explicit: str | None, wait_seconds: float) -> str | None:
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

    if last_error:
        return None
    return None


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


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def launch_standalone_worker(args: argparse.Namespace) -> int:
    root = repo_root()
    cli_entry = root / "packages" / "cli" / "src" / "index.ts"
    if not cli_entry.is_file():
        print("launch-terminal-app: CLI entry not found; run from the puppet-master repo.", file=sys.stderr)
        return 1

    cmd = ["npm", "run", "worker", "--"]
    if args.agent_type:
        cmd.append(args.agent_type)
    if args.cwd:
        cmd.extend(["--cwd", str(Path(args.cwd).resolve())])
    if args.pane_id:
        cmd.extend(["--pane-id", args.pane_id])
    if args.cols:
        cmd.extend(["--cols", str(args.cols)])
    if args.rows:
        cmd.extend(["--rows", str(args.rows)])
    if args.force_new:
        cmd.append("--force-new")

    env = os.environ.copy()
    print("[launch-terminal-app] starting standalone worker host (no desktop grid required)")
    completed = subprocess.run(cmd, cwd=root, env=env, shell=os.name == "nt")
    return completed.returncode


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch a standalone Puppet Master worker terminal.")
    parser.add_argument("--bridge-url", help="Bridge URL, e.g. http://127.0.0.1:17321")
    parser.add_argument("--pane-id", help="Open an existing pane id instead of spawning/reusing one")
    parser.add_argument(
        "--agent-type",
        default="powershell" if platform.system() == "Windows" else "bash",
        help="Pane agent type to spawn/reuse (default: platform shell)",
    )
    parser.add_argument("--cwd", help="Working directory for a newly spawned pane")
    parser.add_argument("--cols", type=int, default=120, help="New pane columns")
    parser.add_argument("--rows", type=int, default=32, help="New pane rows")
    parser.add_argument("--force-new", action="store_true", help="Always spawn a new pane")
    parser.add_argument("--wait", type=float, default=2.0, help="Seconds to wait for an existing bridge")
    parser.add_argument("--send", help="Optional command/input to send after opening")
    parser.add_argument("--no-enter", action="store_true", help="Do not append Enter to --send")
    parser.add_argument(
        "--standalone",
        action="store_true",
        help="Always start a standalone worker host (skip bridge discovery)",
    )
    args = parser.parse_args()

    try:
        if args.standalone:
            return launch_standalone_worker(args)

        bridge_url = discover_bridge(args.bridge_url, args.wait)
        if bridge_url is None:
            return launch_standalone_worker(args)

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
        print("Falling back to standalone worker host…", file=sys.stderr)
        return launch_standalone_worker(args)


if __name__ == "__main__":
    raise SystemExit(main())
