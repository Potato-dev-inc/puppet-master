// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ITEM_ERROR, ITEM_HIGH, ITEM_LOW, ITEM_QUESTION, tombstone } from '../../dev/attention-fixtures';
import type { AttentionItem, AttentionSnapshot } from '../../lib/attention';
import type { BridgeClient } from '../../lib/bridge-types';
import type { Question } from '../../lib/questions';
import { resetTouchForTests } from '../../hooks/useHeldItems';
import { NeedsYouList } from './NeedsYouList';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  resetTouchForTests();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

const snap = (items: AttentionItem[], device?: AttentionSnapshot['device']): AttentionSnapshot => ({
  v: 1,
  rev: 1,
  counts: { blocking: items.filter((i) => i.state === 'open').length, ready: 0 },
  items,
  device,
});

const question: Question = {
  id: 'q-1',
  question: 'Keep the old /login route as a redirect, or remove it?',
  choices: [
    { id: 'keep', label: 'Keep redirect' },
    { id: 'remove', label: 'Remove it' },
  ],
  allow_free_text: false,
  urgency: 'normal',
  created_at_ms: Date.now(),
  expires_at_ms: Date.now() + 3_600_000,
  state: 'pending',
};

const bridge = { answerAttention: vi.fn(), answerQuestion: vi.fn(async () => ({ accepted: true })), dismissQuestion: vi.fn() } as unknown as BridgeClient;

async function render(s: AttentionSnapshot, props: { canAnswer?: boolean; showEmpty?: boolean } = {}) {
  await act(async () => {
    root.render(
      <NeedsYouList
        snapshot={s}
        bridge={bridge}
        canAnswer={props.canAnswer ?? true}
        questions={[question]}
        refreshQuestions={() => undefined}
        labels={new Map()}
        onOpenPane={() => undefined}
        showEmpty={props.showEmpty}
      />,
    );
  });
}

const cards = () => Array.from(host.querySelectorAll('[data-card-state], [data-question-id]'));

describe('the Needs you queue', () => {
  it('draws one card per blocking item in the PC’s order: high risk, the calm one, the question, the error', async () => {
    await render(snap([ITEM_HIGH, ITEM_LOW, ITEM_QUESTION, ITEM_ERROR]));
    const kinds = cards().map((c) => c.getAttribute('data-item-kind') ?? (c.hasAttribute('data-question-id') ? 'question' : '?'));
    expect(kinds).toEqual(['approval', 'approval', 'question', 'error']);
    expect(host.textContent).toContain('Delete a folder outside this project');
    expect(host.textContent).toContain('Keep the old /login route');
    expect(host.textContent).toContain('PowerShell hit an error');
  });

  it('a high-risk card leads with Deny, and a watch-only phone sees every card without buttons', async () => {
    await render(snap([ITEM_HIGH, ITEM_LOW]));
    const first = host.querySelector('[data-item-kind="approval"] .ph-appr-actions')!;
    expect(first.querySelector('button')?.textContent).toBe('Deny');
    act(() => root.unmount());
    root = createRoot(host);
    await render(snap([ITEM_HIGH, ITEM_LOW, ITEM_QUESTION], { can_answer: false, can_type: false, can_control: false }), { canAnswer: false });
    expect(host.querySelectorAll('button[data-choice]')).toHaveLength(0);
    expect(host.textContent).toContain('This phone can watch only');
    // The question card says so too instead of offering answers the PC would refuse.
    expect(host.querySelector('[data-question-id] .ph-answers')).toBeNull();
  });

  it('draws nothing for an empty list, or the empty state when asked', async () => {
    await render(snap([]));
    expect(host.textContent).toBe('');
    act(() => root.unmount());
    root = createRoot(host);
    await render(snap([]), { showEmpty: true });
    expect(host.textContent).toContain('Nothing needs you');
    expect(host.textContent).toContain('Agents that stop to ask will show up here.');
  });

  it('a card that was answered on the PC stays as a line that says so, and is not a card to answer', async () => {
    await render(snap([ITEM_HIGH, tombstone(ITEM_LOW, { state: 'answered', by: 'desktop', choice_label: 'Allowed once' })]));
    expect(host.textContent).toContain('Already answered on your PC: Allowed once.');
    expect(host.querySelectorAll('[data-card-state="already"] button[data-choice]')).toHaveLength(0);
  });
});

describe('a new card waits while a finger is on the list', () => {
  it('holds a new item until 600 ms after the touch ends, and updates the others in place', async () => {
    vi.useFakeTimers();
    await render(snap([ITEM_LOW]));
    expect(cards()).toHaveLength(1);
    // The finger goes down on the list.
    await act(async () => {
      window.dispatchEvent(new Event('pointerdown'));
    });
    await render(snap([ITEM_HIGH, ITEM_LOW]));
    expect(cards()).toHaveLength(1);
    expect(host.textContent).not.toContain('Delete a folder');
    // Lifted, but not for 600 ms yet.
    await act(async () => {
      window.dispatchEvent(new Event('pointerup'));
      vi.advanceTimersByTime(300);
    });
    expect(cards()).toHaveLength(1);
    await act(async () => void vi.advanceTimersByTime(500));
    expect(cards()).toHaveLength(2);
    expect(host.querySelector('[data-item-kind="approval"] .ph-appr-title')?.textContent).toBe('Delete a folder outside this project');
  });

  it('shows a new item at once when nobody is touching', async () => {
    await render(snap([ITEM_LOW]));
    await render(snap([ITEM_HIGH, ITEM_LOW]));
    expect(cards()).toHaveLength(2);
  });
});
