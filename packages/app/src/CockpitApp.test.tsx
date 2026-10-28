// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import CockpitApp from './CockpitApp';
import { requestGoLive, type LiveStatus } from './lib/live-room';

// Mutable per-test input for the digest (away) path.
const away = vi.hoisted(() => ({ digest: null as null | object }));
// The spawn the (mocked) cockpit hook reports: null, running, or failed.
const ui = vi.hoisted(() => ({ spawn: null as null | { failed: string | null } }));
// The UI mode and the "open the Settings window" command (what Individual mode uses) the mocked cockpit reports.
const cockpit = vi.hoisted(() => ({
  mode: 'orchestration' as 'orchestration' | 'individual',
  openSettings: (async () => {}) as (tab?: string) => Promise<void>,
  // The pane list is known (the split restores on start only then). Off by default, as before.
  ready: false,
}));
// The Live Room the (mocked) app reports, and the toasts the cockpit shows.
const live = vi.hoisted(() => ({
  status: { live: false, paused: false, viewers: 0, endsAtMs: 0, paneIds: [] as string[], hidden: 0, stopped: null as string | null },
  listeners: [] as Array<(s: unknown) => void>,
  notes: [] as string[],
  stops: 0,
}));
// Rust's "show Settings in this window" event (cockpit://settings).
const rust = vi.hoisted(() => ({ openSettings: null as null | ((e: { tab?: string | null }) => void) }));

vi.mock('./hooks/useCockpit', async () => {
  const { useState } = await import('react');
  return {
    useCockpit: () => {
      const [selectedId, setSelectedId] = useState<string | null>('p1');
      const ids = ['p1', 'p2'];
      const infoOf = (id: string) => ({ id, cwd: `/proj/${id}`, agent_type: `agent-${id}` });
      return {
        selectedId,
        // Like the real hook: picking a pane keeps a running spawn and clears a failed one.
        select: (id: string | null) => {
          if (id && ui.spawn?.failed) ui.spawn = null;
          setSelectedId(id);
        },
        spawn: ui.spawn,
        dismissSpawn: () => {},
        mode: cockpit.mode,
        setMode: async () => {},
        openSettings: cockpit.openSettings,
        newAgent: async () => {},
        notify: (text: string) => void live.notes.push(text),
        theme: 'dark',
        now: 0,
        toasts: [],
        names: new Map<string, string>(),
        registry: {
          paneList: ids.map((id) => ({ info: infoOf(id) })),
          panes: new Map(ids.map((id) => [id, { info: infoOf(id), status: 'idle' }])),
          initialReady: cockpit.ready,
        },
      };
    },
  };
});

vi.mock('./lib/cockpit-api', async (importOriginal) => {
  const orig = await importOriginal<typeof import('./lib/cockpit-api')>();
  return {
    ...orig,
    cockpitApi: {
      ...orig.cockpitApi,
      onSettingsOpen: (cb: (e: { tab?: string | null }) => void) => {
        rust.openSettings = cb;
        return Promise.resolve(() => {});
      },
    },
  };
});

vi.mock('./lib/live-room', async (importOriginal) => {
  const orig = await importOriginal<typeof import('./lib/live-room')>();
  return {
    ...orig,
    onLiveState: (cb: (s: unknown) => void) => {
      live.listeners.push(cb);
      return Promise.resolve(() => {
        live.listeners = live.listeners.filter((x) => x !== cb);
      });
    },
    liveRoomApi: {
      ...orig.liveRoomApi,
      status: async () => live.status,
      stop: async () => void (live.stops += 1),
      configGet: async () => ({ relay: 'https://relay.example.test', minutes: 60, maxViewers: 10 }),
      preview: async () => ({ panes: [], items: [], hidden: 0 }),
    },
  };
});

vi.mock('./hooks/useAway', () => ({
  useAway: () => ({ digest: away.digest, dismissDigest: () => {}, lastTurn: null }),
}));

vi.mock('./hooks/useNeedsYou', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./hooks/useNeedsYou')>()),
  useNeedsYouSource: () => [],
}));

vi.mock('./lib/loop-guard', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/loop-guard')>()),
  useLoopSignals: () => ({ signals: {}, dismiss: () => {} }),
  tauriLoopApi: {
    onPanic: () => Promise.resolve(() => {}),
    stopAll: () => Promise.resolve({}),
  },
}));

