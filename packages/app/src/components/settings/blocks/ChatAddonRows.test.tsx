// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpAddonApi, AddonList, AddonStatus } from '../../../lib/acpAddon';
import { SECTIONS } from '../catalog';
import { visibleSections } from '../model';
import { ChatAddonRows } from './ChatAddonRows';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const status = (over: Partial<AddonStatus> = {}): AddonStatus => ({
  agent: 'claude',
  label: 'Claude',
  state: 'not_installed',
  version: null,
  pinned_version: '0.74.0',
  detail: '',
  size_mb: 48,
  lines: [],
  signin_hint: '',
  ...over,
});

function rig(initial: AddonList) {
  let push: ((s: AddonStatus) => void) | null = null;
  const api: AcpAddonApi = {
    list: vi.fn(async () => initial),
    install: vi.fn(async (id: string) => status({ agent: id, state: 'installing' })),
    remove: vi.fn(async (id: string) => status({ agent: id, state: 'not_installed' })),
    watch: async (cb) => {
      push = cb;
      return () => {
        push = null;
      };
    },
  };
  return { api, emit: (s: AddonStatus) => act(async () => push?.(s)) };
}

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

async function mount(api: AcpAddonApi) {
  await act(async () => root.render(<ChatAddonRows api={api} />));
  await act(async () => {});
}
const button = (label: RegExp) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => label.test(b.getAttribute('aria-label') ?? b.textContent ?? ''));

describe('Settings > Agents > Chat add-ons', () => {
  it('shows nothing while Chat is off in Labs', async () => {
    await mount(rig({ labsEnabled: false, addons: [status()] }).api);
    expect(host.textContent).toBe('');
  });

  it('shows nothing outside the desktop app (no add-ons to list)', async () => {
    await mount(rig({ labsEnabled: true, addons: [] }).api);
    expect(host.textContent).toBe('');
  });

  it('an installed add-on shows its version and Remove', async () => {
    const r = rig({ labsEnabled: true, addons: [status({ state: 'installed', version: '0.74.0' }), status({ agent: 'codex', label: 'Codex', size_mb: 16 })] });
    await mount(r.api);
    expect(host.querySelector('[data-testid="addon-status-claude"]')?.textContent).toBe('Installed 0.74.0');
    expect(host.textContent).toContain('Claude chat add-on');
    expect(host.textContent).not.toContain('Claude Code');
    expect(button(/Remove the Claude chat add-on/)).toBeTruthy();
    expect(host.querySelector('[data-testid="addon-status-codex"]')?.textContent).toBe('Not installed');
    expect(host.textContent).toContain('A small download (about 16 MB).');
    await act(async () => button(/Remove the Claude/)!.click());
    expect(r.api.remove).toHaveBeenCalledWith('claude');
  });

  it('Install starts the install and the row follows the progress events', async () => {
    const r = rig({ labsEnabled: true, addons: [status()] });
    await mount(r.api);
    await act(async () => button(/Install the Claude chat add-on/)!.click());
    expect(r.api.install).toHaveBeenCalledWith('claude');
    expect(host.querySelector('[data-testid="addon-status-claude"]')?.textContent).toContain('Installing');
    expect(button(/Install the Claude/)).toBeUndefined();

    await r.emit(status({ state: 'installing', lines: ['Downloading @agentclientprotocol/claude-agent-acp 0.74.0…'] }));
    expect(host.querySelector('[data-testid="addon-status-claude"]')?.textContent).toContain('Downloading @agentclientprotocol');

    await r.emit(status({ state: 'installed', version: '0.74.0' }));
    expect(host.querySelector('[data-testid="addon-status-claude"]')?.textContent).toBe('Installed 0.74.0');
    expect(button(/Remove the Claude/)).toBeTruthy();
  });

  it('a failed install says why and offers to try again', async () => {
    const r = rig({ labsEnabled: true, addons: [status()] });
    (r.api.install as ReturnType<typeof vi.fn>).mockRejectedValueOnce('ACP_ADDON_FAILED: npm could not install the add-on (exit code 1).');
    await mount(r.api);
    await act(async () => button(/Install the Claude/)!.click());
    expect(host.querySelector('[data-testid="addon-status-claude"]')?.textContent).toBe('npm could not install the add-on (exit code 1).');
    expect(button(/Try again/)).toBeTruthy();
  });

  it('a removal that cannot happen keeps the add-on and says why', async () => {
    const r = rig({ labsEnabled: true, addons: [status({ state: 'installed', version: '0.74.0' })] });
    (r.api.remove as ReturnType<typeof vi.fn>).mockRejectedValueOnce('ACP_ADDON_FAILED: The add-on could not be removed: access denied. If a chat with this agent is open, close it first');
    await mount(r.api);
    await act(async () => button(/Remove the Claude/)!.click());
    expect(host.querySelector('[data-testid="addon-status-claude"]')?.textContent).toBe('Installed 0.74.0');
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('If a chat with this agent is open, close it first');
  });

  it('the Settings row exists only while Chat (ACP) is on in Labs', () => {
    const rowIds = (settings: { acp_chat_enabled?: boolean } | null) => (visibleSections(SECTIONS, settings).find((x) => x.id === 'agents')?.rows ?? []).map((r) => r.id);
    expect(rowIds(null)).not.toContain('chat-addons');
    expect(rowIds({ acp_chat_enabled: false })).not.toContain('chat-addons');
    expect(rowIds({ acp_chat_enabled: true })).toContain('chat-addons');
  });
});
