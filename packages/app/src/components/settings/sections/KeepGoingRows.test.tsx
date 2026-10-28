// @vitest-environment jsdom
// Settings > Agents > Keep going: the ordered list (add, move, remove, cap 8), the ask-back switch, and every
// change saves through keepGoingSettingsSet. Fixtures use fake agent ids only; no agent name is hard-coded.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentDescriptor } from '../../../lib/agentRegistry';
import { keepGoingChoices, keepGoingLabel, keepGoingSaveError, KEEP_GOING_ORDER_CAP, KeepGoingRows } from './KeepGoingRows';
import { keepGoingSettingsGet, keepGoingSettingsSet, type KeepGoingSettings } from '../../../lib/keep-going';
import { settle, click, byLabel, buttonByText, hasText, mountRows, pickOption, type Mounted } from '../test-utils';

const agent = (id: string, label: string, installed = true, dispatchable = true): AgentDescriptor =>
  ({ id, label, installed, dispatchable }) as AgentDescriptor;

const AGENTS = [agent('fake_a', 'Agent A'), agent('fake_b', 'Agent B'), agent('fake_c', 'Agent C', false)];

vi.mock('../../../lib/agentRegistry', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../../lib/agentRegistry')>();
  return {
    ...real,
    useAgentRegistry: () => ({ agents: AGENTS, dispatchable_ids: [], source: 'registry' }),
  };
});

vi.mock('../../../lib/accounts', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../../lib/accounts')>();
  return {
    ...real,
    tauriAccountsApi: {
      ...real.tauriAccountsApi,
      list: vi.fn(async () => ({
        agents: [{ agent_id: 'fake_a', login_hint: 'Log in once.', home_env: 'FAKE_HOME' }],
        accounts: [{ id: 'acc_work', agent_id: 'fake_a', label: 'Work', home: 'C:/data/accounts/work' }],
      })),
    },
  };
});

vi.mock('../../../lib/keep-going', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../../lib/keep-going')>();
  return { ...real, keepGoingSettingsGet: vi.fn(), keepGoingSettingsSet: vi.fn() };
});

const SAVED: KeepGoingSettings = { order: [{ agent_id: 'fake_b' }, { agent_id: 'fake_a', account: 'acc_work' }], ask_back: true };

let m: Mounted;

async function mount(settings: KeepGoingSettings | Error = SAVED) {
  if (settings instanceof Error) vi.mocked(keepGoingSettingsGet).mockRejectedValue(settings);
  else vi.mocked(keepGoingSettingsGet).mockResolvedValue(settings);
  vi.mocked(keepGoingSettingsSet).mockImplementation(async (next: KeepGoingSettings) => next);
  m = await mountRows([{ id: 'keep-going', label: 'When an agent runs out of usage', group: 'Keep going', render: (env) => <KeepGoingRows onSaved={env.markSaved} /> }]);
  await settle(10);
  return m;
}

const listItems = () => [...m.host.querySelectorAll<HTMLLIElement>('ol[aria-label="Offer order"] > li')];
const askBack = () => byLabel<HTMLButtonElement>(m.host, 'Ask to move it back when the first agent resets');
const addPicker = () => byLabel<HTMLSelectElement>(m.host, 'Agent to add')!;

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(async () => {
  await m?.unmount();
});

describe('Keep going: the saved list', () => {
  it('shows the heading, each listed agent in order, and the ask-back switch on', async () => {
    await mount();
    expect(hasText(m.host, 'When an agent runs out of usage')).toBe(true);
    expect(hasText(m.host, 'Offer to continue on, in order')).toBe(true);
    const items = listItems();
    expect(items.map((li) => li.dataset.targetKey)).toEqual(['fake_b', 'fake_a@acc_work']);
    expect(items[0].textContent).toContain('Agent B');
    expect(items[1].textContent).toContain('Agent A · Work');
    expect(items[0].textContent).toContain('Installed');
    expect(askBack()!.getAttribute('aria-checked')).toBe('true');
  });

  it('marks a listed agent that is no longer installed', async () => {
    await mount({ order: [{ agent_id: 'fake_c' }], ask_back: true });
    expect(listItems()[0].textContent).toContain('Not installed');
  });

  it('says what an empty list means', async () => {
    await mount({ order: [], ask_back: true });
    expect(listItems()).toEqual([]);
    expect(hasText(m.host, 'Nothing listed. Puppet Master then offers every other installed agent, in its usual order.')).toBe(true);
  });

  it('keeps the same-company rule in plain words', async () => {
    await mount();
    expect(hasText(m.host, 'It never moves a task to another login of the same company.')).toBe(true);
  });
});

