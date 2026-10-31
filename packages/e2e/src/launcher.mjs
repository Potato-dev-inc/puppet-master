// Starts ONE isolated copy of the real app for a run, and tears down only the process tree it started.
//
// Isolation (checked by guards.mjs and by the launcher itself):
//  - its own profile (PUPPET_MASTER_PROFILE=e2e): another app id, its own bridge port, never 17321/17322
//  - APPDATA, HOME and USERPROFILE point into a temp folder per run, so settings, agent definitions and chats are
//    fresh and nothing is read from or written to the real user folders
//  - a random free WebView2 debugging port (never 9333)
//  - fake agents only: copies of tools/pm-fake-tui under the names the agent definitions in fixtures/agents/ ask for
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { appExe, fakeTuiExe, tmpBase, AGENT_FIXTURES, REPO } from './paths.mjs';
import { assertSafePort, assertSafeProfile, isFoundersDataDir } from './guards.mjs';
import { connect, listTargets, sleep } from './cdp.mjs';
import { BridgeClient } from './bridge.mjs';

const DEFAULT_ANCHOR = '-1852,121'; // the secondary (left) monitor, where the dev app opens its windows
const OFFSCREEN_ANCHOR = '-32000,-32000';

const live = new Set();
let hooksInstalled = false;
function installHooks() {
  if (hooksInstalled) return;
  hooksInstalled = true;
  const killAll = () => { for (const a of [...live]) a.killSync(); };
  process.on('exit', killAll);
  for (const sig of ['SIGINT', 'SIGTERM', 'SIGBREAK']) process.on(sig, () => { killAll(); process.exit(130); });
  process.on('uncaughtException', (e) => { console.error(e); killAll(); process.exit(1); });
}

export async function freePort() {
  for (let i = 0; i < 20; i++) {
    const port = await new Promise((res, rej) => {
      const s = net.createServer();
      s.once('error', rej);
      s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => res(p)); });
    });
    try { assertSafePort(port, 'random port'); return port; } catch { /* try another */ }
  }
  throw new Error('no free port');
}

const exists = (p) => { try { return fs.existsSync(p); } catch { return false; } };

/** Agent definitions to install: every *.toml in fixtures/agents, with the program names each one asks for. */
export function agentFixtures() {
  if (!exists(AGENT_FIXTURES)) return [];
  return fs.readdirSync(AGENT_FIXTURES).filter((f) => f.endsWith('.toml')).map((file) => {
    const text = fs.readFileSync(path.join(AGENT_FIXTURES, file), 'utf8');
    const id = (text.match(/^\s*id\s*=\s*"([^"]+)"/m) || [])[1] || file.replace(/\.toml$/, '');
    const names = ((text.match(/^\s*names\s*=\s*\[([^\]]*)\]/m) || [])[1] || '')
      .split(',').map((s) => s.trim().replace(/^"|"$/g, '')).filter(Boolean);
    return { file, id, names, text };
  });
}

export const DEFAULT_SCRIPT = `# The script every fake agent plays unless a test writes its own (t.fake.script).
agent = "nimbus"
boot_ms = 150
inline = true
`;

/**
 * @param {{ runId: string, profile?: string, anchor?: string, headless?: boolean, window?: [number, number], settings?: object }} opts
 */
