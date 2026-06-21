#!/usr/bin/env node
import { copyFileSync, chmodSync, existsSync, mkdirSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const crateDir = join(repoRoot, 'packages', 'app', 'src-tauri');
const outDir = join(repoRoot, 'packages', 'mcp-server', 'dist');
const binaryName = process.platform === 'win32' ? 'puppet-master-mcp.exe' : 'puppet-master-mcp';
const profile = process.env.PUPPET_MASTER_RUST_MCP_PROFILE === 'debug' ? 'debug' : 'release';
const cargoBin = join(homedir(), '.cargo', 'bin');
const cargoExe = join(cargoBin, process.platform === 'win32' ? 'cargo.exe' : 'cargo');
const pathKey = process.platform === 'win32' ? 'Path' : 'PATH';
const sep = process.platform === 'win32' ? ';' : ':';
const env = { ...process.env };

if (existsSync(cargoExe)) {
  env[pathKey] = `${cargoBin}${sep}${env[pathKey] ?? ''}`;
}

const cargoArgs = ['build', '--bin', 'puppet-master-mcp'];
if (profile === 'release') {
  cargoArgs.push('--release');
}

const result = spawnSync('cargo', cargoArgs, {
  cwd: crateDir,
  stdio: 'inherit',
  env,
  shell: process.platform === 'win32',
});

if (result.status !== 0) {
  process.exit(result.status ?? 1);
}

mkdirSync(outDir, { recursive: true });
const source = join(crateDir, 'target', profile, binaryName);
const target = join(outDir, binaryName);

function sleep(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

function copyWithRetry(from, to) {
  let lastError;
  for (let attempt = 0; attempt < 8; attempt++) {
    try {
      copyFileSync(from, to);
      return;
    } catch (err) {
      lastError = err;
      if (!['EBUSY', 'EPERM', 'EACCES'].includes(err?.code)) {
        throw err;
      }
      sleep(125 * (attempt + 1));
    }
  }
  throw lastError;
}

copyWithRetry(source, target);
if (process.platform !== 'win32') {
  chmodSync(target, 0o755);
}
console.error(`[build-rust-mcp] copied ${source} -> ${target}`);