vi.mock('./lib/desktop-nudge', () => ({ useNudgeOnAction: () => ({ onClickCapture: () => {} }) }));
vi.mock('./lib/agent-health', () => ({ markSpawned: () => {} }));

// Child views are stubbed so the test exercises only CockpitApp's view wiring.
vi.mock('./components/cockpit/Sidebar', async () => {
  const { createElement: h } = await import('react');
  type Props = {
    c: { select: (id: string | null) => void; openSettings: (tab?: string) => Promise<void>; registry: { paneList: { info: { id: string } }[] } };
    onOpenDoctor: () => void;
    onOpenMemory: () => void;
    onOpenToSide?: (id: string) => void;
  };
  return {
    Sidebar: ({ c, onOpenDoctor, onOpenMemory, onOpenToSide }: Props) =>
      h(
        'div',
        null,
        h('button', { type: 'button', onClick: onOpenDoctor }, 'Set up agents'),
        h('button', { type: 'button', onClick: () => onOpenToSide?.('p2') }, 'Open p2 to the side'),
        h('button', { type: 'button', onClick: onOpenMemory }, 'Project memory'),
        h('button', { type: 'button', onClick: () => void c.openSettings() }, 'Settings'),
        h('button', { type: 'button', onClick: () => c.select(null) }, 'Inbox'),
        ...c.registry.paneList.map((p) =>
          h('button', { key: p.info.id, type: 'button', onClick: () => c.select(p.info.id) }, `pane ${p.info.id}`),
        ),
      ),
  };
});
vi.mock('./components/cockpit/AgentDoctor', async () => {
  const { createElement: h } = await import('react');
  return { AgentDoctor: () => h('div', null, 'DOCTOR VIEW') };
});
vi.mock('./components/cockpit/ProjectMemory', async () => {
  const { createElement: h } = await import('react');
  return { ProjectMemory: () => h('div', null, 'MEMORY VIEW') };
});
// The split view's own rendering is tested in SplitView.test.tsx; here only its wiring from CockpitApp.
vi.mock('./components/cockpit/SplitView', async () => {
  const { createElement: h, useEffect, useState } = await import('react');
  type Props = { leftId: string; rightId: string; focus: string; ratio: number; onClose: () => void; onFocus: (side: string) => void; onRatio: (r: number) => void };
  return {
    useWindowWidth: () => {
      const [w, setW] = useState(() => window.innerWidth);
      useEffect(() => {
        const on = () => setW(window.innerWidth);
        window.addEventListener('resize', on);
        return () => window.removeEventListener('resize', on);
      }, []);
      return w;
    },
    SplitView: ({ leftId, rightId, focus, ratio, onClose, onFocus }: Props) =>
      h(
        'div',
        null,
        `SPLIT VIEW ${leftId}|${rightId}|${focus}|${ratio}`,
        h('button', { type: 'button', onClick: onClose }, 'Close split'),
        h('button', { type: 'button', onClick: () => onFocus('right') }, 'Focus right'),
      ),
    focusSideComposer: () => {},
  };
});
vi.mock('./components/cockpit/PaneView', async () => {
  const { createElement: h } = await import('react');
  return { PaneView: ({ paneId }: { paneId: string }) => h('div', null, `PANE VIEW ${paneId}`) };
});
vi.mock('./components/cockpit/Inbox', async () => {
  const { createElement: h } = await import('react');
  type Props = { c: { openSettings: (tab?: string) => Promise<void> } };
  return {
    Inbox: ({ c }: Props) =>
      h('div', null, 'INBOX VIEW', h('button', { type: 'button', onClick: () => void c.openSettings('preload') }, 'Inbox deep link')),
  };
});
let settingsMounts = 0;
vi.mock('./components/cockpit/SettingsView', async () => {
  const { createElement: h, useEffect } = await import('react');
  type Props = { initialTab?: string; onBack: () => void; onPageChange?: (page: string) => void };
  return {
    SettingsView: ({ initialTab, onBack, onPageChange }: Props) => {
      useEffect(() => {
        settingsMounts += 1;
        onPageChange?.(initialTab ?? 'agents');
      }, []); // eslint-disable-line react-hooks/exhaustive-deps
      return h(
        'div',
        null,
        'SETTINGS VIEW tab=' + initialTab + ';',
        h('button', { type: 'button', onClick: onBack }, 'Settings back'),
        h('button', { type: 'button', onClick: () => onPageChange?.('keys') }, 'Go to Keys'),
      );
    },
  };
});
vi.mock('./components/cockpit/AwayDigest', async () => {
  const { createElement: h } = await import('react');
  return {
    AwayDigest: ({ onOpenPane }: { onOpenPane: (id: string) => void }) =>
      h('button', { type: 'button', onClick: () => onOpenPane('p2') }, 'Open digest pane'),
  };
});
vi.mock('./components/cockpit/SpawnView', async () => {
  const { createElement: h } = await import('react');
  return { SpawnView: () => h('div', null, 'SPAWN VIEW') };
});
vi.mock('./components/cockpit/Palette', async () => {
  const { createElement: h } = await import('react');
  return { Palette: ({ initialMode }: { initialMode?: string }) => h('div', { 'data-testid': 'palette', 'data-mode': initialMode }) };
});
vi.mock('./components/cockpit/UndoSheet', () => ({ UndoSheet: () => null }));
vi.mock('./components/cockpit/PanicSheet', () => ({ PanicSheet: () => null }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(async () => {
  away.digest = null;
  ui.spawn = null;
  cockpit.mode = 'orchestration';
  cockpit.openSettings = vi.fn(async () => {});
  cockpit.ready = false;
  live.status = { live: false, paused: false, viewers: 0, endsAtMs: 0, paneIds: [], hidden: 0, stopped: null };
  live.listeners = [];
  live.notes = [];
  live.stops = 0;
  localStorage.clear();
  rust.openSettings = null;
  settingsMounts = 0;
  window.history.replaceState(null, '', '/');
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  // Async, so the Live Room status read at mount settles inside the act.
  await act(async () => root.render(createElement(CockpitApp)));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function clickButton(text: string) {
  const button = [...host.querySelectorAll('button')].find((b) => b.textContent === text);
  if (!button) throw new Error(`no button "${text}"`);
  act(() => button.click());
}
// The main view is the first child of <main>; the away digest, when shown, comes after it.
const mainText = () => host.querySelector('main')?.firstElementChild?.textContent;

describe('CockpitApp main view precedence', () => {
  it('shows the selected pane by default', () => {
    expect(mainText()).toBe('PANE VIEW p1');
  });

  it('(1) Project memory replaces the open Set up agents view', () => {
    clickButton('Set up agents');
    expect(mainText()).toBe('DOCTOR VIEW');
    clickButton('Project memory');
    expect(mainText()).toBe('MEMORY VIEW');
  });

  it('(2) clicking the already-selected pane leaves the Project memory view', () => {
    clickButton('Project memory');
    expect(mainText()).toBe('MEMORY VIEW');
    clickButton('pane p1');
    expect(mainText()).toBe('PANE VIEW p1');
  });

  it('(3) clicking a pane leaves the Set up agents view', () => {
    clickButton('Set up agents');
    expect(mainText()).toBe('DOCTOR VIEW');
    clickButton('pane p2');
    expect(mainText()).toBe('PANE VIEW p2');
  });

  it('(4) a pane opened from the away digest leaves the Set up agents view', () => {
    away.digest = {};
    act(() => root.render(createElement(CockpitApp)));
    clickButton('Set up agents');
    expect(mainText()).toBe('DOCTOR VIEW');
    clickButton('Open digest pane');
    expect(mainText()).toBe('PANE VIEW p2');
  });

  // The live bug "last click wins": whatever the person clicked last is what the main area shows.
  it('(5) the last click wins across every view', () => {
    clickButton('Project memory');
    clickButton('Set up agents');
    expect(mainText()).toBe('DOCTOR VIEW');
    clickButton('Project memory');
    expect(mainText()).toBe('MEMORY VIEW');
    clickButton('Inbox');
    expect(mainText()).toContain('INBOX VIEW');
    clickButton('Set up agents');
    expect(mainText()).toBe('DOCTOR VIEW');
    clickButton('pane p2');
    expect(mainText()).toBe('PANE VIEW p2');
    clickButton('Inbox');
    expect(mainText()).toContain('INBOX VIEW');
    clickButton('pane p1');
    expect(mainText()).toBe('PANE VIEW p1');
  });

  it('(6) an agent that is starting shows its spawn view; Set up agents and Project memory cover it', () => {
    ui.spawn = { failed: null };
    act(() => root.render(createElement(CockpitApp)));
    expect(mainText()).toBe('SPAWN VIEW');
    clickButton('Set up agents');
    expect(mainText()).toBe('DOCTOR VIEW');
    clickButton('Project memory');
    expect(mainText()).toBe('MEMORY VIEW');
  });

  it('(7) picking a pane keeps a running spawn in front, but a failed spawn gives way to the pane', () => {
    ui.spawn = { failed: null };
    act(() => root.render(createElement(CockpitApp)));
    clickButton('pane p2');
    expect(mainText()).toBe('SPAWN VIEW');

    ui.spawn = { failed: 'could not start' };
    act(() => root.render(createElement(CockpitApp)));
    clickButton('pane p1');
    expect(mainText()).toBe('PANE VIEW p1');
  });

  // Integration (r10): Settings is one more value of the same view state.
  it('(8) Settings covers a starting agent, and Back returns to its spawn view', () => {
    ui.spawn = { failed: null };
    act(() => root.render(createElement(CockpitApp)));
    expect(mainText()).toBe('SPAWN VIEW');
    clickButton('Settings');
    expect(mainText()).toContain('SETTINGS VIEW');
    clickButton('Settings back');
    expect(mainText()).toBe('SPAWN VIEW');
  });
});

describe('Settings in the cockpit', () => {
  const rerender = () => act(() => root.render(createElement(CockpitApp)));
  const openSettingsMock = () => cockpit.openSettings as unknown as ReturnType<typeof vi.fn>;
  const settingsTab = () => /SETTINGS VIEW tab=([a-z]+);/.exec(mainText() ?? '')?.[1];
  const fromRust = (e: { tab?: string }) => act(() => (rust.openSettings as (e: { tab?: string }) => void)(e));

  it('Orchestration: Settings opens inside the main area and the sidebar stays', () => {
    clickButton('Settings');
    expect(mainText()).toContain('SETTINGS VIEW');
    expect(host.querySelector('button')?.textContent).toBe('Set up agents'); // the sidebar is still there
    expect(openSettingsMock()).not.toHaveBeenCalled(); // no separate window
  });

  it('Individual: Settings is the separate window, never an inline view', () => {
    cockpit.mode = 'individual';
    rerender();
    clickButton('Settings');
    expect(openSettingsMock()).toHaveBeenCalledTimes(1);
    expect(openSettingsMock()).toHaveBeenCalledWith(undefined);
    expect(mainText()).not.toContain('SETTINGS VIEW');
  });

  it('a deep link opens the inline view on that section', () => {
    clickButton('pane p1');
    fromRust({ tab: 'bridge' });
    expect(settingsTab()).toBe('bridge');
  });

  it('a deep link from an Inbox card opens Settings on Agents (the old "preload" id means Agents)', () => {
    clickButton('Inbox');
    expect(mainText()).toContain('INBOX VIEW');
    clickButton('Inbox deep link');
    expect(settingsTab()).toBe('agents');
    clickButton('Settings back');
    expect(mainText()).toContain('INBOX VIEW');
  });

  it('a deep link while Settings is already open moves to that section; a plain open keeps the page', () => {
    clickButton('Settings');
    expect(settingsMounts).toBe(1);
    fromRust({ tab: 'keys' });
    expect(settingsTab()).toBe('keys');
    expect(settingsMounts).toBe(2);
    clickButton('Settings'); // same view, no tab: nothing reloads
    expect(settingsMounts).toBe(2);
    expect(settingsTab()).toBe('keys');
  });

  it('Back returns to the view that was showing before', () => {
    clickButton('Set up agents');
    clickButton('Settings');
    expect(mainText()).toContain('SETTINGS VIEW');
    clickButton('Settings back');
    expect(mainText()).toBe('DOCTOR VIEW');
  });

  it('Back from the default view returns to the selected pane', () => {
    clickButton('Settings');
    clickButton('Settings back');
    expect(mainText()).toBe('PANE VIEW p1');
  });

  it('the last click wins: a pane, Project memory or Set up agents replaces Settings, and Settings replaces them', () => {
    clickButton('Settings');
    clickButton('pane p2');
    expect(mainText()).toBe('PANE VIEW p2');
    clickButton('Settings');
    clickButton('Project memory');
    expect(mainText()).toBe('MEMORY VIEW');
    clickButton('Settings');
    expect(mainText()).toContain('SETTINGS VIEW');
    clickButton('Set up agents');
    expect(mainText()).toBe('DOCTOR VIEW');
  });

  it('switching to Individual with Settings open moves it to the Settings window, on the same page', () => {
    clickButton('Settings');
    clickButton('Go to Keys');
    cockpit.mode = 'individual';
    rerender();
    expect(openSettingsMock()).toHaveBeenCalledTimes(1);
    expect(openSettingsMock()).toHaveBeenCalledWith('keys');
    // Back in Orchestration the cockpit is not stuck on a stale Settings view (the window has it now).
    cockpit.mode = 'orchestration';
    rerender();
    expect(mainText()).toBe('PANE VIEW p1');
    expect(host.textContent).not.toContain('SETTINGS VIEW');
  });

  it('switching to Individual with Settings closed opens no Settings window', () => {
    cockpit.mode = 'individual';
    rerender();
    expect(openSettingsMock()).not.toHaveBeenCalled();
  });

  it('a cockpit window created for Settings (?view=settings&tab=) opens on it, after the view resets', async () => {
    act(() => root.unmount());
    window.history.replaceState(null, '', '/?cockpit=1&view=settings&tab=bridge');
    root = createRoot(host);
    await act(async () => root.render(createElement(CockpitApp)));
    // Effects flush when act ends; the link opens on the next tick.
    await act(async () => new Promise((r) => setTimeout(r, 20)));
    expect(settingsTab()).toBe('bridge');
  });
});

describe('CockpitApp palette shortcuts', () => {
  const press = (init: KeyboardEventInit) => act(() => void window.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init })));
  const palette = () => host.querySelector<HTMLElement>('[data-testid="palette"]');

  it('Ctrl+K opens the commands and a second Ctrl+K closes them', () => {
    expect(palette()).toBeNull();
    press({ key: 'k', ctrlKey: true });
    expect(palette()?.dataset.mode).toBe('commands');
    press({ key: 'k', ctrlKey: true });
    expect(palette()).toBeNull();
  });

  it('Ctrl+Shift+F opens the palette in Search all chats, also when it is open on the commands', () => {
    press({ key: 'F', ctrlKey: true, shiftKey: true });
    expect(palette()?.dataset.mode).toBe('chats');
    press({ key: 'k', ctrlKey: true });
    press({ key: 'k', ctrlKey: true });
    press({ key: 'f', metaKey: true, shiftKey: true });
    expect(palette()?.dataset.mode).toBe('chats');
  });

  it('Ctrl+F and Ctrl+Alt+Shift+F do nothing', () => {
    press({ key: 'f', ctrlKey: true });
    press({ key: 'f', ctrlKey: true, shiftKey: true, altKey: true });
    expect(palette()).toBeNull();
  });

  it('? and Ctrl+/ open the keyboard shortcuts sheet, and typing ? in a text box does not', () => {
    const sheet = () => host.querySelector<HTMLElement>('.ck-sheet--shortcuts');
    expect(sheet()).toBeNull();
    press({ key: '?' });
    expect(sheet()).not.toBeNull();
    act(() => void document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })));
    expect(sheet()).toBeNull();
    press({ key: '/', ctrlKey: true });
    expect(sheet()).not.toBeNull();
    act(() => void document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })));
    const field = document.createElement('input');
    document.body.appendChild(field);
    act(() => void field.dispatchEvent(new KeyboardEvent('keydown', { key: '?', bubbles: true, cancelable: true })));
    expect(sheet()).toBeNull();
    field.remove();
  });
});

