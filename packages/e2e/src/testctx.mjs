// The `t` object a test receives: the page driver, assertions, the bridge, fake agents and panes.
import fs from 'node:fs';
import path from 'node:path';
import { sleep } from './cdp.mjs';
import { Page } from './page.mjs';
import { makeExpect } from './expect.mjs';
import { DEFAULT_SCRIPT } from './launcher.mjs';

export class SkipTest extends Error {
  constructor(reason) { super(reason); this.name = 'SkipTest'; }
}

/** A pane (one agent) in the app. */
export class Pane {
  constructor(t, id, agent, cwd) { this.t = t; this.id = id; this.agent = agent; this.cwd = cwd; }
  get bridge() { return this.t.app.bridge; }
  async info() { return (await this.bridge.get('/panes')).body.find((p) => p.id === this.id) || null; }
  /** What the pane shows right now (the terminal text). */
  async screen() {
    const r = await this.bridge.get(`/panes/${this.id}/buffer`);
    return typeof r.body === 'string' ? r.body : r.body?.content ?? JSON.stringify(r.body);
  }
  async waitScreen(textOrRe, { timeout = 20000 } = {}) {
    await this.t.expect.eventually(async () => {
      const s = await this.screen();
      return textOrRe instanceof RegExp ? textOrRe.test(s) : s.includes(textOrRe);
    }, { timeout, message: async () => `expected the pane ${this.id.slice(0, 8)} to show ${textOrRe}. It shows:\n${(await this.screen()).slice(-800)}` });
  }
  /** Type into the pane the way a person at the terminal would. */
  async write(text, { enter = true } = {}) {
    const r = await this.bridge.post(`/panes/${this.id}/input`, { text, append_newline: enter });
    if (!r.ok) throw new Error(`write to pane failed: ${r.status} ${JSON.stringify(r.body).slice(0, 200)}`);
  }
  async key(name) { return this.bridge.post(`/panes/${this.id}/key`, { key: name }); }
  async kill() { return this.bridge.del(`/panes/${this.id}`); }
}

export function makeContext({ meta, app, conn, runDir, pace, isAborted, emit, project }) {
  const page = new Page(conn, { pace, testId: meta.id, isAborted });
  const expect = makeExpect(page, { isAborted });
  const startedAt = Date.now();
  const steps = [];
  const logs = [];
  const fakeLogStart = (() => { try { return fs.statSync(app.fakeLog).size; } catch { return 0; } })();
  let projectCount = 0;

  const t = {
    meta, page, expect, app,
    /** Bridge client (GET/POST/DELETE with the profile's secret). */
    bridge: app.bridge,
    /** Where this test may write files (screenshots, notes). */
    outDir: runDir,
    startedAt,
    steps, logs,

    /** Mark a named step. It shows in the video (bottom left), in the live log and in result.json. */
    async step(name) {
      const atMs = Date.now() - startedAt;
      steps.push({ name, atMs });
      emit({ type: 'step', id: meta.id, name, atMs });
      await page.step(name).catch(() => {});
    },
    log(...args) {
      const line = args.map((a) => (typeof a === 'string' ? a : JSON.stringify(a))).join(' ');
      logs.push({ atMs: Date.now() - startedAt, line });
      emit({ type: 'log', id: meta.id, line });
    },
    skip(reason) { throw new SkipTest(reason || 'skipped'); },
    sleep,

    // short forms of the page driver
    click: (...a) => page.click(...a),
    hover: (...a) => page.hover(...a),
    type: (...a) => page.type(...a),
    fill: (...a) => page.fill(...a),
    press: (...a) => page.press(...a),
    waitFor: (...a) => page.waitFor(...a),
    waitGone: (...a) => page.waitGone(...a),

    /** A new empty folder to use as a project (a pane's working folder). */
    project(name) {
      const dir = path.join(app.dirs.projects, name || `project-${++projectCount}`);
      fs.mkdirSync(dir, { recursive: true });
      return dir;
    },

    /** Start a pane of a registered agent (default: the fake agent `nimbus`) and wait until the app lists it. */
    async spawnPane(agent = 'nimbus', opts = {}) {
      const cwd = opts.cwd || t.project();
      if (opts.script) t.fake.script(opts.script);
      const r = await app.bridge.post('/panes', { agent_type: agent, cwd, cols: opts.cols || 120, rows: opts.rows || 32, extra_args: opts.args });
      if (!r.ok) throw new Error(`could not start a ${agent} pane: ${r.status} ${JSON.stringify(r.body).slice(0, 300)}`);
      const id = r.body?.snapshot?.pane_id || r.body?.pane_id;
      if (!id) throw new Error('the app did not return a pane id: ' + JSON.stringify(r.body).slice(0, 200));
      const pane = new Pane(t, id, agent, cwd);
      if (opts.wait !== false) {
        const want = opts.waitFor || /NIMBUS/;
        await t.expect.eventually(async () => {
          const s = await pane.screen();
          if (want instanceof RegExp ? want.test(s) : s.includes(want)) return true;
          const info = await pane.info();
          if (!info || info.status === 'error') throw new Error(`the ${agent} pane ended at start (${info ? info.status : 'gone'}). Is the fake script valid TOML? Screen:
${s.slice(-600)}`);
          return false;
        }, { timeout: 20000, message: `expected the new ${agent} pane to show ${want}` });
        // a new pane is "working" for a moment; wait until the app calls it idle so sidebar rows stop moving
        if (opts.idle !== false) await t.expect.eventually(async () => (await pane.info())?.status === 'idle', { timeout: 15000, message: `expected the new ${agent} pane to become idle` });
      }
      return pane;
    },

    async panes() { return (await app.bridge.get('/panes')).body; },

    fake: {
      /** Set the script the NEXT fake agent pane plays (pm-fake-tui TOML, see tools/pm-fake-tui/README.md). */
      script(toml) {
        const text = String(toml);
        fs.writeFileSync(app.scriptPath, /^\s*agent\s*=/m.test(text) ? text : `agent = "nimbus"\n${text}`);
      },
      resetScript() { fs.writeFileSync(app.scriptPath, DEFAULT_SCRIPT); },
      /** Events the fake agents logged since this test started: start, ready, key, prompt, approval_shown, turn_done, exit ... */
      events() {
        let text = '';
        try { text = fs.readFileSync(app.fakeLog, 'utf8').slice(fakeLogStart); } catch { /* none yet */ }
        return text.split('\n').filter(Boolean).map((l) => { try { return JSON.parse(l); } catch { return { raw: l }; } });
      },
      async waitEvent(match, { timeout = 15000 } = {}) {
        const test = typeof match === 'string' ? (e) => e.ev === match || e.event === match || e.kind === match : match;
        return t.expect.eventually(() => t.fake.events().find(test), { timeout, message: () => `expected the fake agent to log ${typeof match === 'string' ? match : 'the event'}. Logged: ${JSON.stringify(t.fake.events().map((e) => e.ev || e.event || e.kind)).slice(0, 400)}` });
      },
    },
  };
  return t;
}
