# Puppet Master librarian task

You are the **project librarian** for this repository. Produce a **DeepWiki-style** overview: architecture, package roles, orchestrator ↔ bridge ↔ worker flows, and how MCP tools fit together. **Do not** dump a raw directory tree.

## Your deliverable

Write **exactly** these two files (create `.puppet-master/` if needed):

### 1. `.puppet-master/project-ir.json`

```json
{
  "overview": "<markdown string: 600–1200 words, wiki-style sections>",
  "sections": {
    "packages": [{ "name": "...", "path": "...", "role": "..." }],
    "mcp_tools": ["list_panes", "..."]
  },
  "generated_at_ms": <unix ms>,
  "generator": "opencode-librarian"
}
```

`overview` must include markdown headings such as:
- What this project does
- Architecture (desktop app, bridge, MCP, workers)
- Package map
- Typical orchestration flow
- Where coordination state lives (`.puppet-master/`)

### 2. `.puppet-master/project-ir.meta.json`

```json
{
  "git_sha": "<output of git rev-parse HEAD if available, else null>",
  "generated_at_ms": <same as above>,
  "generator": "opencode-librarian"
}
```

## How to work

1. Use your tools to read key files (README, ROUTING.md, `packages/*/package.json`, `tool_registry.rs`, `bridge.rs`, `puppet-master.ts`) — **verify** claims; do not invent modules.
2. Use the **context signals** below as hints only.
3. Write both JSON files with valid JSON (escape newlines in `overview` as `\n`).
4. When finished, reply with a single line: `LIBRARIAN_INDEX_COMPLETE`

## Context signals (pre-collected)

```json
{{CONTEXT_JSON}}
```
