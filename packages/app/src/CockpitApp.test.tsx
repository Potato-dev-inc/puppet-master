// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import CockpitApp from './CockpitApp';

// Mutable per-test input for the digest (away) path.
const away = vi.hoisted(() => ({ digest: null as null | object }));
// The spawn the (mocked) cockpit hook reports: null, running, or failed.
const ui = vi.hoisted(() => ({ spawn: null as null | { failed: string | null } }));
// The UI mode and the "open the Settings window" command (what Individual mode uses) the mocked cockpit reports.
const cockpit = vi.hoisted(() => ({
  mode: 'orchestration' as 'orchestration' | 'individual',
  openSettings: (async () => {}) as (tab?: string) => Promise<void>,
}));
// Rust's "show Settings in this window" event (cockpit://settings).
const rust = vi.hoisted(() => ({ openSettings: null as null | ((e: { tab?: string | null }) => void) }));

vi.mock('./hooks/useCockpit', async () => {
  const { useState } = await import('react');
  return {
    useCockpit: () => {
      const [selectedId, setSelectedId] = useState<string | null>('p1');
      const ids = ['p1', 'p2'];
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
        notify: () => {},
        theme: 'dark',
        now: 0,
        toasts: [],
        names: {},
        registry: {
          paneList: ids.map((id) => ({ info: { id, cwd: `/proj/${id}` } })),
          panes: new Map(ids.map((id) => [id, { info: { id, cwd: `/proj/${id}` }, status: 'idle' }])),
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
  type Props = { leftId: string; rightId: string; focus: string; onClose: () => void; onFocus: (side: string) => void };
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
    SplitView: ({ leftId, rightId, focus, onClose, onFocus }: Props) =>
      h(
        'div',
        null,
        `SPLIT VIEW ${leftId}|${rightId}|${focus}`,
        h('button', { type: 'button', onClick: onClose }, 'Close split'),
        h('button', { type: 'button', onClick: () => onFocus('right') }, 'Focus right'),
      ),
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
vi.mock('./components/cockpit/Palette', () => ({ Palette: () => null }));
vi.mock('./components/cockpit/UndoSheet', () => ({ UndoSheet: () => null }));
vi.mock('./components/cockpit/PanicSheet', () => ({ PanicSheet: () => null }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  away.digest = null;
  ui.spawn = null;
  cockpit.mode = 'orchestration';
  cockpit.openSettings = vi.fn(async () => {});
  rust.openSettings = null;
  settingsMounts = 0;
  window.history.replaceState(null, '', '/');
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => root.render(createElement(CockpitApp)));
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
