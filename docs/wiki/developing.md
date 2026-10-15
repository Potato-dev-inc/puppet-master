# Developing

Windows is the primary dev platform. Cargo is expected at `%USERPROFILE%\.cargo\bin`; root scripts wrap PATH via `scripts/with-cargo-path.mjs`. Commands below are from the **repo root** unless noted.

## Prerequisites

| Tool | Notes |
|------|--------|
| Node 22 | `engines` in root `package.json` |
| Rust 1.96+ | `rustup` |
| MSVC 14.51+ / Win SDK (Windows), Xcode CLT (macOS), `build-essential` (Linux) | Tauri native build |
| Agent CLIs on PATH | `claude`, `codex`, `opencode`, `cursor-agent` (or `cursor-agent.ps1` on Windows) |

Local editor/MCP config is **not** committed (`.cursor/`, `.mcp.json`, …). Start from `.mcp.json.example` and [MCP_HOSTS.md](../../MCP_HOSTS.md).

## Daily loop

```bash
npm install
npm run dev          # tauri dev → Vite on 1420 + embedded bridge
```

**Tauri rebuilds and restarts the whole app on every `src-tauri` edit.** The live bridge dies for about a minute. Poll `GET /health` and wait for a new `build_id` before talking to MCP again.

Do not edit `src-tauri` while `scripts/pm-regression.mjs` or a live worker run is in progress — the restart corrupts the run.

Debug builds re-stamp `PM_BUILD_ID` only when sources or the git ref change (`PM_FORCE_NEW_BUILD_ID=1` to force). Release builds always re-stamp.

## Tests

```bash
# Rust — main suite
cd packages/app/src-tauri && cargo test --lib
cd packages/app/src-tauri && cargo test --lib tui::
cd packages/app/src-tauri && cargo test --lib agent_runs::
cd packages/app/src-tauri && cargo test --bin puppet-master-mcp

# JS
npm test
npm run typecheck

# MCP shim vs mock bridge (no paid workers)
npm run test:mcp-operations
npm run test:mcp-package
```

Tests that touch Tauri **window** APIs crash the test binary. Gate that code with `cfg(test)` stubs (see `pane_windows.rs`). Handle names in tests must be unique — `resolve_agent_run` searches a process-wide index.

Live regression (real app, real backends, costs money):

```bash
node scripts/pm-regression.mjs --backends opencode_native,cursor_agent --only pong
```

No `--help`. Read the header comment in the script. `--dry-run` prints a bench plan and spawns nothing.

## MCP exe after catalog or shim changes

Any change to `tool_registry.rs`, `bin/puppet-master-mcp.rs`, or bridge types the shim depends on needs a **new MCP binary**. `node scripts/build-rust-mcp.mjs` copies to `packages/mcp-server/dist/puppet-master-mcp.exe` and fails with **Access is denied** while another session has the exe open.

Do **not** kill other MCP processes. Build into a separate target dir, then swap by rename (Windows allows renaming a running exe):

```bash
# in packages/app/src-tauri, CARGO_TARGET_DIR = a scratch folder
cargo build --release --bin puppet-master-mcp

# swap
mv packages/mcp-server/dist/puppet-master-mcp.exe packages/mcp-server/dist/puppet-master-mcp.old.exe
cp <scratch>/release/puppet-master-mcp.exe packages/mcp-server/dist/puppet-master-mcp.exe
```

Then the user reconnects MCP (`/mcp` in Cursor). Host config points at that `dist` exe.

## CLI `watch` bundle

`node scripts/bundle-mcp.mjs` esbuilds `packages/cli` into `packages/app/src-tauri/resources/puppet-master-cli.bundle.mjs`. The app copies it to `%APPDATA%\com.puppetmaster.app\cli\` when size/mtime differ. An already-running watcher keeps old code. `PUPPET_MASTER_CLI` overrides the entry.

## `build_id` vs `catalog_version`

| | `build_id` | `catalog_version` |
|---|------------|-------------------|
| What | `<short sha>[-dirty]@<unix seconds>` from `build.rs` | `<tool count>-<16 hex>` hash of the tool catalog |
| Where | Lib / GUI (`GET /health`, `bridge_health`). **Not** in the MCP exe. | Both shim and bridge |
| Drift | Stale GUI vs the commit you think you shipped | `SCHEMA_DRIFT` if shim and app catalogs differ |

Commit finished work so the sha is current, rebuild, tell the user the expected sha, and have them **restart the app**. Hand-offs for live retest must carry a fresh `build_id`.

## Invariants you should not break

Full list: [REPO_MAP.md § 5](../REPO_MAP.md). The ones that bite during development:

- Generic code never matches agent-type strings — add a `TuiProfile` method instead.
- Persist the operation before any spawn.
- Warnings never change status.
- `status: interrupted` means cancelled turn; leave the name alone.
- Do not close a pane the run did not spawn.
- OpenCode yes/no: API (`answer_prompt`), not `press_key`.

## Release build

```bash
npm run build:rust
```

Installers land under `packages/app/src-tauri/target/release/bundle/`. `SKIP_RELEASE_UPLOAD=1` skips GitHub upload.
