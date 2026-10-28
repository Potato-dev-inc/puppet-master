#!/usr/bin/env node
/**
 * Live check for "Bring All My Chats" (BC-22). Drives the running app over the Chrome DevTools Protocol
 * and asserts what the person sees. It types only into the app's own search and card, never into an agent pane,
 * and sends nothing anywhere.
 *
 *   node scripts/verify-bring-chats.mjs <shots-dir> [--cdp 9433] [--expect claude=2,codex=3,opencode=2]
 *                                       [--search "stripe webhook"] [--no-move]
 *
 * Start the app first, on its own profile, its own CDP port and the fixture home (SPEC-n32 sections 4a and 6):
 *   PUPPET_MASTER_PROFILE=bc-live PUPPET_MASTER_HISTORY_HOME=<folder from make-fixture-history-home.mjs>
 * Use a fresh profile: step 3 expects every chat to be new. Port 9333 (the founder's app) is refused.
 * --expect counts are per agent (keys match the agent's name, case-insensitive). The default is the fixture alone.
 * The live run adds two real chats, so pass the real counts there.
 *
 * Steps. Each one writes a screenshot into <shots-dir> and a line into <shots-dir>/result.json:
 *   1 find    "Find my chats": each agent's chat count equals --expect.
 *   2 toggle  Codex switched off: the "Add N chats to search" count drops; switched on again: back to the same N.
 *   3 add     "Add N chats to search": the progress bar is shown if seen, and the result says "Added N chats"
 *             with N = the sum of --expect.
 *   4 search  Ctrl+Shift+F, then --search: a hit with the "Old chat" tag shows the text.
 *   5 open    that hit opens a read-only viewer (no composer) with the text; Escape closes it.
 * Exit 0 when every step passes. On the first failure it keeps fail.png and exits 1.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const USAGE = 'usage: node scripts/verify-bring-chats.mjs <shots-dir> [--cdp 9433] [--expect claude=2,codex=3,opencode=2] [--search "stripe webhook"] [--no-move]';
const FORBIDDEN_PORTS = new Set([9333]);
const DEFAULT_EXPECT = 'claude=2,codex=3,opencode=2';
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

class StepFailed extends Error {}

/** Reads the command line. Throws a plain message for a bad one. */
export function parseArgs(argv) {
  const opts = { shots: null, cdp: 9433, expect: DEFAULT_EXPECT, search: 'stripe webhook', move: true };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--cdp') opts.cdp = Number(argv[++i]);
    else if (arg === '--expect') opts.expect = argv[++i] ?? '';
    else if (arg === '--search') opts.search = argv[++i] ?? '';
    else if (arg === '--no-move') opts.move = false;
    else if (arg.startsWith('--')) throw new Error(`unknown option ${arg}`);
    else if (opts.shots === null) opts.shots = arg;
    else throw new Error(`unexpected argument ${arg}`);
  }
  if (!opts.shots) throw new Error('missing <shots-dir>');
  if (!Number.isInteger(opts.cdp) || opts.cdp < 1024 || opts.cdp > 65535) throw new Error('--cdp takes a port number');
  if (FORBIDDEN_PORTS.has(opts.cdp)) throw new Error(`port ${opts.cdp} is the founder's app; use the bc-live CDP port`);
  if (!opts.search.trim()) throw new Error('--search needs text');
  opts.expect = parseExpect(opts.expect);
  return opts;
}

/** "claude=2,codex=3" -> { claude: 2, codex: 3 }. */
export function parseExpect(text) {
  const counts = {};
  for (const part of text.split(',')) {
    const [key, value] = part.split('=');
    if (!key || !/^[a-z0-9_-]+$/i.test(key.trim()) || !/^\d+$/.test(value ?? '')) {
      throw new Error(`--expect takes key=N pairs, got "${part}"`);
    }
    counts[key.trim().toLowerCase()] = Number(value);
  }
  return counts;
}

