// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import CockpitApp from './CockpitApp';

// Mutable per-test input for the digest (away) path.
const away = vi.hoisted(() => ({ digest: null as null | object }));

vi.mock('./hooks/useCockpit', async () => {
  const { useState } = await import('react');
  return {
    useCockpit: () => {
      const [selectedId, setSelectedId] = useState<string | null>('p1');
      const ids = ['p1', 'p2'];
      return {
        selectedId,
        select: setSelectedId,
        spawn: null,
        dismissSpawn: () => {},
        mode: 'orchestration',
        setMode: async () => {},
        newAgent: async () => {},
        notify: () => {},
        theme: 'dark',
        now: 0,
        toasts: [],
        names: {},
        registry: {
          paneList: ids.map((id) => ({ info: { id, cwd: `/proj/${id}` } })),
          panes: new Map(),
        },
      };
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
    c: { select: (id: string | null) => void; registry: { paneList: { info: { id: string } }[] } };
    onOpenDoctor: () => void;
    onOpenMemory: () => void;
  };
  return {
    Sidebar: ({ c, onOpenDoctor, onOpenMemory }: Props) =>
      h(
        'div',
        null,
        h('button', { type: 'button', onClick: onOpenDoctor }, 'Set up agents'),
        h('button', { type: 'button', onClick: onOpenMemory }, 'Project memory'),
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
vi.mock('./components/cockpit/PaneView', async () => {
  const { createElement: h } = await import('react');
  return { PaneView: ({ paneId }: { paneId: string }) => h('div', null, `PANE VIEW ${paneId}`) };
});
vi.mock('./components/cockpit/Inbox', async () => {
  const { createElement: h } = await import('react');
  return { Inbox: () => h('div', null, 'INBOX VIEW') };
});
vi.mock('./components/cockpit/AwayDigest', async () => {
  const { createElement: h } = await import('react');
  return {
    AwayDigest: ({ onOpenPane }: { onOpenPane: (id: string) => void }) =>
      h('button', { type: 'button', onClick: () => onOpenPane('p2') }, 'Open digest pane'),
  };
});
vi.mock('./components/cockpit/SpawnView', () => ({ SpawnView: () => null }));
vi.mock('./components/cockpit/Palette', () => ({ Palette: () => null }));
vi.mock('./components/cockpit/UndoSheet', () => ({ UndoSheet: () => null }));
vi.mock('./components/cockpit/PanicSheet', () => ({ PanicSheet: () => null }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  away.digest = null;
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
});
