// @vitest-environment jsdom
// Settings > Potato Bot: the two rows that open the sub-pages. Each row says what is set; Open shows the page over the
// window; Back, Escape and a click on the dimmed part close it; the page's own reads and saves go through the transports.
import { act } from 'react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fakePotatoTransport, type FakePotatoTransport } from '../../../lib/potato';
import { fakeUsageTransport, type UsageReport } from '../../../lib/potato-usage';
import { byLabel, click, hasText, mountRows, settle, unsearchableLabels, type Mounted } from '../test-utils';
import { makePotatoSubPageRows } from './PotatoSubPages';
import { makePotatoRows } from '../sections/PotatoSection';

let m: Mounted;
let fake: FakePotatoTransport;
const usage: UsageReport = {
  today: '2026-11-01',
  limitCents: 50,
  sandboxActionsPerDay: 500,
  total: { messages: 1, tokens: 1_000, costNano: 10_000_000, sandboxActions: 0 },
  todayTotal: { messages: 1, tokens: 1_000, costNano: 10_000_000, sandboxActions: 0 },
  days: [{ date: '2026-11-01', messages: 1, tokens: 1_000, costNano: 10_000_000, sandboxActions: 0 }],
  bots: [],
};

beforeEach(() => {
  fake = fakePotatoTransport({ state: { settings: { v: 1, brain: 'go', defaultPersona: 'fish', dailyLimitCents: 50, fallbacks: ['deepseek'] } } });
});
afterEach(async () => {
  await m?.unmount();
});

const potatoRows = () => makePotatoSubPageRows(fake, fakeUsageTransport(usage));

async function mount() {
  m = await mountRows(potatoRows(), { settings: { potato_bot_enabled: true } as never });
  await settle(6);
  return m;
}

describe('the sub-page rows on Settings > Potato Bot', () => {
  it('say what is set: the talk brain and its fallback', async () => {
    await mount();
    expect(hasText(m.host, 'Talk brain: OpenCode Go. If it fails, try DeepSeek.')).toBe(true);
  });

  it('say "No fallback." when none is set', async () => {
    fake = fakePotatoTransport({ state: { settings: { v: 1, brain: 'deepseek', defaultPersona: 'fish', dailyLimitCents: 50 } } });
    await mount();
    expect(hasText(m.host, 'Talk brain: DeepSeek. No fallback.')).toBe(true);
  });

  it('Usage says what it counts in one line', async () => {
    await mount();
    expect(hasText(m.host, 'Messages, tokens and cost for the last 30 days, and today against the limits.')).toBe(true);
  });

  it('every word of every label finds its row in search', async () => {
    await mount();
    expect(unsearchableLabels(m.host, potatoRows())).toEqual([]);
  });

  it('is part of the Potato Bot page', () => {
    const ids = makePotatoRows(fake, undefined, fakeUsageTransport(usage)).map((r) => r.id);
    expect(ids).toEqual(['potato-bot', 'potato-models', 'potato-key', 'potato-persona', 'potato-limit', 'potato-usage']);
  });
});

describe('opening a sub-page', () => {
  it('Models and fallbacks opens over the window and Back closes it', async () => {
    await mount();
    expect(m.host.querySelector('[role="dialog"]')).toBeNull();
    await click(byLabel(m.host, 'Open Models and fallbacks'));
    await settle(4);
    const dialog = m.host.querySelector('[role="dialog"]');
    expect(dialog).not.toBeNull();
    expect(dialog?.querySelector('.pot-sub-head h2')?.textContent).toBe('Models and fallbacks');
    expect(byLabel(m.host, 'Talk brain')).not.toBeNull();
    await click(m.host.querySelector('.pot-sub-back'));
    await settle(2);
    expect(m.host.querySelector('[role="dialog"]')).toBeNull();
  });

  it('Escape closes it, and so does a click on the dimmed part', async () => {
    await mount();
    await click(byLabel(m.host, 'Open Usage'));
    await settle(4);
    const scrim = m.host.querySelector('.pot-sub-scrim') as HTMLElement;
    expect(scrim).not.toBeNull();
    await act(async () => scrim.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
    await settle(2);
    expect(m.host.querySelector('[role="dialog"]')).toBeNull();

    await click(byLabel(m.host, 'Open Usage'));
    await settle(4);
    const again = m.host.querySelector('.pot-sub-scrim') as HTMLElement;
    await act(async () => again.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })));
    await settle(2);
    expect(m.host.querySelector('[role="dialog"]')).toBeNull();
  });

  it('Usage opens the numbers, read from the usage command', async () => {
    await mount();
    await click(byLabel(m.host, 'Open Usage'));
    await settle(6);
    expect(m.host.querySelector('.pot-sub-head h2')?.textContent).toBe('Usage');
    expect(hasText(m.host, 'Last 30 days')).toBe(true);
    expect(hasText(m.host, 'Costs are estimates')).toBe(true);
  });

  it('the Back button is the first thing that has focus', async () => {
    await mount();
    await click(byLabel(m.host, 'Open Usage'));
    await settle(4);
    expect(document.activeElement?.className).toContain('pot-sub-back');
  });
});
