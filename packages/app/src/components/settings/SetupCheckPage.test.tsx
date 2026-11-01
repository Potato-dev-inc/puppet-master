// @vitest-environment jsdom
// The page: rows from the checks, one fix button per problem calls its action, Copy report sends scrubbed text.
import { describe, expect, it, vi } from 'vitest';
import type { AddonList } from '../../lib/acpAddon';
import type { AgentHealth } from '../../lib/agent-health';
import type { SetupCheckDeps } from '../../lib/setup-checks';
import type { RowDef } from './model';
import { SetupCheckPage, type SetupCheckActions } from './SetupCheckPage';
import { buttonByText, click, hasText, mountRows, settle } from './test-utils';

const deps = (over: Partial<SetupCheckDeps> = {}): SetupCheckDeps => ({
  agentHealth: async (): Promise<AgentHealth[]> => [
    {
      id: 'claude',
      label: 'Claude',
      state: 'needs_login',
      installed: true,
      version: '1',
      reason: 'Needs sign-in at C:\\Users\\renee\\work',
      install_hint: null,
      login_hint: null,
      can_test: true,
    },
  ],
  deepseekKeySaved: async () => false,
  sandboxState: async () => 'not_installed',
  addons: async (): Promise<AddonList> => ({ labsEnabled: false, addons: [] }),
  ...over,
});

const actions = (): SetupCheckActions => ({
  signIn: vi.fn(),
  installAddon: vi.fn(),
  addKey: vi.fn(),
  openSandbox: vi.fn(),
});

const pageRow = (
  d: SetupCheckDeps,
  a: SetupCheckActions,
  copy?: (t: string) => Promise<void>,
  labs: Record<string, boolean> = {},
): RowDef => ({
  id: 'setup-check',
  label: 'Check my setup',
  render: () => <SetupCheckPage ctx={{ bridgeUrl: 'http://127.0.0.1:1', labs }} deps={d} actions={a} copy={copy} />,
});

describe('SetupCheckPage', () => {
  it('shows the rows and calls the matching fix action', async () => {
    const a = actions();
    const m = await mountRows([pageRow(deps(), a)]);
    await settle(6);
    expect(hasText(m.host, 'Bridge is running')).toBe(true);
    expect(hasText(m.host, 'Claude needs sign-in')).toBe(true);
    expect(hasText(m.host, 'No DeepSeek key yet')).toBe(true);
    await click(buttonByText(m.host, 'Sign in'));
    expect(a.signIn).toHaveBeenCalledWith('claude');
    await click(buttonByText(m.host, 'Add key'));
    expect(a.addKey).toHaveBeenCalledOnce();
    await m.unmount();
  });

  it('a sandbox that is not set up offers Set up, which opens the sandbox', async () => {
    const a = actions();
    const m = await mountRows([pageRow(deps({ agentHealth: async () => [] }), a, undefined, { potato_sandbox_enabled: true })]);
    await settle(6);
    await click(buttonByText(m.host, 'Set up'));
    expect(a.openSandbox).toHaveBeenCalledOnce();
    await m.unmount();
  });

  it('Copy report sends the rows with the path and key removed', async () => {
    const copy = vi.fn(async () => {});
    const m = await mountRows([pageRow(deps(), actions(), copy)]);
    await settle(6);
    await click(buttonByText(m.host, 'Copy report'));
    await settle();
    const sent = (copy.mock.calls as unknown as string[][])[0]?.[0] ?? '';
    expect(sent).toContain('Claude needs sign-in');
    expect(sent).not.toContain('renee');
    expect(hasText(m.host, 'Report copied')).toBe(true);
    await m.unmount();
  });
});
