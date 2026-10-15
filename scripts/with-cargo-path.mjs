#!/usr/bin/env node
/**
 * Run npm tauri with ~/.cargo/bin prepended to PATH.
 * Fixes "cargo metadata: program not found" when rustup didn't update the shell PATH.
 */
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ensureWorkspaceBuilt } from './ensure-workspace-built.mjs';
import { ensureTauriResources } from './ensure-tauri-resources.mjs';

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

if (args[0] === 'dev') {
  ensureWorkspaceBuilt(repoRoot);
  ensureTauriResources();
  // Optional, gitignored per-machine window overrides (position, focus) for dev runs.
  const localConfig = join(repoRoot, 'packages', 'app', 'src-tauri', 'tauri.dev.local.json');
  if (existsSync(localConfig) && !args.includes('--config') && !args.includes('-c')) {
    args.push('--config', localConfig);
    console.error(`[puppet-master] using ${localConfig}`);
  }
  // Panes-first has no main window to anchor pop-outs to: the dev anchor (logical x/y of the
  // secondary monitor) comes from a gitignored per-machine file and reaches Rust as PM_WINDOW_ANCHOR.
  const anchorFile = join(repoRoot, 'packages', 'app', 'src-tauri', 'pm-dev.local.json');
  if (existsSync(anchorFile) && !env.PM_WINDOW_ANCHOR) {
    try {
      const anchor = JSON.parse(readFileSync(anchorFile, 'utf8')).window_anchor;
      if (Number.isFinite(anchor?.x) && Number.isFinite(anchor?.y)) {
        env.PM_WINDOW_ANCHOR = `${anchor.x},${anchor.y}`;
        console.error(`[puppet-master] window anchor ${env.PM_WINDOW_ANCHOR} (pm-dev.local.json)`);
      }
    } catch (err) {
      console.error(`[puppet-master] ignoring ${anchorFile}: ${err.message}`);
    }
  }
  console.error('[puppet-master] starting dev stack (vite + tauri + embedded bridge)…');
}

const result = spawnSync(npm, ['run', 'tauri', '--workspace=@puppet-master/app', '--', ...args], {
  stdio: 'inherit',
  env,
  cwd: repoRoot,
  shell: true,
});

const exitCode = result.status === null ? 1 : result.status;

if (args[0] === 'build' && exitCode === 0) {
  const upload = spawnSync(process.execPath, [join(repoRoot, 'scripts', 'publish-github-release-assets.mjs')], {
    stdio: 'inherit',
    env,
    cwd: repoRoot,
    shell: false,
  });
  const uploadCode = upload.status === null ? 1 : upload.status;
  process.exit(uploadCode);
}

process.exit(exitCode);