export async function startApp(opts) {
  installHooks();
  const exe = appExe();
  const fake = fakeTuiExe();
  if (!exists(exe)) throw new Error(`app binary not found: ${exe}\nRun: npm run e2e:build`);
  if (!exists(fake)) throw new Error(`fake agent not found: ${fake}\nRun: npm run e2e:build`);
  const profile = opts.profile || process.env.E2E_PROFILE || 'e2e';
  assertSafeProfile(profile);

  const root = path.join(tmpBase(), opts.runId);
  const dirs = {
    root,
    appdata: path.join(root, 'appdata'),
    home: path.join(root, 'home'),
    bin: path.join(root, 'bin'),
    projects: path.join(root, 'projects'),
    fake: path.join(root, 'fake'),
    local: path.join(root, 'local'),
  };
  for (const d of Object.values(dirs)) fs.mkdirSync(d, { recursive: true });
  const appId = `com.puppetmaster.app.profile-${profile}`;
  const dataDir = path.join(dirs.appdata, appId);
  if (isFoundersDataDir(dataDir)) throw new Error('refusing: the data folder is the real app folder');
  fs.mkdirSync(path.join(dataDir, 'agents'), { recursive: true });

  // Agent definitions plus the fake programs they name.
  const fixtures = agentFixtures();
  for (const fx of fixtures) {
    fs.writeFileSync(path.join(dataDir, 'agents', fx.file), fx.text);
    for (const n of fx.names) fs.copyFileSync(fake, path.join(dirs.bin, process.platform === 'win32' ? `${n}.exe` : n));
  }
  const scriptPath = path.join(dirs.fake, 'script.toml');
  const fakeLog = path.join(dirs.fake, 'events.jsonl');
  fs.writeFileSync(scriptPath, DEFAULT_SCRIPT);
  fs.writeFileSync(fakeLog, '');

  // Settings the app starts with. The first-run welcome is switched off by default; a test that wants it
  // (or any other setting) passes `meta.seed` (a null value removes a key), which also restarts the app for it.
  const settings = { onboarding_done: true, away_minutes: 0, ...(opts.settings || {}) };
  for (const k of Object.keys(settings)) if (settings[k] === null) delete settings[k];
  fs.writeFileSync(path.join(dataDir, 'settings.json'), JSON.stringify({ settings }, null, 2));

  const cdpPort = await freePort();
  assertSafePort(cdpPort, 'debugging port');
  const headless = opts.headless || process.env.E2E_HEADLESS === '1';
  const anchor = opts.anchor || process.env.PM_WINDOW_ANCHOR || (headless ? OFFSCREEN_ANCHOR : DEFAULT_ANCHOR);
  const pathKey = Object.keys(process.env).find((k) => k.toLowerCase() === 'path') || 'PATH';
  const env = {
    ...process.env,
    PUPPET_MASTER_PROFILE: profile,
    APPDATA: dirs.appdata,
    LOCALAPPDATA: dirs.local,
    HOME: dirs.home,
    USERPROFILE: dirs.home,
    XDG_DATA_HOME: dirs.local,
    WEBVIEW2_USER_DATA_FOLDER: path.join(root, 'webview'), // the browser profile (localStorage, cache) is per run too
    PM_WINDOW_ANCHOR: anchor,
    // A normal start now opens the cockpit by itself, but that window is created before the debugging port is
    // usable, so the harness never sees it. Start headless and let ensureCockpit open the window the supported way.
    PUPPET_MASTER_NO_WINDOW: process.env.E2E_STARTUP_WINDOW === '1' ? '0' : '1',
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${cdpPort}`,
    PM_FAKE_TUI_SCRIPT: scriptPath,
    PM_E2E_PATH_PREPEND: dirs.bin,
    // Potato Bot runs this stand-in for its brain (no model, no key, no network). Only a dev-fake-tui build reads it.
    PM_POTATO_BRAIN: path.join(REPO, 'packages', 'potato-brain', 'test', 'fake-brain.mjs'),
    PM_FAKE_TUI_LOG: fakeLog,
    [pathKey]: dirs.bin + path.delimiter + (process.env[pathKey] || ''),
    RUST_LOG: process.env.RUST_LOG || 'info',
  };
  // Never read a bridge port/secret an earlier instance left in this folder: wait for the NEW ones.
  for (const f of ['puppet-master.bridge.port', 'puppet-master.bridge.secret']) { try { fs.rmSync(path.join(dataDir, f), { force: true }); } catch { /* none */ } }
  const logFile = path.join(root, 'app.log');
  const logFd = fs.openSync(logFile, 'a');
  const child = spawn(exe, [], { env, stdio: ['ignore', logFd, logFd], windowsHide: false, cwd: dirs.root });
  let exited = null;
  child.on('exit', (code, sig) => { exited = { code, sig }; });
  const app = {
    runId: opts.runId, profile, pid: child.pid, cdpPort, dirs, dataDir, scriptPath, fakeLog, logFile, anchor, exe, fixtures,
    bridgePort: 0, secret: '', bridge: null, window: opts.window || [1440, 900],
    get exited() { return exited; },
    alive: () => !exited,
    /** Leave the app running when this process exits (--keep-app). */
    detach() { live.delete(this); },
    killSync() {
      if (this._killed) return;
      this._killed = true;
      live.delete(this);
      killTree(child.pid);
    },
    async stop() {
      this.killSync();
      for (let i = 0; i < 40 && !exited; i++) await sleep(100); // wait until the process is really gone
      await sleep(300);
      try { fs.closeSync(logFd); } catch { /* closed */ }
    },
  };
  live.add(app);
  fs.writeFileSync(path.join(root, 'app.json'), JSON.stringify({ appPid: child.pid, ownerPid: process.pid, exe }));

  try {
    // 1. the bridge port and secret files appear when the app is up
    const portFile = path.join(dataDir, 'puppet-master.bridge.port');
    const secretFile = path.join(dataDir, 'puppet-master.bridge.secret');
    await waitUntil(() => {
      if (exited) throw new Error(`the app exited at start (code ${exited.code}). See ${logFile}`);
      return exists(portFile) && exists(secretFile);
    }, 60000, 'the bridge port file');
    app.bridgePort = +((fs.readFileSync(portFile, 'utf8').match(/\d{4,5}/) || [0])[0]);
    assertSafePort(app.bridgePort, 'bridge port');
    app.secret = fs.readFileSync(secretFile, 'utf8').trim();
    app.bridge = new BridgeClient(app.bridgePort, app.secret);
    await waitUntil(async () => { try { return (await app.bridge.get('/health')).status === 200; } catch { return false; } }, 30000, 'the bridge /health');
    // 2. approve the fake agents' definitions (the app asks for approval of every definition it did not write itself)
    await approveDefinitions(app);
  } catch (e) {
    await app.stop();
    throw e;
  }
  return app;
}

async function approveDefinitions(app) {
  if (process.env.E2E_SKIP_APPROVE) return;
  if (!app.fixtures.length) return;
  await app.bridge.post('/agents/definitions/reload', {});
  const list = (await app.bridge.get('/agents/definitions')).body || {};
  const entries = Array.isArray(list) ? list : list.agents || list.definitions || list.entries || [];
  for (const fx of app.fixtures) {
    const e = entries.find((x) => (x.id || x.agent?.id) === fx.id || String(x.file || '').endsWith(fx.file));
    if (!e) throw new Error(`agent definition ${fx.id} was not loaded: ${JSON.stringify(list).slice(0, 600)}`);
    if (e.approved === true || e.state === 'approved' || e.status === 'approved' || e.status === 'active') continue;
    const a = await app.bridge.post('/agents/definitions/approve', { file: e.file || fx.file, fingerprint: e.fingerprint || e.sha || e.hash });
    if (a.status !== 200) throw new Error(`approving ${fx.id} failed: ${a.status} ${JSON.stringify(a.body).slice(0, 400)}`);
  }
}

function pidAlive(pid, imageStart) {
  try {
    if (process.platform !== 'win32') { process.kill(pid, 0); return true; }
    const r = spawnSync('tasklist', ['/FI', `PID eq ${pid}`, '/FO', 'CSV', '/NH'], { encoding: 'utf8', windowsHide: true });
    const line = (r.stdout || '').split(String.fromCharCode(10)).find((l) => l.includes(`"${pid}"`));
    return !!line && (!imageStart || line.toLowerCase().startsWith(`"${imageStart.toLowerCase()}`));
  } catch { return false; }
}

