// Tests for the Point and Fix click-to-pick script (PF-7).
// The script is src-tauri/src/point/picker.js. Each load() runs it on a fresh fake
// `window` (own top, location, ng, and listener list), so no test sees another
// test's picker. Teardown removes every listener and element the picker added.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const SOURCE = readFileSync(resolve(__dirname, '../src-tauri/src/point/picker.js'), 'utf8');
const ORIGIN = 'http://localhost:5173';

interface PickPayload {
  kind: string;
  ready: boolean;
  url: string;
  title: string;
  tag: string;
  selector: string;
  text: string;
  html: string;
  rect: { x: number; y: number; w: number; h: number };
  viewport: { w: number; h: number; dpr: number };
  attrs: [string, string][];
  styles: [string, string][];
  component: string | null;
  source: { file: string; line: number | null; column: number | null; how: string } | null;
}

interface PickerApi {
  take(): string | null;
  arm(): void;
  disarm(): void;
  done(text: string): void;
}

interface LoadOptions {
  /** Origin baked into the script (the one Rust builds it for). */
  baked?: string;
  /** Origin the page is actually on. */
  origin?: string;
  /** false: the script runs inside a frame (window.top is another window). */
  top?: boolean;
  /** Computed styles the fake window reports. */
  styles?: Record<string, string>;
  /** Angular dev global. */
  ng?: unknown;
}

interface Loaded {
  win: Record<string, unknown>;
  key: string;
  api: PickerApi | undefined;
}

const teardown: Array<() => void> = [];
const rects = new Map<Element, { left: number; top: number; width: number; height: number }>();
const zeroRect = { left: 0, top: 0, width: 0, height: 0 };
let seq = 0;

function load(options: LoadOptions = {}): Loaded {
  const realWin = window;
  seq += 1;
  const key = `__pm_test_${seq}_0123456789abcdef`;
  const origin = options.origin ?? ORIGIN;
  const before = new Set<Element>(Array.from(document.documentElement.children));
  const listeners: Array<[string, EventListenerOrEventListenerObject, boolean | AddEventListenerOptions | undefined]> = [];
  const styles = options.styles ?? {};
  const fake: Record<string, unknown> = {
    innerWidth: 1280,
    innerHeight: 800,
    devicePixelRatio: 1.5,
    location: { href: `${origin}/pricing?ok=1#frag`, origin, pathname: '/pricing', search: '?ok=1', hash: '#frag' },
    addEventListener: (type: string, fn: EventListenerOrEventListenerObject, opts?: boolean | AddEventListenerOptions) => {
      listeners.push([type, fn, opts]);
      realWin.addEventListener(type, fn, opts);
    },
    getComputedStyle: () => ({ getPropertyValue: (name: string) => styles[name] ?? '' }),
    setTimeout: (cb: () => void, ms: number) => realWin.setTimeout(cb, ms),
    clearTimeout: (id: number) => realWin.clearTimeout(id),
    requestAnimationFrame: realWin.requestAnimationFrame
      ? (cb: FrameRequestCallback) => realWin.requestAnimationFrame(cb)
      : undefined,
    ng: options.ng,
  };
  fake.top = options.top === false ? {} : fake;

  const baked = options.baked ?? ORIGIN;
  const source = SOURCE.replaceAll('__PM_ORIGIN__', JSON.stringify(baked)).replaceAll('__PM_KEY__', JSON.stringify(key));
  const run = new Function('window', 'location', source) as (w: unknown, l: unknown) => void;
  run(fake, fake.location);

  teardown.push(() => {
    for (const [type, fn, opts] of listeners) realWin.removeEventListener(type, fn, opts);
    for (const child of Array.from(document.documentElement.children)) {
      if (!before.has(child)) child.remove();
    }
    document.getElementById('pm-point-cursor')?.remove();
  });
  return { win: fake, key, api: fake[key] as PickerApi | undefined };
}

function apiOf(loaded: Loaded): PickerApi {
  if (!loaded.api) throw new Error('the picker did not start');
  return loaded.api;
}

function page(html: string): void {
  document.body.innerHTML = html;
}

function $<T extends Element = HTMLElement>(selector: string): T {
  const el = document.querySelector<T>(selector);
  if (!el) throw new Error(`no element for ${selector}`);
  return el;
}

function place(el: Element, left = 612, top = 380, width = 180, height = 44): void {
  rects.set(el, { left, top, width, height });
}

function hover(el: Element): void {
  el.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
}