/** The chat count in an agent line: "Claude Code · 2 chats · 3 KB" -> 2. */
export function chatsIn(line) {
  const m = /(\d[\d,]*) chats?\b/.exec(line);
  return m ? Number(m[1].replace(/,/g, '')) : null;
}

/** "Add 12 chats to search" -> 12, anything else -> null. */
export function addCountIn(text) {
  const m = /^Add (\d[\d,]*) chats to search$/.exec(text);
  return m ? Number(m[1].replace(/,/g, '')) : null;
}

/** Connects to the app page on the CDP port (the cockpit page when there is one). */
async function connect(port) {
  let list;
  try {
    list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  } catch {
    throw new Error(`no app answers on CDP port ${port}; start it with the bc-live profile first`);
  }
  const page = list.find((t) => t.type === 'page' && t.url.includes('cockpit=1')) ?? list.find((t) => t.type === 'page');
  if (!page) throw new Error(`no app page on CDP port ${port}`);
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((open, fail) => {
    ws.addEventListener('open', open, { once: true });
    ws.addEventListener('error', () => fail(new Error('CDP socket failed')), { once: true });
  });
  let nextId = 0;
  const pending = new Map();
  ws.addEventListener('message', (ev) => {
    const msg = JSON.parse(String(ev.data));
    const waiter = pending.get(msg.id);
    if (!waiter) return;
    pending.delete(msg.id);
    if (msg.error) waiter.reject(new Error(msg.error.message));
    else waiter.resolve(msg.result);
  });
  const send = (method, params = {}) =>
    new Promise((done, fail) => {
      const id = ++nextId;
      pending.set(id, { resolve: done, reject: fail });
      ws.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expression) => {
    const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error('a page script failed');
    return r.result.value;
  };
  return { send, evaluate, close: () => ws.close() };
}

/** Runs the whole check. Returns the process exit code. */
async function run(argv) {
  let opts;
  try {
    opts = parseArgs(argv);
  } catch (err) {
    console.error(`verify-bring-chats: ${err.message}\n${USAGE}`);
    return 2;
  }
  const shotsDir = resolve(opts.shots);
  mkdirSync(shotsDir, { recursive: true });
  let cdp;
  try {
    cdp = await connect(opts.cdp);
  } catch (err) {
    console.error(`verify-bring-chats: ${err.message}`);
    return 1;
  }

  const steps = [];
  let shots = 0;
  const shot = async (name) => {
    const r = await cdp.send('Page.captureScreenshot', { format: 'png' });
    const file = `${String(++shots).padStart(2, '0')}-${name}.png`;
    writeFileSync(join(shotsDir, file), Buffer.from(r.data, 'base64'));
    return file;
  };
  const check = async (name, ok, detail = '', shotName = null) => {
    const shotFile = shotName ? await shot(shotName) : null;
    steps.push({ name, ok: Boolean(ok), detail, shot: shotFile });
    console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` - ${detail}` : ''}`);
    if (!ok) throw new StepFailed(`${name}${detail ? `: ${detail}` : ''}`);
  };
  const waitFor = async (expression, what, ms = 30000) => {
    const end = Date.now() + ms;
    for (;;) {
      const value = await cdp.evaluate(expression);
      if (value) return value;
      if (Date.now() > end) throw new StepFailed(`timed out after ${Math.round(ms / 1000)} s waiting for ${what}`);
      await sleep(250);
    }
  };
  const clickAt = async (expression, what) => {
    const at = await cdp.evaluate(
      `(() => { const el = ${expression}; if (!el) return null; el.scrollIntoView({ block: 'center' });
        const r = el.getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()`,
    );
    if (!at) throw new StepFailed(`nothing to click: ${what}`);
    await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: at[0], y: at[1] });
    await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: at[0], y: at[1], button: 'left', clickCount: 1 });
    await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: at[0], y: at[1], button: 'left', clickCount: 1 });
  };
  const key = async (name, code, vk, modifiers = 0) => {
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyDown', key: name, code, windowsVirtualKeyCode: vk, modifiers });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: name, code, windowsVirtualKeyCode: vk, modifiers });
  };
  // Reads the agent lines of the card: name text and switch state.
  const rows = () =>
    cdp.evaluate(
      `[...document.querySelectorAll('section.bc-panel li.bc-agent')].map((li) => ({
        name: li.querySelector('.bc-agent-name')?.innerText ?? '',
        on: li.querySelector('.ck-switch')?.getAttribute('aria-checked') === 'true',
        switchable: !!li.querySelector('.ck-switch'),
      }))`,
    );
  const addText = () =>
    cdp.evaluate(
      `[...document.querySelectorAll('section.bc-panel button')].map((b) => b.textContent.trim())
        .find((t) => t.startsWith('Add ') && t.endsWith('chats to search')) ?? ''`,
    );
  const rowFor = (agentKey) => `[...document.querySelectorAll('section.bc-panel li.bc-agent')]
    .find((li) => new RegExp(${JSON.stringify(agentKey)}, 'i').test(li.querySelector('.bc-agent-name')?.innerText ?? ''))`;

  try {
    // Moves the app window to the left monitor (SPEC-n32 section 6), so the check never shows on the primary screen.
    if (opts.move) {
      try {
        const w = await cdp.send('Browser.getWindowForTarget');
        await cdp.send('Browser.setWindowBounds', {
          windowId: w.windowId,
          bounds: { windowState: 'normal', left: -1852, top: 121, width: 1400, height: 900 },
        });
      } catch {
        // Not available on this build: the window stays where it is.
      }
    }

    // 1 find
    await waitFor(`!!document.querySelector('section.bc-panel')`, 'the Bring my old chats card (BringChatsPanel)', 20000);
    const findButton = `[...document.querySelectorAll('section.bc-panel button')].find((b) => b.textContent.trim() === 'Find my chats')`;
    if (await cdp.evaluate(`!!${findButton}`)) {
      await clickAt(findButton, 'Find my chats');
    }
    await waitFor(`document.querySelectorAll('section.bc-panel li.bc-agent').length > 0`, 'the agent list');
    const found = await rows();
    const counts = Object.entries(opts.expect).map(([agentKey, want]) => {
      const row = found.find((r) => new RegExp(agentKey, 'i').test(r.name));
      return { agentKey, want, got: row ? chatsIn(row.name) : null };
    });
    const summary = counts.map((c) => `${c.agentKey} ${c.got ?? 'none'} (want ${c.want})`).join(', ');
    await check('find: counts match --expect', counts.every((c) => c.got === c.want), summary, 'found');

    // 2 toggle Codex off and on
    const before = addCountIn(await addText());
    await check('toggle: "Add N chats" is shown', before !== null, `before ${before}`);
    const setCodex = async (on) => {
      const current = (await rows()).find((r) => /codex/i.test(r.name));
      await check('toggle: a Codex line with a switch', current && current.switchable, '');
      if (current.on !== on) await clickAt(`${rowFor('codex')}?.querySelector('.ck-switch')`, 'Codex switch');
      await waitFor(`${rowFor('codex')}?.querySelector('.ck-switch')?.getAttribute('aria-checked') === '${on}'`, 'the Codex switch');
    };
    await setCodex(false);
    const off = addCountIn(await addText());
    await check('toggle: Codex off lowers the count', off !== null && before !== null && off < before, `${before} -> ${off}`, 'toggled-off');
    await setCodex(true);
    const back = addCountIn(await addText());
    await check('toggle: Codex on restores the count', back === before, `${off} -> ${back}`);

    // 3 add
    const total = Object.values(opts.expect).reduce((sum, n) => sum + n, 0);
    await check('add: the count is the sum of --expect', before === total, `${before} vs ${total}`);
    const addButton = `[...document.querySelectorAll('section.bc-panel button')].find((b) => {
      const t = b.textContent.trim(); return t.startsWith('Add ') && t.endsWith('chats to search'); })`;
    await check('add: the Add button is enabled', await cdp.evaluate(`!!(${addButton}) && !(${addButton}).disabled`), '');
    await clickAt(addButton, 'Add chats to search');
    let sawBar = false;
    let result = '';
    const end = Date.now() + 240000;
    while (Date.now() < end && !result) {
      if (!sawBar && (await cdp.evaluate(`!!document.querySelector('section.bc-panel .bc-bar')`))) {
        sawBar = true;
        await shot('reading');
      }
      result = await cdp.evaluate(
        `[...document.querySelectorAll('section.bc-panel [role="status"]')].map((e) => e.innerText.trim()).join(' | ')`,
      );
      if (!/Added [\d,]+ chats|Nothing new to add/.test(result)) result = '';
      if (!result) await sleep(300);
    }
    const added = /Added (\d[\d,]*) chats/.exec(result);
    await check(
      'add: "Added N chats" with N = the sum of --expect',
      added && Number(added[1].replace(/,/g, '')) === total,
      `${result.slice(0, 120) || 'no result line'}; progress bar seen: ${sawBar}`,
      'done',
    );

    // 4 search
    await key('F', 'KeyF', 70, 10); // Ctrl+Shift+F
    await waitFor(`!!document.querySelector('.ck-palette-input')`, 'the search box (Ctrl+Shift+F)');
    await clickAt(`document.querySelector('.ck-palette-input')`, 'search box');
    await cdp.send('Input.insertText', { text: opts.search });
    const query = opts.search.toLowerCase();
    await waitFor(
      `[...document.querySelectorAll('[role="option"]')].some((o) => o.innerText.includes('Old chat'))`,
      `an "Old chat" hit for "${opts.search}"`,
      30000,
    );
    const hitShows = await cdp.evaluate(
      `(() => { const list = document.querySelector('.ck-palette-list');
        return { text: list ? list.innerText.toLowerCase() : '',
          tagged: [...document.querySelectorAll('[role="option"]')].some((o) => o.innerText.includes('Old chat')) }; })()`,
    );
    await check(
      'search: an "Old chat" hit shows the text',
      hitShows.tagged && hitShows.text.includes(query),
      `"${opts.search}" ${hitShows.text.includes(query) ? 'found' : 'not in the list'}`,
      'search',
    );

    // 5 open the hit
    const hitOption = `((list) => list.find((o) => o.innerText.includes('Old chat') && o.innerText.toLowerCase().includes(${JSON.stringify(query)})) ?? list.find((o) => o.innerText.includes('Old chat')) ?? null)([...document.querySelectorAll('[role="option"]')])`;
    await clickAt(hitOption, 'the Old chat hit');
    await waitFor(`!!document.querySelector('.history-viewer')`, 'the read-only viewer');
    const viewer = await cdp.evaluate(
      `(() => { const v = document.querySelector('.history-viewer'); if (!v) return null; return {
        banner: v.querySelector('.history-viewer-banner')?.innerText ?? '',
        composer: !!v.querySelector('textarea, [contenteditable="true"]'),
        hasText: v.innerText.toLowerCase().includes(${JSON.stringify(query)}) }; })()`,
    );
    await check('open: read-only viewer with no composer shows the text', viewer && /read-only/i.test(viewer.banner) && !viewer.composer && viewer.hasText, viewer ? viewer.banner.slice(0, 120) : 'no viewer', 'viewer');
    await key('Escape', 'Escape', 27);
    await waitFor(`!document.querySelector('.history-viewer')`, 'the viewer to close');
    console.log(`All steps passed (${steps.length}).`);
  } catch (err) {
    const message = err instanceof StepFailed ? err.message : `unexpected error: ${err.message}`;
    console.error(`FAIL ${message}`);
    steps.push({ name: 'run', ok: false, detail: message });
    try {
      await shot('fail');
    } catch {
      // The screenshot is best effort.
    }
  } finally {
    cdp.close();
  }

  const ok = steps.every((s) => s.ok);
  writeFileSync(
    join(shotsDir, 'result.json'),
    JSON.stringify({ at: new Date().toISOString(), cdpPort: opts.cdp, expect: opts.expect, ok, steps }, null, 2),
  );
  return ok ? 0 : 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = await run(process.argv.slice(2));
}
