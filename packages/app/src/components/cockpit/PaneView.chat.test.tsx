// @vitest-environment jsdom
// The Chat tab of a pane: shown for an agent pane (its session file), not for a plain shell, and it opens
// the read-only chat over that session file.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { ChatItem } from '../../lib/sessionChat';
import { LoopGuardContext, type LoopSignal } from '../../lib/loop-guard';
import { PaneView } from './PaneView';

const invoke = vi.fn(async (cmd: string) => (cmd === 'checkpoints_list' ? { state: 'ok', message: '', items: [] } : null) as unknown);
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...(a as [string])) }));
vi.mock('../TerminalViewport', () => ({ TerminalViewport: () => <div data-testid="term" /> }));
vi.mock('../../lib/sessionChat', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/sessionChat')>()),
  openSessionChat: vi.fn(async () => ({ available: true, reason: null, items: SAMPLE })),
  closeSessionChat: vi.fn(async () => undefined),
  onSessionChatItems: vi.fn(async () => () => undefined),
  onSessionChatStatus: vi.fn(async () => () => undefined),
}));

import { openSessionChat } from '../../lib/sessionChat';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as { ResizeObserver?: unknown }).ResizeObserver = NoopResizeObserver;
Element.prototype.scrollIntoView = () => {};

const SAMPLE: ChatItem[] = [
  { kind: 'user', id: '10:0', at: null, text: 'List the files' },
  { kind: 'assistant', id: '20:0', at: null, text: 'Two entries.', model: null, finalAnswer: true },
];