function click(el: Element): void {
  el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
}

function press(key: string, init: KeyboardEventInit = {}, el: Element = document.body): void {
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));
}

// Two animation frames (or two 16 ms timers when rAF is missing) plus slack.
async function settle(): Promise<void> {
  await new Promise((r) => setTimeout(r, 80));
}

function pickOf(api: PickerApi): PickPayload | null {
  const raw = api.take();
  return raw === null ? null : (JSON.parse(raw) as PickPayload);
}

async function pickClick(api: PickerApi, el: Element): Promise<PickPayload> {
  api.arm();
  hover(el);
  click(el);
  await settle();
  const payload = pickOf(api);
  if (!payload) throw new Error('no pick was queued');
  return payload;
}

beforeEach(() => {
  rects.clear();
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
    const r = rects.get(this) ?? zeroRect;
    return { ...r, x: r.left, y: r.top, right: r.left + r.width, bottom: r.top + r.height, toJSON: () => r } as DOMRect;
  });
  document.title = 'Pricing - Acme';
});

afterEach(() => {
  while (teardown.length) teardown.pop()?.();
  vi.restoreAllMocks();
  document.body.innerHTML = '';
  document.title = '';
});

describe('picker guards (S5)', () => {
  it('defines nothing and adds nothing inside a frame', () => {
    page('<button>Go</button>');
    const before = document.documentElement.children.length;
    const loaded = load({ top: false });
    expect(loaded.api).toBeUndefined();
    expect(loaded.key in loaded.win).toBe(false);
    expect(document.documentElement.children.length).toBe(before);
  });

  it('defines nothing on another origin', () => {
    page('<button>Go</button>');
    const loaded = load({ baked: ORIGIN, origin: 'http://localhost:9999' });
    expect(loaded.api).toBeUndefined();
  });

  it('exposes a frozen API under a non-enumerable, read-only key', () => {
    const loaded = load();
    expect(loaded.api).toBeDefined();
    expect(Object.isFrozen(loaded.api)).toBe(true);
    const desc = Object.getOwnPropertyDescriptor(loaded.win, loaded.key);
    expect(desc?.enumerable).toBe(false);
    expect(desc?.writable).toBe(false);
    expect(desc?.configurable).toBe(false);
  });

  it('adds the pill as a closed shadow host the page cannot open', () => {
    const before = document.documentElement.children.length;
    load();
    expect(document.documentElement.children.length).toBe(before + 1);
    const host = document.documentElement.lastElementChild as HTMLElement;
    expect(host.shadowRoot).toBeNull();
  });
});

describe('selector', () => {
  it('uses a stable id when it is unique', async () => {
    page('<main><button id="buy-now">Buy</button></main>');
    const btn = $('#buy-now');
    place(btn);
    const p = await pickClick(apiOf(load()), btn);
    expect(p.selector).toBe('#buy-now');
  });

  it('never uses a generated id such as React useId output', async () => {
    page('<main><button id=":r1:">Buy</button></main>');
    const btn = $('button');
    place(btn);
    const p = await pickClick(apiOf(load()), btn);
    expect(p.selector).not.toContain(':r1:');
    expect(Array.from(document.querySelectorAll(p.selector))).toEqual([btn]);
  });

  it('drops hashed CSS-in-JS classes and keeps the stable ones', async () => {
    page(
      '<main><section class="pricing"><div class="card"><button class="css-1x2y3z sc-bdfBwQ card-cta primary" type="button">Get started</button></div></section></main>',
    );
    const btn = $('button');
    place(btn);
    const p = await pickClick(apiOf(load()), btn);
    expect(p.selector).toContain('button.card-cta.primary');
    expect(p.selector).not.toMatch(/css-|sc-/);
    expect(Array.from(document.querySelectorAll(p.selector))).toEqual([btn]);
  });

  it('is unique when identical cards sit side by side', async () => {
    page(
      '<section><div class="card"><button>Buy</button></div><div class="card"><button>Buy</button></div></section>',
    );
    const second = document.querySelectorAll('button')[1] as HTMLElement;
    place(second);
    const p = await pickClick(apiOf(load()), second);
    expect(Array.from(document.querySelectorAll(p.selector))).toEqual([second]);
  });

  it('uses data-testid when there is one', async () => {
    page('<main><button data-testid="buy" class="card-cta">Buy</button></main>');
    const btn = $('button');
    place(btn);
    const p = await pickClick(apiOf(load()), btn);
    expect(p.selector).toBe('[data-testid="buy"]');
  });

  it('uses at most five levels', async () => {
    const chain = (n: number) => '<div>'.repeat(n) + '<button>Buy</button>' + '</div>'.repeat(n);
    page(chain(8) + chain(8));
    const btn = document.querySelectorAll('button')[0] as HTMLElement;
    place(btn);
    const p = await pickClick(apiOf(load()), btn);
    expect(p.selector.split(' > ').length).toBeLessThanOrEqual(5);
  });
});

