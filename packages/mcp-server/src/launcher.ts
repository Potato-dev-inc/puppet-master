#!/usr/bin/env node
/**
 * Thin launcher for the Rust Puppet Master MCP server.
 *
 * Keep this file intentionally boring: MCP JSON-RPC belongs to the Rust
 * binary. This shim only makes npm/npx distribution work across platforms.
 */

import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const binaryName = process.platform === 'win32' ? 'puppet-master-mcp.exe' : 'puppet-master-mcp';
const rustBinary = join(here, binaryName);
const legacyServer = join(here, 'legacy.js');

const command = existsSync(rustBinary) ? rustBinary : process.execPath;
const args = existsSync(rustBinary) ? process.argv.slice(2) : [legacyServer, ...process.argv.slice(2)];

if (!existsSync(rustBinary)) {
  process.stderr.write(
    `[puppet-master-mcp] Rust binary missing at ${rustBinary}; falling back to legacy TypeScript MCP server.\n`,
  );
}

const child = spawn(command, args, {
  stdio: 'inherit',
  env: process.env,
  windowsHide: true,
});

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});

child.on('error', (err) => {
  process.stderr.write(`[puppet-master-mcp] failed to start: ${err.message}\n`);
  process.exit(1);
});
