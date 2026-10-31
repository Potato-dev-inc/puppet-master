// Runs a list of tests against ONE app instance (restarted per test with `fresh`), records a video of each and
// writes e2e-results/<run-id>/<test-id>/{result.json,video.mp4,failure.png} plus e2e-results/<run-id>/run.json.
import fs from 'node:fs';
import path from 'node:path';
import { RESULTS_DIR, newRunId } from './paths.mjs';
import { startApp, ensureCockpit, findCockpit, waitUntil, DEFAULT_SCRIPT } from './launcher.mjs';
import { sleep } from './cdp.mjs';
import { makeContext, SkipTest } from './testctx.mjs';
import { Recorder } from './recorder.mjs';

const VIEWPORT = () => {
  const [w, h] = String(process.env.E2E_VIEWPORT || '1280x800').split('x').map(Number);
  return { width: w || 1440, height: h || 900 };
};

/** Known-bug / unverified tests run only when asked. */
export const runSkipped = () => ['E2E_RUN_KNOWN_BUGS', 'E2E_KNOWN_BUGS', 'E2E_UNVERIFIED'].some((k) => /^(1|true|yes)$/i.test(process.env[k] || ''));

const writeJson = (file, data) => fs.writeFileSync(file, JSON.stringify(data, null, 2));

async function prepareWindow(app, conn) {
  // Size the real window so the page is about the video size; no emulation (it leaves bars around the page).
  const vp = VIEWPORT();
  try {
    const w = await conn.send('Browser.getWindowForTarget');
    await conn.send('Browser.setWindowBounds', { windowId: w.windowId, bounds: { windowState: 'normal', width: vp.width, height: vp.height } });
    await sleep(500);
  } catch { /* the window size is cosmetic */ }
  const inner = await conn.evalJs('({w: innerWidth, h: innerHeight, dpr: devicePixelRatio})').catch(() => null);
  if (inner) app.viewport = inner;
}

/**
 * Keep the first-run welcome and the "While you were away" page off. The app keeps its own in-memory copy of the
 * settings once any window patches them (opening Settings does), and that copy does not hold the harness's seed, so the
 * welcome would cover the cockpit on the next page load; a slow run also gets the away page after five minutes.
 * Saying it again through the app's own settings command keeps both off. A test that wants the welcome seeds
 * `onboarding_done: null` (and the app restarts for it), so this is skipped for it.
 */
export async function keepOverlaysOff(conn, test) {
  if (test && test.seed && 'onboarding_done' in test.seed && test.seed.onboarding_done !== true) return;
  const patch = { onboarding_done: true, away_minutes: 0 };
  await conn.evalJs(`window.__TAURI_INTERNALS__.invoke('settings_patch', { patch: ${JSON.stringify(patch)} }).then(() => true, () => false)`).catch(() => false);
}

/** Close every pane (also the ones runs created). A pane that will not close is retried; what stays is reported. */
async function closeAllPanes(app, conn) {
  let left = [];
  for (let round = 0; round < 4; round++) {
    const list = (await app.bridge.get('/panes')).body;
    left = Array.isArray(list) ? list : [];
    if (!left.length) return [];
    for (const p of left) {
      const r = await app.bridge.del(`/panes/${encodeURIComponent(p.id)}`).catch((e) => ({ status: 0, body: String(e.message) }));
      if (r.status >= 300 && r.status !== 404) {
        // A pane a run (MCP session) created is not closed by bare HTTP (403). The Close button of the app is: use the
        // command it calls, from the cockpit page.
        const viaUi = await conn.evalJs(`window.__TAURI_INTERNALS__.invoke('kill_pane_cmd', { paneId: ${JSON.stringify(p.id)} }).then(() => null, (e) => String((e && e.message) || e))`).catch((e) => e.message);
        if (viaUi) closeRefusals.set(p.id, `${r.status} ${JSON.stringify(r.body).slice(0, 120)}; the app's Close: ${String(viaUi).slice(0, 120)}`);
      }
    }
    await sleep(300 * (round + 1));
  }
  const list = (await app.bridge.get('/panes')).body;
  return Array.isArray(list) ? list : [];
}
const closeRefusals = new Map();