describe('text, HTML and attributes (S8)', () => {
  it('builds the full pick from the element', async () => {
    page(
      '<main><section class="pricing"><div class="card"><button class="css-1x2y3z card-cta primary" type="button" value="sk-live-123" data-testid="buy" style="' +
        'color:red;'.repeat(20) +
        '">Get started</button></div></section></main>',
    );
    const btn = $('button');
    btn.innerText = 'Get started';
    place(btn, 612, 380, 180, 44);
    const loaded = load({
      styles: {
        'font-size': '14px',
        color: 'rgb(255, 255, 255)',
        'background-color': 'rgb(17, 17, 17)',
        padding: '8px 16px',
        display: 'inline-block',
      },
    });
    const p = await pickClick(apiOf(loaded), btn);
    expect(p.kind).toBe('pick');
    expect(p.ready).toBe(true);
    expect(p.url).toBe(`${ORIGIN}/pricing?ok=1`);
    expect(p.title).toBe('Pricing - Acme');
    expect(p.tag).toBe('button');
    expect(p.selector).toBe('[data-testid="buy"]');
    expect(p.text).toBe('Get started');
    expect(p.html.startsWith('<button class="css-1x2y3z card-cta primary" type="button" data-testid="buy" style="')).toBe(true);
    expect(p.html).not.toContain('sk-live-123');
    expect(p.rect).toEqual({ x: 612, y: 380, w: 180, h: 44 });
    expect(p.viewport).toEqual({ w: 1280, h: 800, dpr: 1.5 });
    expect(p.attrs).toEqual([['data-testid', 'buy']]);
    expect(p.styles).toEqual([
      ['font-size', '14px'],
      ['color', 'rgb(255, 255, 255)'],
      ['background-color', 'rgb(17, 17, 17)'],
      ['padding', '8px 16px'],
      ['display', 'inline-block'],
    ]);
    expect(p.component).toBeNull();
    expect(p.source).toBeNull();
    expect(JSON.stringify(p)).not.toContain('sk-live-123');
  });

  it('never reads an input value, only its placeholder', async () => {
    page('<form><input type="email" name="email" placeholder="Your email" value="jane@example.com"></form>');
    const input = $<HTMLInputElement>('input');
    place(input);
    let valueRead = false;
    Object.defineProperty(input, 'value', {
      configurable: true,
      get() {
        valueRead = true;
        return 'jane@example.com';
      },
    });
    const p = await pickClick(apiOf(load()), input);
    expect(valueRead).toBe(false);
    expect(p.text).toBe('Your email');
    expect(JSON.stringify(p)).not.toContain('jane@example.com');
  });

  it('gives no text at all for a password field', async () => {
    page('<input type="password" id="pw" placeholder="Password" aria-label="Your password" value="hunter2">');
    const input = $('#pw');
    place(input);
    const p = await pickClick(apiOf(load()), input);
    expect(p.text).toBe('');
    expect(JSON.stringify(p)).not.toContain('hunter2');
  });

  it('uses only the hints of a contenteditable element, never its text', async () => {
    page('<div contenteditable="true" aria-label="Notes">private draft text</div>');
    const box = $('div');
    place(box);
    const p = await pickClick(apiOf(load()), box);
    expect(p.text).toBe('Notes');
    expect(JSON.stringify(p)).not.toContain('private draft');
  });

  it('drops value attributes, cuts style and data URLs in the HTML', async () => {
    page(
      `<img alt="logo" src="data:image/png;base64,${'Q'.repeat(400)}" style="${'a'.repeat(200)}" value="secret-value">`,
    );
    const img = $('img');
    place(img);
    const p = await pickClick(apiOf(load()), img);
    expect(p.html).not.toContain('secret-value');
    expect(p.html).not.toContain('Q'.repeat(20));
    expect(p.html).not.toContain('a'.repeat(100));
    expect(p.html.length).toBeLessThanOrEqual(300);
  });

  it('keeps link targets as a path only, with no query or fragment', async () => {
    page('<a href="https://example.com/docs/page?token=abc123#top">Docs</a>');
    const link = $('a');
    place(link);
    const p = await pickClick(apiOf(load()), link);
    expect(p.attrs).toEqual([['href', '/docs/page']]);
    expect(p.html).toContain('href="/docs/page"');
    expect(JSON.stringify(p)).not.toContain('token=abc123');
  });

  it('drops the URL fragment and caps text and title', async () => {
    page(`<button>${'x'.repeat(500)}</button>`);
    const btn = $('button');
    btn.innerText = 'x'.repeat(500);
    document.title = 'T'.repeat(300);
    place(btn);
    const p = await pickClick(apiOf(load()), btn);
    expect(p.url).not.toContain('#frag');
    expect(p.text.length).toBe(160);
    expect(p.title.length).toBe(120);
  });
});

