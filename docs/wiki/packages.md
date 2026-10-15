# Packages

npm workspaces under `packages/*`. There is **no** `packages/bridge` — the bridge is Rust inside the Tauri crate. Older READMEs that list a bridge package are stale.

```
puppet-master/
  package.json                 workspaces + root scripts
  packages/
    app/                       Tauri 2 desktop + PWA
      src/                     React (App, PwaApp, TerminalApp, components, hooks, lib)
      src-tauri/               Rust crate puppet_master_app / lib puppet_master_app_lib
        src/bin/puppet-master-mcp.rs   MCP stdio shim (same crate)
        resources/             staged mcp exe, CLI bundle, PWA dist, cloudflared
    mcp-server/                @puppet-master/mcp — launcher + dist/*.exe
    cli/                       @puppet-master/cli — puppet-master bin (gui, mcp, worker, watch)
    shared/                    Zod / TS types used by app + cli + mcp-server
  docs/                        wiki, orchestrator playbooks, design notes, REPO_MAP
  scripts/                     build-rust-mcp, bundle-mcp, pm-regression, live probes
```

Gitignored: `dist/`, `target/`, `gen/`, `node_modules/`. `.claude/worktrees/` is agent scratch.

## `packages/app`

The product. UI talks to Rust via Tauri invoke + the same HTTP bridge the MCP shim uses.

| Path | Role |
|------|------|
| `src/App.tsx` | Desktop shell |
| `src/PwaApp.tsx` | Mobile mirror |
| `src/TerminalApp.tsx` | Standalone worker window |
| `src/lib/bridge.ts`, `mcp-tools.ts`, `pane-state.ts` | Client of the bridge |
| `src-tauri/src/lib.rs` | Module tree, Tauri setup, starts the bridge |
| `src-tauri/src/main.rs` | `run()` only |
| `src-tauri/build.rs` | Stamps `PM_BUILD_ID`, then `tauri_build` |

Rust module index: [REPO_MAP.md § 3](../REPO_MAP.md).

## `packages/mcp-server`

Publishes as `@puppet-master/mcp`. `src/launcher.ts` prefers the Rust binary in `dist/`. `src/index.ts` is the legacy TypeScript fallback (still loads the catalog **from the bridge**, not a second list).

The user's MCP host should point at `packages/mcp-server/dist/puppet-master-mcp.exe` (or `npx -y @puppet-master/mcp` for published installs). After catalog or shim changes, rebuild that exe — [developing.md](developing.md).

## `packages/cli`

`puppet-master` commands: `gui`, `mcp`, `worker`, `watch`, `version`. `watch` is the supported one-shot wait for a background operation (long-poll, not a status loop). The app stages a self-contained bundle `puppet-master-cli.bundle.mjs` into app data so watchers survive `cargo clean`.

## `packages/shared`

Protocol schemas (`protocol.ts`), agent enums (`agents.ts`), bridge port file helpers, mobile pairing types. If you add an `AgentType` in Rust, mirror it here.

## Scripts worth knowing

| Script | Does |
|--------|------|
| `scripts/with-cargo-path.mjs` | Puts `~/.cargo/bin` on PATH for `npm run tauri` / `build:rust` |
| `scripts/build-rust-mcp.mjs` | Release-build MCP exe → `mcp-server/dist` |
| `scripts/bundle-mcp.mjs` | Stage exe + esbuild CLI / stdio bundles into Tauri resources |
| `scripts/pm-regression.mjs` | **Live** MCP vs running app (costs time/money). No `--help`; flags in the file header. |
| `scripts/test-mcp-operations.mjs` | Shim vs mock bridge (no real workers) |

Python helpers (`spawn-close-agent.py`, `project-indexer.py`, `qt_mcp/`) are probes and aux UIs, not the product.

## Docs map

| Tree | Audience |
|------|----------|
| `docs/wiki/` | This handbook (product + how to keep developing) |
| `docs/orchestrator/` | External MCP orchestrators driving workers |
| `docs/plans/` | Design notes (ownership, watcher contract, pane close, parity) |
| `docs/REPO_MAP.md` | File/function map for coding agents |
| Root `README.md`, `MCP_HOSTS.md`, `ROUTING.md`, `ROADMAP.md` | Product landing, host setup, sidebar backends, future work |
