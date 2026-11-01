// @vitest-environment jsdom
// A chat session over ACP on the phone: the pane opens on its chat (there is no terminal and no Terminal | Chat switch),
// nothing claims a size for it, the agent's permission question shows with the agent's own options, and a Stop button
// shows while a turn runs and stops it through the PC.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({
  claim: vi.fn(),
  terminalMounts: 0,
  terminalUnmounts: 0,
  paneChat: vi.fn(),
  acpSession: vi.fn(),
  acpCancel: vi.fn(),
  questions: [] as unknown[],
  answerQuestion: vi.fn(),
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
    acpSession: h.acpSession,
    acpCancel: h.acpCancel,
    listQuestions: async () => h.questions,
    answerQuestion: h.answerQuestion,
    dismissQuestion: vi.fn(),
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

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const pane = (id: string, lane?: 'acp') => ({
  id,
  agent_type: 'codex',
  status: 'idle',
  created_at: 1,
  cwd: 'C:/x',
  pid: lane ? 0 : 1,
  last_output_at: null,
  cols: 80,
  rows: 24,
  ...(lane ? { lane } : {}),
});

const state = (over: Record<string, unknown> = {}) => ({
  lane: 'acp',
  agent: 'codex',
  label: 'Codex',
  phase: 'idle',
  status: 'idle',
  turnInFlight: false,
  stopping: false,
  canSteer: false,
  turn: 1,
  title: null,
  usage: null,
  permissions: [],
  closed: false,
  ...over,
});

const question = {
  id: 'q1',
  question: 'Codex asks permission: Write notes.txt',
  choices: [
    { id: 'allow_once', label: 'Allow Once' },
    { id: 'allow_always', label: 'Allow for Session' },
    { id: 'reject_once', label: 'Reject' },
  ],
  allow_free_text: false,
  urgency: 'high',
  created_at_ms: Date.now(),
  expires_at_ms: Date.now() + 3_600_000,
  state: 'pending',
  kind: 'acp_permission',
  pane_id: 'c1',
};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  localStorage.clear();
  localStorage.setItem('pm.bridge.url', 'http://phone.test');
  h.claim.mockClear();
  h.terminalMounts = 0;
  h.caps = ['watch', 'answer', 'type', 'control'];
  h.panes = [pane('c1', 'acp'), pane('t1')];
  h.questions = [];
  h.paneChat.mockReset();
  h.paneChat.mockResolvedValue({ available: true, reason: null, source: 'acp', waitingFor: null, rev: 1, reset: false, items: [] });
  h.acpSession.mockReset();
  h.acpSession.mockResolvedValue(state());
  h.acpCancel.mockReset();
  h.acpCancel.mockResolvedValue({ stopped: true });
  h.answerQuestion.mockReset();
  h.answerQuestion.mockResolvedValue({ accepted: true });
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

describe('PwaApp with a chat session over ACP', () => {
  it('opens on the chat: no terminal, no Terminal | Chat switch, and no size claimed for it', async () => {
    await open('c1');
    await flush();
    expect(host.querySelector('[data-testid="phone-chat"]')).not.toBeNull();
    expect(h.paneChat).toHaveBeenCalledWith('c1', undefined);
    expect(host.querySelector('[data-testid="terminal"]')).toBeNull();
    expect(host.querySelector('[aria-label="Pane view"]')).toBeNull();
    expect(h.terminalMounts).toBe(0);
  });

  it('an ordinary pane keeps its terminal and its switch, and the PC is not asked about a chat session for it', async () => {
    await open('t1');
    await flush(3000);
    expect(host.querySelector('[data-testid="terminal"]')).not.toBeNull();
    expect(host.querySelector('[aria-label="Pane view"]')).not.toBeNull();
    expect(h.acpSession).not.toHaveBeenCalled();
  });

  it("shows the agent's question with every option by its own name, and sends the chosen one", async () => {
    h.questions = [question, { ...question, id: 'other', pane_id: 't1' }];
    h.acpSession.mockResolvedValue(state({ phase: 'needs_input', status: 'waiting_input', turnInFlight: true }));
    await open('c1');
    await flush();
    const dock = host.querySelector('[data-testid="acp-phone-dock"]')!;
    expect(dock.textContent).toContain('Codex asks permission: Write notes.txt');
    expect(dock.querySelectorAll('[data-question-id]').length).toBe(1);
    for (const name of ['Allow Once', 'Allow for Session', 'Reject']) expect(buttonNamed(name)).toBeTruthy();
    await act(async () => buttonNamed('Allow for Session')!.click());
    expect(h.answerQuestion).toHaveBeenCalledWith('q1', { choice_id: 'allow_always' });
  });

  it('shows a Stop button while a turn runs, stops through the PC, and reads Stopping… at once', async () => {
    h.acpSession.mockResolvedValue(state({ phase: 'working', status: 'running', turnInFlight: true }));
    await open('c1');
    await flush();
    expect(host.querySelector('[data-testid="acp-phone-strip"]')?.textContent).toContain('Working…');
    await act(async () => buttonNamed('Stop')!.click());
    expect(h.acpCancel).toHaveBeenCalledWith('c1');
    expect(host.querySelector('[data-testid="acp-phone-strip"]')?.textContent).toContain('Stopping…');
    // The turn ends: the line goes away.
    h.acpSession.mockResolvedValue(state());
    await flush(2500);
    expect(host.querySelector('[data-testid="acp-phone-strip"]')).toBeNull();
  });

  it('has no Stop button without the type ability, and nothing at all once the agent is idle', async () => {
    h.caps = ['watch', 'answer'];
    h.acpSession.mockResolvedValue(state({ phase: 'working', status: 'running', turnInFlight: true }));
    await open('c1');
    await flush();
    expect(host.querySelector('[data-testid="acp-phone-strip"]')).not.toBeNull();
    expect(buttonNamed('Stop')).toBeUndefined();
    h.acpSession.mockResolvedValue(state());
    await flush(2500);
    expect(host.querySelector('[data-testid="acp-phone-dock"]')).toBeNull();
  });
});