describe('Keep going: add, move, remove', () => {
  it('the add list offers installed agents and their logins, not listed ones or uninstalled ones', async () => {
    await mount();
    const options = [...addPicker().options].map((o) => [o.value, o.textContent]);
    expect(options).toEqual([
      ['', 'Choose an agent'],
      ['fake_a', 'Agent A'],
    ]);
  });

  it('adds the chosen agent as a target without an account key and saves the whole list', async () => {
    await mount({ order: [], ask_back: true });
    await pickOption(addPicker(), 'fake_a');
    await click(buttonByText(m.host, 'Add'));
    await settle();
    expect(keepGoingSettingsSet).toHaveBeenCalledExactlyOnceWith({ order: [{ agent_id: 'fake_a' }], ask_back: true });
    const [saved] = vi.mocked(keepGoingSettingsSet).mock.calls[0];
    expect(Object.keys(saved.order[0])).toEqual(['agent_id']);
  });

  it('adds a saved login as agent@account', async () => {
    await mount({ order: [], ask_back: true });
    await pickOption(addPicker(), 'fake_a@acc_work');
    await click(buttonByText(m.host, 'Add'));
    await settle();
    expect(keepGoingSettingsSet).toHaveBeenCalledExactlyOnceWith({ order: [{ agent_id: 'fake_a', account: 'acc_work' }], ask_back: true });
  });

  it('Add stays off until an agent is picked', async () => {
    await mount({ order: [], ask_back: true });
    expect(buttonByText(m.host, 'Add')!.disabled).toBe(true);
  });

  it('moving an agent down swaps it with the next one and saves', async () => {
    await mount();
    await click(byLabel(m.host, 'Move Agent B down'));
    await settle();
    expect(keepGoingSettingsSet).toHaveBeenCalledExactlyOnceWith({ order: [SAVED.order[1], SAVED.order[0]], ask_back: true });
  });

  it('the first row cannot move up and the last cannot move down', async () => {
    await mount();
    expect(byLabel<HTMLButtonElement>(m.host, 'Move Agent B up')!.disabled).toBe(true);
    expect(byLabel<HTMLButtonElement>(m.host, 'Move Agent A · Work down')!.disabled).toBe(true);
  });

  it('removing an agent saves the list without it', async () => {
    await mount();
    await click(byLabel(m.host, 'Remove Agent A · Work'));
    await settle();
    expect(keepGoingSettingsSet).toHaveBeenCalledExactlyOnceWith({ order: [{ agent_id: 'fake_b' }], ask_back: true });
  });

  it('the list holds at most 8: the add control is off and says so', async () => {
    const eight = Array.from({ length: KEEP_GOING_ORDER_CAP }, (_, i) => ({ agent_id: `fake_x${i}` }));
    await mount({ order: eight, ask_back: true });
    expect(listItems()).toHaveLength(8);
    expect(addPicker().disabled).toBe(true);
    expect(buttonByText(m.host, 'Add')!.disabled).toBe(true);
    expect(hasText(m.host, 'You can list up to 8 agents.')).toBe(true);
  });
});

describe('Keep going: ask to move it back', () => {
  it('turning the switch off saves ask_back false and keeps the order', async () => {
    await mount();
    await click(askBack());
    await settle();
    expect(keepGoingSettingsSet).toHaveBeenCalledExactlyOnceWith({ order: SAVED.order, ask_back: false });
  });

  it('says it never moves a task back on its own', async () => {
    await mount();
    expect(hasText(m.host, 'Never moves a task back on its own.')).toBe(true);
  });
});

describe('Keep going: errors and the desktop-only case', () => {
  it('a refused save shows the plain sentence without the code, and keeps the last saved list', async () => {
    await mount();
    vi.mocked(keepGoingSettingsSet).mockRejectedValueOnce('KEEP_GOING_BAD_TARGET: That agent is not known here.');
    await click(byLabel(m.host, 'Remove Agent B'));
    await settle();
    const alert = m.host.querySelector('[role="alert"]');
    expect(alert?.textContent).toBe('That agent is not known here.');
    expect(m.host.textContent).not.toContain('KEEP_GOING_BAD_TARGET');
    expect(listItems().map((li) => li.dataset.targetKey)).toEqual(['fake_b', 'fake_a@acc_work']);
  });

  it('outside the desktop app it says so and shows no controls', async () => {
    await mount(new Error('Desktop shell required'));
    expect(hasText(m.host, 'Available in the desktop app.')).toBe(true);
    expect(askBack()).toBeNull();
    expect(addPicker()).toBeNull();
  });
});

describe('Keep going: pure helpers', () => {
  it('keepGoingChoices lists each installed agent, then its logins, and skips listed ones', () => {
    const accounts = [{ id: 'acc_work', agent_id: 'fake_a', label: 'Work', home: 'C:/x' }];
    expect(keepGoingChoices(AGENTS, accounts, []).map((c) => c.key)).toEqual(['fake_a', 'fake_a@acc_work', 'fake_b']);
    expect(keepGoingChoices(AGENTS, accounts, [{ agent_id: 'fake_a', account: 'acc_work' }]).map((c) => c.key)).toEqual(['fake_a', 'fake_b']);
  });

  it('keepGoingChoices leaves out an agent that cannot run here', () => {
    const agents = [agent('fake_d', 'Agent D', true, false)];
    expect(keepGoingChoices(agents, [], [])).toEqual([]);
  });

  it('keepGoingLabel names a removed login plainly', () => {
    expect(keepGoingLabel({ agent_id: 'fake_a', account: 'gone' }, AGENTS, [])).toBe('Agent A · login removed');
    expect(keepGoingLabel({ agent_id: 'fake_z' }, AGENTS, [])).toBe('fake_z');
  });

  it('keepGoingSaveError strips the code and falls back to a plain sentence', () => {
    expect(keepGoingSaveError(new Error('KEEP_GOING_X: Try a shorter list.'))).toBe('Try a shorter list.');
    expect(keepGoingSaveError('KEEP_GOING_X:   ')).toBe("Couldn't save. Try again.");
  });
});
