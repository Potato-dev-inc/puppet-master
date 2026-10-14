// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { deriveNeedsYou } from '../../lib/attention';
import { isBudget, type Question } from '../../lib/questions';
import { BudgetCard } from './BudgetCard';

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

const card: Question = {
  id: 'b1',
  agent: 'Claude Code',
  pane_id: 'p1',
  question: 'Claude Code used its 20 turns.',
  context: "20 of 20 turns. It's paused after its last turn. Nothing was lost.",
  choices: [
    { id: 'add_turns', label: 'Add 10 turns' },
    { id: 'stop', label: 'Stop here' },
  ],
  allow_free_text: false,
  urgency: 'normal',
  created_at_ms: 0,
  expires_at_ms: 86_400_000,
  state: 'pending',
  kind: 'budget',
};

const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === text);

function render(q: Question = card) {
  const onAnswer = vi.fn();
  const onDismiss = vi.fn();
  act(() => {
    root.render(
      <ul>
        <BudgetCard q={q} paneName="auth refactor" now={120_000} onAnswer={onAnswer} onDismiss={onDismiss} />
      </ul>,
    );
  });
  return { onAnswer, onDismiss };
}

describe('the budget pause card', () => {
  it('says the agent used its turns, that nothing was lost, and offers Add 10 turns / Stop here', () => {
    render();
    expect(host.querySelector('.ck-row-title')!.textContent).toBe('Claude Code used its 20 turns.');
    expect(host.textContent).toContain("It's paused after its last turn. Nothing was lost.");
    expect(host.textContent).toContain('auth refactor');
    expect(button('Add 10 turns')!.className).toContain('ck-btn--primary');
    expect(button('Stop here')!.className).toContain('ck-btn--plain');
    expect(host.querySelector('input')).toBeNull();
  });

  it('sends the choice id for each button', () => {
    const { onAnswer } = render();
    act(() => button('Add 10 turns')!.click());
    act(() => button('Stop here')!.click());
    expect(onAnswer.mock.calls.map((c) => c[0])).toEqual([{ choice_id: 'add_turns' }, { choice_id: 'stop' }]);
  });

  it('shows no ids or error codes', () => {
    render();
    expect(host.textContent).not.toMatch(/BUDGET_EXHAUSTED|add_turns|b1|p1/);
  });

  it('offers time when the budget has a time limit', () => {
    render({ ...card, choices: [{ id: 'add_turns', label: 'Add 10 turns' }, { id: 'add_minutes', label: 'Add 30 min' }, { id: 'stop', label: 'Stop here' }] });
    expect(button('Add 30 min')!.className).toContain('ck-btn--secondary');
  });
});

describe('a budget card in the Needs-you list', () => {
  it('counts as one item of its own kind, tied to its pane', () => {
    expect(isBudget(card)).toBe(true);
    const items = deriveNeedsYou({ panes: [{ id: 'p1', status: 'idle' }], questions: [card, { id: 'q2', created_at_ms: 3 }] });
    expect(items.map((i) => [i.id, i.kind, i.paneId])).toEqual([
      ['budget:b1', 'budget', 'p1'],
      ['question:q2', 'question', undefined],
    ]);
  });
});
