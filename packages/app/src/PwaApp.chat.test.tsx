// @vitest-environment jsdom
// The phone's Terminal | Chat switch in the PWA root: the terminal stays mounted (hidden) in the chat, nothing in the
// chat claims the pane's size, the choice is kept per pane, and a shell has no Chat.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({
  claim: vi.fn(),
  terminalMounts: 0,
  terminalUnmounts: 0,
  paneChat: vi.fn(),
  panes: [] as unknown[],
  caps: ['watch', 'answer', 'type', 'control'],
}));

vi.mock('./hooks/usePaneViewportLease', () => ({
  VIEWER_ID: 'v-test',
  usePaneViewportLease: () => h.claim,
}));
vi.mock('./hooks/useVisualViewport', () => ({ useVisualViewportSync: () => undefined }));
vi.mock('./hooks/usePushIntents', () => ({ usePushIntents: () => undefined }));
vi.mock('./lib/bridge-sse', () => ({
  subscribeBridgeEventsHandle: () => ({
    unsubscribe: () => undefined,
    stats: () => null,
    onStats: () => () => undefined,
    setWatchedPanes: () => undefined,
    sendInput: () => false,
  }),
}));
vi.mock('./lib/bridge', () => ({
  makeBridgeClient: () => ({
    baseUrl: 'http://phone.test',
    listPanes: async () => h.panes,
    getSettings: async () => ({}),
    listAgentTypes: async () => ({}),
    listPreviews: async () => ({}),
    attention: async () => ({ v: 1, rev: 1, counts: { blocking: 0, ready: 0 }, items: [] }),
    pairMe: async () => ({ local: false, caps: h.caps }),
    paneChat: h.paneChat,
  }),
}));
vi.mock('./hooks/useBridgePaneRegistry', async () => {
  const React = await import('react');
  return {
    useBridgePaneRegistry: () => {
      const [list, setList] = React.useState<unknown[]>([]);
      const panes = React.useMemo(
        () => new Map((list as Array<{ id: string; status: string }>).map((info) => [info.id, { info, status: info.status }])),
        [list],
      );
      return {
        panes,
        paneList: [],
        subscribePaneData: () => () => undefined,
        ingestPaneResize: () => undefined,
        ingestTerminalData: () => undefined,
        updatePaneStatus: () => undefined,
        updatePaneDimensions: () => undefined,
        setPanesFromList: setList,
        resetStreams: () => undefined,
        makeTransport: () => ({ writeInput: () => true }),
      };
    },
  };
});
vi.mock('./components/BridgePaneTerminal', async () => {
  const React = await import('react');
  return {
    useBridgePaneTransport: (make: (id: string) => unknown, id: string) => make(id),
    BridgePaneTerminal: ({ pane }: { pane: { id: string } }) => {
      React.useEffect(() => {
        h.terminalMounts += 1;
        return () => {
          h.terminalUnmounts += 1;
        };
      }, []);
      return <div data-testid="terminal" data-pane={pane.id} />;
    },
  };
});

import PwaApp from './PwaApp';
import { fallbackListing } from './lib/agentRegistry';
import { LS_PANE_VIEWS } from './lib/mobile-pane-view';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const listing = fallbackListing();
const SHELL = listing.agents.find((agent) => agent.kind === 'shell')!.id;
const AGENT = listing.agents.find((agent) => agent.kind !== 'shell')!.id;
const pane = (id: string, agent: string, created_at: number) => ({
  id,
  agent_type: agent,
  status: 'idle',
  created_at,
  cwd: 'C:/x',
  pid: 1,
  last_output_at: null,
  cols: 80,
  rows: 24,
});

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  localStorage.clear();
  localStorage.setItem('pm.bridge.url', 'http://phone.test');
  h.claim.mockClear();
  h.terminalMounts = 0;
  h.terminalUnmounts = 0;
  h.caps = ['watch', 'answer', 'type', 'control'];
  h.panes = [pane('a1', AGENT, 1), pane('s1', SHELL, 2)];
  h.paneChat.mockReset();
  h.paneChat.mockResolvedValue({ available: true, reason: null, source: 'file', waitingFor: null, rev: 1, reset: false, items: [] });
  vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true })));
  vi.stubGlobal('CSS', { escape: (value: string) => value });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