describe('CockpitApp split view wiring', () => {
  const ctrlBackslash = () => window.dispatchEvent(new KeyboardEvent('keydown', { key: '\\', ctrlKey: true, bubbles: true }));
  const resize = (width: number) => {
    window.innerWidth = width;
    act(() => window.dispatchEvent(new Event('resize')));
  };
  afterEach(() => resize(1024));

  it('Ctrl+\\ splits with the pane below the selected one (the first pane has none above), and again closes it', () => {
    expect(mainText()).toBe('PANE VIEW p1');
    act(() => ctrlBackslash());
    expect(mainText()).toContain('SPLIT VIEW p1|p2|left');
    act(() => ctrlBackslash());
    expect(mainText()).toBe('PANE VIEW p1');
  });

  it('"Open to the side" puts that pane on the right, and a click on the right side moves the focus', () => {
    clickButton('Open p2 to the side');
    expect(mainText()).toContain('SPLIT VIEW p1|p2|left');
    clickButton('Focus right');
    expect(mainText()).toContain('SPLIT VIEW p1|p2|right');
  });

  it('picking the right pane in the sidebar swaps the sides; the inbox ends the split', () => {
    clickButton('Open p2 to the side');
    clickButton('pane p2');
    expect(mainText()).toContain('SPLIT VIEW p2|p1|left');
    clickButton('Inbox');
    expect(mainText()).not.toContain('SPLIT VIEW');
    expect(mainText()).toContain('INBOX VIEW');
  });

  it('below 900px the shortcut does nothing, and an open split turns off when the window gets narrow', () => {
    resize(800);
    act(() => ctrlBackslash());
    expect(mainText()).toBe('PANE VIEW p1');
    resize(1024);
    act(() => ctrlBackslash());
    expect(mainText()).toContain('SPLIT VIEW');
    resize(800);
    expect(mainText()).toBe('PANE VIEW p1');
  });
});

