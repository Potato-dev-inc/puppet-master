#!/usr/bin/env node
// UI audit over the evidence harness: loads every scenario in headless Edge (DevTools protocol, no visible window)
// at one or more widths and reports, per scenario: horizontal page overflow, elements that spill past the viewport,
// text that overflows its box, clipped text without a title, snake_case shown as text, any coloured (non-grey)
// computed colour, and tiny tap targets on a phone.
// Usage: node scripts/evidence/audit.mjs [--widths 390,1440] [--json <file>] [scenario...]
import { spawn, spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const EDGE = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const appDir = path.join(root, 'packages/app');
const scenarioDir = path.join(appDir, 'src/dev/scenarios');
const argv = process.argv.slice(2);
let widths = [390, 1440];
let jsonOut = null;
const wanted = [];
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === '--widths') widths = argv[++i].split(',').map(Number);
  else if (argv[i] === '--json') jsonOut = argv[++i];
  else wanted.push(argv[i]);
}
const all = readdirSync(scenarioDir)
  .filter((f) => f.endsWith('.ts') && f !== 'index.ts')
  .map((f) => f.replace(/\.ts$/, ''));
const scenarios = wanted.length ? wanted : all;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const freePort = () =>
  new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.on('error', reject);
    srv.listen(0, 'localhost', () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });

const PORT = await freePort();
const base = `http://localhost:${PORT}`;
const TOKEN = randomBytes(16).toString('hex');
const require = createRequire(path.join(appDir, 'package.json'));
const viteBin = path.join(path.dirname(require.resolve('vite/package.json')), 'bin/vite.js');
const vite = spawn(process.execPath, [viteBin, '--port', String(PORT), '--strictPort', '--host', 'localhost'], {
  cwd: appDir,
  env: { ...process.env, VITE_PM_EVIDENCE_TOKEN: TOKEN },
  stdio: 'ignore',
  windowsHide: true,
});
const profile = mkdtempSync(path.join(tmpdir(), 'pm-audit-'));
function cleanup() {
  spawnSync('taskkill', ['/pid', String(vite.pid), '/T', '/F'], { windowsHide: true });
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
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
}
process.on('exit', cleanup);
for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, () => process.exit(1));

for (let i = 0; i < 180; i++) {
  try {
    const res = await fetch(`${base}/src/main.tsx`, { signal: AbortSignal.timeout(5000) });
    if (res.ok && (await res.text()).includes(TOKEN)) break;
  } catch {
    /* not up yet */
  }
  await sleep(500);
}

const dbg = await freePort();
spawn(
  EDGE,
  ['--headless=new', '--disable-gpu', '--hide-scrollbars', `--remote-debugging-port=${dbg}`, '--remote-allow-origins=*', `--user-data-dir=${profile}`, 'about:blank'],
  { stdio: 'ignore', windowsHide: true },
).unref();
let target = null;
for (let i = 0; i < 80 && !target; i++) {
  try {
    target = (await (await fetch(`http://127.0.0.1:${dbg}/json`)).json()).find((t) => t.type === 'page') ?? null;
  } catch {
    /* not up yet */
  }
  if (!target) await sleep(250);
}
if (!target) throw new Error('no Edge page');
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((res) => ws.addEventListener('open', res, { once: true }));
let nextId = 0;
const pending = new Map();
ws.addEventListener('message', (ev) => {
  const m = JSON.parse(String(ev.data));
  if (m.id !== undefined && pending.has(m.id)) {
    const p = pending.get(m.id);
    pending.delete(m.id);
    if (m.error) p.reject(new Error(m.error.message));
    else p.resolve(m.result);
  }
});
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const id = ++nextId;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
await send('Page.enable');

