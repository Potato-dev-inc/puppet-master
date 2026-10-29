// @vitest-environment jsdom
// "Pick for me" in the "+" menu: the first row, the pick panel, Start, Choose myself, and the stale-answer guard.
// Generic fakes only (fake_a, fake_b): the menu never names an agent.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AccountsApi } from '../../lib/accounts';
import type { PickApi, PickResult } from '../../lib/pick-for-me';
import type { ShipApi } from '../../lib/ship-api';
import { NewAgentMenu, type LaunchEntry, type ProjectChoice } from './NewAgentMenu';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const accounts = { list: async () => ({ agents: [], accounts: [] }) } as unknown as AccountsApi;
const ENTRIES: LaunchEntry[] = [
  { type: 'fake_a', label: 'Fake A' },
  { type: 'fake_b', label: 'Fake B' },
];
const SHOP: ProjectChoice = { path: 'C:\\Users\\you\\shop', name: 'shop', state: 'repo' };
const SITE: ProjectChoice = { path: 'C:\\Users\\you\\site', name: 'site', state: 'repo' };

/** A ready answer: Fake A is the choice, with saved login acc_1. */
function answer(over: Partial<PickResult> = {}): PickResult {
  return {
    choice: { agentId: 'fake_a', accountId: 'acc_1', label: 'Fake A' },
    reason: 'Fake A passed its checks 9 of 10 times here; Fake B 5 of 10.',
    basis: 'clear',
    metric: 'checks_green',
    sample: 'steady',
    considered: [
      { label: 'Fake A', state: 'ready', line: '9 of 10 checks green' },
      { label: 'Fake B', state: 'ready', line: '5 of 10 checks green' },
    ],
    notes: ['Counted from your own turns in this project, last 90 days.'],
    ...over,
  };
}

function deferred() {
  let resolve!: (result: PickResult) => void;
  const promise = new Promise<PickResult>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

const ship = () =>
  ({
    recent: vi.fn().mockResolvedValue([]),
    pickFolder: vi.fn().mockResolvedValue(null),
    forget: vi.fn().mockResolvedValue(undefined),
    gitInitPlan: vi.fn(),
    gitInit: vi.fn(),
  }) as unknown as ShipApi;

let host: HTMLDivElement;
let root: Root;
let onStart: ReturnType<typeof vi.fn>;
let onClose: ReturnType<typeof vi.fn>;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  onStart = vi.fn();
  onClose = vi.fn();
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes(text));
const pickRow = () => Array.from(host.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]')).find((b) => b.textContent?.includes('Pick for me'));
const click = async (el: HTMLElement | undefined, label: string) => {
  if (!el) throw new Error(`no "${label}" in: ${host.textContent}`);
  await act(async () => el.click());
};

interface View {
  pick?: PickApi;
  project?: ProjectChoice | null;
  entries?: LaunchEntry[];
}

/** Draws the menu. Pass `pick: undefined` or `project: null` to leave that prop out. */
async function show(over: View = {}) {
  const pick = 'pick' in over ? over.pick : { pick: vi.fn(async (_project: string) => answer()) };
  const project = 'project' in over ? over.project : SHOP;
  const render = (folder: ProjectChoice | null | undefined) =>
    root.render(
      <NewAgentMenu
        entries={over.entries ?? ENTRIES}
        readyAgents={new Set()}
        onStart={onStart}
        onClose={onClose}
        accountsApi={accounts}
        project={folder}
        onProject={() => undefined}
        ship={ship()}
        pick={pick}
      />,
    );
  await act(async () => render(project));
  return { rerender: (folder: ProjectChoice) => act(async () => render(folder)) };
}

describe('the Pick for me row', () => {
  it('is not drawn without a pick API, and the list still works', async () => {
    await show({ pick: undefined });
    expect(pickRow()).toBeUndefined();
    expect(host.textContent).not.toContain('Pick for me');
    await click(button('Fake B'), 'Fake B');
    expect(onStart.mock.calls[0][0]).toBe('fake_b');
  });

  it('is not drawn without a project, and nothing is asked', async () => {
    const pick = { pick: vi.fn(async (_project: string) => answer()) };
    await show({ pick, project: null });
    expect(pickRow()).toBeUndefined();
    expect(pick.pick).not.toHaveBeenCalled();
  });

  it('shows Looking... while the call runs, then the agent label, and starts nothing', async () => {
    const d = deferred();
    const pick = { pick: vi.fn((_project: string) => d.promise) };
    await show({ pick });
    expect(pickRow()?.textContent).toContain('Looking...');
    expect(pickRow()?.disabled).toBe(true);
    await act(async () => d.resolve(answer()));
    expect(pickRow()?.textContent).toContain('Fake A');
    expect(pickRow()?.disabled).toBe(false);
    expect(onStart).not.toHaveBeenCalled();
  });

  it('shows None ready when no agent is ready', async () => {
    await show({ pick: { pick: vi.fn(async () => answer({ choice: null, basis: 'none_ready', reason: 'No agent is installed yet.', considered: [], notes: [] })) } });
    expect(pickRow()?.textContent).toContain('None ready');
  });

  it('is not drawn when the call fails, and the list still works', async () => {
    await show({ pick: { pick: vi.fn(async () => Promise.reject(new Error('boom'))) } });
    expect(pickRow()).toBeUndefined();
    await click(button('Fake A'), 'Fake A');
    expect(onStart.mock.calls[0][0]).toBe('fake_a');
  });

  it('asks only once for the folder it is given, with the path and nothing else', async () => {
    const pick = { pick: vi.fn(async (_project: string) => answer()) };
    await show({ pick });
    expect(pick.pick).toHaveBeenCalledTimes(1);
    expect(pick.pick.mock.calls[0]).toEqual([SHOP.path]);
  });

  it('asks again when the folder changes, and an old answer is ignored', async () => {
    const first = deferred();
    const second = deferred();
    const pick = { pick: vi.fn((project: string) => (project === SHOP.path ? first.promise : second.promise)) };
    const { rerender } = await show({ pick });
    await rerender(SITE);
    await act(async () => second.resolve(answer({ choice: { agentId: 'fake_b', label: 'Fake B' }, reason: 'Fake B passed its checks here.' })));
    expect(pickRow()?.textContent).toContain('Fake B');
    await act(async () => first.resolve(answer({ reason: 'Fake A passed its checks for shop.' })));
    expect(pickRow()?.textContent).toContain('Fake B');
    expect(host.textContent).not.toContain('for shop');
    expect(pick.pick.mock.calls.map((c) => c[0])).toEqual([SHOP.path, SITE.path]);
  });
});

