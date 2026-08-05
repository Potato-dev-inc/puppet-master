#!/usr/bin/env python3
"""Puppet Master librarian — project IR for orchestrator context packs.

Static mode: wiki-style sections (packages, MCP tools, docs) — not a raw tree dump.
LLM mode (--llm): DeepWiki-style narrative via Anthropic/OpenAI (needs API key in env).

Writes:
  .puppet-master/project-ir.json   — overview (+ optional sections)
  .puppet-master/project-ir.meta.json — git_sha, generator, generated_at_ms
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

SKIP_DIRS = {
    ".git",
    ".puppet-master",
    ".cursor",
    ".vscode",
    "__pycache__",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    "coverage",
    "dev-dist",
}

TOOL_REGISTRY = Path("packages/app/src-tauri/src/tool_registry.rs")
PROMPT_TEMPLATE = Path("scripts/librarian-prompt.md")


def git_sha(root: Path) -> str | None:
    try:
        out = subprocess.check_output(
            ["git", "rev-parse", "HEAD"],
            cwd=root,
            text=True,
            stderr=subprocess.DEVNULL,
        )
        return out.strip() or None
    except (subprocess.CalledProcessError, FileNotFoundError, OSError):
        return None


def read_json(path: Path) -> dict | None:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        return data if isinstance(data, dict) else None
    except (OSError, json.JSONDecodeError):
        return None


def readme_blurb(root: Path) -> str:
    readme = root / "README.md"
    if not readme.is_file():
        return ""
    lines = readme.read_text(encoding="utf-8", errors="replace").splitlines()
    body: list[str] = []
    for line in lines:
        stripped = line.strip()
        if not stripped:
            if body:
                break
            continue
        if stripped.startswith("[!") or stripped.startswith("![") or stripped.startswith("<"):
            continue
        if stripped.startswith("#"):
            if body:
                break
            continue
        body.append(stripped)
        if len(body) >= 4:
            break
    return " ".join(body)


def workspace_packages(root: Path) -> list[dict[str, str]]:
    root_pkg = read_json(root / "package.json") or {}
    workspaces = root_pkg.get("workspaces") or []
    packages: list[dict[str, str]] = []
    globs: list[str] = []
    for entry in workspaces:
        if isinstance(entry, str):
            globs.append(entry)
    for glob in globs:
        if glob.endswith("/*"):
            base = root / glob[:-2]
            if not base.is_dir():
                continue
            for child in sorted(base.iterdir()):
                pkg_json = child / "package.json"
                if not pkg_json.is_file():
                    continue
                data = read_json(pkg_json) or {}
                packages.append(
                    {
                        "name": str(data.get("name") or child.name),
                        "path": child.relative_to(root).as_posix(),
                        "description": str(data.get("description") or "").strip(),
                    }
                )
    return packages


def extract_mcp_tools(root: Path) -> list[str]:
    path = root / TOOL_REGISTRY
    if not path.is_file():
        return []
    text = path.read_text(encoding="utf-8", errors="replace")
    # ToolDefinition blocks only — avoid matching arbitrary name: strings.
    tools: list[str] = []
    for block in re.findall(
        r"ToolDefinition\s*\{[^}]*name:\s*\"([a-z][a-z0-9_]*)\"",
        text,
        flags=re.DOTALL,
    ):
        tools.append(block)
    return sorted(set(tools))


def doc_outline(root: Path, rel: str, max_sections: int = 6) -> list[str]:
    path = root / rel
    if not path.is_file():
        return []
    lines: list[str] = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("## "):
            lines.append(line[3:].strip())
        if len(lines) >= max_sections:
            break
    return lines


def top_level_dirs(root: Path) -> list[str]:
    names: list[str] = []
    try:
        for child in sorted(root.iterdir(), key=lambda p: p.name.lower()):
            if not child.is_dir():
                continue
            if child.name in SKIP_DIRS or child.name.startswith("."):
                continue
            names.append(child.name + "/")
    except OSError:
        pass
    return names


def build_static_context(root: Path) -> dict:
    root_pkg = read_json(root / "package.json") or {}
    return {
        "name": str(root_pkg.get("name") or root.name),
        "description": str(root_pkg.get("description") or "").strip(),
        "readme_blurb": readme_blurb(root),
        "packages": workspace_packages(root),
        "mcp_tools": extract_mcp_tools(root),
        "doc_sections": {
            "ROUTING.md": doc_outline(root, "ROUTING.md"),
            "ROADMAP.md": doc_outline(root, "ROADMAP.md"),
        },
        "top_level": top_level_dirs(root),
        "git_sha": git_sha(root),
    }


def format_static_overview(ctx: dict) -> str:
    parts: list[str] = []
    title = ctx.get("name") or "project"
    parts.append(f"# {title}")
    if ctx.get("description"):
        parts.append(str(ctx["description"]))
    if ctx.get("readme_blurb"):
        parts.append("")
        parts.append(str(ctx["readme_blurb"]))

    packages = ctx.get("packages") or []
    if packages:
        parts.append("")
        parts.append("## Packages")
        for pkg in packages:
            line = f"- **{pkg['name']}** (`{pkg['path']}`)"
            if pkg.get("description"):
                line += f" — {pkg['description']}"
            parts.append(line)

    tools = ctx.get("mcp_tools") or []
    if tools:
        parts.append("")
        parts.append("## MCP / bridge tools")
        parts.append(
            "Orchestrators call these over the HTTP bridge (same surface for sidebar + external MCP):"
        )
        parts.append(", ".join(tools))

    parts.append("")
    parts.append("## Architecture (high level)")
    parts.append(
        "- **Desktop app** (`packages/app`): Tauri + React UI, embedded HTTP bridge, PTY workers."
    )
    parts.append(
        "- **MCP server** (`packages/mcp-server`): stdio launcher → Rust MCP binary → bridge HTTP."
    )
    parts.append(
        "- **Coordination state**: project-local `.puppet-master/` (tasks, locks, audit, this index)."
    )
    parts.append(
        "- **Workers**: real PTY panes (Claude, Codex, OpenCode, bash) driven via `write_terminal_input` / `press_key`."
    )

    doc_sections = ctx.get("doc_sections") or {}
    for doc_name, sections in doc_sections.items():
        if not sections:
            continue
        parts.append("")
        parts.append(f"## {doc_name} topics")
        for section in sections:
            parts.append(f"- {section}")

    top = ctx.get("top_level") or []
    if top:
        parts.append("")
        parts.append("## Top-level directories")
        parts.append(", ".join(top))

    return "\n".join(parts).strip() + "\n"


def llm_prompt(ctx: dict) -> str:
    return f"""You are a codebase librarian (like DeepWiki / zread.ai). Write a developer-facing wiki page.

