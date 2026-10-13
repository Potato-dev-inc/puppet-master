#!/usr/bin/env node
// UI evidence: renders harness scenarios (packages/app/src/dev/scenarios) in headless Edge.
// Usage: node scripts/evidence/screenshot.mjs --out <dir> [--port <p>] [scenario...]
// Headless only: no visible window is ever opened.
// The dev server gets a free port (or --port <p>) and is verified to be THIS worktree's server
// by a per-run token that vite injects into every module it serves. Other worktrees run their own
// vite at the same time, so a bare "something answers on the port" check is not enough.
import { spawn, spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const EDGE = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const appDir = path.join(root, 'packages/app');
const scenarioDir = path.join(appDir, 'src/dev/scenarios');
const USAGE = 'usage: node scripts/evidence/screenshot.mjs --out <dir> [--port <p>] [scenario...]';

const argv = process.argv.slice(2);
let out = null;
let portArg = null;
const wanted = [];
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === '--out') out = argv[++i];
  else if (argv[i] === '--port') portArg = argv[++i];
  else wanted.push(argv[i]);
}
if (!out) {
  console.error(USAGE);
  process.exit(2);
}
if (portArg !== null && !(/^\d+$/.test(portArg) && +portArg >= 1 && +portArg <= 65535)) {
  console.error(`invalid --port ${portArg}\n${USAGE}`);
  process.exit(2);
}
out = path.resolve(out);
mkdirSync(out, { recursive: true });

// Scenario names = the `name:` of each file; file basenames match by convention.
const all = readdirSync(scenarioDir)
  .filter((f) => f.endsWith('.ts') && f !== 'index.ts')
  .map((f) => f.replace(/\.ts$/, ''));
const scenarios = wanted.length ? wanted : all;
for (const s of scenarios) {
  if (!all.includes(s)) console.warn(`warning: no scenarios/${s}.ts (names must match file names)`);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** A port that was free a moment ago (listen on 0, read the assigned port, close). */
function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.unref();
    srv.on('error', reject);
    srv.listen(0, 'localhost', () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

const PORT = portArg !== null ? Number(portArg) : await freePort();
const base = `http://localhost:${PORT}`;
const TOKEN = randomBytes(16).toString('hex');
// vite serves every module with `import.meta.env = {..., "VITE_PM_EVIDENCE_TOKEN": "<token>"}` at the top.
const TOKEN_NEEDLE = `"VITE_PM_EVIDENCE_TOKEN": "${TOKEN}"`;

const require = createRequire(path.join(appDir, 'package.json'));
const viteBin = path.join(path.dirname(require.resolve('vite/package.json')), 'bin/vite.js');
const vite = spawn(process.execPath, [viteBin, '--port', String(PORT), '--strictPort', '--host', 'localhost'], {
  cwd: appDir,
  env: { ...process.env, VITE_PM_EVIDENCE_TOKEN: TOKEN },
  stdio: ['ignore', 'pipe', 'pipe'],
  windowsHide: true,
});
let viteLog = '';
vite.stdout.on('data', (d) => (viteLog += d));
vite.stderr.on('data', (d) => (viteLog += d));
let viteExited = false;
vite.on('exit', () => (viteExited = true));

function stopVite() {
  if (viteExited) return;
  viteExited = true;
  if (process.platform === 'win32') spawnSync('taskkill', ['/pid', String(vite.pid), '/T', '/F'], { windowsHide: true });
  else vite.kill();
}

/** Confirm the server on PORT is the vite this script started (token present), not another worktree's. */
async function verifyOwnServer(timeoutMs = 90_000) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end && !viteExited) {
    try {
      const res = await fetch(`${base}/src/main.tsx`, { signal: AbortSignal.timeout(5_000) });
      if (res.ok && (await res.text()).includes(TOKEN_NEEDLE)) return true;
    } catch {
      /* not up yet */
    }
    await sleep(500);
  }
  return false;
}

async function waitForPng(file, timeoutMs) {
  const end = Date.now() + timeoutMs;
  let last = -1;
  while (Date.now() < end) {
    if (existsSync(file)) {
      const size = statSync(file).size;
      if (size > 0 && size === last) return true;
      last = size;
    }
    await sleep(700);
  }
  return false;
}

/** Kill the headless Edge processes started with this temp --user-data-dir (never anyone else's). */
function killProfile(profile) {
  if (process.platform !== 'win32') return;
  const needle = profile.replace(/'/g, "''");
  spawnSync(
    'powershell',
    [
      '-NoProfile',
      '-Command',
      `Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" | Where-Object { $_.CommandLine -like '*${needle}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`,
    ],
    { windowsHide: true, stdio: 'ignore' },
  );
}

// Temp Edge profiles still alive. Anything left here is killed and removed on exit.
const liveProfiles = new Set();

function cleanupProfile(profile) {
  killProfile(profile);
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
  liveProfiles.delete(profile);
}

// Single cleanup point: every exit path (normal, process.exit, SIGINT/SIGTERM, uncaught error) runs this.
process.on('exit', () => {
  for (const profile of liveProfiles) {
    try {
      cleanupProfile(profile);
    } catch {
      /* best effort */
    }
  }
  stopVite();
});
for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, () => process.exit(sig === 'SIGINT' ? 130 : 143));
process.on('uncaughtException', (err) => {
  console.error(err);
  process.exit(1);
});
process.on('unhandledRejection', (err) => {
  console.error(err);
  process.exit(1);
});

let failed = 0;
if (!(await verifyOwnServer()) || viteExited) {
  console.error(`vite did not become ready with this run's token on ${PORT}\n` + viteLog.slice(-2000));
  process.exit(1);
}

const shoot = async (name, png) => {
  let ok = false;
  for (let attempt = 1; attempt <= 3 && !ok; attempt++) {
    rmSync(png, { force: true });
    const profile = mkdtempSync(path.join(tmpdir(), 'pm-evidence-'));
    liveProfiles.add(profile);
    try {
      const args = [
        '--headless=new',
        '--disable-gpu',
        '--hide-scrollbars',
        '--force-dark-mode',
        '--window-size=1440,900',
        '--virtual-time-budget=5000',
        `--user-data-dir=${profile}`,
        `--screenshot=${png}`,
        `${base}/?harness=${encodeURIComponent(name)}`,
      ];
      // msedge.exe hands off to a detached browser process and returns at once; the png is written
      // later, so poll for it (stable size), then kill that profile's processes.
      spawnSync(EDGE, args, { windowsHide: true, timeout: 60_000, stdio: 'ignore' });
      ok = await waitForPng(png, 45_000);
    } finally {
      await sleep(500);
      cleanupProfile(profile);
    }
    if (!ok) console.warn(`${name}: no png (attempt ${attempt}/3)`);
  }
  return ok;
};

// Throwaway first shot: lets vite compile the module graph so the real shots are not cold.
// Unique name per process so two concurrent runs never share (and delete) one file.
const warm = path.join(tmpdir(), `pm-evidence-warmup-${process.pid}.png`);
await shoot(scenarios[0], warm);
rmSync(warm, { force: true });

for (const name of scenarios) {
  const png = path.join(out, `${name}.png`);
  const ok = await shoot(name, png);
  if (ok) console.log(`ok   ${name} -> ${png}`);
  else {
    console.error(`FAIL ${name}`);
    failed++;
  }
}
process.exit(failed ? 1 : 0);