// The segment controls of the Go live sheet measure themselves with a ResizeObserver, which jsdom does not have.
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

describe('CockpitApp Live Room wiring', () => {
  const flush = () =>
    act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
  /** The app tells every window the room changed (`live://state`). */
  const emit = (over: Partial<LiveStatus>) =>
    act(async () => {
      const next = { ...live.status, ...over };
      live.status = next;
      live.listeners.forEach((cb) => cb(next));
    });
  const bar = () => host.querySelector('.ck-live-bar');
  const running = (over: Partial<LiveStatus> = {}): Partial<LiveStatus> => ({ live: true, viewers: 7, endsAtMs: Date.now() + 41 * 60_000, paneIds: ['p1'], stopped: null, ...over });

  it('shows no LIVE bar while there is no room', async () => {
    await flush();
    expect(bar()).toBeNull();
  });

  it('the LIVE bar appears on a live://state event and goes on stop, with the reason toast', async () => {
    await flush();
    await emit(running());
    expect(bar()?.textContent).toContain('LIVE');
    expect(bar()?.textContent).toContain('7 watching');
    expect(bar()?.textContent).not.toMatch(/https?:|#/);
    expect(live.notes).toEqual([]);
    await emit({ live: false, paused: false, stopped: 'expired' });
    expect(bar()).toBeNull();
    expect(live.notes).toEqual(['Live Room ended: time is up.']);
  });

  it('says each stop in plain English, and nothing when the app quits', async () => {
    await flush();
    await emit(running());
    await emit({ live: false, stopped: 'idle' });
    await emit(running());
    await emit({ live: false, stopped: 'app_quit' });
    await emit(running());
    await emit({ live: false, stopped: 'owner' });
    expect(live.notes).toEqual(['Live Room stopped: nothing happened for 20 minutes.', 'Live Room stopped.']);
  });

  it('a stopped status seen at start (no room before) says nothing', async () => {
    await flush();
    await emit({ live: false, stopped: 'expired' });
    expect(live.notes).toEqual([]);
  });

  it('Stop in the LIVE bar ends the room through the app', async () => {
    await flush();
    await emit(running());
    await act(async () => clickButton('Stop'));
    expect(live.stops).toBe(1);
  });

  it('the Go live event (chat menu, palette) opens the sheet with that pane checked', async () => {
    await flush();
    act(() => requestGoLive('p2'));
    await flush();
    const sheet = document.querySelector('[role="dialog"]');
    expect(sheet?.textContent).toContain('Go live');
    const boxes = [...(sheet?.querySelectorAll<HTMLInputElement>('input[type="checkbox"]') ?? [])];
    expect(boxes.map((b) => b.checked)).toEqual([false, true]);
  });

  it('the Go live event with no pane checks the selected pane', async () => {
    await flush();
    act(() => requestGoLive(null));
    await flush();
    const boxes = [...document.querySelectorAll<HTMLInputElement>('[role="dialog"] input[type="checkbox"]')];
    expect(boxes.map((b) => b.checked)).toEqual([true, false]);
  });

  it('while a room runs, the Go live event opens nothing and says why', async () => {
    await flush();
    await emit(running());
    act(() => requestGoLive('p1'));
    await flush();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(live.notes).toEqual(['Live Room is already running. Stop it first.']);
  });
});
