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

/** Put the app back to a known state between tests: no panes, default fake script, a freshly loaded cockpit. */
export async function resetApp(app, conn) {
  const list = (await app.bridge.get('/panes')).body;
  for (const p of Array.isArray(list) ? list : []) await app.bridge.del(`/panes/${encodeURIComponent(p.id)}`).catch(() => {});
  await waitUntil(async () => { const l = (await app.bridge.get('/panes')).body; return !Array.isArray(l) || l.length === 0; }, 8000, 'the panes to close').catch(() => {});
  fs.writeFileSync(app.scriptPath, DEFAULT_SCRIPT);
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
  const closeApp = async () => {
    try { conn?.close(); } catch { /* closed */ }
    conn = null;
    if (app && !opts.keepApp) await app.stop();
    else if (app) app.detach();
    app = null;
  };
  const openApp = async (settings) => {
    const tStart = Date.now();
    app = await startApp({ runId, headless: opts.headless, anchor: opts.anchor, settings });
    conn = await ensureCockpit(app);
    await prepareWindow(app, conn);
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
        if (failedToStart) throw new Error('app did not start: ' + failedToStart);
        if (!app || !app.alive() || !conn || conn.closed || opts.fresh || test.fresh || test.seed || seeded) {
          await closeApp();
          try { await openApp(test.seed); seeded = !!test.seed; } catch (e) { failedToStart = e.message; throw new Error('app did not start: ' + e.message); }
          appFresh = true;
        }
        if (!appFresh) await resetApp(app, conn);
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
      emit({ type: 'test-end', id: test.id, status: result.status, durationMs: result.durationMs, error: result.error?.message, video: result.video ? `${runId}/${test.id}/video.mp4` : null });
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
