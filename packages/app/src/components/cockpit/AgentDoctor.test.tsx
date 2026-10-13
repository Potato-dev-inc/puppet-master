// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentHealth, AgentHealthApi } from '../../lib/agent-health';
import { AgentDoctor, glyphFor, stateLabel } from './AgentDoctor';
import { Inbox } from './Inbox';
import type { CockpitApi } from '../../hooks/useCockpit';

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
  window.localStorage.clear();
});

const row = (over: Partial<AgentHealth>): AgentHealth => ({
  id: 'a',
  label: 'Agent A',
  state: 'unchecked',
  installed: true,
  version: null,
  reason: 'Installed. Press Test to check it can answer.',
  install_hint: null,
  login_hint: null,
  can_test: true,
  ...over,
});

const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });

async function mount(api: AgentHealthApi, extra: Partial<Parameters<typeof AgentDoctor>[0]> = {}) {
  await act(async () => {
    root.render(<AgentDoctor api={api} onOpenAgent={() => {}} {...extra} />);
  });
  await flush();
}

const rowOf = (id: string) => host.querySelectorAll('.ck-doctor-row')[id === 'first' ? 0 : 1] as HTMLElement;
const button = (scope: ParentNode, text: string) =>
  Array.from(scope.querySelectorAll('button')).find((b) => b.textContent === text) as HTMLButtonElement | undefined;

describe('AgentDoctor', () => {
  it('maps states to plain labels and DotSpinner glyphs', () => {
    expect(glyphFor('unchecked')).toBe('idle');
    expect(glyphFor('not_installed')).toBe('idle');
    expect(glyphFor('ready')).toBe('exited');
    expect(glyphFor('needs_login')).toBe('waiting_input');
    expect(glyphFor('error')).toBe('error');
    expect(stateLabel('needs_login')).toBe('Needs login');
    expect(stateLabel('unchecked')).toBe('Not tested');
  });

  it('lists agents with status, version and reason; install command only when missing', async () => {
    const api: AgentHealthApi = {
      check: async () => [
        row({ id: 'a', label: 'Agent A', version: '1.2.3' }),
        row({
          id: 'b',
          label: 'Agent B',
          state: 'not_installed',
          installed: false,
          can_test: false,
          reason: 'Not installed on this computer.',
          install_hint: 'npm i -g agent-b',
        }),
      ],
      test: vi.fn(),
    };
    await mount(api);
    const a = rowOf('first');
    expect(a.textContent).toContain('Agent A');
    expect(a.textContent).toContain('Not tested');
    expect(a.textContent).toContain('1.2.3');
    expect(a.querySelector('[aria-label="Idle"]')).not.toBeNull();
    expect(button(a, 'Copy install command')).toBeUndefined();
    expect(button(a, 'Test')).toBeDefined();
    const b = rowOf('second');
    expect(b.textContent).toContain('Not installed');
    expect(b.querySelector('.ck-doctor-cmd')?.textContent).toBe('npm i -g agent-b');
    expect(button(b, 'Copy install command')).toBeDefined();
    expect(button(b, 'Test')).toBeUndefined();
    expect(button(b, 'Open to log in')).toBeUndefined();
  });

  it('copies the install command', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const notify = vi.fn();
    const api: AgentHealthApi = {
      check: async () => [row({ state: 'not_installed', installed: false, can_test: false, install_hint: 'npm i -g x' })],
      test: vi.fn(),
    };
    await mount(api, { onNotify: notify });
    await act(async () => button(host, 'Copy install command')!.click());
    await flush();
    expect(writeText).toHaveBeenCalledWith('npm i -g x');
    expect(notify).toHaveBeenCalledWith(expect.stringContaining('Copied'));
  });

  it('Test runs only on click, shows progress, then the result', async () => {
    let finish!: (h: AgentHealth) => void;
    const test = vi.fn(() => new Promise<AgentHealth>((res) => (finish = res)));
    const api: AgentHealthApi = { check: async () => [row({})], test };
    await mount(api);
    expect(test).not.toHaveBeenCalled(); // never automatic
    await act(async () => button(host, 'Test')!.click());
    expect(test).toHaveBeenCalledWith('a');
    expect(host.textContent).toContain('Testing');
    expect(button(host, 'Test')!.disabled).toBe(true);
    await act(async () => finish(row({ state: 'ready', reason: 'Ready. It answered the test question.' })));
    await flush();
    expect(host.textContent).toContain('Ready');
    expect(host.querySelector('[aria-label="Exited"]')).not.toBeNull(); // the check glyph
  });

  it('shows the login hint and opens the agent so the user can log in', async () => {
    const onOpenAgent = vi.fn();
    const api: AgentHealthApi = {
      check: async () => [
        row({ state: 'needs_login', reason: 'It needs you to sign in first.', login_hint: 'Run: agent login' }),
      ],
      test: vi.fn(),
    };
    await mount(api, { onOpenAgent });
    expect(host.textContent).toContain('Run: agent login');
    expect(host.querySelector('[aria-label="Needs you"]')).not.toBeNull();
    await act(async () => button(host, 'Open to log in')!.click());
    expect(onOpenAgent).toHaveBeenCalledWith('a');
  });

  it('turns a failed test call into a plain problem row', async () => {
    const api: AgentHealthApi = { check: async () => [row({})], test: async () => { throw new Error('boom'); } };
    await mount(api);
    await act(async () => button(host, 'Test')!.click());
    await flush();
    expect(host.textContent).toContain('Problem');
    expect(host.textContent).toContain('boom');
  });

  it('reports a failed check in plain words', async () => {
    await mount({ check: async () => { throw new Error('no shell'); }, test: vi.fn() });
    expect(host.textContent).toContain('Could not check your agents: no shell');
  });
});

describe('Inbox first-run hint', () => {
  const cockpit = (paneCount: number, ready = true) =>
    ({
      registry: { paneList: Array.from({ length: paneCount }, () => ({ status: 'idle', info: { id: 'x', agent_type: 'a', cwd: '' } })), initialReady: ready },
      prompts: {},
      statusSince: new Map(),
      names: new Map(),
      now: 0,
      answer: vi.fn(),
      select: vi.fn(),
    }) as unknown as CockpitApi;

  it('invites first-time users to set up agents', () => {
    const open = vi.fn();
    act(() => root.render(<Inbox c={cockpit(0)} onOpenDoctor={open} />));
    expect(host.textContent).toContain('Welcome to Puppet Master');
    act(() => button(host, 'Set up your agents')!.click());
    expect(open).toHaveBeenCalled();
  });

  it('stays a quiet button after something was ever spawned, and hides when panes exist', () => {
    window.localStorage.setItem('pm.cockpit.everSpawned', '1');
    act(() => root.render(<Inbox c={cockpit(0)} onOpenDoctor={() => {}} />));
    expect(host.textContent).not.toContain('Welcome');
    expect(button(host, 'Set up your agents')).toBeDefined();
    act(() => root.render(<Inbox c={cockpit(1)} onOpenDoctor={() => {}} />));
    expect(button(host, 'Set up your agents')).toBeUndefined();
  });
});
