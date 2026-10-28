// @vitest-environment jsdom
// "Remember" on a side answer: the answer text goes to the project memory once (the same API the chat selection
// uses), a line says so with Undo, and Undo forgets exactly the stored line. Nothing is remembered while no answer shows.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RememberApi, RememberResult } from '../../lib/chat-remember';
import type { AskResult } from '../../lib/askbar';
import type { SideQuestionApi, SideTarget } from '../../lib/sideQuestion';
import { REMEMBER_ALREADY, REMEMBER_REMOVED, REMEMBER_SAVED } from './RememberSelection';
import { SideQuestionBox } from './SideQuestionBox';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

const TARGET: SideTarget = { agent: 'a1', label: 'Agent One' };
const ANSWER = 'The build id lives in **version.json**.';

function result(over: Partial<AskResult> = {}): AskResult {
  return { ask_id: 'x', agent: 'a1', agent_label: 'Agent One', question: 'q', ok: true, answer: ANSWER, error: null, ...over };
}

function fakeApi() {
  let resolveAsk: ((r: AskResult) => void) | null = null;
  const api: SideQuestionApi = {
    target: vi.fn(async () => TARGET),
    ask: vi.fn(() => new Promise<AskResult>((resolve) => (resolveAsk = resolve))),
  };
  return { api, answer: (r: AskResult) => resolveAsk?.(r) };
}

function fakeMemory(stored: Partial<RememberResult> = {}) {
  const memory = {
    remember: vi.fn(async (_pane: string, text: string): Promise<RememberResult> => ({ text, added: true, view: {} as never, ...stored })),
    forget: vi.fn(async () => ({}) as never),
    setProjectEnabled: vi.fn(async () => ({}) as never),
  };
  return { memory, api: memory as RememberApi };
}

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const q = (testid: string) => host.querySelector(`[data-testid="${testid}"]`);
const button = (label: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === label) as HTMLButtonElement | undefined;

function typeQuestion(text: string) {
  const field = host.querySelector('textarea') as HTMLTextAreaElement;
  const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  act(() => {
    set.call(field, text);
    field.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

/** The box with one answer on screen. */
async function showAnswer(memory: RememberApi, answer: (r: AskResult) => void, api: SideQuestionApi) {
  act(() =>
    root.render(<SideQuestionBox paneId="p1" api={api} turns={[]} memory={memory} onClose={() => undefined} />),
  );
  await flush();
  typeQuestion('Where is the build id?');
  act(() => button('Ask')!.click());
  await act(async () => answer(result()));
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

describe('Remember on a side answer', () => {
  it('sends the answer text once and says it is saved', async () => {
    const { api, answer } = fakeApi();
    const { memory, api: memoryApi } = fakeMemory();
    await showAnswer(memoryApi, answer, api);
    await act(async () => button('Remember')!.click());
    expect(memory.remember).toHaveBeenCalledTimes(1);
    expect(memory.remember).toHaveBeenCalledWith('p1', ANSWER);
    expect(q('side-question-remember')?.textContent).toContain(REMEMBER_SAVED);
  });

  it('Undo forgets exactly the stored line', async () => {
    const { api, answer } = fakeApi();
    const { memory, api: memoryApi } = fakeMemory({ text: 'Stored line.' });
    await showAnswer(memoryApi, answer, api);
    await act(async () => button('Remember')!.click());
    await act(async () => button('Undo')!.click());
    expect(memory.forget).toHaveBeenCalledWith('p1', 'Stored line.');
    expect(q('side-question-remember')?.textContent).toContain(REMEMBER_REMOVED);
    expect(button('Undo')).toBeUndefined();
  });

  it('says so when the line was already there, with no Undo', async () => {
    const { api, answer } = fakeApi();
    const { memory, api: memoryApi } = fakeMemory({ added: false });
    await showAnswer(memoryApi, answer, api);
    await act(async () => button('Remember')!.click());
    expect(q('side-question-remember')?.textContent).toContain(REMEMBER_ALREADY);
    expect(button('Undo')).toBeUndefined();
    expect(memory.forget).not.toHaveBeenCalled();
  });

  it('shows the backend sentence when remembering fails', async () => {
    const { api, answer } = fakeApi();
    const { api: memoryApi } = fakeMemory();
    vi.mocked(memoryApi.remember).mockRejectedValueOnce(new Error('SECRET: That text holds a secret, so it is not kept.'));
    await showAnswer(memoryApi, answer, api);
    await act(async () => button('Remember')!.click());
    expect(q('side-question-remember')?.textContent).toBe('That text holds a secret, so it is not kept.');
    expect(button('Undo')).toBeUndefined();
  });

  it('is not offered until an answer is on screen', async () => {
    const { api } = fakeApi();
    const { api: memoryApi } = fakeMemory();
    act(() =>
      root.render(<SideQuestionBox paneId="p1" api={api} turns={[]} memory={memoryApi} onClose={() => undefined} />),
    );
    await flush();
    typeQuestion('still asking');
    act(() => button('Ask')!.click());
    expect(button('Remember')).toBeUndefined();
  });
});
