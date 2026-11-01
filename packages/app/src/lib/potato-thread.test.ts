// @vitest-environment node
// The pure rules of the thread: separator words, the unread line, tool lines turned into steps (never raw JSON in the
// words), the activity title, and the quote text.
import { describe, expect, it, vi } from 'vitest';
import type { PotatoMessage } from './potato';
import { POTATO_QUOTE_EVENT, activityTitle, clockLabel, dayLabel, emitQuote, firstUnreadId, isRawJson, quoteExcerpt, sameDay, toolStep } from './potato-thread';

const at = (y: number, mo: number, d: number, h = 12, mi = 0) => new Date(y, mo - 1, d, h, mi).getTime();
const m = (id: string, kind: 'you' | 'bot' | 'note' = 'bot', text = id): PotatoMessage => ({ id, at: 0, from: { kind }, kind: 'text', text });

describe('separator words', () => {
  it('writes a clock time the same way on every machine', () => {
    expect(clockLabel(at(2026, 11, 1, 0, 11))).toBe('12:11 AM');
    expect(clockLabel(at(2026, 11, 1, 12, 0))).toBe('12:00 PM');
    expect(clockLabel(at(2026, 11, 1, 15, 5))).toBe('3:05 PM');
    expect(clockLabel(at(2026, 11, 1, 9, 30))).toBe('9:30 AM');
  });

  it('names the day: Today, Yesterday, a weekday this week, else the date', () => {
    const now = at(2026, 11, 1, 15);
    expect(dayLabel(at(2026, 11, 1, 1), now)).toBe('Today');
    expect(dayLabel(at(2026, 10, 31, 23), now)).toBe('Yesterday');
    expect(dayLabel(at(2026, 10, 29), now)).toBe('Thursday');
    expect(dayLabel(at(2026, 10, 20), now)).toBe('Oct 20');
    expect(dayLabel(at(2025, 12, 31), now)).toBe('Dec 31, 2025');
  });

  it('knows whether two moments share a day', () => {
    expect(sameDay(at(2026, 11, 1, 0, 1), at(2026, 11, 1, 23, 59))).toBe(true);
    expect(sameDay(at(2026, 11, 1, 23, 59), at(2026, 11, 2, 0, 1))).toBe(false);
  });
});

describe('firstUnreadId', () => {
  const list = [m('a', 'bot'), m('b', 'you'), m('c', 'bot'), m('d', 'bot'), m('n', 'note')];
  it('counts the bot messages from the end, and skips yours and notes', () => {
    expect(firstUnreadId(list, 1)).toBe('d');
    expect(firstUnreadId(list, 2)).toBe('c');
  });
  it('is null with nothing unread, and when the very first message is the first unread', () => {
    expect(firstUnreadId(list, 0)).toBeNull();
    expect(firstUnreadId(list, 3)).toBeNull();
    expect(firstUnreadId([], 2)).toBeNull();
  });
});

describe('toolStep', () => {
  const step = (text: string) => toolStep({ id: 'x', text });

  it('turns an ok line into words: what it did, and what it saw', () => {
    expect(step('Fish used computer_observe: ok, saw Fake desktop')).toMatchObject({
      tool: 'computer_observe',
      label: 'Looked at the screen',
      summary: 'Fake desktop',
      ok: true,
      details: null,
      group: 'computer',
    });
  });

  it('keeps raw JSON out of the words: it goes to details only', () => {
    const s = step('Fish used ask_person_for_screen: ok, saw {"granted":true,"reason":"a code","lease":{"id":"x1"}}')!;
    expect(s.label).toBe('Asked you for the screen');
    expect(s.summary).toBe('');
    expect(s.details).toContain('"granted": true');
    expect(`${s.label} ${s.summary}`).not.toMatch(/[{}"]/);
  });

  it('keeps a cut-short JSON result out of the words too', () => {
    const s = step('Fish used browser_snapshot: ok, saw {"title":"Inbox","nodes":[{"role":"link","na')!;
    expect(s.summary).toBe('');
    expect(s.details).toContain('"title":"Inbox"');
  });

  it('says a failure plainly, and when it needs you', () => {
    expect(step('Fish used computer_act: failed LEASE_LOST')).toMatchObject({ ok: false, needsYou: false, summary: 'The screen changed hands.' });
    expect(step('Fish used computer_act: failed SOMETHING_ODD (needs you)')).toMatchObject({ ok: false, needsYou: true, summary: 'Something odd.' });
  });

  it('turns a whole-message JSON object into a "Got a result" step', () => {
    const s = step('{"ok":true,"result":{"url":"https://example.com"}}')!;
    expect(s.label).toBe('Got a result');
    expect(s.details).toContain('"ok": true');
  });

  it('leaves ordinary text alone', () => {
    expect(step('Fish heard: hello')).toBeNull();
    expect(step('I used the browser: it was fine')).toBeNull();
    expect(step('Here is a list: {not json}')).toBeNull();
    expect(isRawJson('[1,2]')).toBe(true);
    expect(isRawJson('{broken')).toBe(false);
  });
});

describe('activityTitle', () => {
  const s = (tool: string, ok = true) => toolStep({ id: tool + Math.random(), text: `Fish used ${tool}: ${ok ? 'ok' : 'failed LEASE_LOST'}` })!;
  it('says "Used the computer" and counts the steps', () => {
    expect(activityTitle([s('computer_observe'), s('computer_act'), s('browser_snapshot')])).toBe('Used the computer · 3 steps');
    expect(activityTitle([s('computer_observe')])).toBe('Used the computer · 1 step');
  });
  it('says "Started a helper" for one helper', () => {
    expect(activityTitle([s('ask_helper')])).toBe('Started a helper');
  });
  it('mixes into "Used tools", and counts failures', () => {
    expect(activityTitle([s('computer_observe'), s('memory_search')])).toBe('Used tools · 2 steps');
    expect(activityTitle([s('computer_observe'), s('computer_act', false)])).toBe('Used the computer · 2 steps · 1 failed');
  });
});

describe('quote', () => {
  it('cuts a very long message short', () => {
    const q = quoteExcerpt('x'.repeat(2000));
    expect(q.length).toBeLessThan(700);
    expect(q).toContain('…');
    expect(quoteExcerpt('  short  ')).toBe('short');
  });
  it('sends the text on one window event with the bot id', () => {
    const seen = vi.fn();
    emitQuote('fish', 'a line', { dispatchEvent: (e) => (seen(e.type, (e as CustomEvent).detail), true) });
    expect(seen).toHaveBeenCalledWith('pm:potato-quote', { botId: 'fish', text: 'a line' });
    expect(POTATO_QUOTE_EVENT).toBe('pm:potato-quote');
  });
});
