// @vitest-environment jsdom
// The side question: Ctrl+; and the header button open the box, Esc closes it, a question shows "Asking {agent}…"
// then the answer (Markdown) with Copy and Ask again, or a plain error. Nothing is written into the pane.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AskResult } from '../../lib/askbar';
import { groupTurns, type ChatItem } from '../../lib/sessionChatModel';
import type { SideQuestionApi, SideTarget } from '../../lib/sideQuestion';
import { SessionChatView } from './SessionChatView';
import { NO_SIDE_AGENT } from './SideQuestionBox';
import { isSideShortcut, useSideQuestion } from './useSideQuestion';

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

function result(over: Partial<AskResult> = {}): AskResult {
  return { ask_id: 'x', agent: 'a1', agent_label: 'Agent One', question: 'q', ok: true, answer: 'It made **sideq.txt**.', error: null, ...over };
}

function fakeApi(target: SideTarget | null = TARGET) {
  const asks: { resolve: (r: AskResult) => void; reject: (e: unknown) => void; request: { question: string; context: string } }[] = [];
  const api: SideQuestionApi = {
    target: vi.fn(async () => target),
    ask: vi.fn(
      (_pane, request) =>
        new Promise<AskResult>((resolve, reject) => {
          asks.push({ resolve, reject, request });
        }),
    ),
  };
  return { api, asks };
}

function Chat({ api }: { api: SideQuestionApi }) {
  const side = useSideQuestion('p1', TURNS, api);
  return <SessionChatView state="ready" reason={null} turns={TURNS} model={null} cliVersion={null} paneId="p1" headerExtra={side.button} overlay={side.box} />;
}

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const q = (testid: string) => host.querySelector(`[data-testid="${testid}"]`);
const press = (init: KeyboardEventInit) => act(() => { window.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init })); });
const ctrlSemicolon = () => press({ key: ';', code: 'Semicolon', ctrlKey: true });
const button = (label: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === label) as HTMLButtonElement;

function typeQuestion(text: string) {
  const field = host.querySelector('textarea') as HTMLTextAreaElement;
  const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  act(() => {
    set.call(field, text);
    field.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

async function mount(api: SideQuestionApi) {
  act(() => root.render(<Chat api={api} />));
  await flush();
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('opening and closing', () => {
  it('recognises Ctrl+; and no other combination', () => {
    expect(isSideShortcut({ key: ';', code: 'Semicolon', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false })).toBe(true);
    expect(isSideShortcut({ key: ';', code: 'Semicolon', ctrlKey: false, metaKey: false, altKey: false, shiftKey: false })).toBe(false);
    expect(isSideShortcut({ key: ';', code: 'Semicolon', ctrlKey: true, metaKey: false, altKey: false, shiftKey: true })).toBe(false);
  });

  it('opens with Ctrl+; and closes with Esc', async () => {
    const { api } = fakeApi();
    await mount(api);
    expect(q('side-question')).toBeNull();
    ctrlSemicolon();
    await flush();
    expect(q('side-question')).not.toBeNull();
    expect(host.textContent).toContain('Ask about this session');
    expect(document.activeElement?.tagName).toBe('TEXTAREA');
    press({ key: 'Escape' });
    expect(q('side-question')).toBeNull();
  });

  it('opens and closes with the Ask aside button', async () => {
    const { api } = fakeApi();
    await mount(api);
    act(() => button('Ask aside').click());
    await flush();
    expect(q('side-question')).not.toBeNull();
    act(() => button('Esc').click());
    expect(q('side-question')).toBeNull();
  });

  it('ignores Ctrl+; while its chat is hidden', async () => {
    const { api } = fakeApi();
    await mount(api);
    host.style.display = 'none';
    ctrlSemicolon();
    await flush();
    expect(q('side-question')).toBeNull();
  });
});

describe('asking', () => {
  it('shows the loading line with the agent name, then the Markdown answer', async () => {
    const { api, asks } = fakeApi();
    await mount(api);
    ctrlSemicolon();
    await flush();
    typeQuestion('What file did the agent create?');
    act(() => button('Ask').click());
    expect(q('side-question-loading')?.textContent).toContain('Asking Agent One…');
    expect(asks).toHaveLength(1);
    // The transcript goes along as data; the question as it was typed.
    expect(asks[0]!.request.question).toBe('What file did the agent create?');
    expect(asks[0]!.request.context).toContain('Person: Create sideq.txt with the word hi');
    await act(async () => asks[0]!.resolve(result()));
    expect(q('side-question-loading')).toBeNull();
    const answer = q('side-question-answer')!;
    expect(answer.querySelector('strong')?.textContent).toBe('sideq.txt');
  });

  it('sends with Enter, and Ask again asks the same question once more', async () => {
    const { api, asks } = fakeApi();
    await mount(api);
    ctrlSemicolon();
    await flush();
    typeQuestion('Why?');
    act(() => {
      (host.querySelector('textarea') as HTMLTextAreaElement).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    });
    await act(async () => asks[0]!.resolve(result()));
    act(() => button('Ask again').click());
    expect(asks).toHaveLength(2);
    expect(asks[1]!.request.question).toBe('Why?');
    expect(q('side-question-loading')).not.toBeNull();
  });

  it('copies the answer', async () => {
    const { api, asks } = fakeApi();
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await mount(api);
    ctrlSemicolon();
    await flush();
    typeQuestion('x');
    act(() => button('Ask').click());
    await act(async () => asks[0]!.resolve(result()));
    await act(async () => button('Copy').click());
    expect(writeText).toHaveBeenCalledWith('It made **sideq.txt**.');
    expect(button('Copied')).toBeDefined();
  });

  it('shows the plain error when the helper could not answer', async () => {
    const { api, asks } = fakeApi();
    await mount(api);
    ctrlSemicolon();
    await flush();
    typeQuestion('x');
    act(() => button('Ask').click());
    await act(async () =>
      asks[0]!.resolve(result({ ok: false, answer: null, error: { code: 'BACKEND_UNAVAILABLE', message: 'Agent One is not installed.' } })),
    );
    expect(q('side-question-error')?.textContent).toBe('Agent One is not installed.');
    expect(q('side-question-answer')).toBeNull();
  });

  it('shows an error when the call itself fails', async () => {
    const { api, asks } = fakeApi();
    await mount(api);
    ctrlSemicolon();
    await flush();
    typeQuestion('x');
    act(() => button('Ask').click());
    await act(async () => asks[0]!.reject(new Error('The helper stopped.')));
    expect(q('side-question-error')?.textContent).toBe('The helper stopped.');
  });

  it('drops an answer that arrives after the box was closed', async () => {
    const { api, asks } = fakeApi();
    await mount(api);
    ctrlSemicolon();
    await flush();
    typeQuestion('x');
    act(() => button('Ask').click());
    press({ key: 'Escape' });
    await act(async () => asks[0]!.resolve(result()));
    expect(q('side-question')).toBeNull();
    // Opened again: empty, nothing was kept.
    ctrlSemicolon();
    await flush();
    expect(q('side-question-answer')).toBeNull();
    expect((host.querySelector('textarea') as HTMLTextAreaElement).value).toBe('');
  });

  it('says so clearly when no agent can answer, and cannot send', async () => {
    const { api, asks } = fakeApi(null);
    await mount(api);
    ctrlSemicolon();
    await flush();
    expect(host.textContent).toContain(NO_SIDE_AGENT);
    expect((host.querySelector('textarea') as HTMLTextAreaElement).disabled).toBe(true);
    expect(button('Ask').disabled).toBe(true);
    expect(asks).toHaveLength(0);
  });
});
