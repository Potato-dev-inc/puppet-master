#!/usr/bin/env python3
"""Spawn an OpenCode (or other) agent in Puppet Master, then close it again.

Talks to the HTTP bridge of the running desktop app (or worker host); nothing else to start.

Two modes:

  run  (default)  Start an agent run with a task, wait for the result, then close the agent and
                  check that its pane and window are gone. Exercises the full lifecycle:
                      run_agent(keep_pane=true) -> close_agent -> pane removed from the registry.
  pane            Only spawn a bare pane, hold it open for --hold seconds, then kill it.

Examples:

  python scripts/spawn-close-agent.py
  python scripts/spawn-close-agent.py --task "Say hello" --hold 5
  python scripts/spawn-close-agent.py --agent-type opencode --mode pane --hold 10
  python scripts/spawn-close-agent.py --keep          # leave it open to inspect, print how to close
  python scripts/spawn-close-agent.py --close <handle-or-pane-id>   # close one you kept earlier

Exit code is 0 only when the agent was spawned and the pane was confirmed gone afterwards.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

DEFAULT_HOST = "127.0.0.1"
PORT_MIN = 17321
PORT_MAX = 17399
REPO_ROOT = Path(__file__).resolve().parent.parent
AGENT_TYPES = ["opencode_native", "opencode", "claude", "codex", "cursor_agent"]
PANE_TYPES = AGENT_TYPES + ["powershell", "cmd", "bash"]  # --mode pane also takes plain shells

# Agent routes need an MCP-style session and only the session that started an agent may close it.
# A fixed id lets a later run (--close) reach what an earlier --keep run left open; --session
# overrides it. No pid is sent, so the app never reaps this session as a dead MCP process.
SESSION_HEADERS = {"X-Puppet-Master-Session": "spawn-close-agent"}


def request_json(method: str, url: str, payload: dict | None = None, timeout: float = 30.0):
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    headers = dict(SESSION_HEADERS)
    if payload is not None:
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as res:
            raw = res.read().decode("utf-8")
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as err:
        body = err.read().decode("utf-8", errors="replace")
        raise SystemExit(f"{method} {url} -> HTTP {err.code}: {body[:600]}") from None


def discover_bridge(explicit: str | None) -> str:
    if explicit:
        return explicit.rstrip("/")
    for port in range(PORT_MIN, PORT_MAX + 1):
        base = f"http://{DEFAULT_HOST}:{port}"
        try:
            health = request_json("GET", f"{base}/health", timeout=0.25)
        except (SystemExit, OSError):
            continue
        if isinstance(health, dict) and health.get("ok"):
            return base
    raise SystemExit("No Puppet Master bridge found on 127.0.0.1:17321-17399. Start the app first.")


def kill_pane(base: str, pane_id: str) -> None:
    """Kill a pane. Agent panes are read-only to terminal controls until taken over, and bare
    pane control is a shell-mode tool, so switch mode and take it over first."""
    set_mode(base, "shell")
    request_json("POST", f"{base}/agents/take-over", {"pane_id": pane_id, "grant": True}, timeout=10.0)
    request_json("DELETE", f"{base}/panes/{pane_id}")


def set_mode(base: str, mode: str) -> None:
    """The session is sticky: run/close_agent need agent mode, bare panes need shell mode."""
    request_json("POST", f"{base}/mcp/mode", {"mode": mode}, timeout=5.0)


def pane_ids(base: str) -> list[str]:
    panes = request_json("GET", f"{base}/panes", timeout=5.0)
    return [pane["id"] for pane in panes] if isinstance(panes, list) else []


def wait_until_gone(base: str, pane_id: str, timeout: float) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if pane_id not in pane_ids(base):
            return True
        time.sleep(0.25)
    return pane_id not in pane_ids(base)


def step(label: str, started: float) -> None:
    print(f"[{time.time() - started:6.2f}s] {label}", flush=True)


def close_by_id(base: str, target: str, project: str) -> int:
    """Close an agent handle, or kill a bare pane id."""
    if target in pane_ids(base):
        kill_pane(base, target)
        gone = wait_until_gone(base, target, 5.0)
        print(f"killed pane {target[:8]}: {'gone' if gone else 'STILL PRESENT'}")
        return 0 if gone else 1
    set_mode(base, "agent")
    view = request_json("POST", f"{base}/agents/close", {"handle": target, "project_path": project})
    print(f"closed agent {target[:8]}: status={view.get('status')} stage={view.get('stage')}")
    return 0


def run_mode(base: str, args, started: float) -> int:
    request = {
        "task": args.task,
        "agent_type": args.agent_type,
        "project_path": args.project,
        "keep_pane": True,
        "wait_ms": int(args.wait * 1000),
        "idempotency_key": f"spawn-close-{uuid.uuid4()}",
    }
    set_mode(base, "agent")
    step(f"run_agent {args.agent_type} ...", started)
    view = request_json("POST", f"{base}/agents/run", request, timeout=args.wait + 30)
    handle, pane_id = view.get("handle"), view.get("pane_id")
    step(f"status={view.get('status')} stage={view.get('stage')} pane={str(pane_id)[:8]}", started)
    if view.get("result") is not None:
        print(f"         result: {str(view['result']).strip()[:200]!r}")
    if not handle:
        print("no handle returned; nothing to close", file=sys.stderr)
        return 1

    if args.hold > 0:
        step(f"holding the pane open for {args.hold:g}s (look at the window now)", started)
        time.sleep(args.hold)

    if args.keep:
        print(f"\nLeft open. Close it with:\n  python scripts/spawn-close-agent.py --close {handle}")
        return 0

    step("close_agent ...", started)
    closed = request_json(
        "POST", f"{base}/agents/close", {"handle": handle, "project_path": args.project}
    )
    step(f"closed: status={closed.get('status')} stage={closed.get('stage')}", started)
    if not pane_id:
        return 0
    gone = wait_until_gone(base, pane_id, 5.0)
    step(f"pane {pane_id[:8]} {'removed from registry' if gone else 'STILL IN REGISTRY'}", started)
    # The desktop app destroys the pane's window ~1.5s after the pane leaves the registry.
    if gone:
        time.sleep(2.0)
        print("         (its window should be gone by now)")
    return 0 if gone else 1


def pane_mode(base: str, args, started: float) -> int:
    # Spawning and killing bare panes is a shell-mode capability of the session.
    set_mode(base, "shell")
    step(f"spawn pane {args.agent_type} ...", started)
    spawned = request_json(
        "POST",
        f"{base}/panes",
        {"agent_type": args.agent_type, "cwd": args.project, "cols": 120, "rows": 32},
    )
    pane_id = (
        spawned.get("id")
        or spawned.get("pane_id")
        or (spawned.get("snapshot") or {}).get("pane_id")
    )
    if not pane_id:
        print(f"no pane id in response: {spawned}", file=sys.stderr)
        return 1
    step(f"pane {pane_id[:8]} spawned", started)
    if args.keep:
        print(f"\nLeft open. Close it with:\n  python scripts/spawn-close-agent.py --close {pane_id}")
        return 0
    step(f"holding for {args.hold:g}s", started)
    time.sleep(args.hold)
    kill_pane(base, pane_id)
    gone = wait_until_gone(base, pane_id, 5.0)
    step(f"pane {pane_id[:8]} {'killed and gone' if gone else 'STILL PRESENT'}", started)
    return 0 if gone else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mode", choices=["run", "pane"], default="run")
    parser.add_argument("--agent-type", choices=PANE_TYPES, default="opencode_native")
    parser.add_argument("--task", default="Reply with exactly the word PONG and nothing else.")
    parser.add_argument("--project", default=str(REPO_ROOT), help="project_path / cwd (default: this repo)")
    parser.add_argument("--hold", type=float, default=2.0, help="seconds to keep it open before closing")
    parser.add_argument("--wait", type=float, default=60.0, help="seconds to wait for the run result")
    parser.add_argument("--keep", action="store_true", help="do not close; print the close command")
    parser.add_argument("--close", metavar="HANDLE_OR_PANE_ID", help="only close this agent/pane, then exit")
    parser.add_argument("--session", help="session id (default: a fixed one, so --close can find kept agents)")
    parser.add_argument("--bridge", help="bridge base URL (default: auto-discover)")
    args = parser.parse_args()

    if args.mode == "run" and args.agent_type not in AGENT_TYPES:
        parser.error(f"--mode run needs an agent type ({', '.join(AGENT_TYPES)}); shells work with --mode pane")
    if args.session:
        SESSION_HEADERS["X-Puppet-Master-Session"] = args.session
    base = discover_bridge(args.bridge)
    started = time.time()
    print(f"bridge: {base}")
    if args.close:
        return close_by_id(base, args.close, args.project)
    if args.mode == "pane":
        return pane_mode(base, args, started)
    return run_mode(base, args, started)


if __name__ == "__main__":
    sys.exit(main())