/** Put the app back to a known state between tests: no panes, default fake script, a freshly loaded cockpit. */
export async function resetApp(app, conn, test) {
  const stuck = await closeAllPanes(app, conn);
  if (stuck.length) throw new Error(`could not close ${stuck.length} pane(s) between tests: ${stuck.map((p) => p.id.slice(0, 8) + ' ' + (closeRefusals.get(p.id) || 'still listed')).join('; ')}`);
  fs.writeFileSync(app.scriptPath, DEFAULT_SCRIPT);
  await keepOverlaysOff(conn, test);
  const target = await findCockpit(app);
  const base = target ? target.url.split('?')[0] : 'http://tauri.localhost/index.html';
  await conn.evalJs('try{localStorage.clear();sessionStorage.clear()}catch(e){}').catch(() => {});
  await conn.send('Page.navigate', { url: base + '?cockpit=1' });
  await waitUntil(async () => (await conn.evalJs('document.readyState === "complete" && document.body && document.body.innerText.includes("Puppet Master")').catch(() => false)), 20000, 'the cockpit to load');
  await sleep(400);
}

/**
 * @param tests registry entries
 * @param {{ video?: boolean, keepApp?: boolean, fresh?: boolean, headless?: boolean, runId?: string, anchor?: string, signal?: { stopped: boolean } }} opts
 * @param {(event: object) => void} emit
 */