describe('component and source', () => {
  it('names the React component and its source line (React 18 debug source)', async () => {
    page('<div class="card"><button>Buy</button></div>');
    const btn = $('button');
    place(btn);
    function PricingCard() {}
    (btn as unknown as Record<string, unknown>)['__reactFiber$abc123'] = {
      type: 'button',
      _debugSource: { fileName: 'C:/Users/you/app/src/components/PricingCard.tsx', lineNumber: 42, columnNumber: 7 },
      return: { type: PricingCard, return: null },
    };
    const p = await pickClick(apiOf(load()), btn);
    expect(p.component).toBe('PricingCard');
    expect(p.source).toEqual({
      file: 'C:/Users/you/app/src/components/PricingCard.tsx',
      line: 42,
      column: 7,
      how: 'react-debug-source',
    });
  });

  it('gives a file only, from the debug stack, when React 19 has no debug source', async () => {
    page('<div class="card"><button>Buy</button></div>');
    const btn = $('button');
    place(btn);
    function PricingCard() {}
    const stack = {
      stack:
        'Error: x\n' +
        '    at PricingCard (http://localhost:5173/src/components/PricingCard.tsx:42:5)\n' +
        '    at renderWithHooks (http://localhost:5173/node_modules/react-dom/client.js:10:3)',
    };
    (btn as unknown as Record<string, unknown>)['__reactFiber$r19xyz'] = {
      type: 'button',
      _debugStack: stack,
      return: { type: PricingCard, return: null },
    };
    const p = await pickClick(apiOf(load()), btn);
    expect(p.component).toBe('PricingCard');
    expect(p.source).toEqual({ file: '/src/components/PricingCard.tsx', line: null, column: null, how: 'stack' });
  });

  it('reads Svelte dev metadata', async () => {
    page('<button>Buy</button>');
    const btn = $('button');
    place(btn);
    (btn as unknown as Record<string, unknown>).__svelte_meta = { loc: { file: 'src/lib/Card.svelte', line: 9, column: 3 } };
    const p = await pickClick(apiOf(load()), btn);
    expect(p.source).toEqual({ file: 'src/lib/Card.svelte', line: 9, column: 3, how: 'svelte-meta' });
    expect(p.component).toBeNull();
  });

  it('reads the Vue component name and file', async () => {
    page('<button>Buy</button>');
    const btn = $('button');
    place(btn);
    (btn as unknown as Record<string, unknown>).__vueParentComponent = {
      type: { __name: 'PricingCard', __file: 'C:/Users/you/app/src/components/PricingCard.vue' },
    };
    const p = await pickClick(apiOf(load()), btn);
    expect(p.component).toBe('PricingCard');
    expect(p.source).toEqual({
      file: 'C:/Users/you/app/src/components/PricingCard.vue',
      line: null,
      column: null,
      how: 'vue-file',
    });
  });

  it('reads react-dev-inspector attributes from the nearest ancestor', async () => {
    page(
      '<section data-inspector-relative-path="src/components/Pricing.tsx" data-inspector-line="12" data-inspector-column="5"><button>Buy</button></section>',
    );
    const btn = $('button');
    place(btn);
    const p = await pickClick(apiOf(load()), btn);
    expect(p.source).toEqual({ file: 'src/components/Pricing.tsx', line: 12, column: 5, how: 'data-attr' });
  });

  it('reads the vite-plugin-vue-inspector file:line:col attribute', async () => {
    page('<div data-v-inspector="src/Card.vue:4:3"><button>Buy</button></div>');
    const btn = $('button');
    place(btn);
    const p = await pickClick(apiOf(load()), btn);
    expect(p.source).toEqual({ file: 'src/Card.vue', line: 4, column: 3, how: 'data-attr' });
  });

  it('reads the explicit data-component name', async () => {
    page('<div data-component="PriceCard"><button>Buy</button></div>');
    const btn = $('button');
    place(btn);
    const p = await pickClick(apiOf(load()), btn);
    expect(p.component).toBe('PriceCard');
  });

  it('reads the Angular dev component name', async () => {
    page('<button>Buy</button>');
    const btn = $('button');
    place(btn);
    const ng = { getOwningComponent: () => ({ constructor: { name: 'PricingCardComponent' } }) };
    const p = await pickClick(apiOf(load({ ng })), btn);
    expect(p.component).toBe('PricingCardComponent');
  });

  it('still picks when a framework read throws', async () => {
    page('<button>Buy</button>');
    const btn = $('button');
    place(btn);
    Object.defineProperty(btn, '__svelte_meta', {
      configurable: true,
      get() {
        throw new Error('boom');
      },
    });
    const p = await pickClick(apiOf(load()), btn);
    expect(p.tag).toBe('button');
    expect(p.source).toBeNull();
  });
});