/**
 * A run killed hard (the dashboard's process was killed, a crash) can leave its app running. Each run records
 * who started which app in <run folder>/app.json; this stops only an app recorded there whose starter is gone and
 * whose image is still puppet_master_app. Returns the pids it stopped.
 */
export function reapOrphans() {
  const stopped = [];
  let runs = [];
  try { runs = fs.readdirSync(tmpBase()); } catch { return stopped; }
  for (const name of runs) {
    const f = path.join(tmpBase(), name, 'app.json');
    let rec; try { rec = JSON.parse(fs.readFileSync(f, 'utf8')); } catch { continue; }
    if (!rec.appPid || rec.reaped) continue;
    if (pidAlive(rec.ownerPid)) continue; // the run that started it is still going
    if (pidAlive(rec.appPid, 'puppet_master_app')) { killTree(rec.appPid); stopped.push(rec.appPid); }
    try { fs.writeFileSync(f, JSON.stringify({ ...rec, reaped: true })); } catch { /* read-only */ }
  }
  return stopped;
}

export function killTree(pid) {
  if (!pid) return;
  try {
    if (process.platform === 'win32') spawnSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true });
    else process.kill(pid, 'SIGKILL');
  } catch { /* already gone */ }
}

export async function waitUntil(fn, timeoutMs, what) {
  const end = Date.now() + timeoutMs;
  let last;
  while (Date.now() < end) {
    try { if (await fn()) return; } catch (e) { last = e; if (/exited at start/.test(e.message)) throw e; }
    await sleep(250);
  }
  throw new Error(`timed out waiting for ${what}${last ? ': ' + last.message : ''}`);
}

/** The cockpit page target on the app's debugging port, or null. */
export async function findCockpit(app) {
  try { return (await listTargets(app.cdpPort)).find((t) => t.type === 'page' && /cockpit=1/.test(t.url)) || null; } catch { return null; }
}

/**
 * Make sure a cockpit window exists and return a CDP connection to it. If the app already opened one (it may do
 * that at start), use it. Otherwise open it the supported way without taking focus: spawn a plain shell pane and
 * ask the bridge to detach it, which opens the cockpit unfocused at the window anchor; the helper pane is closed.
 */
export async function ensureCockpit(app) {
  // A newer app opens its cockpit by itself at start: give it a moment before opening one ourselves.
  let t = null;
  for (let i = 0; i < 16 && !t; i++) {
    t = await findCockpit(app);
    if (!t) await sleep(250);
  }
  if (!t) {
    const sp = await app.bridge.post('/panes', { agent_type: 'powershell', cwd: app.dirs.projects, cols: 100, rows: 30 });
    if (sp.status >= 300) throw new Error(`could not open the cockpit (spawn): ${sp.status} ${JSON.stringify(sp.body).slice(0, 300)}`);
    const id = sp.body?.snapshot?.pane_id || sp.body?.pane_id;
    await app.bridge.post(`/panes/${encodeURIComponent(id)}/detach`, {});
    await waitUntil(async () => (t = await findCockpit(app)), 30000, 'the cockpit window');
    await sleep(1000);
    await app.bridge.del(`/panes/${encodeURIComponent(id)}`);
  }
  const c = await connect(t.webSocketDebuggerUrl);
  await c.send('Page.enable');
  await c.send('Runtime.enable');
  return c;
}