export async function runTests(tests, opts = {}, emit = () => {}) {
  const runId = opts.runId || newRunId();
  const runDir = path.join(RESULTS_DIR, runId);
  fs.mkdirSync(runDir, { recursive: true });
  const video = opts.video !== false;
  const signal = opts.signal || { stopped: false };
  const startedAt = new Date().toISOString();
  const t0 = Date.now();
  const summary = { runId, startedAt, status: 'running', options: { video, fresh: !!opts.fresh, headless: !!opts.headless }, tests: tests.map((t) => t.id), results: [] };
  writeJson(path.join(runDir, 'run.json'), summary);
  emit({ type: 'run-start', runId, total: tests.length });

  let app = null;
  let conn = null;
  let consoleErrors = [];
  let starts = 0;
  const closeApp = async () => {
    try { conn?.close(); } catch { /* closed */ }
    conn = null;
    if (app && !opts.keepApp) await app.stop();
    else if (app) app.detach();
    app = null;
  };
  const openApp = async (settings) => {
    const tStart = Date.now();
    // One retry: a start whose cockpit window never shows up on the debugging port is rare but happens.
    for (let attempt = 1; ; attempt++) {
      // Every start gets a folder of its own (-r2, -r3 ... next to the first): a restarted app never reads the bridge
      // port/secret files, settings or webview data a previous instance left behind.
      starts += 1;
      try {
        app = await startApp({ runId: starts === 1 ? runId : `${runId}-r${starts}`, headless: opts.headless, anchor: opts.anchor, settings });
        conn = await ensureCockpit(app);
        await prepareWindow(app, conn);
        break;
      } catch (e) {
        try { conn?.close(); } catch { /* closed */ }
        conn = null;
        if (app) { await app.stop().catch(() => {}); app = null; }
        if (attempt >= 2) throw e;
      }
    }
    conn.on((m) => {
      if (m.method === 'Runtime.exceptionThrown') consoleErrors.push(String(m.params.exceptionDetails?.exception?.description || m.params.exceptionDetails?.text).slice(0, 300));
      else if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') consoleErrors.push(m.params.args.map((a) => a.value ?? a.description ?? '').join(' ').slice(0, 300));
    });
    emit({ type: 'app-start', viewport: app.viewport, pid: app.pid, cdpPort: app.cdpPort, bridgePort: app.bridgePort, ms: Date.now() - tStart });
    return true;
  };

  let appFresh = false;
  let seeded = false; // the running app was started with a test's own settings
  let failedToStart = null;
  try {
    for (let i = 0; i < tests.length; i++) {
      const test = tests[i];
      if (signal.stopped) break;
      const outDir = path.join(runDir, test.id);
      fs.mkdirSync(outDir, { recursive: true });
      const result = { id: test.id, title: test.title, area: test.area, tags: test.tags, edge: test.edge, runId, startedAt: new Date().toISOString(), status: 'fail', durationMs: 0, steps: [], logs: [], consoleErrors: [], error: null, video: null };
      emit({ type: 'test-start', id: test.id, index: i + 1, total: tests.length, title: test.title });
      const tStart = Date.now();
      let recorder = null;
      let ctx = null;
      let aborted = false;
      try {
        // A test whose meta carries skip is a known bug (or not yet verified): it stays listed and shows as skipped
        // with its reason. E2E_RUN_KNOWN_BUGS=1 (or E2E_KNOWN_BUGS=1, E2E_UNVERIFIED=1, --known-bugs) runs it anyway.
        if (test.skip && !runSkipped()) throw new SkipTest(test.skip);
        if (failedToStart) throw new Error('app did not start: ' + failedToStart);
        if (!app || !app.alive() || !conn || conn.closed || opts.fresh || test.fresh || test.seed || seeded) {
          await closeApp();
          try { await openApp(test.seed); seeded = !!test.seed; } catch (e) { failedToStart = e.message; throw new Error('app did not start: ' + e.message); }
          appFresh = true;
        }
        if (!appFresh) await resetApp(app, conn, test);
        else await keepOverlaysOff(conn, test);
        appFresh = false;
        consoleErrors = [];
        ctx = makeContext({ meta: test, app, conn, runDir: outDir, pace: video ? 1 : 0.2, isAborted: () => aborted || signal.stopped, emit, project: null });
        await ctx.page.inject();
        if (video) { recorder = new Recorder(conn, outDir, VIEWPORT()); await recorder.start(); await sleep(300); }
        if (test.setup) await test.setup(ctx);
        let timer;
        const timeout = new Promise((_, rej) => { timer = setTimeout(() => rej(new Error(`the test took longer than ${test.timeout} ms`)), test.timeout); });
        try { await Promise.race([test.run(ctx), timeout]); } finally { clearTimeout(timer); }
        result.status = 'pass';
      } catch (e) {
        aborted = true;
        if (e instanceof SkipTest) { result.status = 'skip'; result.skipReason = e.message; }
        else {
          result.status = 'fail';
          result.error = { name: e.name, message: e.message, stack: String(e.stack || '').split('\n').slice(0, 8).join('\n') };
          try { if (conn && !conn.closed) { await ctx?.page.screenshot(path.join(outDir, 'failure.png')); result.screenshot = 'failure.png'; } } catch { /* no page */ }
        }
      }
      aborted = true;
      if (recorder) {
        await sleep(500);
        try {
          await recorder.stop();
          const enc = recorder.encode(path.join(outDir, 'video.mp4'));
          if (enc) { result.video = 'video.mp4'; result.videoSeconds = Math.round(enc.seconds * 10) / 10; }
          recorder.keepFrame(recorder.frames.length - 1, path.join(outDir, 'thumb.jpg'));
          result.thumb = 'thumb.jpg';
        } catch (e) { result.videoError = e.message; }
        recorder.cleanup();
      }
      if (ctx) { result.steps = ctx.steps; result.logs = ctx.logs; }
      result.consoleErrors = [...new Set(consoleErrors)].slice(0, 20);
      result.durationMs = Date.now() - tStart;
      result.finishedAt = new Date().toISOString();
      if (app) result.app = { pid: app.pid, cdpPort: app.cdpPort, bridgePort: app.bridgePort };
      writeJson(path.join(outDir, 'result.json'), result);
      summary.results.push({ id: test.id, status: result.status, durationMs: result.durationMs });
      writeJson(path.join(runDir, 'run.json'), summary);
      emit({ type: 'test-end', id: test.id, status: result.status, durationMs: result.durationMs, error: result.error?.message, skipReason: result.skipReason, video: result.video ? `${runId}/${test.id}/video.mp4` : null });
    }
  } finally {
    summary.status = signal.stopped ? 'stopped' : 'done';
    summary.finishedAt = new Date().toISOString();
    summary.durationMs = Date.now() - t0;
    summary.passed = summary.results.filter((r) => r.status === 'pass').length;
    summary.failed = summary.results.filter((r) => r.status === 'fail').length;
    summary.skipped = summary.results.filter((r) => r.status === 'skip').length;
    writeJson(path.join(runDir, 'run.json'), summary);
    if (opts.keepApp && app) emit({ type: 'app-kept', pid: app.pid, cdpPort: app.cdpPort, bridgePort: app.bridgePort, dir: app.dirs.root });
    await closeApp();
    emit({ type: 'run-end', runId, passed: summary.passed, failed: summary.failed, skipped: summary.skipped, durationMs: summary.durationMs });
  }
  return summary;
}
