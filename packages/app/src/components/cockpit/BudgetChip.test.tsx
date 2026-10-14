// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { chipText, helperLine, type BudgetApi, type BudgetView } from '../../lib/budgets';
import { BudgetChip } from './BudgetChip';

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

const view = (over: Partial<BudgetView> = {}): BudgetView => ({
  state: 'ok',
  depth: 0,
  is_helper: false,
  turns: { used: 12, own: 8, held_for_helpers: 4, limit: 20, add_step: 10 },
  minutes: null,
  time_up_in_turn: false,
  helpers: [],
  ...over,
});

async function mount(api: BudgetApi, onNotify = vi.fn()) {
  await act(async () => {
    root.render(<BudgetChip paneId="p1" api={api} onNotify={onNotify} />);
  });
  return onNotify;
}
const chip = () => host.querySelector<HTMLButtonElement>('[data-testid="budget-chip"]');
const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === text);

describe('the budget chip', () => {
  it('shows nothing when the pane has no budget (the default)', async () => {
    await mount({ forPane: async () => null, adjust: vi.fn() });
    expect(chip()).toBeNull();
    expect(host.textContent).toBe('');
  });

  it('reads "12 of 20 turns" and stays regular weight below 80%', async () => {
    await mount({ forPane: async () => view(), adjust: vi.fn() });
    expect(chip()!.textContent).toBe('12 of 20 turns');
    expect(chip()!.className).toContain('is-ok');
  });

  it('goes bold at 80% or more and says so when used up, in words, with no ids', async () => {
    await mount({ forPane: async () => view({ state: 'nearly', turns: { used: 16, own: 16, held_for_helpers: 0, limit: 20, add_step: 10 } }), adjust: vi.fn() });
    expect(chip()!.className).toContain('is-nearly');
    expect(chip()!.textContent).toBe('16 of 20 turns');
    expect(chipText(view({ state: 'used_up' }))).toBe('Budget used up');
    expect(chipText(view({ state: 'stopped' }))).toBe('Stopped at its budget');
    expect(chipText(view({ time_up_in_turn: true }))).toBe("Time's up · pauses after this turn");
    expect(chipText(view({ turns: null, minutes: { used: 14, limit: 60, add_step: 30 } }))).toBe('14 of 60 min');
    expect(chipText(view({ turns: { used: 1, own: 1, held_for_helpers: 0, limit: 1, add_step: 1 } }))).toBe('1 of 1 turn');
  });

  it('opens a popover with turns, time, helpers and the pause note', async () => {
    const api: BudgetApi = {
      forPane: async () =>
        view({
          minutes: { used: 14, limit: 60, add_step: 30 },
          helpers: [{ agent: 'Codex', used: 4, limit: 10, closed: false }],
        }),
      adjust: vi.fn(),
    };
    await mount(api);
    await act(async () => chip()!.click());
    const pop = host.querySelector('[data-testid="budget-popover"]')!;
    expect(pop.textContent).toContain('Turns12 of 20');
    expect(pop.textContent).toContain('Time14 of 60 min');
    expect(pop.textContent).toContain('Codex · 4 of 10 turns (from this budget)');
    expect(pop.textContent).toContain('Pauses after the current turn. Nothing is stopped mid-way.');
    expect(button('Add 10 turns')).toBeTruthy();
    expect(button('Add 30 min')).toBeTruthy();
    expect(helperLine({ agent: 'Codex', used: 4, limit: 10, closed: true })).toBe('Codex · 4 of 10 turns, finished (from this budget)');
  });

  it('Add 10 turns and Stop here call the app and tell the person what happened', async () => {
    const adjust = vi.fn(async (_pane: string, action: string) => (action === 'stop' ? 'Stopped at its budget.' : 'Added 10 turns.'));
    const onNotify = await mount({ forPane: async () => view(), adjust });
    await act(async () => chip()!.click());
    await act(async () => button('Add 10 turns')!.click());
    expect(adjust).toHaveBeenCalledWith('p1', 'add_turns');
    expect(onNotify).toHaveBeenCalledWith('Added 10 turns.');
    await act(async () => button('Stop here')!.click());
    expect(adjust).toHaveBeenCalledWith('p1', 'stop');
  });

  it('a stopped run offers no more turns', async () => {
    await mount({ forPane: async () => view({ state: 'stopped' }), adjust: vi.fn() });
    await act(async () => chip()!.click());
    expect(button('Add 10 turns')).toBeUndefined();
    expect(button('Stop here')).toBeUndefined();
  });
});
