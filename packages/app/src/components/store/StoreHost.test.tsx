// @vitest-environment jsdom
// The Store page in the cockpit: a card opens its detail sheet, the sheet's open and close go back to the host,
// a change in the sheet reloads the grid, and "Check it works" appears only when the host can run a check.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentStoreApi, StoreCard, StoreDetail } from '../../lib/agent-store';
import type { RunStoreCheck } from './StoreCheck';
import { StoreHost } from './StoreHost';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

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

const flush = () => act(async () => {});
const button = (name: string) =>
  [...document.body.querySelectorAll('button')].find((b) => b.textContent?.trim() === name || b.getAttribute('aria-label') === name) as
    | HTMLButtonElement
    | undefined;
// Sheets sit in front of the grid: a button of the open sheet, else a button of the page.
const sheet = () => document.body.querySelector('.ck-sheet');
const inSheet = (name: string) =>
  [...(sheet()?.querySelectorAll('button') ?? [])].find((b) => b.textContent?.trim() === name) as HTMLButtonElement | undefined;
const click = async (el: HTMLElement | undefined) => {
  if (!el) throw new Error('button not found');
  await act(async () => el.click());
};

const card = (over: Partial<StoreCard> = {}): StoreCard => ({
  id: 'qwen',
  label: 'Qwen Code',
  icon: 'Q',
  tagline: 'A terminal coding agent.',
  tier: 'starter',
  builtin: false,
  categories: ['open_source'],
  status: 'active',
  replaced_by: '',
  vendor: { name: 'Vendor', homepage: 'https://vendor.example', license: 'Apache-2.0', license_kind: 'open_source', license_url: null, terms_url: null },
  sign_in: { kind: 'account', summary: 'Sign in in the pane.' },
  checked: { date: '2026-10-28', version: '0.26.2', how: 'package_registry', windows_tested: false },
  state: 'ready',
  state_reason: 'Ready.',
  installed_version: '0.26.2',
  modes: ['Pane'],
  can_install: true,
  ambiguous_name: false,
  last_check: null,
  ...over,
});

const detail = (): StoreDetail => ({
  ...card(),
  install_plan: { agent_id: 'qwen', label: 'Qwen Code', available: true, reason: null, hint: null, steps: [] },
  run_preview: ['qwen'],
  fixtures: { count: 0 },
  uninstall: { available: false, steps: [], note: '' },
  found_path_display: null,
});

function fakeApi(): AgentStoreApi {
  return {
    list: vi.fn(async () => ({ catalog_version: 'abc12345', entries: [card()] })),
    detail: vi.fn(async () => detail()),
    add: vi.fn(),
    remove: vi.fn(async () => undefined),
    recordInstall: vi.fn(),
    uninstallPlan: vi.fn(),
    uninstallRun: vi.fn(),
    recordCheck: vi.fn(),
  };
}

async function mount(props: Partial<Parameters<typeof StoreHost>[0]> = {}) {
  const api = fakeApi();
  const onOpenAgent = vi.fn();
  const onBack = vi.fn();
  await act(async () => root.render(<StoreHost api={api} onOpenAgent={onOpenAgent} onBack={onBack} {...props} />));
  await flush();
  return { api, onOpenAgent, onBack };
}

describe('StoreHost', () => {
  it('shows the grid, and Back goes to the host', async () => {
    const { api, onBack } = await mount();
    expect(host.textContent).toContain('Agent Store');
    expect(host.textContent).toContain('Qwen Code');
    expect(api.list).toHaveBeenCalledTimes(1);
    await click(button('Back'));
    expect(onBack).toHaveBeenCalled();
  });

  it('a card opens the detail sheet, and Open hands the agent to the host and closes the sheet', async () => {
    const { api, onOpenAgent } = await mount();
    await click(button('Open Qwen Code'));
    await flush();
    expect(api.detail).toHaveBeenCalledWith('qwen');
    expect(sheet()).not.toBeNull();
    await click(inSheet('Open Qwen Code'));
    expect(onOpenAgent).toHaveBeenCalledWith('qwen');
    expect(sheet()).toBeNull();
  });

  it('a change made in the sheet loads the grid again', async () => {
    const { api } = await mount();
    await click(button('Open Qwen Code'));
    await flush();
    await click(inSheet('Remove from Puppet Master'));
    await click(inSheet('Remove from Puppet Master'));
    await flush();
    expect(api.remove).toHaveBeenCalledWith('qwen');
    expect(api.list).toHaveBeenCalledTimes(2);
  });

  it('has no "Check it works" button without a runner', async () => {
    await mount();
    await click(button('Open Qwen Code'));
    await flush();
    expect(inSheet('Check it works')).toBeUndefined();
  });

  it('a runner still gives no "Check it works" button to an agent that is not ready', async () => {
    const runCheck = vi.fn<RunStoreCheck>(async () => ({ status: 'done', passed: 7, total: 7, version: '0.26.2' }));
    const api = fakeApi();
    api.detail = vi.fn(async () => ({ ...detail(), state: 'available' as const, installed_version: null }));
    await mount({ api, runCheck });
    await click(button('Open Qwen Code'));
    await flush();
    expect(sheet()).not.toBeNull();
    expect(inSheet('Check it works')).toBeUndefined();
    expect(runCheck).not.toHaveBeenCalled();
  });

  it('with a runner, "Check it works" opens the Check sheet for that agent, and Close returns to the grid', async () => {
    const runCheck = vi.fn<RunStoreCheck>(async () => ({ status: 'done', passed: 7, total: 7, version: '0.26.2' }));
    const { api } = await mount({ runCheck });
    await click(button('Open Qwen Code'));
    await flush();
    await click(inSheet('Check it works'));
    await flush();
    expect(host.textContent).toContain('Qwen Code');
    expect(inSheet('Start')).toBeTruthy();
    expect(runCheck).not.toHaveBeenCalled();
    await click(inSheet('Start'));
    expect(runCheck).toHaveBeenCalledWith('qwen', expect.anything(), expect.any(Function));
    await flush();
    expect(sheet()?.textContent).toContain('7 of 7 checks passed on version 0.26.2.');
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(sheet()).toBeNull();
    expect(api.list).toHaveBeenCalledTimes(2);
  });
});
