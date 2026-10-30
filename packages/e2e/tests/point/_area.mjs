// Helpers for the Point and Fix tests: a small local web page to point at, the preview window, the picker and the
// Point box. The preview and the Point box are separate windows of the app; the harness records the cockpit window only,
// so each test also shows the result in the cockpit (the agent's screen, the Chat box, a notice).
import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import { fakeTuiExe } from '../../src/paths.mjs';
import { connect, listTargets, sleep } from '../../src/cdp.mjs';
import { Page } from '../../src/page.mjs';

export * from '../whatbroke/_area.mjs';

export const PAGE_HTML = `<!doctype html><html><head><title>Demo shop</title></head>
<body style="font-family:sans-serif;margin:0;padding:24px">
<h1 id="title">Demo shop</h1>
<p id="intro">Fresh bread every morning.</p>
<button id="buy" class="btn" style="padding:20px;font-size:24px">Buy now</button>
<p><a id="ext" href="https://example.com/docs">External docs</a></p>
<form onsubmit="return false"><input id="pw" type="password" value="hunter2-secret-value"><input id="name" value="Ada"></form>
</body></html>`;

/** A web server on 127.0.0.1 with one page. Returns { port, url, close }. Close it in a `finally`. */
export async function startDevServer(html = PAGE_HTML) {
  const server = http.createServer((req, res) => {
    res.setHeader('content-type', 'text/html; charset=utf-8');
    res.end(html);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  return {
    port,
    url: `http://localhost:${port}/`,
    close: () => new Promise((resolve) => { server.closeAllConnections?.(); server.close(() => resolve()); }),
  };
}

/** A Claude-shaped fake agent that prints this line when asked to "serve the app", and answers anything else with "noted". */
export const serveScript = (text) => `agent = "claude"
boot_ms = 100
[[turn]]
on_prompt = "serve the app"
steps = [
  { do = "think", ms = 300 },
  { do = "say", text = ${JSON.stringify(text)} },
]
[[turn]]
steps = [
  { do = "say", text = "noted" },
]
`;

/** The fake Claude: a copy of the fake agent program named like Claude Code in this run's own program folder. */
export function useFakeClaude(t) {
  const exe = process.platform === 'win32' ? 'claude.exe' : 'claude';
  const dest = path.join(t.app.dirs.bin, exe);
  if (!fs.existsSync(dest)) fs.copyFileSync(fakeTuiExe(), dest); // a copy that is running cannot be overwritten
}

/** A pane that "started a dev server": its screen shows the local address. (Claude-shaped, so a pasted message arrives whole.) */
export async function serverPane(t, port, { cwd } = {}) {
  useFakeClaude(t);
  t.fake.script(serveScript(`Local: http://localhost:${port}/`));
  const pane = await t.spawnPane('claude', { cwd, waitFor: /Claude Code/ });
  await pane.write('serve the app');
  await pane.waitScreen(`localhost:${port}`);
  return pane;
}

/** Windows of the app other than the cockpit, by their address. */
export const targets = (t) => listTargets(t.app.cdpPort);

export async function waitTarget(t, part, { timeout = 20000 } = {}) {
  return t.expect.eventually(async () => (await targets(t)).find((x) => x.type === 'page' && x.url.includes(part)), { timeout, message: `expected a window with "${part}" in its address` });
}

/** Connect to another window of the app. `page` finds things by text like the cockpit driver; `eval` runs script in it. */
export async function attach(t, target) {
  const conn = await connect(target.webSocketDebuggerUrl);
  await conn.send('Runtime.enable');
  const page = new Page(conn, { pace: 0.2, testId: t.meta.id });
  return { conn, page, eval: (js) => conn.evalJs(js), close: () => conn.close() };
}

/**
 * Close the preview windows (they stay open between tests otherwise) and clear the Point box. The Point box window itself
 * is made once by the app and only hidden afterwards, so it is emptied (Discard) and never closed.
 */
export async function closeExtraWindows(t) {
  try {
    const v = await (await fetch(`http://127.0.0.1:${t.app.cdpPort}/json/version`, { signal: AbortSignal.timeout(3000) })).json();
    const browser = await connect(v.webSocketDebuggerUrl);
    for (const x of await targets(t)) {
      if (x.type !== 'page' || /cockpit=1/.test(x.url)) continue;
      if (/pointbox=1/.test(x.url)) {
        const box = await attach(t, x);
        await box.eval(`(()=>{const b=[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Discard');if(b)b.click()})()`).catch(() => {});
        box.close();
      } else await browser.send('Target.closeTarget', { targetId: x.id }).catch(() => {});
    }
    browser.close();
  } catch { /* nothing to close */ }
}

/** Press the header chip "Preview" of the selected pane and return the preview window. */
export async function openPreviewWindow(t, port) {
  await t.click('exact=Preview');
  const target = await waitTarget(t, `localhost:${port}`);
  return attach(t, target);
}

/** Arm the picker from the command palette ("Point and fix in the preview"). */
export async function armPicker(t) {
  await t.press('Ctrl+K');
  await t.type('Point and fix');
  await t.expect.text('Point and fix in the preview');
  await t.press('Enter');
}

/** Click an element of the page in the preview window like a person would (move, then press). */
export async function pointAt(preview, selector) {
  const rect = await preview.eval(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});const b=e.getBoundingClientRect();return {x:b.x+b.width/2,y:b.y+b.height/2}})()`);
  await preview.conn.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: rect.x, y: rect.y });
  await sleep(250);
  await preview.conn.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: rect.x, y: rect.y, button: 'left', clickCount: 1 });
  await preview.conn.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: rect.x, y: rect.y, button: 'left', clickCount: 1 });
}

/** The Point box window, once a pick opened it. */
export async function pointBox(t) {
  const target = await waitTarget(t, 'pointbox=1');
  const box = await attach(t, target);
  await box.page.waitFor('Point and Fix');
  return box;
}

/** Choose an option of the first <select> in a window by the text of the option. */
export async function chooseOption(win, label) {
  const ok = await win.eval(`(()=>{const s=document.querySelector('select');if(!s)return false;const o=[...s.options].find(o=>o.text.includes(${JSON.stringify(label)}));if(!o)return false;Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,'value').set.call(s,o.value);s.dispatchEvent(new Event('change',{bubbles:true}));return true})()`);
  if (!ok) throw new Error(`no option with "${label}" in the select`);
}

/** The options of the first <select> in a window. */
export const optionTexts = (win) => win.eval(`(()=>{const s=document.querySelector('select');return s?[...s.options].map(o=>o.text):[]})()`);

/** Pick an element in the preview window and wait for the Point box. Returns the box. */
export async function pickElement(t, preview, selector) {
  await pointAt(preview, selector);
  const box = await pointBox(t);
  await t.expect.eventually(async () => /[1-3] of 3/.test(await box.page.text()), { message: 'the pick shows in the Point box' });
  return box;
}

/** The text the fake agents were sent: one entry per prompt they logged (a pasted message is one entry). */
export const sentPrompts = (t) => t.fake.events().filter((e) => e.ev === 'prompt').map((e) => String(e.text ?? ''));

/** Open the preview, arm the picker and pick one element: returns { preview, box }. */
export async function previewAndPick(t, port, selector) {
  const preview = await openPreviewWindow(t, port);
  await t.expect.eventually(async () => (await preview.eval('document.title')) === 'Demo shop', { message: 'the preview shows the page' });
  await armPicker(t);
  await t.expect.eventually(() => preview.eval(`!!document.getElementById('pm-point-cursor')`), { message: 'the picker is armed' });
  const box = await pickElement(t, preview, selector);
  return { preview, box };
}

/** Press a button of the Point box by script (for a button that sits below the bottom edge of the small window). */
export const boxClick = (box, label) => box.eval(`(()=>{const b=[...document.querySelectorAll('button')].find(b=>b.textContent.trim()===${JSON.stringify(label)});if(!b)return false;b.click();return true})()`);

/**
 * A test whose meta carries `skip` stops here, before it does anything: a known bug stays in the list and in the
 * dashboard as "skipped" with its reason. (The harness registry does not read meta.skip yet, so the test asks itself.)
 */
export function skipKnownBug(t, meta) {
  if (meta.skip) t.skip(meta.skip);
}
