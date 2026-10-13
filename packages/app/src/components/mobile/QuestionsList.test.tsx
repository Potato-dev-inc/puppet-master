// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BridgeClient } from '../../lib/bridge';
import type { Question } from '../../lib/questions';
import { QuestionsList } from './QuestionsList';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const q: Question = {
  id: 'q1',
  question: 'Deploy to staging?',
  choices: [{ id: 'yes', label: 'Yes' }],
  allow_free_text: true,
  urgency: 'normal',
  created_at_ms: Date.now(),
  expires_at_ms: Date.now() + 600_000,
  state: 'pending',
};

async function mount(questions: Question[]) {
  const bridge = {
    listQuestions: vi.fn(async () => questions),
    answerQuestion: vi.fn(async () => ({ accepted: true })),
    dismissQuestion: vi.fn(async () => ({ accepted: true })),
  } as unknown as BridgeClient;
  await act(async () => {
    root.render(<QuestionsList bridge={bridge} bridgeReady labels={new Map()} />);
  });
  return bridge;
}

describe('phone QuestionsList', () => {
  it('renders nothing without questions', async () => {
    await mount([]);
    expect(host.textContent).toBe('');
  });

  it('answers with a choice button and dismisses', async () => {
    const bridge = await mount([q]);
    expect(host.textContent).toContain('Question from an agent');
    const yes = Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Yes')!;
    await act(async () => yes.click());
    expect(bridge.answerQuestion).toHaveBeenCalledWith('q1', { choice_id: 'yes' });
    const dismiss = Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Dismiss')!;
    await act(async () => dismiss.click());
    expect(bridge.dismissQuestion).toHaveBeenCalledWith('q1');
  });
});
