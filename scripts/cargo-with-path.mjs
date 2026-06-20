#!/usr/bin/env node
/**
 * Run cargo in the Tauri crate with ~/.cargo/bin prepended to PATH.
 */
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const tauriRoot = join(repoRoot, 'packages', 'app', 'src-tauri');
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
    `[cargo-with-path] cargo not found at ${cargoExe}\n` +
      'Install Rust from https://rustup.rs/ then reopen your terminal.',
  );
  process.exit(1);
}

const result = spawnSync(cargoExe, process.argv.slice(2), {
  stdio: 'inherit',
  env,
  cwd: tauriRoot,
  shell: false,
});

process.exit(result.status === null ? 1 : result.status);
