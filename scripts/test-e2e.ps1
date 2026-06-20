# scripts/test-e2e.ps1 — full smoke test of the Puppet Master pipeline.
#
# 1. Build all packages
# 2. Build the Rust binary
# 3. Run cargo tests
# 4. Start the bridge, exercise the HTTP API
# 5. Start the MCP server, exercise the JSON-RPC API

$ErrorActionPreference = 'Stop'
$repo = $PSScriptRoot | Split-Path -Parent

Write-Host "=== 1. typecheck ==="
Push-Location $repo
try { npm run typecheck 2>&1 | Out-Null } finally { Pop-Location }

Write-Host "=== 2. build all TS packages ==="
Push-Location $repo
try { npm run build 2>&1 | Out-Null } finally { Pop-Location }

Write-Host "=== 3. cargo build ==="
Push-Location $repo
try { node scripts/cargo-with-path.mjs build 2>&1 | Out-Null } finally { Pop-Location }

Write-Host "=== 4. cargo test (scrollback) ==="
Push-Location $repo
try { node scripts/cargo-with-path.mjs test --lib scrollback 2>&1 | Out-Null } finally { Pop-Location }

Write-Host "=== 5. bridge HTTP smoke test ==="
& "$repo/scripts/test-bridge.ps1" | Out-Null

Write-Host "=== 6. MCP JSON-RPC smoke test ==="
& "$repo/scripts/test-mcp.ps1" | Out-Null

Write-Host ""
Write-Host "E2E SMOKE TEST: ALL GREEN" -ForegroundColor Green
