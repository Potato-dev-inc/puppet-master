// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PlanMonth, PlanRow, PlansApi, PlansReport } from '../../lib/plans-report';
import { plansCsv, plansSummaryText } from '../../lib/plans-copy';
import { PlansMonth } from './PlansMonth';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

const OCT: PlanMonth = { id: '2026-10', label: 'October 2026', startMs: 0, endMs: 1 };
const SEP: PlanMonth = { id: '2026-09', label: 'September 2026', startMs: 0, endMs: 1 };

function row(over: Partial<PlanRow>): PlanRow {
  return {
    agentId: 'alpha',
    agentLabel: 'Alpha Code',
    accountId: null,
    accountLabel: 'Default account',
    turns: 0,
    busyMs: 0,
    files: { knownTurns: 0, changed: 0, added: 0, deleted: 0 },
    merged: 0,
    limits: { hits: 0, limitedMs: 0 },
    ...over,
  };
}

const ROWS: PlanRow[] = [
  row({
    accountId: 'acct-work',
    accountLabel: 'Work',
    turns: 42,
    busyMs: (3 * 60 + 12) * 60_000,
    files: { knownTurns: 40, changed: 12, added: 340, deleted: 45 },
    merged: 2,
    limits: { hits: 3, limitedMs: 7 * 3_600_000 },
  }),
  row({ agentId: 'nimbus', agentLabel: 'Nimbus', turns: 5, busyMs: 90_000 }),
];

const FULL: PlansReport = {
  month: OCT,
  months: [OCT, SEP],
  rows: ROWS,
  totals: { turns: 47, busyMs: 11_610_000, filesChanged: 12, added: 340, deleted: 45, merged: 2, limitHits: 3, limitedMs: 7 * 3_600_000 },
  notes: ['Counting limits since 12 October 2026. Earlier limits were not recorded.', 'Tokens are not counted. Only usage an agent reports itself would show here.'],
  limitsCountedSince: '2026-10-12',
};

const EMPTY: PlansReport = {
  month: OCT,
  months: [OCT],
  rows: [],
  totals: { turns: 0, busyMs: 0, filesChanged: 0, added: 0, deleted: 0, merged: 0, limitHits: 0, limitedMs: 0 },
  notes: ['Counting limits from now on. Earlier limits were not recorded.'],
};

function fakeApi(report: PlansReport | ((month?: string) => PlansReport)): PlansApi & { report: ReturnType<typeof vi.fn> } {
  return { report: vi.fn(async (month?: string) => (typeof report === 'function' ? report(month) : report)) };
}

async function mount(api: PlansApi) {
  await act(async () => {
    root.render(<PlansMonth api={api} onClose={() => undefined} />);
  });
  await act(async () => {});
}

const text = () => host.textContent ?? '';
const button = (label: string) => host.querySelector(`button[aria-label="${label}"]`) as HTMLButtonElement;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('PlansMonth', () => {
  it('shows one row per subscription with the plain wording', async () => {
    await mount(fakeApi(FULL));
    expect(host.querySelector('h1')?.textContent).toBe('Your plans this month');
    const rows = [...host.querySelectorAll('tbody tr')];
    expect(rows).toHaveLength(2);
    const first = rows[0].textContent ?? '';
    expect(first).toContain('Alpha Code');
    expect(first).toContain('Work');
    expect(first).toContain('42');
    expect(first).toContain('3 h 12 min');
    expect(first).toContain('12 files +340 −45');
    expect(first).toContain('Hit its limit 3 times · 7 h waiting');
    const second = rows[1].textContent ?? '';
    expect(second).toContain('Nimbus');
    expect(second).toContain('Default account');
    expect(second).toContain('No limits hit');
    expect(second).toContain('2 min');
  });

  it('adds up an All plans row and lists what is not counted', async () => {
    await mount(fakeApi(FULL));
    const total = host.querySelector('tfoot')?.textContent ?? '';
    expect(total).toContain('All plans');
    expect(total).toContain('47');
    expect(text()).toContain('Counting limits since 12 October 2026');
    expect(text()).toContain('Tokens are not counted');
  });

  it('says "No turns this month yet." when there is nothing, and still says what it counts', async () => {
    await mount(fakeApi(EMPTY));
    expect(text()).toContain('No turns this month yet.');
    expect(host.querySelector('table')).toBeNull();
    expect(text()).toContain('Counting limits from now on');
  });

  it('switches months with the arrows and the list, asking the backend for that month', async () => {
    const api = fakeApi((month) => (month === '2026-09' ? { ...FULL, month: SEP, rows: [ROWS[1]] } : FULL));
    await mount(api);
    expect(api.report).toHaveBeenLastCalledWith(undefined);
    expect(button('Later month').disabled).toBe(true);
    await act(async () => button('Earlier month').click());
    await act(async () => {});
    expect(api.report).toHaveBeenLastCalledWith('2026-09');
    expect(host.querySelector('h1')?.textContent).toBe('Your plans in September 2026');
    expect(host.querySelectorAll('tbody tr')).toHaveLength(1);
    expect(button('Earlier month').disabled).toBe(true);
    await act(async () => button('Later month').click());
    await act(async () => {});
    expect(api.report).toHaveBeenLastCalledWith('2026-10');
  });

  it('shows a plain error when the report cannot be read', async () => {
    const api: PlansApi = { report: vi.fn().mockRejectedValue(new Error('Open Puppet Master on your computer to see your plans.')) };
    await mount(api);
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('Open Puppet Master');
  });

  it('copies the plans as CSV and as text, and says Copied for a moment', async () => {
    vi.useFakeTimers();
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    try {
      await mount(fakeApi(FULL));
      const copyButtons = () => [...host.querySelectorAll<HTMLButtonElement>('.ck-plans-copy button')];
      const copyCsv = () => copyButtons()[0];
      const copyText = () => copyButtons()[1];
      await act(async () => copyCsv().click());
      expect(writeText).toHaveBeenLastCalledWith(plansCsv(ROWS));
      expect(copyCsv().textContent).toBe('Copied');
      await act(async () => copyText().click());
      expect(writeText).toHaveBeenLastCalledWith(plansSummaryText(ROWS));
      expect(copyText().textContent).toBe('Copied');
      await act(async () => vi.advanceTimersByTime(1500));
      expect(copyCsv().textContent).toBe('Copy as CSV');
      expect(copyText().textContent).toBe('Copy as text');
    } finally {
      vi.useRealTimers();
    }
  });

  it('says so when the clipboard refuses, and does not say Copied', async () => {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: vi.fn().mockRejectedValue(new Error('denied')) }, configurable: true });
    await mount(fakeApi(FULL));
    const copyCsv = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Copy as CSV') as HTMLButtonElement;
    await act(async () => copyCsv.click());
    await act(async () => {});
    expect(host.querySelector('[role="status"]')?.textContent).toBe('Could not copy.');
    expect(text()).not.toContain('Copied');
  });

  it('hides the copy buttons for an empty month', async () => {
    await mount(fakeApi(EMPTY));
    expect(text()).not.toContain('Copy as CSV');
    expect(text()).not.toContain('Copy as text');
  });

  it('shows no folder, pane id or chat text, and uses no colour class', async () => {
    await mount(fakeApi(FULL));
    expect(text()).not.toMatch(/[A-Z]:\\|pane-|\/Users\//);
    expect(host.innerHTML).not.toMatch(/class="[^"]*(?:red|green|blue|amber|orange|yellow|purple)/i);
  });
});