function cockpit(agentType: string): CockpitApi {
  const p = { status: 'idle', info: { id: 'p1', agent_type: agentType, status: 'idle', pid: 4120, created_at: 1, last_output_at: null, cwd: 'C:\\work\\shop', cols: 158, rows: 56 } };
  return {
    registry: { paneList: [p], panes: new Map([['p1', p]]), initialReady: true },
    names: new Map([['p1', 'Agent']]),
    activities: {},
    previews: {},
    asked: { questions: [] },
    prompts: {},
    statusSince: new Map(),
    now: 12_000,
    selectedId: 'p1',
    mode: 'orchestration',
    select: vi.fn(),
    notify: vi.fn(),
    answer: vi.fn(),
    closePane: vi.fn(),
  } as unknown as CockpitApi;
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockImplementation(async (cmd: string) => (cmd === 'checkpoints_list' ? { state: 'ok', message: '', items: [] } : null));
  vi.mocked(openSessionChat).mockClear();
  host = document.createElement('div');
  host.className = 'cockpit';
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (window as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__;
});

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const tabs = () => [...host.querySelectorAll<HTMLElement>('[role="tab"]')].map((t) => t.textContent);
const tab = (name: string) => [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find((t) => t.textContent === name)!;

describe('pane Chat tab', () => {
  it('is shown for an agent pane, right after Terminal', async () => {
    await act(async () => root.render(<PaneView c={cockpit('claude')} paneId="p1" />));
    await flush();
    const names = tabs();
    expect(names.indexOf('Chat')).toBe(names.indexOf('Terminal') + 1);
  });

  it('is not shown for a plain shell pane', async () => {
    await act(async () => root.render(<PaneView c={cockpit('powershell')} paneId="p1" />));
    await flush();
    expect(tabs()).not.toContain('Chat');
    expect(tabs()).toContain('Terminal');
  });

  it('renders the chat panel when clicked, and reads the pane session file', async () => {
    await act(async () => root.render(<PaneView c={cockpit('claude')} paneId="p1" />));
    await flush();
    expect(host.querySelector('[data-testid="session-chat"]')).toBeNull();

    await act(async () => tab('Chat').click());
    await flush();
    expect(tab('Chat').getAttribute('aria-selected')).toBe('true');
    expect(openSessionChat).toHaveBeenCalledWith('p1');
    const chat = host.querySelector('.ck-pane-body [data-testid="session-chat"]');
    expect(chat).not.toBeNull();
    expect(chat!.textContent).toContain('List the files');
    expect(chat!.textContent).toContain('Two entries.');
  });

  it('falls back to Terminal when the pane becomes a plain shell while Chat is open', async () => {
    const c = cockpit('claude');
    await act(async () => root.render(<PaneView c={c} paneId="p1" />));
    await flush();
    await act(async () => tab('Chat').click());
    await flush();
    expect(host.querySelector('[data-testid="session-chat"]')).not.toBeNull();

    const shell = cockpit('powershell');
    await act(async () => root.render(<PaneView c={shell} paneId="p1" />));
    await flush();
    expect(tabs()).not.toContain('Chat');
    expect(tab('Terminal').getAttribute('aria-selected')).toBe('true');
    expect(host.querySelector('[data-testid="session-chat"]')).toBeNull();
  });
});

// A question waits on the person: the same prompt the Terminal tab's dock tests use.
const question = {
  promptId: 'q1',
  kind: 'permission',
  text: 'Run npm test?',
  summary: 'Run `npm test`',
  choices: [
    { id: 'yes', label: 'Yes' },
    { id: 'no', label: 'No' },
  ],
  risk: { level: 'low', headline: 'Run tests', reasons: ['It runs your tests.'] },
};
const stuck: LoopSignal = { pane_id: 'p1', kind: 'repeated_error', evidence_lines: ['cargo build failed'], turns: 4, since: 1, summary: 'the same error 4 times' };

/** The agent's pane waiting on the person, with the given prompt (null: nothing to answer). */
function waitingCockpit(prompt: unknown): CockpitApi {
  const c = cockpit('claude');
  const info = { ...c.registry.panes.get('p1')!.info, status: 'waiting_input' };
  const p = { status: 'waiting_input', info };
  return {
    ...c,
    registry: { paneList: [p], panes: new Map([['p1', p]]), initialReady: true },
    prompts: prompt ? { p1: prompt } : {},
  } as unknown as CockpitApi;
}

const renderPane = (c: CockpitApi, signals: Record<string, LoopSignal> = {}) =>
  act(async () => {
    root.render(
      <LoopGuardContext.Provider value={{ signals, dismiss: () => {} }}>
        <PaneView c={c} paneId="p1" />
      </LoopGuardContext.Provider>,
    );
  });
const dockOnChat = () => host.querySelector<HTMLElement>('.ck-pane-body .ck-chat-dock');
const buttonIn = (el: ParentNode, text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === text)!;

describe('the docked question in the Chat tab', () => {
  it('shows the Terminal tab card under the conversation, and its Allow goes to the same handler', async () => {
    const c = waitingCockpit(question);
    await renderPane(c);
    await flush();
    await act(async () => tab('Chat').click());
    await flush();
    const dock = dockOnChat()!;
    expect(dock).not.toBeNull();
    // outside the scrolling conversation, so it stays in view while the chat scrolls
    expect(host.querySelector('.ck-chat-body')!.contains(dock)).toBe(false);
    expect(dock.querySelector('[data-ib-card]')!.getAttribute('data-kind')).toBe('approval');
    await act(async () => buttonIn(dock, 'Allow once').click());
    // the answer carries the prompt, the choice and the person's click (the gesture has its own id and time)
    const answer = () => vi.mocked(c.answer).mock.calls.at(-1)!;
    expect(answer().slice(0, 4)).toEqual(['p1', 'q1', 'yes', 'Yes']);
    expect(answer()[4]).toEqual(expect.objectContaining({ kind: 'click' }));

    // the same card on the Terminal tab answers the same way
    const fromChat = answer().slice(0, 4);
    await act(async () => tab('Terminal').click());
    await flush();
    await act(async () => buttonIn(host.querySelector('.ck-dock')!, 'Allow once').click());
    expect(answer().slice(0, 4)).toEqual(fromChat);
    expect(answer()[4]).toEqual(expect.objectContaining({ kind: 'click' }));
  });

  it('takes the card away once the question is answered, and the person stays on Chat', async () => {
    await renderPane(waitingCockpit(question));
    await flush();
    await act(async () => tab('Chat').click());
    await flush();
    expect(dockOnChat()).not.toBeNull();
    await renderPane(waitingCockpit(null));
    await flush();
    expect(dockOnChat()).toBeNull();
    expect(tab('Chat').getAttribute('aria-selected')).toBe('true');
    expect(host.querySelector('[data-testid="session-chat"]')).not.toBeNull();
  });

  it('shows the stuck card on Chat', async () => {
    await renderPane(waitingCockpit(null), { p1: stuck });
    await flush();
    await act(async () => tab('Chat').click());
    await flush();
    expect(dockOnChat()!.querySelector('[data-kind="stuck"]')).not.toBeNull();
  });

  it('keeps a question with no choices as a compact bar that goes to the terminal', async () => {
    await renderPane(waitingCockpit({ ...question, choices: [] }));
    await flush();
    await act(async () => tab('Chat').click());
    await flush();
    const dock = dockOnChat()!;
    expect(dock.querySelector('[data-ib-card]')).toBeNull();
    const bar = dock.querySelector<HTMLElement>('.ck-dock-bar')!;
    expect(bar.textContent).toBe('Needs you · Run testsOpen terminal ›');
    await act(async () => bar.click());
    expect(tab('Terminal').getAttribute('aria-selected')).toBe('true');
  });

  it('cycles the docked cards with "N more ›"', async () => {
    await renderPane(waitingCockpit(question), { p1: stuck });
    await flush();
    await act(async () => tab('Chat').click());
    await flush();
    expect(dockOnChat()!.querySelector('[data-kind="approval"]')).not.toBeNull();
    expect(dockOnChat()!.querySelector('[data-kind="stuck"]')).toBeNull();
    await act(async () => buttonIn(dockOnChat()!, '1 more ›').click());
    expect(dockOnChat()!.querySelector('[data-kind="stuck"]')).not.toBeNull();
    expect(dockOnChat()!.querySelector('[data-kind="approval"]')).toBeNull();
    await act(async () => buttonIn(dockOnChat()!, '1 more ›').click());
    expect(dockOnChat()!.querySelector('[data-kind="approval"]')).not.toBeNull();
  });

  it('keeps the old bar on the Transcript tab, not the card', async () => {
    await renderPane(waitingCockpit(question));
    await flush();
    await act(async () => tab('Transcript').click());
    await flush();
    expect(host.querySelector('.ck-dock [data-ib-card]')).toBeNull();
    expect(host.querySelector('.ck-dock-bar')!.textContent).toContain('Needs you · Run tests');
  });
});
