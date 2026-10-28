// @vitest-environment jsdom
// The side question keeps the last three answers for each pane, in memory only. Reopening the box lists them under
// "Earlier", newest first; a click shows one. Another pane sees none, nothing is written to localStorage, and an
// earlier answer is never sent to the agent.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AskResult } from '../../lib/askbar';
import { groupTurns, type ChatItem } from '../../lib/sessionChatModel';
import type { SideQuestionApi, SideTarget } from '../../lib/sideQuestion';
import { SessionChatView } from './SessionChatView';
import { clearRecentSideAnswers, useSideQuestion } from './useSideQuestion';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

const ITEMS: ChatItem[] = [
  { kind: 'user', id: '1:0', at: null, text: 'Create sideq.txt with the word hi' },
  { kind: 'assistant', id: '2:0', at: null, text: 'Done.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: '3:0', at: null, outcome: 'completed', durationMs: 1000 },
];
const TURNS = groupTurns(ITEMS).turns;
const TARGET: SideTarget = { agent: 'a1', label: 'Agent One' };

interface Pending {
  resolve: (r: AskResult) => void;
  reject: (e: unknown) => void;
  request: { question: string; context: string };
}

function result(over: Partial<AskResult> = {}): AskResult {
  return { ask_id: 'x', agent: 'a1', agent_label: 'Agent One', question: 'q', ok: true, answer: 'An answer.', error: null, ...over };
}

function fakeApi() {
  const asks: Pending[] = [];
  const api: SideQuestionApi = {
    target: vi.fn(async () => TARGET),
    ask: vi.fn(
      (_pane, request) =>
        new Promise<AskResult>((resolve, reject) => {
          asks.push({ resolve, reject, request });
        }),
    ),
  };
  return { api, asks };
}

function Chat({ api, paneId }: { api: SideQuestionApi; paneId: string }) {
  const side = useSideQuestion(paneId, TURNS, api);
  return <SessionChatView state="ready" reason={null} turns={TURNS} model={null} cliVersion={null} paneId={paneId} headerExtra={side.button} overlay={side.box} />;
}

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const q = (testid: string) => host.querySelector(`[data-testid="${testid}"]`);
const press = (init: KeyboardEventInit) => act(() => { window.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init })); });
const ctrlSemicolon = () => press({ key: ';', code: 'Semicolon', ctrlKey: true });
const button = (label: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === label) as HTMLButtonElement;
const earlierQuestions = () => [...host.querySelectorAll('.ck-sq-earlier-q')].map((b) => b.textContent);

function typeQuestion(text: string) {
  const field = host.querySelector('textarea') as HTMLTextAreaElement;
  const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  act(() => {
    set.call(field, text);
    field.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

/** Asks one question in the open box and lets its answer come back. */
async function askAndAnswer(asks: Pending[], question: string, answer = `Answer to ${question}`) {
  typeQuestion(question);
  act(() => button('Ask').click());
  await act(async () => asks[asks.length - 1]!.resolve(result({ answer })));
}

async function mount(api: SideQuestionApi, paneId = 'p1') {
  act(() => root.render(<Chat api={api} paneId={paneId} />));
  await flush();
}

/** Closes the box with Esc and opens it again with Ctrl+;. */
async function reopen() {
  press({ key: 'Escape' });
  ctrlSemicolon();
  await flush();
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  clearRecentSideAnswers();
  localStorage.clear();
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

describe('the last answers of a pane', () => {
  it('keeps three, newest first, and lists them when the box opens again', async () => {
    const { api, asks } = fakeApi();
    await mount(api);
    ctrlSemicolon();
    await flush();
    await askAndAnswer(asks, 'first');
    await askAndAnswer(asks, 'second');
    await askAndAnswer(asks, 'third');
    await askAndAnswer(asks, 'fourth');
    // The newest answer is on screen, so "Earlier" shows the two before it.
    expect(q('side-question-answer')?.textContent).toContain('Answer to fourth');
    expect(earlierQuestions()).toEqual(['third', 'second']);
    await reopen();
    expect(q('side-question-answer')).toBeNull();
    expect(earlierQuestions()).toEqual(['fourth', 'third', 'second']);
    expect(host.textContent).not.toContain('first');
  });

  it('shows an earlier answer when its question is clicked', async () => {
    const { api, asks } = fakeApi();
    await mount(api);
    ctrlSemicolon();
    await flush();
    await askAndAnswer(asks, 'alpha', 'The **alpha** result.');
    await askAndAnswer(asks, 'beta');
    expect(q('side-question-earlier-answer')).toBeNull();
    const alpha = [...host.querySelectorAll('.ck-sq-earlier-q')].find((b) => b.textContent === 'alpha') as HTMLButtonElement;
    expect(alpha.getAttribute('aria-expanded')).toBe('false');
    act(() => alpha.click());
    expect(alpha.getAttribute('aria-expanded')).toBe('true');
    expect(q('side-question-earlier-answer')?.querySelector('strong')?.textContent).toBe('alpha');
  });

  it('is not shown for another pane', async () => {
    const { api, asks } = fakeApi();
    await mount(api, 'p1');
    ctrlSemicolon();
    await flush();
    await askAndAnswer(asks, 'only in p1');
    press({ key: 'Escape' });
    await mount(api, 'p2');
    ctrlSemicolon();
    await flush();
    expect(q('side-question-earlier')).toBeNull();
    expect(earlierQuestions()).toEqual([]);
  });

  it('keeps nothing on disk: localStorage is not written', async () => {
    const setItem = vi.spyOn(Storage.prototype, 'setItem');
    const { api, asks } = fakeApi();
    await mount(api);
    ctrlSemicolon();
    await flush();
    await askAndAnswer(asks, 'private');
    await reopen();
    expect(earlierQuestions()).toEqual(['private']);
    expect(setItem).not.toHaveBeenCalled();
    expect(localStorage.length).toBe(0);
  });

  it('is never sent to the agent: a new question carries only the chat', async () => {
    const { api, asks } = fakeApi();
    await mount(api);
    ctrlSemicolon();
    await flush();
    await askAndAnswer(asks, 'one', 'Earlier secret answer.');
    await askAndAnswer(asks, 'two');
    expect(asks[1]!.request.question).toBe('two');
    expect(asks[1]!.request.context).not.toContain('Earlier secret answer');
  });

  it('does not keep a question that failed', async () => {
    const { api, asks } = fakeApi();
    await mount(api);
    ctrlSemicolon();
    await flush();
    typeQuestion('broken');
    act(() => button('Ask').click());
    await act(async () => asks[0]!.resolve(result({ ok: false, answer: null, error: { code: 'BACKEND_UNAVAILABLE', message: 'Agent One is not installed.' } })));
    await reopen();
    expect(q('side-question-earlier')).toBeNull();
  });

  it('does not keep an answer that arrives after the box was closed', async () => {
    const { api, asks } = fakeApi();
    await mount(api);
    ctrlSemicolon();
    await flush();
    typeQuestion('late');
    act(() => button('Ask').click());
    press({ key: 'Escape' });
    await act(async () => asks[0]!.resolve(result()));
    ctrlSemicolon();
    await flush();
    expect(q('side-question-earlier')).toBeNull();
  });
});
