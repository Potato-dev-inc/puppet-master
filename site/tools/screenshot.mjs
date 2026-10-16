#!/usr/bin/env node
// Evidence for the landing page: full-page screenshots at 1440 and 390 px, dark and light, plus one shot per
// section, taken with headless Edge over the DevTools protocol (same approach as scripts/evidence/screenshot.mjs:
// a temp profile, a device-metrics override because Edge's window can't be narrower than ~500 px, and every
// started process killed at the end). No visible window is ever opened.
// Also records every network request the page made, and fails if any host other than Google Fonts shows up.
//
// Usage: node site/tools/screenshot.mjs --out <dir>
import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const EDGE = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const here = path.dirname(fileURLToPath(import.meta.url));
const page = path.resolve(here, '../index.html');
const argv = process.argv.slice(2);
const outIdx = argv.indexOf('--out');
if (outIdx < 0 || !argv[outIdx + 1]) {
  console.error('usage: node site/tools/screenshot.mjs --out <dir>');
  process.exit(2);
}
const out = path.resolve(argv[outIdx + 1]);
mkdirSync(out, { recursive: true });
if (!existsSync(page)) {
  console.error(`missing ${page}; run node site/build.mjs first`);
  process.exit(2);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const freePort = () =>
  new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.unref();
    srv.on('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });

const profile = mkdtempSync(path.join(tmpdir(), 'pm-site-shot-'));
function cleanup() {
  if (process.platform === 'win32') {
    const needle = profile.replace(/'/g, "''");
    spawnSync(
      'powershell',
      ['-NoProfile', '-Command', `Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" | Where-Object { $_.CommandLine -like '*${needle}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`],
      { windowsHide: true, stdio: 'ignore' },
    );
  }
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
}
process.on('exit', cleanup);
for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, () => process.exit(1));

const port = await freePort();
spawn(
  EDGE,
  ['--headless=new', '--disable-gpu', '--hide-scrollbars', `--remote-debugging-port=${port}`, '--remote-allow-origins=*', `--user-data-dir=${profile}`, 'about:blank'],
  { stdio: 'ignore', windowsHide: true },
).unref();

let target = null;
for (let i = 0; i < 80 && !target; i++) {
  try {
    const list = await (await fetch(`http://127.0.0.1:${port}/json`, { signal: AbortSignal.timeout(2000) })).json();
    target = list.find((t) => t.type === 'page') ?? null;
  } catch {
    /* not up yet */
  }
  if (!target) await sleep(250);
}
if (!target) {
  console.error('headless Edge did not start');
  process.exit(1);
}

const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve, { once: true });
  ws.addEventListener('error', reject, { once: true });
});
let nextId = 0;
const pending = new Map();
const requests = [];
const consoleErrors = [];
ws.addEventListener('message', (ev) => {
  const msg = JSON.parse(String(ev.data));
  if (msg.id !== undefined && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) reject(new Error(msg.error.message));
    else resolve(msg.result);
  } else if (msg.method === 'Network.requestWillBeSent') {
    requests.push(msg.params.request.url);
  } else if (msg.method === 'Runtime.exceptionThrown') {
    consoleErrors.push(msg.params.exceptionDetails?.text ?? 'exception');
  } else if (msg.method === 'Runtime.consoleAPICalled' && msg.params.type === 'error') {
    consoleErrors.push(msg.params.args.map((a) => a.value ?? a.description).join(' '));
  }
});
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const id = ++nextId;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
const evalJs = async (expression) => (await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })).result?.value;

await send('Page.enable');
await send('Network.enable');
await send('Runtime.enable');

const SECTIONS = ['top', 'pillars', 'tour', 'features', 'cowork', 'security', 'pricing', 'faq'];
const report = [];

