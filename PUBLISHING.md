# Publishing guide

Two packages are intended for npm publish:

1. **`@puppet-master/mcp`** — the stdio MCP server (consumed by Cursor, Claude Desktop, Codex).
2. **`puppet-master`** — the CLI that launches the GUI via Tauri's dev/build pipeline (the GUI itself is a desktop app and is *not* published to npm).

The GUI is distributed as a `.msi` installer built by Tauri.

## One-time setup

```bash
# log in once
npm login

# make sure the org scope exists (for @puppet-master)
npm access public @puppet-master/mcp
```

## Publish the MCP server

```bash
cd packages/mcp-server

# 1. ensure it's clean + builds
rm -rf dist
npm run build

# 2. bump + tag
npm version patch    # or minor / major

# 3. publish
npm publish --access public

# 4. (one-off) tell npm that the scope is public so unscoped pulls work
npm access public @puppet-master/mcp
```

The published package exposes:

- `dist/index.js` — bundled, ESM, runs on Node ≥22
- `README.md` — auto-pulled from this repo

Users install via `npx -y @puppet-master/mcp`.

## Publish the CLI

```bash
cd packages/cli

rm -rf dist
npm run build

npm version patch
npm publish --access public
```

After publishing, users can run `npx puppet-master` to launch the GUI.

## Tauri bundle (.msi)

```bash
# from repo root
npm install
npm run build:rust
# (this calls `tauri build`, which produces:)
ls packages/app/src-tauri/target/release/bundle/msi/
ls packages/app/src-tauri/target/release/bundle/nsis/
```

Output:
- `bundle/msi/Puppet Master_0.1.1_x64_en-US.msi` (Windows Installer)
- `bundle/nsis/Puppet Master_0.1.1_x64-setup.exe` (NSIS installer)

The bundle config in `packages/app/src-tauri/tauri.conf.json` controls:
- Product name: **Puppet Master**
- Identifier: `com.puppetmaster.app`
- Targets: `msi`, `nsis`
- Icon: `packages/app/src-tauri/icons/icon.ico`

## Pre-publish checklist

- [ ] `npm run typecheck` passes
- [ ] `npm run build` passes (all 5 packages)
- [ ] `cargo test --manifest-path packages/app/src-tauri/Cargo.toml` passes
- [ ] `pwsh scripts/test-bridge.ps1` passes (HTTP round-trip)
- [ ] `pwsh scripts/test-mcp.ps1` passes (MCP JSON-RPC round-trip)
- [ ] Bumped versions on the packages you're publishing
- [ ] README on each package reflects current API
- [ ] GitHub release created with the `.msi` attached
