// @vitest-environment jsdom
// The dock of a chat session over ACP: the agent's permission questions as cards with its own options, and the strip with
// Stop that reads "Stopping…" until the turn ends.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { AcpChatApi, AcpChatState } from '../../lib/acpChat';
import type { Question } from '../../lib/questions';
import { AcpChatDock } from './AcpChatDock';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const question = (over: Partial<Question> = {}): Question => ({
  id: 'q1',
  question: 'Codex asks permission: Write notes.txt',
  choices: [
    { id: 'allow_once', label: 'Allow Once' },
    { id: 'allow_always', label: 'Allow for Session' },
    { id: 'reject_once', label: 'Reject' },
  ],
  allow_free_text: false,
  urgency: 'high',
  created_at_ms: 1,
  expires_at_ms: 99,
  state: 'pending',
  kind: 'acp_permission',
  pane_id: 'p1',
  ...over,
});

const state = (over: Partial<AcpChatState> = {}): AcpChatState => ({
  lane: 'acp',
  agent: 'codex',
  label: 'Codex',
  phase: 'working',
  status: 'running',
  turnInFlight: true,
  stopping: false,
  canSteer: false,
  turn: 1,
  title: null,
  usage: null,
  permissions: [],
  closed: false,
  ...over,
});

function cockpit(questions: Question[]) {
  const answerQuestion = vi.fn(async () => undefined);
  const dismissQuestion = vi.fn(async () => undefined);
  const c = { now: 5, asked: { questions, answerQuestion, dismissQuestion } } as unknown as CockpitApi;
  return { c, answerQuestion, dismissQuestion };
}

