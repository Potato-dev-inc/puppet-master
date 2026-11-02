// @vitest-environment jsdom
// My computers page: the states (skeleton, error, empty, labs off), the screen hops switch, the find ring, the hop
// mode, and the left bar row (shown only while the Labs switch is on). No mock data is shipped; these fixtures are test-only.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DeskSnapshot } from '../../lib/desk';

const h = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Map<string, (e: { payload: unknown }) => void>(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: h.invoke }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
    h.listeners.set(name, cb);
    return () => h.listeners.delete(name);
  }),
}));

import { DeskPage } from './DeskPage';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { ShipApi } from '../../lib/ship-api';
import { Sidebar } from '../cockpit/Sidebar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const HOTKEYS = {
  config: { next: 'ctrl+alt+right', prev: 'ctrl+alt+left', screen_n: 'ctrl+alt+{n}', find_pointer: 'ctrl+alt+f', pause: 'scrolllock', center: 'ctrl+alt+c', lock_pointer: 'ctrl+alt+l', jump_n: 'ctrl+alt+shift+{n}', return_home: 'ctrl+alt+shift+h' },
  defaults: { next: 'ctrl+alt+right', prev: 'ctrl+alt+left', screen_n: 'ctrl+alt+{n}', find_pointer: 'ctrl+alt+f', pause: 'scrolllock', center: 'ctrl+alt+c', lock_pointer: 'ctrl+alt+l', jump_n: 'ctrl+alt+shift+{n}', return_home: 'ctrl+alt+shift+h' },
  bindings: [
    { action: 'next', keys: 'ctrl+alt+right' },
    { action: 'prev', keys: 'ctrl+alt+left' },
    { action: 'find', keys: 'ctrl+alt+f' },
    { action: 'pause', keys: 'scrolllock' },
  ],
};