async function shootPage({ width, height, scheme, tag, sections }) {
  await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
  await send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: scheme }] });
  await send('Page.navigate', { url: pathToFileURL(page).href });
  for (let i = 0; i < 60; i++) {
    await sleep(300);
    if ((await evalJs('document.readyState')) === 'complete') break;
  }
  // Lazy images that are below the fold would not load for a full-page shot, so load them all first.
  await evalJs("document.querySelectorAll('img[loading=lazy]').forEach((i) => { i.loading = 'eager'; })");
  // Fonts and every image decoded before the shot.
  await evalJs('document.fonts.ready.then(() => true)');
  await evalJs('Promise.all([...document.images].map((i) => i.decode().catch(() => null))).then(() => true)');
  await sleep(1200);

  const m = await evalJs(`({
    scrollW: document.documentElement.scrollWidth,
    innerW: window.innerWidth,
    docH: Math.ceil(document.documentElement.scrollHeight),
    images: document.images.length,
    brokenImages: [...document.images].filter((i) => !i.complete || i.naturalWidth === 0).map((i) => i.getAttribute('src')),
    wideImages: [...document.images].filter((i) => i.getBoundingClientRect().right > window.innerWidth + 1).map((i) => i.getAttribute('src')),
    theme: getComputedStyle(document.body).backgroundColor,
    fontsLoaded: [...document.fonts].filter((f) => f.status === 'loaded').map((f) => f.family.replace(/"/g, '')).filter((v, i, a) => a.indexOf(v) === i)
  })`);
  report.push({ tag, width, scheme, ...m, overflowX: m.scrollW > m.innerW || m.wideImages.length > 0 });

  const full = await send('Page.captureScreenshot', {
    format: 'png',
    captureBeyondViewport: true,
    clip: { x: 0, y: 0, width, height: m.docH, scale: 1 },
  });
  writeFileSync(path.join(out, `site-${tag}.png`), Buffer.from(full.data, 'base64'));

  if (sections) {
    for (const id of SECTIONS) {
      const r = await evalJs(`(() => { const e = document.getElementById(${JSON.stringify(id)}); const b = e.getBoundingClientRect(); return { y: b.top + window.scrollY, h: b.height }; })()`);
      const shot = await send('Page.captureScreenshot', {
        format: 'png',
        captureBeyondViewport: true,
        clip: { x: 0, y: Math.max(0, Math.floor(r.y)), width, height: Math.ceil(r.h), scale: 1 },
      });
      writeFileSync(path.join(out, `section-${tag}-${id}.png`), Buffer.from(shot.data, 'base64'));
    }
  }
}

await shootPage({ width: 1440, height: 900, scheme: 'dark', tag: '1440-dark', sections: true });
await shootPage({ width: 1440, height: 900, scheme: 'light', tag: '1440-light', sections: false });
await shootPage({ width: 390, height: 844, scheme: 'dark', tag: '390-dark', sections: true });
await shootPage({ width: 390, height: 844, scheme: 'light', tag: '390-light', sections: false });

const hosts = [...new Set(requests.filter((u) => !u.startsWith('data:') && !u.startsWith('about:')).map((u) => (u.startsWith('file:') ? 'file' : new URL(u).host)))];
const allowed = new Set(['file', 'fonts.googleapis.com', 'fonts.gstatic.com']);
const foreign = hosts.filter((h) => !allowed.has(h));
const lines = [
  `# Landing page evidence (${new Date().toISOString()})`,
  '',
  '## Request hosts (every network request, de-duplicated)',
  ...hosts.map((h) => `- ${h}`),
  '',
  foreign.length ? `FAIL: foreign hosts: ${foreign.join(', ')}` : 'PASS: only local files and Google Fonts (fonts.googleapis.com, fonts.gstatic.com).',
  '',
  '## Layout',
  ...report.map((r) => `- ${r.tag}: viewport ${r.innerW}, scrollWidth ${r.scrollW}, height ${r.docH}, horizontal overflow: ${r.overflowX ? 'YES (FAIL)' : 'no'}, images ${r.images}, broken: ${r.brokenImages.length ? r.brokenImages.join(', ') + ' (FAIL)' : 'none'}, past the edge: ${r.wideImages.length ? r.wideImages.join(', ') : 'none'}, body ${r.theme}, fonts loaded: ${r.fontsLoaded.join(', ') || 'none'}`),
  '',
  '## Console errors',
  ...(consoleErrors.length ? consoleErrors.map((e) => `- ${e}`) : ['none']),
  '',
];
writeFileSync(path.join(out, 'checks.md'), lines.join('\n'));
console.log(lines.join('\n'));
ws.close();
process.exit(foreign.length || report.some((r) => r.overflowX || r.brokenImages.length) ? 1 : 0);
