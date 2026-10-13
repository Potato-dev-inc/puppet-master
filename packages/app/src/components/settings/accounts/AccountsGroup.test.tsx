// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Account, AccountsApi, AccountsOverview } from '../../../lib/accounts';
import { AccountsGroup } from './AccountsGroup';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const AGENTS = [
  { agent_id: 'claude', home_env: 'CLAUDE_CONFIG_DIR', login_hint: 'Run `claude` and log in, then close it.' },
  { agent_id: 'codex', home_env: 'CODEX_HOME', login_hint: 'Run `codex` and sign in, then close it.' },
];

function fakeApi(initial: Account[] = []) {
  let accounts = [...initial];
  const api = {
    list: vi.fn(async (): Promise<AccountsOverview> => ({ agents: AGENTS, accounts })),
    add: vi.fn(async (agentId: string, label: string) => {
      const a = { id: `acc_${accounts.length + 1}`, agent_id: agentId, label, home: `C:/data/accounts/${accounts.length + 1}` };
      accounts = [...accounts, a];
      return a;
    }),
    rename: vi.fn(async (id: string, label: string) => {
      accounts = accounts.map((a) => (a.id === id ? { ...a, label } : a));
      return accounts.find((a) => a.id === id)!;
    }),
    remove: vi.fn(async (id: string) => {
      accounts = accounts.filter((a) => a.id !== id);
      return false;
    }),
    openLoginPane: vi.fn(async () => 'pane-1'),
  };
  return api satisfies AccountsApi;
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

async function render(api: AccountsApi) {
  await act(async () => {
    root.render(<AccountsGroup api={api} />);
  });
}
const button = (scope: ParentNode, label: string) =>
  Array.from(scope.querySelectorAll('button')).find((b) => b.textContent?.trim() === label);

describe('AccountsGroup', () => {
  it('lists only agents that support accounts, with their login hint', async () => {
    await render(fakeApi());
    expect(host.querySelector('[data-testid="accounts-claude"]')?.textContent).toContain('Run `claude` and log in');
    expect(host.querySelector('[data-testid="accounts-codex"]')).toBeTruthy();
    expect(host.querySelector('[data-testid="accounts-opencode"]')).toBeNull();
    expect(host.textContent).toContain('never reads or shares');
  });

  it('adds an account for one agent', async () => {
    const api = fakeApi();
    await render(api);
    const claude = host.querySelector('[data-testid="accounts-claude"]') as HTMLElement;
    const input = claude.querySelector('input[aria-label="New claude account name"]') as HTMLInputElement;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'Work');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await act(async () => {
      button(claude, 'Add account')!.click();
    });
    expect(api.add).toHaveBeenCalledWith('claude', 'Work');
    expect(host.querySelector('[data-testid="accounts-claude"]')?.querySelector('input[aria-label="Name of Work"]')).toBeTruthy();
  });

  it('Open login pane starts that agent under that account', async () => {
    const api = fakeApi([{ id: 'acc_9', agent_id: 'codex', label: 'Personal', home: 'C:/a' }]);
    await render(api);
    const codex = host.querySelector('[data-testid="accounts-codex"]') as HTMLElement;
    await act(async () => {
      button(codex, 'Open login pane')!.click();
    });
    expect(api.openLoginPane).toHaveBeenCalledWith('codex', 'acc_9');
    expect(host.textContent).toContain('Log in there once');
  });

  it('remove keeps the folder (deleteHome stays false)', async () => {
    const api = fakeApi([{ id: 'acc_9', agent_id: 'claude', label: 'Work', home: 'C:/a' }]);
    await render(api);
    await act(async () => {
      button(host, 'Remove')!.click();
    });
    expect(api.remove).toHaveBeenCalledWith('acc_9', false);
  });

  it('shows a backend error plainly', async () => {
    const api = fakeApi([{ id: 'acc_9', agent_id: 'claude', label: 'Work', home: 'C:/a' }]);
    api.remove.mockRejectedValueOnce(new Error('unknown account: acc_9'));
    await render(api);
    await act(async () => {
      button(host, 'Remove')!.click();
    });
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('unknown account');
  });

  it('renders nothing without a backend', async () => {
    const api = fakeApi();
    api.list.mockRejectedValueOnce(new Error('Desktop shell required'));
    await render(api);
    expect(host.innerHTML).toBe('');
  });
});
