// The page driver: find things by what a person sees, click them with a visible cursor, type, press keys, wait.
//
// Targets (the first argument of click, hover, waitFor, ...):
//   'Send'                    visible text (case-insensitive substring; also aria-label, title, placeholder)
//   'text=Send'               the same, explicit
//   'exact=Send'              the whole text must be "Send" (case-sensitive)
//   're=/^Send( now)?$/i'     a regular expression on the text
//   'css=button.primary'      a CSS selector (use it when text is not enough, never to assert styling)
//   'aria=Close'              the aria-label
//   { text: 'Send', within: 'css=.composer', nth: 1 }   an object with options
// When several elements match, interactive ones (button, link, tab, input ...) and shorter texts come first;
// `nth` picks one of them (0-based).
import fs from 'node:fs';
import { sleep } from './cdp.mjs';
import { overlaySource } from './inject.mjs';

// Runs inside the page. Defined once per page load by ensureFinder().
const FINDER_SOURCE = `(()=>{
  if (window.__pmE2eFind) return;
  const INTERACTIVE = 'button,a,[role=button],[role=tab],[role=menuitem],[role=option],[role=switch],[role=checkbox],[role=radio],label,input,textarea,select,summary,[tabindex]';
  const norm = (s) => (s || '').replace(/\\s+/g, ' ').trim();
  const vis = (e) => {
    const r = e.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) return false;
    const cs = getComputedStyle(e);
    return cs.visibility !== 'hidden' && cs.display !== 'none' && +cs.opacity !== 0;
  };
  const labelOf = (e) => {
    const parts = [];
    if (e.matches('input,textarea,select')) { if (e.placeholder) parts.push(e.placeholder); if (e.type !== 'password' && e.value) parts.push(e.value); }
    else parts.push(e.innerText);
    for (const a of ['aria-label', 'title', 'alt']) if (e.getAttribute && e.getAttribute(a)) parts.push(e.getAttribute(a));
    return norm(parts.join(' \\u0001 '));
  };
  const scopeOf = (within) => {
    if (!within) return document;
    if (typeof within === 'string' && within.startsWith('css=')) return [...document.querySelectorAll(within.slice(4))].find(vis) || null;
    const w = String(within).toLowerCase();
    const all = [...document.body.querySelectorAll('*')].filter((e) => vis(e) && norm(e.innerText).toLowerCase().includes(w));
    all.sort((a, b) => a.innerText.length - b.innerText.length);
    return all[0] || null;
  };
  window.__pmE2eFind = (spec) => {
    const scope = scopeOf(spec.within);
    if (!scope) return { count: 0, items: [] };
    let list;
    if (spec.css) list = [...scope.querySelectorAll(spec.css)].filter((e) => vis(e) && !e.closest('#__pmE2e'));
    else {
      let test;
      if (spec.re) { const rx = new RegExp(spec.re.source, spec.re.flags); test = (t) => rx.test(t); }
      else if (spec.exact != null) test = (t) => t.split(' \\u0001 ').some((p) => p === spec.exact);
      else if (spec.aria != null) test = null;
      else { const w = String(spec.text).toLowerCase(); test = (t) => t.toLowerCase().includes(w); }
      const els = [...scope.querySelectorAll('*')].filter((e) => !['SCRIPT', 'STYLE', 'HEAD', 'META', 'LINK', 'NOSCRIPT'].includes(e.tagName) && !e.closest('#__pmE2e') && vis(e));
      const hit = spec.aria != null ? els.filter((e) => e.getAttribute('aria-label') === spec.aria || (e.getAttribute('aria-label') || '').includes(spec.aria)) : els.filter((e) => test(labelOf(e)));
      const set = new Set(hit);
      // innermost: drop an element when a descendant also matches
      list = hit.filter((e) => ![...e.querySelectorAll('*')].some((d) => set.has(d)));
      const score = (e) => (e.closest(INTERACTIVE) ? 0 : 1000) + Math.min(labelOf(e).length, 900);
      list.sort((a, b) => score(a) - score(b));
    }
    const items = list.map((e) => { const b = e.getBoundingClientRect(); return { x: b.x, y: b.y, w: b.width, h: b.height, tag: e.tagName.toLowerCase(), text: labelOf(e).slice(0, 80) }; });
    return { count: items.length, items, nth: spec.nth || 0 };
  };
})()`;

export function parseTarget(t, extra = {}) {
  if (t && typeof t === 'object' && !(t instanceof RegExp)) return { ...t, ...extra };
  if (t instanceof RegExp) return { re: { source: t.source, flags: t.flags }, ...extra };
  const s = String(t);
  if (s.startsWith('css=')) return { css: s.slice(4), ...extra };
  if (s.startsWith('aria=')) return { aria: s.slice(5), ...extra };
  if (s.startsWith('exact=')) return { exact: s.slice(6), ...extra };
  if (s.startsWith('text=')) return { text: s.slice(5), ...extra };
  const m = s.match(/^re=\/(.*)\/([a-z]*)$/s);
  if (m) return { re: { source: m[1], flags: m[2] }, ...extra };
  return { text: s, ...extra };
}
export const describeTarget = (t) => (typeof t === 'string' ? JSON.stringify(t) : JSON.stringify(t instanceof RegExp ? String(t) : t));