// Runs in the page; returns short finding strings.
const PAGE_AUDIT = String.raw`(() => {
  const out = [];
  const seen = new Set();
  const add = (key, text) => { if (!seen.has(key)) { seen.add(key); out.push(text); } };
  const vw = document.documentElement.clientWidth;
  const desc = (el) => {
    const t = (el.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 40);
    const cls = typeof el.className === 'string' ? el.className.split(' ').filter(Boolean).slice(0, 2).join('.') : '';
    return el.tagName.toLowerCase() + (cls ? '.' + cls : '') + (t ? ' "' + t + '"' : '');
  };
  if (document.documentElement.scrollWidth > vw + 1) out.push('PAGE-HSCROLL scrollWidth=' + document.documentElement.scrollWidth + ' vw=' + vw);
  const visible = (el) => { const r = el.getBoundingClientRect(); const cs = getComputedStyle(el); return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden' && cs.display !== 'none'; };
  const clipped = (el) => { for (let p = el.parentElement; p; p = p.parentElement) { const o = getComputedStyle(p).overflowX; if (o === 'auto' || o === 'scroll' || o === 'hidden' || o === 'clip') return true; } return false; };
  for (const el of document.querySelectorAll('body *')) {
    if (!visible(el)) continue;
    if (el.closest('.xterm, canvas, svg, [data-terminal]')) continue;
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    const directText = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
    if (r.right > vw + 1 && !clipped(el) && directText) add('vp' + desc(el), 'OFF-RIGHT ' + desc(el) + ' right=' + Math.round(r.right));
    if (directText && el.clientWidth > 0 && el.scrollWidth > el.clientWidth + 2 && cs.display !== 'inline') {
      if (cs.overflowX === 'visible') add('ov' + desc(el), 'TEXT-OVERFLOW ' + desc(el) + ' ' + el.scrollWidth + '>' + el.clientWidth);
      else if ((cs.textOverflow === 'ellipsis' || cs.overflowX === 'hidden') && !el.title && !el.getAttribute('aria-label')) add('cl' + desc(el), 'TRUNCATED-NO-TITLE ' + desc(el));
    }
    if (directText && !/^(PRE|TEXTAREA|INPUT|CODE)$/.test(el.tagName) && !el.closest('pre,code')) {
      for (const n of el.childNodes) {
        if (n.nodeType !== 3) continue;
        const m = n.textContent.match(/\b[a-z][a-z0-9]*(?:_[a-z0-9]+)+\b/);
        if (m) add('sn' + m[0], 'RAW-TEXT ' + m[0] + ' in ' + desc(el));
      }
    }
    for (const prop of ['color', 'backgroundColor', 'borderTopColor']) {
      const m = cs[prop].match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
      if (!m) continue;
      const rgb = [+m[1], +m[2], +m[3]];
      const a = m[4] === undefined ? 1 : +m[4];
      if (a > 0.05 && Math.max(...rgb) - Math.min(...rgb) > 18) add('co' + prop + cs[prop], 'COLOUR ' + prop + ' ' + cs[prop] + ' on ' + desc(el));
    }
    if (vw <= 500 && /^(BUTTON|A)$/.test(el.tagName) && (r.height < 30 || r.width < 30)) add('ht' + desc(el), 'SMALL-TARGET ' + desc(el) + ' ' + Math.round(r.width) + 'x' + Math.round(r.height));
  }

  const parse = (c) => { const m = c.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/); return m ? [+m[1], +m[2], +m[3], m[4] === undefined ? 1 : +m[4]] : null; };
  const lum = ([r, g, b]) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); }; return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b); };
  const bgOf = (el) => { let base = [255, 255, 255]; const chain = []; for (let p = el; p; p = p.parentElement) chain.push(p); for (const p of chain.reverse()) { const c = parse(getComputedStyle(p).backgroundColor); if (c && c[3] > 0) base = base.map((v, i) => Math.round(c[i] * c[3] + v * (1 - c[3]))); } return base; };
  for (const el of document.querySelectorAll('body *')) {
    if (!visible(el) || el.closest('.xterm, canvas, svg')) continue;
    const direct = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim().length > 1);
    if (!direct) continue;
    const cs = getComputedStyle(el);
    const fg = parse(cs.color);
    if (!fg || fg[3] < 0.05 || parseFloat(cs.opacity) < 0.5 || el.closest('[disabled], [aria-disabled=true]')) continue;
    const bg = bgOf(el);
    const eff = fg.slice(0, 3).map((v, i) => Math.round(v * fg[3] + bg[i] * (1 - fg[3])));
    const l1 = lum(eff), l2 = lum(bg);
    const ratio = (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
    if (ratio < 2.5) add('ct' + desc(el), 'LOW-CONTRAST ' + ratio.toFixed(2) + ' ' + desc(el));
  }
  return out;
})()`;

const report = {};
for (const w of widths) {
  const h = w <= 500 ? 844 : 900;
  await send('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false });
  for (const name of scenarios) {
    await send('Page.navigate', { url: `${base}/?harness=${encodeURIComponent(name)}` });
    let ready = false;
    for (let i = 0; i < 80 && !ready; i++) {
      await sleep(400);
      try {
        ready = (await send('Runtime.evaluate', { expression: 'window.__harnessReady === true', returnByValue: true })).result?.value === true;
      } catch {
        /* navigating */
      }
    }
    await sleep(300);
    let findings = ['NOT-READY'];
    if (ready) {
      const r = await send('Runtime.evaluate', { expression: PAGE_AUDIT, returnByValue: true });
      findings = r.result?.value ?? ['EVAL-ERROR ' + JSON.stringify(r.exceptionDetails?.text)];
    }
    report[`${name}@${w}`] = findings;
    console.log(`${name}@${w}: ${findings.length ? '\n  ' + findings.join('\n  ') : 'clean'}`);
  }
}
if (jsonOut) writeFileSync(jsonOut, JSON.stringify(report, null, 2));
process.exit(0);
