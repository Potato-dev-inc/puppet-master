// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { EMPTY_CANARY, type CanaryApi, type CanaryStatus } from '../../lib/canary';
import { CHANGED_SCREENS_STATUS, CLAUDE_UNTESTED_STATUS, UPDATED_GOOD_STATUS, VERSIONS_STATUS } from '../../dev/canary-fixtures';
import { UntestedChip } from './UntestedChip';
import { CanaryNotices } from './CanaryNotices';
import { AgentVersions } from '../settings/blocks/AgentVersions';

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

const flush = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });

function fakeApi(status: CanaryStatus, over: Partial<CanaryApi> = {}): CanaryApi {
  return {
    status: vi.fn(async () => status),
    check: vi.fn(async (agent: string) => status.agents.find((a) => a.agent === agent)!),
    setAuto: vi.fn(async (on: boolean) => ({ ...status, auto_check: on })),
    dismissNotice: vi.fn(async () => {}),
    openCaptures: vi.fn(async () => 'C:\\captures'),
    onChanged: vi.fn(async () => () => {}),
    ...over,
  };
}

describe('UntestedChip', () => {
  it('shows "Untested version" for an agent in strict mode, with the plain sentence as its tooltip', async () => {
    await act(async () => root.render(<UntestedChip agent="claude" api={fakeApi(CLAUDE_UNTESTED_STATUS)} />));
    await flush();
    const chip = host.querySelector('[data-testid="untested-chip"]');
    expect(chip?.textContent).toBe('Untested version');
    expect(chip?.getAttribute('title')).toContain('has not been tested with Claude Code 2.4.0');
  });

  it('shows nothing for another agent, or when there is no status', async () => {
    await act(async () => root.render(<UntestedChip agent="codex" api={fakeApi(CLAUDE_UNTESTED_STATUS)} />));
    await flush();
    expect(host.querySelector('[data-testid="untested-chip"]')).toBeNull();
    await act(async () => root.render(<UntestedChip agent="claude" api={fakeApi(EMPTY_CANARY)} />));
    await flush();
    expect(host.querySelector('[data-testid="untested-chip"]')).toBeNull();
  });
});

describe('CanaryNotices', () => {
  it('shows the changed-screens card and dismisses it', async () => {
    const api = fakeApi(CHANGED_SCREENS_STATUS);
    await act(async () => root.render(<CanaryNotices api={api} />));
    await flush();
    expect(host.querySelector('[data-testid="canary-notice"]')?.textContent).toContain(
      'Codex 0.159.3 \u2192 0.162.1: screens changed. Puppet Master will ask you before acting.',
    );
    const button = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Dismiss')!;
    await act(async () => button.click());
    expect(api.dismissNotice).toHaveBeenCalledWith('canary:codex:0.162.1');
  });

  it('renders nothing without notices', async () => {
    await act(async () => root.render(<CanaryNotices api={fakeApi(VERSIONS_STATUS)} />));
    await flush();
    expect(host.querySelector('[data-testid="canary-notice"]')).toBeNull();
  });
});

describe('AgentVersions (Settings > Agents)', () => {
  it('lists each installed agent with its version, chip, plain sentence and last check', async () => {
    const at = VERSIONS_STATUS.agents[0].last!.at_ms;
    await act(async () => root.render(<AgentVersions api={fakeApi(VERSIONS_STATUS)} now={() => at + 12 * 60_000} />));
    await flush();
    const codex = host.querySelector('[data-testid="agent-version-codex"]')!;
    expect(codex.textContent).toContain('0.171.0');
    expect(host.querySelector('[data-testid="agent-version-chip-codex"]')?.textContent).toBe('Untested version');
    expect(codex.textContent).toContain('ask you before closing a start-up notice');
    expect(host.querySelector('[data-testid="agent-version-chip-claude"]')).toBeNull();
    expect(host.textContent).toContain('Checked 12 minutes ago');
    expect(host.textContent).toContain('2 screens saved on this computer');
  });

  it('says how the last update got on: checked and fine (no chip), or screens changed', async () => {
    const at = UPDATED_GOOD_STATUS.agents[0].change!.at_ms;
    await act(async () => root.render(<AgentVersions api={fakeApi(UPDATED_GOOD_STATUS)} now={() => at + 2 * 3_600_000} />));
    await flush();
    expect(host.querySelector('[data-testid="agent-version-change-claude"]')?.textContent).toBe(
      'Claude Code 2.3.0 \u2192 2.4.0: checked, all good. Updated 2 hours ago.',
    );
    expect(host.querySelector('[data-testid="agent-version-chip-claude"]')).toBeNull();
    expect(host.querySelector('[data-testid="agent-version-claude"]')?.textContent).toContain('was checked on this computer');

    await act(async () => root.render(<AgentVersions api={fakeApi(CHANGED_SCREENS_STATUS)} now={() => at} />));
    await flush();
    const row = host.querySelector('[data-testid="agent-version-change-codex"]')!;
    expect(row.getAttribute('data-verdict')).toBe('changed');
    expect(row.textContent).toContain('Codex 0.159.3 \u2192 0.162.1: screens changed. Puppet Master will ask you before acting.');
    // An agent with no update to report shows no such line.
    expect(host.querySelector('[data-testid="agent-version-change-claude"]')).toBeNull();
  });

  it('"Check this agent" runs the check for that agent and shows it is busy meanwhile', async () => {
    let finish: () => void = () => {};
    const api = fakeApi(VERSIONS_STATUS, {
      check: vi.fn(
        () =>
          new Promise((resolve) => {
            finish = () => resolve(VERSIONS_STATUS.agents[1]);
          }),
      ) as CanaryApi['check'],
    });
    await act(async () => root.render(<AgentVersions api={api} />));
    await flush();
    const button = host.querySelector('[data-testid="agent-version-codex"] button') as HTMLButtonElement;
    expect(button.textContent).toBe('Check this agent');
    await act(async () => button.click());
    expect(api.check).toHaveBeenCalledWith('codex');
    expect((host.querySelector('[data-testid="agent-version-codex"] button') as HTMLButtonElement).disabled).toBe(true);
    expect(host.textContent).toContain('Checking. This can take up to a minute.');
    await act(async () => finish());
    await flush();
    expect((host.querySelector('[data-testid="agent-version-codex"] button') as HTMLButtonElement).disabled).toBe(false);
  });

  it('shows a failed check as a sentence', async () => {
    const api = fakeApi(VERSIONS_STATUS, {
      check: vi.fn(async () => {
        throw new Error('Codex is already being checked');
      }),
    });
    await act(async () => root.render(<AgentVersions api={api} />));
    await flush();
    const button = host.querySelector('[data-testid="agent-version-codex"] button') as HTMLButtonElement;
    await act(async () => button.click());
    await flush();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Codex is already being checked');
  });

  it('the update switch and the folder button call the backend', async () => {
    const api = fakeApi(VERSIONS_STATUS);
    await act(async () => root.render(<AgentVersions api={api} />));
    await flush();
    await act(async () => (host.querySelector('[role="switch"]') as HTMLButtonElement).click());
    expect(api.setAuto).toHaveBeenCalledWith(false);
    const open = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Open folder')!;
    await act(async () => open.click());
    expect(api.openCaptures).toHaveBeenCalled();
  });
});
