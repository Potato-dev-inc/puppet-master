// @vitest-environment jsdom
// The Chat tab of a pane that reads idle: a menu with a cursor on its screen (a first-run step no profile knew) is
// shown as "Agent is asking", the idle input box of an agent is not, and a proper card keeps the place alone.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { ChatItem } from '../../lib/sessionChat';
import { PaneView } from './PaneView';

const invoke = vi.fn(async (_cmd: string, _args?: unknown) => null as unknown);
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...(a as [string])) }));
vi.mock('../TerminalViewport', () => ({ TerminalViewport: () => <div data-testid="term" /> }));
vi.mock('../../lib/sessionChat', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/sessionChat')>()),
  openSessionChat: vi.fn(async () => ({ available: true, reason: null, items: SAMPLE })),
  closeSessionChat: vi.fn(async () => undefined),
  onSessionChatItems: vi.fn(async () => () => undefined),
  onSessionChatStatus: vi.fn(async () => () => undefined),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as { ResizeObserver?: unknown }).ResizeObserver = NoopResizeObserver;
Element.prototype.scrollIntoView = () => {};

const SAMPLE: ChatItem[] = [{ kind: 'user', id: '10:0', at: null, text: 'List the files' }];
const fixture = (path: string) => readFileSync(resolve(__dirname, '../../../src-tauri/src', path), 'utf8');
const THEME = fixture('tui/claude/fixtures/first_run_theme_2_1_312.txt');
const CLAUDE_IDLE = fixture('tui/claude/fixtures/first_run_idle_box_2_1_312.txt');
const CODEX_IDLE = fixture('tui/codex/fixtures/standby_ready_live.txt');

function cockpit(agentType: string, status: string, prompt: unknown = null): CockpitApi {
  const info = { id: 'p1', agent_type: agentType, status, pid: 4120, created_at: 1, last_output_at: null, cwd: 'C:\\work\\shop', cols: 158, rows: 56 };
  const p = { status, info };
  return {
    registry: { paneList: [p], panes: new Map([['p1', p]]), initialReady: true },
    names: new Map([['p1', 'Agent']]),
    activities: {},
    previews: {},
    asked: { questions: [] },
    prompts: prompt ? { p1: prompt } : {},
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

let host: HTMLDivElement;
let root: Root;
let screen = '';
beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'read_pane_snapshot') return screen;
    if (cmd === 'checkpoints_list') return { state: 'ok', message: '', items: [] };
    return null;
  });
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

const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
const tab = (name: string) => [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find((t) => t.textContent === name)!;
const screenCard = () => host.querySelector('.ck-pane-body [data-screen-question]');
const dockOnChat = () => host.querySelector('.ck-pane-body .ck-chat-dock');

async function openChat(c: CockpitApi) {
  await act(async () => root.render(<PaneView c={c} paneId="p1" />));
  await flush();
  await act(async () => tab('Chat').click());
  await flush();
}

describe('the Chat tab of a pane that reads idle', () => {
  it('shows a menu with a cursor as "Agent is asking" and answers it', async () => {
    screen = THEME;
    await openChat(cockpit('claude', 'idle'));
    const card = screenCard()!;
    expect(card).not.toBeNull();
    expect(card.textContent).toContain('Choose the text style that looks best with your terminal');
    const labels = Array.from(card.querySelectorAll('button')).map((b) => b.textContent);
    expect(labels).toEqual(expect.arrayContaining(['Auto (match terminal)', 'Dark mode', 'Light mode']));
    // It sits under the conversation, in a dock of its own.
    expect(host.querySelector('.ck-chat-body')!.contains(card)).toBe(false);
    await act(async () => Array.from(card.querySelectorAll('button')).find((b) => b.textContent === 'Light mode')!.click());
    // The pointer is on Dark mode: one press down, then Enter on its own.
    const typed = invoke.mock.calls.filter(([cmd]) => cmd === 'write_pane_input').map((call) => (call[1] as { args: { text: string; append_newline: boolean } }).args);
    expect(typed.map((a) => [a.text, a.append_newline])).toEqual([
      ['[B', false],
      ['', true],
    ]);
  });

  it.each([
    ['claude', CLAUDE_IDLE],
    ['codex', CODEX_IDLE],
  ])('shows nothing under the conversation for the idle input box of %s', async (agent, text) => {
    screen = text;
    await openChat(cockpit(agent, 'idle'));
    expect(screenCard()).toBeNull();
    expect(dockOnChat()).toBeNull();
  });

  it('shows nothing for a pane that is working', async () => {
    screen = THEME;
    await openChat(cockpit('claude', 'running'));
    expect(screenCard()).toBeNull();
  });

  it('keeps a proper card alone: no second card for the same pane', async () => {
    screen = THEME;
    await openChat(cockpit('claude', 'waiting_input', question));
    expect(host.querySelector('.ck-pane-body .ck-chat-dock [data-kind="approval"]')).not.toBeNull();
    expect(screenCard()).toBeNull();
  });

  it('shows the same card for a waiting pane that has no proper card', async () => {
    screen = THEME;
    await openChat(cockpit('claude', 'waiting_input'));
    expect(screenCard()).not.toBeNull();
    expect(host.querySelectorAll('.ck-pane-body [data-screen-question]')).toHaveLength(1);
  });
});
