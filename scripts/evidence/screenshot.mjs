#!/usr/bin/env node
// UI evidence: renders harness scenarios (packages/app/src/dev/scenarios) in headless Edge.
// Usage: node scripts/evidence/screenshot.mjs --out <dir> [scenario...]
// Headless only: no visible window is ever opened. Dev server uses port 1430 (not 1420).
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const PORT = 1430;
const EDGE = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const appDir = path.join(root, 'packages/app');
const scenarioDir = path.join(appDir, 'src/dev/scenarios');

const argv = process.argv.slice(2);
let out = null;
const wanted = [];
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === '--out') out = argv[++i];
  else wanted.push(argv[i]);
}
if (!out) {
  console.error('usage: node scripts/evidence/screenshot.mjs --out <dir> [scenario...]');
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
const base = `http://localhost:${PORT}`;

async function ready(timeoutMs = 90_000) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    try {
      const res = await fetch(`${base}/`);
      if (res.ok) return true;
    } catch {
      /* not up yet */
    }
    await sleep(500);
  }
  return false;
}

const require = createRequire(path.join(appDir, 'package.json'));
const viteBin = path.join(path.dirname(require.resolve('vite/package.json')), 'bin/vite.js');
const vite = spawn(process.execPath, [viteBin, '--port', String(PORT), '--strictPort', '--host', 'localhost'], {
  cwd: appDir,
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
  if (process.platform === 'win32') spawnSync('taskkill', ['/pid', String(vite.pid), '/T', '/F'], { windowsHide: true });
  else vite.kill();
}
process.on('SIGINT', () => {
  stopVite();
  process.exit(130);
});

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

let failed = 0;
try {
  if (!(await ready()) || viteExited) {
    console.error('vite did not become ready on', PORT, '\n' + viteLog.slice(-2000));
    process.exit(1);
  }
  // Warm the module graph so the first screenshot is not racing on-demand transforms.
  await fetch(`${base}/src/main.tsx`).catch(() => {});

  const shoot = async (name, png) => {
    let ok = false;
    for (let attempt = 1; attempt <= 3 && !ok; attempt++) {
      rmSync(png, { force: true });
      const profile = mkdtempSync(path.join(tmpdir(), 'pm-evidence-'));
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
      killProfile(profile);
      await sleep(500);
      rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
      if (!ok) console.warn(`${name}: no png (attempt ${attempt}/3)`);
    }
    return ok;
  };

  // Throwaway first shot: lets vite compile the module graph so the real shots are not cold.
  const warm = path.join(tmpdir(), 'pm-evidence-warmup.png');
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
} finally {
  stopVite();
}
process.exit(failed ? 1 : 0);