describe('pointing flow', () => {
  it('ArrowUp moves to the parent and ArrowDown moves back', async () => {
    page('<main><section class="pricing"><button class="card-cta">Buy</button></section></main>');
    const btn = $('button');
    place(btn);
    const loaded = load();
    const api = apiOf(loaded);

    api.arm();
    hover(btn);
    press('ArrowUp');
    click(btn);
    await settle();
    expect(pickOf(api)?.tag).toBe('section');

    api.arm();
    hover(btn);
    press('ArrowUp');
    press('ArrowDown');
    click(btn);
    await settle();
    expect(pickOf(api)?.tag).toBe('button');
  });

  it('Esc turns the picker off, so clicks reach the page again', async () => {
    page('<button id="go">Go</button>');
    const btn = $('button');
    place(btn);
    let pageClicks = 0;
    btn.addEventListener('click', () => {
      pageClicks += 1;
    });
    const api = apiOf(load());
    api.arm();
    hover(btn);
    press('Escape');
    click(btn);
    await settle();
    expect(pageClicks).toBe(1);
    expect(pickOf(api)).toBeNull();
  });

  it('stops a picking click from reaching the page', async () => {
    page('<button id="go">Go</button>');
    const btn = $('button');
    place(btn);
    let pageClicks = 0;
    btn.addEventListener('click', () => {
      pageClicks += 1;
    });
    const api = apiOf(load());
    api.arm();
    hover(btn);
    click(btn);
    expect(pageClicks).toBe(0);
    await settle();
    expect(pickOf(api)?.tag).toBe('button');
  });

  it('queues one pick after two frames, and take() returns it once', async () => {
    page('<button id="buy-now">Buy</button>');
    const btn = $('button');
    place(btn);
    const api = apiOf(load());
    api.arm();
    hover(btn);
    click(btn);
    expect(api.take()).toBeNull();
    await settle();
    expect(api.take()).not.toBeNull();
    expect(api.take()).toBeNull();
  });

  it('a pick turns the picker off', async () => {
    page('<button id="buy-now">Buy</button>');
    const btn = $('button');
    place(btn);
    const api = apiOf(load());
    api.arm();
    hover(btn);
    click(btn);
    expect(document.getElementById('pm-point-cursor')).toBeNull();
    await settle();
  });

  it('Alt+P toggles the picker, and a bare P does nothing', () => {
    const api = apiOf(load());
    press('p');
    expect(document.getElementById('pm-point-cursor')).toBeNull();
    press('p', { altKey: true, code: 'KeyP' });
    expect(document.getElementById('pm-point-cursor')).not.toBeNull();
    press('p', { altKey: true, code: 'KeyP' });
    expect(document.getElementById('pm-point-cursor')).toBeNull();
    expect(api).toBeDefined();
  });

  it('arm() twice adds one cursor style only', () => {
    const api = apiOf(load());
    api.arm();
    api.arm();
    expect(document.querySelectorAll('#pm-point-cursor')).toHaveLength(1);
    api.disarm();
    expect(document.querySelectorAll('#pm-point-cursor')).toHaveLength(0);
  });

  it('done() shows its text without throwing', () => {
    const api = apiOf(load());
    expect(() => api.done('Sent to Codex · fix-pricing')).not.toThrow();
  });
});
