// @vitest-environment jsdom
// The New agent page, Chat chosen: the add-on prompt (install, progress, then Send works) and the sign-in card.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpAddonApi, AddonList, AddonStatus } from '../../../lib/acpAddon';
import type { AgentDescriptor } from '../../../lib/agentRegistry';
import type { RecentProject } from '../../../lib/ship-api';
import type { NewAgentApi } from './api';
import type { LaneView, StartRequest } from './model';
import { NewAgentPage, type NewAgentPageProps } from './NewAgentPage';
import type { StartBlock } from './start';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const agent = (id: string, label: string): AgentDescriptor => ({ id, label, kind: 'cli', installed: true, modes: { headless: true, visible: true } }) as unknown as AgentDescriptor;
const project: RecentProject = { path: '/work/alpha-app', name: 'alpha-app', state: 'repo', last_used_ms: 5 };
const lanes: LaneView = { labsEnabled: true, agents: [{ id: 'claude', lane: 'terminal', disabled: false }] };

const status = (over: Partial<AddonStatus> = {}): AddonStatus => ({
  agent: 'claude',
  label: 'Claude',
  state: 'not_installed',
  version: null,
  pinned_version: '0.74.0',
  detail: '',
  size_mb: 48,
  lines: [],
  signin_hint: 'In the terminal that opens, type /login.',
  ...over,
});

function pageApi(): NewAgentApi {
  return {
    recent: async () => [project],
    pickFolder: async () => null,
    pickPaths: async () => [],
    defaultFolder: async () => null,
    gitState: async () => 'repo',
    lanes: async () => lanes,
    accounts: async () => [],
    loadHome: async () => ({ name: '', cards: [] }),
    saveCards: async () => undefined,
    onHomeChanged: async () => () => undefined,
  };
}

function addonRig(initial: AddonList) {
  let push: ((s: AddonStatus) => void) | null = null;
  const api: AcpAddonApi = {
    list: vi.fn(async () => initial),
    install: vi.fn(async (id: string) => status({ agent: id, state: 'installing' })),
    remove: vi.fn(async (id: string) => status({ agent: id })),
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
  try {
    localStorage.clear();
    sessionStorage.clear();
  } catch {
    /* none */
  }
  host = document.createElement('div');
  host.className = 'cockpit';
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

async function mount(addonApi: AcpAddonApi, props: Partial<NewAgentPageProps> = {}) {
  const onStart = vi.fn(async (_request: StartRequest): Promise<unknown> => undefined);
  const onSignIn = vi.fn(async () => undefined);
  await act(async () => {
    root.render(<NewAgentPage agents={[agent('claude', 'Claude')]} panes={[]} needs={[]} api={pageApi()} addonApi={addonApi} onStart={onStart} onSignIn={onSignIn} onOpenPane={vi.fn()} {...props} />);
  });
  await act(async () => {});
  return { onStart, onSignIn };
}

const input = () => host.querySelector<HTMLTextAreaElement>('textarea[aria-label="What should the agent do?"]')!;
const sendButton = () => host.querySelector<HTMLButtonElement>('button[aria-label="Send"]')!;
const chooseChat = () => act(async () => host.querySelector<HTMLButtonElement>('[role=radio]:not([aria-checked=true])')!.click());
const button = (text: string | RegExp) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => (typeof text === 'string' ? b.textContent === text : text.test(b.textContent ?? '')));
async function type(value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(input(), value);
    input().dispatchEvent(new Event('input', { bubbles: true }));
  });
}

