// @vitest-environment jsdom
// "Your plans" on the phone: one card per plan, the month switcher, the empty and error states. The screen only
// reads; nothing on it writes.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PlanMonth, PlanRow, PlansApi, PlansReport } from '../../lib/plans-report';
import { PhonePlans } from './PhonePlans';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

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

const FULL: PlansReport = {
  month: OCT,
  months: [OCT, SEP],
  rows: [
    row({
      accountId: 'acct-work',
      accountLabel: 'Work',
      turns: 42,
      busyMs: (3 * 60 + 12) * 60_000,
      files: { knownTurns: 40, changed: 12, added: 340, deleted: 45 },
      merged: 2,
      limits: { hits: 3, limitedMs: 7 * 3_600_000 },
    }),
    row({ agentId: 'nimbus', agentLabel: 'Nimbus', turns: 1, busyMs: 60_000 }),
  ],
  totals: { turns: 43, busyMs: (3 * 60 + 13) * 60_000, filesChanged: 12, added: 340, deleted: 45, merged: 2, limitHits: 3, limitedMs: 7 * 3_600_000 },
  notes: ['Counting limits since 12 October 2026. Earlier limits were not recorded.', 'Tokens are not counted.'],
};

const EMPTY: PlansReport = {
  month: OCT,
  months: [OCT],
  rows: [],
  totals: { turns: 0, busyMs: 0, filesChanged: 0, added: 0, deleted: 0, merged: 0, limitHits: 0, limitedMs: 0 },
  notes: [],
};

/** An api that answers the given report per month id (`undefined` = this month). */
function apiWith(answer: (month?: string) => PlansReport): PlansApi & { report: ReturnType<typeof vi.fn> } {
  return { report: vi.fn(async (month?: string) => answer(month)) } as PlansApi & { report: ReturnType<typeof vi.fn> };
}

async function mount(api: PlansApi, onClose = () => {}) {
  await act(async () => root.render(<PhonePlans api={api} onClose={onClose} />));
  // Let the pending promise of the first read settle.
  await act(async () => {});
}

const button = (label: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;
const cards = () => [...host.querySelectorAll<HTMLElement>('.ph-plans-card')];

describe('PhonePlans', () => {
  it('is titled "Your plans" and shows one card per plan with agent, account, turns, time, files and limits', async () => {
    await mount(apiWith(() => FULL));
    expect(host.querySelector('[role="dialog"]')?.getAttribute('aria-label')).toBe('Your plans');
    expect(host.querySelector('.ph-plans-month')?.textContent).toBe('October 2026');
    const list = cards();
    expect(list).toHaveLength(2);
    expect(list[0].querySelector('.ph-plans-name')?.textContent).toBe('Alpha Code · Work');
    expect(list[0].querySelector('.ph-plans-stats')?.textContent).toBe('42 turns · 3 h 12 min');
    expect(list[0].querySelectorAll('.ph-plans-line')[0].textContent).toBe('12 files +340 −45');
    expect(list[0].querySelectorAll('.ph-plans-line')[1].textContent).toBe('Hit its limit 3 times · 7 h waiting');
    expect(list[1].querySelector('.ph-plans-name')?.textContent).toBe('Nimbus · Default account');
    expect(list[1].querySelector('.ph-plans-stats')?.textContent).toBe('1 turn · 1 min');
    expect(host.querySelector('.ph-plans-total')?.textContent).toBe('All plans · 43 turns · 3 h 13 min');
    expect(host.querySelector('.ph-plans-notes')?.textContent).toContain('Counting limits since 12 October 2026.');
  });

  it('moves to an earlier month with the switcher and asks the PC for that month', async () => {
    const api = apiWith((month) => (month === '2026-09' ? { ...EMPTY, month: SEP, months: [OCT, SEP] } : FULL));
    await mount(api);
    expect(button('Earlier month').disabled).toBe(false);
    // This month is the newest, so there is no later one.
    expect(button('Later month').disabled).toBe(true);
    await act(async () => button('Earlier month').click());
    await act(async () => {});
    expect(api.report).toHaveBeenLastCalledWith('2026-09');
    expect(host.querySelector('.ph-plans-month')?.textContent).toBe('September 2026');
    await act(async () => button('Later month').click());
    await act(async () => {});
    expect(api.report).toHaveBeenLastCalledWith('2026-10');
  });

  it('says "No turns this month yet." when nothing ran', async () => {
    await mount(apiWith(() => EMPTY));
    expect(host.querySelector('.ph-plans-empty')?.textContent).toBe('No turns this month yet.');
    expect(cards()).toHaveLength(0);
  });

  it('shows the reason in a plain line when the PC cannot give the plans', async () => {
    const api: PlansApi = { report: async () => Promise.reject(new Error('This PC is too old to show your plans on the phone.')) };
    await mount(api);
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('This PC is too old to show your plans on the phone.');
    expect(cards()).toHaveLength(0);
  });

  it('is read only: the only buttons are the month switcher and Done', async () => {
    await mount(apiWith(() => FULL));
    const labels = [...host.querySelectorAll('button')].map((b) => b.getAttribute('aria-label') || b.textContent);
    // The sheet's own Done button (in the title bar, first) is labelled "Close" for screen readers.
    expect(labels).toEqual(['Close', 'Earlier month', 'Later month']);
    expect(host.querySelectorAll('input, textarea, select')).toHaveLength(0);
  });

  it('keeps every month button at 44 px in its stylesheet', () => {
    const css = readFileSync(join(__dirname, '../../styles/ph-plans.css'), 'utf8');
    const icon = css.slice(css.indexOf('.ph-plans-icon {'), css.indexOf('}', css.indexOf('.ph-plans-icon {')));
    expect(icon).toMatch(/width:\s*44px/);
    expect(icon).toMatch(/height:\s*44px/);
  });
});
