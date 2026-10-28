// Point and Fix: the click-to-pick script (PF-7).
//
// Rust injects this file into each `preview-*` window with initialization_script,
// after it replaces the two placeholders below with JSON string literals. The page
// never calls Puppet Master. Rust pulls picks back with `take()` (point/window.rs).
//
// Guard (S5): the script does nothing unless it runs in the top document, on the
// exact origin it was built for. Page text is data: it is read, never run.
(function () {
  'use strict';

  const ORIGIN = __PM_ORIGIN__;
  const KEY = __PM_KEY__;

  let topOk = false;
  let here = '';
  try {
    topOk = window.top === window;
    here = location.origin;
  } catch (err) {
    return;
  }
  if (!topOk || here !== ORIGIN || Object.prototype.hasOwnProperty.call(window, KEY)) return;

  // Built-ins are captured now, before any page script runs, so a page cannot
  // swap them to change what the picker reads (S6, S8).
  const doc = document;
  const win = window;
  const getAttr = Element.prototype.getAttribute;
  const hasAttr = Element.prototype.hasAttribute;
  const attachShadow = Element.prototype.attachShadow;
  const getRect = Element.prototype.getBoundingClientRect;
  const docQsa = Document.prototype.querySelectorAll;
  const arrSlice = Array.prototype.slice;
  const arrFilter = Array.prototype.filter;
  const jsonStringify = JSON.stringify;
  const addListener = win.addEventListener.bind(win);
  const getStyle = win.getComputedStyle.bind(win);
  const setTimer = win.setTimeout.bind(win);
  const clearTimer = win.clearTimeout.bind(win);
  const nextFrameRaw = typeof win.requestAnimationFrame === 'function' ? win.requestAnimationFrame.bind(win) : null;
  const cssEscape = win.CSS && typeof win.CSS.escape === 'function' ? win.CSS.escape.bind(win.CSS) : null;

  const MAX_TEXT = 160;
  const MAX_SELECTOR = 300;
  const MAX_HTML = 300;
  const MAX_TITLE = 120;
  const MAX_URL = 2048;
  const MAX_ATTR = 200;
  const MAX_STYLE = 80;
  const MAX_NAME = 120;
  const MAX_FILE = 300;
  const MAX_ATTRS = 8;
  const MAX_LEVELS = 5;
  const MAX_QUEUE = 10;
  const MAX_WALK = 25;
  const TOAST_MS = 2000;
  const STYLE_NAMES = ['font-size', 'font-weight', 'color', 'background-color', 'padding', 'margin', 'display', 'border-radius'];
  const ATTR_NAMES = ['id', 'data-testid', 'aria-label', 'role', 'name', 'href'];
  const URL_ATTRS = ['href', 'src', 'action'];
  const LIBRARY_FRAME = /node_modules|react-dom|react\/jsx/;

  // Pure helpers ------------------------------------------------------------

  function collapse(value) {
    return String(value == null ? '' : value).replace(/\s+/g, ' ').trim();
  }

  // Cuts by characters, not UTF-16 units, so an emoji is never split.
  function cut(value, max) {
    const chars = Array.from(String(value));
    return chars.length > max ? chars.slice(0, max).join('') : chars.join('');
  }

  function attrOf(el, name) {
    return hasAttr.call(el, name) ? getAttr.call(el, name) : null;
  }

  function safe(fn, fallback) {
    try {
      return fn();
    } catch (err) {
      return fallback;
    }
  }

  function num(value) {
    const n = Number(value);
    return Number.isFinite(n) ? n : 0;
  }

  function toLine(value) {
    const n = parseInt(value, 10);
    return Number.isFinite(n) && n >= 0 ? n : null;
  }

  function escapeHtml(value) {
    return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  // Names from CSS-in-JS and CSS modules change on every build, so they are not stable.
  function isHashed(name) {
    const base = name.charAt(0) === '_' ? name.slice(1) : name;
    if (/^(css|sc|jsx|svelte)-/.test(name)) return true;
    return /^[a-z0-9]{5,}$/.test(base) && /[0-9]/.test(base) && /[a-z]/.test(base);
  }

  function stableClasses(el) {
    const raw = attrOf(el, 'class');
    if (!raw) return [];
    return collapse(raw)
      .split(' ')
      .filter((name) => name && !isHashed(name))
      .slice(0, 2);
  }

  function stableId(el) {
    const id = attrOf(el, 'id');
    return id && id.indexOf(':') === -1 && !isHashed(id) ? id : null;
  }

  function cssEsc(value) {
    const text = String(value);
    if (cssEscape) return cssEscape(text);
    return text.replace(/[^\w\u00a0-\uffff-]/g, (c) => '\\' + c).replace(/^\d/, (c) => '\\3' + c + ' ');
  }

  // Selectors ---------------------------------------------------------------

  function isUnique(selector, el) {
    try {
      const found = docQsa.call(doc, selector);
      return found.length === 1 && found[0] === el;
    } catch (err) {
      return false;
    }
  }

  function segmentOf(el) {
    let text = el.tagName.toLowerCase();
    for (const name of stableClasses(el)) text += '.' + cssEsc(name);
    const parent = el.parentElement;
    if (parent) {
      const same = arrFilter.call(parent.children, (child) => child.tagName === el.tagName);
      if (same.length > 1) text += ':nth-of-type(' + (same.indexOf(el) + 1) + ')';
    }
    return text;
  }

  // Order: #id, then data-testid, then a path of at most five levels. The path is
  // checked for uniqueness at each level. If none is unique, the five-level path
  // is returned anyway; it is a hint, not a lookup key.
  function selectorFor(el) {
    const id = stableId(el);
    if (id) {
      const byId = '#' + cssEsc(id);
      if (isUnique(byId, el)) return cut(byId, MAX_SELECTOR);
    }
    const testId = attrOf(el, 'data-testid');
    if (testId) {
      const byTest = '[data-testid="' + testId.replace(/["\\]/g, '\\$&') + '"]';
      if (isUnique(byTest, el)) return cut(byTest, MAX_SELECTOR);
    }
    const parts = [];
    let node = el;
    while (node && node.nodeType === 1 && node !== doc.documentElement && parts.length < MAX_LEVELS) {
      parts.unshift(segmentOf(node));
      const path = parts.join(' > ');
      if (isUnique(path, el)) return cut(path, MAX_SELECTOR);
      node = node.parentElement;
    }
    return cut(parts.join(' > '), MAX_SELECTOR);
  }

  function labelOf(el) {
    const id = stableId(el);
    if (id) return el.tagName.toLowerCase() + '#' + id;
    let text = el.tagName.toLowerCase();
    for (const name of stableClasses(el)) text += '.' + name;
    return text;
  }

  // Text, HTML and attributes -----------------------------------------------

  function textOf(el) {
    const tag = el.tagName;
    const type = String(attrOf(el, 'type') || '').toLowerCase();
    if (tag === 'INPUT' && type === 'password') return '';
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || hasAttr.call(el, 'contenteditable')) {
      // Never the typed value: only the hints the page gives for the field.
      const hint = attrOf(el, 'placeholder') || attrOf(el, 'aria-label') || attrOf(el, 'name') || '';
      return cut(collapse(hint), MAX_TEXT);
    }
    const raw = typeof el.innerText === 'string' ? el.innerText : el.textContent;
    return cut(collapse(String(raw == null ? '' : raw).slice(0, MAX_TEXT * 8)), MAX_TEXT);
  }

  function pathOf(href) {
    try {
      const url = new URL(href, location.href);
      return url.protocol === 'http:' || url.protocol === 'https:' ? url.pathname : null;
    } catch (err) {
      return null;
    }
  }

  // The opening tag only. No value attributes; URLs keep their path only.
  function openingTag(el) {
    let out = '<' + el.tagName.toLowerCase();
    for (const attr of arrSlice.call(el.attributes)) {
      const name = attr.name.toLowerCase();
      if (name === 'value') continue;
      let value = String(attr.value);
      if (/^\s*data:/i.test(value)) {
        value = cut(value.trim(), 20) + '...';
      } else if (URL_ATTRS.indexOf(name) !== -1) {
        const path = pathOf(value);
        if (path === null) continue;
        value = path;
      } else if (name === 'style') {
        value = cut(value, 60);
      }
      out += ' ' + name + '="' + escapeHtml(value) + '"';
    }
    return cut(out + '>', MAX_HTML);
  }

  function attrsOf(el) {
    const out = [];
    for (const name of ATTR_NAMES) {
      if (out.length >= MAX_ATTRS) break;
      const raw = attrOf(el, name);
      if (raw == null) continue;
      const value = name === 'href' ? pathOf(raw) : raw;
      if (value != null) out.push([name, cut(collapse(value), MAX_ATTR)]);
    }
    return out;
  }

  function stylesOf(el) {
    const style = getStyle(el);
    const out = [];
    for (const name of STYLE_NAMES) {
      const value = collapse(style.getPropertyValue(name));
      if (value) out.push([name, cut(value, MAX_STYLE)]);
    }
    return out;
  }

  function rectOf(el) {
    const r = safe(() => getRect.call(el), null) || { left: 0, top: 0, width: 0, height: 0 };
    return { x: num(r.left), y: num(r.top), w: num(r.width), h: num(r.height) };
  }

  // Component and source (best effort; every read is guarded) --------------

  // Walks up from the element (inclusive) and returns the first non-empty answer.
  function nearest(el, read) {
    let node = el;
    for (let i = 0; node && node.nodeType === 1 && i < MAX_WALK; i++, node = node.parentElement) {
      const found = safe(() => read(node), null);
      if (found) return found;
    }
    return null;
  }

  function hint(file, line, column, how) {
    const name = cut(collapse(file), MAX_FILE);
    return name ? { file: name, line: toLine(line), column: toLine(column), how } : null;
  }

  function dataComponentAt(node) {
    const value = attrOf(node, 'data-component');
    return value ? cut(collapse(value), MAX_NAME) : null;
  }

  function dataSourceAt(node) {
    const rel = attrOf(node, 'data-inspector-relative-path');
    if (rel) return hint(rel, attrOf(node, 'data-inspector-line'), attrOf(node, 'data-inspector-column'), 'data-attr');
    const vue = attrOf(node, 'data-v-inspector');
    const vueLoc = vue ? /^(.+):(\d+):(\d+)$/.exec(vue) : null;
    if (vueLoc) return hint(vueLoc[1], vueLoc[2], vueLoc[3], 'data-attr');
    const astroFile = attrOf(node, 'data-astro-source-file');
    if (astroFile) {
      const loc = /^(\d+)(?::(\d+))?$/.exec(collapse(attrOf(node, 'data-astro-source-loc') || ''));
      return hint(astroFile, loc ? loc[1] : null, loc ? loc[2] : null, 'data-attr');
    }
    const file = attrOf(node, 'data-source-file');
    if (file) return hint(file, attrOf(node, 'data-source-line'), attrOf(node, 'data-source-column'), 'data-attr');
    return null;
  }

  function svelteAt(node) {
    const loc = node.__svelte_meta && node.__svelte_meta.loc;
    return loc && typeof loc.file === 'string' ? hint(loc.file, loc.line, loc.column, 'svelte-meta') : null;
  }

  function vueAt(node) {
    const type = node.__vueParentComponent && node.__vueParentComponent.type;
    if (!type) return null;
    const name = type.__name || type.name || '';
    const file = typeof type.__file === 'string' ? hint(type.__file, null, null, 'vue-file') : null;
    if (!name && !file) return null;
    return { name: name ? cut(name, MAX_NAME) : null, source: file };
  }

  function angularName(el) {
    const ng = win.ng;
    const owner = ng && typeof ng.getOwningComponent === 'function' ? ng.getOwningComponent(el) : null;
    const name = owner && owner.constructor ? owner.constructor.name : '';
    return name ? cut(name, MAX_NAME) : null;
  }

  function typeName(type) {
    if (typeof type === 'function') return type.displayName || type.name || '';
    if (type && typeof type === 'object') {
      if (type.displayName) return type.displayName;
      const inner = type.render || type.type; // forwardRef and memo wrap the real type
      return inner ? typeName(inner) : '';
    }
    return '';
  }

  // First file outside libraries in a stack string. Line is unknown here.
  function stackFile(stack) {
    for (const line of String(stack).split('\n')) {
      if (LIBRARY_FRAME.test(line)) continue;
      const m = /([^\s()]+):\d+:\d+\)?\s*$/.exec(line);
      if (!m) continue;
      const raw = m[1];
      if (/^https?:\/\//i.test(raw)) {
        const path = safe(() => new URL(raw).pathname, '');
        if (path) return path;
        continue;
      }
      return raw;
    }
    return '';
  }

  function fiberSource(fiber) {
    const debugSource = fiber._debugSource;
    if (debugSource && typeof debugSource.fileName === 'string') {
      return hint(debugSource.fileName, debugSource.lineNumber, debugSource.columnNumber, 'react-debug-source');
    }
    const stack = fiber._debugStack && fiber._debugStack.stack;
    const file = typeof stack === 'string' ? stackFile(stack) : '';
    return file ? hint(file, null, null, 'stack') : null;
  }

  // React fibers: the element's own fiber first, then up to the nearest named
  // component. The source is taken only from fibers below that component, so a
  // parent's JSX line is never reported as this component's source.
  function reactInfo(el) {
    const key = Object.keys(el).find((k) => k.indexOf('__reactFiber$') === 0);
    if (!key) return null;
    let fiber = el[key];
    let name = '';
    let source = null;
    for (let i = 0; fiber && i < MAX_WALK; i++, fiber = fiber.return) {
      name = typeName(fiber.type);
      if (name) break;
      if (!source) source = safe(() => fiberSource(fiber), null);
    }
    return { name: name ? cut(name, MAX_NAME) : null, source };
  }

  // Returns { component, source } for the element. Each read is guarded.
  function frameworkInfo(el) {
    const explicit = nearest(el, dataComponentAt);
    const react = safe(() => reactInfo(el), null);
    const vue = nearest(el, vueAt);
    const angular = safe(() => angularName(el), null);
    const component = explicit || (react && react.name) || (vue && vue.name) || angular || null;
    const source =
      nearest(el, dataSourceAt) ||
      (react && react.source) ||
      nearest(el, svelteAt) ||
      (vue && vue.source) ||
      null;
    return { component, source };
  }

  // The pick -----------------------------------------------------------------

  function pickOf(el) {
    const fw = frameworkInfo(el);
    return {
      kind: 'pick',
      ready: true,
      url: cut(String(location.href).split('#')[0], MAX_URL),
      title: cut(collapse(doc.title), MAX_TITLE),
      tag: el.tagName.toLowerCase(),
      selector: selectorFor(el),
      text: textOf(el),
      html: openingTag(el),
      rect: rectOf(el),
      viewport: { w: num(win.innerWidth), h: num(win.innerHeight), dpr: num(win.devicePixelRatio) || 1 },
      attrs: attrsOf(el),
      styles: safe(() => stylesOf(el), []),
      component: fw.component,
      source: fw.source,
    };
  }

  // Picker state and UI (closed shadow root, so the page cannot reach it) ----

  const CSS_TEXT = [
    '.pill { display: inline-flex; align-items: center; gap: 8px; height: 28px; padding: 0 12px; box-sizing: border-box;',
    '  border: 1px solid rgba(255, 255, 255, 0.18); border-radius: 999px; background: #111; color: #f5f5f5;',
    '  font: 500 12px/1 system-ui, sans-serif; cursor: pointer; opacity: 0.5; box-shadow: 0 2px 8px rgba(0, 0, 0, 0.25);',
    '  transition: opacity 0.15s ease; }',
    '.pill:hover, .pill[data-armed] { opacity: 1; }',
    '.key { color: #a3a3a3; }',
    '.outline { position: fixed; display: none; box-sizing: border-box; pointer-events: none; border: 2px solid #111;',
    '  box-shadow: 0 0 0 1px #fff; background: rgba(17, 17, 17, 0.08); }',
    '.chip { position: fixed; display: none; pointer-events: none; padding: 4px 6px; border-radius: 4px; background: #111;',
    '  color: #fff; font: 500 11px/1 ui-monospace, Menlo, Consolas, monospace; white-space: nowrap; }',
    '.toast { position: fixed; left: 50%; bottom: 56px; transform: translateX(-50%); max-width: 70vw; padding: 8px 12px;',
    '  border-radius: 8px; background: #111; color: #fff; font: 500 12px/1.3 system-ui, sans-serif;',
    '  opacity: 0; pointer-events: none; transition: opacity 0.2s ease; }',
    '.toast.on { opacity: 1; }',
  ].join('\n');

  const queue = [];
  let armed = false;
  let current = null; // element the outline is on
  let hovered = null; // last element the mouse entered
  let chipFor = null;
  let chipText = '';
  const downStack = []; // elements passed with ArrowUp, for ArrowDown
  let host = null;
  let pill = null;
  let pillText = null;
  let pillKey = null;
  let outline = null;
  let chip = null;
  let toast = null;
  let toastTimer = null;
  let cursorStyle = null;

  function buildUi() {
    host = doc.createElement('div');
    host.style.cssText = 'all: initial; position: fixed; right: 12px; bottom: 12px; z-index: 2147483647;';
    const root = attachShadow.call(host, { mode: 'closed' });

    const style = doc.createElement('style');
    style.textContent = CSS_TEXT;

    pill = doc.createElement('button');
    pill.type = 'button';
    pill.className = 'pill';
    pill.title = 'Point at something to fix it';
    pillText = doc.createElement('span');
    pillText.textContent = 'Point';
    pillKey = doc.createElement('span');
    pillKey.className = 'key';
    pillKey.textContent = 'Alt+P';
    pill.append(pillText, pillKey);
    pill.addEventListener('click', (e) => {
      e.stopPropagation();
      toggle();
    });

    outline = doc.createElement('div');
    outline.className = 'outline';
    chip = doc.createElement('div');
    chip.className = 'chip';
    toast = doc.createElement('div');
    toast.className = 'toast';
    toast.setAttribute('role', 'status');

    root.append(style, outline, chip, toast, pill);
    doc.documentElement.appendChild(host);
  }

  // The pill goes on the page after load, as the spec asks, so a page that
  // builds its own <html> early does not remove it.
  function mount() {
    if (!host && doc.documentElement) buildUi();
  }

  function showArmed(on) {
    if (!pill) return;
    if (on) {
      pill.setAttribute('data-armed', '');
      pillText.textContent = 'Click an element';
      pillKey.textContent = 'Esc to stop';
    } else {
      pill.removeAttribute('data-armed');
      pillText.textContent = 'Point';
      pillKey.textContent = 'Alt+P';
    }
  }

  function hideBoxes() {
    if (outline) outline.style.display = 'none';
    if (chip) chip.style.display = 'none';
  }

  function paint() {
    if (!outline || !armed || !current || !current.isConnected) {
      hideBoxes();
      return;
    }
    const r = rectOf(current);
    if (r.w <= 0 || r.h <= 0) {
      hideBoxes();
      return;
    }
    outline.style.display = 'block';
    outline.style.left = r.x + 'px';
    outline.style.top = r.y + 'px';
    outline.style.width = r.w + 'px';
    outline.style.height = r.h + 'px';
    if (chipFor !== current) {
      chipFor = current;
      const fw = frameworkInfo(current);
      chipText = cut(labelOf(current) + (fw.component ? ' · ' + fw.component : ''), MAX_NAME);
    }
    chip.textContent = chipText;
    chip.style.display = 'block';
    chip.style.left = Math.max(0, r.x) + 'px';
    chip.style.top = (r.y >= 24 ? r.y - 22 : r.y + r.h + 4) + 'px';
  }

  function addCursor() {
    if (cursorStyle) return;
    cursorStyle = doc.createElement('style');
    cursorStyle.id = 'pm-point-cursor';
    cursorStyle.textContent = '*, *::before, *::after { cursor: crosshair !important; }';
    (doc.head || doc.documentElement).appendChild(cursorStyle);
  }

  function removeCursor() {
    if (cursorStyle) cursorStyle.remove();
    cursorStyle = null;
  }

  function arm() {
    if (armed) return;
    armed = true;
    current = null;
    hovered = null;
    downStack.length = 0;
    addCursor();
    showArmed(true);
  }

  function disarm() {
    armed = false;
    current = null;
    hovered = null;
    downStack.length = 0;
    removeCursor();
    hideBoxes();
    showArmed(false);
  }

  function toggle() {
    if (armed) disarm();
    else arm();
  }

  function enqueue(text) {
    if (queue.length >= MAX_QUEUE) queue.shift();
    queue.push(text);
  }

  function afterFrames(count, then) {
    if (count <= 0) {
      then();
      return;
    }
    if (nextFrameRaw) nextFrameRaw(() => afterFrames(count - 1, then));
    else setTimer(() => afterFrames(count - 1, then), 16);
  }

  function pick(el) {
    let payload = null;
    try {
      payload = pickOf(el);
    } catch (err) {
      payload = null;
    }
    disarm();
    if (payload) afterFrames(2, () => enqueue(jsonStringify(payload)));
  }

  // Events -------------------------------------------------------------------

  // Our own pill is a shadow host, so events from it arrive with target = host.
  function targetOf(e) {
    const node = e.target;
    if (!node || node.nodeType !== 1 || node === host) return null;
    return node;
  }

  function block(e) {
    e.preventDefault();
    e.stopImmediatePropagation();
  }

  function isAltP(e) {
    return (
      e.altKey &&
      !e.ctrlKey &&
      !e.metaKey &&
      !e.shiftKey &&
      !e.repeat &&
      (e.code === 'KeyP' || String(e.key).toLowerCase() === 'p')
    );
  }

  function climb() {
    const parent = current && current.parentElement;
    if (!parent || parent === doc.documentElement) return;
    downStack.push(current);
    current = parent;
    paint();
  }

  function descend() {
    if (!downStack.length) return;
    current = downStack.pop();
    paint();
  }

  function onKey(e) {
    if (isAltP(e)) {
      e.preventDefault();
      toggle();
      return;
    }
    if (!armed) return;
    if (e.key === 'Escape') {
      block(e);
      disarm();
    } else if (e.key === 'ArrowUp') {
      block(e);
      climb();
    } else if (e.key === 'ArrowDown') {
      block(e);
      descend();
    }
  }

  function onMove(e) {
    if (!armed) return;
    const node = targetOf(e);
    if (!node || node === hovered) return;
    hovered = node;
    current = node;
    downStack.length = 0;
    paint();
  }

  function onClick(e) {
    if (!armed) return;
    const target = targetOf(e);
    if (!target) return; // the pill: its own handler runs
    block(e);
    pick(current || target);
  }

  // While armed, presses do not reach the page either, so menus and buttons stay still.
  function onPress(e) {
    if (armed && targetOf(e)) block(e);
  }

  function onViewport() {
    if (armed) paint();
  }

  // The page's toast (2 s). Shown in the closed shadow root.
  function done(text) {
    if (!toast) return;
    toast.textContent = cut(collapse(text), 200);
    toast.classList.add('on');
    if (toastTimer) clearTimer(toastTimer);
    toastTimer = setTimer(() => {
      toast.classList.remove('on');
      toastTimer = null;
    }, TOAST_MS);
  }

  // Start ----------------------------------------------------------------------

  addListener('keydown', onKey, true);
  addListener('mousemove', onMove, true);
  addListener('click', onClick, true);
  addListener('mousedown', onPress, true);
  addListener('pointerdown', onPress, true);
  addListener('mouseup', onPress, true);
  addListener('scroll', onViewport, true);
  addListener('resize', onViewport, true);

  if (doc.readyState === 'complete') mount();
  else addListener('load', mount, { once: true });

  // Frozen, non-enumerable, read-only: Rust calls take/arm/disarm/done by this key.
  const api = Object.freeze({
    take() {
      return queue.length ? queue.shift() : null;
    },
    arm,
    disarm,
    done,
  });
  Object.defineProperty(win, KEY, { value: api, enumerable: false, configurable: false, writable: false });
})();
