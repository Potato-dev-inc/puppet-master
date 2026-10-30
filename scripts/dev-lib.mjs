// Shared helpers for the local check scripts (`check.mjs`, `test-changed.mjs`).
// Nothing here is read by the app or by CI.

import { spawn, spawnSync } from 'node:child_process';
import { createWriteStream, existsSync, mkdirSync, readFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ensureWorkspaceBuilt } from './ensure-workspace-built.mjs';

export const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const appDir = join(repoRoot, 'packages', 'app');
export const tauriDir = join(appDir, 'src-tauri');
export const isWindows = process.platform === 'win32';

/** Environment for cargo: ~/.cargo/bin on PATH, and no bundle resources (tests do not read them; CI does the same). */
export function cargoEnv(extra = {}) {
  const env = { ...process.env, ...extra };
  const key = isWindows ? 'Path' : 'PATH';
  const sep = isWindows ? ';' : ':';
  const cargoBin = join(homedir(), '.cargo', 'bin');
  const current = env[key] ?? env.PATH ?? env.Path ?? '';
  if (existsSync(cargoBin) && !current.split(sep).some((p) => p.toLowerCase() === cargoBin.toLowerCase())) {
    env[key] = `${cargoBin}${sep}${current}`;
  }
  // `tauri-build` insists that the bundle's resource globs match files; the unit tests never read them.
  if (!env.TAURI_CONFIG) env.TAURI_CONFIG = '{"bundle":{"resources":[]}}';
  return env;
}

/** Where the logs of one run go: PM_CHECK_LOGS, or a folder under the OS temp dir named after this checkout. */
export function logDir(name) {
  const base = process.env.PM_CHECK_LOGS || join(tmpdir(), 'pm-check', name || 'default');
  mkdirSync(base, { recursive: true });
  return base;
}

/** A short, filesystem-safe name for this checkout (its folder name). */
export function checkoutName() {
  return repoRoot.split(/[\\/]/).filter(Boolean).pop().replace(/[^A-Za-z0-9_.-]/g, '_');
}

/**
 * Run one command to the end with its output in a log file (and nothing on the terminal).
 * Resolves to { code, seconds, log, tail(n) } and never rejects.
 */
export function runLogged(label, cmd, args, { cwd = repoRoot, env = process.env, logPath } = {}) {
  return new Promise((done) => {
    const started = Date.now();
    const out = createWriteStream(logPath);
    out.write(`$ ${cmd} ${args.join(' ')}\n(cwd ${cwd})\n\n`);
    // .cmd shims (npm, npx) need a shell on Windows; the arguments here never contain user input.
    const child = spawn(cmd, args, { cwd, env, shell: isWindows && /\.cmd$|^(npm|npx)$/.test(cmd), windowsHide: true });
    child.stdout.on('data', (d) => out.write(d));
    child.stderr.on('data', (d) => out.write(d));
    const finish = (code) => {
      out.end(() => {
        done({
          label,
          code,
          seconds: (Date.now() - started) / 1000,
          log: logPath,
          lines: () => readFileSync(logPath, 'utf8').replace(/\x1b\[[0-9;]*m/g, '').split(/\r?\n/),
        });
      });
    };
    child.on('error', (e) => {
      out.write(`spawn error: ${e.message}\n`);
      finish(127);
    });
    child.on('close', (code) => finish(code ?? 1));
  });
}

/** Run a command with its output on the terminal. Returns the exit code. */
export function runLoud(cmd, args, { cwd = repoRoot, env = process.env } = {}) {
  const r = spawnSync(cmd, args, { cwd, env, stdio: 'inherit', shell: isWindows && /^(npm|npx)$/.test(cmd) });
  return r.status ?? 1;
}

export const npm = isWindows ? 'npm.cmd' : 'npm';
export const npx = isWindows ? 'npx.cmd' : 'npx';

export function git(args, opts = {}) {
  const r = spawnSync('git', args, { cwd: repoRoot, encoding: 'utf8', maxBuffer: 1 << 26, ...opts });
  return r.status === 0 ? r.stdout : null;
}

/** Make sure what the tests compile against exists. Cheap when it does. Returns a list of what it did. */
export function prepare({ install = false } = {}) {
  const did = [];
  if (install || !existsSync(join(repoRoot, 'node_modules'))) {
    did.push('npm ci');
    if (runLoud(npm, ['ci']) !== 0) throw new Error('npm ci failed');
  }
  // @puppet-master/app imports @puppet-master/shared through its dist/ output (rebuilt when its sources are newer).
  ensureWorkspaceBuilt(repoRoot);
  // tauri::generate_context! embeds packages/app/dist (frontendDist). Any build of it will do for the tests.
  if (!existsSync(join(appDir, 'dist', 'index.html'))) {
    did.push('vite build');
    if (runLoud(npx, ['vite', 'build'], { cwd: appDir }) !== 0) throw new Error('vite build failed');
  }
  return did;
}

/** The last lines of a log that say how a lane went: test result lines and the first compile errors. */
export function summarize(lines, patterns) {
  const hit = lines.filter((l) => patterns.some((p) => p.test(l)));
  return hit.slice(0, 12);
}

export function fmt(seconds) {
  return seconds >= 60 ? `${Math.floor(seconds / 60)}m${String(Math.round(seconds % 60)).padStart(2, '0')}s` : `${seconds.toFixed(1)}s`;
}
