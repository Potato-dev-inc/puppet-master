#!/usr/bin/env node
/**
 * Bundle @puppet-master/mcp into a single Node script for the Tauri app bundle.
 * Production Codex/Claude/OpenCode configs invoke this file — not `npx @puppet-master/mcp`
 * (that package is not published to npm).
 */
import { spawnSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import esbuild from 'esbuild';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outfile = resolve(root, 'packages/app/src-tauri/resources/mcp-stdio.bundle.cjs');
const entry = resolve(root, 'packages/mcp-server/src/index.ts');

function run(cmd, args) {
  const result = spawnSync(cmd, args, { cwd: root, stdio: 'inherit', shell: true });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

run('npm', ['run', 'build', '-w', '@puppet-master/shared']);
run('npm', ['run', 'build', '-w', '@puppet-master/mcp']);

mkdirSync(dirname(outfile), { recursive: true });

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
