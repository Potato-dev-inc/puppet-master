#!/usr/bin/env node
import { chmodSync, copyFileSync, existsSync, mkdirSync, renameSync, unlinkSync } from 'node:fs';
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

if (!existsSync(cargoExe)) {
  console.error(
    `[build-rust-mcp] cargo not found at ${cargoExe} — skipping Rust MCP binary\n` +
      'Install Rust from https://rustup.rs/ or use the legacy TypeScript MCP server.',
  );
  process.exit(0);
}

const current = env[pathKey] ?? '';
const parts = current.split(sep).filter(Boolean);
const already = parts.some((p) => p.toLowerCase() === cargoBin.toLowerCase());
if (!already) {
  env[pathKey] = `${cargoBin}${sep}${current}`;
}

const cargoArgs = ['build', '--bin', 'puppet-master-mcp'];
if (profile === 'release') {
  cargoArgs.push('--release');
}

const result = spawnSync(cargoExe, cargoArgs, {
  cwd: crateDir,
  stdio: 'inherit',
  env,
  shell: false,
});

if (result.status !== 0) {
  console.error('[build-rust-mcp] cargo build failed — legacy TypeScript MCP server remains available');
  process.exit(0);
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

function stageBinary(from, to) {
  const temp = `${to}.${process.pid}.tmp`;
  copyWithRetry(from, temp);

  let lastError;
  for (let attempt = 0; attempt < 8; attempt++) {
    try {
      if (existsSync(to)) {
        unlinkSync(to);
      }
      renameSync(temp, to);
      return;
    } catch (err) {
      lastError = err;
      if (!['EBUSY', 'EPERM', 'EACCES', 'ENOENT'].includes(err?.code)) {
        throw err;
      }
      sleep(125 * (attempt + 1));
    }
  }

  if (existsSync(temp)) {
    try {
      unlinkSync(temp);
    } catch {
      // best-effort cleanup
    }
  }

  throw lastError;
}

try {
  stageBinary(source, target);
  if (process.platform !== 'win32') {
    chmodSync(target, 0o755);
  }
  console.error(`[build-rust-mcp] copied ${source} -> ${target}`);
} catch (err) {
  if (existsSync(target)) {
    console.error(
      `[build-rust-mcp] could not refresh ${target} (${err?.code ?? err?.message}) — keeping existing binary`,
    );
    process.exit(0);
  }
  console.error(
    `[build-rust-mcp] could not stage Rust MCP binary (${err?.code ?? err?.message}) — legacy TypeScript MCP server remains available`,
  );
  process.exit(0);
}