async function open(tab: string) {
  localStorage.setItem('pm.mobile.activeTab', tab);
  await act(async () => {
    root.render(<PwaApp />);
    await vi.advanceTimersByTimeAsync(50);
  });
}

const buttonNamed = (name: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent === name);
const flush = (ms = 50) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

describe('PwaApp Terminal | Chat', () => {
  it('shows the switch on an agent tab, with the terminal first and no chat asked for', async () => {
    await open('a1');
    expect(host.querySelector('[aria-label="Pane view"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="terminal"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="phone-chat"]')).toBeNull();
    expect(h.paneChat).not.toHaveBeenCalled();
  });

  it('has no switch on a shell tab', async () => {
    await open('s1');
    expect(host.querySelector('[data-testid="terminal"]')).not.toBeNull();
    expect(host.querySelector('[aria-label="Pane view"]')).toBeNull();
  });

  it('opens the chat over the terminal, which stays mounted and hidden, and claims no size', async () => {
    await open('a1');
    const claimsBefore = h.claim.mock.calls.length;
    await act(async () => buttonNamed('Chat')!.click());
    await flush();
    expect(host.querySelector('[data-testid="phone-chat"]')).not.toBeNull();
    expect(h.paneChat).toHaveBeenCalledWith('a1', undefined);
    const terminal = host.querySelector('[data-testid="terminal"]')!;
    expect(terminal).not.toBeNull();
    expect(terminal.parentElement!.className).toContain('invisible');
    expect(h.terminalUnmounts).toBe(0);

    // Taps and scrolls in the chat, and coming back to the app, take nothing from the PC window.
    const main = host.querySelector('main')!;
    await act(async () => {
      main.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 5, clientY: 5 }));
      main.dispatchEvent(new MouseEvent('pointerup', { bubbles: true, clientX: 5, clientY: 5 }));
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await flush(2000);
    expect(h.claim.mock.calls.length).toBe(claimsBefore);
  });

  it('claims the size again only when the terminal is picked', async () => {
    await open('a1');
    await act(async () => buttonNamed('Chat')!.click());
    await flush();
    const before = h.claim.mock.calls.length;
    await act(async () => buttonNamed('Terminal')!.click());
    await flush();
    expect(host.querySelector('[data-testid="phone-chat"]')).toBeNull();
    expect(host.querySelector('[data-testid="terminal"]')!.parentElement!.className).not.toContain('invisible');
    // The terminal is the same one (never remounted); a tap on it claims as before.
    expect(h.terminalMounts).toBe(1);
    expect(h.terminalUnmounts).toBe(0);
    const main = host.querySelector('main')!;
    await act(async () => {
      main.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 5, clientY: 5 }));
    });
    expect(h.claim.mock.calls.length).toBeGreaterThan(before);
  });

  it('remembers the choice per pane across a reload', async () => {
    await open('a1');
    await act(async () => buttonNamed('Chat')!.click());
    await flush();
    expect(JSON.parse(localStorage.getItem(LS_PANE_VIEWS)!)).toEqual({ a1: 'chat' });
    act(() => root.unmount());
    root = createRoot(host);
    await open('a1');
    expect(host.querySelector('[data-testid="phone-chat"]')).not.toBeNull();
  });

  it('a phone without watch gets the plain line and asks for nothing', async () => {
    h.caps = ['answer'];
    await open('a1');
    await act(async () => buttonNamed('Chat')!.click());
    await flush(3000);
    expect(host.textContent).toContain('This phone cannot read chats.');
    expect(h.paneChat).not.toHaveBeenCalled();
  });
});
