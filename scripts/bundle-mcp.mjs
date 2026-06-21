#!/usr/bin/env node
/**
 * Bundle @puppet-master/mcp into a single Node script for the Tauri app bundle.
 * Production Codex/Claude/OpenCode configs invoke this file — not `npx @puppet-master/mcp`
 * (that package is not published to npm).
 */
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import esbuild from 'esbuild';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outfile = resolve(root, 'packages/app/src-tauri/resources/mcp-stdio.bundle.cjs');
const entry = resolve(root, 'packages/mcp-server/src/index.ts');
const binName = process.platform === 'win32' ? 'puppet-master-mcp.exe' : 'puppet-master-mcp';
const mcpDistBinary = resolve(root, 'packages/mcp-server/dist', binName);
const resourceBin = resolve(root, 'packages/app/src-tauri/resources/bin');
const resourceBinary = resolve(resourceBin, binName);

function run(cmd, args) {
  const result = spawnSync(cmd, args, { cwd: root, stdio: 'inherit', shell: true });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

run('npm', ['run', 'build', '-w', '@puppet-master/shared']);

const mcpDist = resolve(root, 'packages/mcp-server/dist/index.js');
if (!existsSync(mcpDist)) {
  const mcpBuild = spawnSync('npm', ['run', 'build', '-w', '@puppet-master/mcp'], {
    cwd: root,
    stdio: 'inherit',
    shell: true,
  });
  if (mcpBuild.status !== 0) {
    console.error('[bundle-mcp] @puppet-master/mcp build failed — bundling from source via esbuild');
  }
}

run('node', ['scripts/build-rust-mcp.mjs']);
if (existsSync(mcpDistBinary)) {
  mkdirSync(resourceBin, { recursive: true });
  copyFileSync(mcpDistBinary, resourceBinary);
  if (process.platform !== 'win32') {
    chmodSync(resourceBinary, 0o755);
  }
  console.error(`[bundle-mcp] staged Rust MCP binary ${resourceBinary}`);
}

mkdirSync(dirname(outfile), { recursive: true });

try {
  await esbuild.build({
    entryPoints: [entry],
    bundle: true,
    platform: 'node',
    target: 'node22',
    format: 'cjs',
    outfile,
    logLevel: 'info',
  });
  console.error(`[bundle-mcp] wrote ${outfile}`);
} catch (err) {
  if (existsSync(outfile)) {
    console.error('[bundle-mcp] esbuild failed — keeping existing mcp-stdio.bundle.cjs');
  } else {
    throw err;
  }
}
