// @vitest-environment jsdom
// Settings > Potato Bot > Usage: today against each bot's limit and sandbox cap, the 30 days in total, per bot and per
// day (newest first), the estimate note, and a plain sentence when the read fails. Read only: nothing is ever saved.
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';
import { fakeUsageTransport, type UsageDay, type UsageReport, type UsageTotals } from '../../../lib/potato-usage';
import { hasText, settle } from '../test-utils';
import { UsagePage } from './PotatoUsagePage';

let host: HTMLElement | null = null;
let unmount: (() => Promise<void>) | null = null;

afterEach(async () => {
  await unmount?.();
  unmount = null;
  host = null;
});

async function mountUsage(transport: Parameters<typeof UsagePage>[0]['transport']): Promise<HTMLElement> {
  host = document.createElement('div');
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(<UsagePage transport={transport} />));
  await settle(6);
  unmount = async () => {
    await act(async () => root.unmount());
    host?.remove();
  };
  return host;
}

const totals = (over: Partial<UsageTotals> = {}): UsageTotals => ({ messages: 0, tokens: 0, costNano: 0, sandboxActions: 0, ...over });

/** 30 days, oldest first. Only today and yesterday have use. */
function days(today: string): UsageDay[] {
  const list: UsageDay[] = [];
  for (let i = 29; i >= 0; i--) {
    const d = new Date(`${today}T00:00:00`);
    d.setDate(d.getDate() - i);
    const date = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    list.push({ date, ...(i === 0 ? totals({ messages: 3, tokens: 4_200, costNano: 700_000_000 }) : i === 1 ? totals({ messages: 2, tokens: 800, costNano: 300_000_000 }) : totals()) });
  }
  return list;
}

const TODAY = '2026-11-01';

function report(over: Partial<UsageReport> = {}): UsageReport {
  return {
    today: TODAY,
    limitCents: 50,
    sandboxActionsPerDay: 500,
    total: totals({ messages: 5, tokens: 5_000, costNano: 1_000_000_000, sandboxActions: 7 }),
    todayTotal: totals({ messages: 3, tokens: 4_200, costNano: 700_000_000, sandboxActions: 7 }),
    days: days(TODAY),
    bots: [
      {
        id: 'nimbus',
        name: 'Nimbus',
        total: totals({ messages: 3, tokens: 3_000, costNano: 600_000_000, sandboxActions: 7 }),
        today: totals({ messages: 2, tokens: 2_000, costNano: 250_000_000, sandboxActions: 7 }),
        days: days(TODAY),
      },
      {
        id: 'mochi',
        name: 'Mochi',
        total: totals({ messages: 2, tokens: 2_000, costNano: 400_000_000 }),
        today: totals({ messages: 1, tokens: 2_200, costNano: 450_000_000 }),
        days: days(TODAY),
      },
    ],
    ...over,
  };
}

describe('Settings > Potato Bot > Usage', () => {
  it('shows today per bot against the daily limit and the sandbox cap, in plain words', async () => {
    const root = await mountUsage(fakeUsageTransport(report()));
    expect(hasText(root, 'Nimbus')).toBe(true);
    expect(hasText(root, '$0.25 of $0.50')).toBe(true);
    // Mochi spent 90% of its limit: the warning shows
    expect(hasText(root, '$0.45 of $0.50')).toBe(true);
    expect(hasText(root, 'Close to the limit.')).toBe(true);
    expect(hasText(root, '7 of 500')).toBe(true);
    const bar = root.querySelector('[aria-label="Nimbus"]');
    expect(bar?.getAttribute('aria-valuenow')).toBe('50');
  });

  it('says the limit is reached once a bot is at 100%', async () => {
    const over = report({ bots: [{ id: 'mochi', name: 'Mochi', total: totals(), today: totals({ costNano: 600_000_000 }), days: [] }] });
    const root = await mountUsage(fakeUsageTransport(over));
    expect(hasText(root, 'Limit reached. It stops until tomorrow.')).toBe(true);
  });

  it('shows the 30 days in total, per bot, and each day newest first with a bar', async () => {
    const root = await mountUsage(fakeUsageTransport(report()));
    expect(root.querySelector('.pot-stats')?.textContent).toContain('Messages5');
    expect(hasText(root, '5.0k')).toBe(true);
    expect(hasText(root, '$1.00')).toBe(true);
    expect(hasText(root, '3 messages · 3.0k tokens · $0.60')).toBe(true);
    const rows = root.querySelectorAll<HTMLElement>('ol[aria-label="Each day, newest first"] > .pot-day');
    expect(rows.length).toBe(30);
    expect(root.querySelectorAll('ol[aria-label="Nimbus each day, newest first"] > .pot-day').length).toBe(30);
    expect(rows[0].querySelector('.pot-day-date')?.textContent).toBe('Today');
    expect(rows[1].querySelector('.pot-day-date')?.textContent).toBe('Yesterday');
    expect(rows[0].querySelector('.pot-bar-fill')?.getAttribute('style')).toContain('100%');
    expect(rows[2].querySelector('.pot-bar-fill')?.getAttribute('style')).toContain('0%');
  });

  it('says what the costs are: estimates from our prices', async () => {
    const root = await mountUsage(fakeUsageTransport(report()));
    expect(hasText(root, 'Costs are estimates from the prices Puppet Master knows.')).toBe(true);
  });

  it('with no bot and no use, says so and shows no bars for bots', async () => {
    const quiet = days(TODAY).map((d) => ({ ...d, messages: 0, tokens: 0, costNano: 0, sandboxActions: 0 }));
    const empty = report({ bots: [], total: totals(), todayTotal: totals(), days: quiet });
    const root = await mountUsage(fakeUsageTransport(empty));
    expect(hasText(root, 'No bots yet.')).toBe(true);
    expect(hasText(root, 'Nothing used in the last 30 days.')).toBe(true);
  });

  it('a read that fails says why, in plain words, and does not show numbers', async () => {
    const root = await mountUsage(fakeUsageTransport('the usage file is damaged.'));
    expect(root.querySelector('[role="alert"].st-note')?.textContent).toBe('the usage file is damaged.');
    expect(root.querySelector('.pot-stats')).toBeNull();
  });

  it('asks once, and never writes', async () => {
    const fake = fakeUsageTransport(report());
    await mountUsage(fake);
    expect(fake.asked).toBe(1);
  });
});