Requirements:
- Explain WHAT the project does and HOW it is structured (layers, packages, data flow).
- Name the orchestrator ↔ bridge ↔ worker pane relationship and MCP tool surface.
- Mention where coordination state lives and how agents should delegate work.
- Do NOT dump a file tree. No more than ~900 words.
- Use markdown headings (##) and bullet lists where helpful.

Structured signals (trust these over guessing):
{json.dumps(ctx, indent=2)[:12000]}
"""


def call_anthropic(api_key: str, prompt: str, model: str) -> str:
    body = json.dumps(
        {
            "model": model,
            "max_tokens": 2048,
            "messages": [{"role": "user", "content": prompt}],
        }
    ).encode("utf-8")
    req = urllib.request.Request(
        "https://api.anthropic.com/v1/messages",
        data=body,
        headers={
            "Content-Type": "application/json",
            "x-api-key": api_key,
            "anthropic-version": "2023-06-01",
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        payload = json.loads(resp.read().decode("utf-8"))
    blocks = payload.get("content") or []
    text = "".join(
        block.get("text", "")
        for block in blocks
        if isinstance(block, dict) and block.get("type") == "text"
    )
    if not text.strip():
        raise RuntimeError("empty LLM response")
    return text.strip() + "\n"


def call_openai(api_key: str, prompt: str, model: str) -> str:
    body = json.dumps(
        {
            "model": model,
            "max_tokens": 2048,
            "messages": [{"role": "user", "content": prompt}],
        }
    ).encode("utf-8")
    req = urllib.request.Request(
        "https://api.openai.com/v1/chat/completions",
        data=body,
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {api_key}",
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        payload = json.loads(resp.read().decode("utf-8"))
    choices = payload.get("choices") or []
    message = choices[0].get("message") if choices else {}
    text = str(message.get("content") or "").strip()
    if not text:
        raise RuntimeError("empty LLM response")
    return text + "\n"


def run_llm(ctx: dict, provider: str, model: str | None) -> str:
    prompt = llm_prompt(ctx)
    if provider == "openai":
        api_key = os.environ.get("OPENAI_API_KEY", "").strip()
        if not api_key:
            raise RuntimeError("OPENAI_API_KEY not set")
        return call_openai(api_key, prompt, model or "gpt-4o-mini")
    api_key = os.environ.get("ANTHROPIC_API_KEY", "").strip()
    if not api_key:
        raise RuntimeError("ANTHROPIC_API_KEY not set")
    return call_anthropic(api_key, prompt, model or "claude-haiku-4-5-20251001")


def emit_librarian_prompt(root: Path) -> str:
    template_path = root / PROMPT_TEMPLATE
    if not template_path.is_file():
        raise FileNotFoundError(f"missing librarian template: {template_path}")
    template = template_path.read_text(encoding="utf-8")
    ctx = build_static_context(root)
    context_json = json.dumps(ctx, indent=2)
    if "{{CONTEXT_JSON}}" not in template:
        raise ValueError("librarian-prompt.md missing {{CONTEXT_JSON}} placeholder")
    return template.replace("{{CONTEXT_JSON}}", context_json)


def validate_project_ir(root: Path) -> tuple[bool, str]:
    storage = root / ".puppet-master"
    ir_path = storage / "project-ir.json"
    meta_path = storage / "project-ir.meta.json"
    if not ir_path.is_file():
        return False, f"missing {ir_path}"
    try:
        ir = json.loads(ir_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as err:
        return False, f"invalid JSON in project-ir.json: {err}"
    overview = ir.get("overview")
    if not isinstance(overview, str) or not overview.strip():
        return False, "project-ir.json: overview must be a non-empty string"
    if meta_path.is_file():
        try:
            json.loads(meta_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as err:
            return False, f"invalid JSON in project-ir.meta.json: {err}"
    return True, "ok"


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build Puppet Master project IR")
    parser.add_argument("root", nargs="?", default=".", help="Project root directory")
    parser.add_argument(
        "--emit-prompt",
        action="store_true",
        help="Print OpenCode librarian prompt to stdout (for write_terminal_input)",
    )
    parser.add_argument(
        "--validate",
        action="store_true",
        help="Validate .puppet-master/project-ir.json and exit",
    )
    parser.add_argument(
        "--llm",
        action="store_true",
        help="Generate DeepWiki-style overview via direct API (needs API key in env)",
    )
    parser.add_argument(
        "--provider",
        choices=("anthropic", "openai"),
        default=os.environ.get("PUPPET_MASTER_LIBRARIAN_PROVIDER", "anthropic"),
    )
    parser.add_argument("--model", default=None, help="Override LLM model id")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    root = Path(args.root).resolve()
    if not root.is_dir():
        print(f"error: not a directory: {root}", file=sys.stderr)
        return 1

    if args.emit_prompt:
        try:
            sys.stdout.write(emit_librarian_prompt(root))
        except (OSError, ValueError) as err:
            print(f"error: {err}", file=sys.stderr)
            return 1
        return 0

    if args.validate:
        ok, message = validate_project_ir(root)
        if ok:
            print(message)
            return 0
        print(f"error: {message}", file=sys.stderr)
        return 1

    storage = root / ".puppet-master"
    storage.mkdir(parents=True, exist_ok=True)

    ctx = build_static_context(root)
    generator = "static-v2"
    overview = format_static_overview(ctx)

    if args.llm:
        try:
            overview = run_llm(ctx, args.provider, args.model)
            generator = f"llm-{args.provider}"
        except (urllib.error.URLError, RuntimeError, json.JSONDecodeError) as err:
            print(f"warning: LLM overview failed ({err}); using static-v2", file=sys.stderr)

    generated_at_ms = int(time.time() * 1000)
    ir = {
        "overview": overview,
        "sections": {
            "packages": ctx.get("packages") or [],
            "mcp_tools": ctx.get("mcp_tools") or [],
        },
        "generated_at_ms": generated_at_ms,
        "generator": generator,
    }
    ir_path = storage / "project-ir.json"
    ir_path.write_text(json.dumps(ir, indent=2), encoding="utf-8")

    meta = {
        "git_sha": ctx.get("git_sha"),
        "generated_at_ms": generated_at_ms,
        "generator": generator,
    }
    meta_path = storage / "project-ir.meta.json"
    meta_path.write_text(json.dumps(meta, indent=2), encoding="utf-8")

    print(f"wrote {ir_path} ({len(overview)} chars, generator={generator})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