const KEYS = {
  Enter: [13, 'Enter', '\r'], Escape: [27, 'Escape'], Esc: [27, 'Escape'], Tab: [9, 'Tab'], Backspace: [8, 'Backspace'], Delete: [46, 'Delete'],
  ArrowDown: [40, 'ArrowDown'], ArrowUp: [38, 'ArrowUp'], ArrowLeft: [37, 'ArrowLeft'], ArrowRight: [39, 'ArrowRight'],
  Home: [36, 'Home'], End: [35, 'End'], PageUp: [33, 'PageUp'], PageDown: [34, 'PageDown'], Space: [32, ' ', ' '],
};
for (let i = 1; i <= 12; i++) KEYS['F' + i] = [111 + i, 'F' + i];

export class Page {
  /**
   * @param conn CDP connection (cdp.mjs)
   * @param {{ pace?: number, testId?: string, isAborted?: () => boolean, defaultTimeout?: number }} opts
   *   pace 1 = the speed of a person watching (video on), 0.2 = as fast as is safe
   */
  constructor(conn, opts = {}) {
    this.c = conn;
    this.pace = opts.pace ?? 1;
    this.testId = opts.testId || '';
    this.isAborted = opts.isAborted || (() => false);
    this.defaultTimeout = opts.defaultTimeout ?? 10000;
    this.pos = [300, 300];
  }

  eval(js) { return this.c.evalJs(js); }
  pause(ms) { return sleep(Math.max(0, Math.round(ms * this.pace))); }

  async inject() {
    await this.c.evalJs(FINDER_SOURCE);
    await this.c.evalJs(overlaySource({ testId: this.testId }));
  }

  async ensure() {
    const ok = await this.c.evalJs('!!window.__pmE2e && !!window.__pmE2eFind').catch(() => false);
    if (!ok) await this.inject();
  }

  async step(name) { await this.ensure(); await this.c.evalJs(`window.__pmE2e.step(${JSON.stringify(name)})`); }

  async find(target, extra) {
    await this.ensure();
    const spec = parseTarget(target, extra);
    const r = await this.c.evalJs(`window.__pmE2eFind(${JSON.stringify(spec)})`);
    const i = spec.nth || 0;
    return { count: r.count, rect: r.items[i] || null, items: r.items };
  }

  async count(target, extra) { return (await this.find(target, extra)).count; }
  async has(target, extra) { return (await this.count(target, extra)) > 0; }

  /** The visible text of the whole page. */
  async text() { return this.c.evalJs('document.body ? document.body.innerText : ""'); }

  async snippet() {
    const t = (await this.text().catch(() => '')).replace(/\s+/g, ' ').trim();
    return t.length > 500 ? t.slice(0, 500) + ' ...' : t;
  }

  /** Wait until the target is on screen (state 'visible', the default) or off screen ('gone'). Returns its rectangle. */
  async waitFor(target, opts = {}) {
    const timeout = opts.timeout ?? this.defaultTimeout;
    const state = opts.state || 'visible';
    const end = Date.now() + timeout;
    let last = null;
    for (;;) {
      if (this.isAborted()) throw new Error('the test was stopped');
      const r = await this.find(target, opts).catch((e) => { last = e; return null; });
      if (r) {
        if (state === 'visible' && r.rect) return r.rect;
        if (state === 'gone' && r.count === 0) return null;
      }
      if (Date.now() > end) break;
      await sleep(120);
    }
    const what = state === 'gone' ? 'to disappear' : 'to appear';
    throw new Error(`timed out after ${timeout} ms waiting for ${describeTarget(target)} ${what}${last ? ' (' + last.message + ')' : ''}. The page shows: ${await this.snippet()}`);
  }

  async waitGone(target, opts = {}) { return this.waitFor(target, { ...opts, state: 'gone' }); }

  async locate(target, opts = {}) {
    const rect = await this.waitFor(target, opts);
    // scroll into view when needed, then measure again
    const inView = rect.y >= 0 && rect.y + rect.h <= (await this.c.evalJs('innerHeight')) && rect.x >= 0;
    if (!inView) {
      await this.scrollIntoView(target, opts);
      await sleep(150);
      return (await this.find(target, opts)).rect || rect;
    }
    return rect;
  }

  async scrollIntoView(target, opts = {}) {
    const spec = parseTarget(target, opts);
    const r = await this.c.evalJs(`window.__pmE2eFind(${JSON.stringify(spec)})`);
    const it = r.items[spec.nth || 0];
    if (!it) return;
    await this.c.evalJs(`(()=>{const el=document.elementFromPoint(${Math.max(1, Math.min(it.x + it.w / 2, 10000))},${Math.max(1, Math.min(it.y + it.h / 2, 10000))});if(el)el.scrollIntoView({block:'center',inline:'nearest'});})()`);
  }