/** Test-only fixture. Lives in this test file, never in lib/desk.ts. */
function snap(over: Partial<DeskSnapshot> = {}, status: Partial<DeskSnapshot['status']> = {}): DeskSnapshot {
  return {
    seq: 1,
    status: { state: 'on', hops_enabled: true, paused: false, hooks_installed: true, pointer_locked: false, hook_reinstalls: 0, error: null, ...status },
    screens: [
      { id: 'a', name: 'A', x: 0, y: 0, width: 2560, height: 1440, scale: 1, primary: true, index: 1 },
      { id: 'b', name: 'B', x: 2560, y: 0, width: 1920, height: 1080, scale: 1, primary: false, index: 2 },
    ],
    pointer: null,
    hop_mode: 'relative',
    hotkeys: HOTKEYS,
    find: null,
    ...over,
  };
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  h.invoke.mockReset();
  h.listeners.clear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

const flush = () => act(async () => {});
const button = (text: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === text);

describe('DeskPage', () => {
  it('shows a skeleton while the first snapshot is on its way', async () => {
    h.invoke.mockReturnValue(new Promise(() => {}));
    await act(async () => root.render(<DeskPage />));
    expect(host.querySelector('h2')?.textContent).toBe('My computers');
    expect(host.querySelector('[aria-label="Loading your screens"]')).not.toBeNull();
  });

  it('shows an honest error with Try again, and retries', async () => {
    h.invoke.mockRejectedValueOnce('access denied');
    await act(async () => root.render(<DeskPage />));
    await flush();
    expect(host.textContent).toContain("Couldn't read your screens: access denied");
    h.invoke.mockResolvedValueOnce(snap());
    await act(async () => button('Try again')?.click());
    await flush();
    expect(host.textContent).toContain('On · 2 screens');
    expect(h.invoke).toHaveBeenCalledWith('desk_snapshot');
  });

  it('lists paired computers, turns a switch on, and shows the question from another computer', async () => {
    /** Test-only fixture. */
    const view = {
      hosting: null,
      pending: { id: 3, role: 'host', name: 'Laptop', code: '4F7Q-9K2M', expires_at: 1 },
      joining: null,
      message: null,
      locked_until: null,
      tampered: false,
      devices: [{ node: 'ab'.repeat(32), name: 'Tower', added_at: 1, online: null, allowed: [] }],
    };
    h.invoke.mockImplementation(async (name: string, args?: Record<string, unknown>) => {
      if (name === 'desk_snapshot') return snap();
      if (name === 'pair_view') return view;
      if (name === 'trust_set_permission') return { ...view, pending: null };
      if (name === 'pair_answer') return { ...view, pending: null };
      return undefined;
    });
    await act(async () => root.render(<DeskPage />));
    await flush();
    expect(host.textContent).toContain('Tower');
    expect(document.body.textContent).toContain('Laptop wants to pair');
    await act(async () => [...document.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === 'No')!.click());
    expect(h.invoke).toHaveBeenCalledWith('pair_answer', { id: 3, allow: false });
    await flush();
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Files for Tower"]')!.click());
    expect(h.invoke).toHaveBeenCalledWith('trust_set_permission', { node: 'ab'.repeat(32), capability: 'files', on: true });
  });

  it('follows the link: online dot on the row, "Mouse is on Laptop" in the status line, and back', async () => {
    const node = 'cd'.repeat(32);
    /** Test-only fixtures. */
    const pairView = {
      hosting: null,
      pending: null,
      joining: null,
      message: null,
      locked_until: null,
      tampered: false,
      devices: [{ node, name: 'Laptop', added_at: 1, online: null, allowed: ['desk/input'] }],
    };
    const linkDevice = { node, name: 'Laptop', state: 'online', side: 'right', mouse_here: false, controls_me: false, input_on: true, clipboard_on: false };
    const linkView = (over: Record<string, unknown> = {}) => ({
      seq: 1,
      devices: [linkDevice],
      mouse_on: null,
      controlled_by: null,
      paused: false,
      sharing: true,
      tray: 'Mouse sharing is on',
      admin_note: "Can't control windows that run as admin.",
      error: null,
      clipboard_note: null,
      supported: true,
      ...over,
    });
    h.invoke.mockImplementation(async (name: string) => {
      if (name === 'desk_snapshot') return snap();
      if (name === 'pair_view') return pairView;
      if (name === 'desk_link_view') return linkView();
      if (name === 'desk_link_set_side') return linkView({ devices: [{ ...linkDevice, side: 'left' }] });
      return undefined;
    });
    await act(async () => root.render(<DeskPage />));
    await flush();
    const line = () => host.querySelector('[data-testid="link-status"]')?.textContent ?? '';
    expect(line()).toContain('Mouse sharing is on · Laptop connected');
    expect(host.querySelector('.ck-desk-device-status')?.textContent).toBe('Connected');
    expect(host.textContent).toContain("Can't control windows that run as admin.");

    // The desktop app pushes the new state when the pointer crosses.
    await act(async () => h.listeners.get('desk://link')!({ payload: linkView({ mouse_on: 'Laptop', devices: [{ ...linkDevice, mouse_here: true }] }) }));
    expect(line()).toContain('Mouse is on Laptop');
    expect(host.querySelector('.ck-desk-device-status')?.textContent).toBe('Connected · Mouse is here');
    expect(button('Bring the mouse back')).toBeDefined();

    // And when the cable goes: the row says Offline, the line says it is still on.
    await act(async () => h.listeners.get('desk://link')!({ payload: linkView({ devices: [{ ...linkDevice, state: 'offline' }] }) }));
    expect(host.querySelector('.ck-desk-device-status')?.textContent).toBe('Offline');
    expect(line()).toContain('Mouse sharing is on');
    expect(line()).not.toContain('connected');

    // The person says where the laptop sits.
    const select = host.querySelector<HTMLSelectElement>('select[aria-label="Where Laptop sits"]')!;
    await act(async () => {
      select.value = 'left';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(h.invoke).toHaveBeenCalledWith('desk_link_set_side', { node, side: 'left' });
    await act(async () => button('Pause sharing')!.click());
    expect(h.invoke).toHaveBeenCalledWith('desk_link_pause', { paused: true });
  });

  it('draws nothing while the Labs switch is off', async () => {
    h.invoke.mockRejectedValue('FEATURE_OFF: My computers is off in Labs');
    await act(async () => root.render(<DeskPage />));
    await flush();
    expect(host.querySelector('h2')).toBeNull();
    expect(host.textContent).toBe('');
  });

  it('shows the status, the switch and the screens from the engine', async () => {
    h.invoke.mockResolvedValue(snap());
    await act(async () => root.render(<DeskPage />));
    await flush();
    expect(host.textContent).toContain('On · 2 screens');
    expect(host.textContent).toContain('Move your mouse to another screen with a shortcut.');
    expect(host.textContent).toContain('Screen 1 · 2560×1440 · main');
    expect(host.textContent).toContain('Your computers');
    expect(host.textContent).not.toContain('Coming next');
    expect(host.textContent).toContain('Only this computer is here. Add another to share one mouse.');
  });

  it('the switch calls set_hops_enabled with the opposite value', async () => {
    h.invoke.mockResolvedValue(snap({}, { hops_enabled: false, state: 'off' }));
    await act(async () => root.render(<DeskPage />));
    await flush();
    const sw = host.querySelector<HTMLButtonElement>('[role="switch"]')!;
    expect(sw.getAttribute('aria-checked')).toBe('false');
    h.invoke.mockResolvedValueOnce(snap({}, { hops_enabled: true, state: 'on' }));
    await act(async () => sw.click());
    expect(h.invoke).toHaveBeenLastCalledWith('desk_command', { cmd: { type: 'set_hops_enabled', enabled: true } });
  });

  it('clicking a screen calls hop_to with its number', async () => {
    h.invoke.mockResolvedValue(snap());
    await act(async () => root.render(<DeskPage />));
    await flush();
    h.invoke.mockResolvedValueOnce(snap());
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Go to screen 2"]')!.click());
    expect(h.invoke).toHaveBeenLastCalledWith('desk_command', { cmd: { type: 'hop_to', screen: 2 } });
  });

  it('shows a plain line when a command fails', async () => {
    h.invoke.mockResolvedValue(snap());
    await act(async () => root.render(<DeskPage />));
    await flush();
    h.invoke.mockRejectedValueOnce('hook busy');
    await act(async () => button('Pause')!.click());
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('Could not do that: hook busy');
  });

  it('a pushed snapshot updates the page, and an older one does not replace a newer one', async () => {
    h.invoke.mockResolvedValue(snap({ seq: 5 }));
    await act(async () => root.render(<DeskPage />));
    await flush();
    await act(async () => h.listeners.get('desk://snapshot')?.({ payload: snap({ seq: 6 }, { paused: true, state: 'paused' }) }));
    expect(host.textContent).toContain('Paused');
    await act(async () => h.listeners.get('desk://snapshot')?.({ payload: snap({ seq: 4 }) }));
    expect(host.textContent).toContain('Paused');
  });

  it('the Pause button sends pause with the opposite value', async () => {
    h.invoke.mockResolvedValue(snap());
    await act(async () => root.render(<DeskPage />));
    await flush();
    h.invoke.mockResolvedValueOnce(snap({}, { paused: true, state: 'paused' }));
    await act(async () => button('Pause')!.click());
    expect(h.invoke).toHaveBeenLastCalledWith('desk_command', { cmd: { type: 'pause', paused: true } });
  });

  it('the hop mode choice calls set_hop_mode', async () => {
    h.invoke.mockResolvedValue(snap());
    await act(async () => root.render(<DeskPage />));
    await flush();
    h.invoke.mockResolvedValueOnce(snap({ hop_mode: 'center' }));
    const center = host.querySelector<HTMLInputElement>('input[value="center"]')!;
    await act(async () => center.click());
    expect(h.invoke).toHaveBeenLastCalledWith('desk_command', { cmd: { type: 'set_hop_mode', mode: 'center' } });
  });
});

function fakeApi(): CockpitApi {
  return {
    registry: { paneList: [], panes: new Map(), initialReady: true },
    names: new Map(),
    asked: { questions: [] },
    launchEntries: [],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: null,
    select: vi.fn(),
    prompts: {},
    statusSince: new Map(),
    now: 60_000,
    spawn: null,
    toasts: [],
    notify: vi.fn(),
    openSettings: vi.fn(async () => {}),
    bridgeLine: '',
  } as unknown as CockpitApi;
}
const ship = { recent: vi.fn().mockResolvedValue([]), gitState: vi.fn().mockResolvedValue('repo') } as unknown as ShipApi;
const deskRow = () => [...host.querySelectorAll<HTMLButtonElement>('button.ck-row')].find((b) => b.textContent?.includes('My computers'));

describe('My computers left bar row', () => {
  it('is hidden while the Labs switch is off', async () => {
    await act(async () => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} ship={ship} desk={{ enabled: false }} onOpenDesk={() => {}} />));
    expect(deskRow()).toBeUndefined();
  });

  it('is shown when the Labs switch is on, and opens the page', async () => {
    const onOpenDesk = vi.fn();
    await act(async () => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} ship={ship} desk={{ enabled: true }} deskOpen onOpenDesk={onOpenDesk} />));
    const row = deskRow();
    expect(row).toBeDefined();
    expect(row?.getAttribute('aria-current')).toBe('true');
    await act(async () => row?.click());
    expect(onOpenDesk).toHaveBeenCalledTimes(1);
  });
});