function fakeApi(initial: AcpChatState | null) {
  let push: ((paneId: string, next: AcpChatState) => void) | null = null;
  const cancel = vi.fn(async () => true);
  const api: AcpChatApi = {
    state: async () => initial,
    cancel,
    watch: async (cb) => {
      push = cb;
      return () => {
        push = null;
      };
    },
  };
  return { api, cancel, emit: (next: AcpChatState) => act(async () => push?.('p1', next)) };
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  host.className = 'cockpit';
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const buttons = () => [...host.querySelectorAll<HTMLButtonElement>('button')];
const button = (text: string) => buttons().find((b) => b.textContent === text);

describe('AcpChatDock', () => {
  it('shows nothing while the session is idle and nothing is asked', async () => {
    const { c } = cockpit([]);
    const { api } = fakeApi(state({ turnInFlight: false, phase: 'idle', status: 'idle' }));
    await act(async () => root.render(<AcpChatDock c={c} paneId="p1" paneName="Codex" api={api} />));
    await flush();
    expect(host.querySelector('[data-testid="acp-dock"]')).toBeNull();
  });

  it("shows the agent's question with the agent's own options and sends the chosen one", async () => {
    const { c, answerQuestion } = cockpit([question(), question({ id: 'other', pane_id: 'p2' })]);
    const { api } = fakeApi(state({ phase: 'needs_input', status: 'waiting_input' }));
    await act(async () => root.render(<AcpChatDock c={c} paneId="p1" paneName="Codex" api={api} />));
    await flush();
    expect(host.textContent).toContain('Codex asks permission: Write notes.txt');
    expect(host.querySelectorAll('[aria-label="Agent permission question"]').length).toBe(1);
    expect(['Allow Once', 'Allow for Session', 'Reject'].map((t) => !!button(t))).toEqual([true, true, true]);
    // The strip tells that the agent waits for the answer.
    expect(host.querySelector('[data-testid="acp-strip"]')?.textContent).toContain('Waiting for your answer');
    await act(async () => button('Allow for Session')!.click());
    expect(answerQuestion).toHaveBeenCalledWith('q1', { choice_id: 'allow_always' });
  });

  it('dismissing the card is a no, and goes through the same route as every card', async () => {
    const { c, dismissQuestion } = cockpit([question()]);
    const { api } = fakeApi(state({ phase: 'needs_input' }));
    await act(async () => root.render(<AcpChatDock c={c} paneId="p1" paneName="Codex" api={api} />));
    await flush();
    const dismiss = buttons().find((b) => b.textContent === 'Dismiss');
    expect(dismiss).toBeTruthy();
    await act(async () => dismiss!.click());
    expect(dismissQuestion).toHaveBeenCalledWith('q1');
  });

  it('Stop asks the session to cancel and reads Stopping… at once, until the turn has ended', async () => {
    const { c } = cockpit([]);
    const { api, cancel, emit } = fakeApi(state());
    await act(async () => root.render(<AcpChatDock c={c} paneId="p1" paneName="Codex" api={api} />));
    await flush();
    expect(host.querySelector('[data-testid="acp-strip"]')?.textContent).toContain('Working…');
    await act(async () => button('Stop')!.click());
    expect(cancel).toHaveBeenCalledWith('p1');
    // Before the backend has said anything: already Stopping…, and the button cannot be pressed twice.
    const strip = host.querySelector('[data-testid="acp-strip"]')!;
    expect(strip.textContent).toContain('Stopping…');
    expect(button('Stopping…')?.disabled).toBe(true);
    // The backend says it is stopping, then that the turn has ended.
    await emit(state({ phase: 'cancelling', stopping: true }));
    expect(host.querySelector('[data-testid="acp-strip"]')?.textContent).toContain('Stopping…');
    await emit(state({ phase: 'idle', status: 'idle', turnInFlight: false }));
    expect(host.querySelector('[data-testid="acp-dock"]')).toBeNull();
    // A new turn starts: Stop is available again.
    await emit(state({ turn: 2 }));
    expect(button('Stop')?.disabled).toBe(false);
  });

  it('a Stop that could not be sent gives the button back', async () => {
    const { c } = cockpit([]);
    const { api } = fakeApi(state());
    api.cancel = vi.fn(async () => {
      throw new Error('no');
    });
    await act(async () => root.render(<AcpChatDock c={c} paneId="p1" paneName="Codex" api={api} />));
    await flush();
    await act(async () => button('Stop')!.click());
    await flush();
    expect(button('Stop')?.disabled).toBe(false);
  });

  it('a chat that needs a sign-in shows the card; Open sign-in opens the agent terminal in the pane folder, then try again', async () => {
    const newAgent = vi.fn(async () => 'term-1');
    const c = { now: 5, asked: { questions: [], answerQuestion: vi.fn(), dismissQuestion: vi.fn() }, newAgent, registry: { panes: new Map([['p1', { info: { cwd: '/work/app' } }]]) } } as unknown as CockpitApi;
    const retrySignIn = vi.fn(async () => undefined);
    const { api, emit } = fakeApi(state({ turnInFlight: false, phase: 'idle', status: 'idle', agent: 'claude', label: 'Claude' }));
    api.retrySignIn = retrySignIn;
    await act(async () => root.render(<AcpChatDock c={c} paneId="p1" paneName="Claude" api={api} />));
    await flush();
    expect(host.querySelector('[data-testid="signin-card"]')).toBeNull();

    await emit(state({ turnInFlight: false, phase: 'idle', status: 'idle', agent: 'claude', label: 'Claude', signIn: { needed: true, hint: 'Type /login in the terminal.' } }));
    const card = host.querySelector('[data-testid="signin-card"]')!;
    expect(card.textContent).toContain('Sign in to Claude to use Chat');
    expect(card.textContent).toContain('Type /login in the terminal.');

    await act(async () => button('Open sign-in')!.click());
    expect(newAgent).toHaveBeenCalledWith('claude', undefined, false, '/work/app');
    await act(async () => buttons().find((b) => /signed in — try again/.test(b.textContent ?? ''))!.click());
    expect(retrySignIn).toHaveBeenCalledWith('p1');

    // The backend clears the flag once the message went: the card goes.
    await emit(state({ turnInFlight: true, agent: 'claude', label: 'Claude', signIn: { needed: false, hint: '' } }));
    expect(host.querySelector('[data-testid="signin-card"]')).toBeNull();
  });
});