describe('the pick panel', () => {
  it('shows the name and one plain sentence', async () => {
    await show();
    await click(pickRow(), 'Pick for me');
    expect(host.querySelector('.ck-pick-name')?.textContent).toBe('Fake A');
    expect(host.querySelector('.ck-pick-reason')?.textContent).toBe('Fake A passed its checks 9 of 10 times here; Fake B 5 of 10.');
  });

  it('shows the early chip only for a few runs', async () => {
    await show({ pick: { pick: vi.fn(async () => answer({ sample: 'early' })) } });
    await click(pickRow(), 'Pick for me');
    expect(host.querySelector('.ck-pick-chip')?.textContent).toBe('early');
  });

  it('has no early chip at steady or none', async () => {
    await show({ pick: { pick: vi.fn(async () => answer({ sample: 'steady' })) } });
    await click(pickRow(), 'Pick for me');
    expect(host.querySelector('.ck-pick-chip')).toBeNull();
  });

  it('Why this one is closed and lists every candidate, the notes and the footer', async () => {
    await show();
    await click(pickRow(), 'Pick for me');
    const why = host.querySelector('.ck-pick-why');
    expect(why?.hasAttribute('open')).toBe(false);
    const lines = Array.from(host.querySelectorAll('.ck-pick-line')).map((l) => l.textContent);
    expect(lines).toEqual(['Fake A9 of 10 checks green', 'Fake B5 of 10 checks green']);
    expect(host.querySelector('.ck-pick-foot')?.textContent).toBe('Counted from your own turns in this project, last 90 days.');
    expect(host.textContent).toContain('Worked out on your computer. Nothing you type is sent anywhere.');
  });

  it('Start calls onStart once with the agent and its login, then closes the menu', async () => {
    await show();
    await click(pickRow(), 'Pick for me');
    await click(button('Start Fake A'), 'Start Fake A');
    expect(onStart).toHaveBeenCalledTimes(1);
    expect(onStart).toHaveBeenCalledWith('fake_a', 'acc_1');
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('Choose myself goes back to the list and starts nothing', async () => {
    await show();
    await click(pickRow(), 'Pick for me');
    await click(button('Choose myself'), 'Choose myself');
    expect(onStart).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(host.querySelector('.ck-pick')).toBeNull();
    expect(pickRow()).toBeDefined();
    expect(button('Fake B')).toBeDefined();
  });

  it('with no choice shows the reason and only Back', async () => {
    const reason = 'No agent is ready: Fake A - Not installed on this computer.';
    await show({ pick: { pick: vi.fn(async () => answer({ choice: null, basis: 'none_ready', reason, considered: [], notes: [] })) } });
    await click(pickRow(), 'Pick for me');
    expect(host.querySelector('.ck-pick-reason')?.textContent).toBe(reason);
    const buttons = Array.from(host.querySelectorAll<HTMLButtonElement>('.ck-pick button')).map((b) => b.textContent);
    expect(buttons).toEqual(['Back']);
    await click(button('Back'), 'Back');
    expect(host.querySelector('.ck-pick')).toBeNull();
    expect(onStart).not.toHaveBeenCalled();
  });

  it.each([
    ['is not in the menu', [{ type: 'fake_b', label: 'Fake B' }] as LaunchEntry[]],
    ['is disabled in the menu', [{ type: 'fake_a', label: 'Fake A', disabled: true }, { type: 'fake_b', label: 'Fake B' }] as LaunchEntry[]],
  ])('a choice that %s is not startable: the reason and only Back', async (_case, entries) => {
    await show({ entries });
    await click(pickRow(), 'Pick for me');
    expect(host.textContent).toContain("Fake A can't be started from this menu.");
    const buttons = Array.from(host.querySelectorAll<HTMLButtonElement>('.ck-pick button')).map((b) => b.textContent);
    expect(buttons).toEqual(['Back']);
    expect(button('Start Fake A')).toBeUndefined();
    expect(onStart).not.toHaveBeenCalled();
  });
});
