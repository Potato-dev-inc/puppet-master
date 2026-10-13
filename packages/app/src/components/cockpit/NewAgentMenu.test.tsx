// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Account, AccountsApi } from '../../lib/accounts';
import { NewAgentMenu } from './NewAgentMenu';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ENTRIES = [
  { type: 'claude', label: 'Claude Code' },
  { type: 'codex', label: 'Codex' },
];

function api(accounts: Account[]): AccountsApi {
  return {
    list: async () => ({ agents: [], accounts }),
    add: vi.fn(),
    rename: vi.fn(),
    remove: vi.fn(),
    openLoginPane: vi.fn(),
  } as unknown as AccountsApi;
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const item = (label: string) =>
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes(label));

async function mount(accounts: Account[]) {
  const onStart = vi.fn();
  const onClose = vi.fn();
  await act(async () => {
    root.render(
      <NewAgentMenu entries={ENTRIES} readyAgents={new Set(['codex'])} onStart={onStart} onClose={onClose} accountsApi={api(accounts)} />,
    );
  });
  return { onStart, onClose };
}

describe('NewAgentMenu', () => {
  it('an agent without accounts starts straight away', async () => {
    const { onStart, onClose } = await mount([]);
    await act(async () => item('Codex')!.click());
    expect(onStart).toHaveBeenCalledWith('codex', undefined);
    expect(onClose).toHaveBeenCalled();
  });

  it('an agent with accounts asks which login, then starts under it', async () => {
    const work = { id: 'acc_1', agent_id: 'claude', label: 'Work', home: 'C:/a' };
    const { onStart } = await mount([work, { id: 'acc_2', agent_id: 'claude', label: 'Personal', home: 'C:/b' }]);
    expect(item('Claude Code')!.textContent).toContain('3 logins');
    await act(async () => item('Claude Code')!.click());
    expect(onStart).not.toHaveBeenCalled();
    expect(host.textContent).toContain('Default login');
    expect(host.textContent).toContain('Personal');
    await act(async () => item('Work')!.click());
    expect(onStart).toHaveBeenCalledWith('claude', 'acc_1');
  });

  it('the default login starts the agent without an account; Back returns to the list', async () => {
    const { onStart } = await mount([{ id: 'acc_1', agent_id: 'claude', label: 'Work', home: 'C:/a' }]);
    await act(async () => item('Claude Code')!.click());
    await act(async () => item('Back')!.click());
    expect(item('Codex')).toBeTruthy();
    await act(async () => item('Claude Code')!.click());
    await act(async () => item('Default login')!.click());
    expect(onStart).toHaveBeenCalledWith('claude', undefined);
  });

  it('accounts of another agent do not add a step', async () => {
    const { onStart } = await mount([{ id: 'acc_1', agent_id: 'claude', label: 'Work', home: 'C:/a' }]);
    await act(async () => item('Codex')!.click());
    expect(onStart).toHaveBeenCalledWith('codex', undefined);
  });
});