  async moveTo(x, y) {
    this.pos = [x, y];
    await this.c.evalJs(`window.__pmE2e&&window.__pmE2e.cursor(${x},${y})`);
    await this.pause(380);
    await this.c.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
  }

  async hover(target, opts = {}) {
    const r = await this.locate(target, opts);
    await this.moveTo(r.x + r.w / 2, r.y + r.h / 2);
    return r;
  }

  async click(target, opts = {}) {
    const r = Array.isArray(target) ? { x: target[0] - 1, y: target[1] - 1, w: 2, h: 2 } : await this.locate(target, opts);
    const x = Math.round(r.x + r.w / 2 + (opts.dx || 0));
    const y = Math.round(r.y + r.h / 2 + (opts.dy || 0));
    await this.moveTo(x, y);
    await this.c.evalJs(`window.__pmE2e&&window.__pmE2e.ripple(${x},${y})`);
    const button = opts.button || 'left';
    const clickCount = opts.double ? 2 : 1;
    for (let n = 1; n <= clickCount; n++) {
      await this.c.send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button, clickCount: n });
      await sleep(50);
      await this.c.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button, clickCount: n });
    }
    await this.pause(opts.after ?? 350);
    return r;
  }

  async rightClick(target, opts = {}) { return this.click(target, { ...opts, button: 'right' }); }
  async doubleClick(target, opts = {}) { return this.click(target, { ...opts, double: true }); }

  /** Type text into whatever has focus (or into `opts.into`, which is clicked first). */
  async type(text, opts = {}) {
    if (opts.into) await this.click(opts.into, opts);
    const delay = opts.delay ?? Math.round(28 * this.pace);
    if (delay <= 2) { await this.c.send('Input.insertText', { text }); }
    else for (const ch of text) { await this.c.send('Input.insertText', { text: ch }); await sleep(delay); }
    await this.pause(150);
  }

  /** Replace the contents of a field: click, select all, type. */
  async fill(target, text, opts = {}) {
    await this.click(target, opts);
    await this.c.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', windowsVirtualKeyCode: 65, key: 'a', code: 'KeyA', modifiers: 2, commands: ['selectAll'] });
    await this.c.send('Input.dispatchKeyEvent', { type: 'keyUp', windowsVirtualKeyCode: 65, key: 'a', code: 'KeyA', modifiers: 2 });
    await this.c.send('Input.dispatchKeyEvent', { type: 'keyDown', windowsVirtualKeyCode: 8, key: 'Backspace', code: 'Backspace' });
    await this.c.send('Input.dispatchKeyEvent', { type: 'keyUp', windowsVirtualKeyCode: 8, key: 'Backspace', code: 'Backspace' });
    if (text) await this.type(text, { delay: opts.delay });
  }

  /** press('Enter'), press('Ctrl+K'), press('Shift+Tab'). Shows a key cap in the video. */
  async press(combo, opts = {}) {
    const parts = combo.split('+');
    const k = parts.pop();
    const mods = parts.reduce((m, p) => m | ({ Alt: 1, Ctrl: 2, Control: 2, Meta: 4, Cmd: 4, Shift: 8 }[p] || 0), 0);
    const known = KEYS[k];
    const vk = known ? known[0] : k.toUpperCase().charCodeAt(0);
    const key = known ? known[1] : (mods & 8 ? k.toUpperCase() : k.toLowerCase());
    const text = known ? known[2] : (mods & ~8 ? undefined : key);
    const code = known ? known[1] : k.length === 1 ? 'Key' + k.toUpperCase() : k;
    if (opts.show !== false) await this.c.evalJs(`window.__pmE2e&&window.__pmE2e.keycap(${JSON.stringify(combo.replace(/\+/g, ' + '))})`);
    await this.c.send('Input.dispatchKeyEvent', { type: text !== undefined ? 'keyDown' : 'rawKeyDown', key, code, windowsVirtualKeyCode: vk, modifiers: mods, text });
    await this.c.send('Input.dispatchKeyEvent', { type: 'keyUp', key, code, windowsVirtualKeyCode: vk, modifiers: mods });
    await this.pause(opts.after ?? 250);
  }

  async wheel(dy, at) {
    const [x, y] = at || this.pos;
    await this.c.send('Input.dispatchMouseEvent', { type: 'mouseWheel', x, y, deltaX: 0, deltaY: dy });
    await this.pause(300);
  }

  async value(target, opts = {}) {
    const rect = await this.waitFor(target, opts);
    return this.c.evalJs(`(()=>{const el=document.elementFromPoint(${rect.x + rect.w / 2},${rect.y + rect.h / 2});return el&&('value' in el)?el.value:(el?el.innerText:null)})()`);
  }

  async screenshot(file) {
    const r = await this.c.send('Page.captureScreenshot', { format: 'png' });
    fs.writeFileSync(file, Buffer.from(r.data, 'base64'));
    return file;
  }

  async reload() {
    await this.c.send('Page.reload', { ignoreCache: false });
    await sleep(600);
  }
}
