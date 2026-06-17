#!/usr/bin/env node
/**
 * Run npm tauri with ~/.cargo/bin prepended to PATH.
 * Fixes "cargo metadata: program not found" when rustup didn't update the shell PATH.
 */
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const cargoBin = join(homedir(), '.cargo', 'bin');
const cargoExe = join(cargoBin, process.platform === 'win32' ? 'cargo.exe' : 'cargo');
const pathKey = process.platform === 'win32' ? 'Path' : 'PATH';
const sep = process.platform === 'win32' ? ';' : ':';

const env = { ...process.env };

if (existsSync(cargoExe)) {
  const current = env[pathKey] ?? '';
  const parts = current.split(sep).filter(Boolean);
  const already = parts.some((p) => p.toLowerCase() === cargoBin.toLowerCase());
  if (!already) {
    env[pathKey] = `${cargoBin}${sep}${current}`;
  }
} else {
  console.error(
    `[with-cargo-path] cargo not found at ${cargoExe}\n` +
      'Install Rust from https://rustup.rs/ then reopen your terminal.',
  );
  process.exit(1);
}

const args = process.argv.slice(2);
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const result = spawnSync(npm, ['run', 'tauri', '--workspace=@puppet-master/app', ...args], {
  stdio: 'inherit',
  env,
  cwd: repoRoot,
  shell: true,
});

process.exit(result.status === null ? 1 : result.status);