describe('Chat needs the add-on', () => {
  it('asks to install it only when Chat is chosen, with the size, and Send waits', async () => {
    const r = addonRig({ labsEnabled: true, addons: [status()] });
    await mount(r.api);
    expect(host.querySelector('[data-testid="addon-prompt"]')).toBeNull();
    await chooseChat();
    expect(host.querySelector('[data-testid="addon-prompt"]')?.textContent).toContain('Chat needs a small add-on for Claude (about 48 MB).');
    await type('fix the tests');
    expect(sendButton().disabled).toBe(true);
    expect(sendButton().title).toBe('Install the chat add-on first.');
  });

  it('Install shows progress, and once it is installed the prompt goes and Send starts the chat', async () => {
    const r = addonRig({ labsEnabled: true, addons: [status()] });
    const { onStart } = await mount(r.api);
    await chooseChat();
    await type('fix the tests');
    await act(async () => button('Install')!.click());
    expect(r.api.install).toHaveBeenCalledWith('claude');
    expect(host.textContent).toContain('Installing the chat add-on for Claude…');

    await r.emit(status({ state: 'installing', lines: ['npm http fetch GET 200 https://registry.npmjs.org/x'] }));
    expect(host.querySelector('[data-testid="addon-progress"]')?.textContent).toContain('registry.npmjs.org');
    expect(sendButton().disabled).toBe(true);

    await r.emit(status({ state: 'installed', version: '0.74.0' }));
    expect(host.querySelector('[data-testid="addon-prompt"]')).toBeNull();
    expect(sendButton().disabled).toBe(false);
    await act(async () => sendButton().click());
    expect(onStart).toHaveBeenCalledWith(expect.objectContaining({ agentId: 'claude', runAs: 'chat', prompt: 'fix the tests' }));
  });

  it('a failed install says why and can be tried again', async () => {
    const r = addonRig({ labsEnabled: true, addons: [status()] });
    (r.api.install as ReturnType<typeof vi.fn>).mockRejectedValueOnce('ACP_ADDON_FAILED: Node.js is needed to install the chat add-on.');
    await mount(r.api);
    await chooseChat();
    await act(async () => button('Install')!.click());
    expect(host.querySelector('[data-testid="addon-error"]')?.textContent).toBe('Node.js is needed to install the chat add-on.');
    expect(button('Try again')).toBeTruthy();
  });

  it('Terminal mode never asks, and an installed add-on is not mentioned', async () => {
    const r = addonRig({ labsEnabled: true, addons: [status({ state: 'installed', version: '0.74.0' })] });
    await mount(r.api);
    await chooseChat();
    expect(host.querySelector('[data-testid="addon-prompt"]')).toBeNull();
    await type('go');
    expect(sendButton().disabled).toBe(false);
  });

  it('a chat start that says the add-on is missing refreshes the state instead of showing an error', async () => {
    const r = addonRig({ labsEnabled: true, addons: [status({ state: 'installed', version: '0.74.0' })] });
    const block: StartBlock = { blocked: 'addon', agentId: 'claude', message: 'ACP_ADDON_MISSING: …' };
    const { onStart } = await mount(r.api, { onStart: vi.fn(async () => block) });
    void onStart;
    await chooseChat();
    await type('go');
    (r.api.list as ReturnType<typeof vi.fn>).mockResolvedValue({ labsEnabled: true, addons: [status()] });
    await act(async () => sendButton().click());
    await act(async () => {});
    expect(host.querySelector('[data-testid="addon-prompt"]')).not.toBeNull();
    expect(host.querySelector('[role="alert"]')).toBeNull();
  });
});

describe('Chat needs a sign-in', () => {
  it('shows the sign-in card, opens the agent terminal, and tries again when the person says so', async () => {
    const r = addonRig({ labsEnabled: true, addons: [status({ state: 'installed', version: '0.74.0' })] });
    const block: StartBlock = { blocked: 'signin', agentId: 'claude', message: 'ACP_SIGN_IN_NEEDED: The agent needs you to sign in first.' };
    const onStart = vi.fn(async (_request: StartRequest): Promise<unknown> => block);
    const { onSignIn } = await mount(r.api, { onStart });
    await chooseChat();
    await type('fix the tests');
    await act(async () => sendButton().click());
    const card = host.querySelector('[data-testid="signin-card"]')!;
    expect(card.textContent).toContain('Sign in to Claude to use Chat');
    expect(card.textContent).toContain('type /login');

    await act(async () => button('Open sign-in')!.click());
    expect(onSignIn).toHaveBeenCalledWith('claude', '/work/alpha-app');

    // Signed in: the same message goes again, and a chat that starts clears the card.
    onStart.mockResolvedValueOnce('pane-9');
    await act(async () => button(/signed in — try again/)!.click());
    expect(onStart).toHaveBeenCalledTimes(2);
    expect(onStart.mock.calls[1][0]).toMatchObject({ agentId: 'claude', runAs: 'chat', prompt: 'fix the tests' });
    expect(host.querySelector('[data-testid="signin-card"]')).toBeNull();
  });

  it('keeps the message, agent, folder and Chat choice for the trip to the sign-in terminal', async () => {
    const r = addonRig({ labsEnabled: true, addons: [status({ state: 'installed', version: '0.74.0' })] });
    const block: StartBlock = { blocked: 'signin', agentId: 'claude', message: 'ACP_SIGN_IN_NEEDED: …' };
    await mount(r.api, { onStart: vi.fn(async () => block) });
    await chooseChat();
    await type('fix the tests');
    await act(async () => sendButton().click());
    await act(async () => button('Open sign-in')!.click());

    // The page was replaced by the terminal; coming back mounts it again.
    await act(async () => root.unmount());
    root = createRoot(host);
    await mount(r.api, { onStart: vi.fn(async () => 'pane-1') });
    expect(input().value).toBe('fix the tests');
    expect(host.querySelector('[data-testid="signin-card"]')).not.toBeNull();
    expect(host.querySelector('[aria-label^="Folder"]')?.textContent).toContain('alpha-app');
    // The saved trip is used once.
    expect(sessionStorage.getItem('pm.newagent.signin')).toBeNull();
  });
});
