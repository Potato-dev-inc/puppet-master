// Assertions. The ones about the page wait (they poll until the timeout), so a test never needs a sleep.
import { describeTarget } from './page.mjs';
import { sleep } from './cdp.mjs';

export class AssertionFailed extends Error {
  constructor(message) { super(message); this.name = 'AssertionFailed'; }
}

const fmt = (v) => { try { return typeof v === 'string' ? JSON.stringify(v) : JSON.stringify(v, null, 0); } catch { return String(v); } };

export function makeExpect(page, { isAborted = () => false, defaultTimeout = 10000 } = {}) {
  /** Poll `fn` until it returns a truthy value; throws AssertionFailed with `describe()` when time runs out. */
  async function eventually(fn, { timeout = defaultTimeout, message = 'condition', interval = 150 } = {}) {
    const end = Date.now() + timeout;
    let last;
    for (;;) {
      if (isAborted()) throw new Error('the test was stopped');
      try { const v = await fn(); if (v) return v; last = undefined; } catch (e) { last = e; }
      if (Date.now() > end) break;
      await sleep(interval);
    }
    throw new AssertionFailed(`${typeof message === 'function' ? await message() : message} (waited ${timeout} ms)${last ? ': ' + last.message : ''}`);
  }

  const textOf = (v) => (v instanceof RegExp ? String(v) : fmt(v));
  const textMatches = (page_text, v) => (v instanceof RegExp ? v.test(page_text) : page_text.toLowerCase().includes(String(v).toLowerCase()));

  return {
    eventually,
    /** The page shows this text (substring, case-insensitive) or matches this regular expression. */
    async text(v, opts = {}) {
      await eventually(async () => textMatches(await page.text(), v), { ...opts, message: async () => `expected the page to show ${textOf(v)}. It shows: ${await page.snippet()}` });
    },
    async noText(v, opts = {}) {
      await eventually(async () => !textMatches(await page.text(), v), { timeout: opts.timeout ?? 3000, ...opts, message: `expected the page NOT to show ${textOf(v)}` });
    },
    /** A target (see page.mjs) is on screen. */
    async visible(target, opts = {}) {
      await eventually(() => page.has(target, opts), { ...opts, message: async () => `expected ${describeTarget(target)} to be visible. The page shows: ${await page.snippet()}` });
    },
    async notVisible(target, opts = {}) {
      await eventually(async () => !(await page.has(target, opts)), { timeout: opts.timeout ?? 3000, ...opts, message: `expected ${describeTarget(target)} NOT to be visible` });
    },
    async count(target, n, opts = {}) {
      let seen = -1;
      await eventually(async () => (seen = await page.count(target, opts)) === n, { ...opts, message: () => `expected ${n} of ${describeTarget(target)}, found ${seen}` });
    },
    /** A field holds this value. */
    async value(target, v, opts = {}) {
      let seen;
      await eventually(async () => (seen = await page.value(target, opts)) === v, { ...opts, message: () => `expected ${describeTarget(target)} to hold ${fmt(v)}, it holds ${fmt(seen)}` });
    },
    ok(cond, message = 'expected a true value') { if (!cond) throw new AssertionFailed(message); },
    equal(actual, expected, message = 'values differ') {
      if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new AssertionFailed(`${message}: expected ${fmt(expected)}, got ${fmt(actual)}`);
    },
    includes(haystack, needle, message = 'missing text') {
      const ok = Array.isArray(haystack) ? haystack.includes(needle) : String(haystack).includes(needle);
      if (!ok) throw new AssertionFailed(`${message}: ${fmt(needle)} not found in ${fmt(haystack).slice(0, 400)}`);
    },
    match(actual, re, message = 'text does not match') {
      if (!re.test(String(actual))) throw new AssertionFailed(`${message}: ${String(re)} does not match ${fmt(String(actual)).slice(0, 400)}`);
    },
  };
}
