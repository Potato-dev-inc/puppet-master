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

function makeBridge(questions: Question[]) {
  return {
    listQuestions: vi.fn(async () => questions),
    answerQuestion: vi.fn(async () => ({ accepted: true })),
    dismissQuestion: vi.fn(async () => ({ accepted: true })),
  } as unknown as BridgeClient & { listQuestions: ReturnType<typeof vi.fn> };
}

async function render(bridge: BridgeClient, revision?: number) {
  await act(async () => {
    root.render(<QuestionsList bridge={bridge} bridgeReady labels={new Map()} revision={revision} />);
  });
}

async function mount(questions: Question[]) {
  const bridge = makeBridge(questions);
  await render(bridge);
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

  it('gives every choice equal weight and shows the countdown', async () => {
    await mount([{ ...q, choices: [{ id: 'a', label: 'Yes' }, { id: 'b', label: 'No' }], expires_at_ms: Date.now() + 8 * 60_000 - 1000 }]);
    expect(host.querySelector('.ph-btn--primary')).toBeNull();
    expect(host.querySelector('[data-testid="question-wait"]')?.textContent).toBe('The agent stops waiting in 8 min');
  });

  describe('refetching', () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });
    afterEach(() => {
      vi.useRealTimers();
    });

    it('fetches once on mount', async () => {
      const bridge = await mount([q]);
      expect(bridge.listQuestions).toHaveBeenCalledTimes(1);
    });

    it('refetches when the revision prop changes', async () => {
      const bridge = makeBridge([q]);
      await render(bridge, 0);
      expect(bridge.listQuestions).toHaveBeenCalledTimes(1);
      await render(bridge, 1);
      expect(bridge.listQuestions).toHaveBeenCalledTimes(2);
    });

    it('does not poll every 3 seconds', async () => {
      const bridge = await mount([q]);
      await act(async () => {
        vi.advanceTimersByTime(3000);
      });
      expect(bridge.listQuestions).toHaveBeenCalledTimes(1);
    });

    it('falls back to a 30 second poll', async () => {
      const bridge = await mount([q]);
      await act(async () => {
        vi.advanceTimersByTime(30000);
      });
      expect(bridge.listQuestions).toHaveBeenCalledTimes(2);
    });
  });
});
